pub mod error;
mod macro_args;
pub mod mhchem;
pub mod packages;
pub mod parse;
pub mod physics;
pub mod preprocess;
pub mod siunitx;

/// Length of a control word after the backslash.  Internal KaTeX names may
/// begin with `@`; an `@` occurring after an ordinary alphabetic command is
/// still an ordinary source character (`\alpha@x`).
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
        "textit" => TextStyle::Italic,
        "texttt" => TextStyle::Monospace,
        "textsf" => TextStyle::SansSerif,
        "text" | "textrm" | "textnormal" | "textup" | "mbox" | "hbox" => TextStyle::Normal,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
