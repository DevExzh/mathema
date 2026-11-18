//! Character-level tables for the MTEF writer: typeface (style) values, font
//! positions in the Symbol and MT Extra encodings, embellishment types and
//! the characters MathType and Equation Editor store after template slots.

use std::sync::OnceLock;

use smallvec::SmallVec;

use crate::ast::AccentPosition;
use crate::reader::mtef::chars::{MT_EXTRA_CONFIRMED, symbol_font_char};

// Typeface values (MTEFv3.md / MTEFv5.md "Typeface values").
pub(super) const FN_TEXT: i32 = 1;
pub(super) const FN_FUNCTION: i32 = 2;
pub(super) const FN_VARIABLE: i32 = 3;
pub(super) const FN_LCGREEK: i32 = 4;
pub(super) const FN_UCGREEK: i32 = 5;
pub(super) const FN_SYMBOL: i32 = 6;
pub(super) const FN_VECTOR: i32 = 7;
pub(super) const FN_NUMBER: i32 = 8;
pub(super) const FN_MTEXTRA: i32 = 11;
pub(super) const FN_TEXT_FE: i32 = 12;
/// `fnEXPAND`: the stretchy characters a template stores after its slots.
pub(super) const FN_EXPAND: i32 = 22;
pub(super) const FN_SPACE: i32 = 24;

/// A character the writer stores in a CHAR record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Glyph {
    pub(super) typeface: i32,
    pub(super) ch: char,
    /// Position in the font's 8-bit encoding (MTEF 5 `mtefOPT_CHAR_ENC_CHAR_8`).
    pub(super) font_position: Option<u8>,
}

impl Glyph {
    pub(super) const fn new(typeface: i32, ch: char) -> Self {
        Self {
            typeface,
            ch,
            font_position: None,
        }
    }

    pub(super) const fn at(typeface: i32, ch: char, position: u8) -> Self {
        Self {
            typeface,
            ch,
            font_position: Some(position),
        }
    }
}

/// Characters of the CJK scripts and their punctuation and full-width forms,
/// which the applications write in the Far East text style.
pub(super) fn is_far_east(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{11FF}'
            | '\u{2E80}'..='\u{9FFF}'
            | '\u{A960}'..='\u{A97F}'
            | '\u{AC00}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE30}'..='\u{FE4F}'
            | '\u{FF00}'..='\u{FFEF}'
    )
}

/// Inverse of an 8-bit font encoding, sorted by character.
fn inverse(decode: fn(u8) -> Option<char>) -> Vec<(char, u8)> {
    let mut table: Vec<(char, u8)> = Vec::new();
    for code in 0x20..=0xFE_u8 {
        if let Some(ch) = decode(code)
            && !table.iter().any(|&(c, _)| c == ch)
        {
            table.push((ch, code));
        }
    }
    table.sort_unstable();
    table
}

fn lookup(table: &[(char, u8)], ch: char) -> Option<u8> {
    table
        .binary_search_by_key(&ch, |&(c, _)| c)
        .ok()
        .map(|i| table[i].1)
}

/// The position of `ch` in the Adobe Symbol font encoding.
pub(super) fn symbol_position(ch: char) -> Option<u8> {
    static TABLE: OnceLock<Vec<(char, u8)>> = OnceLock::new();
    lookup(TABLE.get_or_init(|| inverse(symbol_font_char)), ch)
}

/// The position of `ch` in the MT Extra font, for the characters whose
/// position real equations written by MathType confirm (the reader's table).
/// Other MT Extra characters are written with their MTCode only, which the
/// specification allows.
pub(super) fn mt_extra_position(ch: char) -> Option<u8> {
    MT_EXTRA_CONFIRMED
        .iter()
        .find(|&&(_, c)| c == ch)
        .map(|&(position, _)| position)
}

/// Characters Equation Editor and MathType give the MT Extra style even
/// where no font position is known for them.
fn is_mt_extra_style(ch: char) -> bool {
    matches!(
        ch,
        '\u{2218}' | '\u{2235}' | '\u{2113}' | '\u{2213}' | '\u{22F0}' | '\u{210F}'
    )
}

