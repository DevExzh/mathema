use super::*;
use crate::ast::*;
use crate::error::{DiagnosticKind, Error, LimitKind, Strictness};
use crate::reader::Reader;
use crate::reader::mtef::MtefReader;
use crate::writer::Writer;

#[test]
fn write_emits_warning_for_dropped_nodes() {
    let config = Config::default();
    // \color{red}{x} lowers to a Color node, which the MTEF writer drops.
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\color{red}{x}", &config).unwrap();
    let (hex, diags) = MtefWriter::write_compact_with_diagnostics(&result.expr, &config).unwrap();
    assert!(!hex.is_empty());
    assert!(
        diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning && d.message.contains("colour")),
        "expected a color-loss warning, got {diags:?}"
    );
}

#[test]
fn write_emits_no_warning_for_lossless_input() {
    let config = Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("x + y", &config).unwrap();
    let (_, diags) = MtefWriter::write_compact_with_diagnostics(&result.expr, &config).unwrap();
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

fn roundtrip(node: MathNode) -> MathExpr {
    let expr = MathExpr::inline(vec![node]);
    let config = Config::default();
    let hex = MtefWriter::write(&expr, &config).unwrap();
    MtefReader::parse(&hex, &config).unwrap().expr
}

fn write_hex(node: MathNode) -> String {
    let expr = MathExpr::inline(vec![node]);
    MtefWriter::write(&expr, &Config::default()).unwrap()
}

#[test]
fn records5_core_template_selectors_are_written() {
    assert!(
        write_hex(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        })
        .contains("03000b00")
    );

    assert!(
        write_hex(MathNode::Sqrt(Box::new(MathNode::Identifier("x".into())))).contains("03000a00")
    );

    assert!(
        write_hex(MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Number("2".into())),
        })
        .contains("03001b00")
    );

    assert!(
        write_hex(MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        })
        .contains("03001c00")
    );

    assert!(
        write_hex(MathNode::SubSup {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Number("2".into())),
        })
        .contains("03001d00")
    );

    let line = MathNode::Symbol(SymbolKind::Ordinary, "\u{203E}".into());
    assert!(
        write_hex(MathNode::Overscript {
            base: Box::new(MathNode::Identifier("x".into())),
            over: Box::new(line.clone()),
            movable_limits: false,
        })
        .contains("03000d00")
    );
    assert!(
        write_hex(MathNode::Underscript {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(line),
            movable_limits: false,
        })
        .contains("03000c00")
    );

    assert!(
        write_hex(MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::AboveBelow,
        })
        .contains("03001070")
    );

    assert!(
        write_hex(MathNode::Nary {
            op: '\u{222B}',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: Some(Box::new(MathNode::Number("1".into()))),
            body: Box::new(MathNode::Identifier("f".into())),
            limits: LimitStyle::SubSup,
        })
        .contains("03000f31")
    );
}

#[test]
fn number_roundtrip() {
    let expr = roundtrip(MathNode::Number("2".into()));
    assert!(!expr.nodes.is_empty());
}

#[test]
fn identifier_roundtrip() {
    let expr = roundtrip(MathNode::Identifier("x".into()));
    assert!(!expr.nodes.is_empty());
}

#[test]
fn fraction_roundtrip() {
    let expr = roundtrip(MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Number("1".into())),
        den: Box::new(MathNode::Number("2".into())),
    });
    assert!(!expr.nodes.is_empty());
    assert!(matches!(&expr.nodes[0], MathNode::Fraction { .. }));
}

#[test]
fn sqrt_roundtrip() {
    let expr = roundtrip(MathNode::Sqrt(Box::new(MathNode::Identifier("x".into()))));
    assert!(matches!(&expr.nodes[0], MathNode::Sqrt(_)));
}

#[test]
fn accent_roundtrip_preserves_embellishment() {
    let expr = roundtrip(MathNode::Accent {
        base: Box::new(MathNode::Identifier("x".into())),
        accent: '\u{0302}',
        position: AccentPosition::Above,
        wide: false,
    });
    assert!(
        matches!(
            &expr.nodes[0],
            MathNode::Accent {
                accent: '\u{0302}',
                ..
            }
        ),
        "MTEF writer should attach embellishment to the character, got {:?}",
        expr.nodes
    );
}

#[test]
fn parentheses_roundtrip() {
    let expr = roundtrip(MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
    });
    assert!(matches!(&expr.nodes[0], MathNode::Fenced { .. }));
}

