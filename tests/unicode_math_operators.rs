//! UnicodeMath operands and operators (Unicode Technical Note 28): simple
//! script operands (section 2.2), signed script operands (appendix A
//! `soperand`), the operators of table 3.1 (`·`, `*`), negated operators
//! (sections 2.1 and 4.1), function names in an n-aryand (sections 3.4 and
//! 3.5) and the space characters of section 3.16.

use mathema::ast::{MathExpr, MathNode, Rational};
use mathema::writer::Writer;
use mathema::{Config, Format, convert};

fn unicode_math(input: &str) -> String {
    convert(input, Format::UnicodeMath, Format::UnicodeMath).unwrap()
}

fn mathml_body(input: &str, from: Format) -> String {
    let mathml = convert(input, from, Format::MathML).unwrap();
    let start = mathml.find('>').unwrap() + 1;
    let end = mathml.rfind("</math>").unwrap();
    mathml[start..end].to_owned()
}

#[test]
fn mathml_font_scopes_style_raw_and_normalized_runs_on_every_route() {
    use mathema::writer::unicode_math::UnicodeMathWriter;

    for (source, wanted) in [
        (
            r#"<math><mstyle mathvariant="monospace"><mi>c</mi><mi>o</mi><mi>d</mi><mi>e</mi></mstyle></math>"#,
            "𝚌𝚘𝚍𝚎",
        ),
        (
            r#"<math><mstyle mathvariant="sans-serif"><mi>A</mi><mi>B</mi><mi>C</mi><mi>D</mi></mstyle></math>"#,
            "𝖠𝖡𝖢𝖣",
        ),
        (
            r#"<math><mstyle mathvariant="monospace"><mi>code</mi></mstyle></math>"#,
            "𝚌𝚘𝚍𝚎",
        ),
    ] {
        let parsed = mathema::from_mathml(source).unwrap();
        let config = Config::default();
        assert_eq!(
            parsed.to_unicode_math().unwrap(),
            wanted,
            "builder: {source}"
        );
        assert_eq!(
            UnicodeMathWriter::write(&parsed.expr(), &config).unwrap(),
            wanted,
            "heap: {source}"
        );
        assert_eq!(
            UnicodeMathWriter::write_compact(parsed.compact_expr(), &config).unwrap(),
            wanted,
            "compact: {source}"
        );
        assert_eq!(
            convert(source, Format::MathML, Format::UnicodeMath).unwrap(),
            wanted,
            "one-shot: {source}"
        );
    }
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
    ("x^1sin1", "x^1 sin1", r"x^{1}sin1"),
    ("x_max", "x_max", r"x_{\max}"),
    // Accents, primes and scripts on a function name in a script.
    ("x_max̅ y", "x_max̅ y", r"x_{\overline{\max}}y"),
    ("x_min̅^2", "x_min̅^2", r"x_{\overline{\min}}^{2}"),
    ("x_max̂/2", "x_max̂/2", r"\frac{x_{\hat{\max}}}{2}"),
    ("x_max̂!", "x_max̂!", r"x_{\hat{\max}}!"),
    ("x_max̂′", "x_max̂^′", r"x_{\hat{\max}}^{\prime}"),
    ("x_max̂_i", "x_(max̂_i)", r"x_{{\hat{\max}}_i}"),
    ("x_-max̂", "x_(-max̂)", r"x_{- \hat{\max}}"),
    ("(_max̂)F", "(_max̂)F", r"{{}_{\hat{\max}}F}"),
    ("x┬max̂", "x┬max̂", r"\underset{\hat{\max}}{x}"),
    ("∑ x_max̂", "∑ x_max̂", r"\sum\limits {x_{\hat{\max}}}"),
    (
        "∑_i x_max̂ y",
        "∑_i x_max̂ y",
        r"\sum\limits_i {x_{\hat{\max}}}y",
    ),
    (
        "∑_max̂^min̂ x",
        "∑_max̂^min̂ x",
        r"\sum\limits_{\hat{\max}}^{\hat{\min}} x",
    ),
    (
        "∑ −inf!^loĝ(x)",
        "∑ − inf!^loĝ(x)",
        r"\sum\limits {-}\inf !^{\hat{\log}}\left( x \right)",
    ),
    // The minus sign is an operator as the hyphen is (table 3.1).
    ("a/≈b/c", "a≉b/c", r"a\mathrel{≉} \frac{b}{c}"),
    ("a≣b/c", "a≣b/c", r"a\mathrel{≣} \frac{b}{c}"),
    ("a†/b", "(a†)/b", r"\frac{a\dagger}{b}"),
    ("|x/y|", "| x/y|", r"|\frac{x}{y}|"),
    (
        "√(n&∑_i _1)x",
        "√(n&∑_i _1)x",
        r"\sqrt[n]{{\sum\limits_i}_{1}}x",
    ),
    ("a≲b/c", "a≲b/c", r"a\lesssim \frac{b}{c}"),
    (
        "x〖∑ a〗^2/b",
        "(x〖∑ a〗^2)/b",
        r"\frac{x{\sum\limits a}^{2}}{b}",
    ),
    (
        "〖∑ a〗_1¦c",
        "〖∑ a〗_1¦c",
        r"\genfrac{}{}{0pt}{}{{\sum\limits a}_{1}}{c}",
    ),
    ("∑^-1^′ a", "∑^(-1′) a", r"\sum\limits^{- 1^{\prime}} a"),
    ("∑_a_b x", "∑_(a_b) x", r"\sum\limits_{a_b} x"),
    ("∑^2_1 x", "∑_1^2 x", r"\sum\limits_{1}^{2} x"),
    ("a−b/c", "a−b/c", r"a- \frac{b}{c}"),
    // The sign keeps the operand after it; the function's argument is
    // outside the script, which the space ends (appendix A, section 2.3).
    ("e^-sin x", "e^(- sin) x", r"e^{- \sin}x"),
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

#[test]
fn unicode_math_reports_an_unrecoverable_symbol_class() {
    let read = mathema::from_latex(r"{:}_{x}xx").unwrap();
    let (written, diagnostics) = read.to_unicode_math_with_diagnostics().unwrap();

    assert_eq!(written, ":_x xx");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("math atom spacing class"),
        "{diagnostics:?}"
    );
}