/// ASCII punctuation that MathType and Equation Editor give the Function
/// style (upright text font) rather than the Symbol style.
fn is_function_style_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | ','
            | '.'
            | ':'
            | ';'
            | '!'
            | '?'
            | '%'
            | '*'
            | '\''
            | '"'
            | '&'
            | '#'
            | '@'
            | '$'
            | '_'
            | '`'
            | '\\'
    )
}

/// The glyph for a symbol character: Function style for ASCII punctuation,
/// Symbol style when the Symbol font has it, MT Extra when that font has it,
/// otherwise Symbol style with only the character code.
pub(super) fn symbol_glyph(ch: char) -> Glyph {
    if is_function_style_punctuation(ch) {
        return Glyph::new(FN_FUNCTION, ch);
    }
    if let Some(position) = symbol_position(ch) {
        return Glyph::at(FN_SYMBOL, ch, position);
    }
    if let Some(position) = mt_extra_position(ch) {
        return Glyph::at(FN_MTEXTRA, ch, position);
    }
    if is_mt_extra_style(ch) {
        return Glyph::new(FN_MTEXTRA, ch);
    }
    Glyph::new(FN_SYMBOL, ch)
}

fn is_lower_greek(ch: char) -> bool {
    matches!(
        ch,
        '\u{03B1}'..='\u{03C9}' | '\u{03D1}' | '\u{03D5}' | '\u{03D6}' | '\u{03F1}' | '\u{03F5}'
    )
}

fn is_upper_greek(ch: char) -> bool {
    matches!(ch, '\u{0391}'..='\u{03A9}' | '\u{03D2}')
}

/// The glyph for a character of a variable name: Variable style for Latin
/// letters, the Greek styles for Greek letters, and the symbol rules for the
/// rest (letter-like symbols such as aleph live in the Symbol font).
pub(super) fn identifier_glyph(ch: char) -> Glyph {
    let greek = if is_lower_greek(ch) {
        Some(FN_LCGREEK)
    } else if is_upper_greek(ch) {
        Some(FN_UCGREEK)
    } else {
        None
    };
    if let Some(typeface) = greek {
        return match symbol_position(ch) {
            Some(position) => Glyph::at(typeface, ch, position),
            None => Glyph::new(typeface, ch),
        };
    }
    if ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
        return Glyph::new(FN_VARIABLE, ch);
    }
    if ch.is_alphabetic() {
        if let Some(position) = symbol_position(ch) {
            return Glyph::at(FN_SYMBOL, ch, position);
        }
        if let Some(position) = mt_extra_position(ch) {
            return Glyph::at(FN_MTEXTRA, ch, position);
        }
        return Glyph::new(FN_VARIABLE, ch);
    }
    symbol_glyph(ch)
}

/// The embellishment type that draws `accent` (MTEFv5.md "EMBELL record";
/// MTEF 3 defines types 2 to 20 with the same meaning). These are exactly the
/// accents the reader produces for each type.
pub(super) fn embellishment(accent: char, position: AccentPosition) -> Option<u8> {
    Some(match (position, accent) {
        (AccentPosition::Above, '\u{0307}') => 2,
        (AccentPosition::Above, '\u{0308}') => 3,
        (AccentPosition::Above, '\u{20DB}') => 4,
        (AccentPosition::Above, '\u{0303}') => 8,
        (AccentPosition::Above, '\u{0302}') => 9,
        (AccentPosition::Above, '\u{20D7}') => 11,
        (AccentPosition::Above, '\u{2190}') => 12,
        (AccentPosition::Above, '\u{2194}') => 13,
        (AccentPosition::Above, '\u{21C0}') => 14,
        (AccentPosition::Above, '\u{21BC}') => 15,
        (AccentPosition::Above, '\u{0304}') => 17,
        (AccentPosition::Above, '\u{23DC}') => 19,
        (AccentPosition::Above, '\u{0306}') => 20,
        (AccentPosition::Above, '\u{20DC}') => 24,
        (AccentPosition::Below, '\u{0323}') => 25,
        (AccentPosition::Below, '\u{0324}') => 26,
        (AccentPosition::Below, '\u{20E8}') => 27,
        (AccentPosition::Below, '\u{0330}') => 30,
        (AccentPosition::Below, '\u{032F}') => 31,
        (AccentPosition::Below, '\u{23DD}') => 32,
        (AccentPosition::Below, '\u{2192}') => 33,
        (AccentPosition::Below, '\u{2190}') => 34,
        (AccentPosition::Below, '\u{2194}') => 35,
        (AccentPosition::Below, '\u{21C1}') => 36,
        (AccentPosition::Below, '\u{21BD}') => 37,
        _ => return None,
    })
}