#[test]
fn prescripts_emit_script_template() {
    // Leading scripts use the script templates with tvSU_PRECEDES (variation
    // 1, MTEFv5.md "Subscripts and superscripts"), then the base.
    let hex = write_hex(MathNode::PreScripts {
        base: Box::new(MathNode::Identifier("X".into())),
        pre_sub: Some(Box::new(MathNode::Number("1".into()))),
        pre_sup: Some(Box::new(MathNode::Number("2".into()))),
    });
    assert!(hex.contains("03001d01"), "missing tmSUBSUP: {hex}");

    let sub_only = write_hex(MathNode::PreScripts {
        base: Box::new(MathNode::Identifier("X".into())),
        pre_sub: Some(Box::new(MathNode::Number("1".into()))),
        pre_sup: None,
    });
    assert!(sub_only.contains("03001b01"), "missing tmSUB: {sub_only}");
    let sup_only = write_hex(MathNode::PreScripts {
        base: Box::new(MathNode::Identifier("X".into())),
        pre_sub: None,
        pre_sup: Some(Box::new(MathNode::Number("2".into()))),
    });
    assert!(sup_only.contains("03001c01"), "missing tmSUP: {sup_only}");
}

#[test]
fn prescripts_roundtrip_preserves_content() {
    // The base and both scripts must survive a write→read round-trip.
    let expr = roundtrip(MathNode::PreScripts {
        base: Box::new(MathNode::Identifier("X".into())),
        pre_sub: Some(Box::new(MathNode::Number("1".into()))),
        pre_sup: Some(Box::new(MathNode::Number("2".into()))),
    });
    let flat = format!("{:?}", expr.nodes);
    assert!(flat.contains("\"X\""), "lost base: {flat}");
    assert!(flat.contains("\"1\""), "lost pre-sub: {flat}");
    assert!(flat.contains("\"2\""), "lost pre-sup: {flat}");
}

#[test]
fn color_preserves_inner_content() {
    let config = Config::default();
    // `\textcolor{red}{x}` lowers to a Color node; MTEF drops the color but
    // must preserve the inner `x` (verified by a write→read round-trip).
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\textcolor{red}{x}", &config).unwrap();
    let hex = MtefWriter::write_compact(&result.expr, &config).unwrap();
    let expr = MtefReader::parse(&hex, &config).unwrap().expr;
    let flat = format!("{:?}", expr.nodes);
    assert!(flat.contains("\"x\""), "lost colored content: {flat}");
}

fn bytes(latex: &str, options: &MtefWriteOptions) -> Vec<u8> {
    let result = crate::from_latex(latex).unwrap();
    result.to_mtef_bytes(options).unwrap()
}

fn v3() -> MtefWriteOptions {
    MtefWriteOptions::new(MtefVersion::V3)
}

/// The body after the MTEF 5 header and definitions.
fn v5_body(data: &[u8]) -> &[u8] {
    let key_end = 5 + data[5..].iter().position(|&b| b == 0).unwrap();
    let body = &data[key_end + 2..];
    let prelude = [V5_DEFINITIONS, V5_PREFERENCES, far_east_style(false)].concat();
    assert_eq!(&body[..prelude.len()], prelude.as_slice());
    &body[prelude.len()..]
}

#[test]
fn mtef3_superscript_has_equation_editor_layout() {
    // As Equation Editor writes x² (compare the real streams in the fixture
    // README): FULL, LINE, x, tmSCRIPT tvSUPER with SUB, a null subscript
    // line and the superscript line, END of line, END of equation.
    assert_eq!(
        bytes("x^2", &v3()),
        [
            3, 1, 1, 3, 10, // header
            0x0A, 0x01, // FULL, LINE
            0x12, 0x83, b'x', 0, // CHAR (xfAUTO) Variable 'x'
            0x03, 15, 0, 0, // TMPL tmSCRIPT tvSUPER
            0x0B, 0x11, // SUB, null LINE
            0x01, 0x02, 0x88, b'2', 0, 0x00, // LINE '2' END
            0x00, 0x00, 0x00, // END template, line, equation
        ]
    );
}

#[test]
fn mtef5_header_and_definitions_follow_mathtype() {
    let data = bytes("x", &MtefWriteOptions::default());
    assert_eq!(&data[..11], b"\x05\x01\x00\x06\x00DSMT6\x00");
    // Display equation (the LaTeX reader's default).
    assert_eq!(data[11], 0);
    assert_eq!(
        v5_body(&data),
        [0x0A, 0x01, 0x00, 0x02, 0x00, 0x83, b'x', 0, 0x00, 0x00]
    );
}

