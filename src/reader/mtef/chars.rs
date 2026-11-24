//! Character-level helpers for the MTEF reader: typeface (style) values,
//! font encodings, MathType private-use code points and symbol classes.

use crate::ast::{Rational, SymbolKind};

// Typeface values (MTEFv3.md / MTEFv5.md "Typeface values"). A positive value
// names one of MathType's styles; a negative value names an explicit font.
pub(super) const FN_TEXT: i32 = 1;
pub(super) const FN_FUNCTION: i32 = 2;
pub(super) const FN_LCGREEK: i32 = 4;
pub(super) const FN_UCGREEK: i32 = 5;
pub(super) const FN_SYMBOL: i32 = 6;
pub(super) const FN_VECTOR: i32 = 7;
pub(super) const FN_NUMBER: i32 = 8;
pub(super) const FN_MTEXTRA: i32 = 11;
pub(super) const FN_TEXT_FE: i32 = 12;
pub(super) const FN_MARKER: i32 = 23;
pub(super) const FN_SPACE: i32 = 24;

/// Font encodings a character code can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Encoding {
    /// Unicode, or MathType's MTCode superset of it.
    Unicode,
    /// The Adobe Symbol font encoding.
    Symbol,
    /// The MT Extra font encoding.
    MtExtra,
    /// Any other (text) encoding; 8-bit codes are read as Windows Latin-1.
    Other,
}

impl Encoding {
    /// The encoding implied by a font name, as far as it matters for decoding.
    pub(super) fn from_font_name(name: &[u8]) -> Self {
        if name.eq_ignore_ascii_case(b"Symbol") {
            Encoding::Symbol
        } else if name.eq_ignore_ascii_case(b"MT Extra") {
            Encoding::MtExtra
        } else {
            Encoding::Other
        }
    }

    /// The encoding of a predefined or written ENCODING_DEF (MTEFv5.md
    /// "ENCODING_DEF records": 1 MTCode, 2 Unknown, 3 Symbol, 4 MTExtra).
    pub(super) fn from_encoding_name(name: &[u8]) -> Self {
        if name.eq_ignore_ascii_case(b"MTCode") {
            Encoding::Unicode
        } else if name.eq_ignore_ascii_case(b"Symbol") {
            Encoding::Symbol
        } else if name.eq_ignore_ascii_case(b"MTExtra") {
            Encoding::MtExtra
        } else {
            Encoding::Other
        }
    }
}

/// Decode a font-position code in the given encoding. Returns `None` when the
/// code has no known Unicode equivalent.
pub(super) fn decode_font_position(code: u16, encoding: Encoding) -> Option<char> {
    match encoding {
        Encoding::Unicode => char::from_u32(u32::from(code)),
        Encoding::Symbol => u8::try_from(code).ok().and_then(symbol_font_char),
        Encoding::MtExtra => u8::try_from(code).ok().and_then(mt_extra_char),
        Encoding::Other => char::from_u32(u32::from(code)),
    }
}

