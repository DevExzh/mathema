//! Every XML output is well-formed XML 1.0, whatever characters the tree holds.

use mathema::ast::{MathExpr, MathNode, SymbolKind, TextStyle};
use mathema::error::Config;
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::omml::OmmlWriter;

/// Trees holding the characters from `start`, a few of each kind of node.
fn trees(chars: &[char]) -> Vec<MathExpr> {
    let text: String = chars.iter().collect();
    let first = chars[0].to_string();
    vec![
        MathExpr::inline(
            chars
                .iter()
                .map(|c| MathNode::Identifier(c.to_string()))
                .collect(),
        ),
        MathExpr::inline(
            chars
                .iter()
                .map(|c| MathNode::Symbol(SymbolKind::Ordinary, c.to_string()))
                .collect(),
        ),
        MathExpr::inline(vec![
            MathNode::Text(TextStyle::Normal, text.clone()),
            MathNode::Number(text.clone()),
            MathNode::Symbol(SymbolKind::Relation, first.clone()),
            MathNode::Superscript {
                base: Box::new(MathNode::Identifier(first.clone())),
                sup: Box::new(MathNode::Text(TextStyle::Monospace, text.clone())),
            },
            MathNode::Fenced {
                open: first.clone(),
                close: first.clone(),
                content: vec![mathema::ast::FencedItem::Element(MathNode::Identifier(
                    first.clone(),
                ))],
            },
        ]),
    ]
}

fn parse(kind: &str, xml: &str, chars: &[char]) {
    if let Err(error) = roxmltree::Document::parse(xml) {
        let codes: Vec<String> = chars
            .iter()
            .take(4)
            .map(|c| format!("{:X}", *c as u32))
            .collect();
        panic!("{kind} is not well-formed ({error}) from {codes:?}…:\n{xml:.300}");
    }
}

#[test]
fn boundary_characters_are_well_formed() {
    let config = Config::default();
    let chars: Vec<char> = [
        0x00, 0x01, 0x08, 0x09, 0x0a, 0x0d, 0x1f, 0x20, 0x7f, 0x84, 0x85, 0x9f, 0xd7ff, 0xe000,
        0xfffd, 0xfffe, 0xffff, 0x10000, 0x10ffff,
    ]
    .into_iter()
    .filter_map(char::from_u32)
    .collect();
    for expr in trees(&chars) {
        parse(
            "MathML",
            &MathmlWriter::write(&expr, &config).unwrap(),
            &chars,
        );
        let compact = mathema::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
        parse(
            "MathML",
            &MathmlWriter::write_compact(&compact, &config).unwrap(),
            &chars,
        );
        // An OMML fragment: one root around it.
        if let Ok(omml) = OmmlWriter::write(&expr, &config) {
            parse("OMML", &omml, &chars);
        }
    }
}

#[cfg(feature = "render")]
#[test]
fn every_svg_is_well_formed() {
    use mathema::render::RenderOptions;
    let options = RenderOptions::default();
    let chars: Vec<char> = [
        0x00, 0x01, 0x08, 0x09, 0x0a, 0x0d, 0x1f, 0x20, 0x7f, 0x85, 0xd7ff, 0xe000, 0xfffd, 0xfffe,
        0xffff, 0x10000, 0x10ffff,
    ]
    .into_iter()
    .filter_map(char::from_u32)
    .collect();
    for expr in trees(&chars) {
        parse(
            "SVG",
            &mathema::render::render_to_svg(&expr, &options).unwrap(),
            &chars,
        );
        parse(
            "standalone SVG",
            &mathema::render::render_to_svg_standalone(&expr, &options).unwrap(),
            &chars,
        );
        let compact = mathema::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
        parse(
            "SVG",
            &mathema::render::render_to_svg_compact(&compact, &options).unwrap(),
            &chars,
        );
        parse(
            "standalone SVG",
            &mathema::render::render_to_svg_standalone_compact(&compact, &options).unwrap(),
            &chars,
        );
    }
}

/// `\char'23` and other inputs that hold such characters, through the API.
#[test]
fn control_characters_from_latex_are_replaced() {
    for input in [
        r"\char'23",
        r#"\char"1 x"#,
        r#"\text{a\char"8 b}"#,
        r#"\char"FFFF"#,
    ] {
        let read = mathema::from_latex(input).unwrap();
        let (mathml, diagnostics) = read.to_mathml_with_diagnostics().unwrap();
        roxmltree::Document::parse(&mathml).unwrap_or_else(|e| panic!("{input}: {e}\n{mathml}"));
        assert!(mathml.contains('\u{FFFD}'), "{input}: {mathml}");
        assert!(
            diagnostics.iter().any(|d| d.message.contains("U+FFFD")),
            "{input}: {diagnostics:?}"
        );
        let (omml, diagnostics) = read.to_omml_with_diagnostics().unwrap();
        roxmltree::Document::parse(&format!(
            "<r xmlns:m=\"m\" xmlns:w=\"w\" xmlns:a=\"a\">{omml}</r>"
        ))
        .unwrap_or_else(|e| panic!("{input}: {e}\n{omml}"));
        assert!(!diagnostics.is_empty(), "{input}");
        #[cfg(feature = "render")]
        for svg in [read.to_svg().unwrap(), read.to_svg_text().unwrap()] {
            roxmltree::Document::parse(&svg).unwrap_or_else(|e| panic!("{input}: {e}"));
        }
    }
}
