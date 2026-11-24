//! TeX's delimited generalized fractions (`\overwithdelims`,
//! `\atopwithdelims`, `\abovewithdelims`) and the null delimiter `.` read as
//! the equivalent `\genfrac`.

use mathema::{Config, Strictness};
use mathema::{from_latex, from_mathml};

fn latex(input: &str) -> String {
    from_latex(input).unwrap().to_latex().unwrap()
}

fn mathml(input: &str) -> String {
    from_latex(input).unwrap().to_mathml().unwrap()
}

/// Each infix form parses to the same AST as its `\genfrac` spelling.
#[test]
fn withdelims_reads_as_genfrac() {
    let cases = [
        (r"{a \overwithdelims() b}", r"\genfrac(){}{}{a}{b}"),
        (r"{a\overwithdelims\{\}b}", r"\genfrac\{\}{}{}{a}{b}"),
        (r"{a \overwithdelims ( ) b}", r"\genfrac(){}{}{a}{b}"),
        (
            r"{a \overwithdelims\langle\rangle b}",
            r"\genfrac\langle\rangle{}{}{a}{b}",
        ),
        (r"{a \overwithdelims<> b}", r"\genfrac<>{}{}{a}{b}"),
        (r"{a \atopwithdelims[] b}", r"\genfrac[]{0pt}{}{a}{b}"),
        (r"{n \atopwithdelims() k}", r"\binom{n}{k}"),
        (r"{n \atopwithdelims() k}", r"{n \choose k}"),
        (
            r"{a \abovewithdelims\{\} 0pt b}",
            r"\genfrac\{\}{0pt}{}{a}{b}",
        ),
        (r"{a \abovewithdelims[] 0.0em b}", r"{a \brack b}"),
        (r"{a \abovewithdelims\{\} 2pt b}", r"\genfrac\{\}{}{}{a}{b}"),
        (r"{a \overwithdelims.) b}", r"\genfrac.){}{}{a}{b}"),
        (r"{a \overwithdelims.. b}", r"{a \over b}"),
        (r"{a \atopwithdelims.. b}", r"{a \atop b}"),
        (r"{a \abovewithdelims|. 0pt b}", r"\genfrac|.{0pt}{}{a}{b}"),
        (r"x+a \overwithdelims() b", r"\genfrac(){}{}{x+a}{b}"),
    ];
    for (input, equivalent) in cases {
        let parsed = from_latex(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            parsed.expr(),
            from_latex(equivalent).unwrap().expr(),
            "{input} vs {equivalent}"
        );
        assert!(
            !parsed.to_mathml().unwrap().contains("__infix"),
            "{input}: marker leaked"
        );
    }
}

/// The LaTeX writer writes them back stably, without the infix form.
#[test]
fn withdelims_latex_round_trip() {
    let cases = [
        (r"{a \overwithdelims() b}", r"\left( \frac{a}{b} \right)"),
        (r"{a \atopwithdelims[] b}", r"\genfrac[]{0pt}{}{a}{b}"),
        (r"{a \atopwithdelims() b}", r"\binom{a}{b}"),
        (
            r"{a \abovewithdelims\{\} 2pt b}",
            r"\left\{ \frac{a}{b} \right\}",
        ),
        (
            r"{a \abovewithdelims\{\} 0pt b}",
            r"\genfrac\{\}{0pt}{}{a}{b}",
        ),
        (r"{a \overwithdelims.) b}", r"\left. \frac{a}{b} \right)"),
        (r"{a \atopwithdelims.. b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (
            r"x^{a \overwithdelims<> b}",
            r"x^{\left\langle \frac{a}{b} \right\rangle}",
        ),
    ];
    for (input, expected) in cases {
        let written = latex(input);
        assert_eq!(written, expected, "input: {input}");
        assert_eq!(latex(&written), written, "unstable: {input}");
        assert_eq!(
            from_latex(&written).unwrap().expr(),
            from_latex(input).unwrap().expr(),
            "AST changed: {input}"
        );
    }
}

