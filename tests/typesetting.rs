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
    assert_eq!(tried, 13);
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

/// Sizes that a review found lost, moved or costly.
mod review {
    use super::*;
    use mathema::ast::ExprMeta;
    use mathema::ast::compact::CompactExpr;
    use mathema::ast::{AccentPosition, TextStyle};
    use mathema::writer::Writer;
    use mathema::writer::mathml::MathmlWriter;

    fn through_mtef(nodes: &[MathNode], version: MtefVersion) -> (Vec<MathNode>, Vec<String>) {
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes.to_vec())).unwrap();
        let (bytes, diagnostics) = mathema::writer::mtef::MtefWriter::write_bytes_with_diagnostics(
            &expr,
            &Config::default(),
            &MtefWriteOptions::new(version),
        )
        .unwrap();
        (
            from_mtef_bytes(&bytes).unwrap().expr().nodes,
            diagnostics.into_iter().map(|d| d.message).collect(),
        )
    }

    #[test]
    fn a_sized_styled_character_under_an_accent() {
        let bold = |size: i32| {
            sized(
                points(size, 1),
                MathNode::Styled(TextStyle::Bold, vec![id("K")]),
            )
        };
        let hat = |base: MathNode, mark: Option<i32>| {
            let accent = MathNode::Accent {
                base: Box::new(base),
                accent: '\u{0302}',
                position: AccentPosition::Above,
                wide: false,
            };
            match mark {
                Some(size) => sized(
                    NodeAttrs::with_glyph_size(FontSize::Points(Rational::new(size, 1))),
                    accent,
                ),
                None => accent,
            }
        };
        for version in [MtefVersion::V3, MtefVersion::V5] {
            // Mark and letter at one size: an embellishment, exactly.
            let node = hat(bold(24), Some(24));
            let (back, diagnostics) = through_mtef(std::slice::from_ref(&node), version);
            assert_eq!(back, [node], "{version:?}");
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            // The mark at the size of the text around it: exactly over a bare
            // letter, at the letter's size and reported over a styled one.
            let bare = hat(sized(points(24, 1), id("K")), None);
            let (back, diagnostics) = through_mtef(std::slice::from_ref(&bare), version);
            assert_eq!(back, [bare], "{version:?}");
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let (back, diagnostics) = through_mtef(&[hat(bold(24), None)], version);
            assert_eq!(back, [hat(bold(24), Some(24))]);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            // An outer size does not reach a letter with one of its own.
            let (back, _) = through_mtef(&[sized(points(30, 1), hat(bold(9), None))], version);
            let text = format!("{back:?}");
            assert!(
                text.contains("num: 9,") && text.contains("Styled"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_sized_pile() {
        let pile = from_latex(r"\begin{array}{c}a\\b\end{array}")
            .unwrap()
            .expr()
            .nodes
            .remove(0);
        let frac = MathNode::Fraction {
            kind: mathema::ast::FractionKind::Normal,
            num: Box::new(sized(points(18, 1), pile.clone())),
            den: Box::new(id("d")),
        };
        for nodes in [vec![sized(points(18, 1), pile)], vec![frac]] {
            for version in [MtefVersion::V3, MtefVersion::V5] {
                let text = format!("{:?}", through_mtef(&nodes, version).0);
                assert_eq!(text.matches("num: 18").count(), 2, "{version:?} {text}");
            }
        }
    }

    /// SYM (13) before text, digits, a function name and a prime.
    #[test]
    fn every_dropped_symbol_size_is_reported() {
        let runs: [&[u8]; 4] = [
            &[13, 2, 129, b'a', 0, 10, 2, 129, b'b', 0],
            &[10, 2, 136, b'1', 0, 13, 2, 136, b'2', 0],
            &[10, 2, 130, b's', 0, 13, 2, 130, b'i', 0],
            &[10, 2, 131, b'x', 0, 13, 2, 130, b'\'', 0],
        ];
        for run in runs {
            let bytes = [&[3, 1, 1, 3, 10, 1][..], run, &[0, 0]].concat();
            let read = from_mtef_bytes(&bytes).unwrap();
            let reports = read
                .diagnostics()
                .iter()
                .filter(|d| d.message.contains("symbol sizes"))
                .count();
            assert_eq!(reports, 1, "{run:?}: {:?}", read.diagnostics());
        }
    }

    #[test]
    fn a_known_base_size_is_written_without_other_sizes() {
        let mut expr = mathema::MathExpr::block(vec![id("x")]);
        expr.meta = ExprMeta::default();
        expr.meta.base_size_pt = Some(Rational::new(18, 1));
        let config = Config::default();
        let heap = MathmlWriter::write(&expr, &config).unwrap();
        assert!(heap.contains(r#"mathsize="18pt""#), "{heap}");
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        assert_eq!(
            MathmlWriter::write_compact(&compact, &config).unwrap(),
            heap
        );
        // MTEF 5: 18 pt in the preferences, nibbles 2 1 8 F.
        let bytes = mathema::writer::mtef::MtefWriter::write_bytes(
            &compact,
            &config,
            &MtefWriteOptions::new(MtefVersion::V5),
        )
        .unwrap();
        assert!(
            bytes.windows(3).any(|w| w == [8, 0x21, 0x8f]),
            "{bytes:02x?}"
        );
    }

    #[test]
    fn sizes_that_are_none_are_ignored() {
        for size in [
            Rational { num: 1, den: 0 },
            Rational { num: 0, den: 0 },
            Rational { num: 0, den: 1 },
            Rational { num: -5, den: 1 },
        ] {
            let attrs = NodeAttrs::with_size(FontSize::Points(size));
            let mut expr = mathema::MathExpr::block(vec![sized(attrs, id("x"))]);
            expr.meta.base_size_pt = Some(size);
            let mathml = MathmlWriter::write(&expr, &Config::default()).unwrap();
            assert!(!mathml.contains("mathsize"), "{mathml}");
        }
        let delta = NodeAttrs::with_size(FontSize::Logical {
            level: TypeSize::Full,
            delta_pt: Rational { num: 1, den: 0 },
        });
        let expr = mathema::MathExpr::block(vec![sized(delta, id("x"))]);
        let mathml = MathmlWriter::write(&expr, &Config::default()).unwrap();
        assert!(
            !mathml.contains("NaN") && !mathml.contains("inf"),
            "{mathml}"
        );
    }

    /// A style moves the limits of a sum to its side and shrinks its sign.
    #[test]
    fn a_typesize_on_a_structure_is_no_style() {
        let sum = from_latex(r"\sum_{i}^{n}").unwrap().expr().nodes.remove(0);
        let nodes = vec![sized(logical(TypeSize::Sub), sum)];
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
        let (latex, diagnostics) =
            <mathema::writer::latex::LatexWriter as Writer>::write_compact_with_diagnostics(
                &expr,
                &Config::default(),
            )
            .unwrap();
        assert!(!latex.contains("style"), "{latex}");
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn typesizes_in_root_indices_and_around_scripts_keep_their_size() {
        // An index is at half size in the AST: `Sub` is larger.
        let root = MathNode::Root {
            index: Box::new(sized(logical(TypeSize::Sub), MathNode::Number("3".into()))),
            radicand: Box::new(id("x")),
        };
        // 0.7 of 12 pt, in 32nds of a point.
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let text = format!("{:?}", through_mtef(std::slice::from_ref(&root), version).0);
            assert!(
                text.contains("Points(Rational { num: 269, den: 32 })"),
                "{text}"
            );
            // Scripts of a structure at `Sub2` shrink from it: 0.5 and 0.35.
            let script = MathNode::Superscript {
                base: Box::new(id("x")),
                sup: Box::new(MathNode::Number("2".into())),
            };
            let nodes = [sized(logical(TypeSize::Sub2), script)];
            let text = format!("{:?}", through_mtef(&nodes, version).0);
            assert!(
                text.contains("num: 6, den: 1") && text.contains("num: 67, den: 16"),
                "{text}"
            );
        }
    }

    #[test]
    fn sizes_on_table_rows_and_cells_are_reported() {
        for table in [
            r#"<mtable><mtr mathsize="18pt"><mtd><mi>x</mi></mtd></mtr></mtable>"#,
            r#"<mtable><mtr><mtd mathsize="18pt"><mi>x</mi></mtd></mtr></mtable>"#,
        ] {
            let read = from_mathml(&format!("<math>{table}</math>")).unwrap();
            assert_eq!(read.diagnostics().len(), 1, "{:?}", read.diagnostics());
        }
    }

    /// Doubling the input must not quadruple the work: each of these took
    /// seconds or gigabytes at a fraction of this size.
    #[test]
    fn large_inputs_cost_in_proportion() {
        let text: Vec<u8> = (0..40_000u32)
            .flat_map(|i| [9, 101, 64 + 10 * (i % 2) as u8, 1, 2, 129, b'a', 0])
            .collect();
        let bytes = [&[3, 1, 1, 3, 10, 1][..], &text, &[0, 0]].concat();
        let read = from_mtef_bytes(&bytes).unwrap();
        let latex = read.to_latex().unwrap();
        assert!(latex.len() < 1_000_000, "{}", latex.len());
        // Separators between the sizes must not keep it from being written.
        read.to_mtef_bytes(&MtefWriteOptions::new(MtefVersion::V5))
            .unwrap();
        // 1 MB of digits in the preferences, 20,000 sized characters.
        let mut nibbles = vec![2, 1, 2, 15, 4];
        nibbles.extend(std::iter::repeat_n(1, 2_000_000));
        nibbles.extend([15, 4, 4, 2, 15, 4, 1, 5, 0, 15, 4, 1, 0, 0, 15, 4, 7, 6, 15]);
        nibbles.extend([4, 1, 5, 1, 15, 2, 1, 15]);
        nibbles.resize(nibbles.len().next_multiple_of(2), 0);
        let mut bytes = b"\x05\x01\x00\x06\x00DSMT6\x00\x00\x12\x00\x08".to_vec();
        bytes.extend(nibbles.chunks(2).map(|n: &[u8]| n[0] << 4 | n[1]));
        bytes.extend([0, 0, 10, 1, 0, 9, 5, 128]);
        for _ in 0..20_000 {
            bytes.extend([2, 0, 0x83, b'x', 0]);
        }
        bytes.extend([0, 0]);
        let start = std::time::Instant::now();
        let read = from_mtef_bytes(&bytes).unwrap();
        assert!(read.compact_expr().arena.has_attrs());
        // Seconds in a debug build; two minutes when it was quadratic.
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
    }

    /// Each of these was quadratic: seconds or minutes at this size.
    #[test]
    fn runs_cost_in_proportion() {
        let lines = |line: &[u8]| {
            // Lines of a fraction: one list holds no more than 65,535 items.
            let mut bytes = vec![3, 1, 1, 3, 10, 1];
            for _ in 0..4 {
                bytes.extend([3, 14, 0, 0, 1]);
                bytes.extend(line);
                bytes.extend([0, 1]);
                bytes.extend(line);
                bytes.extend([0, 0]);
            }
            bytes.extend([0, 0]);
            bytes
        };
        let blanks = [2, 129, b' ', 0].repeat(60_000);
        let start = std::time::Instant::now();
        from_mtef_bytes(&lines(&blanks)).unwrap();
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
        // Sizes that change within a function name.
        let time = |step: u32| {
            let letters: Vec<u8> = (0..60_000u32)
                .flat_map(|i| [9, 101, 64 + (i % 2 * step) as u8, 1, 2, 130, b'q', 0])
                .collect();
            let bytes = lines(&letters);
            let start = std::time::Instant::now();
            from_mtef_bytes(&bytes).unwrap();
            start.elapsed()
        };
        let (same, changing) = (time(0), time(10));
        assert!(changing < same * 5, "{same:?} {changing:?}");
        // A size around a list that has one already costs little more than
        // the list.
        let list = format!("<mrow>{}</mrow>", "<mi>x</mi>".repeat(60_000)).repeat(6);
        let time = |depth: usize| {
            let open: String = (0..depth)
                .map(|i| format!(r#"<mstyle mathsize="{}pt"><mi>y</mi>"#, 10 + i))
                .collect();
            let mathml = format!("<math>{open}{list}{}</math>", "</mstyle>".repeat(depth));
            let start = std::time::Instant::now();
            let read = from_mathml(&mathml).unwrap();
            assert!(read.compact_expr().arena.has_attrs());
            start.elapsed()
        };
        let (one, hundred) = (time(1), time(100));
        assert!(hundred < one * 5, "{one:?} {hundred:?}");
    }

    /// Every loss is reported once.
    #[test]
    fn a_dropped_size_is_reported_once() {
        let fence = [3, 1, 0, 0, 1, 2, 131, b'x', 0, 0, 9, 101, 128, 2];
        let fence = [&fence[..], &[2, 150, b'(', 0, 2, 150, b')', 0, 0, 10]].concat();
        let bytes = [&[3, 1, 1, 3, 10, 1][..], &fence.repeat(50), &[0, 0]].concat();
        let read = from_mtef_bytes(&bytes).unwrap();
        let dropped = read
            .diagnostics()
            .iter()
            .filter(|d| d.message.contains("delimiters"))
            .count();
        assert_eq!(dropped, 1, "{:?}", read.diagnostics());
    }

    /// A size in a cell, by itself and in a fraction, wherever the table is.
    #[test]
    fn omml_sizes_in_cells() {
        let x = || sized(points(20, 1), id("x"));
        let fraction = |num| MathNode::Fraction {
            kind: mathema::ast::FractionKind::Normal,
            num: Box::new(num),
            den: Box::new(id("y")),
        };
        for content in [x(), fraction(x()), fraction(fraction(x()))] {
            let table = |content| MathNode::Array {
                columns: vec![mathema::ast::ColumnAlign::Center],
                rows: vec![vec![vec![content]]],
                arraystretch: None,
                add_jot: false,
            };
            let table = table(content);
            for node in [
                table.clone(),
                MathNode::Subscript {
                    base: Box::new(id("z")),
                    sub: Box::new(table.clone()),
                },
                MathNode::Superscript {
                    base: Box::new(id("z")),
                    sup: Box::new(table.clone()),
                },
                fraction(table),
            ] {
                let nodes = vec![node, id("w")];
                let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
                let write = |expr: &CompactExpr| {
                    mathema::writer::omml::OmmlWriter::write_with_options(
                        expr,
                        &Config::default(),
                        &wml(),
                    )
                    .unwrap()
                    .0
                };
                let written = write(&expr);
                let read = from_omml(&written).unwrap();
                // A half point is 0.7 or 0.5 of one in a script.
                let text = format!("{:?}", read.expr());
                assert!(text.contains("Rational { num: "), "{written} {text}");
                let size = text
                    .split("Rational { num: ")
                    .nth(1)
                    .unwrap()
                    .split(" }")
                    .next()
                    .unwrap();
                let (num, den) = size.split_once(", den: ").unwrap();
                let size = num.parse::<f64>().unwrap() / den.parse::<f64>().unwrap();
                assert!((size - 20.0).abs() <= 0.25, "{size} {written}");
                assert_eq!(write(read.compact_expr()), written);
            }
        }
    }

    /// What is not written does not split runs where it is dropped first, and
    /// a writer that is given a split tree writes that.
    #[test]
    fn unwritten_sizes_do_not_split_runs() {
        use mathema::{Format, convert};
        let runs = [
            ("T", None),
            ("H", Some("17")),
            ("1", None),
            ("2", Some("17")),
        ];
        let source = omml(&runs, None);
        let plain = omml(&runs.map(|(text, _)| (text, None)), None);
        for format in [Format::Omml, Format::UnicodeMath, Format::LaTeX] {
            let written = convert(&source, Format::Omml, format).unwrap();
            if format != Format::LaTeX {
                assert_eq!(written, convert(&plain, Format::Omml, format).unwrap());
            }
        }
        let written = convert(&source, Format::Omml, Format::Omml).unwrap();
        assert!(
            written.contains(">TH<") && written.contains(">12<"),
            "{written}"
        );
        assert_eq!(
            convert(&written, Format::Omml, Format::Omml).unwrap(),
            written
        );
        let read = from_omml(&source).unwrap();
        assert_eq!(
            read.to_omml().unwrap(),
            from_omml(&plain).unwrap().to_omml().unwrap()
        );
    }

    /// The letters of MTEF are variables of their own whatever their sizes.
    #[test]
    fn letters_of_several_sizes_stay_variables() {
        let letters = [
            9, 101, 44, 1, 2, 131, b'Q', 0, 9, 101, 64, 1, 2, 131, b'R', 0,
        ];
        let bytes = [&[3, 1, 1, 3, 10, 1][..], &letters, &[0, 0]].concat();
        let plain = [3, 1, 1, 3, 10, 1, 2, 131, b'Q', 0, 2, 131, b'R', 0, 0, 0];
        let (read, plain) = (
            from_mtef_bytes(&bytes).unwrap(),
            from_mtef_bytes(&plain).unwrap(),
        );
        assert!(read.compact_expr().arena.has_attrs());
        assert_eq!(read.to_omml().unwrap(), plain.to_omml().unwrap());
        assert_eq!(
            read.to_unicode_math().unwrap(),
            plain.to_unicode_math().unwrap()
        );
        let omml = read.to_omml().unwrap();
        let back = from_omml(&omml).unwrap().expr().nodes;
        assert_eq!(back, [id("Q"), id("R")]);
    }

    /// The expression without its sizes.
    fn strip(expr: &CompactExpr) -> CompactExpr {
        use mathema::ast::visit::{MutVisitor, walk_node_mut};
        struct Strip;
        impl MutVisitor for Strip {
            fn visit_node_mut(&mut self, node: &mut MathNode) {
                while let MathNode::Attributed { content, .. } = node {
                    *node = (**content).clone();
                }
                walk_node_mut(self, node);
            }
        }
        let mut heap = expr.to_math_expr();
        Strip.visit_expr_mut(&mut heap);
        heap.meta = ExprMeta::default();
        let plain = CompactExpr::from_math_expr(&heap).unwrap();
        assert!(!plain.arena.has_attrs());
        plain
    }

    /// For a format that writes no sizes: the output is that of the expression
    /// without them, stripped before it is normalized.
    #[track_caller]
    fn written_as_if_unsized(sized: &CompactExpr) {
        use mathema::writer::{omml::OmmlWriter, unicode_math::UnicodeMathWriter};
        let config = Config::default();
        let plain = strip(sized);
        for (with, without) in [
            (
                <OmmlWriter as Writer>::write_compact(sized, &config).unwrap(),
                <OmmlWriter as Writer>::write_compact(&plain, &config).unwrap(),
            ),
            (
                <UnicodeMathWriter as Writer>::write_compact(sized, &config).unwrap(),
                <UnicodeMathWriter as Writer>::write_compact(&plain, &config).unwrap(),
            ),
        ] {
            assert_eq!(with, without);
        }
    }

    /// The same through [`mathema::convert`], from each format that has sizes.
    #[test]
    fn sizes_that_are_not_written_change_nothing() {
        use mathema::reader::{Reader, mathml::MathmlReader, mtef::MtefReader, omml::OmmlReader};
        use mathema::{Format, convert};
        let config = Config::default();
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let mut inputs = Vec::new();
        for dir in ["mtef", "mtef-sizes"] {
            let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
            for entry in std::fs::read_dir(format!("{root}/{dir}")).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "bin")
                    && let Ok(read) = from_mtef_bytes(&std::fs::read(&path).unwrap())
                    && read.compact_expr().arena.has_attrs()
                {
                    written_as_if_unsized(read.compact_expr());
                    inputs.push((Format::Mtef, hex(&std::fs::read(&path).unwrap())));
                    inputs.push((Format::MathML, read.to_mathml().unwrap()));
                    inputs.push((Format::Omml, read.to_omml_with_options(&wml()).unwrap().0));
                }
            }
        }
        for (from, input) in inputs {
            let parsed = match from {
                Format::Mtef => MtefReader::parse_compact(&input, &config),
                Format::MathML => MathmlReader::parse_compact(&input, &config),
                _ => OmmlReader::parse_compact(&input, &config),
            };
            let mut plain = strip(&parsed.unwrap().expr);
            use mathema::writer::{omml::OmmlWriter, unicode_math::UnicodeMathWriter};
            let expected = if from == Format::Mtef {
                // The letters of MTEF are not joined.
                let read = from_mtef_bytes(&hex_bytes(&input)).unwrap();
                (read.to_omml().unwrap(), read.to_unicode_math().unwrap())
            } else {
                mathema::ast::normalize::normalize_compact_expr(&mut plain).unwrap();
                (
                    <OmmlWriter as Writer>::write_compact(&plain, &config).unwrap(),
                    <UnicodeMathWriter as Writer>::write_compact(&plain, &config).unwrap(),
                )
            };
            assert_eq!(convert(&input, from, Format::Omml).unwrap(), expected.0);
            assert_eq!(
                convert(&input, from, Format::UnicodeMath).unwrap(),
                expected.1
            );
        }
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    /// A mark with a size is a limit: a group character has none.
    #[test]
    fn a_sized_mark_keeps_its_size_in_omml() {
        let mark = MathNode::Symbol(mathema::ast::SymbolKind::Relation, "\u{2193}".into());
        let nodes = vec![MathNode::Underscript {
            base: Box::new(id("B")),
            under: Box::new(sized(points(12, 1), mark)),
            movable_limits: false,
        }];
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
        let written = mathema::writer::omml::OmmlWriter::write_with_options(
            &expr,
            &Config::default(),
            &wml(),
        )
        .unwrap()
        .0;
        assert!(written.contains(r#"<w:sz w:val="34"/>"#), "{written}");
    }

    /// The scripts of a structure shrink from its size, with a difference too.
    #[test]
    fn a_typesize_and_difference_on_a_structure() {
        let script = MathNode::Superscript {
            base: Box::new(id("x")),
            sup: Box::new(MathNode::Number("2".into())),
        };
        let size = NodeAttrs::with_size(FontSize::Logical {
            level: TypeSize::Sub2,
            delta_pt: Rational::new(2, 1),
        });
        let nodes = [sized(size, script)];
        // That of a character in a root's index is kept as it is.
        let sub = NodeAttrs::with_size(FontSize::Logical {
            level: TypeSize::Sub,
            delta_pt: Rational::new(3, 1),
        });
        let root = MathNode::Root {
            index: Box::new(sized(sub, MathNode::Number("4".into()))),
            radicand: Box::new(id("x")),
        };
        let root = [root];
        assert_eq!(through_mtef(&root, MtefVersion::V5).0, root);
        for version in [MtefVersion::V3, MtefVersion::V5] {
            // 8 pt and 0.7 of it, 5.6.
            let text = format!("{:?}", through_mtef(&nodes, version).0);
            assert!(
                text.contains("num: 8, den: 1") && text.contains("num: 179, den: 32"),
                "{text}"
            );
        }
    }

    /// The base of a limit with a size is one element, not a list of one.
    #[test]
    fn a_sized_limit_base_survives_mathml() {
        let nodes = vec![MathNode::Underscript {
            base: Box::new(sized(
                points(18, 1),
                MathNode::Operator(mathema::ast::OperatorAttr {
                    name: "lim".into(),
                    limit_style: mathema::ast::LimitStyle::AboveBelow,
                }),
            )),
            under: Box::new(id("n")),
            movable_limits: true,
        }];
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
        let write = |expr: &CompactExpr| {
            <mathema::writer::mathml::MathmlWriter as Writer>::write_compact(
                expr,
                &Config::default(),
            )
            .unwrap()
        };
        let written = write(&expr);
        let read = from_mathml(&written).unwrap();
        assert!(written.contains(r#"mathsize="18pt""#), "{written}");
        assert!(
            !format!("{:?}", read.expr()).contains("base: Row"),
            "{:?}",
            read.expr()
        );
        assert_eq!(write(read.compact_expr()), written);
    }

    /// A table in a script sets its cells in text style: so is its size.
    #[cfg(feature = "render")]
    #[test]
    fn a_sized_table_in_a_script() {
        use mathema::render::{RenderOptions, render_to_svg_compact};
        let table = MathNode::Array {
            columns: vec![mathema::ast::ColumnAlign::Center],
            rows: vec![vec![vec![MathNode::Number("7".into())]]],
            arraystretch: None,
            add_jot: false,
        };
        let node = MathNode::Superscript {
            base: Box::new(MathNode::Number("8".into())),
            sup: Box::new(sized(points(12, 1), table)),
        };
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(vec![node])).unwrap();
        let svg = render_to_svg_compact(&expr, &RenderOptions::default()).unwrap();
        let size = |text: &str| -> f64 {
            let node = svg.split("<text ").find(|t| t.contains(text)).unwrap();
            let size = node.split("font-size=\"").nth(1).unwrap();
            size.split('"').next().unwrap().parse().unwrap()
        };
        assert!((size(">7<") / size(">8<") - 1.0).abs() < 0.01, "{svg}");
    }

    /// Everything an expression is written as: with and without sizes.
    fn written(expr: &CompactExpr) -> Vec<String> {
        use mathema::writer::{
            latex::LatexWriter, mtef::MtefWriter, omml::OmmlWriter, unicode_math::UnicodeMathWriter,
        };
        let config = Config::default();
        let show = |written: mathema::Result<String>| written.unwrap_or_else(|e| e.to_string());
        let mut out = vec![
            show(<LatexWriter as Writer>::write_compact(expr, &config)),
            show(<MathmlWriter as Writer>::write_compact(expr, &config)),
            show(<OmmlWriter as Writer>::write_compact(expr, &config)),
            show(<UnicodeMathWriter as Writer>::write_compact(expr, &config)),
            show(OmmlWriter::write_with_options(expr, &config, &wml()).map(|(omml, _)| omml)),
        ];
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let bytes = MtefWriter::write_bytes(expr, &config, &MtefWriteOptions::new(version));
            out.push(show(bytes.map(|bytes| format!("{bytes:x?}"))));
        }
        out
    }

    fn written_from_heap(expr: &mathema::MathExpr) -> Vec<String> {
        use mathema::writer::{
            latex::LatexWriter, omml::OmmlWriter, unicode_math::UnicodeMathWriter,
        };
        let config = Config::default();
        let show = |written: mathema::Result<String>| written.unwrap_or_else(|e| e.to_string());
        let mut out = vec![
            show(LatexWriter::write(expr, &config)),
            show(MathmlWriter::write(expr, &config)),
            show(OmmlWriter::write(expr, &config)),
            show(UnicodeMathWriter::write(expr, &config)),
        ];
        out.extend(written(&CompactExpr::from_math_expr(expr).unwrap()).split_off(4));
        out
    }

    /// What is written depends on the tree, not on how it came to be.
    #[track_caller]
    fn every_route_agrees(expr: &CompactExpr) {
        use mathema::ast::normalize::{normalize_compact_expr, normalize_expr};
        let compact = written(expr);
        assert_eq!(written_from_heap(&expr.to_math_expr()), compact);
        let rebuilt = CompactExpr::from_math_expr(&expr.to_math_expr()).unwrap();
        assert_eq!(written(&rebuilt), compact);
        let mut normalized = expr.clone();
        normalize_compact_expr(&mut normalized).unwrap();
        let mut heap = expr.to_math_expr();
        normalize_expr(&mut heap);
        assert_eq!(heap, normalized.to_math_expr());
        assert_eq!(written_from_heap(&heap), written(&normalized));
        // Normalizing what is normalized changes nothing.
        let once = written(&normalized);
        normalize_compact_expr(&mut normalized).unwrap();
        assert_eq!(written(&normalized), once);
    }

    #[test]
    fn heap_and_compact_agree_on_sizes() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        let mut read = 0;
        for dir in ["mtef", "mtef-sizes"] {
            for entry in std::fs::read_dir(format!("{root}/{dir}")).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "bin")
                    && let Ok(result) = from_mtef_bytes(&std::fs::read(&path).unwrap())
                    && result.compact_expr().arena.has_attrs()
                {
                    read += 1;
                    every_route_agrees(result.compact_expr());
                }
            }
        }
        assert!(read >= 6, "{read}");
        // Digits of 300/32 and 320/32 pt, one number without sizes.
        let bytes = [
            9, 101, 44, 1, 2, 136, b'3', 0, 9, 101, 64, 1, 2, 136, b'4', 0,
        ];
        let bytes = [&[3, 1, 1, 3, 10, 1][..], &bytes, &[0, 0]].concat();
        let read = from_mtef_bytes(&bytes).unwrap();
        every_route_agrees(read.compact_expr());
        assert_eq!(read.to_unicode_math().unwrap(), "34");
    }

    /// Trees of neighbours that can be joined, from a fixed sequence.
    #[test]
    fn random_sizes_agree_in_every_route() {
        struct Random(u64);
        impl Random {
            fn below(&mut self, n: u64) -> u64 {
                self.0 = self
                    .0
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (self.0 >> 33) % n
            }
            fn leaf(&mut self) -> MathNode {
                let letter = |r: &mut Self| ["x", "T", "H", "\u{3b1}"][r.below(4) as usize];
                let leaf = match self.below(6) {
                    0 | 1 => id(letter(self)),
                    2 => MathNode::Number(["1", "23", "4.5", ".6"][self.below(4) as usize].into()),
                    3 => MathNode::Text(
                        TextStyle::Normal,
                        ["ab", " ", "c"][self.below(3) as usize].into(),
                    ),
                    4 => MathNode::Text(TextStyle::Bold, "d".into()),
                    _ => MathNode::Symbol(mathema::ast::SymbolKind::BinaryOp, "+".into()),
                };
                self.maybe_sized(leaf)
            }
            fn maybe_sized(&mut self, node: MathNode) -> MathNode {
                match self.below(5) {
                    0 => sized(points(18, 1), node),
                    1 => sized(points(12, 1), node),
                    2 => sized(logical(TypeSize::Sub), node),
                    _ => node,
                }
            }
            fn list(&mut self, depth: u64) -> Vec<MathNode> {
                (0..1 + self.below(4)).map(|_| self.node(depth)).collect()
            }
            fn row(&mut self, depth: u64) -> Box<MathNode> {
                Box::new(MathNode::Row(self.list(depth)))
            }
            fn node(&mut self, depth: u64) -> MathNode {
                if depth == 0 || self.below(3) > 0 {
                    return self.leaf();
                }
                let node = match self.below(5) {
                    0 => MathNode::Fraction {
                        kind: mathema::ast::FractionKind::Normal,
                        num: self.row(depth - 1),
                        den: self.row(depth - 1),
                    },
                    1 => MathNode::Superscript {
                        base: Box::new(self.leaf()),
                        sup: self.row(depth - 1),
                    },
                    2 => MathNode::Sqrt(self.row(depth - 1)),
                    3 => MathNode::Row(self.list(depth - 1)),
                    _ => MathNode::Styled(TextStyle::Bold, vec![id("z")]),
                };
                self.maybe_sized(node)
            }
        }
        for seed in 0..1500 {
            let mut random = Random(seed * 7919 + 1);
            let mut heap = mathema::MathExpr::block(random.list(3));
            let mut expr = CompactExpr::from_math_expr(&heap).unwrap();
            every_route_agrees(&expr);
            written_as_if_unsized(&expr);
            // As the nodes are given, sizes around lists among them.
            assert_eq!(written_from_heap(&heap), written(&expr));
            mathema::ast::normalize::normalize_expr(&mut heap);
            mathema::ast::normalize::normalize_compact_expr(&mut expr).unwrap();
            assert_eq!(written_from_heap(&heap), written(&expr), "{seed}");
        }
    }

    /// A normalized arena that is edited afterwards is written as it is.
    #[test]
    fn edits_of_a_normalized_expression() {
        use mathema::ast::compact::CompactNode;
        use mathema::ast::normalize::normalize_compact_expr;
        let nodes = vec![sized(points(18, 1), id("T")), id("H"), id("\u{3b1}")];
        let mut expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
        normalize_compact_expr(&mut expr).unwrap();
        let ids: Vec<_> = expr.arena.children(expr.roots).to_vec();
        let name = expr.arena.push_str("Q");
        *expr.arena.node_mut(ids[1]) = CompactNode::Identifier(name);
        // Two unsized neighbours, apart because they were put there.
        let third = expr.arena.push_node(CompactNode::Identifier(name));
        expr.roots = expr.arena.push_children(&[ids[0], ids[1], third]).unwrap();
        let heap = expr.to_math_expr();
        assert_eq!(written_from_heap(&heap), written(&expr));
        // Every neighbour is written by itself.
        assert_eq!(written(&expr)[2].matches("<m:r>").count(), 3);
    }

    /// Runs without sizes that are apart stay apart.
    #[test]
    fn unsized_runs_stay_apart() {
        for node in [
            id("x"),
            MathNode::Number("3".into()),
            MathNode::Text(TextStyle::Normal, "a".into()),
        ] {
            let nodes = vec![
                node.clone(),
                node.clone(),
                MathNode::Space(Rational::new(1, 1)),
                sized(points(20, 1), id("z")),
            ];
            let sized_ = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
            let nodes = vec![
                node.clone(),
                node,
                MathNode::Space(Rational::new(1, 1)),
                id("z"),
            ];
            let plain = CompactExpr::from_math_expr(&mathema::MathExpr::block(nodes)).unwrap();
            assert_eq!(written(&sized_)[2..4], written(&plain)[2..4]);
        }
    }

    /// The parts of a fraction inside another or in a script are smaller.
    #[cfg(feature = "render")]
    #[test]
    fn a_size_around_fractions_survives_mtef() {
        let source = "<math><mstyle mathsize='18pt'><mfrac><mn>1</mn><mfrac><mn>2</mn>\
                      <mn>3</mn></mfrac></mfrac><msup><mi>x</mi><mfrac><mn>4</mn><mn>5</mn>\
                      </mfrac></msup><mfrac linethickness='0'><mn>6</mn><mn>7</mn></mfrac>\
                      <msqrt><mfrac><mn>8</mn><mn>9</mn></mfrac></msqrt></mstyle></math>";
        for source in [
            source.to_owned(),
            source.replace("<math>", "<math display='block'>"),
        ] {
            let read = from_mathml(&source).unwrap();
            for version in [MtefVersion::V3, MtefVersion::V5] {
                let bytes = read.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
                let back = from_mtef_bytes(&bytes).unwrap();
                for digit in '1'..='9' {
                    let size = |read: &mathema::ConvertResult| -> f64 {
                        let svg = read.to_svg_text().unwrap();
                        let text = format!(">{digit}<");
                        let node = svg.split("<text ").find(|t| t.contains(&text)).unwrap();
                        let size = node.split("font-size=\"").nth(1).unwrap();
                        size.split('"').next().unwrap().parse().unwrap()
                    };
                    let (from, to) = (size(&read), size(&back));
                    assert!(
                        (from / to - 1.0).abs() < 0.02,
                        "{version:?} {digit} {from} {to}"
                    );
                }
            }
        }
    }

    /// A full size where a smaller one is expected, alone in the equation.
    #[test]
    fn a_lone_full_size_in_a_smaller_slot() {
        let full = || sized(logical(TypeSize::Full), id("n"));
        let pile = MathNode::Array {
            columns: vec![mathema::ast::ColumnAlign::Center],
            rows: vec![vec![vec![full()]], vec![vec![full()]]],
            arraystretch: None,
            add_jot: false,
        };
        for script in [full(), pile] {
            let nodes = [MathNode::Underscript {
                base: Box::new(id("B")),
                under: Box::new(script),
                movable_limits: false,
            }];
            for version in [MtefVersion::V3, MtefVersion::V5] {
                let (back, diagnostics) = through_mtef(&nodes, version);
                assert_eq!(back, nodes, "{version:?} {diagnostics:?}");
            }
        }
    }

    #[cfg(feature = "render")]
    #[test]
    fn a_sized_fraction_with_a_style_of_its_own() {
        use mathema::render::{RenderOptions, render_to_svg_compact};
        // `\tfrac` in a script: its size is that of its text style, the parts
        // are 0.7 of it.
        let frac = from_latex(r"\tfrac{1}{2}").unwrap().expr().nodes.remove(0);
        let node = MathNode::Superscript {
            base: Box::new(id("x")),
            sup: Box::new(sized(points(18, 1), frac)),
        };
        let expr = CompactExpr::from_math_expr(&mathema::MathExpr::block(vec![node])).unwrap();
        let svg = render_to_svg_compact(&expr, &RenderOptions::default()).unwrap();
        let one = svg.split("<text ").find(|t| t.contains(">1<")).unwrap();
        let size: f64 = one
            .split("font-size=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((size / 16.0 - 1.05).abs() < 0.01, "{}", size / 16.0);
    }
}
