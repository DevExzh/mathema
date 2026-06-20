//! `amsmath` package support.
//!
//! Currently provides the variable-limit operators (`\varlimsup`, `\varliminf`,
//! `\varinjlim`, `\varprojlim`). These expand to a named limit operator (`\lim`)
//! decorated with an over/under line or arrow, matching amsmath's definitions:
//!
//! ```text
//! \varlimsup → \overline{\lim}        \varliminf → \underline{\lim}
//! \varinjlim → \underrightarrow{\lim} \varprojlim → \underleftarrow{\lim}
//! ```
//!
//! Scripts attached to these (e.g. `\varlimsup_{n\to\infty}`) are placed
//! above/below the decorated stack, because the layout treats a decorated limit
//! operator as a limit operator.

use super::SymbolClass;

/// Builtin macros contributed by `amsmath`: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("varlimsup", 0, r"\overline{\lim}"),
    ("varliminf", 0, r"\underline{\lim}"),
    ("varinjlim", 0, r"\underrightarrow{\lim}"),
    ("varprojlim", 0, r"\underleftarrow{\lim}"),
];

/// Symbol entries contributed by `amsmath`: none.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    fn roundtrip(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::LaTeX).expect("convert should succeed")
    }

    #[test]
    fn varlimsup_is_overline_lim() {
        let m = to_mathml(r"\varlimsup");
        assert!(m.contains("lim"), "expected lim operator in {m}");
        // Overline accent renders as an over-decoration (U+203E line).
        assert!(
            m.contains("mover") || m.contains('\u{203E}'),
            "expected overline decoration in {m}"
        );
    }

    #[test]
    fn varliminf_is_underline_lim() {
        let m = to_mathml(r"\varliminf");
        assert!(m.contains("lim"), "expected lim in {m}");
        assert!(
            m.contains("munder") || m.contains("under"),
            "expected underline in {m}"
        );
    }

    #[test]
    fn varinjlim_is_underrightarrow_lim() {
        let m = to_mathml(r"\varinjlim");
        assert!(m.contains("lim"), "expected lim in {m}");
        assert!(m.contains('\u{2192}'), "expected right arrow in {m}");
    }

    #[test]
    fn varprojlim_is_underleftarrow_lim() {
        let m = to_mathml(r"\varprojlim");
        assert!(m.contains("lim"), "expected lim in {m}");
        assert!(m.contains('\u{2190}'), "expected left arrow in {m}");
    }

    /// Regression for the `Below`-accent LaTeX writer bug: these must NOT
    /// round-trip to `\hat{\lim}`.
    #[test]
    fn varlim_family_roundtrips() {
        assert_eq!(roundtrip(r"\varlimsup"), r"\overline{\lim}");
        assert_eq!(roundtrip(r"\varliminf"), r"\underline{\lim}");
        assert_eq!(roundtrip(r"\varinjlim"), r"\underrightarrow{\lim}");
        assert_eq!(roundtrip(r"\varprojlim"), r"\underleftarrow{\lim}");
        for cmd in [r"\varinjlim", r"\varprojlim"] {
            assert!(!roundtrip(cmd).contains("hat"), "{cmd} regressed to \\hat");
        }
    }
}
