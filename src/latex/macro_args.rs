//! Token-aware parameter text for primitive TeX macro definitions.
//!
//! The ordered preprocessor owns the input queue and macro scopes.  This
//! module owns only the small grammar shared by `\def`, `\gdef`, `\edef`, and
//! `\xdef`: a prefix followed by consecutive `#1`…`#9` markers and their
//! delimiter token sequences.  The parser receives tokens that have already
//! been lexed by the active stream, so it never detokenizes and retokenizes a
//! synthetic definition string.

/// A token in a primitive definition's parameter text.
///
/// The active stream stores control words and symbols separately, but the
/// parameter grammar needs only their exact TeX token spelling for delimiter
/// identity.  Keeping this owned spelling is cheap (parameter headers are
/// bounded by the input) and lets the stream retain its own token queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParameterToken(String);

impl ParameterToken {
    pub(crate) fn from_text(text: String) -> Self {
        Self(text)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A parsed primitive parameter text.
///
/// `delimiters[0]` is the prefix before the first parameter.  For `n > 0`,
/// `delimiters[n]` is the token sequence between `#n` and either `#(n+1)` or
/// the replacement text.  The shape matches KaTeX's delimiter table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParameterText {
    delimiters: Vec<Vec<ParameterToken>>,
    parameter_count: u8,
    terminal_open: bool,
}

impl ParameterText {
    pub(crate) fn parameter_count(&self) -> u8 {
        self.parameter_count
    }

    pub(crate) fn terminal_open(&self) -> bool {
        self.terminal_open
    }

    pub(crate) fn delimiters(&self) -> &[Vec<ParameterToken>] {
        &self.delimiters
    }
}

/// Reasons a tokenized parameter text cannot define a macro.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParameterTextError {
    InvalidParameterNumber,
    ParameterNumberOutOfOrder,
    TokensAfterTerminalHash,
}

/// Parse tokens between a macro name and its replacement group's opening
/// brace.
///
/// `parse_def_txn` has already consumed the opening brace boundary, so a
/// terminal `#` is represented by the final token in `tokens`; TeX treats the
/// immediately following replacement-group `{` as the delimiter and inserts
/// that opening brace into the replacement stream.  Accepting an explicit `{`
/// after `#` as well keeps this helper useful for direct callers/tests.
pub(crate) fn parse_parameter_text(
    tokens: &[ParameterToken],
) -> Result<ParameterText, ParameterTextError> {
    let mut delimiters = vec![Vec::new()];
    let mut parameter_count = 0u8;
    let mut terminal_open = false;
    let mut index = 0usize;

    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token != "#" {
            delimiters[parameter_count as usize].push(tokens[index].clone());
            index += 1;
            continue;
        }

        let Some(number) = tokens.get(index + 1) else {
            delimiters[parameter_count as usize].push(ParameterToken::from_text("{".to_owned()));
            terminal_open = true;
            index += 1;
            break;
        };
        if number.as_str() == "{" {
            delimiters[parameter_count as usize].push(ParameterToken::from_text("{".to_owned()));
            terminal_open = true;
            index += 2;
            break;
        }
        if number.as_str().len() != 1
            || !number.as_str().as_bytes()[0].is_ascii_digit()
            || number.as_str() == "0"
        {
            return Err(ParameterTextError::InvalidParameterNumber);
        }
        let expected = parameter_count.saturating_add(1);
        let actual = number.as_str().as_bytes()[0] - b'0';
        if actual != expected || parameter_count == 9 {
            return Err(ParameterTextError::ParameterNumberOutOfOrder);
        }
        parameter_count = actual;
        delimiters.push(Vec::new());
        index += 2;
    }

    if terminal_open && index != tokens.len() {
        return Err(ParameterTextError::TokensAfterTerminalHash);
    }

    Ok(ParameterText {
        delimiters,
        parameter_count,
        terminal_open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<ParameterToken> {
        text.chars()
            .map(|ch| ParameterToken::from_text(ch.to_string()))
            .collect()
    }

    #[test]
    fn parses_prefix_and_consecutive_parameters() {
        let parsed = parse_parameter_text(&tokens("|#1#2|")).unwrap();
        assert_eq!(parsed.parameter_count(), 2);
        assert_eq!(parsed.delimiters()[0][0].as_str(), "|");
        assert!(parsed.delimiters()[1].is_empty());
        assert_eq!(parsed.delimiters()[2][0].as_str(), "|");
    }

    #[test]
    fn validates_number_order_and_terminal_hash() {
        assert_eq!(
            parse_parameter_text(&tokens("#2")),
            Err(ParameterTextError::ParameterNumberOutOfOrder)
        );
        assert_eq!(
            parse_parameter_text(&tokens("#a")),
            Err(ParameterTextError::InvalidParameterNumber)
        );
        let parsed = parse_parameter_text(&tokens("#1#")).unwrap();
        assert!(parsed.terminal_open());
        assert_eq!(parsed.delimiters()[1][0].as_str(), "{");
    }
}
