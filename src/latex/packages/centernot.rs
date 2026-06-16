//! `centernot` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! # Status: satisfied by the core parser
//!
//! The `centernot` package provides a single command, `\centernot`, which
//! overlays a centered negation slash on its argument (a better-centered
//! alternative to `\not`). This command is **already handled directly by the
//! core parser** in `src/latex/parse.rs` (see the `"centernot"` arm, which
//! overlays a combining long solidus `U+0338` on the parsed argument and
//! supports both braced `\centernot{=}` and unbraced `\centernot\rightarrow`
//! forms via `parse_required_group`).
//!
//! Because the parser builds the negation overlay structurally, there is no
//! macro rewrite or fallback symbol that would express it more faithfully —
//! seeding a builtin macro here would only *shadow* the existing parser
//! handling. Therefore both [`MACROS`] and [`SYMBOLS`] are intentionally left
//! empty. The `#[cfg(test)]` regression tests below pin the parser behavior so
//! a future change cannot silently regress `centernot` support.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Empty: `\centernot` is handled directly by the core parser (see module
/// docs). Defining a macro here would shadow that handling.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// Empty: `\centernot` is a structural overlay, not a single glyph.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    /// The combining long solidus overlay used by `\centernot`.
    const COMBINING_SOLIDUS: char = '\u{0338}';

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    #[test]
    fn centernot_arrow_parses_and_overlays_solidus() {
        // Unbraced single-token argument form: `\centernot\rightarrow`.
        let mathml = to_mathml(r"\centernot\rightarrow");
        // The right arrow glyph (U+2192) must survive.
        assert!(
            mathml.contains('\u{2192}'),
            "expected right arrow glyph, got: {mathml}"
        );
        // ...and be negated by the combining solidus overlay.
        assert!(
            mathml.contains(COMBINING_SOLIDUS),
            "expected combining solidus overlay (U+0338), got: {mathml}"
        );
    }

    #[test]
    fn centernot_equals_parses_and_overlays_solidus() {
        // Unbraced single-token argument form: `\centernot=`.
        let mathml = to_mathml(r"\centernot=");
        assert!(
            mathml.contains('='),
            "expected equals sign, got: {mathml}"
        );
        assert!(
            mathml.contains(COMBINING_SOLIDUS),
            "expected combining solidus overlay (U+0338), got: {mathml}"
        );
    }

    #[test]
    fn centernot_braced_argument_parses() {
        // Braced argument form: `\centernot{=}`.
        let mathml = to_mathml(r"\centernot{=}");
        assert!(mathml.contains('='));
        assert!(
            mathml.contains(COMBINING_SOLIDUS),
            "expected combining solidus overlay (U+0338), got: {mathml}"
        );
    }

    #[test]
    fn centernot_unicode_math_path() {
        // Exercise the LaTeX -> Unicode-math output path as well.
        let uni = convert(r"\centernot=", Format::LaTeX, Format::UnicodeMath)
            .expect("unicode-math convert should succeed");
        assert!(
            uni.contains(COMBINING_SOLIDUS),
            "expected combining solidus in unicode-math output, got: {uni:?}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn centernot_svg_path_renders() {
        // LaTeX -> SVG must succeed and produce a non-trivial document.
        let svg = crate::convert::from_latex(r"\centernot\rightarrow")
            .expect("parse should succeed")
            .to_svg()
            .expect("svg render should succeed");
        assert!(svg.contains("<svg"), "expected an <svg> document, got: {svg}");
    }

    #[test]
    fn centernot_contributes_no_builtin_macro() {
        // Confirm we did not seed a macro that would shadow the parser handler.
        assert!(!super::super::is_builtin_macro_name("centernot"));
    }
}
