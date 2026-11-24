//! MTEF 2 (Equation Editor 1.x/2.x, MathType 3.0) and MTEF 4 (MathType 3.5).
//!
//! Both versions lay their records out exactly as MTEF 3 does, except that a
//! CHAR record's character value is a single byte: a position in the font of
//! the character's typeface, not Unicode. `docs/mtef-legacy.md` describes
//! the format and which of its rules were established from real files.
//!
//! The decoder handles them with its MTEF 3 code; this module decodes the
//! 8-bit character values and holds the few other rules specific to them.

use super::Decoder;
use crate::error::Result;
use crate::reader::mtef::chars::{self, Encoding};
use crate::reader::mtef::info::MtefVersion;

impl Decoder<'_> {
    /// The typeface and character of an 8-bit MTEF 2 or 4 character value.
    /// Symbol-font styles (Greek and Symbol) use the Symbol font encoding,
    /// MT Extra its own; an explicit font uses the encoding of its FONT
    /// record's name; all other styles use `text_char`. A space code is an
    /// fnSPACE character whatever typeface it is written in.
    pub(super) fn legacy_char(
        &mut self,
        start: usize,
        typeface: i32,
        code: u8,
    ) -> Result<(i32, char)> {
        // Only these space codes occur in real files; see `chars::space_width`.
        if matches!(code, 1 | 2 | 4 | 5 | 8) && typeface != chars::FN_MARKER {
            let ch = char::from_u32(0xEF00 + u32::from(code)).unwrap_or(' ');
            return Ok((chars::FN_SPACE, ch));
        }
        // Alignment marks: only 1 and 9 occur.
        if typeface == chars::FN_MARKER && matches!(code, 1 | 9) {
            return Ok((typeface, ' '));
        }
        if matches!(typeface, chars::FN_SPACE | chars::FN_MARKER) || code < 0x20 {
            self.recover(start, format!("unknown space character {code:#04x}"))?;
            // A thin space, as for unknown MTEF 3 and 5 spaces.
            return Ok((chars::FN_SPACE, '\u{EF02}'));
        }
        let ch = self.legacy_glyph(start, typeface, code)?;
        Ok((typeface, ch))
    }

    fn legacy_glyph(&mut self, start: usize, typeface: i32, code: u8) -> Result<char> {
        if typeface == FN_EXPAND && code < 0x80 {
            return match expanding_char(code) {
                Some(ch) => Ok(ch),
                None => {
                    self.recover(
                        start,
                        format!("unknown expanding-delimiter character {code:#04x}"),
                    )?;
                    Ok(char::REPLACEMENT_CHARACTER)
                },
            };
        }
        let encoding = self.legacy_encoding(typeface);
        let decoded = match encoding {
            Encoding::Symbol => chars::decode_font_position(u16::from(code), encoding),
            Encoding::MtExtra => chars::decode_font_position(u16::from(code), encoding)
                .or_else(|| legacy_mt_extra(code)),
            Encoding::Unicode | Encoding::Other => self.text_char(code),
        };
        match decoded {
            Some(ch) => Ok(ch),
            None => {
                self.recover(
                    start,
                    format!(
                        "font position {code:#04x} has no known Unicode equivalent in its font encoding"
                    ),
                )?;
                Ok(char::REPLACEMENT_CHARACTER)
            },
        }
    }

    /// The encoding of a typeface's characters.
    fn legacy_encoding(&self, typeface: i32) -> Encoding {
        match typeface {
            chars::FN_LCGREEK | chars::FN_UCGREEK | chars::FN_SYMBOL | FN_EXPAND => {
                Encoding::Symbol
            },
            chars::FN_MTEXTRA => Encoding::MtExtra,
            t if t < 0 => usize::try_from(-t)
                .ok()
                .and_then(|i| self.v3_fonts.get(i).copied().flatten())
                .map_or(Encoding::Other, |(encoding, _)| encoding),
            _ => Encoding::Other,
        }
    }

    /// A character of a text font. Positions above ASCII are only known where
    /// the pictures stored with real equations show the glyph drawn for them:
    /// some are Adobe StandardEncoding positions, others Mac OS Roman.
    fn text_char(&self, code: u8) -> Option<char> {
        Some(match code {
            0x20..=0x7E => char::from(code),
            // The prime and the apostrophe, which `mark_primes` tells apart.
            0xA9 => '\'',
            0xB2 => '\u{2020}',
            _ if !self.mac => return None,
            0x8E => '\u{00E9}',
            0xAA => '\u{201C}',
            0xB1 => '\u{2013}',
            0xB7 => '\u{2022}',
            0xBA => '\u{201D}',
            0xCA => '\u{00A0}',
            0xFB => '\u{02DA}',
            _ => return None,
        })
    }

    /// Whether `len` bytes after the equation, within the size an OLE header
    /// declares, can be slack of an MTEF 2 or 4 writer.
    pub(super) fn legacy_slack(&self, len: usize) -> bool {
        self.legacy && len <= MAX_SLACK
    }

    fn v2(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|info| info.version == MtefVersion::V2)
    }

    /// Report an MTEF 2 template selector no real file has. No vendor table
    /// covers MTEF 2, so its MTEF 3 meaning is unconfirmed there; Lenient mode
    /// still reads it that way. (MTEF 4 has a vendor table.)
    pub(super) fn check_v2_selector(&mut self, start: usize, selector: u8) -> Result<()> {
        let confirmed = matches!(
            selector,
            0..=5 | 7 | 13..=17 | 19 | 21 | 22 | 24..=26 | 28..=32 | 37 | 39 | 41..=45 | 47
        );
        if self.v2() && !confirmed {
            self.recover(
                start,
                format!(
                    "template selector {selector} is unconfirmed in MTEF 2; \
                     it was read as in MTEF 3"
                ),
            )?;
        }
        Ok(())
    }

    /// As `check_v2_selector`, for an embellishment type.
    pub(super) fn check_v2_embellishment(&mut self, start: usize, kind: u8) -> Result<()> {
        // Types outside 2 to 20 are reported as unknown in every version.
        if self.v2() && matches!(kind, 4 | 7 | 18 | 20) {
            self.recover(
                start,
                format!(
                    "embellishment type {kind} is unconfirmed in MTEF 2; it was read as in MTEF 3"
                ),
            )?;
        }
        Ok(())
    }
}

