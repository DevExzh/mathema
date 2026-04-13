use crate::ast::TextStyle;

/// Map a plain ASCII letter to its styled Unicode equivalent.
///
/// This covers the Mathematical Alphanumeric Symbols block (U+1D400–U+1D7FF)
/// and related ranges.
#[inline]
pub fn styled_char(ch: char, style: TextStyle) -> Option<char> {
    let offset = match style {
        TextStyle::Normal => return Some(ch),
        TextStyle::Italic => italic_offset(ch)?,
        TextStyle::Bold => bold_offset(ch)?,
        TextStyle::BoldItalic => bold_italic_offset(ch)?,
        TextStyle::Script => script_offset(ch)?,
        TextStyle::BoldScript => bold_script_offset(ch)?,
        TextStyle::Fraktur => fraktur_offset(ch)?,
        TextStyle::BoldFraktur => bold_fraktur_offset(ch)?,
        TextStyle::DoubleStruck => double_struck_offset(ch)?,
        TextStyle::SansSerif => sans_offset(ch)?,
        TextStyle::SansSerifBold => sans_bold_offset(ch)?,
        TextStyle::SansSerifItalic => sans_italic_offset(ch)?,
        TextStyle::SansSerifBoldItalic => sans_bold_italic_offset(ch)?,
        TextStyle::Monospace => monospace_offset(ch)?,
    };
    char::from_u32(offset)
}

/// Attempt to detect the style of a Unicode mathematical alphanumeric character
/// and return (base_char, style).
#[inline]
pub fn unstyled_char(ch: char) -> Option<(char, TextStyle)> {
    let cp = ch as u32;

    // Bold uppercase A-Z: U+1D400–U+1D419
    if (0x1D400..=0x1D419).contains(&cp) {
        return Some((char::from_u32(cp - 0x1D400 + b'A' as u32)?, TextStyle::Bold));
    }
    // Bold lowercase a-z: U+1D41A–U+1D433
    if (0x1D41A..=0x1D433).contains(&cp) {
        return Some((char::from_u32(cp - 0x1D41A + b'a' as u32)?, TextStyle::Bold));
    }

    // Italic uppercase A-Z: U+1D434–U+1D44D (with hole at h=U+210E)
    if (0x1D434..=0x1D44D).contains(&cp) {
        return Some((
            char::from_u32(cp - 0x1D434 + b'A' as u32)?,
            TextStyle::Italic,
        ));
    }
    // Italic lowercase a-z: U+1D44E–U+1D467 (with hole at h=U+210E)
    if cp == 0x210E {
        return Some(('h', TextStyle::Italic));
    }
    if (0x1D44E..=0x1D467).contains(&cp) {
        return Some((
            char::from_u32(cp - 0x1D44E + b'a' as u32)?,
            TextStyle::Italic,
        ));
    }

    // Monospace uppercase A-Z: U+1D670–U+1D689
    if (0x1D670..=0x1D689).contains(&cp) {
        return Some((
            char::from_u32(cp - 0x1D670 + b'A' as u32)?,
            TextStyle::Monospace,
        ));
    }
    // Monospace lowercase a-z: U+1D68A–U+1D6A3
    if (0x1D68A..=0x1D6A3).contains(&cp) {
        return Some((
            char::from_u32(cp - 0x1D68A + b'a' as u32)?,
            TextStyle::Monospace,
        ));
    }

    // Double-struck uppercase: U+1D538–U+1D551 (with exceptions C,H,N,P,Q,R,Z)
    if (0x1D538..=0x1D551).contains(&cp) {
        return Some((
            char::from_u32(cp - 0x1D538 + b'A' as u32)?,
            TextStyle::DoubleStruck,
        ));
    }
    // Common double-struck exceptions
    match cp {
        0x2102 => return Some(('C', TextStyle::DoubleStruck)),
        0x210D => return Some(('H', TextStyle::DoubleStruck)),
        0x2115 => return Some(('N', TextStyle::DoubleStruck)),
        0x2119 => return Some(('P', TextStyle::DoubleStruck)),
        0x211A => return Some(('Q', TextStyle::DoubleStruck)),
        0x211D => return Some(('R', TextStyle::DoubleStruck)),
        0x2124 => return Some(('Z', TextStyle::DoubleStruck)),
        _ => {},
    }

    None
}

// --- Offset helpers ---
// Each returns a u32 codepoint for the styled character, or None if not mappable.

