//! `bboldx` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! The `bboldx` package extends classic blackboard-bold ("double-struck")
//! support: it ships a full alphabet (Latin letters, digits **and** Greek)
//! in three weights (light / regular / bold) and two shapes (upright /
//! italic), selected through a family of `\mathbb…` math alphabets, plus a
//! large set of named `\bb…` symbol macros for glyphs that the alphabet
//! selectors cannot reach directly (Greek letters, the n-ary summation, …).
//!
//! Mathema models blackboard-bold as the single [`DoubleStruck`] text style,
//! which the parser already exposes through `\mathbb` (covering uppercase Latin
//! letters *and* digits 0–9 via the Unicode mathematical-alphanumeric block).
//! There is no separate font face for the light/bold weights or the italic
//! shape, so every `bboldx` alphabet selector is expanded to the standard
//! `\mathbb{#1}` — the closest supported approximation.
//!
//! For the named Greek symbols we emit the canonical Unicode double-struck
//! codepoints where they exist (only Γ, Π, γ, π and the n-ary summation have
//! dedicated double-struck codepoints in Unicode); the remaining `bboldx`
//! Greek glyphs have no Unicode representation and are listed as deferred in
//! the package summary.
//!
//! [`DoubleStruck`]: crate::ast::TextStyle::DoubleStruck

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Every alphabet selector collapses onto the standard blackboard-bold style
/// (`\mathbb`), which the parser already supports for both letters and digits.
/// `\mathbb` itself is intentionally **not** redefined here — it is handled
/// directly by the parser, so leaving it alone preserves the established
/// behavior (and lets a user `\renewcommand` still win).
pub const MACROS: &[(&str, u8, &str)] = &[
    // Regular-weight italic double-struck. bboldx spells this `\mathbbit`;
    // the assignment also asks for the shorter alias `\mathbbi`.
    ("mathbbit", 1, r"\mathbb{#1}"),
    ("mathbbi", 1, r"\mathbb{#1}"),
    // Light weight (upright + italic).
    ("mathbblight", 1, r"\mathbb{#1}"),
    ("mathbblightit", 1, r"\mathbb{#1}"),
    // Bold weight (upright + italic).
    ("mathbbb", 1, r"\mathbb{#1}"),
    ("mathbbbit", 1, r"\mathbb{#1}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// Only the Greek glyphs that have dedicated Unicode double-struck codepoints
/// are mapped; the rest of `bboldx`'s named Greek macros have no Unicode home
/// and are deferred.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    ("bbGamma", "\u{213E}", SymbolClass::Alphabetic), // ℾ DOUBLE-STRUCK CAPITAL GAMMA
    ("bbPi", "\u{213F}", SymbolClass::Alphabetic),    // ℿ DOUBLE-STRUCK CAPITAL PI
    ("bbgamma", "\u{213D}", SymbolClass::Alphabetic), // ℽ DOUBLE-STRUCK SMALL GAMMA
    ("bbpi", "\u{213C}", SymbolClass::Alphabetic),    // ℼ DOUBLE-STRUCK SMALL PI
    ("bbsum", "\u{2140}", SymbolClass::LargeOp),      // ⅀ DOUBLE-STRUCK N-ARY SUMMATION
];

#[cfg(test)]
mod tests {
    fn mathml(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_mathml()
            .unwrap()
    }

    fn unicode(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_unicode_math()
            .unwrap()
    }

    // --- \mathbb baseline (provided by the parser) -------------------------

    #[test]
    fn mathbb_uppercase_letters() {
        // MathML keeps the base glyph + a double-struck mathvariant; the
        // canonical Unicode codepoint shows up in the unicode-math output.
        let m = mathml(r"\mathbb{Z}");
        assert!(
            m.contains("double-struck") && m.contains('Z'),
            "expected double-struck Z, got: {m}"
        );
        // \mathbb{Z} -> ℤ (U+2124), \mathbb{R} -> ℝ (U+211D).
        assert!(unicode(r"\mathbb{Z}").contains('\u{2124}'));
        let r = unicode(r"\mathbb{R}");
        assert!(
            r.contains('\u{211D}') || r.contains('\u{1D54D}'),
            "expected double-struck R, got: {r}"
        );
    }

    #[test]
    fn mathbb_digits() {
        // \mathbb{0}..\mathbb{9} map to U+1D7D8..U+1D7E1 in unicode-math.
        assert!(
            unicode(r"\mathbb{0}").contains('\u{1D7D8}'),
            "expected double-struck 0, got: {}",
            unicode(r"\mathbb{0}")
        );
        assert!(
            unicode(r"\mathbb{1}").contains('\u{1D7D9}'),
            "expected double-struck 1, got: {}",
            unicode(r"\mathbb{1}")
        );
        assert!(
            unicode(r"\mathbb{9}").contains('\u{1D7E1}'),
            "expected double-struck 9, got: {}",
            unicode(r"\mathbb{9}")
        );
    }

    // --- italic double-struck selector -------------------------------------

    #[test]
    fn mathbbi_letter() {
        // \mathbbi is approximated by upright \mathbb (no italic bb face).
        let m = mathml(r"\mathbbi{Z}");
        assert!(
            m.contains("double-struck") && m.contains('Z'),
            "expected double-struck Z from \\mathbbi, got: {m}"
        );
        assert!(unicode(r"\mathbbi{Z}").contains('\u{2124}'));
    }

    #[test]
    fn mathbbit_alias() {
        assert!(unicode(r"\mathbbit{Z}").contains('\u{2124}'));
    }

    #[test]
    fn mathbbi_digit_unicode() {
        let out = unicode(r"\mathbbi{1}");
        assert!(
            out.contains('\u{1D7D9}'),
            "expected double-struck 1 (U+1D7D9), got: {out:?}"
        );
    }

    // --- light / bold weight selectors -------------------------------------

    #[test]
    fn weight_selectors_approximate_bb() {
        assert!(unicode(r"\mathbblight{Z}").contains('\u{2124}'));
        assert!(unicode(r"\mathbblightit{Z}").contains('\u{2124}'));
        assert!(unicode(r"\mathbbb{Z}").contains('\u{2124}'));
        assert!(unicode(r"\mathbbbit{Z}").contains('\u{2124}'));
    }

    // --- named Greek double-struck symbols ---------------------------------

    #[test]
    fn greek_double_struck_symbols() {
        assert!(mathml(r"\bbGamma").contains('\u{213E}'));
        assert!(mathml(r"\bbPi").contains('\u{213F}'));
        assert!(mathml(r"\bbgamma").contains('\u{213D}'));
        assert!(mathml(r"\bbpi").contains('\u{213C}'));
        assert!(mathml(r"\bbsum").contains('\u{2140}'));
    }

    // --- registry plumbing -------------------------------------------------

    #[test]
    fn user_macro_overrides_builtin() {
        let out = mathml(r"\newcommand{\mathbbi}[1]{X#1}\mathbbi{Z}");
        assert!(out.contains('X'));
    }

    #[cfg(feature = "render")]
    #[test]
    fn mathbbi_renders_svg() {
        let svg = crate::convert::from_latex(r"\mathbbi{Z}")
            .unwrap()
            .to_svg()
            .unwrap();
        assert!(svg.starts_with("<svg"));
    }
}
