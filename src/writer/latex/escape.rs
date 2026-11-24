//! How one character is spelled in LaTeX: the single escaping policy of the
//! writer. Every place that puts a character of the expression into the
//! output asks [`spelling`], which is a table of [`Place`]s and characters.
//!
//! The spellings keep to what TeX, KaTeX and MathJax read alike, and read
//! back as the same glyph:
//!
//! - a control symbol (`\{`, `\%`) where LaTeX has one for the glyph;
//! - the character token ``\char`\^`` for a glyph that would be syntax and
//!   has no command in math. Ordinary symbols and identifiers also use it for
//!   braces and for glyphs with a class of their own (`<`), since it is an
//!   ordinary atom;
//! - the decimal code `\char7 ` for a character that must not appear in the
//!   source at all.

use crate::unicode::{decode_script_scalar, math_space_width};

/// Where a character is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// A symbol that must stay an ordinary atom.
    Ordinary,
    /// A letter of an identifier.
    Identifier,
    /// Part of an operator of several characters, inside its class command.
    Compound,
    /// A symbol of an operator, delimiter or punctuation class.
    Class,
    /// The glyph inside a class command (`\mathop{…}`), with the class the
    /// glyph has on its own.
    Atom,
    /// Text in the place of a delimiter, inside its class command.
    Delimiter,
    /// A number, which may hold any text (`<mn>50%</mn>`).
    Number,
    /// An accent glyph without an accent command.
    Accent,
    /// The name of a function.
    Name,
    /// Text mode.
    Text,
    /// Text mode in the typewriter face.
    Monospace,
}

/// How a character is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Spelling {
    /// As itself.
    Raw,
    /// As a control symbol: `\%`.
    Backslash,
    /// As a character token: ``\char`\^``.
    Token,
    /// As a decimal character code: `\char7 ` (`\char7{}` in text).
    Code,
    /// As a fixed spelling.
    Fixed(&'static str),
    /// Not at all: TeX rejects the character in text.
    Dropped,
}

/// TeX's special category codes (escape, group, comment, parameter, math
/// shift, alignment, scripts, active `~`). None of them can be written as
/// itself to mean the glyph.
pub(super) const fn is_tex_special(ch: char) -> bool {
    matches!(
        ch,
        '\\' | '{' | '}' | '%' | '#' | '$' | '&' | '_' | '^' | '~'
    )
}

/// A character KaTeX's lexer does not take on its own: a combining mark joins
/// the character before it (the brace opening a group included), and
/// private-use characters and the Unicode line separators are no tokens.
pub(super) fn is_no_token(ch: char) -> bool {
    matches!(ch, '\u{0300}'..='\u{036F}' | '\u{E000}'..='\u{F8FF}' | '\u{2028}' | '\u{2029}')
}

const PLACES: [Place; 11] = [
    Place::Ordinary,
    Place::Identifier,
    Place::Compound,
    Place::Class,
    Place::Atom,
    Place::Delimiter,
    Place::Number,
    Place::Accent,
    Place::Name,
    Place::Text,
    Place::Monospace,
];

/// For each ASCII character, a bit for each place it is not written as itself
/// in.
static ASCII_ESCAPED: [u16; 128] = {
    let mut table = [0; 128];
    let mut byte = 0;
    while byte < 128 {
        let mut place = 0;
        while place < PLACES.len() {
            if !matches!(
                ascii_spelling(byte as u8 as char, PLACES[place]),
                Spelling::Raw
            ) {
                table[byte] |= 1 << PLACES[place] as u16;
            }
            place += 1;
        }
        byte += 1;
    }
    table
};

/// Whether the ASCII character `byte` is not written as itself in `place`.
#[inline]
pub(super) fn ascii_needs_escape(byte: u8, place: Place) -> bool {
    ASCII_ESCAPED[usize::from(byte & 0x7F)] & (1 << place as u16) != 0
}

/// Whether `ch` is not written as itself in `place`.
#[inline]
pub(super) fn needs_escape(ch: char, place: Place) -> bool {
    if ch.is_ascii() {
        ascii_needs_escape(ch as u8, place)
    } else {
        unicode_spelling(ch, place) != Spelling::Raw
    }
}