/// Highest embellishment type MTEF 3 defines.
pub(super) const V3_MAX_EMBELLISHMENT: u8 = 20;

/// Fence families of the ParBoxClass templates, in MTEF selector order
/// (MTEFv5.md "Fences"; MTEF 3 uses the same numbers for the first eight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fence {
    Angle,
    Paren,
    Brace,
    Bracket,
    Bar,
    DoubleBar,
    Floor,
    Ceiling,
    WhiteBracket,
}

impl Fence {
    const ALL: [Fence; 9] = [
        Fence::Angle,
        Fence::Paren,
        Fence::Brace,
        Fence::Bracket,
        Fence::Bar,
        Fence::DoubleBar,
        Fence::Floor,
        Fence::Ceiling,
        Fence::WhiteBracket,
    ];

    pub(super) fn selector(self) -> u8 {
        self as u8
    }

    /// The delimiters the AST uses for this family.
    fn delimiters(self) -> (&'static str, &'static str) {
        match self {
            Fence::Angle => ("\u{27E8}", "\u{27E9}"),
            Fence::Paren => ("(", ")"),
            Fence::Brace => ("{", "}"),
            Fence::Bracket => ("[", "]"),
            Fence::Bar => ("|", "|"),
            Fence::DoubleBar => ("\u{2016}", "\u{2016}"),
            Fence::Floor => ("\u{230A}", "\u{230B}"),
            Fence::Ceiling => ("\u{2308}", "\u{2309}"),
            Fence::WhiteBracket => ("\u{27E6}", "\u{27E7}"),
        }
    }

    /// The `fnEXPAND` characters MathType and Equation Editor store after the
    /// slot of this family's template (left, right).
    pub(super) fn chars(self) -> (char, char) {
        match self {
            Fence::Angle => ('\u{2329}', '\u{232A}'),
            Fence::Paren => ('(', ')'),
            Fence::Brace => ('{', '}'),
            Fence::Bracket => ('[', ']'),
            Fence::Bar => ('\u{EC07}', '\u{EC08}'),
            Fence::DoubleBar => ('\u{EC09}', '\u{EC0A}'),
            Fence::Floor => ('\u{F8F0}', '\u{F8FB}'),
            Fence::Ceiling => ('\u{F8EE}', '\u{F8F9}'),
            Fence::WhiteBracket => ('\u{301A}', '\u{301B}'),
        }
    }

    /// The family whose opening delimiter is `s`.
    pub(super) fn of_open(s: &str) -> Option<Fence> {
        let s = normalize_delimiter(s);
        Self::ALL.into_iter().find(|f| f.delimiters().0 == s)
    }

    /// The family whose closing delimiter is `s`.
    pub(super) fn of_close(s: &str) -> Option<Fence> {
        let s = normalize_delimiter(s);
        Self::ALL.into_iter().find(|f| f.delimiters().1 == s)
    }
}

/// Map equivalent delimiter spellings onto the ones the fence table uses.
fn normalize_delimiter(s: &str) -> &str {
    match s {
        "\u{2329}" | "<" => "\u{27E8}",
        "\u{232A}" | ">" => "\u{27E9}",
        "\u{2223}" => "|",
        "\u{2225}" => "\u{2016}",
        "\u{301A}" => "\u{27E6}",
        "\u{301B}" => "\u{27E7}",
        _ => s,
    }
}

