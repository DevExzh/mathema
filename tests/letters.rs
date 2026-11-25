//! Letters that are separate in the source are separate in the tree: TeX sets
//! each as an atom, MathML gives each its `<mi>` (MathML Core 3.2.2: one of
//! several letters would be upright), and a run of OMML says how text looks.

use mathema::ast::{LimitStyle, OperatorAttr};
use mathema::{Format, MathNode, convert, from_mathml, from_omml};

fn body(mathml: &str) -> &str {
    let start = mathml.find('>').unwrap() + 1;
    &mathml[start..mathml.len() - "</math>".len()]
}

fn mathml(source: &str, from: Format) -> String {
    body(&convert(source, from, Format::MathML).unwrap()).to_owned()
}

fn omml(body: &str) -> String {
    format!(
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">{body}</m:oMath>"#
    )
}

fn run(text: &str, style: &str) -> String {
    let style = match style {
        "" => String::new(),
        style => format!(r#"<m:rPr><m:sty m:val="{style}"/></m:rPr>"#),
    };
    format!("<m:r>{style}<m:t>{text}</m:t></m:r>")
}

#[test]
fn letters_of_latex() {
    for (source, expected) in [
        ("kr", "<mi>k</mi><mi>r</mi>"),
        ("2xy", "<mn>2</mn><mi>x</mi><mi>y</mi>"),
        (r"\sin xy", "<mo>sin</mo><mi>x</mi><mi>y</mi>"),
        (
            r"\frac{ab}{c}",
            "<mfrac><mrow><mi>a</mi><mi>b</mi></mrow><mi>c</mi></mfrac>",
        ),
        (
            r"\left(kr\right)",
            r#"<mrow><mo fence="true">(</mo><mi>k</mi><mi>r</mi><mo fence="true">)</mo></mrow>"#,
        ),
        ("{k}{r}", "<mi>k</mi><mi>r</mi>"),
        (
            r"\mathit{kr}",
            r#"<mstyle mathvariant="italic"><mi>k</mi><mi>r</mi></mstyle>"#,
        ),
        // The style is spelled on each letter, as KaTeX does: `mstyle` has no
        // `mathvariant` in MathML Core.
        (
            r"\mathrm{kr}",
            r#"<mstyle mathvariant="normal"><mi mathvariant="normal">k</mi><mi mathvariant="normal">r</mi></mstyle>"#,
        ),
        (r"\operatorname{kr}", "<mo>kr</mo>"),
    ] {
        assert_eq!(mathml(source, Format::LaTeX), expected, "{source}");
        let latex = convert(source, Format::LaTeX, Format::LaTeX).unwrap();
        assert_eq!(mathml(&latex, Format::LaTeX), expected, "{latex}");
    }
}

#[test]
fn letters_of_the_other_formats() {
    let apart = "<mi>k</mi><mi>r</mi>";
    let source = "<math><mi>k</mi><mi>r</mi></math>";
    assert_eq!(mathml(source, Format::MathML), apart);
    assert_eq!(mathml("kr", Format::UnicodeMath), apart);
    // One identifier stays one.
    let source = r#"<math><mi>kr</mi><mi>sin</mi><mi>x</mi></math>"#;
    assert_eq!(
        mathml(source, Format::MathML),
        r#"<mi mathvariant="normal">kr</mi><mi mathvariant="normal">sin</mi><mi>x</mi>"#
    );
    // Word's default style is italic ([MS-OI29500] 2.1.1705).
    for source in [
        omml(&run("kr", "")),
        omml(&run("kr", "i")),
        omml(&(run("k", "") + &run("r", ""))),
    ] {
        assert_eq!(mathml(&source, Format::Omml), apart, "{source}");
        let written = convert(&source, Format::Omml, Format::Omml).unwrap();
        assert_eq!(written, omml(&(run("k", "") + &run("r", ""))));
    }
    let plain = omml(&run("kr", "p"));
    let MathNode::Styled(_, inside) = &from_omml(&plain).unwrap().expr().nodes[0] else {
        panic!("{:?}", from_omml(&plain).unwrap().expr());
    };
    assert_eq!(inside[..], [MathNode::Identifier("kr".into())]);
    // A function name is one in an upright run or one of no style.
    for style in ["", "p"] {
        assert_eq!(
            from_omml(&omml(&run("sin", style))).unwrap().expr().nodes,
            [MathNode::Operator(OperatorAttr {
                name: "sin".into(),
                limit_style: LimitStyle::SubSup,
            })]
        );
    }
}

/// A word of a style is in that style, an upright one upright.
#[test]
fn styled_words() {
    for (variant, source) in [
        ("italic", r#"<mi mathvariant="italic">kr</mi>"#),
        ("bold", r#"<mi mathvariant="bold">kr</mi>"#),
    ] {
        let source = format!("<math>{source}</math>");
        let expected = format!(
            r#"<mstyle mathvariant="{variant}"><mi mathvariant="{variant}">kr</mi></mstyle>"#
        );
        assert_eq!(mathml(&source, Format::MathML), expected);
        let again = format!("<math>{expected}</math>");
        assert_eq!(mathml(&again, Format::MathML), expected);
        assert_eq!(
            from_mathml(&source).unwrap().expr().nodes,
            from_mathml(&again).unwrap().expr().nodes
        );
    }
}

/// The name of an operator is one, whatever it is made of.
#[test]
fn letters_of_operator_names() {
    for source in [
        r"\varinjlim",
        r"\varliminf",
        r"\operatorname*{\underline{lim}}",
    ] {
        let written = mathml(source, Format::LaTeX);
        assert!(written.contains(">lim<"), "{source}: {written}");
        let heap = mathema::from_latex(source).unwrap();
        let mut normalized = heap.expr();
        mathema::ast::normalize::normalize_expr(&mut normalized);
        assert!(format!("{normalized:?}").contains(r#"Identifier("lim")"#));
    }
}

/// The heap route spells letters as the compact one does.
#[test]
fn heap_and_compact_agree_on_letters() {
    use mathema::writer::{Writer, mathml::MathmlWriter};
    let config = mathema::Config::default();
    for source in [
        r"\mathrm{kr}x",
        r"\mathit{kr}",
        r"\mathbf{k\mathrm{r}}",
        r"\varinjlim kr",
    ] {
        let read = mathema::from_latex(source).unwrap();
        let mut heap = read.expr();
        let mut compact = read.compact_expr().clone();
        for _ in 0..2 {
            assert_eq!(
                MathmlWriter::write(&heap, &config).unwrap(),
                <MathmlWriter as Writer>::write_compact(&compact, &config).unwrap(),
                "{source}"
            );
            mathema::ast::normalize::normalize_expr(&mut heap);
            mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        }
    }
}

fn omml_again(source: &str) -> String {
    convert(source, Format::Omml, Format::Omml).unwrap()
}

/// The name of an `m:func` is one identifier, whatever its style; what is above
/// or below it, and its argument, are not names.
#[test]
fn function_names_of_omml() {
    let func = |name: &str, argument: &str| {
        omml(&format!(
            "<m:func><m:fName>{name}</m:fName><m:e>{argument}</m:e></m:func>"
        ))
    };
    for name in ["erf", "rank", "tr", "foo"] {
        for style in ["", "i", "p"] {
            let source = func(&run(name, style), &run("x", ""));
            let nodes = format!("{:?}", from_omml(&source).unwrap().expr().nodes);
            assert!(
                nodes.contains(&format!("Identifier(\"{name}\")")),
                "{nodes}"
            );
            // Written upright, as a function, and stable.
            let written = omml_again(&source);
            let expected = func(&run(name, "p"), &run("x", ""));
            assert_eq!(written, expected, "{name} {style}");
            assert_eq!(omml_again(&written), written);
        }
    }
    let written = omml_again(&func(&run("sin", "i"), &run("x", "")));
    assert_eq!(written, func(&run("sin", "p"), &run("x", "")));
    // A limit below the name, and an argument of letters.
    let limit = format!(
        "<m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow>",
        run("argfoo", ""),
        run("xy", "")
    );
    let read = from_omml(&func(&limit, &run("ab", ""))).unwrap();
    let nodes = format!("{:?}", read.expr().nodes);
    for name in ["argfoo", "x", "y", "a", "b"] {
        assert!(
            nodes.contains(&format!("Identifier(\"{name}\")")),
            "{name} {nodes}"
        );
    }
    // Nor in what follows a function, or in the argument of one in a name, and
    // what follows that is a name again.
    let nested = func(&run("tr", ""), &run("AB", "")).replace(&omml(""), "");
    let nested = &nested[nested.find("<m:func>").unwrap()..nested.rfind("</m:oMath>").unwrap()];
    let read = from_omml(&omml(&format!(
        "<m:func><m:fName>{nested}{}</m:fName><m:e>{}</m:e></m:func>{}",
        run("foo", ""),
        run("uv", ""),
        run("yz", "")
    )))
    .unwrap();
    let nodes = format!("{:?}", read.expr().nodes);
    for name in ["tr", "A", "B", "foo", "u", "v", "y", "z"] {
        assert!(
            nodes.contains(&format!("Identifier(\"{name}\")")),
            "{name} {nodes}"
        );
    }
    // In a script of a name there is none either, and in a function inside an
    // argument there is one again.
    let inner = "<m:func><m:fName><m:r><m:t>tr</m:t></m:r></m:fName><m:e><m:r><m:t>AB</m:t></m:r></m:e></m:func>";
    let script = format!(
        "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
        run("rank", ""),
        run("kl", "")
    );
    let read = from_omml(&func(&script, inner)).unwrap();
    let nodes = format!("{:?}", read.expr().nodes);
    for name in ["rank", "k", "l", "tr", "A", "B"] {
        assert!(
            nodes.contains(&format!("Identifier(\"{name}\")")),
            "{name} {nodes}"
        );
    }
}

/// An identifier of several letters is upright in OMML as in MathML.
#[test]
fn words_are_upright_in_omml() {
    for (source, word) in [
        ("<mi>foo</mi>", "foo"),
        ("<mi>foo</mi><mo>+</mo><mi>x</mi>", "foo"),
        ("<apply><plus/><ci>foo</ci><ci>x</ci></apply>", "foo"),
        ("<msub><mi>sin</mi><mi>x</mi></msub>", "sin"),
    ] {
        let source = format!("<math>{source}</math>");
        let written = convert(&source, Format::MathML, Format::Omml).unwrap();
        assert!(written.contains(&run(word, "p")), "{source}: {written}");
        assert_eq!(omml_again(&written), written, "{source}");
        // It comes back upright.
        let mathml = convert(&written, Format::Omml, Format::MathML).unwrap();
        let upright = [
            format!(r#"mathvariant="normal">{word}<"#),
            format!("<mo>{word}<"),
        ];
        assert!(upright.iter().any(|w| mathml.contains(w)), "{mathml}");
    }
    // What is no word is left alone.
    let written = convert("<math><mi>x1</mi></math>", Format::MathML, Format::Omml).unwrap();
    assert!(written.contains(&run("x1", "i")), "{written}");
    let written = convert("<math><mi/><mi>x</mi></math>", Format::MathML, Format::Omml).unwrap();
    assert!(!written.contains(r#"m:val="p""#), "{written}");
    let italic = r#"<math><mi mathvariant="italic">foo</mi></math>"#;
    let written = convert(italic, Format::MathML, Format::Omml).unwrap();
    assert!(written.contains(&run("foo", "i")), "{written}");
    let letters: String = ["f", "o", "o"].map(|letter| run(letter, "")).concat();
    assert_eq!(omml_again(&written), omml(&letters));
}

/// OMML with names, words and letters settles after one pass.
/// The outline of OMML without its runs, only their text.
fn outline(omml: &str) -> String {
    let mut out = String::new();
    for part in omml.split('<').skip(1) {
        let (tag, text) = part.split_once('>').unwrap();
        let name = tag.trim_start_matches('/').split(' ').next().unwrap();
        if !matches!(name, "m:r" | "m:rPr" | "m:sty" | "m:t" | "m:scr") && !tag.ends_with('/') {
            out.push_str(&format!("<{tag}>"));
        }
        out.push_str(text);
    }
    out
}

/// What each function of some OMML is, with its argument.
fn functions(omml: &str) -> Vec<String> {
    let outline = outline(omml);
    outline
        .match_indices("<m:func>")
        .map(|(start, _)| {
            let mut depth = 0;
            let end = outline[start..]
                .match_indices("m:func>")
                .find(|(at, _)| {
                    depth += if outline[start..][..*at].ends_with('/') {
                        -1
                    } else {
                        1
                    };
                    depth == 0
                })
                .map(|(at, tag)| start + at + tag.len())
                .unwrap();
            outline[start..end].to_owned()
        })
        .collect()
}

#[test]
fn generated_omml_settles() {
    struct Random(u64);
    impl Random {
        fn below(&mut self, limit: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) % limit
        }
        fn run(&mut self) -> String {
            let words = [
                "x", "kr", "erf", "sin", "lim", "foo", "2x", "a+b", "αβ", "Pr", ",",
            ];
            let text = words[self.below(words.len() as u64) as usize];
            run(text, ["", "i", "p", "b", "bi"][self.below(5) as usize])
        }
        fn list(&mut self, depth: u64) -> String {
            (0..1 + self.below(3)).map(|_| self.item(depth)).collect()
        }
        fn item(&mut self, depth: u64) -> String {
            if depth == 0 {
                return self.run();
            }
            let depth = depth - 1;
            match self.below(8) {
                0 => {
                    let mut name = self.run();
                    let argument = self.list(depth);
                    match self.below(4) {
                        0 => {
                            let sup = self.run();
                            name =
                                format!("<m:sSup><m:e>{name}</m:e><m:sup>{sup}</m:sup></m:sSup>");
                        },
                        1 => {
                            let lim = self.run();
                            name = format!(
                                "<m:limLow><m:e>{name}</m:e><m:lim>{lim}</m:lim></m:limLow>"
                            );
                        },
                        _ => {},
                    }
                    format!("<m:func><m:fName>{name}</m:fName><m:e>{argument}</m:e></m:func>")
                },
                1 => {
                    let (base, sub) = (self.list(depth), self.list(depth));
                    format!("<m:sSub><m:e>{base}</m:e><m:sub>{sub}</m:sub></m:sSub>")
                },
                2 => {
                    let (num, den) = (self.list(depth), self.list(depth));
                    format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>")
                },
                3 => {
                    let (base, lim) = (self.run(), self.list(depth));
                    format!("<m:limLow><m:e>{base}</m:e><m:lim>{lim}</m:lim></m:limLow>")
                },
                4 => {
                    let content = self.list(depth);
                    format!("<m:d><m:e>{content}</m:e></m:d>")
                },
                _ => self.run(),
            }
        }
    }
    let mut unsettled = Vec::new();
    let mut changed = Vec::new();
    for seed in 0..3000u64 {
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let source = omml(&random.list(2));
        let Ok(first) = convert(&source, Format::Omml, Format::Omml) else {
            continue;
        };
        // A name of several nodes is one only in its last.
        let outlined = outline(&source);
        let named = !outlined.contains("<m:fName>2x") && !outlined.contains("<m:fName>a+b");
        if named && functions(&first) != functions(&source) {
            changed.push((source.clone(), first.clone()));
        }
        if omml_again(&first) != first {
            unsettled.push(source);
        }
    }
    assert!(
        changed.is_empty(),
        "{} {:?}",
        changed.len(),
        changed.iter().min_by_key(|(s, _)| s.len())
    );
    assert!(
        unsettled.is_empty(),
        "{} {:?}",
        unsettled.len(),
        unsettled.iter().min_by_key(|s| s.len())
    );
}

/// Letters are apart in every other list, and in the names of MTEF.
#[test]
fn where_letters_are_not_joined() {
    for (source, expected) in [
        (r"\mathord{ab}", "<mi>a</mi><mi>b</mi>"),
        (r"\foo{ab}", "<mi>a</mi><mi>b</mi>"),
    ] {
        let written = convert(source, Format::LaTeX, Format::MathML).unwrap();
        assert!(written.contains(expected), "{source}: {written}");
    }
    // A name in an aligned cell is one.
    let source = r"\begin{aligned}a&\operatorname*{\underline{lim}}x\end{aligned}";
    let written = convert(source, Format::LaTeX, Format::MathML).unwrap();
    assert!(written.contains(">lim<"), "{written}");
    // An inner font wins over the name's, on both routes.
    let source = r"\operatorname{\underline{\mathbf{ab}}}";
    let written = convert(source, Format::LaTeX, Format::MathML).unwrap();
    assert!(
        written.contains(r#"<mi mathvariant="bold">ab</mi>"#),
        "{written}"
    );
    let source = r"\mathbf{\operatorname{\underline{a}bc}}";
    let written = convert(source, Format::LaTeX, Format::MathML).unwrap();
    assert!(
        written.contains(r#"<mi mathvariant="normal">bc</mi>"#),
        "{written}"
    );
    // So is a name that is all of a cell.
    let source = r"\begin{aligned}&\operatorname*{\underline{lim}}\end{aligned}";
    let written = convert(source, Format::LaTeX, Format::MathML).unwrap();
    assert!(written.contains(">lim<"), "{written}");
    let name = MathNode::StructuredOperator {
        content: Box::new(MathNode::Row(
            ["l", "i", "m"]
                .map(|l| MathNode::Identifier(l.into()))
                .to_vec(),
        )),
        form: mathema::ast::OperatorNameForm::Unstarred,
    };
    let cell = MathNode::AlignedCell {
        alignment: mathema::ast::ColumnAlign::Left,
        content: Box::new(name),
    };
    let table = MathNode::Array {
        columns: vec![mathema::ast::ColumnAlign::Center],
        rows: vec![vec![vec![cell]]],
        arraystretch: None,
        add_jot: false,
    };
    let mut expr =
        mathema::ast::compact::CompactExpr::from_math_expr(&mathema::MathExpr::block(vec![table]))
            .unwrap();
    mathema::ast::normalize::normalize_compact_expr(&mut expr).unwrap();
    let nodes = format!("{:?}", expr.to_math_expr());
    assert!(nodes.contains(r#"Identifier("lim")"#), "{nodes}");
    // But letters that are no name are apart, in a cell of one node too.
    let atom = MathNode::AlignedCell {
        alignment: mathema::ast::ColumnAlign::Left,
        content: Box::new(MathNode::MathAtom {
            atom_type: mathema::ast::MathAtomType::Ord,
            content: Box::new(MathNode::Row(
                ["a", "b"].map(|l| MathNode::Identifier(l.into())).to_vec(),
            )),
        }),
    };
    let mut expr =
        mathema::ast::compact::CompactExpr::from_math_expr(&mathema::MathExpr::block(vec![atom]))
            .unwrap();
    mathema::ast::normalize::normalize_compact_expr(&mut expr).unwrap();
    let nodes = format!("{:?}", expr.to_math_expr());
    assert!(!nodes.contains(r#"Identifier("ab")"#), "{nodes}");
    // On the heap route too.
    for source in [r"\mathord{ab}", r"\foo{ab}"] {
        let mut heap = mathema::from_latex(source).unwrap().expr();
        mathema::ast::normalize::normalize_expr(&mut heap);
        assert!(
            !format!("{heap:?}").contains(r#"Identifier("ab")"#),
            "{heap:?}"
        );
    }
}

/// An operator without a command has no group for what it applies to.
#[test]
fn an_unnamed_operator_settles() {
    for source in [r"\bbsum {yu}y", r"\bbsum {y+u}y", r"\bbsum {2u}y"] {
        let latex = convert(source, Format::LaTeX, Format::LaTeX).unwrap();
        assert_eq!(
            convert(&latex, Format::LaTeX, Format::LaTeX).unwrap(),
            latex,
            "{source}"
        );
    }
    // A row that a symbol starts keeps its group.
    let latex = convert(r"\bbsum {+u}y", Format::LaTeX, Format::LaTeX).unwrap();
    assert!(latex.ends_with("{+ u}y"), "{latex}");
}

/// The argument of a function is one node, whatever it holds, so what follows
/// it stays outside and nothing of it goes there.
#[test]
fn arguments_of_functions() {
    use mathema::writer::{
        Writer, mathml::MathmlWriter, omml::OmmlWriter, unicode_math::UnicodeMathWriter,
    };
    let config = mathema::Config::default();
    let normalized = |read: &mathema::ConvertResult| {
        let mut tree = read.expr();
        mathema::ast::normalize::normalize_expr(&mut tree);
        tree
    };
    let sin = "<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t></m:r>";
    let func = |name: &str, argument: &str| {
        format!("<m:func><m:fName>{name}</m:fName><m:e>{argument}</m:e></m:func>")
    };
    let squared = format!(
        "<m:sSup><m:e>{sin}</m:e><m:sup>{}</m:sup></m:sSup>",
        run("2", "")
    );
    let limit = format!(
        "<m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow>",
        run("lim", "p"),
        run("n", "")
    );
    for argument in ["x", "xy", "2x", "a+b", "-x", "xyz"] {
        for name in [sin, &squared, &limit, &run("f", "")] {
            let one = func(name, &run(argument, ""));
            for source in [
                omml(&(one.clone() + &run("z", ""))),
                omml(&(run("z", "") + &one + &one)),
                omml(&func(sin, &(one.clone() + &run("z", "")))),
                omml(&format!(
                    "<m:f><m:num>{one}{}</m:num><m:den>{one}</m:den></m:f>",
                    run("z", "")
                )),
                omml(&format!("<m:d><m:e>{one}{}</m:e></m:d>", run("z", ""))),
                omml(&format!(
                    "<m:sSup><m:e>{}</m:e><m:sup>{one}{}</m:sup></m:sSup>",
                    run("e", ""),
                    run("z", "")
                )),
            ] {
                let written = omml_again(&source);
                assert_eq!(outline(&written), outline(&source));
                assert_eq!(omml_again(&written), written);
                // By way of MathML it is the same.
                let mathml = convert(&source, Format::Omml, Format::MathML).unwrap();
                let back = convert(&mathml, Format::MathML, Format::Omml).unwrap();
                assert_eq!(functions(&back), functions(&source), "{mathml}");
                // And normalized as a tree.
                let tree = normalized(&from_omml(&source).unwrap());
                assert_eq!(
                    outline(&OmmlWriter::write(&tree, &config).unwrap()),
                    outline(&source)
                );
            }
        }
    }
    // A size is on the argument, and stays inside.
    for size in ["", " mathsize=\"20pt\""] {
        let source = format!(
            "<math><mi>f</mi><mo>&#x2061;</mo><mrow{size}><mi>x</mi><mi>y</mi></mrow><mi>z</mi></math>"
        );
        assert_eq!(
            mathml(&source, Format::MathML),
            format!(
                "<mi>f</mi><mo>\u{2061}</mo><mrow><mi{size}>x</mi><mi{size}>y</mi></mrow><mi>z</mi>"
            )
        );
        let omml = convert(&source, Format::MathML, Format::Omml).unwrap();
        assert!(omml.contains("y</m:t></m:r></m:e></m:func>"), "{omml}");
    }
    // An empty argument does not take what follows it.
    let empty = "<math><mi>f</mi><mo>&#x2061;</mo><mrow/><mi>z</mi></math>";
    assert_eq!(
        functions(&convert(empty, Format::MathML, Format::Omml).unwrap()),
        ["<m:func><m:fName>f</m:fName><m:e></m:e></m:func>"]
    );
    // In a fence too, as a tree and as an arena.
    let fenced = "<math><mrow><mo>(</mo><mi>f</mi><mo>&#x2061;</mo><mrow><mi>x</mi><mi>y</mi></mrow><mi>z</mi><mo>)</mo></mrow></math>";
    let tree = normalized(&from_mathml(fenced).unwrap());
    for written in [
        MathmlWriter::write(&tree, &config).unwrap(),
        convert(fenced, Format::MathML, Format::MathML).unwrap(),
    ] {
        assert!(
            written.contains("<mrow><mi>x</mi><mi>y</mi></mrow><mi>z</mi>"),
            "{written}"
        );
    }
    // UnicodeMath groups it where it is several nodes; LaTeX has none, and a
    // group would be an atom of its own.
    for (argument, unicode, latex) in [
        ("x", "sin\u{2061}xz", r"\sin \mathord{}xz"),
        ("xy", "sin\u{2061}〖xy〗z", r"\sin \mathord{}xyz"),
        ("a+b", "sin\u{2061}〖a+b〗z", r"\sin \mathord{}a+ bz"),
    ] {
        let source = omml(&(func(sin, &run(argument, "")) + &run("z", "")));
        let to = |format| convert(&source, Format::Omml, format).unwrap();
        assert_eq!(to(Format::UnicodeMath), unicode);
        assert_eq!(to(Format::LaTeX), latex);
        assert_eq!(convert(latex, Format::LaTeX, Format::LaTeX).unwrap(), latex);
        let tree = normalized(&from_omml(&source).unwrap());
        assert_eq!(UnicodeMathWriter::write(&tree, &config).unwrap(), unicode);
    }
    assert_eq!(
        mathml(r"\sin {xy}z", Format::LaTeX),
        "<mo>sin</mo><mi>x</mi><mi>y</mi><mi>z</mi>"
    );
}

/// What [`arguments_of_functions`] covers no source for.
#[test]
fn arguments_of_functions_in_trees() {
    use mathema::ast::normalize::{normalize_compact_expr, normalize_expr};
    use mathema::ast::{FencedItem, SymbolKind};
    use mathema::writer::{Writer, unicode_math::UnicodeMathWriter};
    let config = mathema::Config::default();
    let letter = |c: &str| MathNode::Identifier(c.into());
    let apply = || MathNode::Symbol(SymbolKind::Ordinary, "\u{2061}".into());
    let nodes = || {
        vec![
            letter("f"),
            apply(),
            MathNode::Row(vec![letter("x"), letter("y")]),
            letter("z"),
        ]
    };
    let mut tree = mathema::from_latex("x").unwrap().expr();
    let fence = |nodes: Vec<MathNode>| MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: nodes.into_iter().map(FencedItem::Element).collect(),
    };
    tree.nodes = vec![fence(nodes())];
    normalize_expr(&mut tree);
    assert_eq!(tree.nodes, vec![fence(nodes())]);
    // Nor is an application a node with the scripts before it, and an empty
    // argument stays one.
    for source in [
        "<msup><mrow/><mn>2</mn></msup><mo>&#x2061;</mo><mrow><mi>a</mi><mi>b</mi></mrow><mi>z</mi>",
        "<mi>f</mi><mo>&#x2061;</mo><mrow/><mi>z</mi>",
        "<mi>f</mi><mo>&#x2061;</mo><mrow><mspace width=\"0\"/></mrow><mi>z</mi>",
    ] {
        let source = format!("<math>{source}</math>");
        let read = from_mathml(&source).unwrap();
        let (mut heap, mut compact) = (read.expr(), read.compact_expr().clone());
        for _ in 0..2 {
            normalize_expr(&mut heap);
            normalize_compact_expr(&mut compact).unwrap();
            assert!(
                matches!(&heap.nodes[..], [_, _, MathNode::Row(_), MathNode::Identifier(z)] if z == "z"),
                "{heap:?}"
            );
            assert_eq!(heap.nodes, compact.to_math_expr().nodes);
        }
    }
    // What LaTeX spells the application with is an atom like another.
    for source in ["\\sin \u{2061}{xy}z", r"\sin \mathord{}{xy}z"] {
        let written = convert(source, Format::LaTeX, Format::LaTeX).unwrap();
        assert_eq!(written, r"\sin \mathord{}xyz");
    }
    assert_eq!(
        mathml(r"a\mathord{}{xy}z", Format::LaTeX),
        mathml(r"a\mathord{}xyz", Format::LaTeX)
    );
    // A function in an argument, and an argument written as nothing.
    let nested = "<math><mo>sin</mo><mo>&#x2061;</mo><mrow><mo>sin</mo><mo>&#x2061;</mo><mi>x</mi></mrow><mi>z</mi></math>";
    assert_eq!(
        convert(nested, Format::MathML, Format::UnicodeMath).unwrap(),
        "sin\u{2061}〖sin\u{2061}x〗z"
    );
    tree.nodes = vec![
        letter("f"),
        apply(),
        MathNode::Row(vec![MathNode::Row(vec![]), MathNode::Row(vec![])]),
    ];
    assert_eq!(
        UnicodeMathWriter::write(&tree, &config).unwrap(),
        "f\u{2061}"
    );
    // Nor an argument of one node, before normalization.
    for (argument, expected) in [(vec![letter("x")], "f\u{2061}x"), (vec![], "f\u{2061}")] {
        let nodes = vec![letter("f"), apply(), MathNode::Row(argument)];
        let expr = mathema::MathExpr::block(nodes);
        let compact = mathema::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
        assert_eq!(UnicodeMathWriter::write(&expr, &config).unwrap(), expected);
        assert_eq!(
            <UnicodeMathWriter as Writer>::write_compact(&compact, &config).unwrap(),
            expected
        );
    }
    // What an empty group follows is written.
    let expr = mathema::MathExpr::block(vec![letter("x"), MathNode::Row(vec![])]);
    let compact = mathema::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
    let written =
        <mathema::writer::omml::OmmlWriter as Writer>::write_compact(&compact, &config).unwrap();
    assert!(written.contains("<m:t>x</m:t>"), "{written}");
    // On the way to LaTeX scripts lead an application, as they are read back.
    for rest in ["<mi>z</mi>", "<mrow><mi>a</mi><mi>b</mi></mrow><mi>z</mi>"] {
        let source = format!("<math><msup><mrow/><mn>2</mn></msup><mo>&#x2061;</mo>{rest}</math>");
        let latex = convert(&source, Format::MathML, Format::LaTeX).unwrap();
        assert_eq!(
            convert(&latex, Format::LaTeX, Format::LaTeX).unwrap(),
            latex
        );
    }
    // An application in a chain with an empty argument settles as OMML.
    let chain = "<math><mi>f</mi><mo>&#x2061;</mo><mi>g</mi><mo>&#x2061;</mo><mrow/><mi>h</mi><mo>&#x2061;</mo><mi>k</mi><mo>&#x2061;</mo><mn>2</mn></math>";
    let written = convert(chain, Format::MathML, Format::Omml).unwrap();
    assert_eq!(omml_again(&written), written);
    // MTEF has no such group.
    let source =
        "<math><mi>f</mi><mo>&#x2061;</mo><mrow><mi>x</mi><mi>y</mi></mrow><mi>z</mi></math>";
    let open = source.replace("<mrow>", "").replace("</mrow>", "");
    let mtef = |source: &str| from_mathml(source).unwrap().to_mtef().unwrap();
    assert_eq!(mtef(source), mtef(&open));
}

/// What a name has above, below or beside it is no part of it, also where the
/// argument is empty and the name is an operator's.
#[test]
fn what_is_beside_a_name() {
    let source = |name: &str| {
        omml(&format!(
            "<m:func><m:fName>{name}</m:fName><m:e></m:e></m:func>"
        ))
    };
    let plain = |text: &str| run(text, "");
    let lim = run("lim", "p");
    let letters = run("n", "") + &run("k", "");
    for (tag, base, side) in [
        ("m:limLow", "m:e", "m:lim"),
        ("m:limUpp", "m:e", "m:lim"),
        ("m:sSub", "m:e", "m:sub"),
        ("m:sSup", "m:e", "m:sup"),
        ("m:f", "m:num", "m:den"),
    ] {
        let of = |side_text: &str, base_text: &str| {
            source(&format!(
                "<{tag}><{base}>{base_text}</{base}><{side}>{side_text}</{side}></{tag}>"
            ))
        };
        let written = omml_again(&of(&plain("nk"), &lim));
        assert_eq!(written, of(&letters, &lim), "{tag}");
        let word = run("foo", "p");
        assert_eq!(
            omml_again(&of(&plain("nk"), &word)),
            of(&letters, &word),
            "{tag}"
        );
        let mut heap = from_omml(&of(&plain("nk"), &lim)).unwrap().expr();
        mathema::ast::normalize::normalize_expr(&mut heap);
        assert!(
            !format!("{heap:?}").contains(r#"Identifier("nk")"#),
            "{heap:?}"
        );
    }
    let cells = |text: &str| source(&format!("<m:m><m:mr><m:e>{text}</m:e></m:mr></m:m>"));
    assert!(omml_again(&cells(&plain("nk"))).contains(&letters));
}

/// A name in several runs is one, as is one written of letters.
#[test]
fn names_in_several_runs() {
    let func = |name: &str| {
        omml(&format!(
            "<m:func><m:fName>{name}</m:fName><m:e>{}</m:e></m:func>",
            run("x", "")
        ))
    };
    let sup = |base: &str| {
        format!(
            "<m:sSup><m:e>{base}</m:e><m:sup>{}</m:sup></m:sSup>",
            run("2", "")
        )
    };
    for (runs, name) in [
        (&["er", "f"][..], "erf"),
        (&["f", "oo"], "foo"),
        (&["e", "r", "f"], "erf"),
        (&["si", "n"], "sin"),
        (&["α", "β"], "αβ"),
    ] {
        for style in ["", "p"] {
            let split: String = runs.iter().map(|r| run(r, style)).collect();
            assert_eq!(omml_again(&func(&split)), func(&run(name, "p")), "{runs:?}");
            assert_eq!(
                omml_again(&func(&sup(&split))),
                func(&sup(&run(name, "p"))),
                "{runs:?}"
            );
        }
    }
    // Runs in other styles or sizes are apart.
    let apart = run("a", "p") + &run("b", "b");
    assert!(omml_again(&func(&sup(&apart))).contains(&apart));
    let sized = |text: &str, size: &str| {
        format!(r#"<m:r><w:rPr><w:sz w:val="{size}"/></w:rPr><m:t>{text}</m:t></m:r>"#)
    };
    let read = |second: &str| {
        let body = func(&sup(&(sized("a", "20") + &sized("b", second)))).replace(
            "<m:oMath ",
            r#"<m:oMath xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        );
        format!("{:?}", from_omml(&body).unwrap().expr().nodes)
    };
    assert!(read("20").contains(r#"Identifier("ab")"#), "{}", read("20"));
    assert!(!read("40").contains(r#"Identifier("ab")"#));
    let colored = |color: &str| {
        let second = format!(r#"<m:r><w:rPr><w:color w:val="{color}"/></w:rPr><m:t>b</m:t></m:r>"#);
        func(&sup(&(run("a", "") + &second))).replace(
            "<m:oMath ",
            r#"<m:oMath xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        )
    };
    let word = mathema::OmmlOptions::default()
        .with_run_properties(mathema::OmmlRunProperties::WordprocessingMl);
    for body in [
        colored("FF0000"),
        func(&sup(&(sized("a", "20") + &sized("b", "40")))).replace(
            "<m:oMath ",
            r#"<m:oMath xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        ),
    ] {
        let read = from_omml(&body).unwrap();
        let (written, _) = read.to_omml_with_options(&word).unwrap();
        assert!(
            written.contains(">a</m:t>") && written.contains(">b</m:t>"),
            "{written}"
        );
    }
    // As they are from other formats, words with letters too, but not the
    // argument of a function in the name.
    for (name, expected) in [
        ("<mi>a</mi><mi>b</mi>", "ab"),
        ("<mi>foo</mi><mi>x</mi>", "foox"),
    ] {
        let source = format!(
            "<math><msup><mrow>{name}</mrow><mn>2</mn></msup><mo>&#x2061;</mo><mi>x</mi></math>"
        );
        let written = convert(&source, Format::MathML, Format::Omml).unwrap();
        assert_eq!(written, func(&sup(&run(expected, "p"))));
    }
    let inner = format!(
        "<m:func><m:fName>{}</m:fName><m:e>{}</m:e></m:func>",
        run("sin", "p"),
        run("nk", "")
    );
    let written = from_omml(&func(&sup(&inner))).unwrap().to_omml().unwrap();
    assert!(
        written.contains(&(run("n", "") + &run("k", ""))),
        "{written}"
    );
    // Letters in the style of words that are a name are written in one run.
    let written = convert("\\mathrm{ab}\u{2061}x", Format::LaTeX, Format::Omml).unwrap();
    assert_eq!(written, func(&run("ab", "p")));
    assert_eq!(omml_again(&written), written);
}

/// Applications that have no function (first, or after an argument) hold no
/// group, and neither does what follows an empty argument.
#[test]
fn applications_are_paired() {
    let a = "<mo>&#x2061;</mo>";
    let ab = "<mrow><mi>a</mi><mi>b</mi></mrow>";
    for (source, kept) in [
        (format!("<mi>f</mi>{a}{ab}"), true),
        (format!("{a}{ab}"), false),
        (format!("<mi>f</mi>{a}<mi>g</mi>{a}{ab}"), false),
        (format!("<mi>f</mi>{a}<mrow/><mi>g</mi>{a}{ab}"), true),
        (
            format!("<mfenced><mi>f</mi>{a}<mrow/><mi>g</mi>{a}{ab}</mfenced>"),
            true,
        ),
        (
            format!("<mfenced><mrow><msup><mrow/><mn>2</mn></msup>{a}{ab}</mrow></mfenced>"),
            true,
        ),
        // A number that another joins is still one argument.
        (format!("<mi>f</mi>{a}<mn>2</mn><mn>3</mn>{a}{ab}"), false),
    ] {
        let source = format!("<math>{source}<mi>z</mi></math>");
        let read = from_mathml(&source).unwrap();
        let (mut heap, mut compact) = (read.expr(), read.compact_expr().clone());
        for _ in 0..2 {
            mathema::ast::normalize::normalize_expr(&mut heap);
            mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
            assert_eq!(heap.nodes, compact.to_math_expr().nodes, "{source}");
            let nodes = format!("{:?}", heap.nodes);
            assert_eq!(
                nodes.contains(r#"Row([Identifier("a"), Identifier("b")])"#),
                kept,
                "{source} {nodes}"
            );
        }
        let written = convert(&source, Format::MathML, Format::Omml).unwrap();
        if !source.contains("mfenced") {
            assert_eq!(omml_again(&written), written, "{source}");
        }
    }
    // A style switch is no name, and a middle delimiter neither that nor an
    // argument.
    for (source, kept) in [
        ("\\displaystyle\u{2061}{ab}z", false),
        ("\\left(\\middle|\u{2061}{ab}z\\right)", false),
        ("\\left(f\u{2061}\\middle|\u{2061}{ab}z\\right)", false),
        // Opening a group joins two numbers, and what followed the second is
        // no argument any more; it is opened too, and the argument of `h` stays.
        ("\u{2061}{g\u{2061}2}3\u{2061}{ab}h\u{2061}{ab}z", true),
        (
            "\u{2061}{g\u{2061}2}3\u{2061}{ab}h\u{2061}{f\u{2061}{ab}z}z",
            true,
        ),
        (r"{}^2\mathrm{}x", false),
        (r"{}^2\mathrm{}\mathrm{}", false),
        // An empty group or a zero space is no name.
        ("\\operatorname{{}\u{2061}\\mathord{ab}}z", false),
        ("\\operatorname{\\kern0pt\u{2061}\\mathord{ab}}z", false),
    ] {
        let read = mathema::from_latex(source).unwrap();
        let (mut heap, mut compact) = (read.expr(), read.compact_expr().clone());
        mathema::ast::normalize::normalize_expr(&mut heap);
        mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        let once = heap.nodes.clone();
        assert_eq!(once, compact.to_math_expr().nodes, "{source}");
        assert_eq!(
            format!("{once:?}").contains(r#"Row([Identifier("a"), Identifier("b")])"#),
            kept,
            "{source} {once:?}"
        );
        mathema::ast::normalize::normalize_expr(&mut heap);
        mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        assert_eq!(once, heap.nodes, "{source}");
        assert_eq!(once, compact.to_math_expr().nodes, "{source}");
    }
    // Scripts with an empty base lead what follows a font of nothing at once.
    let latex = convert(r"{}^2\mathrm{}x", Format::LaTeX, Format::MathML).unwrap();
    assert!(latex.contains("mprescripts"), "{latex}");
    // Both UnicodeMath writers space a name of words alike.
    let source = omml(&format!(
        "{}<m:func><m:fName>{}</m:fName><m:e></m:e></m:func>",
        run("x", ""),
        run("foo", "")
    ));
    let read = from_omml(&source).unwrap();
    use mathema::writer::{Writer, unicode_math::UnicodeMathWriter};
    let config = mathema::Config::default();
    assert_eq!(
        UnicodeMathWriter::write(&read.expr(), &config).unwrap(),
        <UnicodeMathWriter as Writer>::write_compact(read.compact_expr(), &config).unwrap()
    );
}
