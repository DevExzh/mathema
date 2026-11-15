use super::*;
use crate::reader::Reader as _;
use crate::reader::omml::OmmlReader;

fn write(node: MathNode) -> String {
    OmmlWriter::write(&MathExpr::inline(vec![node]), &Config::default()).unwrap()
}

fn write_with(node: MathNode, options: &OmmlOptions) -> (String, Vec<Diagnostic>) {
    let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
    OmmlWriter::write_with_options(&compact, &Config::default(), options).unwrap()
}

fn id(s: &str) -> MathNode {
    MathNode::Identifier(s.into())
}

fn reparse(omml: &str) -> Vec<MathNode> {
    OmmlReader::parse(omml, &Config::default())
        .unwrap()
        .expr
        .nodes
}

#[test]
fn number_and_identifier() {
    assert!(write(MathNode::Number("42".into())).contains("<m:r><m:t>42</m:t></m:r>"));
    // A single letter is math italic by default and needs no properties.
    assert!(write(id("x")).contains("<m:r><m:t>x</m:t></m:r>"));
    assert!(write(id("ab")).contains(r#"<m:rPr><m:sty m:val="i"/></m:rPr><m:t>ab</m:t>"#));
}

#[test]
fn bare_equation_declares_only_the_math_namespace() {
    let out = write(id("x"));
    assert_eq!(
        out,
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r></m:oMath>"#
    );
}

#[test]
fn script_comes_before_style_in_run_properties() {
    // CT_RPR: lit?, (nor | (scr?, sty?)), brk?, aln?
    let out = write(MathNode::Styled(TextStyle::BoldFraktur, vec![id("g")]));
    assert!(
        out.contains(r#"<m:rPr><m:scr m:val="fraktur"/><m:sty m:val="b"/></m:rPr>"#),
        "{out}"
    );
}

#[test]
fn normal_text_is_written_with_nor_and_keeps_whitespace() {
    let out = write(MathNode::Text(TextStyle::Normal, " if ".into()));
    assert!(
        out.contains(r#"<m:r><m:rPr><m:nor/></m:rPr><m:t xml:space="preserve"> if </m:t></m:r>"#),
        "{out}"
    );
    assert_eq!(
        reparse(&out),
        vec![MathNode::Text(TextStyle::Normal, " if ".into())]
    );
}

#[test]
fn operator_is_an_upright_run_not_a_function() {
    let out = write(MathNode::Operator(OperatorAttr {
        name: "sin".into(),
        limit_style: LimitStyle::SubSup,
    }));
    assert!(!out.contains("m:func"), "{out}");
    assert!(
        out.contains(r#"<m:sty m:val="p"/></m:rPr><m:t>sin</m:t>"#),
        "{out}"
    );
}

#[test]
fn function_application_is_written_as_func() {
    let node = MathNode::Row(vec![
        MathNode::Operator(OperatorAttr {
            name: "sin".into(),
            limit_style: LimitStyle::SubSup,
        }),
        MathNode::Symbol(SymbolKind::Ordinary, "\u{2061}".into()),
        id("x"),
    ]);
    let out = write(node.clone());
    assert!(
        out.contains("<m:func><m:fName><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>"),
        "{out}"
    );
    assert_eq!(reparse(&out), vec![node]);
}

#[test]
fn delimiter_properties_follow_schema_order() {
    // CT_DPr: begChr, sepChr, endChr.
    let out = write(MathNode::Fenced {
        open: "[".into(),
        close: "]".into(),
        content: vec![
            FencedItem::Element(id("a")),
            FencedItem::Middle(";".into()),
            FencedItem::Element(id("b")),
        ],
    });
    assert!(
        out.contains(
            r#"<m:dPr><m:begChr m:val="["/><m:sepChr m:val=";"/><m:endChr m:val="]"/></m:dPr>"#
        ),
        "{out}"
    );
    assert_eq!(out.matches("<m:e>").count(), 2, "{out}");
}

#[test]
fn delimiter_defaults_are_omitted_and_empty_content_has_one_argument() {
    let out = write(MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![],
    });
    assert!(out.contains("<m:d><m:e></m:e></m:d>"), "{out}");
    // An empty delimiter value means "no delimiter".
    let out = write(MathNode::Fenced {
        open: "{".into(),
        close: String::new(),
        content: vec![FencedItem::Element(id("x"))],
    });
    assert!(
        out.contains(r#"<m:begChr m:val="{"/><m:endChr m:val=""/>"#),
        "{out}"
    );
}

#[test]
fn special_delimiter_attributes_are_escaped() {
    let (out, diags) = write_with(
        MathNode::Fenced {
            open: "<".into(),
            close: "\"".into(),
            content: vec![
                FencedItem::Element(id("a")),
                FencedItem::Middle("&".into()),
                FencedItem::Element(id("b")),
            ],
        },
        &OmmlOptions::default(),
    );
    assert!(out.contains(r#"<m:begChr m:val="&lt;"/>"#), "{out}");
    assert!(out.contains(r#"<m:sepChr m:val="&amp;"/>"#), "{out}");
    assert!(out.contains(r#"<m:endChr m:val="&quot;"/>"#), "{out}");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn multi_character_delimiter_is_reported() {
    let (out, diags) = write_with(
        MathNode::Fenced {
            open: "((".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(id("a"))],
        },
        &OmmlOptions::default(),
    );
    assert!(out.contains(r#"<m:begChr m:val="("/>"#), "{out}");
    assert!(!diags.is_empty());
}

#[test]
fn equation_array_round_trips_as_eq_arr() {
    let node = MathNode::Array {
        columns: vec![ColumnAlign::Center],
        rows: vec![vec![vec![id("a")]], vec![vec![id("b")]]],
        arraystretch: None,
        add_jot: true,
    };
    let out = write(node.clone());
    assert!(out.contains("<m:eqArr><m:e>"), "{out}");
    assert!(!out.contains("<m:m>"), "{out}");
    assert_eq!(reparse(&out), vec![node]);
}

#[test]
fn matrix_columns_are_grouped_and_written_in_order() {
    let out = write(MathNode::Array {
        columns: vec![ColumnAlign::Left, ColumnAlign::Left, ColumnAlign::Right],
        rows: vec![vec![vec![id("a")], vec![id("b")], vec![id("c")]]],
        arraystretch: None,
        add_jot: false,
    });
    assert!(
        out.contains(r#"<m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="2"/><m:mcJc m:val="left"/></m:mcPr></m:mc><m:mc><m:mcPr><m:count m:val="1"/><m:mcJc m:val="right"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr>"#),
        "{out}"
    );
}

#[test]
fn empty_matrix_has_the_required_row_and_cell() {
    let out = write(MathNode::Array {
        columns: vec![],
        rows: vec![],
        arraystretch: None,
        add_jot: false,
    });
    assert!(out.contains("<m:m><m:mr><m:e></m:e></m:mr></m:m>"), "{out}");
}

#[test]
fn box_atoms_round_trip() {
    for atom_type in [MathAtomType::Ord, MathAtomType::Op] {
        let node = MathNode::MathAtom {
            atom_type,
            content: Box::new(id("x")),
        };
        let out = write(node.clone());
        assert!(out.contains("<m:box>"), "{out}");
        assert_eq!(reparse(&out), vec![node]);
    }
}

#[test]
fn cancel_hides_the_box_sides() {
    for stroke in [
        StrokeKind::Forward,
        StrokeKind::Backward,
        StrokeKind::Cross,
        StrokeKind::Horizontal,
        StrokeKind::Vertical,
    ] {
        let node = MathNode::Cancel {
            stroke,
            content: Box::new(id("x")),
        };
        let out = write(node.clone());
        assert!(
            out.contains("<m:borderBoxPr><m:hideTop m:val=\"1\"/><m:hideBot m:val=\"1\"/><m:hideLeft m:val=\"1\"/><m:hideRight m:val=\"1\"/><m:strike"),
            "{out}"
        );
        assert_eq!(reparse(&out), vec![node], "{out}");
    }
}

#[test]
fn boxed_cancel_is_one_border_box() {
    let node = MathNode::Boxed {
        content: Box::new(MathNode::Cancel {
            stroke: StrokeKind::Forward,
            content: Box::new(id("x")),
        }),
        bg_color: None,
        border_color: None,
    };
    let out = write(node.clone());
    assert_eq!(out.matches("<m:borderBox>").count(), 1, "{out}");
    assert_eq!(reparse(&out), vec![node]);
}

#[test]
fn over_script_with_general_content_keeps_the_content() {
    let (out, diags) = write_with(
        MathNode::Overscript {
            base: Box::new(id("x")),
            over: Box::new(id("y")),
            movable_limits: false,
        },
        &OmmlOptions::default(),
    );
    assert!(out.contains("<m:limUpp>"), "{out}");
    assert!(out.contains("<m:t>y</m:t>"), "{out}");
    assert!(!diags.is_empty());
}

#[test]
fn nary_default_limits_omit_lim_loc() {
    let out = write(MathNode::Nary {
        op: '\u{2211}',
        sub: Some(Box::new(MathNode::Number("0".into()))),
        sup: None,
        body: Box::new(id("i")),
        limits: LimitStyle::Default,
    });
    assert!(
        out.contains(r#"<m:naryPr><m:chr m:val="∑"/><m:supHide m:val="1"/></m:naryPr><m:sub>"#),
        "{out}"
    );
}

#[test]
fn sqrt_has_hidden_empty_degree() {
    let out = write(MathNode::Sqrt(Box::new(MathNode::Number("2".into()))));
    assert!(
        out.contains(r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>"#),
        "{out}"
    );
}

#[test]
fn phantom_writes_show_off_only_when_hidden() {
    let out = write(MathNode::Phantom(Box::new(id("x")), PhantomKind::Full));
    assert!(
        out.contains(r#"<m:phant><m:phantPr><m:show m:val="0"/></m:phantPr>"#),
        "{out}"
    );
}

#[test]
fn spaces_use_unicode_math_spaces() {
    let out = write(MathNode::Space(Rational::new(1, 6)));
    assert!(out.contains("<m:t>\u{2009}</m:t>"), "{out}");
    assert_eq!(reparse(&out), vec![MathNode::Space(Rational::new(1, 6))]);
    let out = write(MathNode::Space(Rational::new(2, 1)));
    assert!(out.contains("<m:t>\u{2003}\u{2003}</m:t>"), "{out}");
}

#[test]
fn invalid_xml_characters_are_dropped_with_a_diagnostic() {
    let (out, diags) = write_with(
        MathNode::Text(TextStyle::Normal, "a\u{1}b".into()),
        &OmmlOptions::default(),
    );
    assert!(out.contains("<m:t>ab</m:t>"), "{out}");
    assert!(!diags.is_empty());
    // A run with nothing representable left is not written at all.
    let (out, _) = write_with(
        MathNode::Symbol(SymbolKind::Ordinary, "\u{0}".into()),
        &OmmlOptions::default(),
    );
    assert!(!out.contains("<m:r>"), "{out}");
}

#[test]
fn display_block_with_justification() {
    let options = OmmlOptions::default().with_display_block(Some(OmmlJustification::Center));
    let (out, _) = write_with(id("x"), &options);
    assert_eq!(
        out,
        r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMathParaPr><m:jc m:val="center"/></m:oMathParaPr><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara>"#
    );
}

fn colored_expr() -> CompactExpr {
    crate::reader::latex::LatexReader::parse_compact(r"\textcolor{red}{x}", &Config::default())
        .unwrap()
        .expr
}

#[test]
fn color_is_never_written_into_math_run_properties() {
    let (out, diags) = OmmlWriter::write_with_options(
        &colored_expr(),
        &Config::default(),
        &OmmlOptions::default(),
    )
    .unwrap();
    assert!(!out.contains("color"), "{out}");
    assert!(!out.contains("w:"), "{out}");
    assert!(out.contains("<m:t>x</m:t>"), "{out}");
    assert!(
        diags.iter().any(|d| d.message.contains("color")),
        "expected a color-loss diagnostic: {diags:?}"
    );
}

#[test]
fn wordprocessing_run_properties_follow_math_run_properties() {
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl);
    let (out, diags) =
        OmmlWriter::write_with_options(&colored_expr(), &Config::default(), &options).unwrap();
    assert!(
        out.starts_with(r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#),
        "{out}"
    );
    assert!(
        out.contains(r#"<m:r><w:rPr><w:color w:val="FF0000"/></w:rPr><m:t>x</m:t></m:r>"#),
        "{out}"
    );
    assert!(diags.is_empty(), "{diags:?}");
    // The color reads back.
    let back = OmmlReader::parse_compact(&out, &Config::default()).unwrap();
    let (again, _) =
        OmmlWriter::write_with_options(&back.expr, &Config::default(), &options).unwrap();
    assert_eq!(again, out);
}

#[test]
fn drawing_run_properties_carry_color_and_text_emphasis() {
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::DrawingMl);
    let (out, _) =
        OmmlWriter::write_with_options(&colored_expr(), &Config::default(), &options).unwrap();
    assert!(
        out.contains(r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#),
        "{out}"
    );
    assert!(
        out.contains(r#"<m:r><a:rPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:rPr><m:t>x</m:t></m:r>"#),
        "{out}"
    );
    let (out, _) = write_with(MathNode::Text(TextStyle::Bold, "and".into()), &options);
    assert!(
        out.contains(r#"<m:rPr><m:nor/></m:rPr><a:rPr b="1"/><m:t>and</m:t>"#),
        "{out}"
    );
    assert_eq!(
        reparse(&out),
        vec![MathNode::Text(TextStyle::Bold, "and".into())]
    );
}

#[test]
fn unresolvable_color_keeps_content_and_warns() {
    let config = Config::default();
    let result =
        crate::reader::latex::LatexReader::parse_compact(r"\textcolor{notacolor}{y}", &config)
            .unwrap();
    let (output, diags) =
        OmmlWriter::write_compact_with_diagnostics(&result.expr, &config).unwrap();
    assert!(output.contains("<m:t>y</m:t>"), "{output}");
    assert!(
        diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning && d.message.contains("color")),
        "expected a color-loss warning, got {diags:?}"
    );
}

#[test]
fn escaped_text_preserves_adjacent_newlines_across_round_trip() {
    use crate::ast::normalize::normalize_compact_expr;
    let config = Config::default();
    let node = MathNode::Text(TextStyle::Normal, "x'\nM".into());
    let mut first = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
    normalize_compact_expr(&mut first).unwrap();
    let out1 = OmmlWriter::write_compact(&first, &config).unwrap();
    let mut second = OmmlReader::parse_compact(&out1, &config).unwrap().expr;
    normalize_compact_expr(&mut second).unwrap();
    let out2 = OmmlWriter::write_compact(&second, &config).unwrap();
    assert_eq!(out1, out2);
}

#[test]
fn paragraph_round_trip_keeps_equations_and_justification() {
    let input = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#;
    let config = Config::default();
    let para = OmmlReader::parse_paragraph(input, &config).unwrap();
    let (out, diags) =
        OmmlWriter::write_paragraph(&para, &config, &OmmlOptions::default()).unwrap();
    assert_eq!(out, input);
    assert!(diags.is_empty());
}

#[test]
fn compact_and_tree_writers_agree() {
    let node = MathNode::Rule {
        width: Rational::new(1, 1),
        height: Rational::new(1, 1),
        lift: Rational::zero(),
    };
    let tree = write(node.clone());
    let compact = OmmlWriter::write_compact(
        &CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap(),
        &Config::default(),
    )
    .unwrap();
    assert_eq!(tree, compact);
}

#[test]
fn array_rows_wider_than_the_column_list_stay_one_row() {
    let out = write(MathNode::Array {
        columns: vec![],
        rows: vec![vec![vec![id("a")], vec![id("b")]]],
        arraystretch: None,
        add_jot: false,
    });
    assert_eq!(out.matches("<m:mr>").count(), 1, "{out}");
    assert_eq!(out.matches("<m:e>").count(), 2, "{out}");
}

#[test]
fn aligned_equation_array_writes_ampersand_alignment_points() {
    let config = Config::default();
    let expr = crate::reader::latex::LatexReader::parse_compact(
        r"\begin{aligned} x &= 1 \\ y + z &= 2 \end{aligned}",
        &config,
    )
    .unwrap()
    .expr;
    let out = OmmlWriter::write_compact(&expr, &config).unwrap();
    assert!(out.contains("<m:eqArr><m:e>"), "{out}");
    assert_eq!(out.matches("<m:t>&amp;</m:t>").count(), 2, "{out}");
    assert!(!out.contains("<m:m>"), "{out}");
    let back = OmmlReader::parse_compact(&out, &config).unwrap();
    assert_eq!(OmmlWriter::write_compact(&back.expr, &config).unwrap(), out);
    // And it reads back as LaTeX alignment, not an escaped ampersand.
    let latex = crate::writer::latex::LatexWriter::write_compact(&back.expr, &config).unwrap();
    assert!(!latex.contains("\\&"), "{latex}");
}

#[test]
fn trailing_alignment_point_survives_the_round_trip() {
    let config = Config::default();
    let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:eqArr><m:e><m:r><m:t>&amp;x&amp;</m:t></m:r></m:e><m:e><m:r><m:t>&amp;</m:t></m:r></m:e></m:eqArr></m:oMath>"#;
    let first = OmmlReader::parse_compact(input, &config).unwrap();
    let out = OmmlWriter::write_compact(&first.expr, &config).unwrap();
    assert!(
        out.contains("<m:eqArr><m:e><m:r><m:t>&amp;</m:t></m:r><m:r><m:t>x</m:t></m:r><m:r><m:t>&amp;</m:t></m:r></m:e>"),
        "{out}"
    );
    let second = OmmlReader::parse_compact(&out, &config).unwrap();
    assert_eq!(first.expr.to_math_expr(), second.expr.to_math_expr());
}

#[test]
fn omml_color_reaches_latex_as_html_color() {
    let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:r><w:rPr><w:color w:val="C00000"/></w:rPr><m:t>e</m:t></m:r></m:oMath>"#;
    let latex = crate::convert(input, crate::Format::Omml, crate::Format::LaTeX).unwrap();
    assert!(latex.contains(r"\textcolor[HTML]{C00000}{e}"), "{latex}");
}
