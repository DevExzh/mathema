//! UnicodeMath scripts (Unicode Technical Note 28) read back as what was
//! written: primes (section 3.10), leading scripts and empty bases (sections
//! 3.3 and 3.16), nested scripts (section 2.2), compound script bases, and
//! scripted, fenced and parenthesized fraction operands (section 2.1).

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
    // Repeated scripts nest right to left; a superscript may come before
    // the subscript (section 2.2). They are not leading scripts.
    ("x^2^3", "x^(2^3)", r"x^{2^{3}}"),
    ("a_b_c", "a_(b_c)", r"a_{b_c}"),
    ("a^b_c", "a_c^b", r"a_c^b"),
    // A script after a space has no base.
    ("x _1", "x〖〗_1", r"x{}_{1}"),
    // Primes after a subscript join the superscript, as in TeX; an operator
    // or a big operator takes them as in TeX too.
    ("x_1′", "x_1^′", r"x_{1}^{\prime}"),
    ("a+′", "a+ ′", r"a+ \prime"),
    ("∑′_i x", "∑_i^′ x", r"\sum\limits_i^{\prime} x"),
    // A fenced run before a fraction bar is the numerator.
    ("|x|^2/2", "(|x|^2)/2", r"\frac{|x|^{2}}{2}"),
    ("|x|/2", "(|x|)/2", r"\frac{|x|}{2}"),
    ("⟨x⟩^2/2", "(⟨x⟩^2)/2", r"\frac{\langle x{\rangle}^{2}}{2}"),
    // Parentheses around a numerator only group it (section 2.1).
    ("(a+b)/c", "(a+b)/c", r"\frac{a+ b}{c}"),
    ("((a+b))/c", "((a+b))/c", r"\frac{\left( a+b \right)}{c}"),
    // A prescript belongs to the operand, not to a fraction after it.
    ("(_1)a/b", "(_1)a/b", r"\frac{{{}_{1}a}}{b}"),
    ("_1 〖a/b〗", "(_1)〖a/b〗", r"{{}_{1}\frac{a}{b}}"),
    ("(_1)〖a/b〗", "(_1)〖a/b〗", r"{{}_{1}\frac{a}{b}}"),
    // A grouped denominator keeps its accent.
    (
        "x/〖ab〗̅",
        "x/〖ab〗̅",
        r"\frac{x}{\overset{̅}{\mathinner{ab}}}",
    ),
    // Empty operands are empty groups.
    ("\u{200B}/y", "〖〗/y", r"\frac{\mathinner{}}{y}"),
    ("√\u{200B}", "√〖〗", r"\sqrt{\mathinner{}}"),
    ("\u{200B}̂", "〖〗̂", r"\hat{\mathinner{}}"),
    // Adjacent numbers and a number after a fraction stay apart.
    ("1 12′", "1 12′", r"1{12}^{\prime}"),
    ("x/y^3 2", "x/y^3 2", r"\frac{x}{y^{3}}2"),
    // `\/` is a literal solidus.
    ("x\\/y", "x\\/y", r"x/ y"),
    // A word in a decorated group stays unquoted.
    (
        "〖〖ab〗┬n〗^2",
        "〖〖ab〗┬n〗^2",
        r"{\mathinner{\underset{n}{\mathinner{ab}}}}^{2}",
    ),
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
    "\\frac{x}{y^3}2",
    "{}_1\\frac{a}{b}",
    "\\frac{\\left(a+b\\right)}{c}",
    "x/{ab}",
    "+'",
];

/// LaTeX whose UnicodeMath spelling writes back the same.
const LATEX_STABLE: &[&str] = &[
    "\\frac{x}{y^3}2",
    "\\overline{ab}^2",
    "{}_1\\frac{a}{b}",
    "x/{ab}",
    "x^{2^3}",
    "a_{b_c}",
];

#[test]
fn latex_scripts_write_stable_unicode_math() {
    for &input in LATEX_STABLE {
        let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
        assert_eq!(unicode_math(&unicode), unicode, "{input}");
    }
}

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