#[inline]
fn bold_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D400 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D41A + (ch as u32 - b'a' as u32)),
        '0'..='9' => Some(0x1D7CE + (ch as u32 - b'0' as u32)),
        // Bold uppercase Greek: Α(U+0391)..Ρ(U+03A1) → U+1D6A8..
        '\u{0391}'..='\u{03A1}' => Some(0x1D6A8 + (ch as u32 - 0x0391)),
        // Bold uppercase Greek: Σ(U+03A3)..Ω(U+03A9) → U+1D6BA..
        // (U+03A2 is unassigned in Unicode, so we skip it)
        '\u{03A3}'..='\u{03A9}' => Some(0x1D6BA + (ch as u32 - 0x03A3)),
        // Bold lowercase Greek: α(U+03B1)..ρ(U+03C1) → U+1D6C2..
        '\u{03B1}'..='\u{03C1}' => Some(0x1D6C2 + (ch as u32 - 0x03B1)),
        // Bold lowercase Greek: ς(U+03C2, final sigma) → U+1D6D3
        '\u{03C2}' => Some(0x1D6D3),
        // Bold lowercase Greek: σ(U+03C3)..ω(U+03C9) → U+1D6D4..
        '\u{03C3}'..='\u{03C9}' => Some(0x1D6D4 + (ch as u32 - 0x03C3)),
        // Bold special math symbols
        '\u{2202}' => Some(0x1D6DB), // ∂ → bold partial
        '\u{2207}' => Some(0x1D6C1), // ∇ → bold nabla
        '\u{03F5}' => Some(0x1D6DC), // ϵ → bold epsilon symbol
        '\u{03D1}' => Some(0x1D6DD), // ϑ → bold theta symbol
        '\u{03F0}' => Some(0x1D6DE), // ϰ → bold kappa symbol
        '\u{03D5}' => Some(0x1D6DF), // ϕ → bold phi symbol
        '\u{03F1}' => Some(0x1D6E0), // ϱ → bold rho symbol
        '\u{03D6}' => Some(0x1D6E1), // ϖ → bold pi symbol
        _ => None,
    }
}

#[inline]
fn italic_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D434 + (ch as u32 - b'A' as u32)),
        'h' => Some(0x210E),
        'a'..='z' => Some(0x1D44E + (ch as u32 - b'a' as u32)),
        // Greek lowercase α-ρ → U+1D6FC-U+1D70C
        '\u{03B1}'..='\u{03C1}' => Some(0x1D6FC + (ch as u32 - 0x03B1)),
        // Greek ς (final sigma) → U+1D70D
        '\u{03C2}' => Some(0x1D70D),
        // Greek σ-ω → U+1D70E-U+1D714
        '\u{03C3}'..='\u{03C9}' => Some(0x1D70E + (ch as u32 - 0x03C3)),
        // Greek variants: ϵ ϑ ϰ ϕ ϱ ϖ
        '\u{03F5}' => Some(0x1D716),
        '\u{03D1}' => Some(0x1D717),
        '\u{03F0}' => Some(0x1D718),
        '\u{03D5}' => Some(0x1D719),
        '\u{03F1}' => Some(0x1D71A),
        '\u{03D6}' => Some(0x1D71B),
        _ => None,
    }
}

#[inline]
fn bold_italic_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D468 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D482 + (ch as u32 - b'a' as u32)),
        // Bold italic uppercase Greek: Α(U+0391)..Ρ(U+03A1) → U+1D71C..
        '\u{0391}'..='\u{03A1}' => Some(0x1D71C + (ch as u32 - 0x0391)),
        // Bold italic uppercase Greek: Σ(U+03A3)..Ω(U+03A9) → U+1D72E..
        '\u{03A3}'..='\u{03A9}' => Some(0x1D72E + (ch as u32 - 0x03A3)),
        // Bold italic lowercase Greek: α(U+03B1)..ρ(U+03C1) → U+1D736..
        '\u{03B1}'..='\u{03C1}' => Some(0x1D736 + (ch as u32 - 0x03B1)),
        // Bold italic lowercase Greek: ς(U+03C2, final sigma) → U+1D747
        '\u{03C2}' => Some(0x1D747),
        // Bold italic lowercase Greek: σ(U+03C3)..ω(U+03C9) → U+1D748..
        '\u{03C3}'..='\u{03C9}' => Some(0x1D748 + (ch as u32 - 0x03C3)),
        _ => None,
    }
}

