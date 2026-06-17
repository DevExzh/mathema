//! `dsfont` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! The `dsfont` package provides `\mathds{#1}`, a double-struck / blackboard
//! alphabet that, unlike `amsfonts`' `\mathbb`, also covers digits. The parser
//! already supports `\mathbb` (mapped to the `DoubleStruck` text style) and its
//! Unicode mapping covers both letters (`R` -> ℝ, U+1D54D / U+211D) **and**
//! digits (`1` -> 𝟙, U+1D7D9). So we simply expand `\mathds` -> `\mathbb`.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    // `\mathds{X}` double-struck/blackboard — reuse the parser's `\mathbb`,
    // which styles letters AND digits as DoubleStruck.
    ("mathds", 1, r"\mathbb{#1}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    fn unicode(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_unicode_math()
            .unwrap()
    }

    fn mathml(latex: &str) -> String {
        crate::convert::from_latex(latex)
            .unwrap()
            .to_mathml()
            .unwrap()
    }

    #[test]
    fn mathds_letter_unicode() {
        // \mathds{R} -> double-struck R. The blackboard R has the canonical
        // codepoint U+211D (ℝ); U+1D54D is the alphanumeric-block alias.
        let out = unicode(r"\mathds{R}");
        assert!(
            out.contains('\u{211D}') || out.contains('\u{1D54D}'),
            "expected double-struck R, got: {out:?}"
        );
    }

    #[test]
    fn mathds_letter_mathml() {
        let out = mathml(r"\mathds{R}");
        assert!(
            out.contains('\u{211D}') || out.contains('\u{1D54D}') || out.contains("double-struck"),
            "expected double-struck R, got: {out}"
        );
    }

    #[test]
    fn mathds_digit_unicode() {
        // \mathds{1} -> 𝟙 (U+1D7D9).
        let out = unicode(r"\mathds{1}");
        assert!(
            out.contains('\u{1D7D9}'),
            "expected double-struck 1 (U+1D7D9), got: {out:?}"
        );
    }

    #[test]
    fn mathds_digit_mathml() {
        let out = mathml(r"\mathds{1}");
        assert!(
            out.contains('\u{1D7D9}') || out.contains("double-struck"),
            "expected double-struck 1, got: {out}"
        );
    }
}
