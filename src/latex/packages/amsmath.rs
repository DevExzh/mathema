//! `amsmath` package support.
//!
//! Currently provides the variable-limit operators (`\varlimsup`, `\varliminf`,
//! `\varinjlim`, `\varprojlim`). These expand to a named limit operator (`\lim`)
//! decorated with an over/under line or arrow, matching amsmath's definitions:
//!
//! ```text
//! \varlimsup → \mathop{\overline{\lim}}        \varliminf → \mathop{\underline{\lim}}
//! \varinjlim → \mathop{\underrightarrow{\lim}} \varprojlim → \mathop{\underleftarrow{\lim}}
//! ```
//!
//! Scripts attached to these (e.g. `\varlimsup_{n\to\infty}`) are placed
//! above/below the decorated stack in display style and at the side in inline
//! style, because the expansion uses a movable compound operator.

use super::SymbolClass;

/// Builtin macros contributed by `amsmath`: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("varlimsup", 0, r"\mathop{\overline{\lim}}"),
    ("varliminf", 0, r"\mathop{\underline{\lim}}"),
    ("varinjlim", 0, r"\mathop{\underrightarrow{\lim}}"),
    ("varprojlim", 0, r"\mathop{\underleftarrow{\lim}}"),
    ("plim", 0, r"\mathop{\operatorname{plim}}\limits"),
];

/// Symbol entries contributed by `amsmath`: none.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::ast::DisplayStyle;
    use crate::error::Config;
    use crate::{Format, convert, convert_with};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    fn roundtrip(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::LaTeX).expect("convert should succeed")
    }

    fn to_mathml_style(latex: &str, display: DisplayStyle) -> String {
        convert_with(
            latex,
            Format::LaTeX,
            Format::MathML,
            &Config::default().with_display(display),
        )
        .expect("convert should succeed")
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

    #[test]
    fn variable_limit_scripts_are_movable_compound_operators() {
        for (display, outer) in [
            (DisplayStyle::Inline, "<msub><mrow data-mjx-texclass=\"OP\""),
            (
                DisplayStyle::Block,
                "<munder><mrow data-mjx-texclass=\"OP\"",
            ),
        ] {
            for cmd in [
                r"\varlimsup_i",
                r"\varliminf_i",
                r"\varinjlim_i",
                r"\varprojlim_i",
            ] {
                let output = to_mathml_style(cmd, display);
                assert!(output.contains(outer), "{display:?} {cmd}: {output}");
                assert!(
                    output.contains("movable\""),
                    "movable operator metadata missing for {cmd}: {output}"
                );
            }
            for cmd in [r"\underrightarrow{\lim}_i", r"\overset{¯}{\lim}_i"] {
                let output = to_mathml_style(cmd, display);
                assert!(
                    output.contains("<msub><munder") || output.contains("<msub><mover"),
                    "literal accent should keep side scripts for {cmd}: {output}"
                );
            }
        }
    }

    #[test]
    fn plim_direct_path_matches_the_builtin_expansion() {
        for source in [
            r"\plim",
            r"\plim_i x",
            r"\plim\limits_{n\to\infty}X_n",
            r"\plim\nolimits_{n\to\infty}X_n",
            r"x+\plim_{n}y",
        ] {
            let explicit = source.replacen(r"\plim", r"\mathop{\operatorname{plim}}\limits", 1);
            assert_eq!(to_mathml(source), to_mathml(&explicit), "{source}");
        }
    }

    #[test]
    fn plim_expansion_reaches_raw_arguments_and_the_public_preprocessor() {
        let expanded = crate::latex::preprocess::preprocess_cow(r"\plim").unwrap();
        assert_eq!(expanded, r"\mathop{\operatorname{plim}}\limits");
        for source in [r"\text{rate \plim}+x", r"\operatorname{\plim}x"] {
            let explicit = source.replace(r"\plim", &expanded);
            assert_eq!(to_mathml(source), to_mathml(&explicit), "{source}");
        }
    }

    #[test]
    fn expanded_operators_contribute_their_printed_name() {
        for (source, expected) in [
            (r"\operatorname{\plim}+x", r"\operatorname{plim}+x"),
            (r"\operatorname{A\plim B}+x", r"\operatorname{AplimB}+x"),
            (
                r"\def\F{\mathop{\operatorname* {a\,b}}\nolimits}\operatorname{A\F B}",
                r"\operatorname{Aa\,bB}",
            ),
            (
                r"\def\plim{Z}\operatorname{A\plim B}",
                r"\operatorname{AZB}",
            ),
        ] {
            assert_eq!(to_mathml(source), to_mathml(expected), "{source}");
        }
        // A defined macro protected outside an expanded definition acts as
        // \relax in TeX. The next math atom remains available to the parser.
        assert_eq!(to_mathml(r"\noexpand\plim x"), to_mathml("x"));
    }

    #[test]
    fn plim_definitions_still_use_macro_expansion() {
        let redefined = crate::convert(
            r"\newcommand{\plim}{Z}\plim",
            crate::Format::LaTeX,
            crate::Format::LaTeX,
        )
        .unwrap();
        assert_eq!(redefined, "Z");

        let let_bound = crate::convert(
            r"\let\foo\plim\foo_i x",
            crate::Format::LaTeX,
            crate::Format::MathML,
        )
        .unwrap();
        assert!(let_bound.contains("plim"), "{let_bound}");
    }
}
