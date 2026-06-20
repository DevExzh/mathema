//! `gensymb` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `gensymb` provides a handful of unit/typographic symbols that are valid in
//! both text and math mode. We implement them as preprocessor macros expanding
//! to the literal Unicode glyph, which makes them mode-agnostic.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("degree", 0, "\u{00B0}"),      // ° DEGREE SIGN
    ("celsius", 0, "\u{2103}"),     // ℃ DEGREE CELSIUS
    ("micro", 0, "\u{00B5}"),       // µ MICRO SIGN
    ("ohm", 0, "\u{03A9}"),         // Ω GREEK CAPITAL LETTER OMEGA
    ("perthousand", 0, "\u{2030}"), // ‰ PER MILLE SIGN
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    #[test]
    fn degree_sign() {
        assert!(to_mathml(r"45\degree").contains('\u{00B0}'));
    }

    #[test]
    fn celsius_sign() {
        assert!(to_mathml(r"\celsius").contains('\u{2103}'));
    }

    #[test]
    fn micro_sign() {
        assert!(to_mathml(r"\micro").contains('\u{00B5}'));
    }

    #[test]
    fn ohm_sign() {
        assert!(to_mathml(r"\ohm").contains('\u{03A9}'));
    }

    #[test]
    fn perthousand_sign() {
        assert!(to_mathml(r"\perthousand").contains('\u{2030}'));
    }

    #[test]
    fn mode_agnostic_in_text() {
        // gensymb symbols are valid in text mode too (mode-agnostic macros).
        assert!(to_mathml(r"\text{room \degree}").contains('\u{00B0}'));
    }
}
