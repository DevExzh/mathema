//! TeX's infix fraction commands (`\over`, `\atop`, `\above`, `\choose`,
//! `\brace`, `\brack`, `\…withdelims`) divide the math list they are in, in
//! every context that holds a math list, and never reach a writer as text.

use mathema::ast::MathNode;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
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

/// The warnings of a lenient parse that contain `text`.
fn lenient_warnings(input: &str, text: &str) -> usize {
    from_latex_with(input, &lenient())
        .unwrap_or_else(|e| panic!("{input}: {e}"))
        .diagnostics()
        .iter()
        .filter(|d| d.message.contains(text))
        .count()
}

/// A structural command after `\above` is no dimension register: the
/// dimension is missing, and the command keeps its meaning.
#[test]
fn structural_command_is_no_dimension() {
    for (input, recovered) in [
        (
            r"\begin{matrix} a \above \end{matrix}",
            r"\begin{matrix}\frac{a}{}\end{matrix}",
        ),
        (r"\left( a \above \right)", r"\left(\frac{a}{}\right)"),
        (r"{a \above \\ b}", r"{\frac{a}{\\ b}}"),
    ] {
        let error = strict_error(input);
        assert!(
            error.contains(r"missing dimension after \above"),
            "{input}: {error}"
        );
        assert_eq!(
            from_latex_with(input, &lenient()).unwrap().expr(),
            from_latex(recovered).unwrap().expr(),
            "{input}"
        );
        assert_eq!(lenient_warnings(input, "missing dimension"), 1, "{input}");
    }
    let error = strict_error(r"{a \abovewithdelims() \end{x}}");
    assert!(error.contains("missing dimension"), "{error}");
    for register in [r"\fboxrule", r"\arrayrulewidth", r"\jot", r"\tabcolsep"] {
        let input = format!(r"{{a \above{register} b}}");
        assert_eq!(
            from_latex(&input).unwrap().expr(),
            from_latex(r"\frac{a}{b}").unwrap().expr(),
            "{input}"
        );
    }
}

/// A declaration between a large operator and an infix command (`\sum \bf
/// \over`) leaves the operator in the numerator, as in TeX and KaTeX.
#[test]
fn large_operator_before_a_declaration_and_infix_is_the_numerator() {
    for (input, equivalent) in [
        (r"\sum \bf \over", r"\frac{\sum}{}"),
        (r"\int \bf \brace", r"\genfrac\{\}{0pt}{}{\int}{}"),
        (r"\sum \rm \above 1pt", r"\frac{\sum}{}"),
    ] {
        assert_eq!(
            from_latex(input)
                .unwrap_or_else(|e| panic!("{input}: {e}"))
                .expr(),
            from_latex(equivalent).unwrap().expr(),
            "{input}"
        );
    }
    for input in [r"\sum_i \color{red} a \over b", r"\sum_i \bf a \over b"] {
        let nodes = from_latex(input).unwrap().expr().nodes;
        let [MathNode::Fraction { num, .. }] = nodes.as_slice() else {
            panic!("{input}: {nodes:?}");
        };
        assert!(format!("{num:?}").contains("Nary"), "{input}: {num:?}");
    }
}

/// `\color` opens no group, so a later infix command divides the whole
/// enclosing list, the color in force on both sides.
#[test]
fn color_switch_does_not_scope_an_infix_fraction() {
    for (input, equivalent) in [
        (
            r"a \color{red} b \over c",
            r"\frac{a\textcolor{red}{b}}{\textcolor{red}{c}}",
        ),
        (
            r"\color{red}a \over b",
            r"\frac{\textcolor{red}{a}}{\textcolor{red}{b}}",
        ),
        (
            r"{x\color{red}a \over b}+c",
            r"\frac{x\textcolor{red}{a}}{\textcolor{red}{b}}+c",
        ),
        (r"\color{red}{a \over b}", r"\textcolor{red}{\frac{a}{b}}"),
    ] {
        assert_eq!(
            from_latex(input).unwrap().expr(),
            from_latex(equivalent).unwrap().expr(),
            "{input}"
        );
    }
}

