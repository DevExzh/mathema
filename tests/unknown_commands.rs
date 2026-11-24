//! Undefined LaTeX control sequences are kept as themselves and reported,
//! the way KaTeX (`throwOnError: false`) and MathJax keep them visible,
//! instead of being turned into identifier letters.

use mathema::error::{Config, DiagnosticKind, Span, Strictness};
use mathema::{
    Format, MathNode, UndefinedCommands, convert, convert_with, from_latex, from_latex_with,
    from_mathml, from_mathml_with,
};

/// The opt-in configuration that writes undefined commands back live.
fn keep() -> Config {
    Config::default().with_undefined_commands(UndefinedCommands::Keep)
}

/// LaTeX to LaTeX with undefined commands written live.
fn latex(input: &str) -> String {
    convert_with(input, Format::LaTeX, Format::LaTeX, &keep()).unwrap()
}

/// LaTeX to LaTeX with the default configuration.
fn latex_default(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

/// MathML read with `config`, written as LaTeX without normalization.
fn mathml_to_latex(mathml: &str, config: &Config) -> String {
    from_mathml_with(mathml, config)
        .unwrap()
        .to_latex()
        .unwrap()
}

fn mathml(input: &str) -> String {
    convert(input, Format::LaTeX, Format::MathML).unwrap()
}

/// The command names the reader reports, in order.
fn reported(input: &str) -> Vec<String> {
    from_latex(input)
        .unwrap()
        .diagnostics()
        .iter()
        .filter_map(|d| {
            d.message
                .strip_prefix("undefined control sequence ")
                .or_else(|| d.message.strip_prefix("misplaced control sequence "))
        })
        .map(str::to_owned)
        .collect()
}

fn contains_undefined(nodes: &[MathNode]) -> bool {
    format!("{nodes:?}").contains("UndefinedCommand")
}

#[test]
fn an_undefined_command_round_trips_as_itself() {
    for (input, expected) in [
        (r"\foo x", r"\foo x"),
        (r"x\foo y", r"x\foo y"),
        (r"\foo\bar x", r"\foo\bar{x}"),
        (r"\foo 1", r"\foo1"),
        // The groups after it are its possible arguments, kept as written,
        // empty ones included, wherever it stands.
        (r"\foo{ab}{c}", r"\foo{ab}{c}"),
        (r"\foo {a} {} {bc}d", r"\foo{a}{}{bc}d"),
        (r"\foo{}{a}", r"\foo{}{a}"),
        (r"\foo{x^2}y", r"\foo{x^{2}}y"),
        (r"\left(\foo{ab}{c}\right)", r"\left( \foo{ab}{c} \right)"),
        (
            r"\begin{array}{c}\foo{ab}{c}\end{array}",
            r"\begin{array}{c} \foo{ab}{c} \end{array}",
        ),
        (r"\sum\foo{ab}", r"\sum {\foo{ab}}"),
        (r"\int\foo{ab}dx", r"\int {\foo{ab}}dx"),
        (r"\foo{a}^2", r"{\foo{a}}^{2}"),
        // Without braces they are ordinary atoms, and a command read as a
        // single-token argument leaves the groups to its reader.
        (r"\foo ab", r"\foo ab"),
        (r"\frac\foo{a}{b}", r"\frac{\foo}{a}b"),
        (r"\frac\sqrt{\foo{a}}{b}", r"\frac{\sqrt{\foo{a}}}{b}"),
        (r"\mathbf{\foo x}", r"\mathbf{\foo x}"),
        (
            r"\begin{matrix}\foo & x\end{matrix}",
            r"\begin{matrix} \foo & x \end{matrix}",
        ),
    ] {
        let first = latex(input);
        assert_eq!(first, expected, "{input}");
        assert_eq!(latex(&first), first, "{input} is not stable");
    }
}

#[test]
fn a_group_that_is_not_an_argument_is_set_apart() {
    // MathML can put a group right after a command that took none. The
    // LaTeX writer separates it so it does not read back as an argument.
    let tex = convert_with(
        "<math><merror><mtext>\\foo</mtext></merror><mrow><msup><mi>a</mi><mn>2</mn></msup>\
         <mi>b</mi></mrow></math>",
        Format::MathML,
        Format::LaTeX,
        &keep(),
    )
    .unwrap();
    assert_eq!(tex, r"\foo a^{2}b");
    let fenced = r"\left\langle \r\relax{\middle|}^{\prime} \right\rangle";
    assert_eq!(latex(fenced), fenced);
}

#[test]
fn letters_after_a_command_are_part_of_its_name() {
    // TeX reads `\alphax` as one control word, never as `\alpha x`.
    assert_eq!(latex(r"\alphax"), r"\alphax");
    assert_eq!(reported(r"\alphax"), [r"\alphax"]);
    assert_eq!(latex(r"\alpha x"), r"\alpha x");
}

#[test]
fn the_ast_holds_the_command_and_its_groups() {
    let expr = from_latex(r"\foo{a}{}x").unwrap().expr();
    assert_eq!(
        expr.nodes[0],
        MathNode::UndefinedCommand {
            name: r"\foo".into(),
            arguments: vec![vec![MathNode::Identifier("a".into())], vec![]],
        }
    );
    assert_eq!(expr.nodes[0].as_undefined_command(), Some(r"\foo"));
}

#[test]
fn authored_red_text_is_not_an_undefined_command() {
    for input in [
        r"\textcolor{#CC0000}{\text{\textbackslash foo}}",
        r"\color{#CC0000}\text{\textbackslash foo}",
        r"\textcolor{#CC0000}{\textrm{\textbackslash end}}",
    ] {
        let parsed = from_latex(input).unwrap();
        assert!(!contains_undefined(&parsed.expr().nodes), "{input}");
        assert!(parsed.diagnostics().is_empty(), "{input}");
        let first = latex(input);
        assert!(
            first.starts_with(r"\textcolor[HTML]{cc0000}{\text{"),
            "{first}"
        );
        assert_eq!(latex(&first), first, "{input} is not stable");
    }
    let mathml = r##"<math><mstyle mathcolor="#CC0000"><mtext>\foo</mtext></mstyle><mrow><mi>a</mi></mrow></math>"##;
    let parsed = from_mathml(mathml).unwrap();
    assert!(!contains_undefined(&parsed.expr().nodes));
    assert_eq!(
        parsed.to_latex().unwrap(),
        r"\textcolor[HTML]{CC0000}{\text{\textbackslash{}foo}}a"
    );
}

#[test]
fn both_strictness_modes_warn_without_failing() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let parsed = from_latex_with(r"\foo x + \foo y + \bar z \baz", &config).unwrap();
        let diagnostics = parsed.diagnostics();
        // Reported once per name, as a warning, at the first occurrence.
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|d| d.kind == DiagnosticKind::Warning)
        );
        assert_eq!(diagnostics[0].message, r"undefined control sequence \foo");
        assert_eq!(diagnostics[0].span, Some(Span::new(0, 4)));
        assert_eq!(diagnostics[1].message, r"undefined control sequence \baz");
        assert_eq!(diagnostics[1].span, Some(Span::new(25, 29)));
    }
}