/// The spelling of `ch` in `place`.
pub(super) fn spelling(ch: char, place: Place) -> Spelling {
    if ch.is_ascii() {
        ascii_spelling(ch, place)
    } else {
        unicode_spelling(ch, place)
    }
}

const fn ascii_spelling(ch: char, place: Place) -> Spelling {
    use Place::*;
    use Spelling::*;
    match (ch, place) {
        ('0'..='9' | 'a'..='z' | 'A'..='Z', Ordinary) => Token,
        ('0'..='9' | 'a'..='z' | 'A'..='Z', _) => Raw,
        ('\0'..=' ' | '\x7F', _) => blank_spelling(ch, place),
        (_, Monospace) if is_tex_special(ch) => Code,
        ('{' | '}', Ordinary | Identifier) => Token,
        ('{' | '}' | '#' | '$' | '%' | '&', _) => Backslash,
        ('_', Name | Text) => Backslash,
        ('\\', Text) => Fixed(r"\textbackslash{}"),
        ('^', Text) => Fixed(r"\textasciicircum{}"),
        ('~', Text) => Fixed(r"\textasciitilde{}"),
        ('\\', Accent) => Fixed(r"\backslash"),
        ('\\' | '^' | '_' | '~', _) => Token,
        (_, Name | Text | Monospace) => Raw,
        // A prime, and an opening quote.
        ('\'' | '`', _) => Token,
        ('<' | '>' | '|', Ordinary | Identifier | Compound | Delimiter) => Token,
        (_, Ordinary | Delimiter) if crate::latex::parse::ascii_ordinary_needs_char(ch) => Token,
        _ => Raw,
    }
}

/// Spaces and control characters. White space is nothing in a math list, and a
/// blank line or another control character is an error anywhere.
const fn blank_spelling(ch: char, place: Place) -> Spelling {
    use Place::*;
    use Spelling::*;
    let space = ch.is_ascii_whitespace() || ch == '\x0B';
    match place {
        Monospace if ch == ' ' => Fixed(r"{\ }"),
        Identifier | Text | Name | Number if ch == ' ' => Raw,
        Text if ch == '\t' => Raw,
        Text | Name | Number if space => Fixed(" "),
        Text | Name => Dropped,
        _ => Code,
    }
}

fn unicode_spelling(ch: char, place: Place) -> Spelling {
    use Place::*;
    use Spelling::*;
    match place {
        Name => match ch {
            '\u{2009}' => Fixed(r"\,"),
            '\u{205F}' => Fixed(r"\:"),
            '\u{2002}' => Fixed(r"\enspace{}"),
            // A space after a control word is literal name content to this
            // reader. An empty group ends the command without growing the name.
            '\u{2001}' => Fixed(r"\quad{}"),
            '\u{00A0}' => Fixed(r"\ "),
            _ if ch.is_control() => Dropped,
            _ => Raw,
        },
        Text if ch.is_control() => Dropped,
        _ if ch.is_control() => Code,
        Text | Monospace => Raw,
        Accent if ch.is_whitespace() => Code,
        Accent => Raw,
        Atom | Delimiter if math_space_width(ch).is_some() => Code,
        Atom | Class if is_no_token(ch) => Code,
        // `²` is a script of what precedes it.
        _ if decode_script_scalar(ch).is_some() => Token,
        _ => Raw,
    }
}

/// Write `ch` with its spelling in `place`. Returns whether the character is
/// in the output.
#[inline]
pub(super) fn write_char(buf: &mut String, ch: char, place: Place) -> bool {
    if needs_escape(ch, place) {
        return write_escaped(buf, ch, place);
    }
    buf.push(ch);
    true
}

fn write_escaped(buf: &mut String, ch: char, place: Place) -> bool {
    match spelling(ch, place) {
        Spelling::Raw => buf.push(ch),
        Spelling::Backslash => {
            buf.push('\\');
            buf.push(ch);
        },
        Spelling::Token => write_char_token(buf, ch),
        // An empty group ends the number in text, where a space would show.
        Spelling::Code if matches!(place, Place::Text | Place::Monospace) => {
            write_char_code(buf, ch, "{}")
        },
        Spelling::Code => write_char_code(buf, ch, " "),
        Spelling::Fixed(text) => buf.push_str(text),
        Spelling::Dropped => return false,
    }
    true
}