#[test]
fn unicode_math_does_not_report_recoverable_symbol_classes() {
    for input in ["a:b", r"\ldotp", r"\left|x\right|", r"\sum x", r"\sin x"] {
        let read = mathema::from_latex(input).unwrap();
        let (_, diagnostics) = read.to_unicode_math_with_diagnostics().unwrap();
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("math atom spacing class")),
            "{input}: {diagnostics:?}"
        );
    }
}

#[test]
fn unicode_math_reports_symbols_that_split_or_become_text() {
    for (source, expected, warning) in [
        (
            r#"<mo data-mjx-texclass="REL">@</mo>"#,
            "@",
            "spacing class",
        ),
        (
            r#"<mo data-mjx-texclass="BIN">?</mo>"#,
            "?",
            "spacing class",
        ),
        (
            r#"<mo data-mjx-texclass="OPEN">(</mo>"#,
            "(",
            "spacing class",
        ),
        ("<mo>≨≨</mo>", "≨≨", "as one operator token"),
        ("<mo>()</mo>", "()", "as one operator token"),
        ("<mo>..</mo>", "..", "as one operator token"),
    ] {
        let read = mathema::from_mathml(&format!("<math>{source}</math>")).unwrap();
        let (written, diagnostics) = read.to_unicode_math_with_diagnostics().unwrap();
        assert_eq!(written, expected, "{source}");
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert!(
            diagnostics[0].message.contains(warning),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn unicode_math_keeps_paired_delimiter_tokens_representable() {
    let read = mathema::from_mathml("<math><mo>(</mo><mi>x</mi><mo>)</mo></math>").unwrap();
    let (written, diagnostics) = read.to_unicode_math_with_diagnostics().unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(written, "(x)");

    let nested = mathema::from_latex(r"\left[(x)\right]").unwrap();
    let (written, diagnostics) = nested.to_unicode_math_with_diagnostics().unwrap();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("math atom spacing class")),
        "{diagnostics:?}"
    );
    assert_eq!(
        mathema::from_unicode_math(&written)
            .unwrap()
            .to_unicode_math()
            .unwrap(),
        written
    );
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

#[test]
fn unicode_math_preserves_a_two_em_space() {
    let parsed = mathema::from_latex(r"x\qquad y").unwrap();
    let (written, diagnostics) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(written, "x\u{2003}\u{2003}y");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("space width")),
        "{diagnostics:?}"
    );
    let reparsed = mathema::from_unicode_math(&written).unwrap();
    let body = mathml_body(&written, Format::UnicodeMath);
    assert_eq!(
        body,
        r#"<mi>x</mi><mspace width="1em"/><mspace width="1em"/><mi>y</mi>"#
    );
    assert_eq!(reparsed.to_unicode_math().unwrap(), written);
}