#[test]
fn spans_skip_text_and_verbatim() {
    let input = r"\text{\foo}\newcommand\x{1}\foo";
    let parsed = from_latex(input).unwrap();
    let start = input.rfind(r"\foo").unwrap();
    assert_eq!(
        parsed.diagnostics()[0].span,
        Some(Span::new(start, start + 4))
    );
}

#[test]
fn preprocessed_input_keeps_its_spans() {
    // A comment or a macro makes the preprocessor rewrite the input; the
    // span is found in the caller's input instead.
    let parsed = from_latex("x \\foo % c").unwrap();
    assert_eq!(parsed.diagnostics()[0].span, Some(Span::new(2, 6)));
    let parsed = from_latex(r"\newcommand{\x}{\nope}\x\x").unwrap();
    assert_eq!(
        parsed.diagnostics()[0].message,
        r"undefined control sequence \nope"
    );
    assert_eq!(parsed.diagnostics()[0].span, Some(Span::new(16, 21)));
}

#[test]
fn reports_are_capped() {
    let many: String = (0..300).map(|i| format!(r"\zzq{}", name(i))).collect();
    let diagnostics = from_latex(&many).unwrap().diagnostics().to_vec();
    assert_eq!(diagnostics.len(), 101);
    assert_eq!(
        diagnostics[100].message,
        "200 more undefined control sequences not listed"
    );
    let long = format!(r"\{}", "a".repeat(500));
    let parsed = from_latex(&long).unwrap();
    let message = &parsed.diagnostics()[0].message;
    assert!(message.len() < 100 && message.ends_with('…'), "{message}");

    fn name(mut i: usize) -> String {
        let mut out = String::new();
        loop {
            out.push(char::from(b'a' + (i % 26) as u8));
            i /= 26;
            if i == 0 {
                return out;
            }
        }
    }
}

