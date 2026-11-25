//! No character of an expression may act as LaTeX syntax in the output: every
//! place the LaTeX writer puts a character must write it so that groups stay
//! balanced and the output reads back, in Strict mode, with that character.

use mathema::ast::normalize::normalize_expr;
use mathema::ast::*;
use mathema::error::{Config, Strictness};
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use proptest::prelude::*;

fn bx(node: MathNode) -> Box<MathNode> {
    Box::new(node)
}

fn id(s: &str) -> MathNode {
    MathNode::Identifier(s.into())
}

fn sym(kind: SymbolKind, s: &str) -> MathNode {
    MathNode::Symbol(kind, s.into())
}

fn nary(op: char, sub: Option<&str>, body: Option<&str>, limits: LimitStyle) -> MathNode {
    MathNode::Nary {
        op,
        sub: sub.map(|s| bx(id(s))),
        sup: None,
        body: bx(body.map_or(MathNode::Row(Vec::new()), id)),
        limits,
    }
}

fn fenced(open: &str, close: &str) -> MathNode {
    MathNode::Fenced {
        open: open.into(),
        close: close.into(),
        content: vec![FencedItem::Element(id("x"))],
    }
}

fn sub(node: MathNode) -> MathNode {
    MathNode::Subscript {
        base: bx(id("x")),
        sub: bx(node),
    }
}

fn expr(nodes: Vec<MathNode>) -> MathExpr {
    MathExpr::new(nodes, DisplayStyle::default())
}

fn write(nodes: Vec<MathNode>) -> String {
    LatexWriter::write(&expr(nodes), &Config::default()).unwrap()
}

/// Whether unescaped braces pair up.
fn balanced(latex: &str) -> bool {
    let mut depth = 0usize;
    let mut chars = latex.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.as_str().starts_with("verb") => return true,
            '\\' => {
                chars.next();
            },
            '{' => depth += 1,
            '}' => match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => return false,
            },
            _ => {},
        }
    }
    depth == 0
}

/// Parse `latex` back in Strict mode, after checking its groups.
fn reread(latex: &str) -> mathema::ConvertResult {
    assert!(balanced(latex), "unbalanced groups: {latex}");
    let read = mathema::from_latex(latex).unwrap_or_else(|e| panic!("{latex:?}: {e}"));
    assert!(read.diagnostics().is_empty(), "{latex:?} has diagnostics");
    read
}

/// The characters of the leaf nodes under `nodes`, in order.
fn glyphs(nodes: &[MathNode]) -> String {
    nodes.iter().map(node_glyphs).collect()
}

fn node_glyphs(node: &MathNode) -> String {
    match node {
        MathNode::Number(s)
        | MathNode::Identifier(s)
        | MathNode::Symbol(_, s)
        | MathNode::Text(_, s) => s.clone(),
        MathNode::Operator(op) => op.name.clone(),
        MathNode::Row(nodes) | MathNode::Styled(_, nodes) => glyphs(nodes),
        MathNode::MathAtom { content, .. } | MathNode::StructuredOperator { content, .. } => {
            node_glyphs(content)
        },
        MathNode::Subscript { base, sub } => node_glyphs(base) + &node_glyphs(sub),
        MathNode::Superscript { base, sup } => node_glyphs(base) + &node_glyphs(sup),
        MathNode::Underscript { base, under, .. } => node_glyphs(base) + &node_glyphs(under),
        MathNode::Overscript { base, over, .. } => node_glyphs(base) + &node_glyphs(over),
        MathNode::Accent { base, accent, .. } => format!("{}{accent}", node_glyphs(base)),
        MathNode::Nary { op, sub, body, .. } => {
            format!(
                "{op}{}{}",
                sub.as_deref().map(node_glyphs).unwrap_or_default(),
                node_glyphs(body)
            )
        },
        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let content: String = content
                .iter()
                .map(|item| match item {
                    FencedItem::Element(node) => node_glyphs(node),
                    FencedItem::Middle(s) => s.clone(),
                })
                .collect();
            format!("{open}{content}{close}")
        },
        MathNode::Space(_) => String::new(),
        MathNode::Color { content, .. } | MathNode::Scaled { content, .. } => node_glyphs(content),
        other => panic!("unexpected node {other:?}"),
    }
}

/// Every ASCII character and Unicode ones the reader treats specially:
/// letters, private-use characters, scripts, combining marks, operators.
fn characters() -> impl Iterator<Item = char> {
    (0u8..128).map(char::from).chain(
        "\u{80}\u{85}\u{9F}\u{E9}\u{3B1}\u{7800}\u{5D0}\u{E000}\u{10FFFD}\u{FFFD}\u{1F600}\u{131}\
         \u{211C}\u{B2}\u{2081}\u{301}\u{20D7}\u{338}\u{2202}\u{2264}\u{27E8}\u{2016}\u{2032}\
         \u{2191}\u{2A0B}\u{2140}\u{22C5}\u{2223}\u{2020}\u{B0}\u{1D400}"
            .chars(),
    )
}

const KINDS: [SymbolKind; 9] = [
    SymbolKind::Ordinary,
    SymbolKind::LargeOp,
    SymbolKind::BinaryOp,
    SymbolKind::Relation,
    SymbolKind::Open,
    SymbolKind::Close,
    SymbolKind::Punctuation,
    SymbolKind::Fence,
    SymbolKind::Alphabetic,
];

/// A minus and a prime read back as the glyphs TeX draws for them.
fn drawn(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{2212}' => '-',
            '\u{2032}' => '\'',
            '\u{27E8}' => '<',
            '\u{27E9}' => '>',
            '\u{2018}' => '`',
            _ => ch,
        })
        .collect()
}