/// Index of a delimiter in the MTEF 5 interval template (`tmINTERVAL`):
/// left parenthesis, right parenthesis, left bracket, right bracket.
pub(super) fn interval_index(s: &str) -> Option<u16> {
    match s {
        "(" => Some(0),
        ")" => Some(1),
        "[" => Some(2),
        "]" => Some(3),
        _ => None,
    }
}

/// The MTEF 3 selector for a pair of brackets and parentheses that do not
/// match (`tmLBLB` .. `tmLPRB`, read as brackets by the reader).
pub(super) fn v3_mixed_fence(open: &str, close: &str) -> Option<u8> {
    Some(match (open, close) {
        ("[", "[") => 8,
        ("]", "]") => 9,
        ("]", "[") => 10,
        ("[", ")") => 11,
        ("(", "]") => 12,
        _ => return None,
    })
}

/// Dirac bracket characters (MTEFv5.md "DiracBoxClass").
pub(super) const DIRAC_LEFT: char = '\u{2329}';
pub(super) const DIRAC_BAR: char = '\u{EC07}';
pub(super) const DIRAC_RIGHT: char = '\u{232A}';

/// An MTEF 5 `tmARROW` variation and the arrow characters MathType stores
/// for it: a double arrow and a harpoon pair are stored as two characters.
pub(super) fn v5_arrow(ch: char) -> Option<(u16, &'static [char])> {
    Some(match ch {
        '\u{2192}' => (0x20, &['\u{2192}']),
        '\u{2190}' => (0x10, &['\u{2190}']),
        '\u{2194}' => (0x30, &['\u{2194}']),
        '\u{21C4}' => (0x01, &['\u{2192}', '\u{2190}']),
        '\u{21CC}' => (0x02, &['\u{21C0}', '\u{21BD}']),
        _ => return None,
    })
}

/// The MTEF 3 arrow template selector (`tmLARROW`, `tmRARROW`, `tmBARROW`).
pub(super) fn v3_arrow(ch: char) -> Option<u8> {
    Some(match ch {
        '\u{2190}' => 18,
        '\u{2192}' => 19,
        '\u{2194}' => 20,
        _ => return None,
    })
}

/// An MTEF 5 accent template (`tmVEC`, `tmTILDE`, `tmHAT`, `tmARC`,
/// `tmJSTATUS`): selector, variation and the character MathType stores.
pub(super) fn v5_accent_template(
    accent: char,
    position: AccentPosition,
) -> Option<(u8, u16, char)> {
    use AccentPosition::{Above, Below};
    Some(match (position, accent) {
        (Above, '\u{0303}') => (32, 0, '\u{0303}'),
        (Above, '\u{0302}') => (33, 0, '\u{0302}'),
        (Above, '\u{23DC}') => (34, 0, '\u{2322}'),
        (Above, '\u{20E7}') => (35, 0, '\u{EE0D}'),
        (Above, '\u{2192}' | '\u{20D7}') => (31, 0x2, '\u{20D7}'),
        (Above, '\u{2190}' | '\u{20D6}') => (31, 0x1, '\u{20D6}'),
        (Above, '\u{2194}' | '\u{20E1}') => (31, 0x3, '\u{20E1}'),
        (Above, '\u{21C0}' | '\u{20D1}') => (31, 0xA, '\u{20D1}'),
        (Above, '\u{21BC}' | '\u{20D0}') => (31, 0x9, '\u{20D0}'),
        (Below, '\u{2192}' | '\u{20EF}') => (31, 0x6, '\u{20D7}'),
        (Below, '\u{2190}' | '\u{20EE}') => (31, 0x5, '\u{20D6}'),
        (Below, '\u{2194}' | '\u{034D}') => (31, 0x7, '\u{20E1}'),
        (Below, '\u{21C1}' | '\u{20EC}') => (31, 0xE, '\u{EB19}'),
        (Below, '\u{21BD}' | '\u{20ED}') => (31, 0xD, '\u{EB18}'),
        _ => return None,
    })
}