#[test]
fn unicode_math_preserves_a_fractional_width_just_over_one_em() {
    let expr = MathExpr::inline(vec![
        MathNode::Identifier("x".into()),
        MathNode::Space(Rational::new(37, 36)),
        MathNode::Identifier("y".into()),
    ]);
    let written =
        mathema::writer::unicode_math::UnicodeMathWriter::write(&expr, &Config::default()).unwrap();
    assert_eq!(
        written.chars().filter(|&ch| ch != 'x' && ch != 'y').count(),
        3
    );
    for glyph in ['\u{2002}', '\u{00A0}', '\u{2005}'] {
        assert_eq!(
            written.chars().filter(|&ch| ch == glyph).count(),
            1,
            "{glyph:?}"
        );
    }
}

#[test]
fn unicode_math_keeps_canonical_single_glyph_space_spellings() {
    for (width, glyph) in [
        (Rational::new(1, 1), '\u{2003}'),
        (Rational::new(1, 2), '\u{2002}'),
        (Rational::new(1, 4), '\u{00A0}'),
        (Rational::new(1, 6), '\u{2009}'),
    ] {
        let expr = MathExpr::inline(vec![MathNode::Space(width)]);
        let written =
            mathema::writer::unicode_math::UnicodeMathWriter::write(&expr, &Config::default())
                .unwrap();
        assert_eq!(written, glyph.to_string(), "{width}");
    }
}

#[test]
fn unicode_math_reports_an_unrepresentable_space_width() {
    let parsed =
        mathema::from_mathml(r#"<math><mi>x</mi><mspace width="0.4em"/><mi>y</mi></math>"#)
            .unwrap();
    let (written, diagnostics) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(written, "x\u{2009}y");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("space width 2/5em")),
        "{diagnostics:?}"
    );
}

#[test]
fn unicode_math_reports_dropped_upright_style() {
    let parsed = mathema::from_latex(r"\mathrm{d}x").unwrap();
    let (written, diagnostics) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(written, "dx");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("upright math alphabet")),
        "{diagnostics:?}"
    );
}

#[test]
fn unicode_math_reports_explicit_delimiter_size_loss() {
    let parsed = mathema::from_latex(r"\bigl(x\bigr)").unwrap();
    let (written, diagnostics) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(written, "(x)");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("explicit scale factor")),
        "{diagnostics:?}"
    );
}

#[test]
fn unicode_math_emits_native_nary_placement_controls() {
    let limits = mathema::from_latex(r"\sum\nolimits_{i=0}^{n} i").unwrap();
    assert_eq!(limits.to_unicode_math().unwrap(), "∑2_(i=0)^n i");
    assert!(
        limits
            .to_unicode_math_with_diagnostics()
            .unwrap()
            .1
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("n-ary placement"))
    );

    let integral = mathema::from_latex(r"\int\limits_{0}^{1} x").unwrap();
    assert_eq!(integral.to_unicode_math().unwrap(), "∫1_0^1 x");

    let unsupported = mathema::from_unicode_math_with("∑3_(i=0)^n▒i", &Config::default()).unwrap();
    assert!(
        unsupported
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unsupported placement flags"))
    );

    let literal = mathema::from_unicode_math("∑1x").unwrap();
    assert!(literal.to_mathml().unwrap().contains("<mn>1</mn>"));
}

