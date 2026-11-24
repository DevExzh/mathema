//! Which `|` and `‖` of a UnicodeMath string pair up (UTN #28 section 3.1).
//!
//! One left-to-right scan with one stack of open brackets and bars decides
//! every bar, so the parser reads each bar alike however often and in
//! whatever construct (script, denominator, n-aryand) it meets it. A bar is
//! one of:
//!
//! - an opening bar: "the first appearance is considered to be an open |", as
//!   is one "following an operator", at its bracket nesting level;
//! - a closing bar: the next one, or "if it is followed by a space", when a
//!   bar is open at the level;
//! - a script base: "if a | is followed by a subscript and/or a superscript
//!   and has no corresponding open |";
//! - with `⟩` or `⟨`, a ket (`|ψ⟩`) or a bra (`⟨ψ|`), as "a delimited pair
//!   need not consist of the same kinds of delimiters";
//! - a divider, when nothing pairs with it (`P(A|B)`). Two bars that would
//!   pair around a relation or separator, each between two operands, are
//!   dividers too (`P(A|B,C|D)`, `a|b∧b|c`).

use super::{breaks_operand_at, classify_unicode_symbol, is_alphanumeric, is_blank, is_nary_char};
use crate::ast::SymbolKind;

/// Whether `c` is a bar or may pair with one.
pub(crate) fn pairs_bars(c: char) -> bool {
    matches!(c, '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}')
}

/// What comes before a bar.
#[derive(Clone, Copy, PartialEq)]
enum Previous {
    Operator,
    Operand,
}

#[derive(Clone, Copy)]
enum Entry {
    Bracket {
        open: char,
        at: usize,
    },
    Bar {
        fence: char,
        at: usize,
        /// Directly after an operand.
        after_operand: bool,
        /// A bar before a space that closed nothing.
        divider: bool,
        /// A relation or separator followed it at its level.
        separated: bool,
    },
}

/// Opening brackets that set a nesting level, with their closing brackets.
const BRACKETS: [(char, char); 7] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('〖', '〗'),
    ('\u{27E8}', '\u{27E9}'),
    ('\u{2308}', '\u{2309}'),
    ('\u{230A}', '\u{230B}'),
];

/// The index in [`BRACKETS`] of the angle brackets.
const ANGLE: usize = 4;

#[derive(Default)]
struct Scan {
    stack: Vec<Entry>,
    /// Indices in `stack` of its brackets.
    brackets: Vec<usize>,
    /// Open brackets of each kind.
    counts: [usize; BRACKETS.len()],
    /// The kinds of the open brackets, by index in [`BRACKETS`].
    kinds: Vec<usize>,
    pairs: Vec<(usize, usize)>,
    /// The matched `⟨⟩`, `⌈⌉` and `⌊⌋`.
    groups: Vec<(usize, usize)>,
}

impl Scan {
    fn pair(&mut self, open: usize, close: usize) {
        self.pairs.push((open, close));
        self.pairs.push((close, open));
    }

    fn push_bracket(&mut self, kind: usize, at: usize) {
        self.brackets.push(self.stack.len());
        self.kinds.push(kind);
        self.counts[kind] += 1;
        self.stack.push(Entry::Bracket {
            open: BRACKETS[kind].0,
            at,
        });
    }

    /// Drop the stack entries from `len` on. An unmatched `⟨` with a bar
    /// right above it is a bra.
    fn unwind(&mut self, len: usize) {
        for index in len..self.stack.len() {
            if let Entry::Bracket {
                open: '\u{27E8}',
                at,
            } = self.stack[index]
                && let Some(&Entry::Bar {
                    fence: '|',
                    at: bar,
                    ..
                }) = self.stack.get(index + 1)
            {
                self.pair(at, bar);
            }
        }
        self.stack.truncate(len);
        while let Some(&index) = self.brackets.last()
            && index >= len
        {
            self.brackets.pop();
            let kind = self.kinds.pop().expect("a kind per bracket");
            self.counts[kind] -= 1;
        }
    }

    /// Drop the entries above the innermost bracket other than `⟨` (a matrix
    /// cell ends).
    fn end_cell(&mut self) {
        let mut len = 0;
        for (&index, &kind) in self.brackets.iter().zip(&self.kinds).rev() {
            if kind != ANGLE {
                len = index + 1;
                break;
            }
        }
        self.unwind(len);
    }

