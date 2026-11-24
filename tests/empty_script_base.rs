//! A script after an explicit empty group keeps that group as its base, as in
//! TeX, KaTeX and MathJax: `x{}_1` is x followed by an empty ordinary atom
//! carrying the subscript (set apart from x, with no italic correction or
//! kerning against it), not `x_1`. Leading scripts (`{}_1 x`) still belong to
//! the node after them.

use mathema::{Format, MathNode, convert, from_latex};

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

fn mathml_body(input: &str) -> String {
    let mathml = convert(input, Format::LaTeX, Format::MathML).unwrap();
    let start = mathml.find('>').unwrap() + 1;
    let end = mathml.rfind("</math>").unwrap();
    mathml[start..end].to_owned()
}

/// Input, its LaTeX spelling, and its MathML.
const CASES: &[(&str, &str, &str)] = &[
    (
        r"x{}_1",
        r"x{}_{1}",
        "<mi>x</mi><msub><mrow></mrow><mn>1</mn></msub>",
    ),
    (
        r"x{}^2",
        r"x{}^{2}",
        "<mi>x</mi><msup><mrow></mrow><mn>2</mn></msup>",
    ),
    (
        r"x{}'",
        r"x{}^{\prime}",
        "<mi>x</mi><msup><mrow></mrow><mo>′</mo></msup>",
    ),
    (
        r"x {}_1^2",
        r"x{}_{1}^{2}",
        "<mi>x</mi><msubsup><mrow></mrow><mn>1</mn><mn>2</mn></msubsup>",
    ),
    (
        r"a+{}_1",
        r"a+{}_{1}",
        "<mi>a</mi><mo>+</mo><msub><mrow></mrow><mn>1</mn></msub>",
    ),
    (
        r"\left(x{}_1\right)",
        r"\left( x{}_{1} \right)",
        "<mrow><mo fence=\"true\">(</mo><mi>x</mi><msub><mrow></mrow><mn>1</mn></msub><mo fence=\"true\">)</mo></mrow>",
    ),
    (
        r"x{}_1\quad",
        r"x{}_{1}\quad ",
        "<mi>x</mi><msub><mrow></mrow><mn>1</mn></msub><mspace width=\"1em\"/>",
    ),
    (
        r"x\sideset{_1 {}_2}{}\sum",
        r"x{}_{1} {{}_{2}\sum}",
        "<mi>x</mi><msub><mrow></mrow><mn>1</mn></msub><mmultiscripts><mo largeop=\"true\" form=\"prefix\">∑</mo><mprescripts/><mn>2</mn><none/></mmultiscripts>",
    ),
];

#[test]
fn latex_keeps_the_empty_base() {
    for &(input, expected, _) in CASES {
        assert_eq!(latex(input), expected, "{input}");
        // Written unnormalized, as parsed, it means the same.
        let unnormalized = from_latex(input).unwrap().to_latex().unwrap();
        assert_eq!(latex(&unnormalized), expected, "{input} as {unnormalized}");
    }
}

#[test]
fn mathml_keeps_the_empty_base() {
    for &(input, _, expected) in CASES {
        assert_eq!(mathml_body(input), expected, "{input}");
    }
}

#[test]
fn latex_round_trip_is_stable() {
    for &(input, expected, _) in CASES {
        assert_eq!(latex(expected), expected, "{input}");
    }
}

/// No script ends up on x through any format and back.
#[test]
fn every_format_keeps_the_empty_base() {
    fn x_is_a_script_base(nodes: &[MathNode]) -> bool {
        nodes.iter().any(|node| match node {
            MathNode::Subscript { base, .. }
            | MathNode::Superscript { base, .. }
            | MathNode::SubSup { base, .. } => {
                matches!(**base, MathNode::Identifier(ref name) if name == "x")
            },
            MathNode::Row(children) => x_is_a_script_base(children),
            _ => false,
        })
    }
    for input in [r"x{}_1", r"x{}^2", r"x{}'", r"x {}_1^2"] {
        for format in [Format::MathML, Format::Omml, Format::Mtef] {
            let written = convert(input, Format::LaTeX, format).unwrap();
            let back = convert(&written, format, Format::LaTeX).unwrap();
            assert_eq!(back, latex(input), "{input} via {format:?}");
            let nodes = from_latex(&back).unwrap().expr().nodes;
            assert!(!x_is_a_script_base(&nodes), "{input} via {format:?}");
        }
    }
}

/// The empty group between a node and its script is what keeps them apart:
/// without it, the script attaches to the node (also after white space).
#[test]
fn a_script_without_a_group_still_attaches() {
    assert_eq!(latex(r"x_1"), r"x_{1}");
    assert_eq!(latex(r"x '"), r"x^{\prime}");
    assert_eq!(latex(r"x{}_1 y"), r"x {{}_{1}y}");
}

#[test]
fn leading_scripts_still_lead_the_next_node() {
    assert_eq!(latex(r"{}_1 x"), r"{{}_{1}x}");
    assert!(mathml_body(r"{}_1 x").starts_with("<mmultiscripts><mi>x</mi><mprescripts/>"));
    assert_eq!(latex(r"x={}^{14}_{6}C"), r"x= {{}_{6}^{14}C}");
}

#[cfg(feature = "render")]
#[test]
fn layout_keeps_the_empty_base() {
    let svg = |input: &str| convert(input, Format::LaTeX, Format::Svg).unwrap();
    assert_ne!(svg(r"x{}_1"), svg(r"x_1"));
    assert_eq!(svg(r"x{}_1"), svg(r"x{}_{1}"));
}