#[test]
fn unicode_math_groups_compound_mathop_bases_before_scripts() {
    let parsed = mathema::from_latex(r"\mathop{\mathrm{Res}}_{x} f").unwrap();
    let (written, diagnostics) = parsed.to_unicode_math_with_diagnostics().unwrap();
    assert_eq!(written, "〖Res〗_x f");
    let reparsed = mathema::from_unicode_math(&written).unwrap();
    let reparsed_mathml = reparsed.to_mathml().unwrap();
    assert!(
        reparsed_mathml.contains("<msub><mrow><mi>R</mi><mi>e</mi><mi>s</mi></mrow>"),
        "the group must keep the whole operator as the scripted base: {reparsed_mathml}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("movable lower bound")),
        "{diagnostics:?}"
    );
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
            ("x_{1sin1}", "x_(1sin1)"),
            ("\\frac{kmax}{2}", "(km ax)/2"),
            ("x_{\\hat{\\log}}y", "x_loĝ y"),
            ("x_{\\hat{\\log}}^2", "x_loĝ^2"),
            ("x_{k\\hat\\log}", "x_(kloĝ)"),
            ("x_{\\hat{\\log}_i}", "x_(loĝ_i)"),
            ("x_{\\hat{max}}", "x_〖m ax〗̂"),
            ("max1", "max1"),
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

/// LaTeX, its UnicodeMath from every path, and the LaTeX that reads back as.
const OPERANDS: &[(&str, &str, &str)] = &[
    (
        r"a\not\approx\frac{b}{c}",
        "a≉b/c",
        r"a\mathrel{≉} \frac{b}{c}",
    ),
    (
        r"a\not\doteq\frac{b}{c}",
        "a≐̸b/c",
        r"a\overset{\char824 }{\doteq}\frac{b}{c}",
    ),
    (
        r"\left(a\frac{b}{c}\right)",
        "(a b/c)",
        r"\left( a\frac{b}{c} \right)",
    ),
    (r"\left|\frac{a}{b}\right|", "|a/b|", r"|\frac{a}{b}|"),
    // A wide combining accent follows its base like any other.
    (r"\widehat{x}", "x̂", r"\hat{x}"),
    (
        r"a\widetilde{x}+\frac{b}{c}",
        "ax̃+b/c",
        r"a\tilde{x}+ \frac{b}{c}",
    ),
    // Bars that may not pair as written are set apart from a fraction.
    (
        r"\left|a\left|\frac{b}{c}\right|\right|",
        "|a| b/c||",
        r"|a|\frac{b}{c}||",
    ),
    (r"\left|\frac{a}{b}+\right|", "| a/b+|", r"|\frac{a}{b}+ |"),
    (r"x_{\left|a\right|}", "x_(|a|)", r"x_{|a|}"),
    (r"{\left|a\right|}_1", "〖|a|〗_1", r"{|a|}_{1}"),
    (
        r"\frac{\left|a\right|}{\left|b\right|}",
        "(|a|)/(|b|)",
        r"\frac{|a|}{|b|}",
    ),
    (r"\hat{\sqrt{x}}", "〖√x〗̂", r"\hat{\sqrt{x}}"),
    (
        r"\frac{x}{\vec{\mid}}a",
        "x/(∣⃗) a",
        r"\frac{x}{\vec{\mid}}a",
    ),
    (
        r"x^{\hat{\left(a\right)}}",
        "x^((a)̂)",
        r"x^{\hat{\left( a \right)}}",
    ),
    (
        r"\sqrt[n]{\sum_{i} a}xx",
        "√(n&∑_i a)xx",
        r"\sqrt[n]{\sum\limits_i a}xx",
    ),
    (r"\sqrt[4]{x}", "∜x", r"\sqrt[4]{x}"),
    (
        r"{\sideset{_1}{}{x}}_2",
        "〖(_1)x〗_2",
        r"{\mathinner{{{}_{1}x}}}_{2}",
    ),
    (r"\sqrt[n]{{}}", "√(n&〖〗)", r"\sqrt[n]{\mathinner{}}"),
    (r"\hat{\,}", "〖 〗̂", r"\hat{\mathinner{\,}}"),
    (r"\frac{a\dagger}{b}", "(a†)/b", r"\frac{a\dagger}{b}"),
    (r"\sideset{_1}{}{s}in", "(_1)s in", r"{{}_{1}s}in"),
    (r"a-\frac{b}{c}", "a−b/c", r"a- \frac{b}{c}"),
    (r"\frac{\mathord{-}}{b}", "(−)/b", r"\frac{-}{b}"),
    (
        r"\frac{{\sum a}^2}{b}",
        "〖∑ a〗^2/b",
        r"\frac{{\sum\limits a}^{2}}{b}",
    ),
    (
        r"\frac{{\int f}^2}{2}",
        "〖∫ f〗^2/2",
        r"\frac{{\int\nolimits f}^{2}}{2}",
    ),
    (r"a\lesssim\frac{b}{c}", "a≲b/c", r"a\lesssim \frac{b}{c}"),
    (
        r"a_1\coloneqq\frac{b}{c}",
        "a_1≔b/c",
        r"a_{1}\coloneqq \frac{b}{c}",
    ),
    (
        r"\sum a\perp\frac{b}{c}",
        "∑ a⊥b/c",
        r"\sum\limits a\perp \frac{b}{c}",
    ),
    (r"\langle\frac{b}{c}", "⟨b/c", r"\langle\frac{b}{c}"),
    (r"a\iff\frac{b}{c}", "a⟺b/c", r"a\iff \frac{b}{c}"),
    (r"\,a", "\u{2009}a", r"\,a"),
    (r"{\sum a}^2", r"〖∑ a〗^2", r"{\sum\limits a}^{2}"),
    (r"{\sum a}_1", r"〖∑ a〗_1", r"{\sum\limits a}_{1}"),
    (
        r"{\int_0^1 f}^2",
        r"〖∫_0^1 f〗^2",
        r"{\int\nolimits_{0}^{1} f}^{2}",
    ),
    (r"{\prod_k x}^2", r"〖∏_k x〗^2", r"{\prod\limits_k x}^{2}"),
    (
        r"{\sum_i a_i}^2",
        r"〖∑_i a_i〗^2",
        r"{\sum\limits_i {a_i}}^{2}",
    ),
    (
        r"\underset{n}{\sum a}",
        r"〖∑ a〗┬n",
        r"\underset{n}{\mathinner{\sum\limits a}}",
    ),
    (
        r"\overbrace{\sum a}^{n}",
        r"〖〖∑ a〗┴⏞〗┴n",
        r"\overset{n}{\mathinner{\overbrace{\mathinner{\sum\limits a}}}}",
    ),
    (
        r"\underbrace{\sum a}_{n}",
        r"〖〖∑ a〗┬⏟〗┬n",
        r"\underset{n}{\mathinner{\underbrace{\mathinner{\sum\limits a}}}}",
    ),
    (
        r"\sideset{_1}{}{\sum a}_2",
        r"(_1)〖∑ a〗_2",
        r"{{}_{1}{\sum\limits a}_{2}}",
    ),
    (
        r"\dot{\sum a}_1",
        r"〖∑ a〗̇_1",
        r"{\dot{\sum\limits a}}_{1}",
    ),
    (
        r"\hat{\sum a}_i^2",
        r"〖∑ a〗̂_i^2",
        r"{\hat{\sum\limits a}}_i^{2}",
    ),
    (
        r"\frac{\hat{\sum a}}{2}",
        r"〖∑ a〗̂/2",
        r"\frac{\hat{\sum\limits a}}{2}",
    ),
    (r"\prod_k {max}", r"∏_k 〖m ax〗", r"\prod\limits_k {max}"),
    (
        r"\sum_{i} {sin} b",
        r"∑_i 〖s in〗 b",
        r"\sum\limits_i {sin}b",
    ),
    (r"\mathinner{log}x", r"〖l og〗x", r"\mathinner{log}x"),
    (
        r"\int {max}\,dx",
        r"∫ 〖m ax〗 dx",
        r"\int\nolimits {max}\,dx",
    ),
    (
        r"\int_0^1 {max}/x dx",
        r"∫_0^1 〖m ax〗 \/xdx",
        r"\int\nolimits_{0}^{1} {max}/xdx",
    ),
    (r"{max}/{max}", r"m ax\/m ax", r"max/max"),
    (
        r"\sum_i {xmin} b",
        r"∑_i 〖xm in〗 b",
        r"\sum\limits_i {xmin}b",
    ),
    (r"\hat{max}", r"〖m ax〗̂", r"\hat{\mathinner{max}}"),
    (r"\bar{sin}", r"〖s in〗̄", r"\bar{\mathinner{sin}}"),
    (r"\vec{log}x", r"〖l og〗⃗x", r"\vec{\mathinner{log}}x"),
    (r"\hat{ab}", r"〖ab〗̂", r"\hat{\mathinner{ab}}"),
    (r"\hat{kmax}y", r"〖km ax〗̂y", r"\hat{\mathinner{kmax}}y"),
    (r"\hat{x_1}", r"〖x_1〗̂", r"\hat{x_{1}}"),
    (r"\bar{\frac{a}{b}}y", r"〖a/b〗̄y", r"\bar{\frac{a}{b}}y"),
    (r"\hat{a+b}", r"〖a+b〗̂", r"\hat{\mathinner{a+ b}}"),
    (r"\underset{max}{x}", r"x┬(m ax)", r"\underset{max}{x}"),
    (r"\sqrt{max}", r"√(m ax)", r"\sqrt{max}"),
    (r"\boxed{sin}", r"▭(s in)", r"\boxed{sin}"),
    (r"x_{max1}", r"x_max1", r"x_{max1}"),
    (r"x_{kmax}y", r"x_kmax y", r"x_{kmax}y"),
    (r"{max}_1", r"〖m ax〗_1", r"{max}_{1}"),
    (r"\frac{sin}{max}", r"(s in)/(m ax)", r"\frac{sin}{max}"),
    (r"\sum_{max} x", r"∑_(m ax) x", r"\sum\limits_{max} x"),
    (r"\left(max\right)", r"(m ax)", r"\left( max \right)"),
    (
        r"\left(x_1\middle|y\right)",
        r"(x_1|y)",
        r"\left( x_{1}|y \right)",
    ),
    (
        r"\left(max\middle|log x\right)",
        r"(m ax|logx)",
        r"\left( max|logx \right)",
    ),
    (
        r"\begin{matrix}max&sin\end{matrix}",
        r"■(m ax&s in)",
        r"\begin{matrix} max & sin \end{matrix}",
    ),
    (r"\sin{max}", r"sin m ax", r"\sin max"),
    (r"x_{\sqrt{x}}", r"x_(√x)", r"x_{\sqrt{x}}"),
    (r"x^{\left(a\right)}", r"x^((a))", r"x^{\left( a \right)}"),
    (r"x_1'^{\sqrt{x}}", r"x′_1^(√x)", r"x_{1}^{\prime\sqrt{x}}"),
];

/// UnicodeMath of a LaTeX formula by each path: `convert` (normalized), the
/// builder (not normalized), and the heap writer for both.
fn unicode_math_by_path(latex: &str) -> [(&'static str, String); 4] {
    use mathema::reader::{Reader, latex::LatexReader};
    use mathema::writer::{Writer, unicode_math::UnicodeMathWriter};
    let config = mathema::Config::default();
    let mut heap = LatexReader::parse(latex, &config).unwrap().expr;
    let builder_heap = UnicodeMathWriter::write(&heap, &config).unwrap();
    mathema::ast::normalize::normalize_expr(&mut heap);
    [
        (
            "convert",
            convert(latex, Format::LaTeX, Format::UnicodeMath).unwrap(),
        ),
        (
            "builder",
            mathema::from_latex(latex)
                .unwrap()
                .to_unicode_math()
                .unwrap(),
        ),
        ("heap builder", builder_heap),
        (
            "heap convert",
            UnicodeMathWriter::write(&heap, &config).unwrap(),
        ),
    ]
}

/// Grouped n-ary operators, accents over several atoms, words that spell or
/// contain function names, and fences, in every construct's operand.
#[test]
fn operands_agree_on_every_path() {
    check(OPERANDS, |&(latex, written, read)| {
        for (path, unicode) in unicode_math_by_path(latex) {
            expect(&unicode, written).map_err(|error| format!("{path}: {error}"))?;
        }
        expect(
            &convert(written, Format::UnicodeMath, Format::LaTeX).unwrap(),
            read,
        )
    });
}

/// An empty script after a fraction does not join its denominator.
#[test]
fn an_empty_script_after_a_fraction_stays_apart() {
    for (path, unicode) in unicode_math_by_path(r"\frac{1}{2}{}{}^2x") {
        let wanted = "1/2 (^2)x";
        assert_eq!(unicode, wanted, "{path}");
        let latex = convert(&unicode, Format::UnicodeMath, Format::LaTeX).unwrap();
        assert!(latex.starts_with(r"\frac{1}{2}"), "{path}: {latex}");
    }
}

/// The readings a script takes from MathML (no LaTeX spells them).
#[test]
fn a_space_after_a_sign_script_ends_it() {
    check(
        &[
            ("x^- 1/2", r"x^-\frac{1}{2}"),
            ("(^2)x^- 1/2", r"{{}^{2}x^-}\frac{1}{2}"),
            ("√^′", r"{\surd}^{\prime}"),
            ("(|)/y", r"\frac{|}{y}"),
        ],
        |&(input, latex)| {
            expect(
                &convert(input, Format::UnicodeMath, Format::LaTeX).unwrap(),
                latex,
            )
        },
    );
}
