#![allow(dead_code)]

//! Token-aware support for TeX primitive macro definitions.
//!
//! A `\def`/`\gdef` parameter text is not a collection of byte prefixes.  It
//! consists of a (possibly empty) sequence of tokens before `#1`, followed by
//! a delimiter sequence after every parameter marker.  Keeping those token
//! boundaries is important for control words (`\foo` must not match the start
//! of `\foobar`) and for escaped braces (`\{` is a control symbol, not a group
//! delimiter).
//!
//! This module deliberately does not know about macro scopes or expansion.  It
//! parses one definition and consumes one invocation, returning borrowed input
//! slices.  The preprocessor can therefore copy an argument only when it is
//! actually substituted, instead of rescanning/copying the whole remaining
//! input for each parameter.

use std::fmt;

use super::split_token;

/// A token in a definition's parameter text.
///
/// `split_token` is the parser's token identity function.  Storing the token
/// text, rather than a byte substring to compare later, keeps delimiter
/// comparisons exact even when a delimiter is a control word or a control
/// symbol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParameterToken(String);

impl ParameterToken {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The parameter text of a primitive macro definition.
///
/// `delimiters[0]` is the prefix before the first parameter.  For `n > 0`,
/// `delimiters[n]` is the token sequence between `#n` and either `#(n+1)` or
/// the replacement text.  This is the same shape used by KaTeX's macro
/// expander and makes prefix and multi-token delimiters explicit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParameterText {
    delimiters: Vec<Vec<ParameterToken>>,
    parameter_count: u8,
    terminal_open: bool,
}

impl ParameterText {
    /// Number of numbered parameters (`#1` through `#9`).
    pub(crate) fn parameter_count(&self) -> u8 {
        self.parameter_count
    }

    /// Whether the parameter text ended in the special `#{` form.
    ///
    /// KaTeX models this as an opening-brace delimiter and prepends that same
    /// opening brace to the replacement token stream.  The closing brace is
    /// supplied by the invocation's remaining input.
    pub(crate) fn terminal_open(&self) -> bool {
        self.terminal_open
    }

    /// Token delimiters in definition order.  The first entry is the prefix
    /// before `#1`; each subsequent entry terminates the corresponding
    /// argument.  The stream preprocessor uses this view while retaining its
    /// own continuation queue (which may contain replacement tokens).
    pub(crate) fn delimiters(&self) -> &[Vec<ParameterToken>] {
        &self.delimiters
    }
}

/// A parsed `\def` or `\gdef` command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedDefinition<'a> {
    /// Input after the balanced replacement text.
    pub(crate) rest: &'a str,
    /// The control sequence name, including its leading backslash.
    pub(crate) name: &'a str,
    /// Parameter text and delimiter sequences.
    pub(crate) parameter_text: ParameterText,
    /// Replacement text without its outer braces.
    pub(crate) body: &'a str,
    /// `true` for `\gdef`, `false` for `\def`.
    pub(crate) global: bool,
}

/// Errors from a malformed primitive definition or invocation.
///
/// The preprocessor intentionally treats these as a failed expansion and
/// leaves the original command for the normal parser to reject.  Keeping the
/// errors typed makes standalone users/tests able to distinguish a malformed
/// definition from a non-definition without panicking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacroArgumentError {
    InvalidControlSequence,
    InvalidParameterNumber,
    ParameterNumberOutOfOrder,
    MissingDefinitionBody,
    UnbalancedDefinitionBody,
    PrefixMismatch,
    MissingArgument,
    UnbalancedArgument,
    ExtraClosingBrace,
    MissingDelimiter,
}

impl fmt::Display for MacroArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidControlSequence => "expected a control sequence",
            Self::InvalidParameterNumber => "invalid macro parameter number",
            Self::ParameterNumberOutOfOrder => "macro parameter numbers must be consecutive",
            Self::MissingDefinitionBody => "expected a macro replacement body",
            Self::UnbalancedDefinitionBody => "unbalanced macro replacement body",
            Self::PrefixMismatch => "macro invocation does not match its definition",
            Self::MissingArgument => "missing macro argument",
            Self::UnbalancedArgument => "unbalanced macro argument",
            Self::ExtraClosingBrace => "extra closing brace in macro argument",
            Self::MissingDelimiter => "missing macro argument delimiter",
        };
        f.write_str(message)
    }
}

