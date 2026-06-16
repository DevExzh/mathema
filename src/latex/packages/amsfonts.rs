//! `amsfonts` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `amsfonts` provides the math alphabets `\mathbb` (blackboard bold /
//! double-struck) and `\mathfrak` (Fraktur). Both are already handled directly
//! by the parser (see `parse.rs` font-command handling, which maps `mathbb` ->
//! `DoubleStruck` and `mathfrak` -> `Fraktur`), so this package contributes no
//! macro for them — the unit tests below pin that existing behavior.
//!
//! It also defines a handful of extra symbols. The ones already present in the
//! core symbol table (`\square` U+25A1, `\maltese` U+2720) are intentionally
//! **not** redefined here — package symbols are only consulted after the core
//! PHF tables miss, so re-listing them would be dead entries. The remaining
//! amsfonts symbols are missing from core and are contributed below.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// `amsfonts` adds no rewrite macros: `\mathbb`/`\mathfrak` are parser builtins,
/// and every extra command is a single glyph handled via [`SYMBOLS`].
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// Only symbols missing from the core table are listed. `\square` (U+25A1) and
/// `\maltese` (U+2720) are already covered by core and deliberately omitted.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    // Corner delimiters (used as open/close fences, e.g. Gödel quotes).
    ("ulcorner", "\u{231C}", SymbolClass::Open),  // ⌜ upper-left
    ("urcorner", "\u{231D}", SymbolClass::Close), // ⌝ upper-right
    ("llcorner", "\u{231E}", SymbolClass::Open),  // ⌞ lower-left
    ("lrcorner", "\u{231F}", SymbolClass::Close), // ⌟ lower-right
    // Geometric / miscellaneous shapes.
    ("lozenge", "\u{25CA}", SymbolClass::Ordinary), // ◊
    ("checkmark", "\u{2713}", SymbolClass::Ordinary), // ✓
    ("circledR", "\u{00AE}", SymbolClass::Ordinary), // ® registered sign
    ("yen", "\u{00A5}", SymbolClass::Ordinary),     // ¥ yen sign
    // Blackboard-bold lowercase k (the only \mathbb glyph with a dedicated cmd).
    ("Bbbk", "\u{1D55C}", SymbolClass::Alphabetic), // 𝕜
];

#[cfg(test)]
mod tests {
    use crate::convert;

    fn mathml(latex: &str) -> String {
        convert::from_latex(latex)
            .expect("parse should succeed")
            .to_mathml()
            .expect("mathml should succeed")
    }

    fn unicode_math(latex: &str) -> String {
        convert::from_latex(latex)
            .expect("parse should succeed")
            .to_unicode_math()
            .expect("unicode-math should succeed")
    }

    // --- Verification: math alphabets are already parser-supported ----------

    #[test]
    fn mathbb_r_is_double_struck() {
        // \mathbb{R} -> ℝ (U+211D)
        let out = mathml(r"\mathbb{R}");
        assert!(
            out.contains('\u{211D}') || out.contains("double-struck"),
            "expected ℝ / double-struck, got: {out}"
        );
    }

    #[test]
    fn mathfrak_lower_g() {
        // \mathfrak{g} -> fraktur small g (U+1D524)
        let out = mathml(r"\mathfrak{g}");
        assert!(
            out.contains('\u{1D524}') || out.contains("fraktur"),
            "expected fraktur g, got: {out}"
        );
    }

    #[test]
    fn mathfrak_upper_a() {
        // \mathfrak{A} -> fraktur capital A (U+1D504)
        let out = mathml(r"\mathfrak{A}");
        assert!(
            out.contains('\u{1D504}') || out.contains("fraktur"),
            "expected fraktur A, got: {out}"
        );
    }

    // --- Added symbols ------------------------------------------------------

    #[test]
    fn corner_delimiters() {
        assert!(mathml(r"\ulcorner x \urcorner").contains('\u{231C}'));
        assert!(mathml(r"\ulcorner x \urcorner").contains('\u{231D}'));
        assert!(mathml(r"\llcorner x \lrcorner").contains('\u{231E}'));
        assert!(mathml(r"\llcorner x \lrcorner").contains('\u{231F}'));
    }

    #[test]
    fn lozenge_checkmark() {
        assert!(mathml(r"\lozenge").contains('\u{25CA}'));
        assert!(mathml(r"\checkmark").contains('\u{2713}'));
    }

    #[test]
    fn circled_r_and_yen() {
        assert!(mathml(r"\circledR").contains('\u{00AE}'));
        assert!(mathml(r"\yen").contains('\u{00A5}'));
    }

    #[test]
    fn bbbk_double_struck_k() {
        let out = mathml(r"\Bbbk");
        assert!(out.contains('\u{1D55C}'), "expected 𝕜, got: {out}");
    }

    #[test]
    fn corners_in_unicode_math_path() {
        // Exercise a second writer to confirm the glyph survives end-to-end.
        let out = unicode_math(r"\ulcorner a \urcorner");
        assert!(out.contains('\u{231C}'), "expected ⌜ in unicode-math, got: {out}");
    }

    // --- Already-in-core symbols still resolve (not double-defined here) ----

    #[test]
    fn square_and_maltese_from_core() {
        assert!(mathml(r"\square").contains('\u{25A1}'));
        assert!(mathml(r"\maltese").contains('\u{2720}'));
    }
}
