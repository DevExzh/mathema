use mathema::writer::Writer;
use mathema::{CompactExpr, Config, Format, MathExpr, MathNode, from_latex};
use std::str::FromStr;

#[test]
fn format_names_are_shared_by_rust_and_cli_aliases() {
    assert_eq!(Format::from_str("tex").unwrap(), Format::LaTeX);
    assert_eq!(Format::from_str("MML").unwrap(), Format::MathML);
    assert_eq!(
        Format::from_str("unicode-math").unwrap(),
        Format::UnicodeMath
    );
    #[cfg(feature = "raster")]
    assert_eq!(Format::from_str("JPG").unwrap(), Format::Jpeg);
    assert_eq!(Format::LaTeX.to_string(), "latex");
    assert_eq!(Format::MathML.to_string(), "mathml");
    assert_eq!(Format::UnicodeMath.to_string(), "unicodemath");
}

#[test]
fn checked_heap_and_compact_write_entrypoints_validate_and_report() {
    let config = Config::default();
    let heap = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
    let compact = CompactExpr::from_math_expr(&heap).unwrap();

    assert_eq!(
        mathema::write_expr(&heap, Format::MathML, &config).unwrap(),
        mathema::write_compact_expr(&compact, Format::MathML, &config).unwrap()
    );
    let (_, diagnostics) =
        mathema::write_compact_expr_with_diagnostics(&compact, Format::UnicodeMath, &config)
            .unwrap();
    assert!(diagnostics.is_empty());
}

#[test]
fn convert_result_debug_does_not_materialize_the_heap_tree() {
    let result = from_latex(r"\frac{1}{2}").unwrap();
    let debug = format!("{result:?}");
    assert!(debug.contains("ConvertResult"));
    assert!(debug.contains("node_count"));
    assert!(!debug.contains("MathNode"));
}

#[test]
fn concrete_writer_trait_remains_usable_for_checked_heap_values() {
    let config = Config::default();
    let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
    let output = <mathema::writer::mathml::MathmlWriter as Writer>::write(&expr, &config).unwrap();
    assert!(output.contains("<mi>x</mi>"));
}

#[cfg(feature = "render")]
#[test]
fn render_writer_entrypoints_honor_configured_depth() {
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..8 {
        node = MathNode::Superscript {
            base: Box::new(node),
            sup: Box::new(MathNode::Identifier("1".into())),
        };
    }
    let heap = MathExpr::inline(vec![node]);
    let compact = CompactExpr::from_math_expr(&heap).unwrap();
    let config = Config::default().with_max_depth(3);
    assert!(mathema::write_expr(&heap, Format::Svg, &config).is_err());
    assert!(mathema::write_compact_expr(&compact, Format::Svg, &config).is_err());
    assert!(mathema::write_compact_expr_with_diagnostics(&compact, Format::Svg, &config).is_err());
}

#[cfg(feature = "render")]
#[test]
fn checked_svg_writers_use_outlines_by_default() {
    let config = Config::default();
    let heap = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
    let compact = CompactExpr::from_math_expr(&heap).unwrap();
    for svg in [
        mathema::write_expr(&heap, Format::Svg, &config).unwrap(),
        mathema::write_compact_expr(&compact, Format::Svg, &config).unwrap(),
        mathema::write_expr_with_diagnostics(&heap, Format::Svg, &config)
            .unwrap()
            .0,
        mathema::write_compact_expr_with_diagnostics(&compact, Format::Svg, &config)
            .unwrap()
            .0,
    ] {
        assert!(svg.contains("<path"), "{svg}");
        assert!(svg.contains("<use"), "{svg}");
        assert!(!svg.contains("<text"), "{svg}");
    }
}
