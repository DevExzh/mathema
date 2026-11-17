//! Expandable LaTeX branch selectors. Read both arguments as tokens; only the
//! selected replacement goes back on the input stream for execution.
use super::{QueueTxn, Queued, Token};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Selector {
    First,
    Second,
    IfStar,
    IfNextChar,
    TextOrMath,
}

impl Selector {
    pub(super) const BUILTINS: &'static [(&'static str, Self)] = &[
        ("@firstoftwo", Self::First),
        ("@secondoftwo", Self::Second),
        ("@ifstar", Self::IfStar),
        ("@ifnextchar", Self::IfNextChar),
        ("TextOrMath", Self::TextOrMath),
    ];

    pub(super) fn select(
        self,
        queue: &mut VecDeque<Queued>,
        budget: Rc<RefCell<super::WorkBudget>>,
        text_mode: bool,
    ) -> Option<Vec<Token>> {
        let mut input = QueueTxn::new(queue, budget);
        let pattern = if self == Self::IfNextChar {
            Some(argument(&mut input)?)
        } else {
            None
        };
        let yes = argument(&mut input)?;
        let no = argument(&mut input)?;
        let choose_first = match self {
            Self::TextOrMath => text_mode,
            Self::First => true,
            Self::Second => false,
            Self::IfStar => {
                input.skip_whitespace();
                if input.peek_token_is('*') {
                    input.pop();
                    true
                } else {
                    false
                }
            },
            Self::IfNextChar => {
                input.skip_whitespace();
                matches!(pattern.as_deref(), Some([token]) if input.peek_token() == Some(token))
            },
        };
        input.commit();
        Some(if choose_first { yes } else { no })
    }
}

fn argument(input: &mut QueueTxn<'_>) -> Option<Vec<Token>> {
    input.skip_whitespace();
    match input.peek_token()? {
        Token::BeginGroup => input.pop_group(),
        Token::EndGroup => None,
        _ => Some(vec![input.pop_token()?]),
    }
}
