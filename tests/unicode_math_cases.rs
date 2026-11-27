use mathema::ast::{MathNode, PhantomKind};
use mathema::error::{Config, DiagnosticCode, DiagnosticKind, DiagnosticPhase};
use mathema::writer::Writer;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{Error, Format, LimitKind, Strictness, convert, from_latex, from_unicode_math_with};

fn has_text(node: &MathNode) -> bool {
    match node {
        MathNode::Text(..) => true,
        MathNode::Row(children) | MathNode::Styled(_, children) => children.iter().any(has_text),
        MathNode::Fenced { content, .. } => content.iter().any(|item| match item {
            mathema::ast::FencedItem::Element(node) => has_text(node),
            mathema::ast::FencedItem::Middle(_) => false,
        }),
        MathNode::Array { rows, .. }
        | MathNode::Matrix { rows, .. }
        | MathNode::NativeArray { rows, .. } => rows.iter().flatten().flatten().any(has_text),
        _ => false,
    }
}

#[test]
fn latex_cases_use_readable_cases_operator_and_round_trip_as_structure() {
    let source =
        from_latex(r"\begin{cases} x & x>0 \\ -x & x\le0 \end{cases}").expect("LaTeX cases");
    let compact = source.to_unicode_math().expect("UnicodeMath");
    let heap = UnicodeMathWriter::write(&source.expr(), &Config::default()).expect("heap");

    assert_eq!(compact, heap);
    assert_eq!(compact, "Ⓒ(x&x>0@−x&x≤0)");

    let reparsed = from_unicode_math_with(
        &compact,
        &Config::default().with_display(source.expr().display),
    )
    .expect("cases should parse");
    assert!(matches!(
        reparsed.expr().nodes.as_slice(),
        [MathNode::Fenced { open, close, content }]
            if open == "{" && close.is_empty() && content.len() == 1
    ));
    assert!(!has_text(&reparsed.expr().nodes[0]));
}

#[test]
fn legacy_one_sided_matrix_cases_are_recovered_as_rows() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let parsed = from_unicode_math_with("{■(x&x≥0@−x&x<0)", &config).expect("legacy cases");
        let mathml = parsed.to_mathml().unwrap();
        assert!(mathml.contains("<mtable"), "{mathml}");
        assert!(!mathml.contains("<mtext>"), "{mathml}");
        assert_eq!(parsed.to_unicode_math().unwrap(), "Ⓒ(x&x≥0@−x&x<0)");
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("legacy one-sided"))
        );
        assert!(parsed.diagnostic_records().iter().any(|record| {
            record.code == DiagnosticCode::ReaderInterpretation
                && record.phase == DiagnosticPhase::Read
                && record.kind == DiagnosticKind::Warning
        }));
    }
}