    /// The closing bracket of `kind` at `at`.
    fn close_bracket(&mut self, kind: usize, at: usize) {
        if kind == ANGLE {
            // An angle bracket does not match across another bracket.
            if self.kinds.last() == Some(&ANGLE) {
                let index = *self.brackets.last().expect("a bracket per kind");
                self.unwind_matched(index, at);
            } else if let Some(&Entry::Bar {
                fence: '|',
                at: bar,
                divider: false,
                ..
            }) = self.stack.last()
            {
                self.pair(bar, at);
                self.stack.pop();
            }
        } else if self.counts[kind] > 0
            && let Some(position) = self.kinds.iter().rposition(|&open| open == kind)
        {
            let index = self.brackets[position];
            self.unwind_matched(index, at);
        }
    }

    /// Drop the entries from the bracket at `index`, which the one at `close`
    /// matches.
    fn unwind_matched(&mut self, index: usize, close: usize) {
        self.unwind(index + 1);
        if let Some(Entry::Bracket { at, .. }) = self.stack.pop()
            && self.kinds.last().is_some_and(|&kind| kind >= ANGLE)
        {
            self.groups.push((at, close));
            self.groups.push((close, at));
        }
        self.brackets.pop();
        let kind = self.kinds.pop().expect("a kind per bracket");
        self.counts[kind] -= 1;
    }

    fn bar(&mut self, fence: char, at: usize, rest: &str, previous: Previous) -> Previous {
        let scripted = rest.starts_with(['_', '^', '┬', '┴']);
        let spaced = rest.starts_with(is_blank);
        let after_operand = previous == Previous::Operand;
        let before_operand = starts_operand(rest);
        if let Some(Entry::Bar {
            fence: open_fence,
            at: open,
            after_operand: open_after_operand,
            divider,
            separated,
        }) = self.stack.last_mut()
            && *open_fence == fence
        {
            let closes = if *divider {
                after_operand && !spaced && !scripted && !before_operand
            } else {
                after_operand || spaced || scripted || !may_open(rest)
            };
            if closes && *separated && *open_after_operand && before_operand {
                // Two dividers; the second may open a pair.
                self.stack.pop();
            } else if closes {
                let open = *open;
                self.pair(open, at);
                self.stack.pop();
                return Previous::Operand;
            }
        }
        if scripted {
            return Previous::Operand;
        }
        self.stack.push(Entry::Bar {
            fence,
            at,
            after_operand,
            divider: spaced && after_operand,
            separated: false,
        });
        Previous::Operator
    }
}

/// Whether a script or an accent starts `rest`.
fn is_decorated(rest: &str) -> bool {
    rest.starts_with(['_', '^', '┬', '┴']) || super::unicode_accent_suffix(rest).is_some()
}

/// Whether an operand starts right at `rest`.
fn starts_operand(rest: &str) -> bool {
    rest.chars().next().is_some_and(|c| {
        is_alphanumeric(c)
            || matches!(c, '(' | '[' | '{' | '〖' | '√' | '∛' | '∜' | '\u{27E8}')
            || is_nary_char(c)
    })
}

/// Whether a bar right before `rest` may open a pair: not at the end, and not
/// before a closing bracket, a fraction bar, a relation or a separator
/// (`|a+|=b`).
fn may_open(rest: &str) -> bool {
    rest.chars().next().is_some_and(|c| {
        !matches!(c, '/' | '\u{A6}' | '&' | '@')
            && BRACKETS.iter().all(|pair| pair.1 != c)
            && !separates_bars(c)
    })
}

/// Whether `c` sets apart what a bar pair rarely spans: a relation, a
/// separator or a logical operator.
fn separates_bars(c: char) -> bool {
    matches!(c, ',' | ';' | '∧' | '∨')
        || (!is_alphanumeric(c) && classify_unicode_symbol(c) == SymbolKind::Relation)
}

/// What pairs with what in a source, by byte offset, in order.
#[derive(Default)]
pub(super) struct Pairs {
    /// The partner of every paired bar, of the `⟩` of a ket and of the `⟨` of
    /// a bra.
    pub(super) bars: Vec<(usize, usize)>,
    /// The partner of every matched `⟨`, `⌈`, `⌊` and their closing brackets.
    pub(super) groups: Vec<(usize, usize)>,
}

impl Pairs {
    pub(super) fn is_empty(&self) -> bool {
        self.bars.is_empty() && self.groups.is_empty()
    }
}

/// What the bar at `at` in `source` pairs with.
pub(crate) fn pair_bars_at(source: &str, at: usize) -> Option<usize> {
    let Pairs { bars, .. } = pair_bars(source);
    let index = bars.binary_search_by_key(&at, |&(bar, _)| bar).ok()?;
    Some(bars[index].1)
}

