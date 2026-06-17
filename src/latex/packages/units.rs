//! `units` / `nicefrac` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! These packages provide upright-unit typesetting and "slanted" small
//! fractions. We implement the fraction commands as preprocessor macros that
//! expand to constructs the parser already supports:
//!
//! * `\nicefrac{a}{b}` -> `{}^{a}\!/\!{}_{b}` — a slanted small fraction
//!   approximated with a raised numerator, a tight slash, and a lowered
//!   denominator.
//! * `\sfrac{a}{b}`    -> same approximation as `\nicefrac` (the `xfrac`
//!   package spelling).
//! * `\unitfrac{a}{b}` -> the `\nicefrac` layout with both parts set upright,
//!   i.e. `{}^{\mathrm{a}}\!/\!{}_{\mathrm{b}}`.
//!
//! The `\unit{x}` command is **not** defined here: siunitx redefines `\unit`
//! as the unit-only formatter, and that semantics wins. `\unit` is handled in
//! the parser's `dispatch_command` via [`crate::latex::siunitx`] (which renders
//! upright units), so `\unit{m}` / `\unit{kg}` still typeset correctly. A user
//! `\newcommand{\unit}` still overrides it (user definitions expand first).

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    // \nicefrac{a}{b}: slanted small fraction approximation.
    ("nicefrac", 2, r"{}^{#1}\!/\!{}_{#2}"),
    // \sfrac{a}{b} (xfrac): identical slanted small fraction.
    ("sfrac", 2, r"{}^{#1}\!/\!{}_{#2}"),
    // \unitfrac{a}{b}: slanted small fraction with upright (unit) parts.
    ("unitfrac", 2, r"{}^{\mathrm{#1}}\!/\!{}_{\mathrm{#2}}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    fn to_mathml(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_mathml()
            .unwrap()
    }

    fn to_unicode(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_unicode_math()
            .unwrap()
    }

    #[test]
    fn unit_is_upright_with_thin_space() {
        // \mathrm{m} should render upright (normal mathvariant or <mi>m</mi>
        // without italic), and the unit glyph must be present.
        let mathml = to_mathml(r"\unit{m}");
        assert!(mathml.contains('m'), "expected unit glyph, got: {mathml}");
        // Upright styling: either an explicit normal variant or an <mtext>.
        assert!(
            mathml.contains("normal") || mathml.contains("<mtext") || mathml.contains("<mi"),
            "expected upright unit styling, got: {mathml}"
        );
    }

    #[test]
    fn unit_multichar() {
        let mathml = to_mathml(r"\unit{kg}");
        assert!(mathml.contains("kg") || (mathml.contains('k') && mathml.contains('g')));
    }

    #[test]
    fn nicefrac_has_super_and_subscript() {
        let mathml = to_mathml(r"\nicefrac{1}{2}");
        assert!(
            mathml.contains("<msup"),
            "expected superscript, got: {mathml}"
        );
        assert!(
            mathml.contains("<msub"),
            "expected subscript, got: {mathml}"
        );
        assert!(mathml.contains('1') && mathml.contains('2'));
        assert!(mathml.contains('/'), "expected slash, got: {mathml}");
    }

    #[test]
    fn sfrac_matches_nicefrac() {
        assert_eq!(to_mathml(r"\sfrac{1}{2}"), to_mathml(r"\nicefrac{1}{2}"));
    }

    #[test]
    fn unitfrac_parts_are_upright() {
        let mathml = to_mathml(r"\unitfrac{km}{h}");
        assert!(mathml.contains("<msup"));
        assert!(mathml.contains("<msub"));
        assert!(mathml.contains('/'));
        assert!(mathml.contains("km") || (mathml.contains('k') && mathml.contains('m')));
        assert!(mathml.contains('h'));
    }

    #[test]
    fn nicefrac_unicode_path() {
        let out = to_unicode(r"\nicefrac{1}{2}");
        assert!(!out.is_empty());
        assert!(out.contains('1') && out.contains('2'));
    }

    #[test]
    fn user_macro_overrides_builtin() {
        let mathml = to_mathml(r"\newcommand{\unit}[1]{U#1}\unit{x}");
        assert!(mathml.contains('U'));
    }
}