#[inline]
fn script_offset(ch: char) -> Option<u32> {
    match ch {
        // Several script uppercase letters have dedicated codepoints
        'B' => Some(0x212C),
        'E' => Some(0x2130),
        'F' => Some(0x2131),
        'H' => Some(0x210B),
        'I' => Some(0x2110),
        'L' => Some(0x2112),
        'M' => Some(0x2133),
        'R' => Some(0x211B),
        'A'..='Z' => Some(0x1D49C + (ch as u32 - b'A' as u32)),
        // Lowercase letters with dedicated codepoints (SMP slots are unassigned)
        'e' => Some(0x212F),
        'g' => Some(0x210A),
        'o' => Some(0x2134),
        'a'..='z' => Some(0x1D4B6 + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn bold_script_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D4D0 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D4EA + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn fraktur_offset(ch: char) -> Option<u32> {
    match ch {
        'C' => Some(0x212D),
        'H' => Some(0x210C),
        'I' => Some(0x2111),
        'R' => Some(0x211C),
        'Z' => Some(0x2128),
        'A'..='Z' => Some(0x1D504 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D51E + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn bold_fraktur_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D56C + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D586 + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn double_struck_offset(ch: char) -> Option<u32> {
    match ch {
        'C' => Some(0x2102),
        'H' => Some(0x210D),
        'N' => Some(0x2115),
        'P' => Some(0x2119),
        'Q' => Some(0x211A),
        'R' => Some(0x211D),
        'Z' => Some(0x2124),
        'A'..='Z' => Some(0x1D538 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D552 + (ch as u32 - b'a' as u32)),
        '0'..='9' => Some(0x1D7D8 + (ch as u32 - b'0' as u32)),
        _ => None,
    }
}

#[inline]
fn sans_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D5A0 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D5BA + (ch as u32 - b'a' as u32)),
        '0'..='9' => Some(0x1D7E2 + (ch as u32 - b'0' as u32)),
        _ => None,
    }
}

#[inline]
fn sans_bold_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D5D4 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D5EE + (ch as u32 - b'a' as u32)),
        '0'..='9' => Some(0x1D7EC + (ch as u32 - b'0' as u32)),
        _ => None,
    }
}

#[inline]
fn sans_italic_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D608 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D622 + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn sans_bold_italic_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D63C + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D656 + (ch as u32 - b'a' as u32)),
        _ => None,
    }
}

#[inline]
fn monospace_offset(ch: char) -> Option<u32> {
    match ch {
        'A'..='Z' => Some(0x1D670 + (ch as u32 - b'A' as u32)),
        'a'..='z' => Some(0x1D68A + (ch as u32 - b'a' as u32)),
        '0'..='9' => Some(0x1D7F6 + (ch as u32 - b'0' as u32)),
        _ => None,
    }
}

/// Map a LaTeX font command to a TextStyle.
#[inline]
pub fn latex_font_to_style(command: &str) -> Option<TextStyle> {
    match command {
        "mathrm" | "textrm" | "text" | "mathup" | "mathsfup" => Some(TextStyle::Normal),
        "mathbf" | "textbf" | "boldsymbol" | "bm" | "pmb" | "mathbfup" | "mathbfsfup" => {
            Some(TextStyle::Bold)
        },
        "mathit" | "textit" | "mathnormal" => Some(TextStyle::Italic),
        "mathbfit" | "mathbfsfit" => Some(TextStyle::BoldItalic),
        "mathtt" | "texttt" => Some(TextStyle::Monospace),
        "mathsf" | "textsf" => Some(TextStyle::SansSerif),
        "mathsfit" => Some(TextStyle::SansSerifItalic),
        "mathbb" => Some(TextStyle::DoubleStruck),
        "mathcal" | "mathscr" => Some(TextStyle::Script),
        "mathbfcal" | "mathbfscr" => Some(TextStyle::BoldScript),
        "mathfrak" => Some(TextStyle::Fraktur),
        "mathbffrak" => Some(TextStyle::BoldFraktur),
        _ => None,
    }
}

