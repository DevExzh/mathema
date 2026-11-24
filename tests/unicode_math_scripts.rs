//! UnicodeMath scripts (Unicode Technical Note 28) read back as what was
//! written: primes (section 3.10), leading scripts and empty bases (sections
//! 3.3 and 3.16), compound script bases and scripted fraction operands.

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
    // Primes after a base are its superscript, merged with a superscript or
    // a subscript that follows.
    ("x′", "x′", r"x^{\prime}"),
    ("x^′", "x′", r"x^{\prime}"),
    ("x′′", "x′′", r"x^{\prime\prime}"),
    ("x^′′", "x′′", r"x^{\prime\prime}"),
    ("x′^2", "x′^2", r"x^{\prime2}"),
    ("x′_1", "x_1^′", r"x_{1}^{\prime}"),
    ("〖〗′", "〖〗′", r"{}^{\prime}"),
    ("′′", "′′", r"\prime\prime"),
    // A zero-width space is an empty operand (`x〖〗_1`).
    ("x\u{200B}_1", "x〖〗_1", r"x{}_{1}"),
    // A script with no base is a leading script of what follows.
    ("_1 F_1", "(_1)F_1", r"{{}_{1}F_{1}}"),
    // An empty leading script argument or base stays a group.
    ("〖〗_1〖〗", "(_1)〖〗", r"{{}_{1}\mathinner{}}"),
    ("〖〗^〖〗|", "(^〖〗)|", r"{{}^{\mathinner{}}|}"),
    ("x^〖〗", "x^〖〗", r"x^{\mathinner{}}"),
    // An invisible group around an empty-base script is kept.
    ("〖〖〗_1〗y", "〖〖〗_1〗y", r"\mathinner{{}_{1}}y"),
    // Scripts bind tighter than a fraction bar.
    ("x^2/y", "x^2/y", r"\frac{x^{2}}{y}"),
    ("a/b^2", "a/b^2", r"\frac{a}{b^{2}}"),
    ("a¦b_1", "a¦b_1", r"\genfrac{}{}{0pt}{}{a}{b_{1}}"),
];

#[test]
fn unicode_math_scripts_read_and_write_back() {
    for &(input, written, latex) in READS {
        assert_eq!(unicode_math(input), written, "{input}");
        assert_eq!(unicode_math(written), written, "{input}: {written}");
        assert_eq!(
            convert(written, Format::UnicodeMath, Format::LaTeX).unwrap(),
            latex,
            "{input}: {written}"
        );
    }
}

/// LaTeX whose UnicodeMath spelling reads back with the same meaning.
const LATEX: &[&str] = &[
    "x'",
    "x''",
    "x'^2",
    "x_1'",
    "x'_1",
    "x^{\\prime}",
    "{}'",
    "x^{'}",
    "a\\mathinner{}_1",
    "{}^{\\mathinner{}}|",
    "{}^{\\prime}y",
    "{}_1 xy",
    "{}_1 \\frac{a}{b}",
    "{a+b}^2",
    "\\frac{a}{b}_1",
    "{a \\atop b}^2",
    "\\frac{x^2}{y}",
    "\\frac{a}{b^2}",
    "\\frac{a'}{b'}",
    "{ab}_1",
    "x\\prime",
    "x_1\\prime",
    "\\mathinner{{}_1}y",
];

#[test]
fn latex_scripts_survive_unicode_math() {
    for &input in LATEX {
        let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
        let strip = |mathml: String| mathml.replacen(" display=\"block\"", "", 1);
        assert_eq!(
            mathml_body(&unicode, Format::UnicodeMath),
            strip(mathml_body(input, Format::LaTeX)),
            "{input}: {unicode}"
        );
    }
}

/// A prime that is not a superscript is set apart from the node before it,
/// which would otherwise take it as one.
#[test]
fn a_sibling_prime_is_not_written_as_a_superscript() {
    assert_eq!(
        convert("x\\prime", Format::LaTeX, Format::UnicodeMath).unwrap(),
        "x ′"
    );
    assert_eq!(
        convert("\\prime\\prime", Format::LaTeX, Format::UnicodeMath).unwrap(),
        "′′"
    );
}
