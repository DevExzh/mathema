//! UnicodeMath operands and operators (Unicode Technical Note 28): simple
//! script operands (section 2.2), signed script operands (appendix A
//! `soperand`), the operators of table 3.1 (`·`, `*`), negated operators
//! (sections 2.1 and 4.1), function names in an n-aryand (sections 3.4 and
//! 3.5) and the space characters of section 3.16.

use mathema::{Format, convert};

fn unicode_math(input: &str) -> String {
    convert(input, Format::UnicodeMath, Format::UnicodeMath).unwrap()
}

fn mathml_body(input: &str, from: Format) -> String {
    let mathml = convert(input, from, Format::MathML).unwrap();
    let start = mathml.find('>').unwrap() + 1;
    let end = mathml.rfind("</math>").unwrap();
    mathml[start..end].to_owned()
}

/// UnicodeMath input, its UnicodeMath spelling and its LaTeX.
const READS: &[(&str, &str, &str)] = &[
    // Section 2.2: "A simple subscript operand consists of the string of
    // one or more characters with the Unicode General Categories Lx
    // (alphabetic) and Nd (decimal digits)".
    ("a_ij", "a_ij", r"a_{ij}"),
    ("δ_μν", "δ_μν", r"\delta_{\mu\nu}"),
    ("x_12", "x_12", r"x_{12}"),
    ("x_2n", "x_2n", r"x_{2n}"),
    ("x^2y", "x^2y", r"x^{2y}"),
    ("x_ij^2", "x_ij^2", r"x_{ij}^{2}"),
    ("∑_ij a_ij", "∑_ij a_ij", r"\sum\limits_{ij} {a_{ij}}"),
    ("(_ij)F", "(_ij)F", r"{{}_{ij}F}"),
    // An operator ends it.
    ("a_i+1", "a_i+1", r"a_i+ 1"),
    // A nested script takes the last letter as its base.
    ("a_ij_k", "a_(ij_k)", r"a_{i{j}_k}"),
    // Appendix A, `soperand ← '-' operand`: a sign starts a script operand.
    ("e^-x", "e^-x", r"e^{- x}"),
    ("x^-1", "x^-1", r"x^{- 1}"),
    ("x^-∞", "x^-∞", r"x^{- \infty}"),
    ("e^-x/2", "e^-x/2", r"\frac{e^{- x}}{2}"),
    // Parentheses after the sign are visible: only those around the whole
    // operand group it.
    ("x^-(a+b)", "x^(-(a+b))", r"x^{- \left( a+b \right)}"),
    // A sign alone is the operand.
    ("x^- y", "x^- y", r"x^-y"),
    ("ℝ^+", "ℝ^+", r"ℝ^+"),
    // Table 3.1: `·` and `*` are operators with the precedence of `+`.
    ("a·b/c", "a·b/c", r"a\centerdot \frac{b}{c}"),
    ("a*b/c", "a*b/c", r"a* \frac{b}{c}"),
    ("∮ E·ⅆl", "∮ E·ⅆl", r"\oint\nolimits E\centerdot ⅆl"),
    // Section 4.1: `/` before an operator negates it.
    ("a/=b", "a≠b", r"a\neq b"),
    ("a/<b", "a≮b", r"a\nless b"),
    ("a/>b", "a≯b", r"a\ngtr b"),
    ("a/∈B", "a∉B", r"a\notin B"),
    ("a/≡b", "a≢b", r"a\nequiv b"),
    ("a/≤b", "a≰b", r"a\nleq b"),
    ("A/⊂B", "A⊄B", r"A\nsubset B"),
    // Section 3.5: a function name and its argument are one n-aryand; the
    // differential ⅆ ends the integrand (section 3.4).
    ("∫ sin x ⅆx", "∫ sin xⅆx", r"\int\nolimits {\sin xⅆx}"),
    ("∑ log k", "∑ log k", r"\sum\limits {\log k}"),
    ("∑_n sin^2 x", "∑_n sin^2 x", r"\sum\limits_n {\sin^{2}x}"),
    (
        "∫_0^1 x^2 ⅆx",
        "∫_0^1 x^2 ⅆx",
        r"\int\nolimits_{0}^{1} {x^{2}ⅆx}",
    ),
    // A space after the argument still ends the n-aryand.
    ("∫ sin x dx", "∫ sin x dx", r"\int\nolimits {\sin x}dx"),
    // The argument of a function outside an n-aryand is unchanged.
    ("sin x/2", "sin x/2", r"\sin \frac{x}{2}"),
    (
        "lim_(n→∞) sin x",
        "lim_(n→∞) sin x",
        r"\lim_{n\rightarrow \infty}\sin x",
    ),
    // Section 3.16: the other spaces are not removed on build up.
    ("x\u{2009}dx", "x\u{2009}dx", r"x\,dx"),
    ("a\u{2005}b", "a\u{2005}b", r"a\;b"),
    ("a\u{2003}b", "a\u{2003}b", r"a\quad b"),
];