/// The Adobe Symbol font encoding, mapped to Unicode.
pub(crate) fn symbol_font_char(code: u8) -> Option<char> {
    let c = match code {
        0x20 => ' ',
        0x21 => '!',
        0x22 => '\u{2200}',
        0x23 => '#',
        0x24 => '\u{2203}',
        0x25 => '%',
        0x26 => '&',
        0x27 => '\u{220B}',
        0x28 => '(',
        0x29 => ')',
        0x2A => '\u{2217}',
        0x2B => '+',
        0x2C => ',',
        0x2D => '\u{2212}',
        0x2E => '.',
        0x2F => '/',
        0x30..=0x39 => char::from(code),
        0x3A => ':',
        0x3B => ';',
        0x3C => '<',
        0x3D => '=',
        0x3E => '>',
        0x3F => '?',
        0x40 => '\u{2245}',
        0x41 => '\u{0391}',
        0x42 => '\u{0392}',
        0x43 => '\u{03A7}',
        0x44 => '\u{0394}',
        0x45 => '\u{0395}',
        0x46 => '\u{03A6}',
        0x47 => '\u{0393}',
        0x48 => '\u{0397}',
        0x49 => '\u{0399}',
        0x4A => '\u{03D1}',
        0x4B => '\u{039A}',
        0x4C => '\u{039B}',
        0x4D => '\u{039C}',
        0x4E => '\u{039D}',
        0x4F => '\u{039F}',
        0x50 => '\u{03A0}',
        0x51 => '\u{0398}',
        0x52 => '\u{03A1}',
        0x53 => '\u{03A3}',
        0x54 => '\u{03A4}',
        0x55 => '\u{03A5}',
        0x56 => '\u{03C2}',
        0x57 => '\u{03A9}',
        0x58 => '\u{039E}',
        0x59 => '\u{03A8}',
        0x5A => '\u{0396}',
        0x5B => '[',
        0x5C => '\u{2234}',
        0x5D => ']',
        0x5E => '\u{22A5}',
        0x5F => '_',
        0x60 => '\u{203E}',
        0x61 => '\u{03B1}',
        0x62 => '\u{03B2}',
        0x63 => '\u{03C7}',
        0x64 => '\u{03B4}',
        0x65 => '\u{03B5}',
        0x66 => '\u{03C6}',
        0x67 => '\u{03B3}',
        0x68 => '\u{03B7}',
        0x69 => '\u{03B9}',
        0x6A => '\u{03D5}',
        0x6B => '\u{03BA}',
        0x6C => '\u{03BB}',
        0x6D => '\u{03BC}',
        0x6E => '\u{03BD}',
        0x6F => '\u{03BF}',
        0x70 => '\u{03C0}',
        0x71 => '\u{03B8}',
        0x72 => '\u{03C1}',
        0x73 => '\u{03C3}',
        0x74 => '\u{03C4}',
        0x75 => '\u{03C5}',
        0x76 => '\u{03D6}',
        0x77 => '\u{03C9}',
        0x78 => '\u{03BE}',
        0x79 => '\u{03C8}',
        0x7A => '\u{03B6}',
        0x7B => '{',
        0x7C => '|',
        0x7D => '}',
        0x7E => '\u{223C}',
        0xA0 => '\u{20AC}',
        0xA1 => '\u{03D2}',
        0xA2 => '\u{2032}',
        0xA3 => '\u{2264}',
        0xA4 => '\u{2044}',
        0xA5 => '\u{221E}',
        0xA6 => '\u{0192}',
        0xA7 => '\u{2663}',
        0xA8 => '\u{2666}',
        0xA9 => '\u{2665}',
        0xAA => '\u{2660}',
        0xAB => '\u{2194}',
        0xAC => '\u{2190}',
        0xAD => '\u{2191}',
        0xAE => '\u{2192}',
        0xAF => '\u{2193}',
        0xB0 => '\u{00B0}',
        0xB1 => '\u{00B1}',
        0xB2 => '\u{2033}',
        0xB3 => '\u{2265}',
        0xB4 => '\u{00D7}',
        0xB5 => '\u{221D}',
        0xB6 => '\u{2202}',
        0xB7 => '\u{2022}',
        0xB8 => '\u{00F7}',
        0xB9 => '\u{2260}',
        0xBA => '\u{2261}',
        0xBB => '\u{2248}',
        0xBC => '\u{2026}',
        0xBD => '\u{23D0}',
        0xBE => '\u{23AF}',
        0xBF => '\u{21B5}',
        0xC0 => '\u{2135}',
        0xC1 => '\u{2111}',
        0xC2 => '\u{211C}',
        0xC3 => '\u{2118}',
        0xC4 => '\u{2297}',
        0xC5 => '\u{2295}',
        0xC6 => '\u{2205}',
        0xC7 => '\u{2229}',
        0xC8 => '\u{222A}',
        0xC9 => '\u{2283}',
        0xCA => '\u{2287}',
        0xCB => '\u{2284}',
        0xCC => '\u{2282}',
        0xCD => '\u{2286}',
        0xCE => '\u{2208}',
        0xCF => '\u{2209}',
        0xD0 => '\u{2220}',
        0xD1 => '\u{2207}',
        0xD2 | 0xE2 => '\u{00AE}',
        0xD3 | 0xE3 => '\u{00A9}',
        0xD4 | 0xE4 => '\u{2122}',
        0xD5 => '\u{220F}',
        0xD6 => '\u{221A}',
        0xD7 => '\u{22C5}',
        0xD8 => '\u{00AC}',
        0xD9 => '\u{2227}',
        0xDA => '\u{2228}',
        0xDB => '\u{21D4}',
        0xDC => '\u{21D0}',
        0xDD => '\u{21D1}',
        0xDE => '\u{21D2}',
        0xDF => '\u{21D3}',
        0xE0 => '\u{25CA}',
        0xE1 => '\u{27E8}',
        0xE5 => '\u{2211}',
        0xE6 => '\u{239B}',
        0xE7 => '\u{239C}',
        0xE8 => '\u{239D}',
        0xE9 => '\u{23A1}',
        0xEA => '\u{23A2}',
        0xEB => '\u{23A3}',
        0xEC => '\u{23A7}',
        0xED => '\u{23A8}',
        0xEE => '\u{23A9}',
        0xEF => '\u{23AA}',
        0xF1 => '\u{27E9}',
        0xF2 => '\u{222B}',
        0xF3 => '\u{2320}',
        0xF4 => '\u{23AE}',
        0xF5 => '\u{2321}',
        0xF6 => '\u{239E}',
        0xF7 => '\u{239F}',
        0xF8 => '\u{23A0}',
        0xF9 => '\u{23A4}',
        0xFA => '\u{23A5}',
        0xFB => '\u{23A6}',
        0xFC => '\u{23AB}',
        0xFD => '\u{23AC}',
        0xFE => '\u{23AD}',
        _ => return None,
    };
    Some(c)
}

