use super::*;

const NS: &str = r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math""#;

fn omath(body: &str) -> String {
    format!("<m:oMath {NS}>{body}</m:oMath>")
}

fn parse(input: &str) -> Vec<MathNode> {
    OmmlReader::parse(input, &Config::default())
        .unwrap()
        .expr
        .nodes
}

fn parse_one(input: &str) -> MathNode {
    let nodes = parse(input);
    if nodes.len() == 1 {
        nodes.into_iter().next().unwrap()
    } else {
        MathNode::Row(nodes)
    }
}

fn strict_err(input: &str) -> String {
    match OmmlReader::parse(input, &Config::default()) {
        Err(e) => e.to_string(),
        Ok(r) => panic!("expected a Strict error, got {:?}", r.expr.nodes),
    }
}

fn lenient(input: &str) -> ParseResult {
    OmmlReader::parse(
        input,
        &Config::default().with_strictness(Strictness::Lenient),
    )
    .unwrap()
}

fn id(s: &str) -> MathNode {
    MathNode::Identifier(s.into())
}

fn run(t: &str) -> String {
    format!("<m:r><m:t>{t}</m:t></m:r>")
}

// ── Basic objects ───────────────────────────────────────────────────────

#[test]
fn simple_run_number() {
    assert_eq!(parse_one(&omath(&run("42"))), MathNode::Number("42".into()));
}

#[test]
fn simple_run_identifier() {
    assert_eq!(parse_one(&omath(&run("x"))), id("x"));
}

#[test]
fn compound_limit_operator_runs_remain_one_named_operator() {
    for name in [
        "lim\u{2009}sup",
        "lim\u{2009}inf",
        "inj\u{2009}lim",
        "proj\u{2009}lim",
    ] {
        let input = omath(&format!(
            r#"<m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>{name}</m:t></m:r>"#
        ));
        assert!(matches!(parse_one(&input), MathNode::Operator(attr) if attr.name == name));

        let result = crate::from_omml(&input).unwrap();
        let mathml = result.to_mathml().unwrap();
        assert!(mathml.contains(name), "{name}: {mathml}");
        let reparsed = crate::from_mathml(&mathml).unwrap();
        assert_eq!(reparsed.to_mathml().unwrap(), mathml, "{name}");

        let bounded = omath(&format!(
            r#"<m:sSub><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>{name}</m:t></m:r></m:e><m:sub><m:r><m:t>n</m:t></m:r></m:sub></m:sSub>"#
        ));
        assert!(matches!(parse_one(&bounded), MathNode::Subscript { .. }));
        let bounded_mathml = crate::from_omml(&bounded).unwrap().to_mathml().unwrap();
        assert!(bounded_mathml.contains(name), "{name}: {bounded_mathml}");
        assert!(
            bounded_mathml.contains("<msub>") || bounded_mathml.contains("<munder>"),
            "{name}: {bounded_mathml}"
        );
    }

    // An arbitrary thin-spaced run remains a sequence of identifiers and a
    // space. Only the canonical named-limit spellings above get operator
    // treatment.
    assert!(matches!(
        parse_one(&omath(&run("foo\u{2009}bar"))),
        MathNode::Row(_)
    ));
}

#[test]
fn compound_limit_objects_decode_side_and_stacked_bounds() {
    let side = omath(
        r#"<m:sSub><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>lim sup</m:t></m:r></m:e><m:sub><m:r><m:t>n</m:t></m:r></m:sub></m:sSub>"#,
    );
    assert!(matches!(parse_one(&side), MathNode::Subscript { .. }));

    let stacked = omath(
        r#"<m:limLow><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>lim sup</m:t></m:r></m:e><m:lim><m:r><m:t>n</m:t></m:r></m:lim></m:limLow>"#,
    );
    assert!(
        matches!(parse_one(&stacked), MathNode::Subscript { base, .. }
        if matches!(*base, MathNode::Operator(ref op) if op.limit_style == LimitStyle::AboveBelow))
    );
    let inline = Config::default().with_display(crate::ast::DisplayStyle::Inline);
    let output = crate::from_omml_with(&stacked, &inline)
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        output.contains("lim sup") && output.contains(r#"movablelimits="false""#),
        "{output}"
    );
    assert!(output.contains("<munder>"), "{output}");
}

#[test]
fn authored_generic_omml_limit_over_an_accent_stays_generic() {
    let input = omath(
        r#"<m:limLow>
          <m:e><m:bar><m:barPr><m:pos m:val="top"/></m:barPr>
            <m:e><m:r><m:t>x</m:t></m:r></m:e>
          </m:bar></m:e>
          <m:lim><m:r><m:t>i</m:t></m:r></m:lim>
        </m:limLow>"#,
    );
    let parsed = crate::from_omml(&input).unwrap();
    assert!(
        !parsed.to_latex().unwrap().contains(r"\operatorname"),
        "generic m:limLow was converted into a named operator"
    );
    assert!(
        matches!(parse_one(&input), MathNode::Underscript { .. }),
        "generic authored limit changed class"
    );
}

#[test]
fn omml_function_name_keeps_a_real_operand_outside_the_name_container() {
    let input = omath(
        r#"<m:func>
          <m:fName><m:bar><m:barPr><m:pos m:val="top"/></m:barPr>
            <m:e><m:r><m:t>lim</m:t></m:r></m:e>
          </m:bar></m:fName>
          <m:e><m:r><m:t>x</m:t></m:r></m:e>
        </m:func>"#,
    );
    let parsed = crate::from_omml(&input).unwrap();
    assert!(
        !parsed.to_latex().unwrap().contains(r"\operatorname"),
        "an occupied m:e was mistaken for a name-only function"
    );
    assert!(
        matches!(parse_one(&input), MathNode::Row(_)),
        "the function operand was not retained as an adjacent row"
    );
}