#[test]
fn macros_and_known_commands_are_not_reported() {
    for input in [
        r"\alpha + \sin x",
        r"\newcommand{\foo}{y}\foo x",
        r"\def\foo{z}\foo x",
        r"\def\foo#1{[#1]}\foo{a}x",
        r"\ce{H2O}",
        r"\Bbbk",
        r"\sum\limits_0 x \displaylimits",
        r"x \! y \; z",
    ] {
        assert!(reported(input).is_empty(), "{input}");
    }
}

#[test]
fn a_macro_left_unexpanded_keeps_its_identifier_spelling() {
    // Found by fuzzing: text the preprocessor never sees (here behind a
    // `\verb` inside `\ce`) can hold a builtin macro. Written back as
    // `\tr` it would expand on the next read, so it stays letters.
    let input = r"\ce{KaTeX$$d$$\0\verb}$$$d\tr?$$?";
    let parsed = from_latex(input).unwrap();
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|d| d.message == r"control sequence \tr is not expanded here"),
        "{:?}",
        parsed.diagnostics()
    );
    let first = latex(input);
    assert!(!first.contains(r"\tr"), "{first}");
    assert_eq!(latex(&first), first);
}

#[test]
fn context_commands_out_of_context_are_misplaced() {
    // They are written in braces with their groups, so that they are not
    // read in a context where they mean something.
    for (input, message, expected) in [
        (
            r"x\right)",
            r"misplaced control sequence \right",
            r"x{\right})",
        ),
        (
            r"x\end{matrix}",
            r"misplaced control sequence \end",
            r"x{\end{matrix}}",
        ),
        (
            r"x\multicolumn{1}{c}{y}",
            r"misplaced control sequence \multicolumn",
            r"x{\multicolumn{1}{c}{y}}",
        ),
        // `\xymatrix` becomes an array, where a bare `\end` would end it.
        (
            r"\xymatrix{\end>}",
            r"misplaced control sequence \end",
            r"\begin{array}{c} {\end}> \end{array}",
        ),
    ] {
        let parsed = from_latex(input).unwrap();
        assert_eq!(parsed.diagnostics()[0].message, message, "{input}");
        let first = latex(input);
        assert_eq!(first, expected, "{input}");
        assert_eq!(latex(&first), first, "{input} is not stable");
    }
}