#[test]
fn unnamed_large_operators_are_operator_atoms() {
    for (op, body) in [
        ('}', r"\mathop{\}}"),
        ('{', r"\mathop{\{}"),
        ('\\', r"\mathop{\char`\\}"),
        ('%', r"\mathop{\%}"),
        ('#', r"\mathop{\#}"),
        ('$', r"\mathop{\$}"),
        ('&', r"\mathop{\&}"),
        ('~', r"\mathop{\char`\~}"),
        ('^', r"\mathop{\char`\^}"),
        ('_', r"\mathop{\char`\_}"),
        ('\'', r"\mathop{\char`'}"),
        ('"', r#"\mathop{"}"#),
        ('8', r"\mathop{8}"),
        ('x', r"\mathop{x}"),
        ('+', r"\mathop{+}"),
        ('-', r"\mathop{-}"),
        ('\0', r"\mathop{\char0 }"),
        ('\n', r"\mathop{\char10 }"),
        (' ', r"\mathop{\char32 }"),
        ('\u{7F}', r"\mathop{\char127 }"),
        ('\u{85}', r"\mathop{\char133 }"),
        ('\u{2009}', r"\mathop{\char8201 }"),
        ('砀', r"\mathop{砀}"),
        ('\u{E000}', r"\mathop{\char57344 }"),
        ('\u{301}', r"\mathop{\char769 }"),
        ('\u{20D7}', "\\mathop{\u{20D7}}"),
        ('`', r"\mathop{\char``}"),
        ('≤', r"\mathop{\leq}"),
        ('α', r"\mathop{\alpha}"),
        ('²', r"\mathop{\char`²}"),
        ('⨋', r"\mathop{⨋}"),
    ] {
        use LimitStyle::*;
        for (limits, modifier) in [
            (SubSup, r"\nolimits"),
            (AboveBelow, r"\limits"),
            (Default, ""),
        ] {
            assert_eq!(
                write(vec![nary(op, Some("i"), Some("a"), limits)]),
                format!(r"{body}{modifier}_ia")
            );
            // Without limits there is nothing to place.
            assert_eq!(
                write(vec![nary(op, None, Some("a"), limits)]),
                format!("{body}a")
            );
            assert_eq!(
                write(vec![nary(op, None, None, limits), id("y")]),
                format!("{body}y")
            );
            assert_eq!(
                write(vec![sub(nary(op, None, Some("a"), limits))]),
                format!("x_{{{body}a}}")
            );
        }
        // A space as a symbol is spacing.
        if op == '\u{2009}' {
            continue;
        }
        let op = sym(SymbolKind::LargeOp, &op.to_string());
        assert_eq!(write(vec![op.clone(), id("y")]), format!("{body}y"));
        assert_eq!(write(vec![sub(op)]), format!("x_{{{body}}}"));
    }
    // A named operator keeps its command.
    assert_eq!(
        write(vec![nary('∑', None, Some("a"), LimitStyle::SubSup)]),
        r"\sum\nolimits a"
    );
    // KaTeX has no `\bbsum`.
    assert_eq!(
        mathema::from_latex(r"\bbsum_i a")
            .unwrap()
            .to_latex()
            .unwrap(),
        r"\mathop{⅀}_ia"
    );
}

#[test]
fn unnamed_large_operator_keeps_its_neighbours_apart() {
    let mut scripted = nary('8', None, Some("a"), LimitStyle::Default);
    if let MathNode::Nary { sub, .. } = &mut scripted {
        *sub = Some(bx(id("α")));
    }
    assert_eq!(write(vec![scripted]), r"\mathop{8}_\alpha a");
    // `\sum` in the body does not take the next sibling as its own body.
    let mut nested = nary('8', None, None, LimitStyle::Default);
    if let MathNode::Nary { body, .. } = &mut nested {
        *body = bx(nary('∑', None, None, LimitStyle::Default));
    }
    assert_eq!(write(vec![nested, id("y")]), r"\mathop{8}\sum{}y");
    let power = MathNode::Superscript {
        base: bx(nary('8', None, None, LimitStyle::Default)),
        sup: bx(id("y")),
    };
    assert_eq!(write(vec![power]), r"{\mathord{\mathop{8}}}^y");
}

#[test]
fn class_wrapped_glyphs_are_one_script_argument() {
    for (kind, glyph, latex) in [
        (SymbolKind::Relation, "砀", r"x_{\mathrel{砀}}"),
        (SymbolKind::BinaryOp, "砀", r"x_{\mathbin{砀}}"),
        (SymbolKind::Relation, "^", r"x_{\mathrel{\char`\^}}"),
        (SymbolKind::BinaryOp, "\\", r"x_{\mathbin{\char`\\}}"),
        (SymbolKind::Open, "_", r"x_{\mathopen{\char`\_}}"),
        (SymbolKind::Close, "~", r"x_{\mathclose{\char`\~}}"),
        (SymbolKind::Punctuation, "'", r"x_{\mathpunct{\char`'}}"),
        (SymbolKind::Fence, "\"", r#"x_""#),
        (SymbolKind::Relation, "\0", r"x_{\mathrel{\char0 }}"),
        (SymbolKind::Relation, " ", r"x_{\mathrel{\char32 }}"),
        (SymbolKind::Ordinary, " ", r"x_{\char32 }"),
        (SymbolKind::Relation, "²", r"x_{\mathrel{\char`²}}"),
        // Unchanged spellings.
        (SymbolKind::Open, "{", r"x_\{"),
        (SymbolKind::Relation, "=", "x_="),
        (SymbolKind::Relation, "≤", r"x_{\leq}"),
        (SymbolKind::Alphabetic, "ℜ", r"x_\Re"),
        (SymbolKind::LargeOp, "∫", r"x_\int"),
    ] {
        assert_eq!(
            write(vec![sub(sym(kind, glyph))]),
            latex,
            "{kind:?} {glyph:?}"
        );
        reread(latex);
    }
}

#[test]
fn numbers_and_identifiers_escape_syntax() {
    for (text, latex) in [
        ("1.5", "1.5"),
        ("1,000", "1,000"),
        ("½", "½"),
        ("10²", r"10\char`²"),
        ("50%", r"50\%"),
        ("#1", r"\#1"),
        ("$5", r"\$5"),
        ("1&2", r"1\&2"),
        ("}{", r"\}\{"),
        ("\\input", r"\char`\\input"),
        ("2^3_4", r"2\char`\^3\char`\_4"),
        ("1\n\n2", "1  2"),
        ("\0", r"\char0 "),
    ] {
        assert_eq!(write(vec![MathNode::Number(text.into())]), latex);
        reread(latex);
    }
    assert_eq!(write(vec![id("\"")]), r#"""#);
    assert_eq!(write(vec![sub(id("\""))]), r#"x_""#);
    assert_eq!(write(vec![sub(id(" "))]), r"x_{\char32 }");
    let caret = MathNode::Operator(OperatorAttr {
        name: "a^b".into(),
        limit_style: LimitStyle::SubSup,
    });
    assert_eq!(write(vec![caret]), r"\operatorname{a\char`\^b}");
    // Every character of a compound operator stays, and no large operator in
    // it takes the rest as its body.
    assert_eq!(
        write(vec![sym(SymbolKind::Ordinary, "∑∫")]),
        r"\mathord{\mathord{\sum}\mathord{\int}}"
    );
    // An accent glyph is not a prime.
    let quote = MathNode::Accent {
        base: bx(id("x")),
        accent: '\'',
        position: AccentPosition::Above,
        wide: false,
    };
    assert_eq!(write(vec![quote]), r"\overset{\char`'}{x}");
}

#[test]
fn only_delimiters_follow_left_and_right() {
    for (open, close, latex) in [
        ("(", ")", r"\left( x \right)"),
        ("{", "", r"\left\{ x \right."),
        ("|", "/", r"\left| x \right/"),
        (
            "%",
            "~",
            r"\left. \mathopen{\%}x\mathclose{\char`\~} \right.",
        ),
        (
            "_",
            "^",
            r"\left. \mathopen{\char`\_}x\mathclose{\char`\^} \right.",
        ),
        ("é", "©é", r"\left. \mathopen{é}x\mathclose{©é} \right."),
        ("a", ")", r"\left. \mathopen{a}x \right)"),
        (
            "\u{301}",
            "\u{20D7}",
            "\\left. \\mathopen{\\char769 }x\\mathclose{\u{20D7}} \\right.",
        ),
        ("(", "a", r"\left( x\mathclose{a} \right."),
        (
            "8",
            "\0",
            r"\left. \mathopen{8}x\mathclose{\char0 } \right.",
        ),
        ("(", "}{", r"\left( x\mathclose{\}\{} \right."),
        (
            "\\foo",
            "\u{2009}",
            r"\left. \mathopen{\char`\\foo}x\mathclose{\char8201 } \right.",
        ),
    ] {
        assert_eq!(write(vec![fenced(open, close)]), latex);
        reread(latex);
    }
    let middle = MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![
            FencedItem::Element(nary('∑', None, None, LimitStyle::Default)),
            FencedItem::Middle("a}".into()),
            FencedItem::Element(id("y")),
        ],
    };
    assert_eq!(
        write(vec![middle]),
        r"\left( \sum{} \middle. \mathord{a\}}y \right)"
    );
    // A sizing command is dropped from what is no delimiter.
    for (delim, latex) in [
        ("(", r"\big("),
        ("", r"\big."),
        ("a", "a"),
        ("}{", r"\operatorname{\}\{}"),
    ] {
        let big = MathNode::Scaled {
            factor: Rational::new(6, 5),
            content: bx(sym(SymbolKind::Open, delim)),
        };
        let got = write(vec![big]);
        reread(&got);
        if delim != "}{" {
            assert_eq!(got, latex);
        }
    }
}

#[test]
fn dropped_color_does_not_join_the_command_before() {
    let colored = MathNode::Color {
        color: "}{".into(),
        content: bx(id("x")),
    };
    assert_eq!(write(vec![id("α"), colored]), r"\alpha\mathord{x}");
}

#[test]
fn every_character_survives_in_every_leaf() {
    for ch in characters() {
        let s = ch.to_string();
        let mut leaves: Vec<MathNode> = KINDS.iter().map(|&kind| sym(kind, &s)).collect();
        leaves.push(MathNode::Number(s.clone()));
        leaves.push(id(&s));
        for limits in [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ] {
            leaves.push(MathNode::Operator(OperatorAttr {
                name: s.clone(),
                limit_style: limits,
            }));
            leaves.push(nary(ch, None, None, limits));
            leaves.push(nary(ch, Some("i"), Some("a"), limits));
        }
        for position in [AccentPosition::Above, AccentPosition::Below] {
            leaves.push(MathNode::Accent {
                base: bx(id("x")),
                accent: ch,
                position,
                wide: false,
            });
        }
        leaves.push(fenced(&s, ")"));
        leaves.push(fenced("(", &s));
        for leaf in leaves {
            for nodes in [
                vec![leaf.clone()],
                vec![id("α"), leaf.clone(), id("y")],
                vec![sub(leaf.clone())],
                vec![MathNode::Superscript {
                    base: bx(leaf.clone()),
                    sup: bx(id("z")),
                }],
                vec![MathNode::Fraction {
                    kind: FractionKind::Normal,
                    num: bx(leaf.clone()),
                    den: bx(id("d")),
                }],
                vec![MathNode::Root {
                    index: bx(leaf.clone()),
                    radicand: bx(id("r")),
                }],
            ] {
                let latex = write(nodes.clone());
                let read = reread(&latex);
                if nodes.len() > 1 || nodes[0] == leaf {
                    // Spaces and a lone script glyph are the exceptions: a
                    // space outside an operator atom is spacing.
                    // Names drop control characters.
                    let spacing = ch.is_whitespace() && !matches!(leaf, MathNode::Nary { .. })
                        || ch.is_control() && matches!(leaf, MathNode::Operator(_));
                    if !spacing {
                        let expected = drawn(&glyphs(&nodes)).replace('.', "");
                        let got = drawn(&glyphs(&read.expr().nodes)).replace('.', "");
                        let mut expected: Vec<char> = expected.chars().collect();
                        let mut got: Vec<char> = got.chars().collect();
                        expected.sort_unstable();
                        got.sort_unstable();
                        if expected != got {
                            panic!("{leaf:?}: {latex:?} reads as {:?}", read.expr().nodes);
                        }
                    }
                }
            }
        }
    }
}

/// TeX expands a color argument: only a color may be one.
#[test]
fn color_arguments_are_inert() {
    for spec in [
        r"\input{pwn}",
        r"\write18{ls}",
        r"\csname input\endcsname",
        "^^5cinput{pwn}",
        "#1",
        "red\n\n",
        "a%",
        "~",
        "$x$",
    ] {
        let boxed = |bg: &str, border: Option<&str>| MathNode::Boxed {
            content: bx(id("x")),
            bg_color: Some(bg.into()),
            border_color: border.map(String::from),
        };
        // A background alone draws no frame.
        assert_eq!(write(vec![boxed(spec, None)]), "x");
        assert_eq!(write(vec![boxed("red", Some(spec))]), r"\colorbox{red}{x}");
        assert_eq!(write(vec![boxed(spec, Some("red"))]), r"\boxed{x}");
        let color = MathNode::Color {
            color: spec.into(),
            content: bx(id("x")),
        };
        assert_eq!(write(vec![color]), "x");
    }
    for input in [
        r"\colorbox{\input{pwn}}{x}",
        r"\fcolorbox{\write18{ls}}{\input{pwn}}{x}",
        r"\begin{array}{c}\cellcolor{\input{a}}x\end{array}",
        r"\textcolor{\input{pwn}}{x}",
        r"\color{\input{pwn}}x",
        r"\definecolor{a}{rgb}{\input{pwn}}\colorbox{a}{x}",
        r"\rowcolors{1}{\input{a}}{\input{a}}\begin{array}{c}x\end{array}",
        r"\begin{array}{>{\columncolor{\input{a}}}c}x\end{array}",
    ] {
        let Ok(read) = mathema::from_latex(input) else {
            continue;
        };
        let (latex, lost) = read.to_latex_with_diagnostics().unwrap();
        for command in ["input", "write"] {
            assert!(!latex.contains(command), "{input}: {latex}");
        }
        let defined = input.contains("definecolor");
        assert!(defined || !lost.is_empty(), "{input}: {latex}");
    }
    let (latex, lost) = mathema::from_latex(r"\fcolorbox{red}{#ff0000}{x}")
        .unwrap()
        .to_latex_with_diagnostics()
        .unwrap();
    assert_eq!(latex, r"\fcolorbox{red}{#ff0000}{x}");
    assert!(lost.is_empty());
}

/// What a dropped color leaves is written as if the color had never been
/// there, and reads back that way.
#[test]
fn dropped_colors_leave_a_stable_expression() {
    for (input, latex) in [
        (r"\color{}\int\\ x", r"\int{}\\ x"),
        (r"a^'{\color{red_b}{}'}Ze", r"a^{\prime}{}^{\prime}Ze"),
        (r"1\textcolor{a=b}{2}3", "123"),
        (r"\alpha\textcolor{é}{x}", r"\alpha x"),
        (r"a\textcolor{$}{b+}c", "a{b+}c"),
        (r"x^\textcolor{a=b}{22}", r"x^{22}"),
        (r"\textcolor{a=b}{\lim}_x", r"{\mathord{\lim}}_x"),
        (
            r"\frac{\textcolor{a=b}{x}}{\colorbox{a=b}{y}}",
            r"\frac{x}{y}",
        ),
        (
            r"\textcolor{a=b}{\textcolor{red}{x}\textcolor{=}{y}}z",
            r"{\textcolor{red}{x}y}z",
        ),
        (r"\textcolor{a=b}{\scriptstyle x}y", r"{\scriptstyle x}y"),
        (
            r"\begin{array}{c}\cellcolor{a=b}x\end{array}",
            r"\begin{array}{c} x \end{array}",
        ),
        (r"\colorbox{my_c}{x}", r"\colorbox{my_c}{x}"),
        (r"\colorbox{red!!+}{x}", r"\colorbox{red!!+}{x}"),
    ] {
        let read = mathema::from_latex(input).unwrap();
        assert_eq!(read.to_latex().unwrap(), latex, "{input}");
        assert_eq!(reread(latex).to_latex().unwrap(), latex, "{input}");
    }
}

#[test]
fn loss_reports_are_bounded() {
    let colors = (0..1000)
        .map(|i| MathNode::Color {
            color: format!("={i}{}", "x".repeat(500)),
            content: bx(id("x")),
        })
        .collect();
    let compact = mathema::CompactExpr::from_math_expr(&expr(colors)).unwrap();
    let (latex, lost) =
        LatexWriter::write_compact_with_diagnostics(&compact, &Config::default()).unwrap();
    assert_eq!(latex, r"\mathord{x}".repeat(1000));
    assert_eq!(lost.len(), 101);
    assert!(lost.iter().all(|d| d.message.len() < 120), "{lost:?}");
    assert!(lost[100].message.starts_with("900 more"), "{:?}", lost[100]);
    // The same loss is reported once.
    let text = MathNode::Text(TextStyle::Normal, "\0".repeat(50));
    let compact = mathema::CompactExpr::from_math_expr(&expr(vec![text])).unwrap();
    let (_, lost) =
        LatexWriter::write_compact_with_diagnostics(&compact, &Config::default()).unwrap();
    assert_eq!(lost.len(), 1);
}

#[test]
fn apostrophes_are_not_two_superscripts() {
    for (nodes, latex) in [
        (vec![MathNode::Number("5''".into())], r"5\char`'{}\char`'"),
        (
            vec![
                sym(SymbolKind::Ordinary, "'"),
                sym(SymbolKind::Ordinary, "'"),
            ],
            r"\char`'{}\char`'",
        ),
        (
            vec![fenced("''", "")],
            r"\left. \mathopen{\char`'{}\char`'}x \right.",
        ),
    ] {
        assert_eq!(write(nodes), latex);
        assert_eq!(reread(latex).to_latex().unwrap(), latex);
    }
}

#[test]
fn text_is_inert() {
    let text = |style, s: &str| write(vec![MathNode::Text(style, s.into())]);
    for (s, latex) in [
        (r"\input{a}", r"\text{\textbackslash{}input\{a\}}"),
        ("a b", r"\text{a b}"),
        ("a\nb\tc", "\\text{a b\tc}"),
        ("a\n\0\nb", r"\text{a  b}"),
        ("a\r\n\r\nb", r"\text{a    b}"),
        ("a\x0Cb", r"\text{a b}"),
        ("a\0\x1B\x07\u{85}b", r"\text{ab}"),
        ("a-\0-b--c", r"\text{a-{}-b-{}-c}"),
        ("}\u{301}a\u{301}", "\\text{\\}\\char769{}a\u{301}}"),
    ] {
        assert_eq!(text(TextStyle::Normal, s), latex);
        assert_eq!(reread(latex).to_latex().unwrap(), latex);
    }
    // `\verb` cannot be used in the argument of a command.
    assert_eq!(
        text(TextStyle::Monospace, r"\input{a}"),
        r"\texttt{\char92{}input\char123{}a\char125{}}"
    );
    let name = |s: &str| {
        write(vec![MathNode::Operator(OperatorAttr {
            name: s.into(),
            limit_style: LimitStyle::SubSup,
        })])
    };
    assert_eq!(name("a\n\n\x0C b\0\x1B"), r"\operatorname{a    b}");
    assert_eq!(name(r"\~"), r"\operatorname{\char`\\\char`\~}");
    assert_eq!(name("\0si\0n"), r"\sin");
    assert_eq!(name("\0inj\u{2009}lim"), r"\injlim\nolimits");
    // Nested faces around nothing would be one face and join their neighbour.
    for (style, latex) in [
        (TextStyle::Bold, r"\textbf{}"),
        (TextStyle::BoldItalic, r"\mathord{\textbf{}}"),
        (TextStyle::SansSerifBoldItalic, r"\mathord{\textsf{}}"),
    ] {
        assert_eq!(text(style, "\0"), latex);
        let around = format!(r"\textbf{{a}}{latex}\textbf{{b}}");
        if style != TextStyle::Bold {
            assert_eq!(
                mathema::convert(&around, mathema::Format::LaTeX, mathema::Format::LaTeX).unwrap(),
                around
            );
        }
    }
    // What is lost is reported.
    let (latex, lost) = mathema::from_mathml("<math><mtext>a&#x7;b</mtext></math>")
        .unwrap()
        .to_latex_with_diagnostics()
        .unwrap();
    assert_eq!(latex, r"\text{ab}");
    assert!(
        lost.len() == 1 && lost[0].message.contains("U+0007"),
        "{lost:?}"
    );
}

#[test]
fn space_commands_are_grouped_as_script_arguments() {
    for (ch, latex) in [
        ("\u{2009}", r"x_{\,}"),
        ("\u{205F}", r"x_{\:}"),
        ("\u{2001}", r"x_{\quad }"),
    ] {
        assert_eq!(write(vec![sub(sym(SymbolKind::LargeOp, ch))]), latex);
        reread(latex);
    }
}

#[test]
fn unnamed_operators_are_stable() {
    for ch in characters() {
        for limits in [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ] {
            for (script, body) in [
                (None, None),
                (None, Some("a")),
                (Some("i"), Some("a")),
                (Some("α"), None),
            ] {
                let op = nary(ch, script, body, limits);
                for nodes in [
                    vec![op.clone()],
                    vec![id("α"), op.clone(), id("y")],
                    vec![sub(op.clone())],
                ] {
                    let latex = write(nodes);
                    assert_eq!(reread(&latex).to_latex().unwrap(), latex);
                }
            }
        }
        let op = sym(SymbolKind::LargeOp, &ch.to_string());
        for nodes in [
            vec![op.clone()],
            vec![id("x"), op.clone(), id("y")],
            vec![sub(op)],
        ] {
            let latex = write(nodes);
            assert_eq!(reread(&latex).to_latex().unwrap(), latex);
        }
    }
}

#[test]
fn every_format_escapes_characters() {
    let omml = |body: &str| {
        format!(
            r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">{body}</m:oMath>"#
        )
    };
    let run = |s: &str| format!("<m:r><m:t>{s}</m:t></m:r>");
    for ch in characters() {
        let mut xml = ch.to_string();
        if ch.is_control() {
            xml = format!("&#x{:X};", ch as u32);
        } else if matches!(ch, '<' | '>' | '&' | '"' | '\'') {
            xml = format!("&#{};", ch as u32);
        }
        let mut outputs = Vec::new();
        for body in [
            format!("<mo>{xml}</mo>"),
            format!("<mi>{xml}</mi>"),
            format!("<mn>1{xml}</mn>"),
            format!(r#"<mo largeop="true">{xml}</mo><mi>a</mi>"#),
            format!(r#"<msub><mi>x</mi><mo data-mjx-texclass="REL">{xml}</mo></msub>"#),
            format!(r#"<msub><mi>x</mi><mo data-mjx-texclass="OP">{xml}</mo></msub>"#),
            format!("<munder><mo>{xml}</mo><mi>i</mi></munder><mi>a</mi>"),
            format!("<mover><mi>x</mi><mo>{xml}</mo></mover>"),
            format!(r#"<mfenced open="{xml}" close="{xml}"><mi>x</mi></mfenced>"#),
            format!("<mtext>{xml}</mtext>"),
        ] {
            outputs.push(mathema::from_mathml(&format!("<math>{body}</math>")));
        }
        for body in [
            run(&xml),
            format!(
                r#"<m:nary><m:naryPr><m:chr m:val="{xml}"/></m:naryPr><m:sub>{}</m:sub><m:sup/><m:e>{}</m:e></m:nary>"#,
                run("i"),
                run("a")
            ),
            format!(
                r#"<m:d><m:dPr><m:begChr m:val="{xml}"/><m:endChr m:val="{xml}"/></m:dPr><m:e>{}</m:e></m:d>"#,
                run("x")
            ),
            format!(
                r#"<m:acc><m:accPr><m:chr m:val="{xml}"/></m:accPr><m:e>{}</m:e></m:acc>"#,
                run("x")
            ),
        ] {
            outputs.push(mathema::from_omml(&omml(&body)));
        }
        outputs.push(mathema::from_unicode_math(&format!("x_{ch} {ch}")));
        outputs.push(mathema::from_unicode_math(&format!("\\{ch}")));
        for output in outputs.into_iter().flatten() {
            reread(&output.to_latex().unwrap());
        }
    }
    // Reported inputs.
    let latex = |s: &str| {
        mathema::from_mathml(&format!("<math>{s}</math>"))
            .unwrap()
            .to_latex()
            .unwrap()
    };
    assert_eq!(
        latex(r#"<msub><mi>x</mi><mo data-mjx-texclass="REL">砀</mo></msub>"#),
        r"x_{\mathrel{砀}}"
    );
    assert_eq!(latex("<mn>50%</mn>"), r"50\%");
    let braces = omml(&format!(
        r#"<m:nary><m:naryPr><m:chr m:val="}}"/><m:limLoc m:val="subSup"/></m:naryPr><m:sub>{}</m:sub><m:sup/><m:e>{}</m:e></m:nary>"#,
        run("i"),
        run("a")
    ));
    assert_eq!(
        mathema::from_omml(&braces).unwrap().to_latex().unwrap(),
        r"\mathop{\}}\nolimits_ia"
    );
}

/// Fuzzer inputs that wrote unbalanced braces, or a script argument that is
/// not one token.
#[test]
fn mtef_fuzz_inputs() {
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    for (data, fragment) in [
        (
            &include_bytes!("fixtures/latex-escaping/mtef-brace-operator.bin")[..],
            r"^{\mathop{\}}\nolimits_t",
        ),
        (
            &include_bytes!("fixtures/latex-escaping/mtef-relation-in-script.bin")[..],
            r"\mathrel{",
        ),
    ] {
        let latex = mathema::from_mtef_bytes_with(data, &lenient)
            .unwrap()
            .to_latex()
            .unwrap();
        assert!(latex.contains(fragment), "{latex}");
        assert_eq!(reread(&latex).to_latex().unwrap(), latex);
    }
}

fn arb_text() -> impl Strategy<Value = String> + Clone {
    let special = prop::sample::select(
        "\\{}%#$&~^_'\"<>|-.8a \n\0\u{85}α砀∑≤²′\u{E000}⅀"
            .chars()
            .collect::<Vec<_>>(),
    );
    prop_oneof![
        4 => special.clone().prop_map(String::from),
        1 => prop::collection::vec(special, 2..4).prop_map(String::from_iter),
        1 => any::<char>()
            // The lexer joins a combining mark to the character before it.
            .prop_filter("combining mark", |ch| !matches!(ch, '\u{300}'..='\u{36F}'))
            .prop_map(String::from),
    ]
}

fn arb_node() -> impl Strategy<Value = MathNode> {
    let limits = prop::sample::select(vec![
        LimitStyle::Default,
        LimitStyle::AboveBelow,
        LimitStyle::SubSup,
    ]);
    let leaf = prop_oneof![
        (prop::sample::select(KINDS.to_vec()), arb_text()).prop_map(|(kind, s)| sym(kind, &s)),
        arb_text().prop_map(MathNode::Number),
        arb_text().prop_map(MathNode::Identifier),
        arb_text().prop_map(|s| MathNode::Text(TextStyle::Normal, s)),
        (arb_text(), limits.clone())
            .prop_map(|(name, limit_style)| MathNode::Operator(OperatorAttr { name, limit_style })),
    ];
    leaf.prop_recursive(3, 24, 3, move |inner| {
        let opt = prop::option::of(inner.clone().prop_map(bx));
        let first = arb_text().prop_map(|s| s.chars().next().unwrap()).boxed();
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(MathNode::Row),
            (arb_text(), prop::option::of(arb_text()), inner.clone()).prop_map(
                |(bg, border, x)| MathNode::Boxed {
                    content: bx(x),
                    bg_color: Some(bg),
                    border_color: border
                }
            ),
            (
                first.clone(),
                opt.clone(),
                opt,
                inner.clone(),
                limits.clone()
            )
                .prop_map(|(op, sub, sup, body, limits)| MathNode::Nary {
                    op,
                    sub,
                    sup,
                    body: bx(body),
                    limits
                }),
            (inner.clone(), inner.clone()).prop_map(|(base, sub)| MathNode::Subscript {
                base: bx(base),
                sub: bx(sub)
            }),
            (inner.clone(), inner.clone()).prop_map(|(base, sup)| MathNode::Superscript {
                base: bx(base),
                sup: bx(sup)
            }),
            (inner.clone(), first, any::<bool>()).prop_map(|(base, accent, above)| {
                MathNode::Accent {
                    base: bx(base),
                    accent,
                    position: if above {
                        AccentPosition::Above
                    } else {
                        AccentPosition::Below
                    },
                    wide: false,
                }
            }),
            (arb_text(), arb_text(), arb_text(), inner.clone()).prop_map(
                |(open, close, middle, x)| MathNode::Fenced {
                    open,
                    close,
                    content: vec![
                        FencedItem::Element(x.clone()),
                        FencedItem::Middle(middle),
                        FencedItem::Element(x)
                    ],
                }
            ),
            (arb_text(), inner.clone()).prop_map(|(color, x)| MathNode::Color {
                color,
                content: bx(x)
            }),
            (prop::sample::select(KINDS.to_vec()), arb_text()).prop_map(|(kind, s)| {
                MathNode::Scaled {
                    factor: Rational::new(6, 5),
                    content: bx(sym(kind, &s)),
                }
            }),
            (inner.clone(), inner.clone()).prop_map(|(num, den)| MathNode::Fraction {
                kind: FractionKind::Normal,
                num: bx(num),
                den: bx(den),
            }),
            (inner.clone(), inner).prop_map(|(index, radicand)| MathNode::Root {
                index: bx(index),
                radicand: bx(radicand)
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Whatever the characters, the output has balanced groups and is read
    /// in Strict mode, as is the output of writing that again.
    #[test]
    fn any_characters_stay_characters(nodes in prop::collection::vec(arb_node(), 1..4)) {
        let mut normalized = expr(nodes.clone());
        normalize_expr(&mut normalized);
        for nodes in [nodes, normalized.nodes] {
            let latex = write(nodes);
            prop_assert!(balanced(&latex), "unbalanced groups: {}", latex);
            let read = mathema::from_latex(&latex);
            prop_assert!(read.is_ok(), "{:?}: {:?}", latex, read.err());
            let again = read.unwrap().to_latex().unwrap();
            prop_assert!(balanced(&again));
            prop_assert!(mathema::from_latex(&again).is_ok(), "{:?} => {:?}", latex, again);
        }
    }
}

#[test]
fn dropped_colors_under_leading_scripts_are_stable() {
    for input in [
        r"_.\color{}[D\atop",
        r"\frac{_a\color{}bc}{2}",
        r"_a\color{}+x",
        r"_t\textcolor{\q}{}_dx",
        r"_t\color{@}_dx",
        r"''\textcolor{@}{'A}",
    ] {
        let latex =
            mathema::convert(input, mathema::Format::LaTeX, mathema::Format::LaTeX).unwrap();
        let again =
            mathema::convert(&latex, mathema::Format::LaTeX, mathema::Format::LaTeX).unwrap();
        assert_eq!(latex, again, "{input}");
        // Normalized first, and written directly.
        let direct = |source: &str| {
            use mathema::reader::Reader;
            let mut read =
                mathema::reader::latex::LatexReader::parse_compact(source, &Config::default())
                    .unwrap();
            mathema::ast::normalize::normalize_compact_expr(&mut read.expr).unwrap();
            LatexWriter::write_compact(&read.expr, &Config::default()).unwrap()
        };
        assert_eq!(direct(&direct(input)), direct(input), "{input}");
    }
}

/// A line break inside a dropped color stays in its group: bare in a table
/// cell, it would end the row. A declaration inside one ends with it.
#[test]
fn dropped_colors_keep_breaks_and_declarations_inside() {
    use mathema::Format::{LaTeX, MathML};
    use mathema::ast::normalize::normalize_compact_expr;
    use mathema::reader::Reader;
    use mathema::reader::latex::LatexReader;
    let config = Config::default();
    // Letters, cells, breaks, sizes and scripts; not how they are grouped.
    let drawn = |latex: &str| {
        let mathml = mathema::convert(latex, LaTeX, MathML).unwrap();
        let mut out = String::new();
        for part in mathml.split('<').skip(1) {
            let (tag, text) = part.split_once('>').unwrap();
            let name = tag
                .trim_start_matches('/')
                .split([' ', '/'])
                .next()
                .unwrap();
            if matches!(name, "mtr" | "mtd" | "mspace" | "mstyle" | "mfrac") {
                out.push_str(&format!("<{tag}>"));
            }
            out.push_str(text);
        }
        out
    };
    let plain = |wrapped: &str| {
        // The same source with plain groups for the dropped colors.
        let mut out = wrapped.to_string();
        for color in [
            r"\textcolor{a=b}",
            r"\textcolor{$}",
            r"\colorbox{a=b}",
            r"\textcolor{é}",
        ] {
            out = out.replace(color, "");
        }
        out
    };
    for color in [
        r"\textcolor{a=b}",
        r"\textcolor{$}",
        r"\colorbox{a=b}",
        r"\textcolor{é}",
    ] {
        for body in [
            // A whole cell, and part of one.
            r"\begin{array}{cc}C{a\\b}&c\end{array}",
            r"\begin{array}{cc}xC{a\\b}y&c\\d&C{e\\f}\end{array}",
            r"\begin{matrix}C{a\\b}&c\end{matrix}",
            r"\begin{pmatrix}c&xC{a\\b}\end{pmatrix}",
            r"\begin{cases}C{a\\b}&c\\d&yC{e\\f}\end{cases}",
            r"\begin{aligned}C{a\\b}&=c\\d&=xC{e\\f}\end{aligned}",
            // Nested colors, all dropped or not.
            r"\begin{array}{cc}C{C{a\\b}}&c\end{array}",
            r"\begin{array}{cc}xC{yC{a\\b}z}&c\end{array}",
            r"\begin{array}{cc}\textcolor{red}{C{a\\b}}&c\end{array}",
            r"\begin{array}{cc}C{\textcolor{red}{a\\b}}&c\end{array}",
            r"\begin{array}{cc}C{\\}&c\end{array}",
            // Declarations.
            r"C{\large x}y",
            r"C{a\large x}y",
            r"C{\scriptstyle a\\b}y",
            r"\begin{array}{cc}C{\large a\\b}&c\end{array}",
            r"\frac{C{\large x}}{C{a\\b}}",
            r"\sum C{\large x}y",
            r"_1C{\large x}y",
            r"_1C{a\\b}y",
            r"\left(C{a\\b}\right)",
        ] {
            let input = body.replace('C', color);
            let expected = plain(&input);
            // Heap and compact, as parsed and normalized.
            let heap = |source: &str, normal: bool| {
                let mut parsed = LatexReader::parse(source, &config).unwrap().expr;
                if normal {
                    normalize_expr(&mut parsed);
                }
                LatexWriter::write(&parsed, &config).unwrap()
            };
            let compact = |source: &str, normal: bool| {
                let mut parsed = LatexReader::parse_compact(source, &config).unwrap().expr;
                if normal {
                    normalize_compact_expr(&mut parsed).unwrap();
                }
                LatexWriter::write_compact(&parsed, &config).unwrap()
            };
            for write in [&heap as &dyn Fn(&str, bool) -> String, &compact] {
                for normal in [false, true] {
                    // The reader unwraps a group that is only a break, so
                    // without normalization that one is bare either way.
                    if !normal && body.contains(r"{\\}") {
                        continue;
                    }
                    let latex = write(&input, normal);
                    assert!(balanced(&latex), "{input}: {latex}");
                    assert_eq!(
                        write(&latex, normal),
                        latex,
                        "{input}, normalized: {normal}"
                    );
                    // MathML writes a leading script after what it leads.
                    if body.starts_with('_') {
                        continue;
                    }
                    assert_eq!(
                        drawn(&latex),
                        drawn(&write(&expected, normal)),
                        "{input}, normalized: {normal}: {latex}"
                    );
                }
            }
        }
    }
    assert_eq!(
        write(
            LatexReader::parse(r"\begin{array}{c}\textcolor{$}{a\\b}&c\end{array}", &config)
                .unwrap()
                .expr
                .nodes
        ),
        r"\begin{array}{c} \mathord{a\\ b} & c \end{array}"
    );
}

/// The colors LaTeX cannot hold are lost in LaTeX only: the other formats (and
/// the expression itself, however it was normalized) keep them.
#[test]
fn colors_stay_in_the_other_formats() {
    use mathema::Format::{LaTeX, MathML, Mtef, Omml};
    for color in [
        "#f00",
        "#ff000080",
        "#f008",
        "#ff0000",
        "hsl(120,50%,50%)",
        "hsl(120 50% 50%)",
        "rgb(1, 2, 3)",
        "rgba(255,0,0,.5)",
        "rebeccapurple",
        "var(--x_y)",
    ] {
        let mathml =
            format!(r#"<math><mi>a</mi><mstyle mathcolor="{color}"><mi>x</mi></mstyle></math>"#);
        let out = mathema::convert(&mathml, MathML, MathML).unwrap();
        assert!(
            out.contains(&format!(r#"mathcolor="{color}""#)),
            "{color}: {out}"
        );
        reread(&mathema::convert(&mathml, MathML, LaTeX).unwrap());
        for format in [Omml, Mtef] {
            mathema::convert(&mathml, MathML, format).unwrap();
        }
        for node in [
            MathNode::Color {
                color: color.into(),
                content: bx(id("x")),
            },
            MathNode::Boxed {
                content: bx(id("x")),
                bg_color: Some(color.into()),
                border_color: None,
            },
        ] {
            let mut normal = expr(vec![MathNode::Row(vec![id("α"), node.clone()])]);
            normalize_expr(&mut normal);
            assert_eq!(normal.nodes, [id("α"), node.clone()], "{color}");
            let mut compact = mathema::CompactExpr::from_math_expr(&normal).unwrap();
            mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
            assert_eq!(compact.to_math_expr(), normal, "{color}");
            let mathml =
                mathema::writer::mathml::MathmlWriter::write(&normal, &Config::default()).unwrap();
            assert!(mathml.contains(color), "{color}: {mathml}");
        }
        let latex = format!(r"\textcolor{{{color}}}{{x}}");
        if !color.contains(['%', '_'])
            && let Ok(out) = mathema::convert(&latex, LaTeX, MathML)
        {
            assert!(
                out.contains(&format!(r#"mathcolor="{color}""#)),
                "{color}: {out}"
            );
        }
    }
    // OMML keeps a color in the host's run properties, MTEF in its color table.
    let options = mathema::OmmlOptions::default()
        .with_run_properties(mathema::OmmlRunProperties::WordprocessingMl);
    for color in ["#f00", "#ff0000", "red"] {
        let mathml = format!(r#"<math><mstyle mathcolor="{color}"><mi>x</mi></mstyle></math>"#);
        let read = mathema::from_mathml(&mathml).unwrap();
        let (omml, lost) = read.to_omml_with_options(&options).unwrap();
        assert!(omml.contains("FF0000"), "{color}: {omml} {lost:?}");
    }
    for (color, hex) in [("#f00", "FF0000"), ("#ff0000", "FF0000")] {
        let mathml = format!(r#"<math><mstyle mathcolor="{color}"><mi>x</mi></mstyle></math>"#);
        let mtef = mathema::convert(&mathml, MathML, Mtef).unwrap();
        let back = mathema::convert(&mtef, Mtef, MathML).unwrap();
        assert!(
            back.to_uppercase().contains(hex) || back.contains("red"),
            "{color}: {back}"
        );
    }
}

/// A list the arena cannot hold is an error, never empty output.
#[test]
fn large_dropped_colors_are_written_or_refused() {
    use mathema::Format::{LaTeX, MathML};
    let run = "<mi>x</mi>".repeat(40_000);
    let one = format!("<mstyle mathcolor='#f00'>{run}</mstyle>");
    for input in [
        format!("<math>{one}</math>"),
        format!("<math>{one}{one}</math>"),
    ] {
        let read = mathema::from_mathml(&input).unwrap();
        let colors = input.matches("mstyle").count() / 2;
        for latex in [
            Some(read.to_latex().unwrap()),
            LatexWriter::write(&read.expr(), &Config::default()).ok(),
            LatexWriter::write_compact(read.compact_expr(), &Config::default()).ok(),
            mathema::convert(&input, MathML, LaTeX).ok(),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(latex.matches('x').count(), colors * 40_000);
        }
    }
}

/// What is dropped in an expression built in code stays a group: nothing
/// leaks out of it, however it is written.
#[test]
fn dropped_colors_in_built_expressions() {
    let colored = |node: MathNode| MathNode::Color {
        color: "a&".into(),
        content: bx(node),
    };
    let boxed = |node: MathNode| MathNode::Boxed {
        content: bx(node),
        bg_color: Some("a&".into()),
        border_color: None,
    };
    for wrap in [&colored as &dyn Fn(MathNode) -> MathNode, &boxed] {
        for node in [
            MathNode::SizeSwitch(6),
            MathNode::StyleSwitch(2),
            MathNode::LineBreak,
            MathNode::Row(vec![MathNode::SizeSwitch(6), id("a")]),
            MathNode::Row(vec![id("a"), MathNode::LineBreak, id("b")]),
            MathNode::Row(Vec::new()),
            id("a"),
        ] {
            for nodes in [
                vec![wrap(node.clone()), id("x")],
                vec![id("y"), wrap(wrap(node.clone())), id("x")],
                vec![MathNode::PreScripts {
                    base: bx(wrap(node.clone())),
                    pre_sub: Some(bx(id("p"))),
                    pre_sup: None,
                }],
                vec![sub(wrap(node.clone()))],
                vec![
                    MathNode::Subscript {
                        base: bx(wrap(node.clone())),
                        sub: bx(id("s")),
                    },
                    id("x"),
                ],
            ] {
                let mut normal = expr(nodes.clone());
                normalize_expr(&mut normal);
                let latex = write(normal.nodes);
                // Read back and normalized, it is written the same.
                let again =
                    mathema::convert(&latex, mathema::Format::LaTeX, mathema::Format::LaTeX)
                        .unwrap();
                assert_eq!(again, latex, "{nodes:?}");
                // A declaration and a break end with the group.
                for mark in [r"\Large", r"\scriptstyle"] {
                    assert!(!latex.starts_with(mark), "{nodes:?}: {latex}");
                }
            }
        }
    }
}

/// Deep nests of dropped and kept colors are written in linear time.
#[test]
fn nested_dropped_colors_are_linear() {
    let mut node = id("x");
    for depth in 0..60 {
        node = MathNode::Color {
            color: if depth % 2 == 0 { "a&" } else { "red" }.into(),
            content: bx(MathNode::Row(vec![node, id("y")])),
        };
    }
    let latex = write(vec![node]);
    assert_eq!(latex.matches(r"\textcolor{red}").count(), 30);
    assert_eq!(latex.matches('y').count(), 60);
    reread(&latex);
}

/// A name with a character token is a math list when it is read back: its
/// other characters are written as the atoms they are read as.
#[test]
fn names_with_character_tokens_are_stable() {
    let name = |s: &str| {
        write(vec![
            MathNode::Operator(OperatorAttr {
                name: s.into(),
                limit_style: LimitStyle::SubSup,
            }),
            id("z"),
        ])
    };
    assert_eq!(name(r"mts i\p"), r"\operatorname{mtsi\char`\\p}z");
    assert_eq!(name(r"\<d"), r"\operatorname{\char`\\\char`<d}z");
    assert_eq!(name("a b~∂y"), r"\operatorname{ab\char`\~\partial y}z");
    // An argument in a name follows the same rule.
    let braket = mathema::convert(
        r"\operatorname{\braket*}",
        mathema::Format::LaTeX,
        mathema::Format::LaTeX,
    );
    assert_eq!(
        braket.unwrap(),
        r"\operatorname{\mathinner{\langle*\rangle}}"
    );
    for input in [
        "\\operatorname{\0lim}0",
        "\\hat{\\operatorname*{\0lim}}",
        "\\operatorname*{\0lim}_x y",
        "\\operatorname{s\0in}x",
        r"\operatorname\not",
        r"\ip{{'\middle~}}d",
        r"\ip{{\middle~d}'}d",
        r"\left('\middle~'x\right)",
        r"\left(a\middle~_1 x\middle\^ '\right)",
        r"\frac{'\middle~}{\middle~'}",
        "\\operatorname{\u{301}}x",
    ] {
        let once = mathema::convert(input, mathema::Format::LaTeX, mathema::Format::LaTeX).unwrap();
        let twice = mathema::convert(&once, mathema::Format::LaTeX, mathema::Format::LaTeX);
        assert_eq!(twice.unwrap(), once, "{input:?}");
    }
    for ch in characters() {
        for name in [
            name(&format!("{ch}^")),
            name(&format!("x{ch}~1{ch}y")),
            name(&format!("\\R{ch}d{ch}\\{ch}e")),
        ] {
            assert_eq!(reread(&name).to_latex().unwrap(), name);
            let again = mathema::convert(&name, mathema::Format::LaTeX, mathema::Format::LaTeX);
            assert_eq!(again.unwrap(), name);
        }
    }
}

/// What is written after a control word does not depend on how a non-ASCII
/// letter there came to be.
#[test]
fn non_ascii_letters_after_control_words_are_stable() {
    use mathema::Format::{LaTeX, MathML};
    for (input, from, latex) in [
        (r"\alpha\char`é", LaTeX, r"\alphaé"),
        (r"\lVert\char17701", LaTeX, "\\lVert\u{4525}"),
        ("<math><mi>α</mi><mo>é</mo></math>", MathML, r"\alphaé"),
        (
            "<math><mi>α</mi><mi>ʰ</mi></math>",
            MathML,
            r"\alpha\char`ʰ",
        ),
    ] {
        let written = mathema::convert(input, from, LaTeX).unwrap();
        assert_eq!(written, latex, "{input}");
        assert_eq!(mathema::convert(&written, LaTeX, LaTeX).unwrap(), latex);
    }
}

/// Text and names are written in linear time.
#[test]
fn long_text_and_names_are_linear() {
    let n = 100_000;
    let start = std::time::Instant::now();
    for node in [
        MathNode::Text(TextStyle::Monospace, "x\u{327}".repeat(n) + "\\"),
        MathNode::Text(TextStyle::Monospace, "x\u{327}".repeat(n)),
        MathNode::Text(TextStyle::Normal, "x\u{327}-\0".repeat(n)),
        MathNode::Operator(OperatorAttr {
            name: "a ".repeat(n) + "^",
            limit_style: LimitStyle::SubSup,
        }),
        MathNode::Operator(OperatorAttr {
            name: "a\0é1".repeat(n) + "^",
            limit_style: LimitStyle::SubSup,
        }),
        MathNode::Number("5'".repeat(n)),
        id(&"α\u{e9}".repeat(n)),
        fenced(&"'".repeat(n), &"\\".repeat(n)),
    ] {
        assert!(write(vec![node]).len() > n);
    }
    // About 0.1 s in a debug build; quadratic, each is many seconds.
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

/// A combining mark that is all of a formula in text does not join the `$`.
#[test]
fn combining_mark_in_inline_math() {
    let mathml = "<math><menclose notation='actuarial'><mi>&#x301;</mi></menclose></math>";
    let latex = mathema::from_mathml(mathml).unwrap().to_latex().unwrap();
    assert_eq!(latex, "\\angl{$\\relax \u{301}$}");
    assert_eq!(reread(&latex).to_latex().unwrap(), latex);
}

/// A script beside a literal delimiter stays where it is: not on the atom.
#[test]
fn scripts_beside_literal_delimiters_are_stable() {
    use mathema::Format::{LaTeX, MathML, Omml};
    use mathema::reader::Reader;
    use mathema::reader::latex::LatexReader;
    let config = Config::default();
    let scripts = [
        "",
        "'",
        "_1",
        "^2",
        "_1^2",
        "{}_1",
        "{}'",
        "''",
        "{}_{a}b",
        "x'",
        "x_1",
        "x{}_1",
        r"{}_{}",
        r"\sum",
        r"\sum{}_1",
        r"\lim",
        r"\lim{}_x",
        "{_1}",
        r"{\large x}'",
        r"\textcolor{@}{'}",
        r"\textcolor{@}{_1x}",
        r"\\ '",
        r"{}_1\middle|'",
        r"\middle~'",
        r"'\middle~",
        r"a\middle\^{}_1b",
        r"\frac{'}{_1}",
        "{'}",
    ];
    let write = |read: &MathExpr| LatexWriter::write(read, &config).unwrap();
    let (mut checked, mut unstable) = (0, 0);
    for before in scripts {
        for after in scripts {
            for (open, close) in [
                ("(", r"\~"),
                (r"\~", ")"),
                (r"\^", r"\_"),
                (r"\~", "."),
                (".", r"\%"),
                ("/", r"\~"),
            ] {
                for outer in ["", "'", "_2", "{}^3", "z"] {
                    let source = format!(r"\left{open} {before} {after} \right{close} {outer}");
                    let Ok(read) = mathema::from_latex(&source) else {
                        continue;
                    };
                    checked += 1;
                    // Normalized: compact, heap, and `convert`.
                    let compact = |source: &str| {
                        let mut read = LatexReader::parse_compact(source, &config).unwrap();
                        mathema::ast::normalize::normalize_compact_expr(&mut read.expr).unwrap();
                        LatexWriter::write_compact(&read.expr, &config).unwrap()
                    };
                    let heap = |source: &str| {
                        let mut read = LatexReader::parse(source, &config).unwrap().expr;
                        normalize_expr(&mut read);
                        write(&read)
                    };
                    let convert = |source: &str| mathema::convert(source, LaTeX, LaTeX).unwrap();
                    for (name, f) in [
                        ("compact", &compact as &dyn Fn(&str) -> String),
                        ("heap", &heap),
                        ("convert", &convert),
                    ] {
                        let once = f(&source);
                        assert!(balanced(&once), "{source}: {once}");
                        if f(&once) != once {
                            // Only what an ordinary fence does not keep either.
                            let ordinary = f(&format!(r"\left( {before} {after} \right) {outer}"));
                            assert_ne!(f(&ordinary), ordinary, "{name}: {source}: {once}");
                            unstable += 1;
                        }
                    }
                    // As read.
                    let builder = read.to_latex().unwrap();
                    reread(&builder);
                }
            }
        }
    }
    assert!(
        checked > 10_000 && unstable < checked / 20,
        "{checked} {unstable}"
    );
    // Exact spellings: only a script without a base needs the null delimiter.
    for (source, latex) in [
        (
            r"\left/ ' \right\~",
            r"\left/ {}^{\prime} \middle. \mathclose{\char`\~} \right.",
        ),
        (
            r"\left/ x' \right\~",
            r"\left/ x^{\prime}\mathclose{\char`\~} \right.",
        ),
        (
            r"\left\~ 'x \right)",
            r"\left. \mathopen{\char`\~}{{}^{\prime}x} \right)",
        ),
        (r"\left( {}_1 \right)", r"\left( {}_{1} \right)"),
    ] {
        assert_eq!(mathema::convert(source, LaTeX, LaTeX).unwrap(), latex);
    }
    // The same fence from other formats.
    let mathml =
        "<math><mfenced open='/' close='~'><msup><mrow/><mo>&#x2032;</mo></msup></mfenced></math>";
    let latex = mathema::convert(mathml, MathML, LaTeX).unwrap();
    assert_eq!(mathema::convert(&latex, LaTeX, LaTeX).unwrap(), latex);
    let omml = mathema::convert(mathml, MathML, Omml).unwrap();
    let latex = mathema::convert(&omml, Omml, LaTeX).unwrap();
    assert_eq!(mathema::convert(&latex, LaTeX, LaTeX).unwrap(), latex);
}

#[test]
fn combining_mark_at_the_start_of_a_name() {
    for (name, latex) in [
        ("\u{301}", r"\operatorname{\char769 }"),
        ("\u{301}b", r"\operatorname{\char769 b}"),
        ("\u{301}^", r"\operatorname{\char769 \char`\^}"),
        ("a\u{301}", "\\operatorname{a\u{301}}"),
    ] {
        let op = MathNode::Operator(OperatorAttr {
            name: name.into(),
            limit_style: LimitStyle::SubSup,
        });
        assert_eq!(write(vec![op.clone()]), latex);
        let boxed = write(vec![MathNode::Actuarial {
            content: bx(op),
            text_mode: true,
        }]);
        assert_eq!(boxed, format!(r"\angl{{${latex}$}}"));
        for latex in [latex, &boxed] {
            assert_eq!(reread(latex).to_latex().unwrap(), latex);
        }
    }
}

/// Dropped text in a text box leaves the math around it one run.
#[test]
fn dropped_text_between_inline_math() {
    for (input, latex) in [
        ("\\angl{$h$\u{14}$`$}", r"\angl{$h`$}"),
        ("\\angl{a\0$x$\0\\textbf{\0}$y$b}", r"\angl{a$xy$b}"),
        ("\\reflectbox{$x$\0$y$}", r"\reflectbox{$xy$}"),
        (r"\angl{$x$ $y$}", r"\angl{$x$ $y$}"),
        (r"\angl{a\texttt{}b}", r"\angl{ab}"),
        (r"\angl{\verb5}", r"\angl{}"),
        ("\\angl{a\\texttt{\0}b}", r"\angl{a\texttt{\char0{}}b}"),
    ] {
        let written = mathema::convert(input, mathema::Format::LaTeX, mathema::Format::LaTeX);
        assert_eq!(written.unwrap(), latex);
        assert_eq!(reread(latex).to_latex().unwrap(), latex);
    }
}