#[test]
fn fraction() {
    let node = parse_one(&omath(
        "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>",
    ));
    assert!(matches!(
        node,
        MathNode::Fraction {
            kind: FractionKind::Normal,
            ..
        }
    ));
}

#[test]
fn subscript_and_superscript() {
    let node = parse_one(&omath(&format!(
        "<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>",
        run("x"),
        run("i")
    )));
    assert!(matches!(node, MathNode::Subscript { .. }));
    let node = parse_one(&omath(&format!(
        "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
        run("x"),
        run("2")
    )));
    assert!(matches!(node, MathNode::Superscript { .. }));
}

#[test]
fn sqrt() {
    let node = parse_one(&omath(&format!(
        r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>"#,
        run("x")
    )));
    assert!(matches!(node, MathNode::Sqrt(_)));
}

#[test]
fn on_off_without_val_means_on() {
    // CT_OnOff: an absent val attribute means "on".
    let node = parse_one(&omath(&format!(
        "<m:rad><m:radPr><m:degHide/></m:radPr><m:deg>{}</m:deg><m:e>{}</m:e></m:rad>",
        run("3"),
        run("x")
    )));
    assert!(matches!(node, MathNode::Sqrt(_)), "{node:?}");
    let node = parse_one(&omath(&format!(
        r#"<m:nary><m:naryPr><m:chr m:val="∑"/><m:subHide/><m:supHide/></m:naryPr><m:sub/><m:sup/><m:e>{}</m:e></m:nary>"#,
        run("a")
    )));
    assert!(
        matches!(
            node,
            MathNode::Nary {
                sub: None,
                sup: None,
                op: '∑',
                ..
            }
        ),
        "{node:?}"
    );
}

#[test]
fn delimiter() {
    let node = parse_one(&omath(&format!(
        r#"<m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e>{}</m:e></m:d>"#,
        run("x")
    )));
    assert!(matches!(node, MathNode::Fenced { .. }));
}

#[test]
fn delimiter_sep_chr_becomes_middle_item() {
    let node = parse_one(&omath(&format!(
        r#"<m:d><m:dPr><m:begChr m:val="["/><m:sepChr m:val=";"/><m:endChr m:val="]"/></m:dPr><m:e>{}</m:e><m:e>{}</m:e></m:d>"#,
        run("a"),
        run("b")
    )));
    assert_eq!(
        node,
        MathNode::Fenced {
            open: "[".into(),
            close: "]".into(),
            content: vec![
                FencedItem::Element(id("a")),
                FencedItem::Middle(";".into()),
                FencedItem::Element(id("b")),
            ],
        }
    );
}

#[test]
fn delimiter_defaults_follow_the_schema() {
    // CT_DPr defaults: begChr "(", sepChr "|", endChr ")".
    let node = parse_one(&omath(&format!(
        "<m:d><m:e>{}</m:e><m:e>{}</m:e></m:d>",
        run("a"),
        run("b")
    )));
    assert_eq!(
        node,
        MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![
                FencedItem::Element(id("a")),
                FencedItem::Middle("|".into()),
                FencedItem::Element(id("b")),
            ],
        }
    );
}

#[test]
fn delimiter_props_accept_alternate_prefix_and_unescape_values() {
    let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:d><q:dPr><q:begChr q:val="&lt;"/><q:sepChr q:val="&amp;"/><q:endChr q:val="&gt;"/></q:dPr><q:e><q:r><q:t>a</q:t></q:r></q:e><q:e><q:r><q:t>b</q:t></q:r></q:e></q:d></q:oMath>"#;
    assert_eq!(
        parse_one(input),
        MathNode::Fenced {
            open: "<".into(),
            close: ">".into(),
            content: vec![
                FencedItem::Element(id("a")),
                FencedItem::Middle("&".into()),
                FencedItem::Element(id("b")),
            ],
        }
    );
}

#[test]
fn matrix_props_accept_alternate_value_prefix() {
    let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:m><q:mPr><q:mcs><q:mc><q:mcPr><q:count q:val="2"/><q:mcJc q:val="left"/></q:mcPr></q:mc></q:mcs></q:mPr><q:mr><q:e><q:r><q:t>a</q:t></q:r></q:e><q:e><q:r><q:t>b</q:t></q:r></q:e></q:mr></q:m></q:oMath>"#;
    match parse_one(input) {
        MathNode::Array {
            columns, add_jot, ..
        } => {
            assert_eq!(columns, vec![ColumnAlign::Left, ColumnAlign::Left]);
            assert!(!add_jot);
        },
        other => panic!("expected matrix array, got {other:?}"),
    }
}

#[test]
fn matrix_mcs_column_alignment_is_preserved() {
    let input = omath(&format!(
        r#"<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="1"/><m:mcJc m:val="left"/></m:mcPr></m:mc><m:mc><m:mcPr><m:count m:val="1"/><m:mcJc m:val="right"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr></m:m>"#,
        run("a"),
        run("b")
    ));
    match parse_one(&input) {
        MathNode::Array { columns, .. } => {
            assert_eq!(columns, vec![ColumnAlign::Left, ColumnAlign::Right]);
        },
        other => panic!("expected matrix array, got {other:?}"),
    }
}

#[test]
fn letters_of_an_italic_run_are_variables() {
    for text in ["abc", "Pr", "sin", "lim"] {
        let input = omath(&format!(
            r#"<m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>{text}</m:t></m:r>"#
        ));
        let letters = text.chars().map(|c| id(&c.to_string())).collect();
        assert_eq!(parse_one(&input), MathNode::Row(letters));
    }
}