/// Map a TextStyle to its preferred LaTeX font command.
#[inline]
pub fn style_to_latex_font(style: TextStyle) -> &'static str {
    match style {
        TextStyle::Normal => "mathrm",
        TextStyle::Bold => "mathbf",
        TextStyle::Italic => "mathit",
        TextStyle::BoldItalic => "boldsymbol",
        TextStyle::Monospace => "mathtt",
        TextStyle::SansSerif => "mathsf",
        TextStyle::SansSerifBold => "mathsf",
        TextStyle::SansSerifItalic => "mathsf",
        TextStyle::SansSerifBoldItalic => "mathsf",
        TextStyle::DoubleStruck => "mathbb",
        TextStyle::Script => "mathcal",
        TextStyle::BoldScript => "mathcal",
        TextStyle::Fraktur => "mathfrak",
        TextStyle::BoldFraktur => "mathfrak",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_mapping() {
        assert_eq!(styled_char('A', TextStyle::Bold), Some('\u{1D400}'));
        assert_eq!(styled_char('z', TextStyle::Bold), Some('\u{1D433}'));
        assert_eq!(styled_char('0', TextStyle::Bold), Some('\u{1D7CE}'));
    }

    #[test]
    fn italic_mapping() {
        assert_eq!(styled_char('A', TextStyle::Italic), Some('\u{1D434}'));
        assert_eq!(styled_char('h', TextStyle::Italic), Some('\u{210E}'));
    }

    #[test]
    fn double_struck_special() {
        assert_eq!(styled_char('R', TextStyle::DoubleStruck), Some('\u{211D}'));
        assert_eq!(styled_char('Z', TextStyle::DoubleStruck), Some('\u{2124}'));
    }

    #[test]
    fn round_trip_bold() {
        for ch in 'A'..='Z' {
            let styled = styled_char(ch, TextStyle::Bold).unwrap();
            let (base, style) = unstyled_char(styled).unwrap();
            assert_eq!(base, ch);
            assert_eq!(style, TextStyle::Bold);
        }
    }

    #[test]
    fn normal_passthrough() {
        assert_eq!(styled_char('x', TextStyle::Normal), Some('x'));
    }

    #[test]
    fn font_command_mapping() {
        assert_eq!(latex_font_to_style("mathbb"), Some(TextStyle::DoubleStruck));
        assert_eq!(latex_font_to_style("mathfrak"), Some(TextStyle::Fraktur));
        assert_eq!(latex_font_to_style("unknown"), None);
    }

    #[test]
    fn bold_greek_lowercase() {
        // α (U+03B1) → bold alpha U+1D6C2 '𝛂'
        assert_eq!(styled_char('\u{03B1}', TextStyle::Bold), Some('\u{1D6C2}'));
        // β (U+03B2) → bold beta U+1D6C3
        assert_eq!(styled_char('\u{03B2}', TextStyle::Bold), Some('\u{1D6C3}'));
        // ω (U+03C9) → bold omega U+1D6DA
        assert_eq!(styled_char('\u{03C9}', TextStyle::Bold), Some('\u{1D6DA}'));
        // σ (U+03C3) → bold sigma U+1D6D4
        assert_eq!(styled_char('\u{03C3}', TextStyle::Bold), Some('\u{1D6D4}'));
        // ς (U+03C2, final sigma) → U+1D6D3
        assert_eq!(styled_char('\u{03C2}', TextStyle::Bold), Some('\u{1D6D3}'));
    }

    #[test]
    fn bold_greek_uppercase() {
        // Α (U+0391) → bold Alpha U+1D6A8
        assert_eq!(styled_char('\u{0391}', TextStyle::Bold), Some('\u{1D6A8}'));
        // Σ (U+03A3) → bold Sigma U+1D6BA
        assert_eq!(styled_char('\u{03A3}', TextStyle::Bold), Some('\u{1D6BA}'));
        // Ω (U+03A9) → bold Omega U+1D6C0
        assert_eq!(styled_char('\u{03A9}', TextStyle::Bold), Some('\u{1D6C0}'));
    }

    #[test]
    fn bold_special_math_symbols() {
        assert_eq!(styled_char('\u{2202}', TextStyle::Bold), Some('\u{1D6DB}')); // ∂
        assert_eq!(styled_char('\u{2207}', TextStyle::Bold), Some('\u{1D6C1}')); // ∇
        assert_eq!(styled_char('\u{03F5}', TextStyle::Bold), Some('\u{1D6DC}')); // ϵ
        assert_eq!(styled_char('\u{03D1}', TextStyle::Bold), Some('\u{1D6DD}')); // ϑ
        assert_eq!(styled_char('\u{03D5}', TextStyle::Bold), Some('\u{1D6DF}')); // ϕ
    }

    #[test]
    fn bold_italic_greek() {
        // α (U+03B1) → bold italic alpha U+1D736
        assert_eq!(styled_char('\u{03B1}', TextStyle::BoldItalic), Some('\u{1D736}'));
        // Α (U+0391) → bold italic Alpha U+1D71C
        assert_eq!(styled_char('\u{0391}', TextStyle::BoldItalic), Some('\u{1D71C}'));
        // Σ (U+03A3) → bold italic Sigma U+1D72E
        assert_eq!(styled_char('\u{03A3}', TextStyle::BoldItalic), Some('\u{1D72E}'));
        // ω (U+03C9) → bold italic omega U+1D74E
        assert_eq!(styled_char('\u{03C9}', TextStyle::BoldItalic), Some('\u{1D74E}'));
    }
}
