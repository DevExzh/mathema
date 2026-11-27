//! Characters XML 1.0 cannot carry.
//!
//! XML 1.0 has no escape for the C0 controls other than tab, line feed and
//! carriage return, nor for U+FFFE and U+FFFF (§2.2): a document holding one is
//! not well-formed, and `&#x13;` is no better. The XML writers (MathML and both
//! SVG outputs) replace each such character with U+FFFD REPLACEMENT CHARACTER,
//! which keeps a visible mark of it; OMML drops it with a loss diagnostic.

/// XML 1.0 `Char` production.
#[inline]
pub(crate) fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}')
        || c >= '\u{10000}'
}

/// Whether `text` holds a character XML 1.0 cannot carry. Looks at bytes:
/// a control below U+0020 is one byte, U+FFFE and U+FFFF are `EF BF BE/BF`.
#[inline]
pub(crate) fn has_non_xml_char(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(at, &byte)| match byte {
        b'\t' | b'\n' | b'\r' => false,
        0..0x20 => true,
        0xEF => matches!(bytes.get(at + 1..at + 3), Some([0xBF, 0xBE | 0xBF])),
        _ => false,
    })
}

/// `text` with each character XML 1.0 cannot carry replaced by U+FFFD.
pub(crate) fn replace_non_xml_chars(text: String) -> String {
    if !has_non_xml_char(&text) {
        return text;
    }
    text.chars()
        .map(|c| if is_xml_char(c) { c } else { '\u{FFFD}' })
        .collect()
}

/// The diagnostic of a replacement.
pub(crate) const REPLACED: &str =
    "a character that XML 1.0 cannot represent was replaced by U+FFFD";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_byte_check_is_the_char_production() {
        for code in 0..=0x10FFFF {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let text = format!("a{c}b");
            assert_eq!(has_non_xml_char(&text), !is_xml_char(c), "{code:X}");
        }
        assert_eq!(
            replace_non_xml_chars("a\u{13}b\u{FFFF}".into()),
            "a\u{FFFD}b\u{FFFD}"
        );
    }
}