/// MathML: `<mfrac>` (barless with `linethickness="0"`) between fences.
#[test]
fn withdelims_mathml() {
    let cases = [
        (
            r"{a \overwithdelims() b}",
            r#"<mrow><mo fence="true">(</mo><mfrac><mi>a</mi><mi>b</mi></mfrac><mo fence="true">)</mo></mrow>"#,
        ),
        (
            r"{a \atopwithdelims[] b}",
            r#"<mrow><mo fence="true">[</mo><mfrac linethickness="0"><mi>a</mi><mi>b</mi></mfrac><mo fence="true">]</mo></mrow>"#,
        ),
        (
            r"{a \abovewithdelims\{\} 0pt b}",
            r#"<mrow><mo fence="true">{</mo><mfrac linethickness="0"><mi>a</mi><mi>b</mi></mfrac><mo fence="true">}</mo></mrow>"#,
        ),
        (
            r"{a \atopwithdelims.. b}",
            r#"<mfrac linethickness="0"><mi>a</mi><mi>b</mi></mfrac>"#,
        ),
    ];
    for (input, expected) in cases {
        let out = mathml(input);
        assert!(out.contains(expected), "{input}: {out}");
        // Reading that MathML back keeps the fences and the (missing) rule.
        assert_eq!(
            from_mathml(&out).unwrap().to_latex().unwrap(),
            latex(input),
            "{input}"
        );
    }
}

/// `.` in a `\genfrac` delimiter slot is the null delimiter, not a period.
#[test]
fn genfrac_dot_is_the_null_delimiter() {
    let cases = [
        (r"\genfrac{.}{.}{}{}{a}{b}", r"\frac{a}{b}"),
        (r"\genfrac..{0pt}{}{a}{b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (r"\genfrac.){}{}{a}{b}", r"\left. \frac{a}{b} \right)"),
        (r"\genfrac{(}{.}{}{}{a}{b}", r"\left( \frac{a}{b} \right."),
        (
            r"\genfrac{.}{]}{0pt}{}{a}{b}",
            r"\left. \genfrac{}{}{0pt}{}{a}{b} \right]",
        ),
    ];
    for (input, expected) in cases {
        let written = latex(input);
        assert_eq!(written, expected, "input: {input}");
        assert_eq!(latex(&written), written, "unstable: {input}");
        assert!(!mathml(input).contains("<mo>.</mo>"), "{input}");
        assert!(
            !mathml(input).contains(r#"<mo fence="true">.</mo>"#),
            "{input}"
        );
    }
    assert_eq!(
        from_latex(r"\genfrac.){}{}{a}{b}").unwrap().expr(),
        from_latex(r"\left. \frac{a}{b} \right)").unwrap().expr()
    );
}

/// `\above` and `\abovewithdelims` warn about a nonzero thickness the AST
/// cannot carry, as `\genfrac` does; a zero one is silent.
#[test]
fn above_custom_thickness_warns_like_genfrac() {
    let mut lenient = Config::default();
    lenient.strictness = Strictness::Lenient;
    for (input, command) in [
        (r"{a \above 2pt b}", r"\above rule thickness `2pt`"),
        (
            r"{a \abovewithdelims() 1.5em b}",
            r"\abovewithdelims rule thickness `1.5em`",
        ),
        (
            r"\genfrac{}{}{2pt}{}{a}{b}",
            r"\genfrac rule thickness `2pt`",
        ),
    ] {
        for parsed in [
            from_latex(input).unwrap(),
            mathema::from_latex_with(input, &lenient).unwrap(),
        ] {
            assert!(
                parsed
                    .diagnostics()
                    .iter()
                    .any(|d| d.message.contains(command)),
                "{input}: {:?}",
                parsed.diagnostics()
            );
            assert!(parsed.to_mathml().unwrap().contains("<mfrac><mi>a</mi>"));
        }
    }
    for input in [r"{a \above 0pt b}", r"{a \abovewithdelims() 0pt b}"] {
        assert!(
            from_latex(input).unwrap().diagnostics().is_empty(),
            "{input}"
        );
    }
}

/// A missing delimiter is an error in strict mode (TeX: "Missing
/// delimiter"), never a leaked marker.
#[test]
fn withdelims_needs_two_delimiters() {
    for input in [r"{a \overwithdelims b}", r"{a \atopwithdelims( b}"] {
        assert!(from_latex(input).is_err(), "{input}");
    }
}