/// An MTEF 3 accent template: selector and variation (`tmUARROW`,
/// `tmOARROW`, `tmOARC`). These templates store no character.
pub(super) fn v3_accent_template(accent: char, position: AccentPosition) -> Option<(u8, u8)> {
    use AccentPosition::{Above, Below};
    let selector = match position {
        Above => 47,
        Below => 46,
    };
    Some(match (position, accent) {
        (Above, '\u{23DC}') => (48, 0),
        (_, '\u{2190}' | '\u{20D6}') => (selector, 0),
        (_, '\u{2192}' | '\u{20D7}') => (selector, 1),
        (_, '\u{2194}' | '\u{20E1}') => (selector, 2),
        _ => return None,
    })
}

/// A bar accent drawn with the over-bar or under-bar template.
pub(super) fn is_bar_accent(accent: char, position: AccentPosition) -> bool {
    match position {
        AccentPosition::Above => {
            matches!(accent, '\u{0304}' | '\u{0305}' | '\u{203E}' | '\u{00AF}')
        },
        AccentPosition::Below => matches!(accent, '\u{0332}' | '\u{0331}' | '\u{203E}' | '_'),
    }
}

/// The line mark the reader uses for over-bars and under-bars.
pub(super) const LINE_MARK: &str = "\u{203E}";

/// Horizontal braces and brackets (`tmHBRACE` / `tmHBRACK` in MTEF 5,
/// `tmUHBRACE` / `tmLHBRACE` in MTEF 3): `(top, MTEF 5 bracket, character)`.
pub(super) fn horizontal_brace(s: &str) -> Option<(bool, bool, char)> {
    Some(match s {
        "\u{23DE}" => (true, false, '\u{FE37}'),
        "\u{23DF}" => (false, false, '\u{FE38}'),
        "\u{23B4}" => (true, true, '\u{EC0D}'),
        "\u{23B5}" => (false, true, '\u{EC0C}'),
        _ => return None,
    })
}

/// The long-division bracket the reader puts before the dividend.
pub(super) const LONG_DIVISION: &str = "\u{27CC}";

/// Integral signs, whose limits default to the integral-style placement.
pub(super) fn is_integral(op: char) -> bool {
    matches!(
        op,
        '\u{222B}'..='\u{2233}' | '\u{2A0B}'..='\u{2A1C}'
    )
}

/// An MTEF 5 big-operator template for `op`: selector, the variation bits
/// that select the sign, and the characters MathType stores after the slots.
/// `None` for operators that need the general `tmINTOP` / `tmSUMOP` form.
pub(super) fn v5_big_operator(op: char) -> Option<(u8, u16, &'static [Glyph])> {
    const INT: Glyph = Glyph::at(FN_SYMBOL, '\u{222B}', 0xF2);
    const LOOP: Glyph = Glyph::at(FN_MTEXTRA, '\u{EE11}', 0xD1);
    const LOOP2: Glyph = Glyph::at(FN_MTEXTRA, '\u{EE12}', 0xD2);
    const LOOP3: Glyph = Glyph::at(FN_MTEXTRA, '\u{EE13}', 0xD3);
    const CW_LOOP: Glyph = Glyph::at(FN_MTEXTRA, '\u{EE01}', 0x6A);
    const CCW_LOOP: Glyph = Glyph::at(FN_MTEXTRA, '\u{EE00}', 0x69);
    const SUM: Glyph = Glyph::at(FN_SYMBOL, '\u{2211}', 0xE5);
    const PROD: Glyph = Glyph::at(FN_SYMBOL, '\u{220F}', 0xD5);
    const COPROD: Glyph = Glyph::at(FN_MTEXTRA, '\u{2210}', 0x43);
    const UNION: Glyph = Glyph::at(FN_MTEXTRA, '\u{222A}', 0x55);
    const INTER: Glyph = Glyph::at(FN_MTEXTRA, '\u{2229}', 0x49);
    const INT1: &[Glyph] = &[INT];
    const INT2: &[Glyph] = &[INT, INT];
    const INT3: &[Glyph] = &[INT, INT, INT];
    const OINT: &[Glyph] = &[LOOP, INT];
    const OIINT: &[Glyph] = &[LOOP2, INT, INT];
    const OIIINT: &[Glyph] = &[LOOP3, INT, INT, INT];
    const CW: &[Glyph] = &[CW_LOOP, INT];
    const CCW: &[Glyph] = &[CCW_LOOP, INT];
    Some(match op {
        '\u{222B}' => (15, 0x1, INT1),
        '\u{222C}' => (15, 0x2, INT2),
        '\u{222D}' => (15, 0x3, INT3),
        '\u{222E}' => (15, 0x5, OINT),
        '\u{222F}' => (15, 0x6, OIINT),
        '\u{2230}' => (15, 0x7, OIIINT),
        '\u{2232}' => (15, 0x9, CW),
        '\u{2233}' => (15, 0xD, CCW),
        '\u{2211}' => (16, 0, &[SUM]),
        '\u{220F}' => (17, 0, &[PROD]),
        '\u{2210}' => (18, 0, &[COPROD]),
        '\u{22C3}' => (19, 0, &[UNION]),
        '\u{22C2}' => (20, 0, &[INTER]),
        _ => return None,
    })
}

