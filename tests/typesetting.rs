//! Font sizes as node attributes, through every format that has them. What a
//! size must be is computed by hand from the source: MathML Core 2.1.4 and
//! 2.1.5 (`mathsize` sets the font size where it is, scripts are 0.7 of
//! it), ECMA-376 17.3.2.38 (`w:sz` in half points) and the MTEF SIZE record.

use mathema::ast::{FontSize, NodeAttrs, Rational, TypeSize};
use mathema::{
    Config, MathNode, MtefVersion, MtefWriteOptions, OmmlOptions, OmmlRunProperties,
    TypesettingPolicy, from_latex, from_mathml, from_mathml_with, from_mtef_bytes, from_omml,
};

fn points(num: i32, den: u32) -> NodeAttrs {
    NodeAttrs::with_size(FontSize::Points(Rational::new(num, den)))
}

fn logical(level: TypeSize) -> NodeAttrs {
    NodeAttrs::with_size(FontSize::Logical {
        level,
        delta_pt: Rational::zero(),
    })
}

fn sized(attrs: NodeAttrs, content: MathNode) -> MathNode {
    MathNode::Attributed {
        attrs,
        content: Box::new(content),
    }
}

fn id(name: &str) -> MathNode {
    MathNode::Identifier(name.into())
}

fn wml() -> OmmlOptions {
    OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl)
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/mtef/{name}.bin",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

