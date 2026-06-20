//! `amsbsy` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `amsbsy` provides `\boldsymbol` and `\pmb` ("poor man's bold").
//!
//! NOTE: Both `\boldsymbol` and `\pmb` are handled directly in
//! `src/latex/parse.rs` / `src/unicode/fonts.rs`. They map to **bold-italic**
//! (math letters and Greek are italic by default, and bolding preserves that
//! shape), distinct from `\mathbf`, which is bold-upright. To avoid duplicating
//! that behavior, this package contributes no macros or symbols.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Intentionally empty: `\pmb` and `\boldsymbol` are handled in `parse.rs`.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    #[test]
    fn boldsymbol_is_bold_italic() {
        // `\boldsymbol{\alpha}` is bold-italic, NOT bold-upright.
        let m = to_mathml(r"\boldsymbol{\alpha}");
        assert!(m.contains("bold-italic"), "expected bold-italic, got: {m}");
    }

    #[test]
    fn boldsymbol_letter_is_bold_italic() {
        let m = to_mathml(r"\boldsymbol{x}");
        assert!(m.contains("bold-italic"), "expected bold-italic, got: {m}");
    }

    #[test]
    fn pmb_is_bold_italic() {
        // Poor man's bold preserves the math-italic shape → bold-italic.
        let m = to_mathml(r"\pmb{\beta}");
        assert!(m.contains("bold-italic"), "expected bold-italic, got: {m}");
    }

    #[test]
    fn mathbf_stays_bold_upright() {
        // `\mathbf` must remain bold-upright (regression guard for the split).
        let m = to_mathml(r"\mathbf{x}");
        assert!(
            m.contains("mathvariant=\"bold\""),
            "expected bold-upright, got: {m}"
        );
    }

    #[test]
    fn boldsymbol_roundtrips() {
        // Bold-italic round-trips to \boldsymbol.
        let latex = convert(r"\boldsymbol{x}", Format::LaTeX, Format::LaTeX)
            .expect("convert should succeed");
        assert!(
            latex.contains("boldsymbol"),
            "expected \\boldsymbol round-trip, got: {latex}"
        );
    }
}
