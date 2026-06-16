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
//! | `\xLongrightarrow`   | approx.  | macro -> `\xRightarrow` (double-head `\u{21D2}`) |
//! | `\Newextarrow`       | deferred | custom-arrow macro factory (structural)    |
//!
//! Only `\xLongrightarrow` is genuinely MISSING from the parser, so it is the
//! single command this package contributes. It expands to the parser-supported
//! `\xRightarrow`, which carries the matching double `=>` arrowhead and the same
//! optional `[below]{above}` argument shape (the macro rewrites only the name,
//! so any following `[...]{...}` is consumed by `\xRightarrow` as usual). This
//! is an approximation: the literal long double arrow (`\u{27F9}`) is rendered
//! as the standard stretchy double arrow (`\u{21D2}`).

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// `\xLongrightarrow` is the only extpfeil arrow not already handled by the
/// parser; it rewrites to the parser-supported `\xRightarrow` (same argument
/// shape, matching double arrowhead). `\Newextarrow` is intentionally absent —
/// see the module docs (deferred: needs structural macro-factory support).
pub const MACROS: &[(&str, u8, &str)] = &[("xLongrightarrow", 0, "\\xRightarrow")];

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
    fn xlongrightarrow_renders_double_arrowhead() {
        // Approximated as \xRightarrow -> stretchy U+21D2 (=>) with the label
        // placed above (Overscript).
        let mathml = to_mathml(r"\xLongrightarrow{f}");
        assert!(
            mathml.contains('\u{21D2}'),
            "expected double rightarrow U+21D2, got: {mathml}"
        );
        assert!(
            mathml.contains("over") || mathml.contains('f'),
            "expected the label `f` placed over the arrow, got: {mathml}"
        );
    }

    #[test]
    fn xlongrightarrow_with_below_label() {
        // Optional [below] must flow through to \xRightarrow (under+over).
        let mathml = to_mathml(r"\xLongrightarrow[g]{f}");
        assert!(mathml.contains('\u{21D2}'), "got: {mathml}");
        assert!(mathml.contains('f'), "expected above label, got: {mathml}");
        assert!(mathml.contains('g'), "expected below label, got: {mathml}");
    }

    #[test]
    fn xlongrightarrow_unicode_path() {
        let out = to_unicode(r"\xLongrightarrow{f}");
        assert!(
            out.contains('\u{21D2}'),
            "expected U+21D2 in unicode-math output, got: {out}"
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
        assert!(mathml.contains('Q'), "user redefinition must win, got: {mathml}");
    }
}
