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

fn expect(found: &str, wanted: &str) -> Result<(), String> {
    if found == wanted {
        Ok(())
    } else {
        Err(format!("found {found:?}, wanted {wanted:?}"))
    }
}

/// Run `check_row` on every row and report all that fail.
fn check<T: std::fmt::Debug>(rows: &[T], check_row: impl Fn(&T) -> Result<(), String>) {
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            check_row(row)
                .err()
                .map(|error| format!("{row:?}: {error}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} rows failed:\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
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
    // An operator ends it, and so does a function name (section 3.5:
    // function names "should be recognized as such").
    ("a_i+1", "a_i+1", r"a_i+ 1"),
    ("x^2sin x", "x^2 sin x", r"x^{2}\sin x"),
    ("x_1sin x", "x_1 sin x", r"x_{1}\sin x"),
    ("x_2cos θ", "x_2 cos θ", r"x_{2}\cos \theta"),
    // Only a whole span of letters is a function name.
    ("e^isin θ", "e^isin θ", r"e^{isin}\theta"),
    ("x_kmax", "x_kmax", r"x_{kmax}"),
    ("x_rmin", "x_rmin", r"x_{rmin}"),
    ("x_admin", "x_admin", r"x_{admin}"),
    ("x_basin", "x_basin", r"x_{basin}"),
    ("x_max1/y", "x_max1/y", r"\frac{x_{max1}}{y}"),
    ("x^1sin1", "x^1 s in1", r"x^{1}sin1"),
    ("x_max", "x_max", r"x_{\max}"),
    // A run is one operand of a bound too.
    (
        "a+′┴nⅆx^′′",
        "a+〖′┴nⅆx〗′′",
        r"a+ {\overset{nⅆx}{\prime}}^{\prime\prime}",
    ),
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
    // A script after the space has no base: it is not the argument.
    ("∑ sin _i", "∑ 〖sin〗 〖〗_i", r"\sum\limits {\sin}{}_{i}"),
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
    check(READS, |&(input, written, latex)| {
        expect(&unicode_math(input), written)?;
        expect(&unicode_math(written), written)?;
        expect(
            &convert(input, Format::UnicodeMath, Format::LaTeX).unwrap(),
            latex,
        )?;
        expect(
            &mathml_body(written, Format::UnicodeMath),
            &mathml_body(input, Format::UnicodeMath),
        )
    });
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
    check(LATEX, |&(input, written)| {
        let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
        expect(&unicode, written)?;
        expect(&unicode_math(&unicode), written)?;
        expect(
            &mathml_body(&unicode, Format::UnicodeMath),
            &mathml_body(input, Format::LaTeX),
        )
    });
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
    check(
        &[("x^-(y)", "x^− (y)"), ("x^- 1", "x^− 1")],
        |&(input, written)| {
            let unicode = convert(input, Format::LaTeX, Format::UnicodeMath).unwrap();
            expect(&unicode, written)?;
            // The superscript is the sign alone.
            let mathml = mathml_body(&unicode, Format::UnicodeMath);
            let sup = &mathml[mathml.find("<msup>").unwrap()..mathml.find("</msup>").unwrap()];
            if sup.contains("<mrow>") {
                return Err(mathml);
            }
            Ok(())
        },
    );
}

/// UnicodeMath has no negative space: LaTeX `\!` is a zero-width space
/// (section 3.16), which keeps the operands apart, or an empty operand.
#[test]
fn a_negative_space_is_dropped() {
    check(
        &[
            ("x\\!y", "x\u{200B}y"),
            ("\\frac{a\\!b}{c}", "(a\u{200B}b)/c"),
            ("x_{a\\!b}", "x_(a\u{200B}b)"),
            ("\\sqrt{a\\!b}", "√(a\u{200B}b)"),
            ("x^{\\!}", "x^〖〗"),
            ("\\sqrt{\\!}", "√〖〗"),
        ],
        |&(latex, written)| {
            let unicode = convert(latex, Format::LaTeX, Format::UnicodeMath).unwrap();
            let mathml = mathml_body(&unicode, Format::UnicodeMath);
            expect(&unicode, written)?;
            expect(&unicode_math(&unicode), written)?;
            if mathml.contains("mspace") || mathml.contains("fence") || mathml.contains(">ab<") {
                return Err(mathml);
            }
            Ok(())
        },
    );
}

/// LaTeX scripts that spell function names read back whole.
#[test]
fn latex_scripts_with_function_names_survive() {
    check(
        &[
            ("x_{max1}", "x_max1"),
            ("T_{min0}", "T_min0"),
            ("a_{log10}", "a_log10"),
            ("x_{sin2}y", "x_sin2 y"),
            ("x_{kmax}", "x_kmax"),
            ("x_{max}", "x_(m ax)"),
            ("x_{1sin}", "x_(1s in)"),
            ("x_{1sin1}", "x_(1s in1)"),
            ("\\frac{kmax}{2}", "(km ax)/2"),
            // A script before an empty group or in a prescripted base ends
            // before the next letter.
            ("\\sideset{_1}{}\\alpha_1^2z", "(_1)α_1^2 z"),
            ("\\sum =^max", "∑ =^m ax"),
        ],
        |&(latex, written)| {
            let unicode = convert(latex, Format::LaTeX, Format::UnicodeMath).unwrap();
            expect(&unicode, written)?;
            expect(&unicode_math(&unicode), written)?;
            expect(
                &mathml_body(&unicode, Format::UnicodeMath),
                &mathml_body(latex, Format::LaTeX),
            )
        },
    );
}

/// A function name ending an n-aryand is grouped only before a script that
/// would otherwise join it.
#[test]
fn a_function_before_a_baseless_script_is_grouped() {
    check(
        &[
            ("\\sum \\log k", "∑ log k"),
            ("\\int \\sin x", "∫ sin x"),
            ("\\prod\\cos{}_i", "∏ 〖cos〗 〖〗_i"),
        ],
        |&(latex, written)| {
            let unicode = convert(latex, Format::LaTeX, Format::UnicodeMath).unwrap();
            expect(&unicode, written)?;
            expect(&unicode_math(&unicode), written)
        },
    );
}

/// A word that spells a function name is written so that it reads back as
/// a word (`i nf`, not the function inf).
#[test]
fn a_word_is_not_written_as_a_function_name() {
    let mathml = |input: &str| mathml_body(input, Format::UnicodeMath);
    check(&["*i nf(x)", "x_(s in)"], |&input| {
        let written = unicode_math(input);
        expect(&mathml(&written), &mathml(input))?;
        expect(&unicode_math(&written), &written)
    });
}