/// MT Extra font positions whose MTCode is confirmed by equations written by
/// MathType 5 and 6, which store both the MTCode value and the 8-bit position
/// of each MT Extra character (the upstream fixtures in `tests/fixtures/mtef/`
/// and the equations they were taken from, and the document fixtures).
/// Positions that no such file confirms are not mapped; without an MTCode
/// value they are reported as having no known equivalent. Private-use MTCode
/// values are kept as MathType defines them (U+E98F becomes U+00B7 later).
pub(crate) const MT_EXTRA_CONFIRMED: [(u8, char); 14] = [
    (0x43, '\u{2210}'), // coproduct
    (0x44, '\u{019B}'), // lambda with stroke
    (0x49, '\u{2229}'), // intersection
    (0x4B, '\u{2026}'), // horizontal ellipsis
    (0x4C, '\u{22EF}'), // midline horizontal ellipsis
    (0x4D, '\u{22EE}'), // vertical ellipsis
    (0x4F, '\u{22F1}'), // down right diagonal ellipsis
    (0x55, '\u{222A}'), // union
    (0x67, '\u{E98F}'), // medium dot operator
    (0x69, '\u{EE00}'),
    (0x6A, '\u{EE01}'),
    (0xD1, '\u{EE11}'), // integral loop glyphs
    (0xD2, '\u{EE12}'),
    (0xD3, '\u{EE13}'),
];

/// The MT Extra font encoding, as far as real files confirm it.
fn mt_extra_char(code: u8) -> Option<char> {
    MT_EXTRA_CONFIRMED
        .iter()
        .find(|&&(position, _)| position == code)
        .map(|&(_, ch)| ch)
}

/// Width, in em, of a MathType space character (typeface `fnSPACE` or the
/// MTCode space block). Equation Editor 3 writes the same spaces in the
/// `0xEB00` block.
pub(super) fn space_width(code: u32) -> Option<Rational> {
    let low = match code {
        0xEF00..=0xEF0F => code - 0xEF00,
        0xEB00..=0xEB0F => code - 0xEB00,
        0x0000..=0x000F => code,
        _ => return None,
    };
    Some(match low {
        // Zero space; 1 point space (about a twelfth of an em at 12 pt).
        0x1 => Rational::zero(),
        0x8 => Rational::new(1, 12),
        // Thin, medium, thick space (TeX's \, \: \;).
        0x2 => Rational::new(3, 18),
        0x3 => Rational::new(4, 18),
        0x4 => Rational::new(5, 18),
        // Em space and 2 em space.
        0x5 => Rational::new(1, 1),
        0x6 => Rational::new(2, 1),
        _ => return None,
    })
}

/// Whether `code` is a MathType marker (alignment or insertion mark) that has
/// no visible content.
pub(super) fn is_marker(code: u32) -> bool {
    matches!(code, 0xEF00 | 0xEB00)
}

/// Replace MathType private-use code points that have a standard Unicode
/// equivalent, and deprecated angle brackets with the mathematical ones.
pub(super) fn normalize_code_point(ch: char) -> char {
    match ch {
        '\u{E98F}' => '\u{00B7}',
        '\u{2329}' => '\u{27E8}',
        '\u{232A}' => '\u{27E9}',
        _ => ch,
    }
}

/// The mathematical character an ASCII character stands for outside the text
/// styles: the hyphen is the minus sign, the tilde the similarity relation,
/// the apostrophe a prime and the backslash the set difference. The double
/// quote is left alone: authors type it both as a double prime and as a
/// quotation mark.
pub(super) fn math_ascii(ch: char) -> char {
    match ch {
        '-' => '\u{2212}',
        '~' => '\u{223C}',
        '\'' => '\u{2032}',
        '\\' => '\u{2216}',
        _ => ch,
    }
}