#[test]
fn equation_arrays_use_an_aligned_structure_and_stable_spelling() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let source = "█(a&=b@c&=d)";
        let parsed = from_unicode_math_with(source, &config).expect("equation array");
        let mathml = parsed.to_mathml().expect("MathML");
        assert!(mathml.contains("<mtable"), "{mathml}");
        assert_eq!(
            mathml.matches(r#"columnalign="right""#).count(),
            2,
            "{mathml}"
        );
        assert_eq!(
            mathml.matches(r#"columnalign="left""#).count(),
            2,
            "{mathml}"
        );
        assert!(!mathml.contains("<mtext>"), "{mathml}");
        assert_eq!(parsed.to_unicode_math().unwrap(), source);
        assert_eq!(
            UnicodeMathWriter::write_compact(parsed.compact_expr(), &Config::default()).unwrap(),
            source
        );
        assert!(
            parsed
                .to_unicode_math_with_diagnostics()
                .unwrap()
                .1
                .is_empty()
        );
        assert!(parsed.diagnostics().is_empty());
        let standard = parsed
            .to_mathml_with_options(&mathema::writer::mathml::MathmlOptions::standard())
            .unwrap();
        assert!(standard.contains(r#"columnalign="right""#), "{standard}");
        assert!(standard.contains(r#"columnalign="left""#), "{standard}");
        let heap = parsed.expr();
        assert_eq!(
            UnicodeMathWriter::write(&heap, &Config::default()).unwrap(),
            source
        );
        let compact_from_heap = mathema::ast::compact::CompactExpr::from_math_expr(&heap).unwrap();
        assert_eq!(
            UnicodeMathWriter::write_compact(&compact_from_heap, &Config::default()).unwrap(),
            source
        );
        let heap_mathml =
            mathema::writer::mathml::MathmlWriter::write(&heap, &Config::default()).unwrap();
        assert!(heap_mathml.contains(r#"columnalign="right""#));
        assert!(heap_mathml.contains(r#"columnalign="left""#));
        let omml = OmmlWriter::write_compact(parsed.compact_expr(), &Config::default()).unwrap();
        assert!(omml.contains(r#"<m:mcJc m:val="right"/>"#), "{omml}");
        assert!(omml.contains(r#"<m:mcJc m:val="left"/>"#), "{omml}");
    }
}

#[test]
fn equation_number_suffix_is_accepted_and_reported_as_metadata_loss() {
    for source in ["█(E&=mc^2)#(30)", "█(E&=mc^2#(30))"] {
        let parsed = from_unicode_math_with(source, &Config::default()).unwrap();
        let mathml = parsed.to_mathml().unwrap();
        assert!(mathml.contains("<mtable"), "{source}: {mathml}");
        assert!(!mathml.contains("<mtext>"), "{source}: {mathml}");
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("equation-number metadata"))
        );
        assert!(parsed.diagnostic_records().iter().any(|record| {
            record.code == DiagnosticCode::ReaderInterpretation
                && record.phase == DiagnosticPhase::Read
                && record.kind == DiagnosticKind::Warning
        }));
        assert_eq!(parsed.to_unicode_math().unwrap(), "█(E&=mc^2)");
    }
}

#[test]
fn equation_array_alignment_alternates_alignment_and_spacer_columns() {
    let source = "█(10&x+&3&y=2@3&x+&13&y=4)";
    let parsed = from_unicode_math_with(source, &Config::default()).unwrap();
    let mathml = parsed.to_mathml().unwrap();
    assert_eq!(
        mathml.matches(r#"columnalign="right""#).count(),
        4,
        "{mathml}"
    );
    assert_eq!(
        mathml.matches(r#"columnalign="left""#).count(),
        4,
        "{mathml}"
    );
    assert_eq!(parsed.to_unicode_math().unwrap(), source);
    let omml = OmmlWriter::write_compact(parsed.compact_expr(), &Config::default()).unwrap();
    assert_eq!(
        omml.matches(r#"<m:mcJc m:val="right"/>"#).count(),
        2,
        "{omml}"
    );
    assert_eq!(
        omml.matches(r#"<m:mcJc m:val="left"/>"#).count(),
        2,
        "{omml}"
    );
}

#[test]
fn phantom_and_smash_shortcuts_preserve_the_modeled_kind() {
    for (source, written, kind) in [
        ("⟡(x)", "⟡x", PhantomKind::Full),
        ("⇳x", "⇳x", PhantomKind::Vertical),
        ("⬌x", "⬌x", PhantomKind::Horizontal),
        ("⬍x", "⬍x", PhantomKind::SmashBoth),
        ("⬇x", "⬇x", PhantomKind::SmashBottom),
        ("⬆x", "⬆x", PhantomKind::SmashTop),
    ] {
        let parsed = from_unicode_math_with(source, &Config::default()).unwrap();
        assert!(
            matches!(
                parsed.expr().nodes.as_slice(),
                [MathNode::Phantom(_, actual)] if *actual == kind
            ),
            "{source}: {:?}",
            parsed.expr()
        );
        assert_eq!(parsed.to_unicode_math().unwrap(), written);
        assert!(
            parsed
                .to_unicode_math_with_diagnostics()
                .unwrap()
                .1
                .is_empty(),
            "{source}"
        );
    }
}

#[test]
fn nested_and_adjacent_cases_remain_structured() {
    let source = from_latex(
        r"\begin{cases} \begin{cases} a & b \\ c & d \end{cases} & y \\ z & w \end{cases}q",
    )
    .expect("nested cases");
    let unicode = source.to_unicode_math().expect("UnicodeMath");
    assert_eq!(unicode.matches('Ⓒ').count(), 2, "{unicode}");

    let reparsed = from_unicode_math_with(&unicode, &Config::default()).expect("reparse");
    assert!(
        !reparsed.expr().nodes.iter().any(has_text),
        "{:?}",
        reparsed.expr().nodes
    );
    assert_eq!(
        convert(&unicode, Format::UnicodeMath, Format::UnicodeMath).unwrap(),
        unicode
    );
}

#[test]
fn unparsed_structured_input_reports_literal_text_recovery() {
    let result = from_unicode_math_with(
        "■(a#b)",
        &Config::default().with_strictness(Strictness::Strict),
    )
    .expect("fallback remains non-fatal");
    assert!(result.expr().nodes.iter().any(has_text));
    assert_eq!(result.diagnostics().len(), 1, "{:?}", result.diagnostics());
    assert!(
        result.diagnostics()[0]
            .message
            .contains("recovered as text")
    );

    let quoted = from_unicode_math_with("\"a#b\"", &Config::default()).expect("quoted text");
    assert!(
        quoted.diagnostics().is_empty(),
        "{:?}",
        quoted.diagnostics()
    );
}

#[test]
fn cases_keep_resource_limits_as_hard_errors() {
    let input = format!("Ⓒ({})", "&".repeat(256));
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default()
            .with_strictness(strictness)
            .with_max_nodes(Some(32));
        assert!(
            matches!(
                from_unicode_math_with(&input, &config),
                Err(Error::LimitExceeded {
                    kind: LimitKind::Nodes,
                    limit: 32
                })
            ),
            "{strictness:?}"
        );
    }
}
