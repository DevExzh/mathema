//! TeX's infix fraction commands (`\over`, `\atop`, `\above`, `\choose`,
//! `\brace`, `\brack`, `\…withdelims`) divide the math list they are in, in
//! every context that holds a math list, and never reach a writer as text.

use mathema::{Config, Strictness, from_latex, from_latex_with};

/// The strict parse error of `input`.
fn strict_error(input: &str) -> String {
    match from_latex(input) {
        Ok(_) => panic!("{input}: parsed"),
        Err(error) => error.to_string(),
    }
}

fn lenient() -> Config {
    Config::default().with_strictness(Strictness::Lenient)
}

/// Each infix form with the explicit command it means.
const FORMS: &[(&str, &str)] = &[
    (r"a \over b", r"\frac{a}{b}"),
    (r"a \atop b", r"\genfrac{}{}{0pt}{}{a}{b}"),
    (r"a \above 1pt b", r"\frac{a}{b}"),
    (r"a \choose b", r"\binom{a}{b}"),
    (r"a \brack b", r"\genfrac[]{0pt}{}{a}{b}"),
    (r"a \overwithdelims() b", r"\genfrac(){}{}{a}{b}"),
    (r"a \abovewithdelims[] 0pt b", r"\genfrac[]{0pt}{}{a}{b}"),
];

/// Contexts whose `X` is a whole math list.
const LIST_CONTEXTS: &[&str] = &[
    "X",
    "{X}",
    r"\operatorname{X}",
    r"\operatorname*{X}",
    r"\mathop{X}",
    r"\mathop{X}\limits_1^2",
    r"\mathrel{X}",
    r"\mathrm{X}",
    r"\mathbf{X}",
    r"\boldsymbol{X}",
    r"\boxed{X}",
    r"\textcolor{red}{X}",
    r"{\color{red}X}",
    r"\color{red}X",
    r"\overset{X}{y}",
    r"\underset{y}{X}",
    r"\begin{array}{c}X\end{array}",
    r"\begin{matrix}X & y\end{matrix}",
    r"\begin{cases}X & y\end{cases}",
    r"\begin{aligned}X &= y\end{aligned}",
    r"\left(X\right)",
    r"\sqrt{X}",
    r"\sqrt[X]{y}",
    r"\xrightarrow{X}",
    r"\xrightarrow[X]{y}",
    r"\substack{X\\y}",
    r"\hbox{$X$}",
    r"\mbox{$X$}",
    r"\text{$X$}",
    r"\overline{X}",
    r"\phantom{X}",
    r"\cancel{X}",
    r"x^{X}",
    r"\frac{X}{y}",
    r"\begin{CD}X @>>> y\end{CD}",
    r"\xymatrix{X & y}",
    r"\boxed{\operatorname{X}}",
];

/// In every list context an infix form reads as the explicit command.
#[test]
fn infix_fractions_divide_every_math_list() {
    for context in LIST_CONTEXTS {
        for (infix, explicit) in FORMS {
            if context.contains("[X]") && infix.contains(']') {
                continue;
            }
            let input = context.replace('X', infix);
            // An optional argument ends at the first `]` outside braces.
            let explicit = if context.contains("[X]") {
                format!("{{{explicit}}}")
            } else {
                (*explicit).to_owned()
            };
            let equivalent = context.replace('X', &explicit);
            let parsed = from_latex(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(
                parsed.expr(),
                from_latex(&equivalent).unwrap().expr(),
                "{input} vs {equivalent}"
            );
        }
    }
}

/// Inputs built from infix commands in every position, valid or not.
fn generated_corpus() -> Vec<String> {
    let commands = [
        r"\over",
        r"\atop",
        r"\above 1pt",
        r"\above",
        r"\above\fboxrule",
        r"\choose",
        r"\brace",
        r"\brack",
        r"\overwithdelims()",
        r"\atopwithdelims[]",
        r"\abovewithdelims\{\}0pt",
        r"\abovewithdelims..",
        r"\overwithdelims",
    ];
    let templates = [
        "C",
        "aCb",
        "a C b C c",
        "{C}",
        "x^C",
        "x_C y",
        "C^2",
        "aC^2b",
        r"\sqrt C",
        r"\frac C b",
        r"\sum C b",
        r"\operatorname{C}",
        r"\operatorname*{aCb}\limits_1",
        r"\operatorname C",
        r"\mathop{aCb}",
        r"\mathop C\limits",
        r"\left(aCb\middle|c\right)",
        r"\color{red}C",
        r"\text{aCb}",
        r"\text{$aCb$}",
        r"\begin{matrix}aCb&C\\C\end{matrix}",
        r"\begin{CD}aCb @>C>> c\end{CD}",
        r"\xymatrix{aCb\ar[r]^{C} & c}",
        r"{\rm aCb}",
        r"\bf aCb",
        r"\overset{C}{\mathop{x}}",
        r"\ce{aCb}",
        r"\braket{a|C|b}",
        r"\sideset{_C}{}\sum",
        r"\tag{C}",
        r"\binom{C}{C}",
        r"\genfrac(){}{}{C}{b}",
        r"\mathchoice{C}{a}{b}{c}",
        r"\underbrace{aCb}_C",
        r"\foo{aCb}",
        r"\hbox{$C$}",
    ];
    let mut corpus = Vec::new();
    for template in templates {
        for command in commands {
            corpus.push(template.replace('C', &format!("{command} ")));
        }
    }
    corpus
}

/// No infix command ever reaches a writer as text, whatever the input or
/// strictness: it is a fraction, an error, or a warning.
#[test]
fn no_infix_marker_reaches_any_writer() {
    let mut parsed_any = false;
    for input in generated_corpus() {
        for config in [Config::default(), lenient()] {
            let Ok(parsed) = from_latex_with(&input, &config) else {
                continue;
            };
            parsed_any = true;
            let outputs = [
                parsed.to_latex(),
                parsed.to_mathml(),
                parsed.to_omml(),
                parsed.to_unicode_math(),
                parsed.to_mtef(),
            ];
            for output in outputs.into_iter().flatten() {
                assert!(
                    !output.contains("infix") && !output.contains("__"),
                    "{input}: marker leaked: {output}"
                );
            }
            let debug = format!("{:?}", parsed.expr());
            assert!(!debug.contains("infix"), "{input}: {debug}");
        }
    }
    assert!(parsed_any);
}

/// `\above`/`\abovewithdelims` need a dimension: an error when strict, as in
/// TeX and MathJax; lenient, a warning and the rule.
#[test]
fn above_without_a_dimension_is_an_error() {
    for (input, command, ruled) in [
        (r"{a \above b}", r"\above", r"\frac{a}{b}"),
        (
            r"{a \abovewithdelims() b}",
            r"\abovewithdelims",
            r"\genfrac(){}{}{a}{b}",
        ),
        (r"{a \above\alpha b}", r"\above", r"\frac{a}{\alpha b}"),
    ] {
        let error = strict_error(input);
        assert!(
            error.contains(&format!("missing dimension after {command}")),
            "{input}: {error}"
        );
        let parsed = from_latex_with(input, &lenient()).unwrap();
        assert_eq!(parsed.expr(), from_latex(ruled).unwrap().expr(), "{input}");
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|d| d.message.contains("missing dimension")),
            "{input}: {:?}",
            parsed.diagnostics()
        );
    }
}