#[test]
fn border_box_strikes_with_hidden_sides_become_cancel_strokes() {
    let hidden = r#"<m:hideTop/><m:hideBot/><m:hideLeft/><m:hideRight/>"#;
    let cases = [
        ("strikeBLTR", StrokeKind::Forward),
        ("strikeTLBR", StrokeKind::Backward),
        ("strikeH", StrokeKind::Horizontal),
        ("strikeV", StrokeKind::Vertical),
    ];
    for (strike, stroke) in cases {
        let input = omath(&format!(
            r#"<m:borderBox><m:borderBoxPr>{hidden}<m:{strike} m:val="1"/></m:borderBoxPr><m:e>{}</m:e></m:borderBox>"#,
            run("x")
        ));
        assert_eq!(
            parse_one(&input),
            MathNode::Cancel {
                stroke,
                content: Box::new(id("x")),
            },
            "{strike} should parse as {stroke:?} cancel"
        );
    }
}

#[test]
fn border_box_strike_with_visible_sides_is_a_boxed_cancel() {
    let input = omath(&format!(
        r#"<m:borderBox><m:borderBoxPr><m:strikeBLTR/><m:strikeTLBR/></m:borderBoxPr><m:e>{}</m:e></m:borderBox>"#,
        run("x")
    ));
    match parse_one(&input) {
        MathNode::Boxed { content, .. } => assert_eq!(
            *content,
            MathNode::Cancel {
                stroke: StrokeKind::Cross,
                content: Box::new(id("x")),
            }
        ),
        other => panic!("expected boxed cancel, got {other:?}"),
    }
}

#[test]
fn disabled_strike_with_alternate_value_prefix_stays_boxed() {
    let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:borderBox><q:borderBoxPr><q:strikeBLTR q:val="0"/></q:borderBoxPr><q:e><q:r><q:t>x</q:t></q:r></q:e></q:borderBox></q:oMath>"#;
    assert!(matches!(parse_one(input), MathNode::Boxed { .. }));
}

#[test]
fn accent() {
    let node = parse_one(&omath(&format!(
        r#"<m:acc><m:accPr><m:chr m:val="̂"/></m:accPr><m:e>{}</m:e></m:acc>"#,
        run("x")
    )));
    assert!(matches!(
        node,
        MathNode::Accent {
            accent: '\u{0302}',
            ..
        }
    ));
}

#[test]
fn bar_defaults_to_bottom() {
    // CT_BarPr pos defaults to "bot".
    let node = parse_one(&omath(&format!("<m:bar><m:e>{}</m:e></m:bar>", run("x"))));
    assert!(matches!(node, MathNode::Underscript { .. }), "{node:?}");
    let node = parse_one(&omath(&format!(
        r#"<m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e>{}</m:e></m:bar>"#,
        run("x")
    )));
    assert!(matches!(node, MathNode::Overscript { .. }), "{node:?}");
}

#[test]
fn empty_input() {
    let result = OmmlReader::parse("", &Config::default()).unwrap();
    assert!(result.expr.nodes.is_empty());
}

#[test]
fn border_box() {
    let node = parse_one(&omath(&format!(
        "<m:borderBox><m:e>{}</m:e></m:borderBox>",
        run("x")
    )));
    assert!(matches!(node, MathNode::Boxed { .. }));
}

#[test]
fn phantom_content_is_shown_unless_show_is_off() {
    // m:show defaults to on: a bare m:phant just shows its content.
    let node = parse_one(&omath(&format!(
        "<m:phant><m:e>{}</m:e></m:phant>",
        run("x")
    )));
    assert_eq!(node, id("x"));
    let node = parse_one(&omath(&format!(
        r#"<m:phant><m:phantPr><m:show m:val="off"/></m:phantPr><m:e>{}</m:e></m:phant>"#,
        run("x")
    )));
    assert!(matches!(node, MathNode::Phantom(_, PhantomKind::Full)));
}

