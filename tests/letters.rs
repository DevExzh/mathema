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
