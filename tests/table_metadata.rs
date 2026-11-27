use mathema::Config;
use mathema::ast::compact::CompactExpr;
use mathema::ast::{ColumnAlign, MathExpr, MathNode};
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;

fn ragged_table(arraystretch: Option<f64>) -> MathExpr {
    MathExpr::inline(vec![MathNode::Array {
        columns: vec![ColumnAlign::Center, ColumnAlign::Center],
        rows: vec![
            vec![vec![MathNode::Identifier("a".into())]],
            vec![
                vec![MathNode::Identifier("b".into())],
                vec![MathNode::Identifier("c".into())],
            ],
        ],
        arraystretch,
        add_jot: false,
    }])
}

#[test]
fn compact_round_trip_preserves_authored_ragged_rows_and_exact_stretch() {
    for stretch in [
        Some(3.0),
        Some(1.234),
        Some(f64::from_bits(1.0f64.to_bits() + 1)),
    ] {
        let heap = ragged_table(stretch);
        let compact = CompactExpr::from_math_expr(&heap).unwrap();
        let rebuilt = compact.to_math_expr();
        let MathNode::Array {
            rows, arraystretch, ..
        } = &rebuilt.nodes[0]
        else {
            panic!("expected array")
        };
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(*arraystretch, stretch);
    }
}

