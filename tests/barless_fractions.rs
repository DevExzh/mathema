//! Barless fractions (`\atop`, `\brace`, `\brack`, `\genfrac` with a zero
//! rule) keep their missing bar through every reader and writer.

use mathema::ast::{FractionKind, MathNode};
use mathema::{from_latex, from_mathml, from_omml, from_unicode_math};

fn latex(input: &str) -> String {
    from_latex(input).unwrap().to_latex().unwrap()
}

fn mathml(input: &str) -> String {
    from_latex(input).unwrap().to_mathml().unwrap()
}

/// The LaTeX writer spells each barless fraction as `\genfrac` (a fenced one
/// with its delimiters), never as `\frac`, and the spelling is stable.
#[test]
fn latex_writer_spells_barless_fractions() {
    let cases = [
        (r"{Y+\atop z}", r"\genfrac{}{}{0pt}{}{Y+}{z}"),
        (r"{Y+\brace z}", r"\genfrac\{\}{0pt}{}{Y+}{z}"),
        (r"{Y+\brack z}", r"\genfrac[]{0pt}{}{Y+}{z}"),
        (r"{Y+\choose z}", r"\binom{Y+}{z}"),
        (r"\genfrac(){0pt}{}{Y+}{z}", r"\binom{Y+}{z}"),
        (r"\genfrac{}{}{0pt}{}{a}{b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (r"\genfrac\{\}{0pt}{}{a}{b}", r"\genfrac\{\}{0pt}{}{a}{b}"),
        (r"\genfrac{[}{]}{0 pt}{}{a}{b}", r"\genfrac[]{0pt}{}{a}{b}"),
        (r"\genfrac{}{}{0em}{}{a}{b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (r"\genfrac{}{}{.0mu}{}{a}{b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (r"x^{a\atop b}c", r"x^{\genfrac{}{}{0pt}{}{a}{b}}c"),
        (r"{a \above 0pt b}", r"\genfrac{}{}{0pt}{}{a}{b}"),
        (r"{a \above 1pt b}", r"\frac{a}{b}"),
        // Other delimiters stay a `\left`/`\right` pair around the fraction.
        (
            r"\genfrac{\langle}{|}{0pt}{}{a}{b}",
            r"\left\langle \genfrac{}{}{0pt}{}{a}{b} \right|",
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

/// A barless fraction inside a longer row does not capture its neighbours
/// (as an unbraced `\atop` would).
#[test]
fn barless_fraction_stays_scoped_in_a_row() {
    let written = latex(r"a+{b\atop c}+d");
    assert_eq!(written, r"a+ \genfrac{}{}{0pt}{}{b}{c}+ d");
    let mathml = mathml(&written);
    assert!(
        mathml.contains(
            r#"<mo>+</mo><mfrac linethickness="0"><mi>b</mi><mi>c</mi></mfrac><mo>+</mo>"#
        ),
        "{mathml}"
    );
}

/// `\genfrac` keeps its style argument and its default rule.
#[test]
fn genfrac_style_and_default_thickness() {
    let cases = [
        (r"\genfrac{}{}{}{}{a}{b}", r"\frac{a}{b}"),
        (r"\genfrac{}{}{}{0}{a}{b}", r"\dfrac{a}{b}"),
        (r"\genfrac{}{}{}{1}{a}{b}", r"\tfrac{a}{b}"),
        (r"\genfrac{}{}{}{2}{a}{b}", r"{\scriptstyle \frac{a}{b}}"),
        (
            r"\genfrac{}{}{}{3}{a}{b}",
            r"{\scriptscriptstyle \frac{a}{b}}",
        ),
        (r"\genfrac(){0pt}{0}{n}{k}", r"{\displaystyle \binom{n}{k}}"),
        (r"\genfrac(){0pt}{1}{n}{k}", r"{\textstyle \binom{n}{k}}"),
        (
            r"\genfrac[]{0pt}{3}{a}{b}",
            r"{\scriptscriptstyle \genfrac[]{0pt}{}{a}{b}}",
        ),
        (
            r"\genfrac(){}{1}{a}{b}",
            r"{\textstyle \left( \frac{a}{b} \right)}",
        ),
    ];
    for (input, expected) in cases {
        let written = latex(input);
        assert_eq!(written, expected, "input: {input}");
        assert_eq!(latex(&written), written, "unstable: {input}");
    }
    assert!(
        mathml(r"\genfrac{}{}{}{2}{a}{b}")
            .contains(r#"<mstyle displaystyle="false" scriptlevel="1"><mfrac><mi>a</mi>"#)
    );
    assert!(
        mathml(r"\genfrac[]{0pt}{0}{a}{b}")
            .contains(r#"<mstyle displaystyle="true" scriptlevel="0"><mrow><mo fence="true">[</mo><mfrac linethickness="0">"#)
    );
}

/// A nonzero thickness the AST cannot carry keeps the bar and says so.
#[test]
fn genfrac_custom_thickness_keeps_the_bar_with_a_warning() {
    let parsed = from_latex(r"\genfrac{}{}{2pt}{}{a}{b}").unwrap();
    assert_eq!(parsed.to_latex().unwrap(), r"\frac{a}{b}");
    assert!(parsed.to_mathml().unwrap().contains("<mfrac><mi>a</mi>"));
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("thickness `2pt`")),
        "{:?}",
        parsed.diagnostics()
    );
}

#[test]
fn mathml_carries_zero_linethickness() {
    for input in [
        r"{Y+\atop z}",
        r"\genfrac{}{}{0pt}{}{Y+}{z}",
        r"\genfrac(){0pt}{}{Y+}{z}",
        r"{Y+\brace z}",
    ] {
        let mathml = mathml(input);
        assert!(
            mathml.contains(r#"<mfrac linethickness="0">"#),
            "{input}: {mathml}"
        );
        // Writing LaTeX from that MathML keeps the fraction barless.
        let back = from_mathml(&mathml).unwrap().to_latex().unwrap();
        assert!(!back.contains(r"\frac"), "{input}: {back}");
    }
    assert!(!mathml(r"\genfrac{}{}{}{}{a}{b}").contains("linethickness"));
}

/// MathML spells a zero rule with any unit.
#[test]
fn mathml_reader_reads_zero_lengths_as_barless() {
    for thickness in ["0", "0px", "0pt", "0.0em", "-0ex", "0%"] {
        let input = format!(
            r#"<math><mfrac linethickness="{thickness}"><mi>a</mi><mi>b</mi></mfrac></math>"#
        );
        let parsed = from_mathml(&input).unwrap();
        assert!(
            matches!(
                parsed.expr().nodes.as_slice(),
                [MathNode::Fraction {
                    kind: FractionKind::NoBar,
                    ..
                }]
            ),
            "{thickness}"
        );
        assert_eq!(
            parsed.to_latex().unwrap(),
            r"\genfrac{}{}{0pt}{}{a}{b}",
            "{thickness}"
        );
    }
    for thickness in ["thin", "2px", "0.5em"] {
        let input = format!(
            r#"<math><mfrac linethickness="{thickness}"><mi>a</mi><mi>b</mi></mfrac></math>"#
        );
        assert_eq!(
            from_mathml(&input).unwrap().to_latex().unwrap(),
            r"\frac{a}{b}",
            "{thickness}"
        );
    }
}

#[test]
fn omml_round_trips_barless_fractions() {
    for (input, expected) in [
        (r"{Y+\atop z}", r"\genfrac{}{}{0pt}{}{Y+}{z}"),
        (r"{Y+\brace z}", r"\genfrac\{\}{0pt}{}{Y+}{z}"),
        (r"{Y+\brack z}", r"\genfrac[]{0pt}{}{Y+}{z}"),
    ] {
        let omml = from_latex(input).unwrap().to_omml().unwrap();
        assert!(omml.contains(r#"<m:type m:val="noBar"/>"#), "{omml}");
        assert_eq!(from_omml(&omml).unwrap().to_latex().unwrap(), expected);
    }
}

#[test]
fn unicode_math_spells_barless_fractions_with_broken_bar() {
    for (input, expected) in [
        (r"{a\atop b}", "a¦b"),
        (r"{a\brace b}", "{a¦b}"),
        (r"\binom{n}{k}", "(n¦k)"),
        (r"\frac{a}{b}", "a/b"),
    ] {
        let written = from_latex(input).unwrap().to_unicode_math().unwrap();
        assert_eq!(written, expected, "{input}");
        assert_eq!(
            from_unicode_math(&written).unwrap().to_latex().unwrap(),
            latex(input),
            "{input}"
        );
    }
}