/// A `\color` switch in a `\left…\right` list stays in force across a
/// `\middle` to the end of the fence, as in TeX and KaTeX; the delimiter
/// stays the fence's own child (uncolored, unlike TeX and KaTeX: a known
/// gap). Across an infix command the color stays in force on both sides, as
/// in TeX; KaTeX colors only the numerator side.
#[test]
fn color_switch_continues_across_middle() {
    for (input, equivalent) in [
        (
            r"\left( \color{red} a \middle| c \right)",
            r"\left( \textcolor{red}{a} \middle| \textcolor{red}{c} \right)",
        ),
        (
            r"\left( \color{red} a \atop b \middle| c \right)",
            r"\left( \genfrac{}{}{0pt}{}{\textcolor{red}{a}}{\textcolor{red}{b}\middle|\textcolor{red}{c}} \right)",
        ),
        (
            r"\left( \color{red} a \middle| b \over c \right)",
            r"\left( \frac{\textcolor{red}{a}\middle|\textcolor{red}{b}}{\textcolor{red}{c}} \right)",
        ),
        (
            r"\left( \color{red} a \middle| \color{blue} b \middle| c \right)",
            r"\left( \textcolor{red}{a} \middle| \textcolor{red}{\textcolor{blue}{b}} \middle| \textcolor{blue}{c} \right)",
        ),
        // In a nested group the `\middle` is the group's, inside the color.
        (
            r"\left( {\color{red} a \middle| c} d \right)",
            r"\left( {\textcolor{red}{a \middle| c}} d \right)",
        ),
        // Outside a fence `\middle` is a plain delimiter.
        (r"\color{red} a \middle| c", r"\textcolor{red}{a|c}"),
        // A fraction after a resumed color divides the whole fence list,
        // however the switches nest.
        (
            r"\left( \color{red} a \middle| \color{blue} b \over c \right)",
            r"\left( \frac{\textcolor{red}{a}\middle|\textcolor{red}{\textcolor{blue}{b}}}{\textcolor{red}{\textcolor{blue}{c}}} \right)",
        ),
        (
            r"\left( \color{red} a \middle| \bf b \over c \right)",
            r"\left( \frac{\textcolor{red}{a}\middle|\textcolor{red}{\mathbf{b}}}{\textcolor{red}{\mathbf{c}}} \right)",
        ),
        (
            r"x \color{red} a \color{blue} b \over c",
            r"\frac{x\textcolor{red}{a\textcolor{blue}{b}}}{\textcolor{red}{\textcolor{blue}{c}}}",
        ),
        // A `\middle` in a colored part keeps the barless fraction's fence.
        (
            r"\left( {\color{red} a \middle| b} \atop c \right)",
            r"\left( \genfrac{}{}{0pt}{}{\textcolor{red}{a\middle|b}}{c} \right)",
        ),
    ] {
        let result = from_latex(input).unwrap();
        assert_eq!(
            result.expr(),
            from_latex(equivalent).unwrap().expr(),
            "{input}"
        );
        let written = result.to_latex().unwrap();
        let again = from_latex(&written).unwrap();
        assert_eq!(again.expr(), result.expr(), "{input}: {written}");
        assert_eq!(again.to_latex().unwrap(), written, "unstable: {input}");
    }
    let ml = from_latex(r"\left( \color{red} a \atop b \middle| c \right)")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        ml.contains(r#"<mstyle mathcolor="red"><mi>c</mi></mstyle>"#),
        "{ml}"
    );
}

