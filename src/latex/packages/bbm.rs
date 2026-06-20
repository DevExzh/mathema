//! `bbm` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! The `bbm` package provides blackboard-bold ("double-struck") letters drawn
//! in the Computer Modern style via three font selectors:
//!
//! * `\mathbbm{#1}`   — blackboard-bold roman
//! * `\mathbbmss{#1}` — blackboard-bold sans-serif
//! * `\mathbbmtt{#1}` — blackboard-bold typewriter
//!
//! Mathema has no dedicated CM-style bbm glyph set, so each selector is
//! expanded to the standard `\mathbb{#1}` blackboard style — the closest
//! supported approximation. This covers the common cases (`\mathbbm{1}` → 𝟙,
//! `\mathbbm{Z}` → ℤ, etc.).
//!
//! **The roman/sans-serif/typewriter collapse is a Unicode limitation, not a
//! bug.** Unicode encodes exactly *one* double-struck alphabet (the
//! mathematical-alphanumeric block) and assigns no distinct codepoints for the
//! CM-roman, sans-serif, or typewriter *shapes* that `bbm` draws. Because the
//! rendering pipeline operates on Unicode codepoints, those shape variants are
//! physically unrepresentable and necessarily fold onto `\mathbb`. The distinct
//! `bbm` glyph shapes are therefore *not* reproduced; see the package summary
//! for the deferred work.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// All three selectors collapse onto the standard blackboard-bold style, which
/// the parser already supports via `\mathbb`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("mathbbm", 1, r"\mathbb{#1}"),
    ("mathbbmss", 1, r"\mathbb{#1}"),
    ("mathbbmtt", 1, r"\mathbb{#1}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
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

    #[test]
    fn mathbbm_digit_one() {
        // \mathbbm{1} approximated by \mathbb{1} -> 𝟙 (U+1D7D9).
        // MathML keeps the literal char under a double-struck mstyle; the
        // Unicode-math output materializes the actual blackboard glyph.
        let mathml = to_mathml(r"\mathbbm{1}");
        assert!(
            mathml.contains("double-struck") && mathml.contains('1'),
            "expected double-struck 1, got: {mathml}"
        );
        assert!(
            to_unicode(r"\mathbbm{1}").contains('\u{1D7D9}'),
            "expected 𝟙 in unicode-math, got: {}",
            to_unicode(r"\mathbbm{1}")
        );
    }

    #[test]
    fn mathbbm_letter_z() {
        // \mathbbm{Z} approximated by \mathbb{Z} -> ℤ (U+2124).
        let mathml = to_mathml(r"\mathbbm{Z}");
        assert!(
            mathml.contains("double-struck") && mathml.contains('Z'),
            "expected double-struck Z, got: {mathml}"
        );
        assert!(
            to_unicode(r"\mathbbm{Z}").contains('\u{2124}'),
            "expected ℤ in unicode-math, got: {}",
            to_unicode(r"\mathbbm{Z}")
        );
    }

    #[test]
    fn mathbbmss_and_mathbbmtt_approximate_bb() {
        // Sans-serif and typewriter variants also fall back to blackboard bold.
        assert!(to_unicode(r"\mathbbmss{Z}").contains('\u{2124}'));
        assert!(to_unicode(r"\mathbbmtt{Z}").contains('\u{2124}'));
    }

    #[cfg(feature = "render")]
    #[test]
    fn mathbbm_renders_svg() {
        let svg = crate::convert::from_latex(r"\mathbbm{Z}")
            .unwrap()
            .to_svg()
            .unwrap();
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn user_macro_overrides_builtin() {
        let mathml = to_mathml(r"\newcommand{\mathbbm}[1]{X#1}\mathbbm{Z}");
        assert!(mathml.contains('X'));
    }
}