#[test]
fn mtef5_symbols_carry_their_font_positions() {
    // α in the LC Greek style and = in the Symbol style are written with
    // mtefOPT_CHAR_ENC_CHAR_8 and their Symbol font positions, as MathType
    // does; digits carry only their MTCode.
    let data = bytes(r"\alpha=1", &MtefWriteOptions::default());
    assert_eq!(
        v5_body(&data),
        [
            0x0A, 0x01, 0x00, // FULL, LINE
            0x02, 0x04, 0x84, 0xB1, 0x03, 0x61, // α
            0x02, 0x04, 0x86, 0x3D, 0x00, 0x3D, // =
            0x02, 0x00, 0x88, b'1', 0x00, // 1
            0x00, 0x00,
        ]
    );
}

#[test]
fn sizes_change_only_where_needed() {
    // x^2 + y: SUB before the script slots, FULL again before '+'.
    let data = bytes("x^2+y", &MtefWriteOptions::default());
    let body = v5_body(&data);
    let sub = body.iter().position(|&b| b == 0x0B).unwrap();
    let full = body.iter().rposition(|&b| b == 0x0A).unwrap();
    assert!(sub < full);
    assert_eq!(body.iter().filter(|&&b| b == 0x0A).count(), 2);
}

#[test]
fn fence_and_operator_characters_follow_the_slots() {
    let data = bytes(r"\left( x \right)", &MtefWriteOptions::default());
    let body = v5_body(&data);
    // TMPL tmPAREN both sides, slot, then '(' and ')' in fnEXPAND.
    let tail = [0x02, 0x00, 0x96, b'(', 0, 0x02, 0x00, 0x96, b')', 0, 0x00];
    assert!(body.windows(tail.len()).any(|w| w == tail), "{body:02x?}");
    let sum = bytes(r"\sum_{i}^{n} i", &MtefWriteOptions::default());
    // tmSUM with lower, upper and summation-style limits, then SYM and ∑.
    assert!(sum.windows(4).any(|w| w == [0x03, 0x00, 16, 0x70]));
    assert!(
        sum.windows(6)
            .any(|w| w == [0x0D, 0x02, 0x04, 0x86, 0x11, 0x22])
    );
}

#[test]
fn ole_header_has_the_equation_native_layout() {
    let options = MtefWriteOptions::default().with_ole_header(true);
    let data = bytes("x", &options);
    assert_eq!(u16::from_le_bytes([data[0], data[1]]), 28);
    assert_eq!(
        u32::from_le_bytes(data[2..6].try_into().unwrap()),
        0x0002_0000
    );
    let size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    assert_eq!(size, data.len() - 28);
    assert!(data[12..28].iter().all(|&b| b == 0));
    let back = crate::from_mtef_bytes(&data).unwrap();
    assert!(back.mtef_info().unwrap().ole_header.is_some());
}

#[test]
fn options_from_info_keep_the_object_kind() {
    let original = bytes(r"\frac{a}{b}", &v3().with_ole_header(true));
    let result = crate::from_mtef_bytes(&original).unwrap();
    let options = MtefWriteOptions::from_info(result.mtef_info().unwrap());
    assert_eq!(options, v3().with_ole_header(true));
    assert_eq!(result.to_mtef_bytes(&options).unwrap(), original);
}

#[test]
fn adjacent_runs_stay_separate() {
    // A number before a number base, two MTEF 3 function names, and two bold
    // runs would merge when read; a zero space keeps them apart.
    for latex in [
        "1{2}^{3}",
        r"\sin\cos x",
        r"\mathbf{a}\mathbf{b}^2",
        r"\text{a}\text{b}^2",
    ] {
        for options in [MtefWriteOptions::default(), v3()] {
            let result = crate::from_latex(latex).unwrap();
            let data = result.to_mtef_bytes(&options).unwrap();
            let back = crate::from_mtef_bytes(&data).unwrap();
            assert_eq!(
                back.to_latex().unwrap(),
                result.to_latex().unwrap(),
                "{latex}"
            );
        }
    }
}

#[test]
fn invisible_operators_are_skipped() {
    let a = crate::from_mathml("<math><mi>f</mi><mo>\u{2061}</mo><mi>x</mi></math>").unwrap();
    let data = a.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert!(!data.windows(2).any(|w| w == [0x61, 0x20]));
}

