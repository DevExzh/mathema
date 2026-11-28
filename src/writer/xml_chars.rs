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
    #[cfg(target_arch = "x86_64")]
    if bytes.len() >= 128 && std::is_x86_feature_detected!("avx2") {
        // The AVX2 path handles the common byte classes in wide chunks. It
        // falls back to one scalar sentinel scan only when an EF byte occurs,
        // which is needed to distinguish U+FFFE/U+FFFF from ordinary UTF-8.
        return unsafe { scan_xml_avx2::<false>(bytes) };
    }
    // The arena string pool is overwhelmingly ASCII. Avoid the per-byte
    // UTF-8 sentinel check on that hot path. `is_ascii` is a tight byte scan
    // which the release compiler lowers to a vector-friendly loop, while the
    // fallback below retains the precise FFFE/FFFF check for non-ASCII text.
    if bytes.is_ascii() {
        return bytes
            .iter()
            .any(|&byte| byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r'));
    }
    bytes.iter().enumerate().any(|(at, &byte)| match byte {
        b'\t' | b'\n' | b'\r' => false,
        0..0x20 => true,
        0xEF => matches!(bytes.get(at + 1..at + 3), Some([0xBF, 0xBE | 0xBF])),
        _ => false,
    })
}

/// Whether a UTF-8 string needs XML escaping or XML 1.0 character repair.
/// This combines the two byte scans used by SVG's streaming escaper.
#[inline]
#[cfg(feature = "render")]
pub(crate) fn needs_xml_sanitization(text: &str) -> bool {
    let bytes = text.as_bytes();
    #[cfg(target_arch = "x86_64")]
    if bytes.len() >= 128 && std::is_x86_feature_detected!("avx2") {
        return unsafe { scan_xml_avx2::<true>(bytes) };
    }
    if bytes.is_ascii() {
        return bytes.iter().any(|&byte| {
            matches!(byte, b'<' | b'>' | b'&' | b'"' | b'\'')
                || (byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r'))
        });
    }
    bytes.iter().enumerate().any(|(at, &byte)| match byte {
        b'<' | b'>' | b'&' | b'"' | b'\'' => true,
        b'\t' | b'\n' | b'\r' | 0x20..=0x7e => false,
        0..0x20 => true,
        0xEF => matches!(bytes.get(at + 1..at + 3), Some([0xBF, 0xBE | 0xBF])),
        _ => false,
    })
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn scan_xml_avx2<const ESCAPE: bool>(bytes: &[u8]) -> bool {
    use std::arch::x86_64::*;

    let high_bit = _mm256_set1_epi8(i8::MIN);
    let unsigned_threshold = _mm256_xor_si256(_mm256_set1_epi8(0x20), high_bit);
    let tab = _mm256_set1_epi8(b'\t' as i8);
    let line_feed = _mm256_set1_epi8(b'\n' as i8);
    let carriage_return = _mm256_set1_epi8(b'\r' as i8);
    let ef = _mm256_set1_epi8(0xEFu8 as i8);
    let less = _mm256_set1_epi8(b'<' as i8);
    let greater = _mm256_set1_epi8(b'>' as i8);
    let ampersand = _mm256_set1_epi8(b'&' as i8);
    let quote = _mm256_set1_epi8(b'"' as i8);
    let apostrophe = _mm256_set1_epi8(b'\'' as i8);
    let mut saw_ef = false;
    let (chunks, remainder) = bytes.as_chunks::<32>();
    for chunk in chunks {
        // SAFETY: AVX2 is checked by the caller and `chunk` is exactly 32
        // bytes. The unaligned load accepts every `&[u8]` address.
        let value = unsafe { _mm256_loadu_si256(chunk.as_ptr().cast()) };
        let below_xml_minimum =
            _mm256_cmpgt_epi8(unsigned_threshold, _mm256_xor_si256(value, high_bit));
        let allowed = _mm256_or_si256(
            _mm256_or_si256(
                _mm256_cmpeq_epi8(value, tab),
                _mm256_cmpeq_epi8(value, line_feed),
            ),
            _mm256_cmpeq_epi8(value, carriage_return),
        );
        let invalid_controls = _mm256_andnot_si256(allowed, below_xml_minimum);
        let specials = if ESCAPE {
            _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(value, less),
                        _mm256_cmpeq_epi8(value, greater),
                    ),
                    _mm256_cmpeq_epi8(value, ampersand),
                ),
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(value, quote),
                    _mm256_cmpeq_epi8(value, apostrophe),
                ),
            )
        } else {
            _mm256_setzero_si256()
        };
        if _mm256_movemask_epi8(_mm256_or_si256(invalid_controls, specials)) != 0 {
            return true;
        }
        saw_ef |= _mm256_movemask_epi8(_mm256_cmpeq_epi8(value, ef)) != 0;
    }
    for &byte in remainder {
        if (ESCAPE && matches!(byte, b'<' | b'>' | b'&' | b'"' | b'\''))
            || (byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r'))
        {
            return true;
        }
        saw_ef |= byte == 0xEF;
    }
    saw_ef && has_xml_sentinel(bytes)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn has_xml_sentinel(bytes: &[u8]) -> bool {
    bytes
        .windows(3)
        .any(|window| matches!(window, [0xEF, 0xBF, 0xBE | 0xBF]))
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

    #[cfg(feature = "render")]
    #[test]
    fn ascii_sanitization_fast_path_matches_the_general_rule() {
        for byte in 0..=0x7f {
            let bytes = [byte];
            let text = bytes.as_slice();
            let expected = match byte {
                b'<' | b'>' | b'&' | b'"' | b'\'' => true,
                b'\t' | b'\n' | b'\r' | 0x20..=0x7e => false,
                0..0x20 => true,
                _ => false,
            };
            assert_eq!(
                needs_xml_sanitization(std::str::from_utf8(text).unwrap()),
                expected
            );
        }

        for text in [
            "prefix\u{FFFE}suffix",
            "prefix\u{FFFF}suffix",
            "prefix\u{13}suffix",
            "prefix α suffix",
        ] {
            let expected = text
                .chars()
                .any(|c| matches!(c, '<' | '>' | '&' | '"' | '\'') || !is_xml_char(c));
            assert_eq!(needs_xml_sanitization(text), expected, "{text:?}");
        }
    }

    #[test]
    fn wide_scans_match_scalar_semantics_at_chunk_boundaries() {
        for offset in 0..64 {
            for marker in ['\u{FFFE}', '\u{FFFF}', '\u{13}', '<', 'α'] {
                let mut text = "x".repeat(128 + offset);
                text.push(marker);
                text.push('y');
                let expected_xml = text.chars().any(|c| !is_xml_char(c));
                assert_eq!(has_non_xml_char(&text), expected_xml, "{offset} {marker:?}");
                #[cfg(feature = "render")]
                {
                    let expected_sanitization = text
                        .chars()
                        .any(|c| matches!(c, '<' | '>' | '&' | '"' | '\'') || !is_xml_char(c));
                    assert_eq!(
                        needs_xml_sanitization(&text),
                        expected_sanitization,
                        "{offset} {marker:?}"
                    );
                }
            }
        }
    }
}
