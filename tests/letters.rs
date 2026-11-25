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
    let italic = r#"<math><mi mathvariant="italic">foo</mi></math>"#;
    let written = convert(italic, Format::MathML, Format::Omml).unwrap();
    assert!(written.contains(&run("foo", "i")), "{written}");
    let letters: String = ["f", "o", "o"].map(|letter| run(letter, "")).concat();
    assert_eq!(omml_again(&written), omml(&letters));
}

/// OMML with names, words and letters settles after one pass.
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
                    let (name, argument) = (self.run(), self.run());
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
    for seed in 0..3000u64 {
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let source = omml(&random.list(2));
        let Ok(first) = convert(&source, Format::Omml, Format::Omml) else {
            continue;
        };
        if omml_again(&first) != first {
            unsettled.push(source);
        }
    }
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
    let source = r"\operatorname{\underline{a}bc}";
    let mtef = convert(source, Format::LaTeX, Format::Mtef).unwrap();
    let apart = convert(
        r"\mathord{\underline{\mathrm{a}}}\mathrm{b}\mathrm{c}",
        Format::LaTeX,
        Format::Mtef,
    );
    assert!(apart.is_ok() && !mtef.is_empty());
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
}