#[test]
fn mathml_uses_merror_and_reads_it_back() {
    let written = mathml(r"\foo{a}{} x");
    assert!(
        written.contains(
            r"<merror><mtext>\foo</mtext><mrow><mi>a</mi></mrow><mrow></mrow></merror><mi>x</mi>"
        ),
        "{written}"
    );
    assert_eq!(
        convert_with(&written, Format::MathML, Format::LaTeX, &keep()).unwrap(),
        r"\foo{a}{}x"
    );
    assert_eq!(
        convert(&written, Format::MathML, Format::LaTeX).unwrap(),
        r"\mathord{\text{\textbackslash{}foo}\{a\}\{\}}x"
    );
    // Groups beside the command are no arguments on the way back.
    for input in [
        r"\d\Big/dv",
        r"\bgg\bigg|T",
        r"\mathbf{\foo{a}}",
        r"\foo\left(x\right)",
    ] {
        let back = convert_with(&mathml(input), Format::MathML, Format::LaTeX, &keep()).unwrap();
        assert_eq!(back, latex(input), "{input}");
        let back = convert(&mathml(input), Format::MathML, Format::LaTeX).unwrap();
        assert_eq!(back, latex_default(input), "{input}");
    }
    // Any other error content keeps reading as its content.
    let other = from_mathml("<math><merror><mtext>bad input</mtext></merror></math>").unwrap();
    assert!(!contains_undefined(&other.expr().nodes));
}