/// MTEF 3 big-operator templates: `(selector, variation)` for an operator,
/// its limit placement and which limits are present, when Equation Editor
/// has a template for that combination (MTEFv3.md "Template selectors").
pub(super) fn v3_big_operator(
    op: char,
    above_below: bool,
    lower: bool,
    upper: bool,
) -> Option<(u8, u8)> {
    // (no limits, lower only, both) variations of the families that have all
    // three, for integral-style and summation-style limits.
    let family = |integral_style: u8, summation_style: u8| -> Option<(u8, u8)> {
        Some(match (above_below, lower, upper) {
            (false, false, false) => (integral_style, 1),
            (false, true, false) => (integral_style, 0),
            (false, _, true) => (integral_style, 1),
            (true, true, false) => (summation_style, 0),
            (true, _, true) => (summation_style, 1),
            (true, false, false) => (summation_style, 2),
        })
    };
    match op {
        '\u{222B}' => Some(match (above_below, lower, upper) {
            (false, false, false) => (21, 0),
            (false, true, false) => (21, 1),
            (false, _, true) => (21, 2),
            (true, true, false) => (24, 1),
            (true, _, _) => (24, 0),
        }),
        '\u{222E}' => match (above_below, lower, upper) {
            (false, false, false) => Some((21, 3)),
            (false, true, false) => Some((21, 4)),
            (true, true, false) => Some((24, 2)),
            _ => None,
        },
        '\u{222C}' | '\u{222D}' => {
            let (sub_sup, sum_style) = if op == '\u{222C}' { (22, 25) } else { (23, 26) };
            match (above_below, lower, upper) {
                (false, false, false) => Some((sub_sup, 0)),
                (false, true, false) => Some((sub_sup, 1)),
                (true, true, false) => Some((sum_style, 1)),
                _ => None,
            }
        },
        '\u{2211}' => family(30, 29),
        '\u{220F}' => family(32, 31),
        '\u{2210}' => family(34, 33),
        '\u{22C3}' => family(36, 35),
        '\u{22C2}' => family(38, 37),
        _ => None,
    }
}

/// Code of the zero-width MathType space in the space block (`0xEF00` in
/// MTEF 5, `0xEB00` in Equation Editor's MTEF 3).
pub(super) const SPACE_ZERO: u16 = 0x01;

/// A MathType space: a character of the space block, or the no-break space
/// MathType stores in the Function style for a typed space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Space {
    /// Low byte of a code in the space block.
    Block(u16),
    /// U+00A0 in the Function style (a quarter em, as the reader reads it).
    NoBreak,
}

/// MathType spaces by width in 36ths of an em, widest first: 2 em, em,
/// thick, no-break, medium, thin and 1 point.
pub(super) const SPACES: [(u32, Space); 7] = [
    (72, Space::Block(0x06)),
    (36, Space::Block(0x05)),
    (10, Space::Block(0x04)),
    (9, Space::NoBreak),
    (8, Space::Block(0x03)),
    (6, Space::Block(0x02)),
    (3, Space::Block(0x08)),
];