#[test]
fn unicode_math_operands_read_and_write_back() {
    for &(input, written, latex) in READS {
        assert_eq!(unicode_math(input), written, "{input}");
        assert_eq!(unicode_math(written), written, "{input}: {written}");
        assert_eq!(
            convert(input, Format::UnicodeMath, Format::LaTeX).unwrap(),
            latex,
            "{input}"
        );
        assert_eq!(
            mathml_body(written, Format::UnicodeMath),
            mathml_body(input, Format::UnicodeMath),
            "{input}: {written}"
        );
    }
}

/// LaTeX whose UnicodeMath spelling reads back as the same MathML.
const LATEX: &[(&str, &str)] = &[
    ("\\int x\\,dx", "∫ x\u{2009}dx"),
    ("x\\,dx", "x\u{2009}dx"),
    ("a\\:b", "a\u{205F}b"),
    ("a\\;b", "a\u{2005}b"),
    ("a\\quad b", "a\u{2003}b"),
    ("a_{ij}", "a_ij"),
    ("x_{2n}", "x_2n"),
    ("x^{2y}", "x^2y"),
    ("x^2 y", "x^2 y"),
    ("a_i j", "a_i j"),
    ("a \\neq b", "a≠b"),
    ("a \\cdot b", "a⋅b"),
    ("\\left(x_i y\\right)", "(x_i y)"),
];

#[test]
fn latex_operands_survive_unicode_math() {
    for &(input, written) in LATEX {
        let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
        assert_eq!(unicode, written, "{input}");
        assert_eq!(unicode_math(&unicode), unicode, "{input}");
        let strip = |mathml: String| mathml.replacen(" display=\"block\"", "", 1);
        assert_eq!(
            mathml_body(&unicode, Format::UnicodeMath),
            strip(mathml_body(input, Format::LaTeX)),
            "{input}: {unicode}"
        );
    }
}

/// LaTeX `\int \sin x \, dx` keeps the thin space before `dx`, which would
/// otherwise read as part of one word `xdx`.
#[test]
fn a_thin_space_keeps_the_differential_apart() {
    let unicode = convert("\\int \\sin x \\, dx", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert_eq!(unicode, "∫ sin x\u{2009}dx");
    let mathml = mathml_body(&unicode, Format::UnicodeMath);
    assert!(mathml.contains("<mspace"), "{mathml}");
    assert!(!mathml.contains("xdx"), "{mathml}");
}

/// A script that is only a sign is set apart from an operand after it.
#[test]
fn a_sign_script_is_set_apart_from_an_operand() {
    for (input, written) in [("x^-(y)", "x^− (y)"), ("x^- 1", "x^− 1")] {
        let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
        assert_eq!(unicode, written, "{input}");
        // The superscript is the sign alone.
        let mathml = mathml_body(&unicode, Format::UnicodeMath);
        let sup = &mathml[mathml.find("<msup>").unwrap()..mathml.find("</msup>").unwrap()];
        assert!(!sup.contains("<mrow>"), "{input}: {mathml}");
    }
}