#[test]
fn other_writers_spell_the_command_as_text() {
    let unicode = convert(r"\foo{a} x", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(unicode.contains(r#""\foo"(a)"#), "{unicode}");
    let omml = convert(r"\foo x", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains(r"<m:t>\foo</m:t>"), "{omml}");
    assert!(convert(r"\foo{a} x", Format::LaTeX, Format::Mtef).is_ok());
}

#[test]
fn text_mode_is_unchanged() {
    // Text runs keep their existing handling: an unknown command inside
    // `\text` contributes nothing and is not reported.
    assert_eq!(latex(r"\text{a \foo b}"), r"\text{a b}");
    assert!(reported(r"\text{a \foo b}").is_empty());
}

#[test]
fn displaylimits_restores_default_placement() {
    assert_eq!(latex(r"\sum\displaylimits_0 x"), r"\sum_0 x");
    assert_eq!(latex(r"\sum\limits\displaylimits_0 x"), r"\sum_0 x");
    assert_eq!(latex(r"\sum\nolimits_0 x"), r"\sum\nolimits_0 x");
    // Without an operator it places nothing, like `\limits`.
    assert_eq!(latex(r"\displaylimits x"), "x");
}

#[cfg(feature = "serde")]
#[test]
fn the_node_serializes() {
    let expr = from_latex(r"\foo{a}").unwrap().expr();
    let json = serde_json::to_string(&expr).unwrap();
    assert_eq!(
        serde_json::from_str::<mathema::MathExpr>(&json).unwrap(),
        expr
    );
}

#[cfg(feature = "render")]
#[test]
fn rendering_shows_the_command_in_the_error_colour() {
    let parsed = from_latex(r"\foo{a} x").unwrap();
    let svg = parsed.to_svg_text().unwrap();
    assert!(svg.contains("#cc0000"), "{svg}");
    assert!(svg.contains(r"\foo"), "{svg}");
    let embedded = parsed.to_svg().unwrap();
    assert!(embedded.contains("#cc0000"), "{embedded}");
}

#[test]
fn dangerous_primitives_stay_inert_text() {
    // In both modes they are spelled like the default inert text, with their
    // groups as literal braces.
    let escaped =
        |name: &str, rest: &str| format!(r"\mathord{{\text{{\textbackslash{{}}{name}}}{rest}}}");
    for (input, expected) in [
        (
            r"\input{/etc/passwd}",
            escaped("input", r"\{/ etc/ passwd\}"),
        ),
        (
            r"\immediate\write18{ls}",
            escaped("immediate", "") + &escaped("write", "") + r"18ls",
        ),
        (r"\openout{f}", escaped("openout", r"\{f\}")),
        (r"\catcode{a}", escaped("catcode", r"\{a\}")),
        (r"\csname{x}", escaped("csname", r"\{x\}")),
        (r"\usepackage{x}", escaped("usepackage", r"\{x\}")),
        (r"\directlua{x}", escaped("directlua", r"\{x\}")),
        (r"\everymath{x}", escaped("everymath", r"\{x\}")),
        (
            r"\inputminted{tex}{secret}",
            escaped("inputminted", r"\{tex\}\{secret\}"),
        ),
    ] {
        let parsed = from_latex(input).unwrap();
        assert!(
            parsed.diagnostics()[0]
                .message
                .ends_with("is not allowed and was kept as text"),
            "{input}"
        );
        for first in [latex(input), latex_default(input)] {
            assert_eq!(first, expected, "{input}");
            assert_eq!(latex(&first), first, "{input} is not stable");
        }
    }
}

#[test]
fn merror_reads_only_undefined_names_as_commands() {
    // A command the LaTeX reader knows, or a dangerous primitive, in an
    // `<merror>` reads as text, as it always did.
    for (body, expected) in [
        (
            "<merror><mtext>\\left</mtext></merror><mo>(</mo>",
            r"\text{\textbackslash{}left}(",
        ),
        (
            "<merror><mtext>\\begin</mtext><mrow><mi>x</mi></mrow></merror>",
            r"{\text{\textbackslash{}begin}x}",
        ),
        (
            "<merror><mtext>\\def</mtext><mrow><mi>x</mi></mrow></merror>",
            r"{\text{\textbackslash{}def}x}",
        ),
        (
            "<merror><mtext>\\frac</mtext><mrow><mi>a</mi></mrow><mrow><mi>b</mi></mrow></merror>",
            r"{\text{\textbackslash{}frac}ab}",
        ),
        (
            "<merror><mtext>\\alpha</mtext></merror>",
            r"\text{\textbackslash{}alpha}",
        ),
        (
            "<merror><mtext>\\mathbf</mtext><mrow><mi>x</mi></mrow></merror>",
            r"{\text{\textbackslash{}mathbf}x}",
        ),
        (
            "<merror><mtext>\\text</mtext><mrow><mi>x</mi></mrow></merror>",
            r"{\text{\textbackslash{}text}x}",
        ),
        (
            "<merror><mtext>\\sqrt</mtext></merror><mrow><mi>a</mi><mi>b</mi></mrow>",
            r"\text{\textbackslash{}sqrt}{ab}",
        ),
        (
            "<merror><mtext>\\immediate</mtext></merror><merror><mtext>\\write</mtext></merror>\
             <mn>18</mn><mrow><mi>l</mi><mi>s</mi></mrow>",
            r"\text{\textbackslash{}immediate}\text{\textbackslash{}write}18{ls}",
        ),
        (
            "<merror><mtext>\\input</mtext><mrow><mo>/</mo><mi>e</mi></mrow></merror>",
            r"{\text{\textbackslash{}input}{\char`/e}}",
        ),
        // Undefined and misplaced names do read back as commands.
        ("<merror><mtext>\\foo</mtext></merror>", r"\foo"),
        ("<merror><mtext>\\right</mtext></merror>", r"{\right}"),
    ] {
        let tex = mathml_to_latex(&format!("<math>{body}</math>"), &keep());
        assert_eq!(tex, expected, "{body}");
        let again = latex(&tex);
        assert_eq!(latex(&again), again, "{body} is not stable");
    }
}

#[test]
fn a_group_after_a_nested_command_is_set_apart() {
    for (mathml, expected) in [
        (
            "<mrow><merror><mtext>\\foo</mtext></merror></mrow><mrow><mi>a</mi><mi>b</mi></mrow>",
            r"\foo\relax {ab}",
        ),
        (
            "<merror><mtext>\\foo</mtext></merror><mrow><mo>{</mo><mi>a</mi><mo>}</mo></mrow>",
            r"\foo\relax{\{a\}}",
        ),
    ] {
        let tex = mathml_to_latex(&format!("<math>{mathml}</math>"), &keep());
        assert_eq!(tex, expected, "{mathml}");
        let again = latex(&tex);
        assert_eq!(latex(&again), again, "{mathml} is not stable");
    }
}

/// An expression with one `UndefinedCommand` named `name`, built as API
/// callers and serde input can.
fn with_name(name: &str) -> mathema::MathExpr {
    mathema::MathExpr::new(
        vec![
            MathNode::UndefinedCommand {
                name: name.into(),
                arguments: vec![vec![MathNode::Identifier("a".into())], vec![]],
            },
            MathNode::Identifier("b".into()),
        ],
        mathema::ast::DisplayStyle::Block,
    )
}

fn write_all(expr: &mathema::MathExpr, config: &Config) -> [String; 3] {
    use mathema::writer::Writer;
    let config = *config;
    [
        mathema::writer::latex::LatexWriter::write(expr, &config).unwrap(),
        mathema::writer::mathml::MathmlWriter::write(expr, &config).unwrap(),
        mathema::writer::unicode_math::UnicodeMathWriter::write(expr, &config).unwrap(),
    ]
}

#[test]
fn api_names_that_are_not_undefined_commands_are_written_as_text() {
    // Even with `Keep`, such a name is escaped text.
    for (name, text) in [
        (r"}\input{x", r"\}\textbackslash{}input\{x"),
        ("\\", r"\textbackslash{}"),
        ("", ""),
        ("foo", "foo"),
        (r"\foo1", r"\textbackslash{}foo1"),
        (r"\alpha", r"\textbackslash{}alpha"),
        (r"\input", r"\textbackslash{}input"),
        (r"\lstinputlisting", r"\textbackslash{}lstinputlisting"),
    ] {
        let [tex, mathml, _] = write_all(&with_name(name), &keep());
        assert_eq!(
            tex,
            format!(r"\mathord{{\text{{{text}}}\{{a\}}\{{\}}}}b"),
            "{name:?}"
        );
        assert!(!mathml.contains("<merror>"), "{name:?}: {mathml}");
    }
    let [tex, mathml, _] = write_all(&with_name(r"\foo"), &keep());
    assert_eq!(tex, r"\foo{a}{}b");
    assert!(mathml.contains(r"<merror><mtext>\foo</mtext>"), "{mathml}");
    let [tex, ..] = write_all(&with_name(r"\foo"), &Config::default());
    assert_eq!(tex, r"\mathord{\text{\textbackslash{}foo}\{a\}\{\}}b");
    // UnicodeMath text cannot hold its own quote.
    let [_, _, unicode] = write_all(&with_name("\"x"), &keep());
    assert_eq!(unicode, "\"\u{FF02}x\"(a)()b");
}

#[cfg(feature = "serde")]
#[test]
fn serde_input_is_validated_when_written() {
    let json = r#"{"nodes":[{"UndefinedCommand":{"name":"}\\input{x","arguments":[]}}],"display":"Block"}"#;
    let expr: mathema::MathExpr = serde_json::from_str(json).unwrap();
    let [tex, ..] = write_all(&expr, &keep());
    assert_eq!(tex, r"\mathord{\text{\}\textbackslash{}input\{x}}");
}

#[test]
fn by_default_undefined_commands_are_written_as_inert_text() {
    for (input, expected) in [
        (r"\foo x", r"\mathord{\text{\textbackslash{}foo}}x"),
        (
            r"\foo{a}{}x",
            r"\mathord{\text{\textbackslash{}foo}\{a\}\{\}}x",
        ),
        (
            r"\foo\bar x",
            r"\mathord{\text{\textbackslash{}foo}}\bar{x}",
        ),
        (
            r"\foo\baz{ab}",
            r"\mathord{\text{\textbackslash{}foo}}\mathord{\text{\textbackslash{}baz}\{ab\}}",
        ),
        (
            r"x\end{matrix}",
            r"x\mathord{\text{\textbackslash{}end}\{matrix\}}",
        ),
        (
            r"\foo{a}^2",
            r"{\mathord{\text{\textbackslash{}foo}\{a\}}}^{2}",
        ),
        // A script left without a base at the end of a group is the
        // literal brace's leading script on the next read.
        (
            r"\foo{a^2{}^3}",
            r"\mathord{\text{\textbackslash{}foo}\{a^{2} {{}^{3}\}}}",
        ),
        (r"\foo{a-}", r"\mathord{\text{\textbackslash{}foo}\{a- \}}"),
        // Found by fuzzing: an operator with an empty body before the brace,
        // and a group holding only a script without a base.
        (
            r"\foo{t\int}",
            r"\mathord{\text{\textbackslash{}foo}\{t\int{}\}}",
        ),
        (
            r"\foo{{}''}",
            r"\mathord{\text{\textbackslash{}foo}\{ {{}^{\prime\prime}\}}}",
        ),
        (
            r"\text{a}\foo\text{b}",
            r"\text{a}\mathord{\text{\textbackslash{}foo}}\text{b}",
        ),
    ] {
        let first = latex_default(input);
        assert_eq!(first, expected, "{input}");
        assert_eq!(latex_default(&first), first, "{input} is not stable");
        // The node, its report and its MathML are the same in both modes.
        assert!(contains_undefined(&from_latex(input).unwrap().expr().nodes));
    }
    // MathML read back is escaped too.
    let tex = mathml_to_latex(
        "<math><merror><mtext>\\foo</mtext><mrow><mi>a</mi></mrow></merror></math>",
        &Config::default(),
    );
    assert_eq!(tex, r"\mathord{\text{\textbackslash{}foo}\{a\}}");
}

#[test]
fn commands_missing_from_the_denylist_are_inert_by_default() {
    for input in [
        r"\lstinputlisting{secret.tex}",
        r"\verbatiminput{secret}",
        r"\VerbatimInput{secret}",
        r"\inputminted{tex}{secret}",
        r"\luadirect{os.execute(1)}",
        r"\pdffiledump{secret}",
        r"\subfile{secret}",
        r"\import{a}{b}",
        r"\includepdf{secret}",
        r"\usetikzlibrary{secret}",
        r"\ExplSyntaxOn",
        r"\loop\repeat",
        r"\font\x=secret",
        r"\openin",
        // Not on any list: escaped only by the default.
        r"\somepackagereader{secret}",
        r"\tracingall",
    ] {
        let out = latex_default(input);
        let name = &input[1..input[1..]
            .find(['{', '\\', '='])
            .map_or(input.len(), |i| i + 1)];
        assert!(!out.contains(&format!("\\{name}")), "{input} -> {out}");
        assert_eq!(latex_default(&out), out, "{input} is not stable");
    }
}

#[test]
fn nested_single_token_commands_parse_in_linear_time() {
    // An undefined command read as an unbraced argument takes no groups, so
    // nothing is parsed twice (this was 2^k work).
    for prefix in [
        r"\frac\foo{",
        r"\sqrt\foo{",
        r"\vec\foo{",
        r"\mathbf\foo{",
        r"\red\foo{",
    ] {
        let depth = 30;
        let input = prefix.repeat(depth) + "x" + &"}".repeat(depth);
        let start = std::time::Instant::now();
        let parsed = from_latex(&input).unwrap_or_else(|e| panic!("{prefix}: {e}"));
        let nodes = parsed.compact_expr().arena.node_count();
        assert!(
            nodes <= 4 * input.len(),
            "{prefix}: {nodes} nodes for {} bytes",
            input.len()
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{prefix}: {:?}",
            start.elapsed()
        );
    }
}

#[test]
fn escaped_groups_with_scripts_without_a_base_are_stable() {
    // Found by fuzzing: scripts without a base at either end of a group
    // attach to the literal braces on the next read.
    for input in [
        r"\foo{{}_{\ }\ m}",
        r"\foo{{}^d}",
        r"\foo{{}^{\prime}{}^{\prime}g\, h}",
        r"\foo{a^2{}^3}",
        r"\foo{{}''}",
        r"\foo{{}^{\prime}\ 2_fk}",
        r"\foo{{}'{}^{\prime g}{}'x}",
    ] {
        let first = latex_default(input);
        assert_eq!(latex_default(&first), first, "{input} is not stable");
    }
}