#[test]
fn spaces_use_mathtype_widths() {
    let data = bytes(r"a\,b\quad c\ d", &MtefWriteOptions::default());
    let body = v5_body(&data);
    // Thin space EF02, em space EF05, and the no-break space MathType writes
    // in the Function style for a quarter em.
    assert!(body.windows(5).any(|w| w == [0x02, 0x00, 0x98, 0x02, 0xEF]));
    assert!(body.windows(5).any(|w| w == [0x02, 0x00, 0x98, 0x05, 0xEF]));
    assert!(body.windows(5).any(|w| w == [0x02, 0x00, 0x82, 0xA0, 0x00]));
    let back = crate::from_mtef_bytes(&data).unwrap();
    assert_eq!(back.to_latex().unwrap(), r"a\,b\quad c\ d");
}

#[test]
fn huge_space_is_bounded() {
    let expr = MathExpr::inline(vec![MathNode::Space(Rational::new(i32::MAX, 1))]);
    let compact = CompactExpr::from_math_expr(&expr).unwrap();
    let (data, diagnostics) = MtefWriter::write_bytes_with_diagnostics(
        &compact,
        &Config::default(),
        &MtefWriteOptions::default(),
    )
    .unwrap();
    assert!(data.len() < 1024);
    assert!(!diagnostics.is_empty());
}

#[test]
fn deep_ast_fails_with_a_limit_error() {
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..40 {
        node = MathNode::Sqrt(Box::new(node));
    }
    let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
    let config = Config::default().with_max_depth(16);
    let err = MtefWriter::write_bytes(&compact, &config, &MtefWriteOptions::default()).unwrap_err();
    assert!(
        matches!(
            err,
            Error::LimitExceeded {
                kind: LimitKind::Depth,
                ..
            }
        ),
        "{err}"
    );
    // Default limits accept it.
    assert!(
        MtefWriter::write_bytes(&compact, &Config::default(), &MtefWriteOptions::default()).is_ok()
    );
}

#[test]
fn output_never_nests_deeper_than_the_reader_accepts() {
    // 100 nested roots are a valid AST (depth 101), but each root takes two
    // object lists, more than the reader reads with the same Config.
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..100 {
        node = MathNode::Sqrt(Box::new(node));
    }
    let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
    for options in [MtefWriteOptions::default(), v3()] {
        let err = MtefWriter::write_bytes(&compact, &Config::default(), &options).unwrap_err();
        assert!(
            matches!(
                err,
                Error::LimitExceeded {
                    kind: LimitKind::Depth,
                    ..
                }
            ),
            "{err}"
        );
    }
    // The deepest nesting that is written reads back in Strict mode.
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..62 {
        node = MathNode::Sqrt(Box::new(node));
    }
    let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
    let data = MtefWriter::write_bytes(&compact, &Config::default(), &MtefWriteOptions::default())
        .unwrap();
    crate::from_mtef_bytes(&data).unwrap();
}

#[test]
fn too_deep_math_expr_is_refused_without_panicking() {
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..10_000 {
        node = MathNode::Row(vec![node]);
    }
    assert!(MtefWriter::write(&MathExpr::inline(vec![node]), &Config::default()).is_err());
}

