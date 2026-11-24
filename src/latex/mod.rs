pub mod error;
mod macro_args;
pub mod mhchem;
pub mod packages;
pub mod parse;
pub mod physics;
pub mod preprocess;
pub mod siunitx;

/// Capture a balanced `{ ... }` group at the start of `input` (after optional
/// leading whitespace).
///
/// Returns `(rest_after_close_brace, body_without_braces)` or `None` when the
/// input does not begin with a brace group. Backslash escapes (`\{`, `\}`) are
/// honored so escaped braces do not unbalance the scan.
#[must_use]
pub(crate) fn capture_braced(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let rest = trimmed.strip_prefix('{')?;
    let mut depth = 1usize;
    let mut escaped = false;
    for (i, &b) in rest.as_bytes().iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' => escaped = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&rest[i + 1..], &rest[..i]));
                }
            },
            _ => {},
        }
    }
    None
}

/// Find the first closing square bracket outside TeX groups. The caller
/// supplies the contents after the optional argument's opening bracket.
/// Literal control symbols and verbatim tokens do not change group depth.
pub(crate) fn optional_argument_end(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if let Some(end) = preprocess::find_verb_end(input, i) {
                    i = end;
                } else {
                    i += 1;
                    if let Some(ch) = input[i..].chars().next() {
                        i += ch.len_utf8();
                    }
                }
                continue;
            },
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b']' if depth == 0 => return Some(i),
            _ => {},
        }
        i += 1;
    }
    None
}

/// Length of a control word after the backslash. Recognize KaTeX's
/// leading-`@` internal names while keeping the ordinary math character in
/// expressions such as `\alpha@x` outside the control word.
#[inline]
fn control_word_len(name: &str) -> usize {
    let bytes = name.as_bytes();
    if bytes.first() == Some(&b'@') && bytes.get(1).is_some_and(u8::is_ascii_alphabetic) {
        bytes
            .iter()
            .take_while(|&&b| b.is_ascii_alphabetic() || b == b'@')
            .count()
    } else {
        bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count()
    }
}

/// Split off one TeX character, control word, or control symbol before argument expansion.
fn split_token(input: &str) -> Option<(&str, &str)> {
    let len = if let Some(name) = input.strip_prefix('\\') {
        let first = name.chars().next()?;
        1 + control_word_len(name).max(first.len_utf8())
    } else {
        let mut chars = input.chars();
        let base = chars.next()?;
        base.len_utf8()
            + chars
                .take_while(|c| ('\u{0300}'..='\u{036f}').contains(c))
                .map(char::len_utf8)
                .sum::<usize>()
    };
    Some((&input[len..], &input[..len]))
}

fn text_style(name: &str) -> Option<crate::ast::TextStyle> {
    use crate::ast::TextStyle;
    Some(match name {
        "textbf" => TextStyle::Bold,
        "textit" | "emph" => TextStyle::Italic,
        "texttt" => TextStyle::Monospace,
        "textsf" => TextStyle::SansSerif,
        "text" | "textrm" | "textnormal" | "textup" | "textmd" | "mbox" | "hbox" => {
            TextStyle::Normal
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