/// A successful argument scan.  Every argument is a borrowed slice of the
/// invocation.  Braces around an undelimited braced argument are omitted, as
/// they are by TeX/KaTeX; delimited arguments retain their source tokens.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ConsumedArguments<'a> {
    pub(crate) rest: &'a str,
    pub(crate) arguments: Vec<&'a str>,
}

#[derive(Clone, Copy, Debug)]
struct TokenRef<'a> {
    start: usize,
    text_end: usize,
    end: usize,
    text: &'a str,
}

/// Parse a primitive definition beginning at `input`.
///
/// `Ok(None)` means that the input does not begin with exactly `\def` or
/// `\gdef`.  A command that does begin with one of those names but is malformed
/// returns `Err`; callers can leave it in the stream so the regular parser
/// reports the usual syntax error.
pub(crate) fn parse_definition(
    input: &str,
) -> Result<Option<ParsedDefinition<'_>>, MacroArgumentError> {
    let Some(command) = next_token(input, 0) else {
        return Ok(None);
    };
    let global = match command.text {
        "\\def" => false,
        "\\gdef" => true,
        _ => return Ok(None),
    };

    let Some(name_token) = next_token(input, command.end) else {
        return Err(MacroArgumentError::InvalidControlSequence);
    };
    if !is_control_sequence(name_token.text) {
        return Err(MacroArgumentError::InvalidControlSequence);
    }

    let mut cursor = name_token.end;
    let mut delimiters = vec![Vec::new()];
    let mut parameter_count = 0u8;
    let mut terminal_open = false;
    let body_open;

    loop {
        let Some(token) = next_token(input, cursor) else {
            return Err(MacroArgumentError::MissingDefinitionBody);
        };
        cursor = token.end;

        if token.text == "{" {
            body_open = token;
            break;
        }

        if token.text == "#" {
            let Some(number) = next_token(input, cursor) else {
                return Err(MacroArgumentError::InvalidParameterNumber);
            };

            // KaTeX/TeX's special terminal form `# {` (with no intervening
            // token; whitespace is a token and therefore does not qualify)
            // turns the body-opening brace into a delimiter and inserts that
            // opening brace into the replacement text.
            if number.text == "{" {
                delimiters[parameter_count as usize].push(ParameterToken("{".to_owned()));
                terminal_open = true;
                body_open = number;
                break;
            }

            if number.text.len() != 1
                || !number.text.as_bytes()[0].is_ascii_digit()
                || number.text == "0"
            {
                return Err(MacroArgumentError::InvalidParameterNumber);
            }
            let expected = parameter_count.saturating_add(1);
            let actual = number.text.as_bytes()[0] - b'0';
            if actual != expected {
                return Err(MacroArgumentError::ParameterNumberOutOfOrder);
            }
            if parameter_count == 9 {
                return Err(MacroArgumentError::ParameterNumberOutOfOrder);
            }
            parameter_count = actual;
            delimiters.push(Vec::new());
            cursor = number.end;
            continue;
        }

        delimiters[parameter_count as usize].push(ParameterToken(token.text.to_owned()));
    }

    let Some((rest, body)) = scan_definition_body(input, body_open) else {
        return Err(MacroArgumentError::UnbalancedDefinitionBody);
    };

    Ok(Some(ParsedDefinition {
        rest,
        name: name_token.text,
        parameter_text: ParameterText {
            delimiters,
            parameter_count,
            terminal_open,
        },
        body,
        global,
    }))
}

