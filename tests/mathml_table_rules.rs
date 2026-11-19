use mathema::ast::MathNode;
use mathema::error::{Config, DiagnosticKind};
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;

fn native_array(expr: &mathema::MathExpr) -> &mathema::ast::MathNode {
    expr.nodes.first().expect("table root")
}

#[test]
fn native_latex_rules_and_padding_survive_heap_compact_mathml_and_latex() {
    let source = r"\begin{array}{|l|}a\\b\\\hline c\end{array}";
    let config = Config::default();
    let parsed = mathema::from_latex(source).unwrap();
    match native_array(&parsed.expr()) {
        MathNode::NativeArray {
            array_style,
            col_seps,
            row_seps,
            ..
        } => {
            assert!(*array_style);
            assert_eq!(*col_seps, 0b11);
            assert_eq!(*row_seps, 0b100);
        },
        other => panic!("expected NativeArray, got {other:?}"),
    };
    let heap_mathml = MathmlWriter::write(&parsed.expr(), &config).unwrap();
    let compact_mathml = parsed.to_mathml().unwrap();
    assert_eq!(heap_mathml, compact_mathml);
    assert!(compact_mathml.contains(r#"rowlines="none solid""#));
    assert!(compact_mathml.contains(r#"data-mathema-col-seps="0x3""#));
    assert!(compact_mathml.contains(r#"data-mathema-array-style="true""#));

    let reimported = mathema::from_mathml(&compact_mathml).unwrap();
    assert!(
        reimported.diagnostics().is_empty(),
        "{:?}",
        reimported.diagnostics()
    );
    assert_eq!(reimported.to_mathml().unwrap(), compact_mathml);
    assert_eq!(
        reimported.to_latex().unwrap(),
        r"\begin{array}{|l|} a \\ b \\ \hline c \end{array}"
    );
    #[cfg(feature = "render")]
    assert_eq!(parsed.to_svg().unwrap(), reimported.to_svg().unwrap());
}

#[test]
fn standard_frame_and_line_lists_follow_mathml_repeat_last_rules() {
    let xml = r#"<math><mtable frame="solid" columnlines="solid none solid" rowlines="solid"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd><mtd><mi>c</mi></mtd></mtr><mtr><mtd><mi>d</mi></mtd><mtd><mi>e</mi></mtd><mtd><mi>f</mi></mtd></mtr><mtr><mtd><mi>g</mi></mtd><mtd><mi>h</mi></mtd><mtd><mi>i</mi></mtd></mtr></mtable></math>"#;
    let parsed = mathema::from_mathml(xml).unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    match native_array(&parsed.expr()) {
        MathNode::NativeArray {
            col_seps, row_seps, ..
        } => {
            // Frame supplies both exterior edges. The excess third
            // columnlines token is ignored; the first interior line is solid
            // and the second is none. A one-token rowlines list repeats.
            assert_eq!(*col_seps, 0b1011);
            assert_eq!(*row_seps, 0b1111);
        },
        other => panic!("expected NativeArray, got {other:?}"),
    }
    let canonical = parsed.to_mathml().unwrap();
    assert_eq!(
        MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap(),
        canonical
    );
    assert!(canonical.contains(r#"frame="solid""#), "{canonical}");
    assert!(
        canonical.contains(r#"columnlines="solid none""#),
        "{canonical}"
    );
    assert!(canonical.contains(r#"rowlines="solid""#), "{canonical}");
    assert_eq!(
        mathema::from_mathml(&canonical)
            .unwrap()
            .to_mathml()
            .unwrap(),
        canonical
    );
}

#[test]
fn dashed_rules_are_dropped_with_reader_diagnostics_not_solidified() {
    let xml = r#"<math><mtable frame="dashed" columnlines="dashed solid" rowlines="none dashed"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd><mtd><mi>c</mi></mtd></mtr><mtr><mtd><mi>d</mi></mtd><mtd><mi>e</mi></mtd><mtd><mi>f</mi></mtd></mtr><mtr><mtd><mi>g</mi></mtd><mtd><mi>h</mi></mtd><mtd><mi>i</mi></mtd></mtr></mtable></math>"#;
    let parsed = mathema::from_mathml(xml).unwrap();
    let warnings: Vec<_> = parsed
        .diagnostics()
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::Warning)
        .map(|diag| diag.message.as_str())
        .collect();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings.iter().any(|message| message.contains("frame")));
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("columnlines"))
    );
    assert!(warnings.iter().any(|message| message.contains("rowlines")));
    match native_array(&parsed.expr()) {
        MathNode::NativeArray {
            col_seps, row_seps, ..
        } => {
            // Only the solid interior column line survives.
            assert_eq!(*col_seps, 0b0100);
            assert_eq!(*row_seps, 0);
        },
        other => panic!("expected NativeArray, got {other:?}"),
    }
    let canonical = parsed.to_mathml().unwrap();
    assert!(!canonical.contains("dashed"), "{canonical}");
    assert!(
        canonical.contains(r#"columnlines="none solid""#),
        "{canonical}"
    );
}

#[test]
fn line_styles_beyond_the_native_32_boundary_are_reported_and_bounded() {
    let cells = (0..33).map(|_| "<mtd><mi>x</mi></mtd>").collect::<String>();
    let xml = format!(r#"<math><mtable columnlines="solid"><mtr>{cells}</mtr></mtable></math>"#);
    let parsed = mathema::from_mathml(&xml).unwrap();
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|diag| diag.message.contains("boundary 32"))
    );
    match native_array(&parsed.expr()) {
        MathNode::NativeArray { col_seps, .. } => {
            assert_eq!(*col_seps, u32::MAX << 1);
            assert_eq!(*col_seps & (1 << 31), 1 << 31);
        },
        other => panic!("expected NativeArray, got {other:?}"),
    }
}

#[test]
fn ordinary_unruled_mathml_tables_do_not_gain_rule_or_native_padding_attributes() {
    let xml =
        r#"<math><mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr></mtable></math>"#;
    let canonical = mathema::from_mathml(xml).unwrap().to_mathml().unwrap();
    for attribute in [
        "frame=",
        "rowlines=",
        "columnlines=",
        "data-mathema-col-seps",
        "data-mathema-row-seps",
        "data-mathema-array-style",
    ] {
        assert!(!canonical.contains(attribute), "{attribute}: {canonical}");
    }
}

#[test]
fn native_padding_extension_survives_authored_mathml_heap_and_compact_paths() {
    let xml = r#"<math><mtable data-mathema-array-style="true"><mtr><mtd><mi>a</mi></mtd></mtr></mtable></math>"#;
    let parsed = mathema::from_mathml(xml).unwrap();
    assert!(matches!(
        native_array(&parsed.expr()),
        MathNode::NativeArray {
            array_style: true,
            col_seps: 0,
            row_seps: 0,
            ..
        }
    ));
    let compact = parsed.to_mathml().unwrap();
    let heap = MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap();
    assert_eq!(compact, heap);
    assert!(compact.contains(r#"data-mathema-array-style="true""#));
}
