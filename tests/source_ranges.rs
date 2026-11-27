use mathema::error::Span;
use mathema::{Config, from_latex_with, from_mathml_with, from_unicode_math_with};

#[test]
fn latex_keeps_ranges_for_source_backed_top_level_nodes_when_enabled() {
    let source = "x^2 + y";
    let parsed = from_latex_with(source, &Config::default().with_source_ranges(true)).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert!(roots.len() >= 3);
    let ranges: Vec<_> = roots
        .iter()
        .filter_map(|&id| parsed.compact_expr().source_range(id))
        .collect();
    assert!(!ranges.is_empty());
    for span in ranges {
        assert!(span.start < span.end);
        assert!(span.end <= source.len());
        assert!(source.is_char_boundary(span.start));
        assert!(source.is_char_boundary(span.end));
    }
    assert_eq!(
        parsed.compact_expr().source_range(roots[0]),
        Some(Span::new(0, 3))
    );
}

#[test]
fn unicode_math_ranges_include_trimmed_leading_whitespace() {
    let source = "  x^2 y";
    let parsed =
        from_unicode_math_with(source, &Config::default().with_source_ranges(true)).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert_eq!(
        parsed.compact_expr().source_range(roots[0]),
        Some(Span::new(2, 5))
    );
}

#[test]
fn ranges_are_disabled_without_the_opt_in_and_xml_inputs_remain_safe() {
    let latex = from_latex_with("x^2", &Config::default()).unwrap();
    let roots = latex
        .compact_expr()
        .arena
        .children(latex.compact_expr().roots);
    assert!(
        roots
            .iter()
            .all(|&id| latex.compact_expr().source_range(id).is_none())
    );

    let mathml_source = "  <math><mi>x</mi><msup><mi>y</mi><mn>2</mn></msup></math>  ";
    let mathml =
        from_mathml_with(mathml_source, &Config::default().with_source_ranges(true)).unwrap();
    let roots = mathml
        .compact_expr()
        .arena
        .children(mathml.compact_expr().roots);
    assert!(roots.iter().any(|&id| {
        mathml.compact_expr().source_range(id).is_some_and(|span| {
            span.start >= 2 && span.start < span.end && span.end <= mathml_source.len()
        })
    }));
}

#[test]
fn rewritten_macro_text_has_no_fabricated_source_range() {
    let config = Config::default().with_source_ranges(true);
    let parsed = from_latex_with(r"\def\letter{z}\letter", &config).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert!(
        roots
            .iter()
            .all(|&id| parsed.compact_expr().source_range(id).is_none())
    );
}

#[test]
fn mtef_ranges_use_original_record_offsets_when_enabled() {
    let source = mathema::from_latex("x^2").unwrap();
    let bytes = source
        .to_mtef_bytes(&mathema::MtefWriteOptions::default())
        .unwrap();
    let parsed =
        mathema::from_mtef_bytes_with(&bytes, &Config::default().with_source_ranges(true)).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert!(!roots.is_empty());
    let root = roots[0];
    let children = match *parsed.compact_expr().arena.node(root) {
        mathema::CompactNode::Superscript { base, sup }
        | mathema::CompactNode::Subscript { base, sub: sup } => [base, sup],
        _ => [root, root],
    };
    assert!(children.iter().any(|&id| {
        parsed
            .compact_expr()
            .source_range(id)
            .is_some_and(|span| span.start < span.end && span.end <= bytes.len())
    }));
}

#[test]
fn mtef_ranges_include_an_equation_native_header_offset() {
    let source = mathema::from_latex("x").unwrap();
    let bytes = source
        .to_mtef_bytes(&mathema::MtefWriteOptions::default().with_ole_header(true))
        .unwrap();
    let parsed =
        mathema::from_mtef_bytes_with(&bytes, &Config::default().with_source_ranges(true)).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert!(roots.iter().any(|&id| {
        parsed
            .compact_expr()
            .source_range(id)
            .is_some_and(|span| span.start >= 28 && span.end <= bytes.len())
    }));
}

#[test]
fn hex_mtef_ranges_use_decoded_wire_offsets() {
    let source = mathema::from_latex("x").unwrap();
    let bytes = source
        .to_mtef_bytes(&mathema::MtefWriteOptions::default())
        .unwrap();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let parsed =
        mathema::from_mtef_with(&hex, &Config::default().with_source_ranges(true)).unwrap();
    let roots = parsed
        .compact_expr()
        .arena
        .children(parsed.compact_expr().roots);
    assert!(roots.iter().any(|&id| {
        parsed
            .compact_expr()
            .source_range(id)
            .is_some_and(|span| span.end <= bytes.len() && span.end < hex.len())
    }));
}

#[test]
fn omml_ranges_follow_utf8_event_offsets_across_content_controls() {
    let source = r#"<!--π--><m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:oMath><m:r><m:t>α</m:t></m:r></m:oMath><w:sdt><w:sdtContent><m:oMath><m:r><m:t>β</m:t></m:r></m:oMath></w:sdtContent></w:sdt></m:oMathPara>"#;
    let paragraph = mathema::reader::omml::OmmlReader::parse_paragraph(
        source,
        &Config::default().with_source_ranges(true),
    )
    .unwrap();
    assert_eq!(paragraph.equations.len(), 2);
    for (text, equation) in [
        ("α", &paragraph.equations[0]),
        ("β", &paragraph.equations[1]),
    ] {
        let needle = format!("<m:r><m:t>{text}</m:t></m:r>");
        let start = source.find(&needle).unwrap();
        let roots = equation.arena.children(equation.roots);
        assert!(
            roots.iter().any(|&id| {
                equation
                    .source_range(id)
                    .is_some_and(|span| span == Span::new(start, start + needle.len()))
            }),
            "missing exact range for {text}"
        );
    }
}

#[test]
fn source_ranges_do_not_add_recursive_stack_frames_to_deep_latex() {
    let open = r"\begin{matrix}";
    let close = r"\end{matrix}";
    let source = format!("{}x{}", open.repeat(130), close.repeat(130));
    assert!(
        mathema::from_latex_with(&source, &Config::default().with_source_ranges(true),).is_err()
    );
}