/// Consume one invocation's arguments according to `parameter_text`.
///
/// The scan is bounded by `input.len()`: each loop advances by one token and
/// each delimiter look-ahead is bounded by that delimiter's token count.  No
/// temporary copy of the input is made; callers receive source slices.
pub(crate) fn consume_arguments<'a>(
    input: &'a str,
    parameter_text: &ParameterText,
) -> Result<ConsumedArguments<'a>, MacroArgumentError> {
    let mut cursor = 0usize;
    consume_exact_delimiter(input, &mut cursor, &parameter_text.delimiters[0])?;

    let mut arguments = Vec::with_capacity(parameter_text.parameter_count as usize);
    for index in 0..parameter_text.parameter_count as usize {
        let delimiter = &parameter_text.delimiters[index + 1];
        let (rest, argument) = if delimiter.is_empty() {
            consume_undelimited(input, cursor)?
        } else {
            consume_delimited(input, cursor, delimiter)?
        };
        cursor = rest;
        arguments.push(argument);
    }

    Ok(ConsumedArguments {
        rest: &input[cursor..],
        arguments,
    })
}

/// Substitute numbered references in replacement text.
///
/// `terminal_open` mirrors KaTeX's `# {` handling: the body-opening brace is
/// prepended without a matching close; the invocation's opening brace was
/// consumed as the final delimiter and its remaining source supplies the
/// corresponding close.
pub(crate) fn substitute(body: &str, arguments: &[&str], terminal_open: bool) -> String {
    let mut out = String::with_capacity(
        body.len()
            .saturating_add(usize::from(terminal_open))
            .saturating_add(arguments.iter().map(|arg| arg.len()).sum::<usize>()),
    );
    if terminal_open {
        out.push('{');
    }

    let mut cursor = 0usize;
    while cursor < body.len() {
        let ch = body[cursor..]
            .chars()
            .next()
            .expect("cursor is on a char boundary");
        if ch == '\\' {
            // A control symbol such as `\#` owns the following character; its
            // hash is literal, not a parameter reference.  Copy the token in
            // one step so `\#1` stays literal.
            let start = cursor;
            cursor += ch.len_utf8();
            if cursor < body.len() {
                let next = body[cursor..].chars().next().expect("cursor is valid");
                cursor += next.len_utf8();
                if next.is_ascii_alphabetic() || next == '@' {
                    while cursor < body.len() {
                        let letter = body[cursor..].chars().next().expect("cursor is valid");
                        if !(letter.is_ascii_alphabetic() || letter == '@') {
                            break;
                        }
                        cursor += letter.len_utf8();
                    }
                }
            }
            out.push_str(&body[start..cursor]);
            continue;
        }

        if ch == '#' {
            let start = cursor;
            cursor += 1;
            if cursor < body.len() {
                let next = body[cursor..].chars().next().expect("cursor is valid");
                if next == '#' {
                    // TeX's doubled hash denotes a literal hash in replacement
                    // text.  It is also harmless for callers that never use it.
                    out.push('#');
                    cursor += 1;
                    continue;
                }
                if let Some(digit) = next.to_digit(10)
                    && (1..=9).contains(&digit)
                    && (digit as usize) <= arguments.len()
                {
                    out.push_str(arguments[digit as usize - 1]);
                    cursor += next.len_utf8();
                    continue;
                }
            }
            out.push_str(&body[start..cursor]);
            continue;
        }

        out.push(ch);
        cursor += ch.len_utf8();
    }
    out
}

fn is_control_sequence(text: &str) -> bool {
    text.starts_with('\\') && text.len() > 1 && !matches!(text, "\\{" | "\\}" | "\\#")
}

fn next_token(input: &str, offset: usize) -> Option<TokenRef<'_>> {
    let suffix = input.get(offset..)?;
    let (rest, text) = split_token(suffix)?;
    let text_end = offset + (suffix.len() - rest.len());
    let mut end = text_end;
    // TeX's lexer consumes the whitespace following a control word as part of
    // that token.  `split_token` intentionally leaves source whitespace in the
    // returned remainder for the ordinary parser, so advance only this local
    // cursor while retaining the exact token text for comparisons.
    if is_control_word(text) {
        while let Some(ch) = input[end..].chars().next() {
            if !ch.is_ascii_whitespace() {
                break;
            }
            end += ch.len_utf8();
        }
    }
    Some(TokenRef {
        start: offset,
        text_end,
        end,
        text,
    })
}