#[test]
fn mathml_absolute_sizes_are_attributes() {
    let read = |body: &str| from_mathml(&format!("<math>{body}</math>")).unwrap().expr();
    for (value, size) in [
        ("18pt", points(18, 1)),
        ("1in", points(72, 1)),
        ("2pc", points(24, 1)),
        // CSS: 96px is an inch.
        ("16px", points(12, 1)),
    ] {
        let expr = read(&format!(r#"<mi mathsize="{value}">x</mi>"#));
        assert_eq!(expr.nodes, [sized(size, id("x"))], "{value}");
    }
    // A size declared around a list is that of its members, and the size
    // closest to a token wins.
    let expr =
        read(r#"<mstyle mathsize="18pt"><mi>x</mi><mi mathsize="9pt">y</mi></mstyle><mi>z</mi>"#);
    assert_eq!(
        expr.nodes,
        [
            MathNode::Row(vec![
                sized(points(18, 1), id("x")),
                sized(points(9, 1), id("y")),
            ]),
            id("z")
        ]
    );
}

#[test]
fn a_logical_size_is_used_only_while_it_matches_the_drawn_one() {
    let expr = from_mtef_bytes(&fixture("govdocs-v5-size-delta")).unwrap();
    let mathml = expr.to_mathml().unwrap();
    assert!(mathml.contains(r#"mathsize="10pt""#), "{mathml}");
    assert_eq!(
        from_mathml(&mathml).unwrap().expr().nodes,
        expr.expr().nodes
    );
    let edited = mathml.replace(r#"mathsize="10pt""#, r#"mathsize="20pt""#);
    assert_eq!(
        from_mathml(&edited).unwrap().expr().nodes,
        [sized(points(20, 1), id("H"))]
    );
}

/// What the preview pictures of these equations show is in the README.
#[test]
fn mtef_sizes_are_those_of_the_preview_pictures() {
    let nodes = |name| from_mtef_bytes(&fixture(name)).unwrap().expr().nodes;
    assert_eq!(
        nodes("govdocs-v5-size-points"),
        [sized(points(12, 1), id("r"))]
    );
    assert!(matches!(
        &nodes("govdocs-v3-size-points")[..],
        [MathNode::Attributed { attrs, .. }] if *attrs == points(9, 1)
    ));
    assert_eq!(
        nodes("govdocs-v5-size-typesize"),
        [sized(logical(TypeSize::Sub2), id("o"))]
    );
    assert_eq!(
        nodes("govdocs-v3-size-typesize")[1],
        sized(logical(TypeSize::Sub), MathNode::Number("12".into()))
    );
}

#[test]
fn mtef_sizes_survive_every_format_that_has_them() {
    for name in [
        "govdocs-v3-size-points",
        "govdocs-v5-size-points",
        "govdocs-v5-size-delta",
        "govdocs-v3-size-typesize",
        "govdocs-v5-size-typesize",
    ] {
        let read = from_mtef_bytes(&fixture(name)).unwrap();
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let bytes = read.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
            let again = from_mtef_bytes(&bytes).unwrap();
            assert_eq!(again.expr().nodes, read.expr().nodes, "{name} {version:?}");
        }
        let again = from_mathml(&read.to_mathml().unwrap()).unwrap();
        assert_eq!(again.expr().nodes, read.expr().nodes, "{name}");
    }
}

#[test]
fn mtef_size_records_have_the_specified_bytes() {
    let mtef = |mathml: &str| {
        let read = from_mathml(mathml).unwrap();
        read.to_mtef_bytes(&MtefWriteOptions::new(MtefVersion::V3))
            .unwrap()
    };
    // SIZE (9), explicit (101), 18 * 32 = 576 little endian, then the CHAR.
    let bytes = mtef(r#"<math><mi mathsize="18pt">x</mi></math>"#);
    assert!(
        bytes.windows(5).any(|w| w == [9, 101, 0x40, 0x02, 0x12]),
        "{bytes:02x?}"
    );
    // A script of 18 pt text in proportion: 58 % is not its size in MathML,
    // which draws scripts at 0.7; the reader sees a size in points.
    let bytes =
        mtef(r#"<math><mstyle mathsize="18pt"><msup><mi>x</mi><mn>2</mn></msup></mstyle></math>"#);
    assert_eq!(
        bytes.iter().filter(|&&b| b == 101).count(),
        2,
        "{bytes:02x?}"
    );
}

#[test]
fn a_full_size_script_keeps_its_size_in_mtef() {
    let mathml = r#"<math><msup><mi>x</mi><mstyle scriptlevel="0" data-mathema-size="full"><mn>2</mn></mstyle></msup></math>"#;
    let read = from_mathml(mathml).unwrap();
    let full = sized(logical(TypeSize::Full), MathNode::Number("2".into()));
    let expected = MathNode::Superscript {
        base: Box::new(id("x")),
        sup: Box::new(full),
    };
    assert_eq!(read.expr().nodes, [expected]);
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let bytes = read.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
        assert_eq!(
            from_mtef_bytes(&bytes).unwrap().expr().nodes,
            read.expr().nodes,
            "{version:?}"
        );
    }
}

#[test]
fn a_template_at_another_size_does_not_size_its_text() {
    // MTEF 3: SUBSYM (14) before a paren template (1) whose line is at full
    // size (10). No record names a script size.
    let bytes = [
        3, 1, 1, 3, 10, // header, FULL
        1, 14, 3, 1, 0, 0, // LINE, SUBSYM, TMPL 1
        1, 10, 2, 131, b'x', 0, 0, // LINE, FULL, CHAR x, END
        2, 150, b'(', 0, 2, 150, b')', 0, // the fences
        0, 0, 0,
    ];
    let read = from_mtef_bytes(&bytes).unwrap();
    // Only characters have sizes, and the character is at full size.
    let text = format!("{:?}", read.expr());
    assert!(!text.contains("Attributed"), "{text}");
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let bytes = read.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
        assert_eq!(
            from_mtef_bytes(&bytes).unwrap().expr().nodes,
            read.expr().nodes
        );
    }
}

fn omml(runs: &[(&str, Option<&str>)], script: Option<(&str, Option<&str>)>) -> String {
    let run = |(text, size): (&str, Option<&str>)| {
        let size = size.map_or(String::new(), |s| {
            format!(r#"<w:rPr><w:sz w:val="{s}"/></w:rPr>"#)
        });
        format!("<m:r>{size}<m:t>{text}</m:t></m:r>")
    };
    let mut body: String = runs.iter().map(|&r| run(r)).collect();
    if let Some(script) = script {
        body = format!(
            "<m:sSup><m:e>{body}</m:e><m:sup>{}</m:sup></m:sSup>",
            run(script)
        );
    }
    format!(
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{body}</m:oMath>"#
    )
}

#[test]
fn omml_run_sizes() {
    let read = |omml: &str| from_omml(omml).unwrap().expr();
    // Some runs have the size of the document, which is not known.
    let expr = read(&omml(&[("x", Some("36")), ("y", None)], None));
    assert_eq!(expr.nodes, [sized(points(18, 1), id("x")), id("y")]);
    assert_eq!(expr.meta, Default::default());
    // Every run the same size: ordinary text, scripts included.
    let expr = read(&omml(&[("x", Some("24"))], Some(("2", Some("24")))));
    assert!(!format!("{expr:?}").contains("Attributed"), "{expr:?}");
    // `w:sz` is the size outside of the script: Word draws it at 0.7.
    let expr = read(&omml(
        &[("x", Some("24")), ("y", Some("24"))],
        Some(("2", Some("40"))),
    ));
    let MathNode::Superscript { sup, .. } = &expr.nodes[0] else {
        panic!("{expr:?}");
    };
    assert_eq!(**sup, sized(points(14, 1), MathNode::Number("2".into())));
    assert_eq!(expr.meta.base_size_pt, Some(Rational::new(12, 1)));
    // A length instead of half points.
    let expr = read(&omml(&[("x", Some("18pt")), ("y", None)], None));
    assert_eq!(expr.nodes[0], sized(points(18, 1), id("x")));
}

#[test]
fn omml_sizes_are_written_for_word_and_reported_elsewhere() {
    for source in [
        omml(&[("x", Some("36")), ("y", None)], None),
        omml(
            &[("x", Some("24")), ("y", Some("24"))],
            Some(("2", Some("40"))),
        ),
    ] {
        let read = from_omml(&source).unwrap();
        let (written, diagnostics) = read.to_omml_with_options(&wml()).unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            from_omml(&written).unwrap().expr(),
            read.expr(),
            "{written}"
        );
        let (plain, diagnostics) = read.to_omml_with_options(&OmmlOptions::default()).unwrap();
        assert!(!plain.contains("w:sz"));
        assert!(
            diagnostics[0].message.contains("font sizes"),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn latex_gets_the_nearest_size_command_without_diagnostics() {
    let read = from_mathml(r#"<math><mi>a</mi><mi mathsize="14.4pt">x</mi></math>"#).unwrap();
    let (latex, diagnostics) = read.to_latex_with_diagnostics().unwrap();
    // 14.4 pt is 1.2 times 12 pt, which is what `\large` is.
    assert!(latex.contains(r"\large x"), "{latex}");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn unicode_math_reports_dropped_sizes_only_when_preserving() {
    let source = r#"<math><mi mathsize="18pt">x</mi></math>"#;
    for (policy, reports) in [
        (TypesettingPolicy::Semantic, 0),
        (TypesettingPolicy::Preserve, 1),
    ] {
        let config = Config::default().with_typesetting(policy);
        let read = from_mathml_with(source, &config).unwrap();
        let (text, diagnostics) = read.to_unicode_math_with_diagnostics().unwrap();
        assert_eq!(text, "x");
        assert_eq!(diagnostics.len(), reports, "{diagnostics:?}");
    }
}

#[test]
fn a_size_of_every_run_is_kept() {
    let source = omml(&[("x", Some("36"))], None);
    let read = from_omml(&source).unwrap();
    assert_eq!(read.expr().meta.base_size_pt, Some(Rational::new(18, 1)));
    let (written, _) = read.to_omml_with_options(&wml()).unwrap();
    assert!(written.contains(r#"<w:sz w:val="36"/>"#), "{written}");
    assert!(read.to_mathml().unwrap().contains(r#"mathsize="18pt""#));
    // A size that is a number but no size does not refuse the document.
    for value in ["0", "99999"] {
        let read = from_omml(&omml(&[("x", Some(value))], None)).unwrap();
        assert_eq!(read.diagnostics().len(), 1, "{value}");
    }
    for value in ["+24", "1e1pt"] {
        assert!(
            from_omml(&omml(&[("x", Some(value))], None)).is_err(),
            "{value}"
        );
    }
}

#[test]
fn omml_sizes_in_operator_names_and_linear_fractions() {
    let run = |text: &str, size: u32| {
        format!(
            r#"<m:r><m:rPr><m:sty m:val="p"/></m:rPr><w:rPr><w:sz w:val="{size}"/></w:rPr><m:t>{text}</m:t></m:r>"#
        )
    };
    let wrap = |body: String| {
        format!(
            r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{body}<m:r><m:t>u</m:t></m:r></m:oMath>"#
        )
    };
    let limit = wrap(format!(
        "<m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow>",
        run("lim", 40),
        run("n", 40)
    ));
    let fraction = wrap(format!(
        r#"<m:sSup><m:e><m:r><m:t>y</m:t></m:r></m:e><m:sup><m:f><m:fPr><m:type m:val="lin"/></m:fPr><m:num>{}</m:num><m:den>{}</m:den></m:f></m:sup></m:sSup>"#,
        run("a", 40),
        run("b", 40)
    ));
    for source in [limit, fraction] {
        let read = from_omml(&source).unwrap();
        let (written, _) = read.to_omml_with_options(&wml()).unwrap();
        assert_eq!(
            written.matches(r#"<w:sz w:val="40"/>"#).count(),
            2,
            "{written}"
        );
    }
}

#[test]
fn mtef_3_text_at_another_base_size_has_its_size() {
    let read = from_mathml(r#"<math mathsize="24pt"><mi>x</mi><mi mathsize="12pt">y</mi></math>"#)
        .unwrap();
    let bytes = read
        .to_mtef_bytes(&MtefWriteOptions::new(MtefVersion::V3))
        .unwrap();
    assert_eq!(
        from_mtef_bytes(&bytes).unwrap().expr().nodes,
        [sized(points(24, 1), id("x")), sized(points(12, 1), id("y"))]
    );
}

#[test]
fn an_operator_sign_has_its_own_size() {
    let source = r#"<math display="block"><munderover><mo mathsize="18pt">∑</mo><mi>i</mi><mi>n</mi></munderover><mover accent="true"><mi>x</mi><mo stretchy="false" mathsize="18pt">^</mo></mover></math>"#;
    let read = from_mathml(source).unwrap();
    assert!(read.diagnostics().is_empty(), "{:?}", read.diagnostics());
    let own = NodeAttrs::with_glyph_size(FontSize::Points(Rational::new(18, 1)));
    let signs = |nodes: &[MathNode]| {
        let found: Vec<_> = nodes
            .iter()
            .filter(|node| matches!(node, MathNode::Attributed { attrs, .. } if *attrs == own))
            .collect();
        found.len()
    };
    assert_eq!(signs(&read.expr().nodes), 2, "{:?}", read.expr());
    let again = from_mathml(&read.to_mathml().unwrap()).unwrap();
    assert_eq!(again.expr().nodes, read.expr().nodes);
    let (omml, _) = read.to_omml_with_options(&wml()).unwrap();
    assert_eq!(
        omml.matches(r#"<m:ctrlPr><w:rPr><w:sz w:val="36"/>"#)
            .count(),
        2,
        "{omml}"
    );
    assert_eq!(signs(&from_omml(&omml).unwrap().expr().nodes), 2);
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let bytes = read.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
        // The sign of a big operator is 1.5 times its text: 27 pt.
        assert!(
            bytes.windows(4).any(|w| w == [9, 101, 0x60, 0x03]),
            "{bytes:02x?}"
        );
        assert!(signs(&from_mtef_bytes(&bytes).unwrap().expr().nodes) >= 1);
    }
    // LaTeX has no size for a sign.
    let (latex, diagnostics) = read.to_latex_with_diagnostics().unwrap();
    assert!(!latex.contains("large"), "{latex}");
    assert!(
        diagnostics[0].message.contains("dropped"),
        "{diagnostics:?}"
    );
}

#[test]
fn a_middle_delimiter_ends_a_declaration() {
    let read = from_mathml(r#"<math display="block"><mrow><mo fence="true">(</mo><mstyle scriptlevel="1" data-mathema-size="sub"><mi>a</mi></mstyle><mo stretchy="true" fence="true">|</mo><mstyle scriptlevel="1" data-mathema-size="sub"><mi>b</mi></mstyle><mo fence="true">)</mo></mrow></math>"#).unwrap();
    let latex = read.to_latex().unwrap();
    // TeX resets the style at `\middle`: `b` needs its own.
    let (_, after) = latex.split_once(r"\middle").unwrap();
    assert!(after.contains(r"\scriptstyle b"), "{latex}");
}

/// KaTeX draws a size command in a script at that size whatever the script
/// is, and TeX has none there; a style switch means the same everywhere.
#[test]
fn latex_scripts_have_style_switches_only() {
    let latex = |size: &str| {
        let source = format!(r#"<math><msup><mi>x</mi><mi mathsize="{size}">y</mi></msup></math>"#);
        let read = from_mathml(&source).unwrap();
        let (latex, diagnostics) = read.to_latex_with_diagnostics().unwrap();
        (latex, diagnostics.len())
    };
    assert_eq!(latex("12pt"), (r"x^{\textstyle y}".to_owned(), 0));
    assert_eq!(latex("14.4pt"), ("x^y".to_owned(), 1));
    // Too far from every size command.
    let read = from_mathml(r#"<math><mi mathsize="60pt">x</mi></math>"#).unwrap();
    let (text, diagnostics) = read.to_latex_with_diagnostics().unwrap();
    assert_eq!((text.as_str(), diagnostics.len()), ("x", 1));
}

#[test]
fn a_sized_operator_name_keeps_its_limits() {
    let read = from_mathml(r#"<math display="block"><munder><mo mathsize="18pt">max</mo><mi>x</mi></munder><mi>b</mi></math>"#).unwrap();
    let (latex, diagnostics) = read.to_latex_with_diagnostics().unwrap();
    assert!(
        latex.contains(r"\max") && !latex.contains("mathop") && !latex.contains("nolimits"),
        "{latex}"
    );
    assert_eq!(diagnostics.len(), 1);
}

#[test]
fn sized_primes_and_root_indices_survive_mtef() {
    // A prime with its base's size, and one with a size of its own.
    let same = r#"<math><msup mathsize="18pt"><mi>x</mi><mo>′</mo></msup></math>"#;
    let own = r#"<math><msup><mi>x</mi><mo mathsize="12pt">′</mo></msup></math>"#;
    let root =
        r#"<math><mstyle mathsize="18pt"><mroot><mi>x</mi><mn>3</mn></mroot></mstyle></math>"#;
    for source in [same, own, root] {
        let read = from_mathml(source).unwrap();
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let options = MtefWriteOptions::new(version);
            let one = from_mtef_bytes(&read.to_mtef_bytes(&options).unwrap()).unwrap();
            let text = format!("{:?}", one.expr().nodes);
            if source == root {
                // The index of 18 pt text is at 9 pt: 0.5 as the AST draws it.
                assert!(
                    text.contains("Points(Rational { num: 9, den: 1 })"),
                    "{text}"
                );
            } else {
                assert!(text.contains("Attributed"), "{text}");
            }
            let bytes = one.to_mtef_bytes(&options).unwrap();
            assert_eq!(
                from_mtef_bytes(&bytes).unwrap().expr().nodes,
                one.expr().nodes
            );
        }
    }
}

/// MTEF 5 preferences in which the first user size is 24 pt, not 75 %.
#[test]
fn mtef_5_size_preferences_are_the_documents() {
    let read = from_mathml(
        r#"<math mathsize="12pt"><mi mathsize="9pt" data-mathema-size="user1">x</mi></math>"#,
    )
    .unwrap();
    assert_eq!(
        read.expr().nodes,
        [sized(logical(TypeSize::User1), id("x"))]
    );
    let bytes = read
        .to_mtef_bytes(&MtefWriteOptions::new(MtefVersion::V5))
        .unwrap();
    // "75 %" in nibbles, shifted by one as it stands: ..4 7 5 F.. .
    let nibbles: Vec<u8> = bytes.iter().flat_map(|b| [b >> 4, b & 15]).collect();
    let at = nibbles.windows(4).position(|w| w == [4, 7, 5, 15]).unwrap();
    let mut edited = nibbles.clone();
    edited[at..at + 4].copy_from_slice(&[2, 2, 4, 15]);
    let edited: Vec<u8> = edited.chunks(2).map(|n| n[0] << 4 | n[1]).collect();
    assert_eq!(
        from_mtef_bytes(&edited).unwrap().expr().nodes,
        [sized(points(24, 1), id("x"))]
    );
}

/// MTEF 3: SUMOP (43) with no limits drawn, and `symbol` at the symbol size in
/// its operator slot.
fn operator_slot(symbol: u8) -> Vec<u8> {
    vec![
        3,
        1,
        1,
        3,
        10,
        1, // header, FULL, LINE
        3,
        43,
        1,
        0, // TMPL SUMOP, lower limit
        1,
        0, // body
        11,
        1,
        2,
        131,
        b'i',
        0,
        0,    // SUB, LINE, CHAR i
        0x11, // no upper limit
        13,
        1,
        2,
        134,
        symbol,
        if symbol == 0x11 { 0x22 } else { 0 },
        0, // SYM, LINE, CHAR
        0,
        0,
        0,
    ]
}

#[test]
fn symbol_sizes_are_dropped_and_reported() {
    let read = from_mtef_bytes(&operator_slot(b'M')).unwrap();
    assert!(!read.compact_expr().arena.has_attrs(), "{:?}", read.expr());
    let found: Vec<_> = read
        .diagnostics()
        .iter()
        .filter(|d| d.message.contains("symbol sizes"))
        .collect();
    assert_eq!(found.len(), 1, "{:?}", read.diagnostics());
    assert_eq!(found[0].kind, mathema::DiagnosticKind::Info);
    // Anywhere, and in writing: SYM and SUBSYM records before a letter.
    for record in [13, 14] {
        let bytes = [3, 1, 1, 3, 10, 1, record, 2, 131, b'M', 0, 0, 0];
        let read = from_mtef_bytes(&bytes).unwrap();
        assert_eq!(read.expr().nodes, [id("M")]);
        assert_eq!(read.diagnostics().len(), 1, "{:?}", read.diagnostics());
    }
    let source = r#"<math><mi mathsize="18pt" data-mathema-size="sym">x</mi></math>"#;
    let read = from_mathml(source).unwrap();
    assert_eq!(read.expr().nodes, [sized(logical(TypeSize::Sym), id("x"))]);
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let (bytes, diagnostics) = read
            .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(version))
            .unwrap();
        assert_eq!(from_mtef_bytes(&bytes).unwrap().expr().nodes, [id("x")]);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("symbol sizes")),
            "{diagnostics:?}"
        );
    }
    // The sign of a big operator (U+2211) is there at that size by rule.
    let read = from_mtef_bytes(&operator_slot(0x11)).unwrap();
    assert!(
        format!("{:?}", read.expr()).contains("Nary"),
        "{:?}",
        read.expr()
    );
    assert!(read.diagnostics().is_empty(), "{:?}", read.diagnostics());
}

#[test]
fn nested_sizes_settle_after_one_rewrite() {
    let dir = format!("{}/tests/fixtures/mtef-sizes", env!("CARGO_MANIFEST_DIR"));
    let config = Config::default().with_strictness(mathema::Strictness::Lenient);
    let mut tried = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "bin") {
            continue;
        }
        let read = mathema::from_mtef_bytes_with(&std::fs::read(&path).unwrap(), &config).unwrap();
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let options = MtefWriteOptions::new(version);
            let one = from_mtef_bytes(&read.to_mtef_bytes(&options).unwrap()).unwrap();
            let bytes = one.to_mtef_bytes(&options).unwrap();
            let two = from_mtef_bytes(&bytes).unwrap();
            assert_eq!(
                two.to_mtef_bytes(&options).unwrap(),
                bytes,
                "{path:?} {version:?}"
            );
        }
        tried += 1;
    }
    assert_eq!(tried, 10);
}

#[test]
fn replacing_a_node_drops_its_attributes() {
    let read = from_mathml(r#"<math><mi mathsize="18pt">x</mi></math>"#).unwrap();
    let mut expr = read.compact_expr().clone();
    let root = expr.arena.children(expr.roots)[0];
    assert!(expr.arena.attrs(root).is_some());
    *expr.arena.node_mut(root) =
        mathema::ast::compact::CompactNode::Row(mathema::ast::compact::NodeSpan::EMPTY);
    assert!(expr.arena.attrs(root).is_none());
    assert!(!expr.arena.has_attrs());
}

#[test]
fn input_without_sizes_has_no_attributes() {
    let read = from_latex(r"\frac{x^2}{\sqrt{y_i}}+\large z").unwrap();
    assert!(!read.compact_expr().arena.has_attrs());
    assert_eq!(read.expr().meta, Default::default());
}

#[cfg(feature = "render")]
mod drawn {
    use mathema::render::{RenderOptions, render_to_svg, render_to_svg_compact};
    use mathema::{ConvertResult, from_mathml, from_mtef_bytes};

    /// The sizes of the glyphs of an equation, as multiples of the base size.
    fn sizes(read: &ConvertResult, wanted: char) -> Vec<f64> {
        let options = RenderOptions::default();
        let of = |svg: String| -> Vec<f64> {
            svg.split("<text ")
                .skip(1)
                .filter(|text| {
                    let content = text.split('>').nth(1).unwrap().split('<').next().unwrap();
                    content.contains(wanted)
                })
                .map(|text| {
                    let size = text.split("font-size=\"").nth(1).unwrap();
                    size.split('"').next().unwrap().parse::<f64>().unwrap() / 16.0
                })
                .collect()
        };
        let compact = of(render_to_svg_compact(read.compact_expr(), &options).unwrap());
        assert_eq!(compact, of(render_to_svg(&read.expr(), &options).unwrap()));
        assert!(!compact.is_empty(), "{wanted} is not drawn");
        compact
    }

    fn near(found: &[f64], expected: &[f64]) {
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for (found, expected) in found.iter().zip(expected) {
            assert!((found - expected).abs() < 0.01, "{found} for {expected}");
        }
    }

    #[test]
    fn mathml_sizes() {
        // Base 12 pt: 18 pt is 1.5, and overrides the script level.
        let read =
            from_mathml(r#"<math><mn>1</mn><mn mathsize="18pt">2</mn><msup><mn>3</mn><mn mathsize="18pt">4</mn></msup></math>"#)
                .unwrap();
        near(&sizes(&read, '2'), &[1.5]);
        near(&sizes(&read, '4'), &[1.5]);
        near(&sizes(&read, '3'), &[1.0]);
        // Scripts inside a sized element shrink from its size.
        let read = from_mathml(
            r#"<math><mstyle mathsize="18pt"><msup><mn>3</mn><mn>4</mn></msup></mstyle></math>"#,
        )
        .unwrap();
        near(&sizes(&read, '4'), &[1.05]);
    }

    #[test]
    fn signs() {
        let source = |size: &str| {
            format!(
                r#"<math display="block"><munderover><mo{size}>∑</mo><mn>1</mn><mn>2</mn></munderover><mn>3</mn></math>"#
            )
        };
        let plain = from_mathml(&source("")).unwrap();
        let large = from_mathml(&source(r#" mathsize="18pt""#)).unwrap();
        let sum = |read: &ConvertResult| sizes(read, '∑')[0];
        near(&[sum(&large)], &[1.5 * sum(&plain)]);
        // Limits and body keep theirs.
        near(&sizes(&large, '1'), &[0.7]);
        near(&sizes(&large, '3'), &[1.0]);
    }

    #[test]
    fn mtef_sizes() {
        // The picture draws the digits at 0.58 of the text; the AST's
        // subscript size is TeX's 0.7.
        let read = from_mtef_bytes(&super::fixture("govdocs-v3-size-typesize")).unwrap();
        near(&sizes(&read, '1'), &[0.7, 0.7]);
    }
}

/// Colors that LaTeX cannot hold go; the sizes around and inside them stay.
mod dropped_colors {
    use super::*;
    use mathema::reader::mathml::MathmlReader;
    use mathema::writer::Writer;
    use mathema::writer::latex::LatexWriter;
    use mathema::{ConvertResult, Format, convert};

    fn color(content: MathNode) -> MathNode {
        MathNode::Color {
            color: "hsl(1,2%,3%)".into(),
            content: Box::new(content),
        }
    }

    fn at(points_: i32, content: MathNode) -> MathNode {
        sized(points(points_, 1), content)
    }

    fn direct(nodes: Vec<MathNode>) -> String {
        LatexWriter::write(&mathema::MathExpr::block(nodes), &Config::default()).unwrap()
    }

    #[test]
    fn a_size_around_or_inside_one_is_written() {
        // 18 pt is nearest to \Large..
        for nodes in [
            vec![id("a"), at(18, color(id("x"))), id("b")],
            vec![id("a"), color(at(18, id("x"))), id("b")],
            // The inner size wins, whatever is between.
            vec![id("a"), at(30, color(color(at(18, id("x"))))), id("b")],
            vec![id("a"), color(at(18, color(id("x")))), id("b")],
            vec![
                id("a"),
                at(18, color(MathNode::Row(vec![id("x"), color(id("y"))]))),
            ],
            vec![at(18, color(id("x")))],
            // Sizes on two layers of color: the inner one.
            vec![id("a"), at(30, color(at(18, color(id("x"))))), id("b")],
            // Nodes that are only beside one keep theirs.
            vec![
                at(
                    18,
                    MathNode::Styled(mathema::ast::TextStyle::Bold, vec![id("x")]),
                ),
                color(id("y")),
            ],
        ] {
            let latex = direct(nodes.clone());
            assert!(latex.contains(r"\Large"), "{latex}: {nodes:?}");
            assert!(
                !latex.contains(r"\huge") && !latex.contains(r"\Huge"),
                "{latex}"
            );
            assert!(!latex.contains("hsl"), "{latex}");
            // It parses, and back to the same LaTeX.
            let again = from_latex(&latex).unwrap().to_latex().unwrap();
            assert_eq!(from_latex(&again).unwrap().to_latex().unwrap(), again);
        }
    }

    #[test]
    fn every_path_writes_it() {
        let mathml = r#"<math><mi>a</mi><mstyle mathsize="18pt" mathcolor="hsl(1,2%,3%)"><mi>x</mi></mstyle><mi>b</mi></math>"#;
        let read: ConvertResult = from_mathml(mathml).unwrap();
        let has = |latex: &str| {
            assert!(
                latex.contains(r"\Large") && !latex.contains("hsl"),
                "{latex}"
            )
        };
        has(&read.to_latex().unwrap());
        has(&convert(mathml, Format::MathML, Format::LaTeX).unwrap());
        has(&LatexWriter::write_compact(read.compact_expr(), &Config::default()).unwrap());
        let expr = <MathmlReader as mathema::reader::Reader>::parse(mathml, &Config::default())
            .unwrap()
            .expr;
        has(&LatexWriter::write(&expr, &Config::default()).unwrap());
        // Other formats keep both.
        let again = from_mathml(&read.to_mathml().unwrap()).unwrap();
        assert_eq!(again.expr().nodes, read.expr().nodes);
    }

    #[test]
    fn dropped_sizes_and_colors_are_both_reported() {
        let mathml = r#"<math><mi mathsize="60pt">a</mi><mstyle mathcolor="hsl(1,2%,3%)"><mi>x</mi></mstyle></math>"#;
        let (_, diagnostics) = from_mathml(mathml)
            .unwrap()
            .to_latex_with_diagnostics()
            .unwrap();
        let kinds: Vec<_> = diagnostics.iter().map(|d| d.kind).collect();
        assert!(
            kinds.contains(&mathema::DiagnosticKind::Info),
            "{diagnostics:?}"
        );
        assert!(
            kinds.contains(&mathema::DiagnosticKind::Warning),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn a_sized_operator_in_one_keeps_its_limits() {
        let max = from_latex(r"\max").unwrap().expr().nodes.remove(0);
        let latex = direct(vec![MathNode::Subscript {
            base: Box::new(at(18, color(max))),
            sub: Box::new(id("x")),
        }]);
        // Under a script, the group of an operator is an ordinary atom, sized
        // or not.
        assert!(latex.contains(r"\max"), "{latex}");
        assert!(!latex.contains(r"\nolimits"), "{latex}");
    }
}