#[test]
fn word_smash_round_trips_visible() {
    use crate::writer::Writer as _;
    // Word writes \smash as zeroAsc + zeroDesc with no m:show.
    let input = omath(&format!(
        r#"<m:phant><m:phantPr><m:zeroAsc m:val="on"/><m:zeroDesc m:val="on"/></m:phantPr><m:e>{}</m:e></m:phant>"#,
        run("x")
    ));
    let node = parse_one(&input);
    assert_eq!(
        node,
        MathNode::Phantom(Box::new(id("x")), PhantomKind::SmashBoth)
    );
    let expr = MathExpr::inline(vec![node.clone()]);
    let omml = crate::writer::omml::OmmlWriter::write(&expr, &Config::default()).unwrap();
    assert!(!omml.contains("m:show"), "{omml}");
    assert!(
        omml.contains(r#"<m:phantPr><m:zeroAsc m:val="1"/><m:zeroDesc m:val="1"/></m:phantPr>"#),
        "{omml}"
    );
    assert_eq!(parse_one(&omml), node);
}

#[test]
fn phantom_subtypes_from_phant_pr() {
    let cases = [
        (
            r#"<m:show m:val="0"/><m:zeroWid m:val="1"/>"#,
            PhantomKind::Vertical,
        ),
        (
            r#"<m:show m:val="0"/><m:zeroAsc m:val="1"/><m:zeroDesc m:val="1"/>"#,
            PhantomKind::Horizontal,
        ),
        (
            r#"<m:show m:val="1"/><m:zeroAsc m:val="1"/><m:zeroDesc m:val="1"/>"#,
            PhantomKind::SmashBoth,
        ),
        (
            r#"<m:show m:val="1"/><m:zeroAsc m:val="1"/>"#,
            PhantomKind::SmashTop,
        ),
        (
            r#"<m:show m:val="1"/><m:zeroDesc m:val="1"/>"#,
            PhantomKind::SmashBottom,
        ),
    ];
    for (inner, expected) in cases {
        let input = omath(&format!(
            "<m:phant><m:phantPr>{inner}</m:phantPr><m:e>{}</m:e></m:phant>",
            run("x")
        ));
        match parse_one(&input) {
            MathNode::Phantom(_, kind) => assert_eq!(kind, expected, "phantPr {inner}"),
            other => panic!("expected Phantom, got {other:?}"),
        }
    }
}

#[test]
fn phantom_subtype_omml_round_trips() {
    use crate::writer::Writer as _;
    for kind in [
        PhantomKind::Full,
        PhantomKind::Vertical,
        PhantomKind::Horizontal,
        PhantomKind::SmashBoth,
        PhantomKind::SmashTop,
        PhantomKind::SmashBottom,
    ] {
        let expr = MathExpr::inline(vec![MathNode::Phantom(Box::new(id("x")), kind)]);
        let omml = crate::writer::omml::OmmlWriter::write(&expr, &Config::default()).unwrap();
        match parse_one(&omml) {
            MathNode::Phantom(_, got) => assert_eq!(got, kind, "round-trip {kind:?}\n{omml}"),
            other => panic!("expected Phantom for {kind:?}, got {other:?}"),
        }
    }
}

// ── Well-formedness and Strict mode ─────────────────────────────────────

#[test]
fn malformed_xml_is_an_error_in_strict_mode() {
    let cases = [
        // Mismatched end tag.
        format!("<m:oMath {NS}><m:r><m:t>x</m:r></m:t></m:oMath>"),
        // Unclosed comment.
        format!("<m:oMath {NS}><!-- x {}</m:oMath>", run("x")),
        // Truncated input.
        format!("<m:oMath {NS}><m:f><m:num>{}", run("x")),
        // Duplicate attribute.
        format!(
            r#"<m:oMath {NS}><m:acc><m:accPr><m:chr m:val="a" m:val="b"/></m:accPr><m:e/></m:acc></m:oMath>"#
        ),
        // Unknown entity.
        omath("<m:r><m:t>&bogus;</m:t></m:r>"),
        // Text after the root element is fine, but a stray end tag is not.
        format!("{}</m:oMath>", omath(&run("x"))),
    ];
    for input in &cases {
        let err = strict_err(input);
        assert!(err.contains("OMML parse error"), "{input}: {err}");
    }
}

#[test]
fn malformed_xml_in_lenient_mode_recovers_with_a_warning() {
    let result = lenient(&format!("<m:oMath {NS}><m:r><m:t>x</m:r></m:oMath>"));
    assert!(result.has_warnings(), "{:?}", result.diagnostics);
}

#[test]
fn unknown_omml_element_is_named_in_strict_mode() {
    let input = omath(&format!("<m:bogus>{}</m:bogus>{}", run("y"), run("x")));
    let err = strict_err(&input);
    assert!(err.contains("m:bogus"), "{err}");

    let result = lenient(&input);
    assert_eq!(result.expr.nodes, vec![id("x")]);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning && d.message.contains("m:bogus")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn unknown_property_and_misplaced_elements_are_errors() {
    for input in [
        omath(&format!(
            r#"<m:f><m:fPr><m:bogus/></m:fPr><m:num>{}</m:num><m:den/></m:f>"#,
            run("a")
        )),
        omath(&format!(
            "<m:f><m:sub>{}</m:sub><m:num/><m:den/></m:f>",
            run("a")
        )),
        omath(&format!("<m:r><m:t>a</m:t><m:e/></m:r>{}", run("b"))),
        format!("<w:p xmlns:w=\"urn:x\">{}</w:p>", omath(&run("x"))),
    ] {
        strict_err(&input);
    }
}

#[test]
fn invalid_values_are_errors_in_strict_mode() {
    for input in [
        omath(r#"<m:r><m:rPr><m:sty m:val="q"/></m:rPr><m:t>x</m:t></m:r>"#),
        omath(r#"<m:f><m:fPr><m:type m:val="wide"/></m:fPr><m:num/><m:den/></m:f>"#),
        omath(r#"<m:rad><m:radPr><m:degHide m:val="maybe"/></m:radPr><m:deg/><m:e/></m:rad>"#),
        omath(r#"<m:d><m:dPr><m:begChr m:val="(("/></m:dPr><m:e/></m:d>"#),
        omath(
            r#"<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="999999999999"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/></m:mr></m:m>"#,
        ),
    ] {
        strict_err(&input);
        let result = lenient(&input);
        assert!(result.has_warnings(), "{input}: {:?}", result.diagnostics);
    }
}

#[test]
fn duplicated_arguments_are_errors_in_strict_mode() {
    let input = omath(&format!(
        "<m:f><m:num>{}</m:num><m:num>{}</m:num><m:den/></m:f>",
        run("a"),
        run("b")
    ));
    assert!(strict_err(&input).contains("duplicate"));
    let result = lenient(&input);
    assert!(result.has_warnings());
}

#[test]
fn characters_outside_xml_are_rejected() {
    let input = omath("<m:r><m:t>a\u{0}b</m:t></m:r>");
    strict_err(&input);
    let result = lenient(&input);
    assert_eq!(result.expr.nodes, vec![id("a"), id("b")]);
    assert!(result.has_warnings());
    strict_err(&omath(
        "<m:d><m:dPr><m:begChr m:val=\"\u{1}\"/></m:dPr><m:e/></m:d>",
    ));
}

#[test]
fn stray_text_between_elements_is_an_error() {
    strict_err(&omath(&format!("{}stray", run("x"))));
}

#[test]
fn doctype_is_rejected() {
    strict_err(&format!("<!DOCTYPE m:oMath>{}", omath(&run("x"))));
}

#[test]
fn undeclared_html_entity_is_resolved_only_in_lenient_mode() {
    let input = omath("<m:r><m:t>&alpha;</m:t></m:r>");
    strict_err(&input);
    let result = lenient(&input);
    assert_eq!(result.expr.nodes, vec![id("\u{03B1}")]);
    assert!(result.has_warnings());
}

#[test]
fn character_references_and_predefined_entities_resolve() {
    let node = parse_one(&omath("<m:r><m:t>&#x3c;&lt;&#8804;</m:t></m:r>"));
    assert_eq!(
        node,
        MathNode::Row(vec![
            MathNode::Symbol(SymbolKind::Relation, "<".into()),
            MathNode::Symbol(SymbolKind::Relation, "<".into()),
            MathNode::Symbol(SymbolKind::Relation, "\u{2264}".into()),
        ])
    );
    strict_err(&omath("<m:r><m:t>&#0;</m:t></m:r>"));
}

#[test]
fn run_operator_glyphs_take_symbol_table_class() {
    let node = parse_one(&omath("<m:r><m:t>a≨b⋒c∈d</m:t></m:r>"));
    let MathNode::Row(nodes) = node else {
        panic!("expected a row, got {node:?}");
    };
    assert_eq!(nodes[1], MathNode::Symbol(SymbolKind::Relation, "≨".into()));
    assert_eq!(nodes[3], MathNode::Symbol(SymbolKind::BinaryOp, "⋒".into()));
    assert_eq!(nodes[5], MathNode::Symbol(SymbolKind::Relation, "∈".into()));
    let latex = crate::convert(
        &omath("<m:r><m:t>a≨b</m:t></m:r>"),
        crate::Format::Omml,
        crate::Format::LaTeX,
    )
    .unwrap();
    assert_eq!(latex, r"a\lneqq b");
}

#[test]
fn ordinary_operator_glyphs_round_trip_through_omml() {
    for source in [r"a\dag b", r"a\triangledown b", r"a\mathord{\lneqq}b"] {
        let omml = crate::from_latex(source).unwrap().to_omml().unwrap();
        assert!(omml.contains("<m:box>"), "{source}: {omml}");
        let latex = crate::convert(&omml, crate::Format::Omml, crate::Format::LaTeX).unwrap();
        assert_eq!(latex, source);
    }
    // Operator uses stay plain runs.
    let omml = crate::from_latex(r"a\dagger b").unwrap().to_omml().unwrap();
    assert!(!omml.contains("<m:box>"), "{omml}");
}

#[test]
fn nesting_depth_is_bounded() {
    let depth = 2000;
    let mut input = format!("<m:oMath {NS}>");
    for _ in 0..depth {
        input.push_str("<m:sSup><m:e>");
    }
    input.push_str(&run("x"));
    for _ in 0..depth {
        input.push_str("</m:e><m:sup/></m:sSup>");
    }
    input.push_str("</m:oMath>");
    assert!(strict_err(&input).contains("depth"));
}

#[test]
fn child_count_beyond_the_arena_span_limit_is_an_error() {
    let mut body = String::new();
    for _ in 0..70_000 {
        body.push_str("<m:r><m:t>+</m:t></m:r>");
    }
    assert!(strict_err(&omath(&body)).contains("list length"));
}

#[test]
fn matrix_cell_count_is_bounded() {
    let row = format!("<m:mr>{}</m:mr>", "<m:e/>".repeat(300));
    let input = omath(&format!("<m:m>{}</m:m>", row.repeat(300)));
    assert!(strict_err(&input).contains("cells"));
}

#[test]
fn input_size_limit_is_enforced() {
    let config = Config::default().with_max_input_bytes(Some(16));
    let err = OmmlReader::parse(&omath(&run("x")), &config).unwrap_err();
    assert!(matches!(err, Error::InputTooLarge { .. }));
}

#[test]
fn fragment_without_namespace_declaration_is_accepted() {
    assert_eq!(
        parse_one("<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"),
        id("x")
    );
    assert_eq!(parse_one("<m:r><m:t>x</m:t></m:r>"), id("x"));
    assert_eq!(parse_one(&format!("\u{FEFF}{}", omath(&run("x")))), id("x"));
}

// ── Fidelity ────────────────────────────────────────────────────────────

#[test]
fn normal_text_keeps_its_whitespace() {
    let node = parse_one(&omath(
        r#"<m:r><m:rPr><m:nor/></m:rPr><m:t xml:space="preserve"> if </m:t></m:r>"#,
    ));
    assert_eq!(node, MathNode::Text(TextStyle::Normal, " if ".into()));
}

#[test]
fn math_run_whitespace_is_kept() {
    let node = parse_one(&omath("<m:r><m:t>a\u{a0}b</m:t></m:r>"));
    assert_eq!(
        node,
        MathNode::Row(vec![
            id("a"),
            MathNode::Text(TextStyle::Normal, "\u{a0}".into()),
            id("b"),
        ])
    );
    let node = parse_one(&omath("<m:r><m:t>a\u{2009}b</m:t></m:r>"));
    assert_eq!(
        node,
        MathNode::Row(vec![id("a"), MathNode::Space(Rational::new(1, 6)), id("b")])
    );
}

#[test]
fn plain_function_name_run_is_an_operator_not_a_function() {
    let node = parse_one(&omath(&run("sin")));
    assert!(
        matches!(node, MathNode::Operator(ref op) if op.name == "sin"),
        "{node:?}"
    );
}

#[test]
fn func_is_function_application() {
    let node = parse_one(&omath(
        r#"<m:func><m:funcPr><m:ctrlPr/></m:funcPr><m:fName><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>"#,
    ));
    match node {
        MathNode::Row(children) => {
            assert!(matches!(&children[0], MathNode::Operator(op) if op.name == "sin"));
            assert_eq!(
                children[1],
                MathNode::Symbol(SymbolKind::Ordinary, "\u{2061}".into())
            );
            assert_eq!(children[2], id("x"));
        },
        other => panic!("expected function application, got {other:?}"),
    }
}

#[test]
fn explicit_upright_style_is_kept() {
    let node = parse_one(&omath(
        r#"<m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>x</m:t></m:r>"#,
    ));
    assert_eq!(node, MathNode::Styled(TextStyle::Normal, vec![id("x")]));
    let node = parse_one(&omath(
        r#"<m:r><m:rPr><m:scr m:val="double-struck"/><m:sty m:val="p"/></m:rPr><m:t>R</m:t></m:r>"#,
    ));
    assert_eq!(
        node,
        MathNode::Styled(TextStyle::DoubleStruck, vec![id("R")])
    );
}

#[test]
fn mixed_math_run_is_tokenized() {
    let node = parse_one(&omath(&run("2x+3.5")));
    assert_eq!(
        node,
        MathNode::Row(vec![
            MathNode::Number("2".into()),
            id("x"),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("3.5".into()),
        ])
    );
}

#[test]
fn math_alphanumerics_are_decoded() {
    // PowerPoint stores math italic letters as U+1D400-block characters.
    assert_eq!(parse_one(&omath(&run("\u{1D465}"))), id("x"));
    assert_eq!(
        parse_one(&omath(&run("\u{1D431}"))),
        MathNode::Styled(TextStyle::Bold, vec![id("x")])
    );
    assert_eq!(parse_one(&omath(&run("\u{1D703}"))), id("\u{03B8}"));
    assert_eq!(
        parse_one(&omath(&run("\u{1D6AA}"))),
        MathNode::Styled(TextStyle::Bold, vec![id("\u{0393}")])
    );
}

#[test]
fn equation_array_is_an_add_jot_column() {
    let node = parse_one(&omath(&format!(
        "<m:eqArr><m:eqArrPr><m:ctrlPr/></m:eqArrPr><m:e>{}</m:e><m:e>{}</m:e></m:eqArr>",
        run("a"),
        run("b")
    )));
    match node {
        MathNode::Array {
            columns,
            rows,
            add_jot,
            ..
        } => {
            assert!(add_jot);
            assert_eq!(columns, vec![ColumnAlign::Center]);
            assert_eq!(rows, vec![vec![vec![id("a")]], vec![vec![id("b")]]]);
        },
        other => panic!("expected an equation array, got {other:?}"),
    }
}

#[test]
fn box_is_an_atom() {
    let node = parse_one(&omath(&format!("<m:box><m:e>{}</m:e></m:box>", run("x"))));
    assert_eq!(
        node,
        MathNode::MathAtom {
            atom_type: MathAtomType::Ord,
            content: Box::new(id("x")),
        }
    );
    let node = parse_one(&omath(&format!(
        "<m:box><m:boxPr><m:opEmu m:val=\"1\"/></m:boxPr><m:e>{}</m:e></m:box>",
        run("x")
    )));
    assert!(matches!(
        node,
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        }
    ));
}

#[test]
fn unrepresentable_properties_are_reported_as_info() {
    let result = OmmlReader::parse(
        &omath(&format!(
            r#"<m:eqArr><m:eqArrPr><m:rSp m:val="4"/></m:eqArrPr><m:e>{}</m:e></m:eqArr>"#,
            run("a")
        )),
        &Config::default(),
    )
    .unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Info && d.message.contains("m:rSp")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn paragraph_with_two_equations_is_kept_apart() {
    let input = format!(
        r#"<m:oMathPara {NS}><m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr><m:oMath>{}</m:oMath><m:oMath>{}</m:oMath></m:oMathPara>"#,
        run("a"),
        run("b")
    );
    let para = OmmlReader::parse_paragraph(&input, &Config::default()).unwrap();
    assert_eq!(para.justification, Some(OmmlJustification::Left));
    let nodes: Vec<_> = para
        .equations
        .iter()
        .map(|e| e.to_math_expr().nodes)
        .collect();
    assert_eq!(nodes, vec![vec![id("a")], vec![id("b")]]);
    assert!(
        para.equations
            .iter()
            .all(|e| e.display == DisplayStyle::Block)
    );

    // The single-expression API stacks them and says so.
    let result = OmmlReader::parse(&input, &Config::default()).unwrap();
    assert!(
        matches!(&result.expr.nodes[..], [MathNode::Array { add_jot: true, rows, .. }] if rows.len() == 2)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("parse_paragraph"))
    );
}

#[test]
fn host_run_color_is_read_into_a_color_node() {
    use crate::reader::Reader as _;
    let wml = format!(
        r#"<m:oMath {NS} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:r><w:rPr><w:rFonts w:ascii="Cambria Math"/><w:color w:val="FF0000"/></w:rPr><m:t>x</m:t></m:r></m:oMath>"#
    );
    let dml = format!(
        r#"<m:oMath {NS} xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><m:r><a:rPr lang="en-US" i="1"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:latin typeface="Cambria Math"/></a:rPr><m:t>x</m:t></m:r></m:oMath>"#
    );
    for input in [wml, dml] {
        let result = OmmlReader::parse_compact(&input, &Config::default()).unwrap();
        let arena = &result.expr.arena;
        let root = arena.children(result.expr.roots)[0];
        match *arena.node(root) {
            CompactNode::Color { color, content } => {
                assert_eq!(arena.str_val(color), "#FF0000");
                assert!(matches!(*arena.node(content), CompactNode::Identifier(_)));
            },
            ref other => panic!("expected a color node, got {other:?}"),
        }
    }
}

#[test]
fn tracked_changes_and_bookmarks_inside_equations() {
    let input = format!(
        r#"<m:oMath {NS} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:bookmarkStart w:id="0" w:name="eq"/><w:ins w:id="1" w:author="a">{}</w:ins><w:del w:id="2" w:author="a">{}</w:del><w:bookmarkEnd w:id="0"/></m:oMath>"#,
        run("x"),
        run("y")
    );
    assert_eq!(parse_one(&input), id("x"));
}

#[test]
fn content_controls_keep_only_content_in_strict_and_lenient_modes() {
    let input = r#"
      <w:sdt xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
             xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
        <w:sdtPr>
          <w:alias w:val="equation"/>
          <w:tag w:val="not-equation-content"/>
          <m:oMath><m:r><m:t>metadata</m:t></m:r></m:oMath>
        </w:sdtPr>
        <w:sdtEndPr><w:rPr><w:color w:val="00FF00"/></w:rPr></w:sdtEndPr>
        <w:sdtContent><m:r><m:t>x</m:t></m:r></w:sdtContent>
      </w:sdt>
    "#;

    let strict = OmmlReader::parse(input, &Config::default()).unwrap();
    assert_eq!(strict.expr.nodes, vec![id("x")]);
    assert!(
        strict
            .diagnostics
            .iter()
            .all(|d| d.kind == DiagnosticKind::Info)
    );
    assert!(
        strict
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sdtPr") && d.message.contains("dropped"))
    );
    assert!(
        strict
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sdtEndPr") && d.message.contains("dropped"))
    );

    let lenient = lenient(input);
    assert_eq!(lenient.expr.nodes, vec![id("x")]);
    assert!(
        lenient
            .diagnostics
            .iter()
            .all(|d| d.kind == DiagnosticKind::Info)
    );
}

#[test]
fn nested_content_controls_preserve_descendant_equations_and_namespace() {
    let input = r#"
      <q:sdt xmlns:q="http://purl.oclc.org/ooxml/wordprocessingml/main"
              xmlns:z="http://purl.oclc.org/ooxml/officeDocument/math">
        <q:sdtPr><q:id q:val="1"/></q:sdtPr>
        <q:sdtContent>
          <q:sdt>
            <q:sdtPr><q:tag q:val="inner"/></q:sdtPr>
            <q:sdtContent><z:oMath><z:r><z:t>y</z:t></z:r></z:oMath></q:sdtContent>
          </q:sdt>
        </q:sdtContent>
      </q:sdt>
    "#;
    let result = OmmlReader::parse(input, &Config::default()).unwrap();
    assert_eq!(result.expr.nodes, vec![id("y")]);
}

#[test]
fn content_control_inside_math_keeps_its_math_content() {
    let input = r#"
      <m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"
               xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <m:r><m:t>a</m:t></m:r>
        <w:sdt>
          <w:sdtPr><w:alias w:val="inner"/></w:sdtPr>
          <w:sdtContent><m:r><m:t>b</m:t></m:r></w:sdtContent>
        </w:sdt>
        <m:r><m:t>c</m:t></m:r>
      </m:oMath>
    "#;
    assert_eq!(parse(input), vec![id("a"), id("b"), id("c")]);
}

#[test]
fn block_content_controls_traverse_paragraphs_but_skip_prose() {
    let input = r#"
      <w:sdt xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
             xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
        <w:sdtPr><w:alias w:val="equation"/></w:sdtPr>
        <w:sdtContent>
          <w:p>
            <w:pPr><w:pStyle w:val="Normal"/></w:pPr>
            <w:r><w:t>before the equation</w:t></w:r>
            <m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>
            <w:r><w:t>after the equation</w:t></w:r>
          </w:p>
        </w:sdtContent>
      </w:sdt>
    "#;
    let result = OmmlReader::parse(input, &Config::default()).unwrap();
    assert_eq!(result.expr.nodes, vec![id("x")]);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.kind == DiagnosticKind::Info)
    );
}

#[test]
fn content_controls_may_omit_content_in_strict_mode() {
    let metadata_only = r#"
      <w:sdt xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:sdtPr><w:alias w:val="empty"/></w:sdtPr>
      </w:sdt>
    "#;
    let result = OmmlReader::parse(metadata_only, &Config::default()).unwrap();
    assert!(result.expr.nodes.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.kind == DiagnosticKind::Info)
    );

    let empty_in_math = r#"
      <m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"
               xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:sdt><w:sdtPr><w:id w:val="1"/></w:sdtPr></w:sdt>
      </m:oMath>
    "#;
    let result = OmmlReader::parse(empty_in_math, &Config::default()).unwrap();
    assert!(result.expr.nodes.is_empty());
}

#[test]
fn hidden_limit_content_is_dropped_with_a_note() {
    let input = omath(&format!(
        r#"<m:nary><m:naryPr><m:subHide m:val="1"/></m:naryPr><m:sub>{}</m:sub><m:sup>{}</m:sup><m:e>{}</m:e></m:nary>"#,
        run("0"),
        run("n"),
        run("a")
    ));
    let result = OmmlReader::parse(&input, &Config::default()).unwrap();
    assert!(matches!(
        &result.expr.nodes[0],
        MathNode::Nary {
            sub: None,
            sup: Some(_),
            ..
        }
    ));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("hidden"))
    );
}

