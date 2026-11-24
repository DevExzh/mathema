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
    // A numerator or denominator is the whole operand (section 2.1), so a
    // function or product keeps its parentheses before `/`.
    ("f(x)/2", "(f(x))/2", r"\frac{f\left( x \right)}{2}"),
    (
        "P(A)/P(B)",
        "(P(A))/(P(B))",
        r"\frac{P\left( A \right)}{P\left( B \right)}",
    ),
    ("n(n+1)/2", "(n(n+1))/2", r"\frac{n\left( n+1 \right)}{2}"),
    ("sin(x)/x", "(sin(x))/x", r"\frac{\sin \left( x \right)}{x}"),
    (
        "Γ(n)/n!",
        "(Γ(n))/(n!)",
        r"\frac{\Gamma\left( n \right)}{n!}",
    ),
    ("2(a+b)/c", "(2(a+b))/c", r"\frac{2\left( a+b \right)}{c}"),
    (
        "f′(x)/2",
        "(f′(x))/2",
        r"\frac{f^{\prime}\left( x \right)}{2}",
    ),
    ("abc/d", "abc/d", r"\frac{abc}{d}"),
    ("a+(b)/c", "a+b/c", r"a+ \frac{b}{c}"),
    ("a+b/c d", "a+b/c d", r"a+ \frac{b}{c}d"),
    ("a/b/c", "(a/b)/c", r"\frac{\frac{a}{b}}{c}"),
    ("∑ a/b", "∑ a/b", r"\sum\limits {\frac{a}{b}}"),
    // A prescript is an operand wherever one is read.
    (
        "∫_0^1 (_1)F_1",
        "∫_0^1 (_1)F_1",
        r"\int\nolimits_{0}^{1} {{}_{1}F_{1}}",
    ),
    ("√(_1)F", "√((_1)F)", r"\sqrt{{{}_{1}F}}"),
    ("x/(_1)F", "x/(_1)F", r"\frac{x}{{{}_{1}F}}"),
    ("▭(_1)F", "▭((_1)F)", r"\boxed{{{}_{1}F}}"),
    // A quote in text is `\"` (section 3.15).
    ("\"a\\\"b\"", "\"a\\\"b\"", r#"\text{a"b}"#),
    // The n-aryand is the whole operand after the operator (section 3.4).
    (
        "∫_0^1 f(x)/x ⅆx",
        "∫_0^1 (f(x))/x ⅆx",
        r"\int\nolimits_{0}^{1} {\frac{f\left( x \right)}{x}}ⅆx",
    ),
    (
        "∫ f(x)/2",
        "∫ (f(x))/2",
        r"\int\nolimits {\frac{f\left( x \right)}{2}}",
    ),
    (
        "∑ f(x)/2",
        "∑ (f(x))/2",
        r"\sum\limits {\frac{f\left( x \right)}{2}}",
    ),
    (
        "∫ f(x)/g(x)",
        "∫ (f(x))/(g(x))",
        r"\int\nolimits {\frac{f\left( x \right)}{g\left( x \right)}}",
    ),
    (
        "∫ 2(a+b)/2",
        "∫ (2(a+b))/2",
        r"\int\nolimits {\frac{2\left( a+b \right)}{2}}",
    ),
    ("∑_i a_i", "∑_i a_i", r"\sum\limits_i {a_i}"),
    (
        "∑_i a_i b_i+1",
        "∑_i a_i b_i+1",
        r"\sum\limits_i {a_i}b_i+ 1",
    ),
    ("|∑_i a_i|", "|∑_i a_i|", r"|\sum\limits_i {a_i}|"),
    ("∫∫ f", "∫ ∫ f", r"\int\nolimits {\int\nolimits f}"),
    // Parentheses around an n-aryand are kept (section 3.4).
    ("∑ (a+b)", "∑ (a+b)", r"\sum\limits {\left( a+b \right)}"),
    (
        "∏_k (1+x_k)",
        "∏_k (1+x_k)",
        r"\prod\limits_k {\left( 1+x_k \right)}",
    ),
    (
        "∑_(k=0)^n (n¦k) a^k b^(n-k)",
        "∑_(k=0)^n (n¦k) a^k b^(n-k)",
        r"\sum\limits_{k= 0}^n {\binom{n}{k}}a^k{b}^{n- k}",
    ),
    ("∑ 〖a+b〗", "∑ 〖a+b〗", r"\sum\limits {a+ b}"),
    ("∑_i′ a", "∑_i^′ a", r"\sum\limits_i^{\prime} a"),
    // In text, `\\` is a backslash; a backslash before the last quote ends
    // the text, and any other backslash is literal.
    ("\"C:\\\"", "\"C:\\\\\"", r"\text{C:\textbackslash{}}"),
    ("\"a\\b\"", "\"a\\\\b\"", r"\text{a\textbackslash{}b}"),
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
    "\\frac{a}{b}c",
    "a\\frac{b}{c}",
    "\\text{a\"b}",
    "\\text{C:\\textbackslash}+\\text{x}",
    "\\text{a\\textbackslash}x\\text{b}",
];

/// LaTeX whose UnicodeMath spelling writes back the same.
const LATEX_STABLE: &[&str] = &[
    "\\frac{x}{y^3}2",
    "\\overline{ab}^2",
    "{}_1\\frac{a}{b}",
    "x/{ab}",
    "x^{2^3}",
    "a_{b_c}",
    "\\frac{f(x)}{2}",
    "\\frac{a}{b}c",
    "a\\frac{b}{c}",
    "\\text{a\"b}",
    "\\text{C:\\textbackslash}+\\text{x}",
    "\\text{a\\textbackslash}x\\text{b}",
    "\\sum_i a_i",
    "\\int_0^1 \\frac{f(x)}{x} dx",
    "\\prod_k \\left(1+x_k\\right)",
    "\\sum_k \\binom{n}{k}",
    "\\sum_i (a_i+b_i)",
];

/// LaTeX n-aryands in parentheses keep a single pair.
#[test]
fn latex_naryand_parentheses_stay_single() {
    for (input, written) in [
        ("\\prod_k \\left(1+x_k\\right)", "∏_k (1+x_k)"),
        ("\\sum_k \\binom{n}{k}", "∑_k (n¦k)"),
        ("\\sum_i (a_i+b_i)", "∑_i (a_i+b_i)"),
    ] {
        assert_eq!(
            convert(input, Format::LaTeX, Format::UnicodeMath).unwrap(),
            written,
            "{input}"
        );
        let latex = convert(written, Format::UnicodeMath, Format::LaTeX).unwrap();
        assert!(
            latex.contains('(') || latex.contains("binom"),
            "{written}: {latex}"
        );
    }
}

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