#[test]
fn zero_column_empty_rows_and_table_edits_are_safe() {
    let empty = MathExpr::inline(vec![MathNode::Array {
        columns: Vec::new(),
        rows: vec![Vec::new(), Vec::new()],
        arraystretch: None,
        add_jot: false,
    }]);
    let compact = CompactExpr::from_math_expr(&empty).unwrap();
    let rebuilt = compact.to_math_expr();
    let MathNode::Array { rows, .. } = &rebuilt.nodes[0] else {
        panic!("expected empty array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [0, 0]);

    let mut edited = CompactExpr::from_math_expr(&ragged_table(Some(3.0))).unwrap();
    let root = edited.arena.children(edited.roots)[0];
    if let mathema::CompactNode::Array { num_cols, .. } =
        edited.arena.try_node_mut_in_place(root).unwrap()
    {
        *num_cols = 1;
    } else {
        panic!("expected array")
    }
    let rebuilt = edited.to_math_expr();
    assert!(matches!(
        rebuilt.nodes.first(),
        Some(MathNode::Array { .. })
    ));
}

#[test]
fn in_place_borrows_and_benign_edits_keep_exact_metadata() {
    let mut compact = CompactExpr::from_math_expr(&ragged_table(Some(1.234))).unwrap();
    let root = compact.arena.children(compact.roots)[0];
    let _borrow = compact.arena.try_node_mut_in_place(root).unwrap();
    let rebuilt = compact.to_math_expr();
    let MathNode::Array {
        rows, arraystretch, ..
    } = &rebuilt.nodes[0]
    else {
        panic!("expected array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(*arraystretch, Some(1.234));

    if let mathema::CompactNode::Array { add_jot, .. } =
        compact.arena.try_node_mut_in_place(root).unwrap()
    {
        *add_jot = true;
    }
    let rebuilt = compact.to_math_expr();
    let MathNode::Array {
        rows, arraystretch, ..
    } = &rebuilt.nodes[0]
    else {
        panic!("expected array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(*arraystretch, Some(1.234));

    if let mathema::CompactNode::Array {
        arraystretch_pct, ..
    } = compact.arena.try_node_mut_in_place(root).unwrap()
    {
        *arraystretch_pct = 255;
    }
    let rebuilt = compact.to_math_expr();
    let MathNode::Array {
        rows, arraystretch, ..
    } = &rebuilt.nodes[0]
    else {
        panic!("expected array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(*arraystretch, Some(2.55));
}

#[test]
fn compact_writer_keeps_authored_stretch_marker() {
    let heap = ragged_table(Some(1.234));
    let compact = CompactExpr::from_math_expr(&heap).unwrap();
    let xml = MathmlWriter::write_compact(&compact, &Config::default()).unwrap();
    assert!(
        xml.contains("data-mathema-arraystretch=\"123.4%\""),
        "{xml}"
    );
    assert_eq!(xml.matches("<mtr>").count(), 2, "{xml}");
}

#[test]
fn mathml_import_round_trip_keeps_exact_stretch_and_ragged_rows() {
    let source = r#"<math><mtable data-mathema-arraystretch="123.4%"><mtr><mtd><mi>a</mi></mtd></mtr><mtr><mtd/><mtd><mi>b</mi></mtd></mtr></mtable></math>"#;
    let parsed = mathema::from_mathml(source).unwrap();
    let MathNode::Array {
        rows, arraystretch, ..
    } = &parsed.expr().nodes[0]
    else {
        panic!("expected imported array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(*arraystretch, Some(1.234));
    let output = parsed.to_mathml().unwrap();
    assert!(output.contains("data-mathema-arraystretch=\"123.4%\""));
    let reparsed = mathema::from_mathml(&output).unwrap();
    let MathNode::Array {
        rows, arraystretch, ..
    } = &reparsed.expr().nodes[0]
    else {
        panic!("expected reimported array")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(*arraystretch, Some(1.234));
}

#[test]
fn latex_cases_keep_authored_ragged_rows_across_text_writers() {
    let parsed = mathema::from_latex(r"\begin{cases}a\\b&c\end{cases}").unwrap();
    let latex = parsed.to_latex().unwrap();
    assert!(latex.contains("a \\\\ b & c"), "{latex}");
    let mathml = parsed.to_mathml().unwrap();
    assert_eq!(mathml.matches("<mtd").count(), 3, "{mathml}");
    let unicode = parsed.to_unicode_math().unwrap();
    assert_eq!(unicode, "Ⓒ(a@b&c)");
}

#[test]
fn matrix_cell_line_breaks_reach_a_two_pass_fixed_point() {
    let source = r"\begin{matrix}a{\\}b & {c\\d}e & {\\}\end{matrix}";
    let first = mathema::from_latex(source).unwrap().to_latex().unwrap();
    let second = mathema::from_latex(&first).unwrap().to_latex().unwrap();
    assert_eq!(first, second, "{first}");

    for text in [source.to_owned(), first, second] {
        let expression = mathema::from_latex(&text).unwrap().expr();
        let (MathNode::Matrix { rows, .. } | MathNode::Array { rows, .. }) = &expression.nodes[0]
        else {
            panic!("expected matrix: {text}")
        };
        assert_eq!(rows.len(), 1, "{text}");
        assert_eq!(rows[0].len(), 3, "{text}");
        assert!(
            rows[0]
                .iter()
                .any(|cell| { cell.iter().any(|node| matches!(node, MathNode::LineBreak)) })
        );
    }
}

#[test]
fn portable_latex_profile_keeps_authored_stretch() {
    let heap = ragged_table(Some(1.234));
    let compact = CompactExpr::from_math_expr(&heap).unwrap();
    let config = Config::default().with_latex_profile(mathema::LatexProfile::PdfLaTeX);
    let latex = LatexWriter::write_compact(&compact, &config).unwrap();
    assert!(
        latex.contains(r"\renewcommand{\arraystretch}{1.234}"),
        "{latex}"
    );
}

#[cfg(feature = "render")]
#[test]
fn compact_layout_honors_authored_threefold_stretch() {
    let normal = ragged_table(Some(1.0));
    let stretched = ragged_table(Some(3.0));
    let options = mathema::render::RenderOptions::default();
    let normal = mathema::render::render_to_svg(&normal, &options).unwrap();
    let stretched = mathema::render::render_to_svg(&stretched, &options).unwrap();
    let height = |svg: &str| {
        svg.split_once("height=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .and_then(|(value, _)| value.parse::<f64>().ok())
            .unwrap()
    };
    assert!(height(&stretched) > height(&normal) * 2.0);
}