/// The fnEXPAND style: the characters of fence, arrow and brace templates.
const FN_EXPAND: i32 = 22;

/// An fnEXPAND character below 0x80. Brackets and parentheses are themselves;
/// the other delimiters are letters, identified by the templates they appear
/// in (`docs/mtef-legacy.md`). Upper positions are Symbol font pieces.
fn expanding_char(code: u8) -> Option<char> {
    Some(match code {
        b'(' | b')' | b'[' | b']' | b'{' | b'}' => char::from(code),
        b'a' | b'b' => '|',
        b'c' | b'd' => '\u{2016}',
        b'e' => '\u{27E8}',
        b'f' => '\u{27E9}',
        b'B' => '\u{23DF}',
        b'C' => '\u{2192}',
        _ => return None,
    })
}

/// The longest slack after the END that closes the equation. Equation Editor
/// 2 for Windows leaves up to 31 bytes of stale memory (fragments of Windows
/// metafile records, font names, fill bytes) in the size its `Equation
/// Native` header declares.
const MAX_SLACK: usize = 31;

/// MT Extra positions that MTEF 5 equations pair with an MTCode value and that
/// the table shared with MTEF 3 and 5 does not hold.
fn legacy_mt_extra(code: u8) -> Option<char> {
    Some(match code {
        0x68 => '\u{210F}',
        0x6C => '\u{2113}',
        0x6D => '\u{2213}',
        0x6F => '\u{2218}',
        0x70 => '\u{227A}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::error::{Config, DiagnosticKind, Strictness};
    use crate::{MtefProduct, MtefVersion, MtefWriteOptions};

    /// An MTEF 2 equation (Equation Editor 1.0 on `platform`): FULL, one
    /// LINE holding `content`, final END.
    fn v2(platform: u8, content: &[u8]) -> Vec<u8> {
        let mut data = vec![2, platform, 1, 10, 0, 0x0A, 0x01];
        data.extend_from_slice(content);
        data.extend_from_slice(&[0, 0]);
        data
    }

    /// An 8-bit CHAR record.
    fn c(typeface: u8, code: u8) -> [u8; 3] {
        [0x02, 128 + typeface, code]
    }

    fn latex(data: &[u8]) -> String {
        crate::from_mtef_bytes(data).unwrap().to_latex().unwrap()
    }

    #[test]
    fn text_positions_above_ascii() {
        let mac: Vec<u8> = [0x8E, 0xAA, 0xB1, 0xB2, 0xB7, 0xBA, 0xCA, 0xFB]
            .into_iter()
            .flat_map(|code| c(1, code))
            .collect();
        assert_eq!(latex(&v2(0, &mac)), "\\text{é“–†•”\u{A0}˚}");
        assert_eq!(latex(&v2(1, &c(1, 0xB2))), "\\text{†}");
        // Not confirmed for Windows.
        assert!(crate::from_mtef_bytes(&v2(1, &c(1, 0x8E))).is_err());
    }

    #[test]
    fn characters_are_single_bytes_in_their_typeface_encoding() {
        let mut content = Vec::new();
        for record in [
            c(3, b'x'),
            c(6, b'='),
            c(4, b'a'),  // Symbol: alpha
            c(11, 0x6C), // MT Extra: script l
            c(24, 2),    // thin space
            c(1, 0x8E),  // Mac OS Roman: e acute
        ] {
            content.extend_from_slice(&record);
        }
        assert_eq!(latex(&v2(0, &content)), "x= \\alpha\\ell\\,\\text{é}");
        let info = crate::from_mtef_bytes(&v2(1, &c(3, b'x'))).unwrap();
        let info = info.mtef_info().unwrap();
        assert_eq!(info.version, MtefVersion::V2);
        assert_eq!(info.product, MtefProduct::EquationEditor);
    }

    #[test]
    fn text_encoding_0xa9_is_a_prime_or_an_apostrophe() {
        let math: Vec<u8> = [c(3, b'x'), c(2, 0xA9), c(2, 0xA9), c(6, b'+')].concat();
        assert_eq!(latex(&v2(0, &math)), r"x^{\prime\prime}+");
        let text: Vec<u8> = [c(1, b'a'), c(1, 0xA9), c(1, b's')].concat();
        assert_eq!(latex(&v2(1, &text)), r"\text{a's}");
    }

    #[test]
    fn unknown_codes_are_refused_in_strict_mode() {
        for (typeface, code) in [
            (24, 3),
            (24, 0x20),
            (1, 0x0B),
            (1, 0x81),
            (11, 0x3C),
            (1, 0x7F),
            (22, b'A'),
            (4, 0x7F),
        ] {
            assert!(crate::from_mtef_bytes(&v2(1, &c(typeface, code))).is_err());
            let lenient = Config::default().with_strictness(Strictness::Lenient);
            let result = crate::from_mtef_bytes_with(&v2(1, &c(typeface, code)), &lenient).unwrap();
            assert!(
                result
                    .diagnostics()
                    .iter()
                    .any(|d| d.kind == DiagnosticKind::Warning)
            );
        }
        // A space code in the text style is a space, not text.
        assert_eq!(latex(&v2(1, &c(1, 5))), r"\quad ");
    }

    #[test]
    fn embellishments_have_no_trailing_zero_byte() {
        // CHAR with xfEMBELL, then EMBELL hat, END of the embellishment list.
        let content = [0x22, 128 + 3, b'x', 0x06, 9, 0x00, 0x02, 128 + 3, b'y'];
        assert_eq!(latex(&v2(1, &content)), "\\hat{x}y");
    }

    #[test]
    fn short_slack_inside_the_declared_size_is_noted() {
        // 12 bytes of equation.
        let mtef = v2(1, &c(3, b'x'));
        assert_eq!(mtef.len() % 4, 0);
        let ole = |slack: usize| {
            let mut data = vec![28, 0, 0, 0, 2, 0, 0, 0];
            data.extend(u32::try_from(mtef.len() + slack).unwrap().to_le_bytes());
            data.resize(28, 0);
            data.extend_from_slice(&mtef);
            data.resize(data.len() + slack, 0xFF);
            data
        };
        let result = crate::from_mtef_bytes(&ole(28)).unwrap();
        assert_eq!(result.to_latex().unwrap(), "x");
        assert!(
            result
                .diagnostics()
                .iter()
                .all(|d| d.kind == DiagnosticKind::Info)
        );
        // Not a whole number of 32-bit words, too long, or no OLE header.
        assert!(crate::from_mtef_bytes(&ole(27)).is_err());
        assert!(crate::from_mtef_bytes(&ole(32)).is_err());
        assert!(crate::from_mtef_bytes(&ole(4)[28..]).is_err());
        // MTEF 3 keeps its own, shorter bound.
        let mut v3 = ole(8);
        v3[28] = 3;
        assert!(crate::from_mtef_bytes(&v3).is_err());
    }

    #[test]
    fn unconfirmed_mtef_2_templates_and_embellishments_are_reported() {
        let template = |selector: u8| {
            let mut content = vec![0x03, selector, 0, 0, 0x01];
            content.extend_from_slice(&c(3, b'x'));
            content.extend_from_slice(&[0, 0]);
            content
        };
        let embellished = |kind: u8| vec![0x22, 128 + 3, b'x', 0x06, kind, 0];
        let lenient = Config::default().with_strictness(Strictness::Lenient);
        for (content, expected) in [
            (template(6), r"\left\lfloor x \right\rfloor"),
            (embellished(20), r"\breve{x}"),
        ] {
            let data = v2(1, &content);
            assert!(crate::from_mtef_bytes(&data).is_err());
            let result = crate::from_mtef_bytes_with(&data, &lenient).unwrap();
            assert!(
                result
                    .diagnostics()
                    .iter()
                    .any(|d| d.message.contains("unconfirmed"))
            );
            assert_eq!(result.to_latex().unwrap(), expected);
            // The MTEF 4 specification defines both.
            let mut v4 = data.clone();
            v4[0] = 4;
            assert_eq!(latex(&v4), expected);
        }
    }

    #[test]
    fn unknown_mt_extra_positions_are_refused_in_strict_mode() {
        let data = v2(1, &c(11, 0x7A));
        assert!(crate::from_mtef_bytes(&data).is_err());
        let lenient = Config::default().with_strictness(Strictness::Lenient);
        let result = crate::from_mtef_bytes_with(&data, &lenient).unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind == DiagnosticKind::Warning)
        );
    }

    #[test]
    fn mtef_4_is_read_and_written_as_mtef_5() {
        let data = [4, 0, 0, 3, 7, 0x0A, 0x01, 0x02, 128 + 3, b'k', 0, 0];
        let result = crate::from_mtef_bytes(&data).unwrap();
        let info = result.mtef_info().unwrap();
        assert_eq!(info.version, MtefVersion::V4);
        let options = MtefWriteOptions::from_info(info);
        assert_eq!(options.version, MtefVersion::V5);
        let written = result.to_mtef_bytes(&options).unwrap();
        assert_eq!(written[0], 5);
        assert_eq!(
            crate::from_mtef_bytes(&written)
                .unwrap()
                .to_latex()
                .unwrap(),
            "k"
        );
        // An Equation Editor equation becomes an Equation Editor 3 one.
        let v2 = crate::from_mtef_bytes(&v2(1, &c(3, b'k'))).unwrap();
        let options = MtefWriteOptions::from_info(v2.mtef_info().unwrap());
        assert_eq!(options.version, MtefVersion::V3);
        assert_eq!(&v2.to_mtef_bytes(&options).unwrap()[..5], &[3, 1, 1, 3, 10]);
    }
}