/// The fewest spaces whose widths add up to `units` 36ths of an em, or to
/// the widest smaller total when no combination is exact. At most `limit`
/// spaces are returned.
pub(super) fn space_plan(units: u32, limit: usize) -> (SmallVec<[Space; 4]>, bool) {
    // Whole double-em spaces first; the rest (below 72) by a small search.
    let mut plan: SmallVec<[Space; 4]> = SmallVec::new();
    let mut remaining = units;
    while remaining >= 72 + 36 && plan.len() < limit {
        plan.push(Space::Block(0x06));
        remaining -= 72;
    }
    // best[n]: fewest spaces totalling n, with the last space used.
    let target = remaining.min(72 + 35) as usize;
    let mut best: [Option<(u8, usize)>; 108] = [None; 108];
    best[0] = Some((0, usize::MAX));
    for n in 1..=target {
        for (index, &(width, _)) in SPACES.iter().enumerate() {
            let width = width as usize;
            if width <= n
                && let Some((count, _)) = best[n - width]
                && best[n].is_none_or(|(c, _)| count + 1 < c)
            {
                best[n] = Some((count + 1, index));
            }
        }
    }
    let reachable = (0..=target).rev().find(|&n| best[n].is_some()).unwrap_or(0);
    let mut n = reachable;
    while n > 0 && plan.len() < limit {
        let Some((_, index)) = best[n] else { break };
        plan.push(SPACES[index].1);
        n -= SPACES[index].0 as usize;
    }
    let exact = reachable == target && remaining as usize == target && n == 0;
    (plan, exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_positions_invert_the_reader_table() {
        assert_eq!(symbol_position('\u{03B1}'), Some(0x61));
        assert_eq!(symbol_position('='), Some(0x3D));
        assert_eq!(symbol_position('\u{2212}'), Some(0x2D));
        assert_eq!(symbol_position('\u{222B}'), Some(0xF2));
        assert_eq!(symbol_position('\u{00AE}'), Some(0xD2));
        assert_eq!(symbol_position('x'), None);
    }

    #[test]
    fn glyph_styles_follow_the_applications() {
        assert_eq!(symbol_glyph('('), Glyph::new(FN_FUNCTION, '('));
        assert_eq!(symbol_glyph('+'), Glyph::at(FN_SYMBOL, '+', 0x2B));
        assert_eq!(symbol_glyph('\u{2235}'), Glyph::new(FN_MTEXTRA, '\u{2235}'));
        assert_eq!(identifier_glyph('x'), Glyph::new(FN_VARIABLE, 'x'));
        assert_eq!(
            identifier_glyph('\u{03C0}'),
            Glyph::at(FN_LCGREEK, '\u{03C0}', 0x70)
        );
        assert_eq!(
            identifier_glyph('\u{0394}'),
            Glyph::at(FN_UCGREEK, '\u{0394}', 0x44)
        );
    }

    #[test]
    fn mt_extra_positions_are_the_ones_mathtype_writes() {
        // Pairs found in equations written by MathType 5 and 6.
        for (ch, position) in [
            ('\u{22EF}', 0x4C),
            ('\u{22EE}', 0x4D),
            ('\u{22F1}', 0x4F),
            ('\u{2026}', 0x4B),
            ('\u{2210}', 0x43),
            ('\u{222A}', 0x55),
            ('\u{2229}', 0x49),
        ] {
            assert_eq!(
                mt_extra_position(ch),
                Some(position),
                "U+{:04X}",
                u32::from(ch)
            );
        }
        // No position without a real file showing it.
        assert_eq!(mt_extra_position('\u{2113}'), None);
        assert_eq!(mt_extra_position('\u{22F0}'), None);
        assert_eq!(
            symbol_glyph('\u{22EF}'),
            Glyph::at(FN_MTEXTRA, '\u{22EF}', 0x4C)
        );
    }

    #[test]
    fn fence_families() {
        assert_eq!(Fence::of_open("("), Some(Fence::Paren));
        assert_eq!(Fence::of_close("\u{27E9}"), Some(Fence::Angle));
        assert_eq!(Fence::of_open("\u{2329}"), Some(Fence::Angle));
        assert_eq!(Fence::of_open(")"), None);
        assert_eq!(Fence::WhiteBracket.selector(), 8);
    }
}