#[test]
fn node_limit_is_enforced() {
    let nodes = (0..100).map(|_| MathNode::Identifier("x".into())).collect();
    let compact = CompactExpr::from_math_expr(&MathExpr::inline(nodes)).unwrap();
    let config = Config::default().with_max_nodes(Some(10));
    let err = MtefWriter::write_bytes(&compact, &config, &MtefWriteOptions::default()).unwrap_err();
    assert!(
        matches!(
            err,
            Error::LimitExceeded {
                kind: LimitKind::Nodes,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn oversized_matrix_is_refused() {
    let row = vec![vec![MathNode::Number("1".into())]; 256];
    let expr = MathExpr::inline(vec![MathNode::Array {
        columns: vec![ColumnAlign::Center; 256],
        rows: vec![row],
        arraystretch: None,
        add_jot: false,
    }]);
    let compact = CompactExpr::from_math_expr(&expr).unwrap();
    let err = MtefWriter::write_bytes(&compact, &Config::default(), &MtefWriteOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err}");
}

#[test]
fn characters_beyond_16_bits_depend_on_strictness() {
    let expr = MathExpr::inline(vec![MathNode::Identifier("\u{1D400}".into())]);
    let compact = CompactExpr::from_math_expr(&expr).unwrap();
    let options = MtefWriteOptions::default();
    assert!(matches!(
        MtefWriter::write_bytes(&compact, &Config::default(), &options),
        Err(Error::Unsupported(_))
    ));
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let (data, diagnostics) =
        MtefWriter::write_bytes_with_diagnostics(&compact, &lenient, &options).unwrap();
    assert!(!diagnostics.is_empty());
    assert!(crate::from_mtef_bytes(&data).is_ok());
}

#[test]
fn every_node_kind_reads_back_in_strict_mode() {
    let cases = [
        r"\phantom{x}y",
        r"\color{red}{x}",
        r"\scalebox{2}{x}",
        r"\raisebox{1pt}{x}",
        r"\mathchoice{a}{b}{c}{d}",
        r"\mathrel{x}",
        r"\rule{1pt}{2pt}",
        r"\displaystyle x",
        r"x \\ y",
        r"\left/ x \right.",
        r"\acute{x}+\check{xy}",
        r"\mathsf{A}\mathcal{B}",
        r"\! x",
        r"{}^{a}",
        r"x^{}",
        r"\vphantom{x}^2",
        r"\overset{a}{\underset{b}{c}}",
        r"\xrightleftharpoons{a}",
        r"\begin{array}{cc} \end{array}",
    ];
    for latex in cases {
        let result = crate::from_latex(latex).unwrap();
        for options in [MtefWriteOptions::default(), v3()] {
            let data = result.to_mtef_bytes(&options).unwrap();
            crate::from_mtef_bytes(&data).unwrap_or_else(|e| panic!("{latex}: {e}"));
        }
    }
}

/// Read without normalization, which keeps leading scripts on their base.
fn raw(data: &[u8]) -> MathExpr {
    MtefReader::parse_bytes(data, &Config::default())
        .unwrap()
        .expr
}

fn leading_script_count(data: &[u8]) -> usize {
    // TMPL, options 0, tmSUB/tmSUP/tmSUBSUP, tvSU_PRECEDES.
    data.windows(4)
        .filter(|w| w[0] == 3 && w[1] == 0 && (27..=29).contains(&w[2]) && w[3] == 1)
        .count()
}

#[test]
fn leading_scripts_keep_their_base_and_variation() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mtef-writer");
    for name in [
        "upstream-v5-leading-scripts.bin",
        "upstream-v5-leading-scripts-sums.bin",
        "upstream-v5-leading-space-script.bin",
    ] {
        let original = std::fs::read(dir.join(name)).unwrap();
        let before = raw(&original);
        let compact = CompactExpr::from_math_expr(&before).unwrap();
        let written =
            MtefWriter::write_bytes(&compact, &Config::default(), &MtefWriteOptions::default())
                .unwrap();
        // The same meaning: every leading script still precedes the same base.
        assert_eq!(raw(&written), before, "{name}");
        assert_eq!(
            leading_script_count(&written),
            leading_script_count(&original),
            "{name}"
        );
        assert!(leading_script_count(&written) >= 4, "{name}");
    }
}

#[test]
fn empty_base_scripts_precede_the_next_item() {
    // `x={}^{14}_{6}C`: the scripts belong to C, written directly before it
    // with tvSU_PRECEDES and no space, as MathType writes them.
    for (latex, base) in [
        (r"x={}^{14}_{6}C", "C"),
        (r"a{}^2b", "b"),
        (r"1+{}^2L", "L"),
    ] {
        let result = crate::from_latex(latex).unwrap();
        for options in [MtefWriteOptions::default(), v3()] {
            let data = result.to_mtef_bytes(&options).unwrap();
            let text = format!("{:?}", raw(&data).nodes);
            assert!(
                text.contains(&format!("PreScripts {{ base: Identifier(\"{base}\")")),
                "{latex}: {text}"
            );
            assert!(
                !data.windows(4).any(|w| w == [0x02, 0x00, 0x98, 0x01]),
                "{latex}"
            );
        }
    }
}

#[test]
fn zero_denominator_space_is_refused_or_dropped() {
    let expr = MathExpr::inline(vec![
        MathNode::Identifier("a".into()),
        MathNode::Space(Rational { num: 1, den: 0 }),
        MathNode::Identifier("b".into()),
    ]);
    assert!(matches!(
        MtefWriter::write(&expr, &Config::default()),
        Err(Error::Unsupported(_))
    ));
    let compact = CompactExpr::from_math_expr(&expr).unwrap();
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let (data, diagnostics) =
        MtefWriter::write_bytes_with_diagnostics(&compact, &lenient, &MtefWriteOptions::default())
            .unwrap();
    assert!(!diagnostics.is_empty());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        "ab"
    );
}

#[test]
fn control_characters_are_refused_or_replaced() {
    let result = crate::from_latex("\\text{a\u{0}b}").unwrap();
    let options = MtefWriteOptions::default();
    assert!(matches!(
        result.to_mtef_bytes(&options),
        Err(Error::Unsupported(_))
    ));
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let (data, diagnostics) =
        MtefWriter::write_bytes_with_diagnostics(result.compact_expr(), &lenient, &options)
            .unwrap();
    assert!(!diagnostics.is_empty());
    let back = crate::from_mtef_bytes(&data).unwrap();
    assert!(back.to_latex().unwrap().contains('\u{FFFD}'));
}

#[test]
fn far_east_text_uses_its_style_and_font() {
    let data = bytes(r"\text{中文}x", &MtefWriteOptions::default());
    // ENCODING_DEF WinAllCodePages, FONT_DEF 5 and the fnTEXT_FE style.
    let fe = [
        V5_DEFINITIONS,
        V5_FAR_EAST_DEFINITIONS,
        V5_PREFERENCES,
        far_east_style(true),
    ]
    .concat();
    let key_end = 5 + data[5..].iter().position(|&b| b == 0).unwrap();
    assert_eq!(&data[key_end + 2..key_end + 2 + fe.len()], fe.as_slice());
    // 中 in typeface 12.
    assert!(data.windows(5).any(|w| w == [0x02, 0x00, 0x8C, 0x2D, 0x4E]));
    let back = crate::from_mtef_bytes(&data).unwrap();
    assert_eq!(back.to_latex().unwrap(), r"\text{中文}x");
    // MTEF 3 uses the style too; text without CJK keeps the plain prelude.
    assert!(
        bytes(r"\text{中}", &v3())
            .windows(4)
            .any(|w| w == [0x02, 0x8C, 0x2D, 0x4E])
    );
    v5_body(&bytes(r"\text{ab}", &MtefWriteOptions::default()));
}

#[test]
fn pile_in_a_template_slot_is_written_as_a_pile() {
    let data = bytes(r"\lim_{\substack{n \to \infty \\ x \to 0}} f", &v3());
    // tmLIM, the main slot, SUB, then PILE (tag 4) directly as the lower slot.
    let lim = data.windows(3).position(|w| w == [0x03, 39, 1]).unwrap();
    let after = &data[lim..];
    let sub = after.iter().position(|&b| b == 0x0B).unwrap();
    assert_eq!(after[sub + 1], 0x04, "{after:02x?}");
}

#[test]
fn adjacent_texts_are_written_stably() {
    let result = crate::from_latex(r"\text{abc}\text{def}").unwrap();
    let first = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let back = crate::from_mtef_bytes(&first).unwrap();
    let second = back.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert_eq!(first, second);
    assert_eq!(back.to_latex().unwrap(), r"\text{abcdef}");
}

#[test]
fn trailing_empty_base_script_is_written_stably() {
    // Not normalized: `x{}^2` reads back as x squared, as it normalizes.
    let result = crate::from_latex("x{}^2").unwrap();
    let first = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let back = crate::from_mtef_bytes(&first).unwrap();
    assert_eq!(back.to_latex().unwrap(), "x^{2}");
    assert_eq!(
        back.to_mtef_bytes(&MtefWriteOptions::default()).unwrap(),
        first
    );
}

#[test]
fn leading_script_in_a_dirac_slot_precedes_its_base() {
    let data = bytes(
        r"\left\langle {}^2 b \middle| c \right\rangle",
        &MtefWriteOptions::default(),
    );
    assert_eq!(leading_script_count(&data), 1);
}

#[test]
fn cjk_variables_use_the_far_east_style() {
    let result = crate::from_mathml("<math><mi>中</mi><mo>=</mo><mn>1</mn></math>").unwrap();
    let data = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert!(data.windows(5).any(|w| w == [0x02, 0x00, 0x8C, 0x2D, 0x4E]));
    assert!(crate::from_mtef_bytes(&data).is_ok());
}