/// Whether `ch` is a prime mark, which follows its base as a superscript.
pub(super) fn is_prime(ch: char) -> bool {
    matches!(ch, '\u{2032}' | '\u{2033}' | '\u{2034}' | '\u{2057}')
}

/// Whether `ch` lies in the Unicode private use area (MathType's own MTCode
/// characters live there).
pub(super) fn is_private_use(ch: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&ch)
}

/// The math class of a symbol character.
pub(super) fn symbol_kind(ch: char) -> SymbolKind {
    match ch {
        '(' | '[' | '{' | '\u{27E8}' | '\u{2308}' | '\u{230A}' | '\u{27E6}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' | '\u{2309}' | '\u{230B}' | '\u{27E7}' => SymbolKind::Close,
        '|' | '\u{2016}' | '\u{2223}' => SymbolKind::Fence,
        ',' | ';' | '\u{3001}' | '\u{FF0C}' | '\u{FF1B}' => SymbolKind::Punctuation,
        '+' | '-' | '\u{2212}' | '*' | '/' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | ':' => SymbolKind::Relation,
        '\u{2211}' | '\u{220F}' | '\u{2210}' | '\u{222B}' | '\u{222C}' | '\u{222D}'
        | '\u{222E}' | '\u{22C3}' | '\u{22C2}' => SymbolKind::LargeOp,
        _ => crate::unicode::symbols::symbol_kind(ch),
    }
}

/// The negated form of a relation, used for the `embNOT` embellishment.
pub(crate) fn negated(ch: char) -> Option<char> {
    Some(match ch {
        '=' => '\u{2260}',
        '<' => '\u{226E}',
        '>' => '\u{226F}',
        '\u{2264}' => '\u{2270}',
        '\u{2265}' => '\u{2271}',
        '\u{2208}' => '\u{2209}',
        '\u{220B}' => '\u{220C}',
        '\u{2282}' => '\u{2284}',
        '\u{2283}' => '\u{2285}',
        '\u{2286}' => '\u{2288}',
        '\u{2287}' => '\u{2289}',
        '\u{2261}' => '\u{2262}',
        '\u{223C}' => '\u{2241}',
        '\u{2248}' => '\u{2249}',
        '\u{2245}' => '\u{2247}',
        '\u{2203}' => '\u{2204}',
        '\u{2225}' => '\u{2226}',
        '\u{2223}' => '\u{2224}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_encoding_covers_greek_and_operators() {
        assert_eq!(symbol_font_char(b'a'), Some('\u{03B1}'));
        assert_eq!(symbol_font_char(b'W'), Some('\u{03A9}'));
        assert_eq!(symbol_font_char(0xB4), Some('\u{00D7}'));
        assert_eq!(symbol_font_char(0xD7), Some('\u{22C5}'));
        assert_eq!(symbol_font_char(0xC5), Some('\u{2295}'));
        assert_eq!(symbol_font_char(0x7F), None);
    }

    #[test]
    fn mt_extra_positions_match_real_files() {
        // Pairs of (8-bit position, MTCode) found together in CHAR records
        // written by MathType 5 and 6.
        for (position, ch) in [
            (0x43, '\u{2210}'),
            (0x44, '\u{019B}'),
            (0x49, '\u{2229}'),
            (0x4B, '\u{2026}'),
            (0x4C, '\u{22EF}'),
            (0x4D, '\u{22EE}'),
            (0x4F, '\u{22F1}'),
            (0x55, '\u{222A}'),
            (0x67, '\u{E98F}'),
            (0x69, '\u{EE00}'),
            (0x6A, '\u{EE01}'),
            (0xD1, '\u{EE11}'),
            (0xD2, '\u{EE12}'),
            (0xD3, '\u{EE13}'),
        ] {
            assert_eq!(
                decode_font_position(position, Encoding::MtExtra),
                Some(ch),
                "{position:#04x}"
            );
        }
        // Unconfirmed positions are not guessed.
        assert_eq!(decode_font_position(0x4E, Encoding::MtExtra), None);
    }

    #[test]
    fn space_codes_map_to_em_widths() {
        assert_eq!(space_width(0xEF02), Some(Rational::new(3, 18)));
        assert_eq!(space_width(0xEB04), Some(Rational::new(5, 18)));
        assert_eq!(space_width(0xEF05), Some(Rational::new(1, 1)));
        assert_eq!(space_width(0xEF07), None);
    }
}