#[test]
fn nary_without_operator_character_is_not_an_integral() {
    let input = omath(&format!(
        r#"<m:nary><m:naryPr><m:chr m:val=""/></m:naryPr><m:sub>{}</m:sub><m:sup/><m:e>{}</m:e></m:nary>"#,
        run("i"),
        run("a")
    ));
    assert!(strict_err(&input).contains("operator character"));
    let result = lenient(&input);
    assert!(result.has_warnings());
    assert!(!format!("{:?}", result.expr.nodes).contains('\u{222B}'));
    assert!(
        matches!(&result.expr.nodes[..], [MathNode::Row(r)] if matches!(r[0], MathNode::Subscript { .. }))
    );
}

#[test]
fn matrix_padding_is_bounded_over_the_whole_input() {
    // One 255-cell row plus 255 one-cell rows pads 64,770 cells.
    let matrix = format!(
        "<m:m><m:mr>{}</m:mr>{}</m:m>",
        "<m:e/>".repeat(255),
        "<m:mr><m:e/></m:mr>".repeat(255)
    );
    // A single such matrix fits the minimum budget ...
    let result = OmmlReader::parse(&omath(&matrix), &Config::default()).unwrap();
    assert!(matches!(&result.expr.nodes[..], [MathNode::Array { rows, .. }] if rows.len() == 256));
    // ... but repeating it cannot multiply memory: the budget is shared by
    // the whole input and scales with its length.
    let input = omath(&matrix.repeat(40));
    assert!(input.len() / 4 < 2 * 64_770);
    assert!(strict_err(&input).contains("padding"));
}

