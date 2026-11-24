//! The LaTeX reader takes exactly one TeX token or one brace group as a
//! script argument, as TeX, KaTeX and MathJax do: `x^22` is `x^{2}2`.

use mathema::error::{Config, DiagnosticKind, Span, Strictness};
use mathema::{Format, convert, from_latex_with};

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

fn mathml(input: &str) -> String {
    convert(input, Format::LaTeX, Format::MathML).unwrap()
}

fn lenient() -> Config {
    let mut config = Config::default();
    config.strictness = Strictness::Lenient;
    config
}

/// Each input reads exactly as its braced spelling does.
const SAME_AS_BRACED: &[(&str, &str)] = &[
    (r"x^22", r"x^{2}2"),
    (r"x^2.5", r"x^{2}.5"),
    (r"x_ab", r"x_{a}b"),
    (r"x_12", r"x_{1}2"),
    (r"x^\alpha\beta", r"x^{\alpha}\beta"),
    (r"x_1^22", r"x_{1}^{2}2"),
    (r"x'^22", r"x'^{2}2"),
    (r"x^ 22", r"x^{2}2"),
    (r"x^\relax22", r"x^{2}2"),
    (r"{}^22X", r"{}^{2}2X"),
    (r"\sum_10^20 k", r"\sum_{1}0^{2}0k"),
    (r"\int\limits_01 f", r"\int\limits_{0}1f"),
    (r"\lim_10 a", r"\lim_{1}0a"),
    (r"x^αβ", r"x^{α}β"),
    // A control sequence is one token together with the arguments it
    // reads itself, each again one token or one group (KaTeX:
    // `\frac` and the font commands are `allowedInArgument`).
    (r"x^\frac12", r"x^{\frac{1}{2}}"),
    (r"x^\frac123", r"x^{\frac{1}{2}}3"),
    (r"x^\mathrm{ab}", r"x^{\mathrm{ab}}"),
    (r"x^\mathbf xy", r"x^{\mathbf{x}}y"),
    (r"x^\text{ab}", r"x^{\text{ab}}"),
    // KaTeX rejects `\sqrt` without braces here; mathema reads it as
    // MathJax does.
    (r"x^\sqrt2", r"x^{\sqrt{2}}"),
    (r"x^\sqrt22", r"x^{\sqrt{2}}2"),
    (r"x^\hat a b", r"x^{\hat{a}}b"),
];

#[test]
fn a_script_argument_is_one_tex_token() {
    for &(input, braced) in SAME_AS_BRACED {
        assert_eq!(mathml(input), mathml(braced), "{input} vs {braced}");
        assert_eq!(latex(input), latex(braced), "{input} vs {braced}");
    }
}

#[test]
fn written_examples() {
    assert_eq!(latex(r"x^22"), r"x^{2}2");
    assert_eq!(latex(r"x^2.5"), r"x^{2}.5");
    assert_eq!(latex(r"x^\frac12"), r"x^{\frac{1}{2}}");
    assert_eq!(latex(r"x^{22}"), r"x^{22}");
    assert_eq!(latex(r"x^{2.5}"), r"x^{2.5}");
}

#[test]
fn mathml_of_a_split_number() {
    let math = mathml(r"x^22");
    assert!(
        math.contains("<msup><mi>x</mi><mn>2</mn></msup><mn>2</mn>"),
        "{math}"
    );
    let math = mathml(r"x^2.5");
    assert!(
        math.contains("<msup><mi>x</mi><mn>2</mn></msup><mo>.</mo><mn>5</mn>"),
        "{math}"
    );
    let math = mathml(r"x^{22}");
    assert!(
        math.contains("<msup><mi>x</mi><mn>22</mn></msup>"),
        "{math}"
    );
}