/// Whether an n-aryand written `text` that has bars is read whole: outside its
/// bar pairs and brackets it has nothing that ends one.
pub(crate) fn reads_as_naryand(text: &str) -> bool {
    let Pairs { bars, groups } = pair_bars(text);
    let mut until = None;
    let mut depth = 0_usize;
    for (at, c) in text.char_indices() {
        if until.is_some_and(|end| at <= end) {
            continue;
        }
        match c {
            '(' | '[' | '{' | '〖' => depth += 1,
            ')' | ']' | '}' | '〗' if depth == 0 => return false,
            ')' | ']' | '}' | '〗' => depth -= 1,
            _ if depth > 0 => {},
            _ if c.is_whitespace() || c == '"' => return false,
            _ => {
                let find = |pairs: &[(usize, usize)]| {
                    let index = pairs.binary_search_by_key(&at, |&(open, _)| open).ok()?;
                    Some(pairs[index].1)
                };
                let pair = find(&bars).or_else(|| find(&groups).filter(|_| at == 0));
                if pair.is_some_and(|end| end > at) {
                    until = pair;
                } else if pairs_bars(c) || super::breaks_naryand(c) {
                    return false;
                }
            },
        }
    }
    depth == 0
}

/// Pair the bars and brackets of `source`.
pub(super) fn pair_bars(source: &str) -> Pairs {
    // `|`, or the UTF-8 of `‖`, `⟨`, `⌈` or `⌊`.
    let bytes = source.as_bytes();
    let pairable = |(index, &byte): (usize, &u8)| match byte {
        b'|' => true,
        0xE2 => matches!(
            bytes.get(index + 1..index + 3),
            Some([0x80, 0x96] | [0x9F, 0xA8] | [0x8C, 0x88 | 0x8A])
        ),
        _ => false,
    };
    if !bytes.iter().enumerate().any(pairable) {
        return Pairs::default();
    }
    let mut scan = Scan::default();
    let mut previous = Previous::Operator;
    // Bracket depths at which an n-ary operator's n-aryand is still to come.
    let mut naries: Vec<usize> = Vec::new();
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        let at = source.len() - rest.len();
        rest = &rest[c.len_utf8()..];
        previous = match c {
            // A space after an n-ary operator and its scripts starts the
            // n-aryand (`∑_i |x|`).
            _ if is_blank(c) => {
                while naries
                    .last()
                    .is_some_and(|&depth| depth > scan.brackets.len())
                {
                    naries.pop();
                }
                if naries.last() == Some(&scan.brackets.len()) {
                    naries.pop();
                    Previous::Operator
                } else {
                    previous
                }
            },
            _ if c.is_ascii_alphanumeric() => Previous::Operand,
            '|' | '\u{2016}' => scan.bar(c, at, rest, previous),
            '"' => {
                rest = rest.split_once('"').map_or("", |(_, after)| after);
                Previous::Operand
            },
            // The character after these is an operand (`\/`, `x^*`).
            '\\' | '_' | '^' | '┬' | '┴' => {
                if let Some(next) = rest.chars().next()
                    && breaks_operand_at(next)
                    && BRACKETS.iter().all(|pair| pair.0 != next)
                {
                    rest = &rest[next.len_utf8()..];
                    Previous::Operand
                } else {
                    Previous::Operator
                }
            },
            '&' | '@' => {
                scan.end_cell();
                Previous::Operator
            },
            '/' | '\u{A6}' | '\u{2592}' | '√' | '∛' | '∜' | '├' => Previous::Operator,
            // A postfix operator (`|A^†|`, `|A†|`).
            '\u{2020}' | '\u{2021}' => Previous::Operand,
            _ => {
                if c == '\u{27E8}' && is_decorated(rest) {
                    // A base, not a delimiter (`⟨̂`).
                    Previous::Operand
                } else if let Some(kind) = BRACKETS.iter().position(|pair| pair.0 == c) {
                    scan.push_bracket(kind, at);
                    Previous::Operator
                } else if let Some(kind) = BRACKETS.iter().position(|pair| pair.1 == c) {
                    scan.close_bracket(kind, at);
                    Previous::Operand
                } else if is_nary_char(c) {
                    if naries.last() != Some(&scan.brackets.len()) {
                        naries.push(scan.brackets.len());
                    }
                    Previous::Operator
                } else if breaks_operand_at(c) {
                    if separates_bars(c)
                        && let Some(Entry::Bar { separated, .. }) = scan.stack.last_mut()
                    {
                        *separated = true;
                    }
                    Previous::Operator
                } else {
                    Previous::Operand
                }
            },
        };
    }
    scan.unwind(0);
    let Scan {
        pairs: mut bars,
        mut groups,
        ..
    } = scan;
    bars.sort_unstable();
    groups.sort_unstable();
    Pairs { bars, groups }
}