#[test]
fn host_run_properties_without_an_ast_home_are_reported() {
    let input = format!(
        r#"<m:oMath {NS} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><m:r><w:rPr><w:vanish/><w:highlight w:val="yellow"/></w:rPr><m:t>x</m:t></m:r><m:r><a:rPr sz="2400" i="1"><a:latin typeface="Cambria Math"/></a:rPr><m:t>y</m:t></m:r></m:oMath>"#
    );
    let result = OmmlReader::parse(&input, &Config::default()).unwrap();
    for name in ["<w:vanish>", "<w:highlight>", "a:rPr@sz", "<a:latin>"] {
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::Info && d.message.contains(name)),
            "{name}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn move_range_anchors_are_skipped() {
    let input = format!(
        r#"<m:oMath {NS} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:moveToRangeStart w:id="1" w:name="m"/>{}<w:moveToRangeEnd w:id="1"/><w:moveFromRangeStart w:id="2" w:name="n"/><w:moveFromRangeEnd w:id="2"/></m:oMath>"#,
        run("x")
    );
    assert_eq!(parse_one(&input), id("x"));
}

#[test]
fn equation_array_ampersands_are_alignment_points() {
    let input = omath(&format!(
        "<m:eqArr><m:e>{}</m:e><m:e>{}</m:e></m:eqArr>",
        run("x&amp;=1"),
        run("y+z&amp;=2")
    ));
    match parse_one(&input) {
        MathNode::Array {
            columns,
            rows,
            add_jot,
            ..
        } => {
            assert!(add_jot);
            assert_eq!(columns, vec![ColumnAlign::Right, ColumnAlign::Left]);
            assert_eq!(rows[0][0], vec![id("x")]);
            assert_eq!(
                rows[0][1],
                vec![
                    MathNode::Symbol(SymbolKind::Relation, "=".into()),
                    MathNode::Number("1".into())
                ]
            );
            assert_eq!(rows[1].len(), 2);
        },
        other => panic!("expected an aligned equation array, got {other:?}"),
    }
}

