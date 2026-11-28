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
    scan_xml::<false>(text.as_bytes())
}

/// Whether a UTF-8 string needs XML escaping or XML 1.0 character repair.
/// This combines the two byte scans used by SVG's streaming escaper.
#[inline]
#[cfg(feature = "render")]
pub(crate) fn needs_xml_sanitization(text: &str) -> bool {
    scan_xml::<true>(text.as_bytes())
}

#[inline]
fn scan_xml<const ESCAPE: bool>(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if bytes.len() >= 128 && std::is_x86_feature_detected!("avx2") {
        return unsafe { scan_xml_avx2::<ESCAPE>(bytes) };
    }
    #[cfg(target_arch = "aarch64")]
    if bytes.len() >= 64 && std::arch::is_aarch64_feature_detected!("neon") {
        return unsafe { scan_xml_neon::<ESCAPE>(bytes) };
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if bytes.len() >= 64 {
        return unsafe { scan_xml_wasm::<ESCAPE>(bytes) };
    }
    scan_xml_scalar::<ESCAPE>(bytes)
}

#[inline]
fn scan_xml_scalar<const ESCAPE: bool>(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().any(|(at, &byte)| match byte {
        b'<' | b'>' | b'&' | b'"' | b'\'' => ESCAPE,
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
    finish_xml_scan::<ESCAPE>(bytes, remainder, saw_ef)
}

// A forbidden EF BF BE/BF sequence can start in one vector and end in the next
// vector or in the tail. Only revisit the full input if a bulk block contained EF.
#[cfg(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    all(target_arch = "wasm32", target_feature = "simd128"),
))]
#[inline]
fn finish_xml_scan<const ESCAPE: bool>(bytes: &[u8], tail: &[u8], saw_ef: bool) -> bool {
    scan_xml_scalar::<ESCAPE>(tail)
        || (saw_ef
            && bytes
                .windows(3)
                .any(|w| matches!(w, [0xEF, 0xBF, 0xBE | 0xBF])))
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn scan_xml_neon<const ESCAPE: bool>(bytes: &[u8]) -> bool {
    use std::arch::aarch64::*;

    let controls = vdupq_n_u8(0x20);
    let tab = vdupq_n_u8(b'\t');
    let line_feed = vdupq_n_u8(b'\n');
    let carriage_return = vdupq_n_u8(b'\r');
    let ef = vdupq_n_u8(0xEF);
    let less = vdupq_n_u8(b'<');
    let greater = vdupq_n_u8(b'>');
    let ampersand = vdupq_n_u8(b'&');
    let quote = vdupq_n_u8(b'"');
    let apostrophe = vdupq_n_u8(b'\'');
    let mut saw_ef = false;
    let (chunks, remainder) = bytes.as_chunks::<16>();
    for chunk in chunks {
        // Each chunk contains 16 readable bytes; NEON permits unaligned loads.
        let value = unsafe { vld1q_u8(chunk.as_ptr()) };
        let below = vcltq_u8(value, controls);
        let allowed = vorrq_u8(
            vorrq_u8(vceqq_u8(value, tab), vceqq_u8(value, line_feed)),
            vceqq_u8(value, carriage_return),
        );
        let mut invalid = vbicq_u8(below, allowed);
        if ESCAPE {
            let special = vorrq_u8(
                vorrq_u8(
                    vorrq_u8(vceqq_u8(value, less), vceqq_u8(value, greater)),
                    vceqq_u8(value, ampersand),
                ),
                vorrq_u8(vceqq_u8(value, quote), vceqq_u8(value, apostrophe)),
            );
            invalid = vorrq_u8(invalid, special);
        }
        if vmaxvq_u8(invalid) != 0 {
            return true;
        }
        saw_ef |= vmaxvq_u8(vceqq_u8(value, ef)) != 0;
    }
    finish_xml_scan::<ESCAPE>(bytes, remainder, saw_ef)
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[target_feature(enable = "simd128")]
unsafe fn scan_xml_wasm<const ESCAPE: bool>(bytes: &[u8]) -> bool {
    use std::arch::wasm32::*;

    let controls = u8x16_splat(0x20);
    let tab = u8x16_splat(b'\t');
    let line_feed = u8x16_splat(b'\n');
    let carriage_return = u8x16_splat(b'\r');
    let ef = u8x16_splat(0xEF);
    let mut saw_ef = false;
    let (chunks, remainder) = bytes.as_chunks::<16>();
    for chunk in chunks {
        // v128.load accepts unaligned addresses and the chunk is 16 bytes.
        let value = unsafe { v128_load(chunk.as_ptr().cast()) };
        let below = u8x16_lt(value, controls);
        let allowed = v128_or(
            v128_or(u8x16_eq(value, tab), u8x16_eq(value, line_feed)),
            u8x16_eq(value, carriage_return),
        );
        let mut invalid = v128_and(below, v128_not(allowed));
        if ESCAPE {
            let special = v128_or(
                v128_or(
                    v128_or(
                        u8x16_eq(value, u8x16_splat(b'<')),
                        u8x16_eq(value, u8x16_splat(b'>')),
                    ),
                    u8x16_eq(value, u8x16_splat(b'&')),
                ),
                v128_or(
                    u8x16_eq(value, u8x16_splat(b'"')),
                    u8x16_eq(value, u8x16_splat(b'\'')),
                ),
            );
            invalid = v128_or(invalid, special);
        }
        if v128_any_true(invalid) {
            return true;
        }
        saw_ef |= v128_any_true(u8x16_eq(value, ef));
    }
    finish_xml_scan::<ESCAPE>(bytes, remainder, saw_ef)
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
        for len in [15, 16, 17, 63, 64, 65, 127, 128, 129, 256] {
            for start in 0..16 {
                for marker in [
                    '\u{FFFE}', '\u{FFFF}', 'Ａ', '�', '\u{13}', '<', '>', '&', '"', '\'', '\r',
                    '\n', '\t', 'α',
                ] {
                    let mut buf = "x".repeat(len + start);
                    buf.push(marker);
                    buf.push_str("end");
                    for text in [&buf[start..], &buf[start..start + len]] {
                        assert_eq!(
                            has_non_xml_char(text),
                            text.chars().any(|c| !is_xml_char(c))
                        );
                        #[cfg(feature = "render")]
                        assert_eq!(
                            needs_xml_sanitization(text),
                            text.chars().any(|c| {
                                matches!(c, '<' | '>' | '&' | '"' | '\'') || !is_xml_char(c)
                            })
                        );
                    }
                }
            }
        }
    }
}