/// One infix command per list holds across nested color switches and a
/// `\middle`, as in TeX and KaTeX.
#[test]
fn second_infix_command_after_nested_color_is_ambiguous() {
    for input in [
        r"\color{red} a \over b \color{blue} c \over d",
        r"\left( \color{red} a \over b \middle| \color{blue} c \over d \right)",
    ] {
        let message = strict_error(input);
        assert!(message.contains("ambiguous"), "{input}: {message}");
    }
}

/// As in KaTeX and MathJax the `\above` dimension may be braced; a number
/// without a unit is TeX's "Illegal unit of measure".
#[test]
fn above_dimension_forms() {
    for input in [r"{a\above{1pt} b}", r"{a\abovewithdelims(){0pt} b}"] {
        assert!(from_latex(input).is_ok(), "{input}");
    }
    assert_eq!(
        from_latex(r"{a\above{0pt} b}").unwrap().expr(),
        from_latex(r"{a\atop b}").unwrap().expr()
    );
    for input in [r"{a\above 1 b}", r"{a\above{2} b}"] {
        let error = strict_error(input);
        assert!(
            error.contains("illegal unit of measure"),
            "{input}: {error}"
        );
        assert_eq!(lenient_warnings(input, "illegal unit"), 1, "{input}");
        assert_eq!(
            from_latex_with(input, &lenient()).unwrap().expr(),
            from_latex(r"\frac{a}{b}").unwrap().expr(),
            "{input}"
        );
    }
}

/// A speculative re-parse of the same source gives each warning once.
#[test]
fn infix_warnings_are_not_repeated() {
    for (input, text) in [
        (r"\begin{matrix} a \above 2pt b \end{matrix}", "thickness"),
        (
            r"\begin{matrix} a \above b \end{matrix}",
            "missing dimension",
        ),
        (r"\begin{matrix} x^\over \end{matrix}", "misplaced"),
        (r"\left( a \over b \over c \right)", "ambiguous"),
    ] {
        assert_eq!(lenient_warnings(input, text), 1, "{input}");
    }
    // Two commands are two warnings.
    assert_eq!(
        lenient_warnings(r"{a \above 2pt b} + {c \above 2pt d}", "thickness"),
        2
    );
}

/// A barless fraction whose part holds a `\middle` keeps its `\left…\right`
/// pair instead of becoming `\binom`.
#[test]
fn middle_keeps_the_fence_of_a_barless_fraction() {
    for input in [
        r"\left( a \atop b \middle| c \right)",
        r"\left[ a \middle| b \atop c \right]",
    ] {
        let first = from_latex(input).unwrap().to_latex().unwrap();
        assert!(first.starts_with(r"\left"), "{input}: {first}");
        let again = from_latex(&first).unwrap();
        assert_eq!(again.expr(), from_latex(input).unwrap().expr(), "{first}");
        assert_eq!(again.to_latex().unwrap(), first, "unstable: {input}");
    }
}

/// TeX and KaTeX allow one infix command per list: a second is an error
/// when strict; lenient, it divides the denominator.
#[test]
fn second_infix_command_is_ambiguous() {
    for input in [
        r"a \over b \over c",
        r"{a \atop b \choose c}",
        r"x+{a\over b\above1pt c}",
    ] {
        let error = strict_error(input);
        assert!(error.contains("ambiguous"), "{input}: {error}");
    }
    assert_eq!(
        from_latex_with(r"a \over b \over c", &lenient())
            .unwrap()
            .expr(),
        from_latex(r"\frac{a}{\frac{b}{c}}").unwrap().expr()
    );
}

/// A long chain of infix commands is resolved without recursion: no stack
/// overflow on a small thread stack.
#[test]
fn long_infix_chain_does_not_overflow_the_stack() {
    let chain = "a \\over ".repeat(5000) + "a";
    let inputs = [
        chain.clone(),
        format!("{{{chain}}}"),
        format!("\\left({chain}\\right)"),
    ];
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            for input in &inputs {
                let _ = LatexReader::parse_compact(input, &lenient());
                assert!(LatexReader::parse_compact(input, &Config::default()).is_err());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
