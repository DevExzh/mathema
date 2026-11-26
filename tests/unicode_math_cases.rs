use mathema::ast::MathNode;
use mathema::error::Config;
use mathema::writer::Writer;
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