/// The braced spellings still take the whole number in every script
/// position.
#[test]
fn braced_numbers_stay_whole() {
    let math = mathml(r"\sideset{_{12}}{^{34}}\sum");
    assert!(
        math.contains("<mn>12</mn>") && math.contains("<mn>34</mn>"),
        "{math}"
    );
    let math = mathml(r"\sum\limits_{10}^{20} k");
    assert!(
        math.contains("<mn>10</mn>") && math.contains("<mn>20</mn>"),
        "{math}"
    );
    let math = mathml(r"x'^{22}");
    assert!(math.contains("<mn>22</mn>"), "{math}");
}

/// The mhchem and siunitx rewrites brace the scripts they generate.
#[test]
fn package_rewrites_keep_whole_numbers() {
    for (input, number) in [
        (r"\ce{C6H12O6}", "<mn>12</mn>"),
        (r"\ce{^{227}_{90}Th}", "<mn>227</mn>"),
        (r"\ce{SO4^2-}", "<mn>2</mn>"),
        (r"\SI{3e45}{m}", "<mn>45</mn>"),
        (r"\SI{3}{m^{22}}", "<mn>22</mn>"),
        (r"\textsuperscript{12}", "<mn>12</mn>"),
    ] {
        let math = mathml(input);
        assert!(math.contains(number), "{input}: {math}");
    }
    let diagnostics = from_latex_with(r"\ce{C6H12O6} \SI{3e45}{m^2}", &lenient())
        .unwrap()
        .diagnostics()
        .to_vec();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn lenient_reports_a_split_number() {
    let cases: &[(&str, &str, Span)] = &[
        (
            r"x^22",
            "only the first digit `2` is the superscript; write `^{22}` to raise the whole number",
            Span::new(2, 4),
        ),
        (
            r"x_2.5",
            "only the first digit `2` is the subscript; write `_{2.5}` to lower the whole number",
            Span::new(2, 5),
        ),
        (
            r"a+x^10.25",
            "only the first digit `1` is the superscript; write `^{10.25}` to raise the whole number",
            Span::new(4, 9),
        ),
        (
            r"x'^22",
            "only the first digit `2` is the superscript; write `^{22}` to raise the whole number",
            Span::new(3, 5),
        ),
    ];
    for &(input, message, span) in cases {
        let result = from_latex_with(input, &lenient()).unwrap();
        let diagnostics = result.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{input}: {diagnostics:?}");
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Warning);
        assert_eq!(diagnostics[0].message, message, "{input}");
        assert_eq!(diagnostics[0].span, Some(span), "{input}");
    }
    let result = from_latex_with(r"\sum_10^20 k", &lenient()).unwrap();
    assert_eq!(result.diagnostics().len(), 2, "{:?}", result.diagnostics());
}

#[test]
fn no_report_for_one_digit_or_a_braced_number() {
    for input in [
        r"x^2", r"x^2.", r"x^2 2", r"x^{22}", r"x^{2.5}", r"x^2+1", r"x^2,3", r"x^a2",
    ] {
        let result = from_latex_with(input, &lenient()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
}

/// Strict mode accepts the TeX reading without a diagnostic.
#[test]
fn strict_accepts_a_split_number() {
    for input in [r"x^22", r"x_2.5", r"\sum_10^20 k"] {
        let result = from_latex_with(input, &Config::default()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn split_numbers_round_trip() {
    for &(input, _) in SAME_AS_BRACED {
        let first = latex(input);
        assert_eq!(latex(&first), first, "{input}");
        assert_eq!(mathml(&first), mathml(input), "{input}");
    }
}

#[cfg(feature = "render")]
#[test]
fn split_numbers_render_as_braced() {
    for (input, braced) in [
        (r"x^22", r"x^{2}2"),
        (r"x^2.5", r"x^{2}.5"),
        (r"x^\frac12", r"x^{\frac{1}{2}}"),
    ] {
        let svg = convert(input, Format::LaTeX, Format::Svg).unwrap();
        assert_eq!(
            svg,
            convert(braced, Format::LaTeX, Format::Svg).unwrap(),
            "{input}"
        );
        assert_ne!(
            svg,
            convert(r"x^{22}", Format::LaTeX, Format::Svg).unwrap(),
            "{input}"
        );
    }
}
