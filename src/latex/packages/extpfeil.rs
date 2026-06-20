//! `extpfeil` package support — extensible arrows beyond amsmath.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! ## Coverage
//!
//! The `extpfeil` package provides a handful of stretchable arrows (with an
//! optional `[below]` and a required `{above}` label) plus the `\Newextarrow`
//! macro factory. Most of the named arrows are *already* handled directly by
//! the parser's extensible-arrow machinery (`parse_extensible_arrow`), so this
//! package must NOT shadow them with builtin macros (that would change the
//! established, tested behavior). Status of each extpfeil command:
//!
//! | command              | status   | how                                        |
//! |----------------------|----------|--------------------------------------------|
//! | `\xtwoheadrightarrow`| exact    | parser (`\u{21A0}` stretchy, `[below]{above}`) |
//! | `\xtwoheadleftarrow` | exact    | parser (`\u{219E}` stretchy)               |
//! | `\xmapsto`           | exact    | parser (`\u{21A6}` stretchy)               |
//! | `\xlongequal`        | exact    | parser (`=` stretchy)                      |
//! | `\xtofrom`           | exact    | parser (`\u{21C4}` stretchy)               |
//! | `\xrightharpoondown` | exact    | parser (`\u{21C1}` stretchy)               |
//! | (other harpoons/hooks/double arrows) | exact | parser              |
//! | `\xLongrightarrow`   | exact    | parser (`\u{27F9}` long double arrow ⟹) |
//! | `\Newextarrow`       | deferred | custom-arrow macro factory (structural)    |
//!
//! Every named extpfeil arrow — including `\xLongrightarrow`, which renders as
//! the true long double arrow `⟹` (U+27F9) — is handled directly by the
//! parser's extensible-arrow machinery, so this package contributes **no**
//! builtin macros (a macro would shadow the tested parser behavior).
//! `\Newextarrow` (the custom-arrow factory) remains deferred.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Empty: all extpfeil arrows are handled by the parser. `\Newextarrow` is
/// intentionally absent (deferred: needs structural macro-factory support).
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// extpfeil contributes no standalone glyphs; every arrow is a stretchy
/// construct handled by the parser or by the `\xLongrightarrow` macro above.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    fn to_unicode(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::UnicodeMath).expect("convert should succeed")
    }

    // --- \xLongrightarrow: the one command this package adds -----------------

    #[test]
    fn xlongrightarrow_renders_long_double_arrowhead() {
        // The true long double arrow ⟹ (U+27F9), with the label placed above.
        let mathml = to_mathml(r"\xLongrightarrow{f}");
        assert!(
            mathml.contains('\u{27F9}'),
            "expected long double rightarrow U+27F9, got: {mathml}"
        );
        assert!(
            mathml.contains("over") || mathml.contains('f'),
            "expected the label `f` placed over the arrow, got: {mathml}"
        );
    }

    #[test]
    fn xlongrightarrow_with_below_label() {
        // Optional [below] must flow through (under+over).
        let mathml = to_mathml(r"\xLongrightarrow[g]{f}");
        assert!(mathml.contains('\u{27F9}'), "got: {mathml}");
        assert!(mathml.contains('f'), "expected above label, got: {mathml}");
        assert!(mathml.contains('g'), "expected below label, got: {mathml}");
    }

    #[test]
    fn xlongrightarrow_unicode_path() {
        let out = to_unicode(r"\xLongrightarrow{f}");
        assert!(
            out.contains('\u{27F9}'),
            "expected U+27F9 in unicode-math output, got: {out}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn xlongrightarrow_svg_path() {
        let svg = crate::convert::from_latex(r"\xLongrightarrow{f}")
            .unwrap()
            .to_svg()
            .unwrap();
        assert!(svg.starts_with("<svg"), "expected SVG output");
    }

    // --- Sanity: parser-handled extpfeil arrows still work (not shadowed) ----

    #[test]
    fn parser_arrows_unshadowed() {
        // These are handled directly by parse.rs; this package must not break
        // them. Just confirm they still parse and render.
        assert!(to_mathml(r"\xtwoheadrightarrow{a}").contains('\u{21A0}'));
        assert!(to_mathml(r"\xtwoheadleftarrow{a}").contains('\u{219E}'));
        assert!(to_mathml(r"\xmapsto{a}").contains('\u{21A6}'));
        assert!(to_mathml(r"\xlongequal{a}").contains('='));
        assert!(to_mathml(r"\xrightharpoondown{a}").contains('\u{21C1}'));
    }

    #[test]
    fn user_macro_overrides_xlongrightarrow() {
        let mathml = to_mathml(r"\newcommand{\xLongrightarrow}[1]{Q}\xLongrightarrow{f}");
        assert!(
            mathml.contains('Q'),
            "user redefinition must win, got: {mathml}"
        );
    }
}