#[test]
fn declared_matrix_columns_draw_on_the_padding_budget() {
    // 257 column groups of 255 columns each and no rows.
    let mc = r#"<m:mc><m:mcPr><m:count m:val="255"/></m:mcPr></m:mc>"#;
    let matrix = format!(
        "<m:m><m:mPr><m:mcs>{}</m:mcs></m:mPr></m:m>",
        mc.repeat(257)
    );
    let input = omath(&matrix.repeat(312));
    assert!(input.len() <= crate::error::DEFAULT_MAX_INPUT_BYTES);
    assert!(strict_err(&input).contains("column alignments"));
    // Declared columns that rows actually use cost nothing extra.
    let used = omath(
        r#"<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="2"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/><m:e/></m:mr></m:m>"#,
    );
    assert!(OmmlReader::parse(&used, &Config::default()).is_ok());
}

#[test]
fn padding_cells_are_separate_nodes() {
    use crate::reader::Reader as _;
    let input = omath(&format!(
        "<m:m><m:mr><m:e/><m:e/><m:e/></m:mr><m:mr><m:e>{}</m:e></m:mr></m:m>",
        run("a")
    ));
    let mut expr = OmmlReader::parse_compact(&input, &Config::default())
        .unwrap()
        .expr;
    let root = expr.arena.children(expr.roots)[0];
    let CompactNode::Array { cells, .. } = *expr.arena.node(root) else {
        panic!("expected an array");
    };
    let cells: Vec<NodeId> = expr.arena.children(cells).to_vec();
    // Row 2 is [a, pad, pad]: edit the first padding cell in place.
    let (pad1, pad2) = (cells[4], cells[5]);
    assert_ne!(pad1, pad2);
    let x = expr.arena.push_str("x");
    let ident = expr.arena.push_node(CompactNode::Identifier(x));
    let span = expr.arena.push_children(&[ident]).unwrap();
    *expr.arena.node_mut_in_place(pad1) = CompactNode::Row(span);
    assert!(matches!(*expr.arena.node(pad2), CompactNode::Row(s) if s.is_empty()));
}