/// TeX's character token. It is a single ordinary atom (the glyph's math code
/// in TeX), and nothing after it can be taken as part of it.
pub(super) fn write_char_token(buf: &mut String, ch: char) {
    if ch.is_ascii_whitespace() || ch.is_control() {
        write_char_code(buf, ch, " ");
        return;
    }
    // TeX's math code of `'` makes it a prime: two are two superscripts.
    if ch == '\'' && buf.ends_with(r"\char`'") {
        buf.push_str("{}");
    }
    buf.push_str(r"\char`");
    if is_tex_special(ch) {
        buf.push('\\');
    }
    buf.push(ch);
}

/// A decimal character code, ended before a following digit.
pub(super) fn write_char_code(buf: &mut String, ch: char, terminator: &str) {
    buf.push_str(r"\char");
    buf.push_str(&(ch as u32).to_string());
    buf.push_str(terminator);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The policy, for every ASCII character and class of other characters.
    // Columns: the places, in declaration order.

    /// `r` raw, `\` control symbol, `t` token, `c` code, `s` space, `d`
    /// dropped, `f` another fixed spelling.
    const TABLE: &[(&str, &str)] = &[
        ("{}", r"tt\\\\\\\\c"),
        ("#$%&", r"\\\\\\\\\\c"),
        ("\\", "tttttttftfc"),
        ("^~", "tttttttttfc"),
        ("_", r"tttttttt\\c"),
        ("'`", "ttttttttrrr"),
        ("<>|", "tttrrtrrrrr"),
        ("+-*/=,;()[]", "trrrrtrrrrr"),
        ("!.:?@\"", "rrrrrrrrrrr"),
        ("0189", "trrrrrrrrrr"),
        ("azAZ", "trrrrrrrrrr"),
        (" ", "crccccrcrrf"),
        ("\t", "ccccccscsrc"),
        ("\n\r\x0B\x0C", "ccccccscssc"),
        (
            "\0\x01\x07\x08\x0E\x1B\x1F\x7F\u{80}\u{85}\u{9F}",
            "ccccccccddc",
        ),
        ("\u{2009}\u{205F}\u{2002}\u{A0}", "rrrrccrcfrr"),
        ("²₁", "tttttttrrrr"),
        ("\u{301}\u{338}\u{E000}\u{F8FF}\u{2028}", "rrrccrrrrrr"),
        ("é砀𝐀∑≤\u{20D7}\u{10FFFD}\u{FFFD}", "rrrrrrrrrrr"),
    ];

    fn letter(ch: char, place: Place) -> char {
        match spelling(ch, place) {
            Spelling::Raw => 'r',
            Spelling::Backslash => '\\',
            Spelling::Token => 't',
            Spelling::Code => 'c',
            Spelling::Fixed(" ") => 's',
            Spelling::Fixed(_) => 'f',
            Spelling::Dropped => 'd',
        }
    }

    #[test]
    fn policy_table() {
        for &(chars, row) in TABLE {
            for ch in chars.chars() {
                let got: String = PLACES.iter().map(|&place| letter(ch, place)).collect();
                // White space as an accent is written by its code.
                let row = match ch {
                    '\u{2028}' => "rrrccrrcrrr",
                    '\u{B2}' | '\u{2081}' => row,
                    _ => row,
                };
                assert_eq!(got, row, "{ch:?}");
                for place in PLACES {
                    assert_eq!(needs_escape(ch, place), letter(ch, place) != 'r');
                }
            }
        }
        // Every ASCII character is in the table or a letter or digit.
        for ch in (0u8..128).map(char::from) {
            assert!(
                ch.is_ascii_alphanumeric()
                    || ch.is_ascii_control()
                    || TABLE.iter().any(|(chars, _)| chars.contains(ch)),
                "{ch:?}"
            );
        }
    }
}