fn scan_definition_body<'a>(input: &'a str, opening: TokenRef<'a>) -> Option<(&'a str, &'a str)> {
    let body_start = opening.end;
    let mut cursor = body_start;
    let mut depth = 1usize;
    while cursor < input.len() {
        let token = next_token(input, cursor)?;
        cursor = token.end;
        match token.text {
            "{" => depth = depth.checked_add(1)?,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[token.end..], &input[body_start..token.start]));
                }
            },
            _ => {},
        }
    }
    None
}

fn consume_exact_delimiter(
    input: &str,
    cursor: &mut usize,
    delimiter: &[ParameterToken],
) -> Result<(), MacroArgumentError> {
    for expected in delimiter {
        let Some(token) = next_token(input, *cursor) else {
            return Err(MacroArgumentError::PrefixMismatch);
        };
        if token.text != expected.as_str() {
            return Err(MacroArgumentError::PrefixMismatch);
        }
        *cursor = token.end;
    }
    Ok(())
}

fn consume_undelimited(input: &str, cursor: usize) -> Result<(usize, &str), MacroArgumentError> {
    let mut start = cursor;
    while let Some(token) = next_token(input, start) {
        if !token.text.chars().all(char::is_whitespace) {
            break;
        }
        start = token.end;
    }
    let Some(token) = next_token(input, start) else {
        return Err(MacroArgumentError::MissingArgument);
    };
    if token.text == "{" {
        let Some((rest, body)) = scan_group(input, token) else {
            return Err(MacroArgumentError::UnbalancedArgument);
        };
        return Ok((input.len() - rest.len(), body));
    }
    if token.text == "}" {
        return Err(MacroArgumentError::ExtraClosingBrace);
    }
    Ok((token.end, &input[token.start..token.text_end]))
}

fn is_control_word(text: &str) -> bool {
    let Some(name) = text.strip_prefix('\\') else {
        return false;
    };
    !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '@')
}

fn consume_delimited<'a>(
    input: &'a str,
    cursor: usize,
    delimiter: &[ParameterToken],
) -> Result<(usize, &'a str), MacroArgumentError> {
    let argument_start = cursor;
    let mut position = cursor;
    let mut depth = 0usize;
    while position < input.len() {
        let Some(token) = next_token(input, position) else {
            break;
        };

        // A terminal `#{` delimiter is matched at the opening brace itself,
        // before that brace contributes to group depth.  This is the one
        // opening brace that TeX allows to act as a parameter delimiter.
        if depth == 0 && delimiter_matches(input, position, delimiter) {
            let Some(end) = delimiter_end(input, position, delimiter) else {
                return Err(MacroArgumentError::MissingDelimiter);
            };
            return Ok((end, &input[argument_start..position]));
        }

        match token.text {
            "{" => depth = depth.saturating_add(1),
            "}" => {
                if depth == 0 {
                    return Err(MacroArgumentError::ExtraClosingBrace);
                }
                depth -= 1;
            },
            _ => {},
        }
        position = token.end;
    }
    Err(MacroArgumentError::MissingDelimiter)
}

fn delimiter_matches(input: &str, position: usize, delimiter: &[ParameterToken]) -> bool {
    delimiter_end(input, position, delimiter).is_some()
}

fn delimiter_end(input: &str, position: usize, delimiter: &[ParameterToken]) -> Option<usize> {
    let mut cursor = position;
    for expected in delimiter {
        let token = next_token(input, cursor)?;
        if token.text != expected.as_str() {
            return None;
        }
        cursor = token.end;
    }
    Some(cursor)
}