/// A dimension register is a dimension: drawn at the default thickness with
/// a warning, and not left in the denominator.
#[test]
fn above_with_a_register_uses_the_default_rule() {
    for (input, ruled) in [
        (r"{a \above\fboxrule b}", r"\frac{a}{b}"),
        (
            r"{a \abovewithdelims()\fboxrule b}",
            r"\genfrac(){}{}{a}{b}",
        ),
    ] {
        let parsed = from_latex(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(parsed.expr(), from_latex(ruled).unwrap().expr(), "{input}");
        assert!(
            parsed.diagnostics().iter().any(|d| d
                .message
                .contains(r"`\fboxrule` is drawn at the default thickness")),
            "{input}: {:?}",
            parsed.diagnostics()
        );
    }
}

/// A missing `\…withdelims` delimiter is named in the strict error; lenient,
/// it is the null delimiter, as TeX inserts `.`.
#[test]
fn withdelims_missing_delimiter_error_is_specific() {
    for (input, command, recovered) in [
        (r"{a \overwithdelims b}", r"\overwithdelims", r"\frac{a}{b}"),
        (
            r"{a \atopwithdelims( b}",
            r"\atopwithdelims",
            r"\genfrac(.{0pt}{}{a}{b}",
        ),
    ] {
        let error = strict_error(input);
        assert!(
            error.contains(&format!("missing delimiter after {command}")),
            "{input}: {error}"
        );
        let parsed = from_latex_with(input, &lenient()).unwrap();
        assert_eq!(
            parsed.expr(),
            from_latex(recovered).unwrap().expr(),
            "{input}"
        );
    }
}

/// An infix command where a single atom is required divides no list: an
/// error when strict, a warning when lenient, never a silently dropped
/// fraction.
#[test]
fn misplaced_infix_commands_are_reported() {
    for input in [
        r"x^\over y",
        r"\sqrt\over",
        r"\frac\over b",
        r"x_\overwithdelims()",
    ] {
        let error = strict_error(input);
        assert!(error.contains("misplaced"), "{input}: {error}");
        let parsed = from_latex_with(input, &lenient()).unwrap();
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|d| d.message.contains("misplaced")),
            "{input}: {:?}",
            parsed.diagnostics()
        );
    }
}

/// Scripts after an infix command begin the denominator's first atom, and a
/// large operator before one is the numerator, not given it as a body.
#[test]
fn infix_command_is_not_an_atom() {
    for (input, equivalent) in [
        (r"a \over^2 b", r"\frac{a}{{}^2 b}"),
        (r"a \over_1^2 b", r"\frac{a}{{}_1^2 b}"),
        (r"\sum\over b", r"\frac{\sum}{b}"),
        (r"\sum\limits_1\over b", r"\frac{\sum\limits_1}{b}"),
    ] {
        assert_eq!(
            from_latex(input)
                .unwrap_or_else(|e| panic!("{input}: {e}"))
                .expr(),
            from_latex(equivalent).unwrap().expr(),
            "{input} vs {equivalent}"
        );
    }
}