fn scan_group<'a>(input: &'a str, opening: TokenRef<'a>) -> Option<(&'a str, &'a str)> {
    let body_start = opening.end;
    let mut cursor = body_start;
    let mut depth = 1usize;
    while cursor < input.len() {
        let token = next_token(input, cursor)?;
        cursor = token.end;
        match token.text {
            "{" => depth = depth.checked_add(1)?,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[token.end..], &input[body_start..token.start]));
                }
            },
            _ => {},
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> ParsedDefinition<'_> {
        parse_definition(input).unwrap().unwrap()
    }

    #[test]
    fn parses_prefix_and_consecutive_parameters() {
        let def = parsed(r"\gdef\foo|#1#2|{#1+#2}tail");
        assert_eq!(def.name, r"\foo");
        assert!(def.global);
        assert_eq!(def.parameter_text.parameter_count(), 2);
        assert_eq!(def.parameter_text.delimiters[0][0].as_str(), "|");
        assert!(def.parameter_text.delimiters[1].is_empty());
        assert_eq!(def.parameter_text.delimiters[2][0].as_str(), "|");
        assert_eq!(def.body, "#1+#2");
        assert_eq!(def.rest, "tail");
    }

    #[test]
    fn rejects_nonconsecutive_or_two_digit_parameter_numbers() {
        assert_eq!(
            parse_definition(r"\def\foo#2{}"),
            Err(MacroArgumentError::ParameterNumberOutOfOrder)
        );
        assert_eq!(
            parse_definition(r"\def\foo#a{}"),
            Err(MacroArgumentError::InvalidParameterNumber)
        );
        assert_eq!(
            parse_definition(r"\def\foo#1#3{}"),
            Err(MacroArgumentError::ParameterNumberOutOfOrder)
        );
        assert_eq!(
            parse_definition(r"\def\foo#1#2#3#4#5#6#7#8#9#10{}"),
            Err(MacroArgumentError::ParameterNumberOutOfOrder)
        );
    }

    #[test]
    fn consumes_balanced_and_delimited_arguments() {
        let def = parsed(r"\gdef\foo|#1||{#1}");
        let args = consume_arguments("| x {y} ||rest", &def.parameter_text).unwrap();
        assert_eq!(args.arguments, [" x {y} "]);
        assert_eq!(args.rest, "rest");

        let def = parsed(r"\gdef\foo#1|#2{#1+#2}");
        let args = consume_arguments(" 1 2 |34", &def.parameter_text).unwrap();
        assert_eq!(args.arguments, [" 1 2 ", "3"]);
        assert_eq!(args.rest, "4");
    }

    #[test]
    fn terminal_hash_opening_brace_is_a_delimiter() {
        let def = parsed(r"\gdef\foo#1#{#1}");
        assert!(def.parameter_text.terminal_open());
        let args = consume_arguments("1^{23}", &def.parameter_text).unwrap();
        assert_eq!(args.arguments, ["1^"]);
        assert_eq!(args.rest, "23}");
        assert_eq!(substitute(def.body, &args.arguments, true), "{1^");
    }

    #[test]
    fn escaped_braces_are_tokens_not_group_boundaries() {
        let def = parsed(r"\def\foo\{#1\}{#1}");
        let args = consume_arguments(r"\{value\}tail", &def.parameter_text).unwrap();
        assert_eq!(args.arguments, ["value"]);
        assert_eq!(args.rest, "tail");

        let def = parsed(r"\def\foo{\{x\}}");
        assert_eq!(def.body, r"\{x\}");
    }

    #[test]
    fn control_word_delimiters_compare_as_single_tokens() {
        let def = parsed(r"\def\foo\bar#1{#1}");
        assert_eq!(
            consume_arguments(r"\barX", &def.parameter_text),
            Err(MacroArgumentError::PrefixMismatch)
        );
        let args = consume_arguments(r"\bar X", &def.parameter_text).unwrap();
        assert_eq!(args.arguments, ["X"]);
    }

    #[test]
    fn malformed_bodies_are_errors_not_panics() {
        assert_eq!(
            parse_definition(r"\def\foo{unclosed"),
            Err(MacroArgumentError::UnbalancedDefinitionBody)
        );
        assert_eq!(
            parse_definition(r"\gdef{\foo}{}"),
            Err(MacroArgumentError::InvalidControlSequence)
        );
        assert_eq!(
            parse_definition(r"\gdef\foo\bar"),
            Err(MacroArgumentError::MissingDefinitionBody)
        );
    }
}
