use super::*;
use crate::ast::*;
use crate::error::{DiagnosticKind, Error, LimitKind, Strictness};
use crate::reader::Reader;
use crate::reader::mtef::MtefReader;
use crate::writer::Writer;

#[test]
fn write_emits_warning_for_dropped_nodes() {
    let config = Config::default();
    // \color{red}{x} lowers to a Color node, which MTEF 3 cannot hold.
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\color{red}{x}", &config).unwrap();
    let (bytes, diags) = MtefWriter::write_bytes_with_diagnostics(
        &result.expr,
        &config,
        &MtefWriteOptions::new(crate::MtefVersion::V3),
    )
    .unwrap();
    assert!(!bytes.is_empty());
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

#[test]
fn write_reports_native_rule_padding_normalization() {
    for (col_seps, row_seps) in [(0b111, 0), (0, 0b11)] {
        let expr = MathExpr::inline(vec![MathNode::NativeArray {
            columns: vec![ColumnAlign::Center, ColumnAlign::Center],
            rows: vec![vec![
                vec![MathNode::Identifier("a".into())],
                vec![MathNode::Identifier("b".into())],
            ]],
            arraystretch: None,
            add_jot: false,
            array_style: false,
            col_seps,
            row_seps,
            dashed_col_seps: 0,
            dashed_row_seps: 0,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let (_, diagnostics) =
            MtefWriter::write_compact_with_diagnostics(&compact, &Config::default()).unwrap();
        assert!(diagnostics.iter().any(|d| {
            d.kind == DiagnosticKind::Warning && d.message.contains("restores outer padding")
        }));
    }
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

fn mtef_latex(input: &str, display: DisplayStyle, options: MtefWriteOptions) -> String {
    let config = Config::default().with_display(display);
    let result = crate::from_latex_with(input, &config).unwrap();
    let bytes = result.to_mtef_bytes(&options).unwrap();
    crate::from_mtef_bytes_with(&bytes, &config)
        .unwrap()
        .to_latex()
        .unwrap()
}

fn mtef_latex_heap(input: &str, display: DisplayStyle) -> String {
    let config = Config::default().with_display(display);
    let result = crate::from_latex_with(input, &config).unwrap();
    let hex = MtefWriter::write(&result.expr(), &config).unwrap();
    let parsed = MtefReader::parse(&hex, &config).unwrap();
    crate::writer::latex::LatexWriter::write(&parsed.expr, &config).unwrap()
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
    // `\textcolor{red}{x}` lowers to a Color node; MTEF 5 writes it with
    // COLOR_DEF and COLOR records (verified by a write→read round-trip).
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\textcolor{red}{x}", &config).unwrap();
    let hex = MtefWriter::write_compact(&result.expr, &config).unwrap();
    let expr = MtefReader::parse(&hex, &config).unwrap().expr;
    assert_eq!(
        expr.nodes,
        [MathNode::Color {
            color: "#ff0000".into(),
            content: Box::new(MathNode::Identifier("x".into())),
        }]
    );
}

#[test]
fn colour_round_trips_and_restores_black() {
    // The content after a colour node is drawn in black again.
    let result = crate::from_latex(r"a\textcolor{blue}{b+c}d").unwrap();
    let bytes = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let back = crate::from_mtef_bytes(&bytes).unwrap();
    assert_eq!(
        back.to_latex().unwrap(),
        r"a\textcolor[HTML]{0000ff}{b+ c}d"
    );
    assert_eq!(
        back.to_mtef_bytes(&MtefWriteOptions::default()).unwrap(),
        bytes
    );
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
    let prelude = [V5_DEFINITIONS, V5_PREFERENCES, far_east_style(false, false)].concat();
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

#[test]
fn mathchoice_mtef_uses_the_effective_style() {
    let choice = r"\mathchoice{D}{T}{S}{Q}";
    for options in [MtefWriteOptions::default(), v3()] {
        assert_eq!(
            mtef_latex(choice, DisplayStyle::Block, options.clone()),
            "D"
        );
        assert_eq!(
            mtef_latex(choice, DisplayStyle::Inline, options.clone()),
            "T"
        );
        assert_eq!(
            mtef_latex(
                r"x^{\mathchoice{D}{T}{S}{Q}}",
                DisplayStyle::Block,
                options.clone()
            ),
            "x^S"
        );
        assert_eq!(
            mtef_latex(
                r"x_{y^{\mathchoice{D}{T}{S}{Q}}}",
                DisplayStyle::Block,
                options.clone()
            ),
            "x_{y^Q}"
        );
        assert_eq!(
            mtef_latex(
                r"\frac{\mathchoice{D}{T}{S}{Q}}{\mathchoice{D}{T}{S}{Q}}",
                DisplayStyle::Block,
                options.clone()
            ),
            r"\frac{T}{T}"
        );
        assert_eq!(
            mtef_latex(
                r"\sqrt[\mathchoice{D}{T}{S}{Q}]{z}",
                DisplayStyle::Block,
                options.clone()
            ),
            r"\sqrt[Q]{z}"
        );
        assert_eq!(
            mtef_latex(
                r"\dfrac{\mathchoice{D}{T}{S}{Q}}{z}",
                DisplayStyle::Inline,
                options.clone()
            ),
            r"\frac{T}{z}"
        );
        assert_eq!(
            mtef_latex(
                r"\tfrac{\mathchoice{D}{T}{S}{Q}}{z}",
                DisplayStyle::Block,
                options.clone()
            ),
            r"\tfrac{S}{z}"
        );
        assert_eq!(
            mtef_latex(
                r"{\displaystyle\mathchoice{D}{T}{S}{Q}}",
                DisplayStyle::Inline,
                options.clone()
            ),
            "D"
        );
        assert_eq!(
            mtef_latex(
                r"{\scriptstyle\mathchoice{D}{T}{S}{Q}}\mathchoice{D}{T}{S}{Q}",
                DisplayStyle::Block,
                options
            ),
            "SD"
        );
    }
    let fence = r"\left(\scriptstyle\mathchoice{D}{T}{S}{Q}\middle|\mathchoice{D}{T}{S}{Q}\right)";
    for options in [MtefWriteOptions::default(), v3()] {
        assert_eq!(
            mtef_latex(fence, DisplayStyle::Block, options.clone()),
            mtef_latex(r"\left(S\middle|D\right)", DisplayStyle::Block, options)
        );
    }
    for source in [choice, r"x^{\mathchoice{D}{T}{S}{Q}}"] {
        assert_eq!(
            mtef_latex(source, DisplayStyle::Block, MtefWriteOptions::default()),
            mtef_latex_heap(source, DisplayStyle::Block),
            "compact/heap MTEF choice mismatch for {source}"
        );
    }
}

#[test]
fn mathchoice_mtef_uses_table_cell_styles() {
    let cases = [
        (r"\begin{matrix}\mathchoice{D}{T}{S}{Q}\end{matrix}", "T"),
        (
            r"\begin{smallmatrix}\mathchoice{D}{T}{S}{Q}\end{smallmatrix}",
            "S",
        ),
        (
            r"x^{\begin{array}{c}\mathchoice{D}{T}{S}{Q}\end{array}}",
            "T",
        ),
        (
            r"\begin{matrix}\scriptstyle\mathchoice{D}{T}{S}{Q}\end{matrix}",
            "S",
        ),
        (r"\begin{aligned}\mathchoice{D}{T}{S}{Q}\end{aligned}", "D"),
    ];
    for options in [MtefWriteOptions::default(), v3()] {
        for (source, expected) in cases {
            let written = mtef_latex(source, DisplayStyle::Inline, options.clone());
            let branches: String = written
                .chars()
                .filter(|ch| matches!(ch, 'D' | 'T' | 'S' | 'Q'))
                .collect();
            assert!(
                branches == expected,
                "{source} selected the wrong MTEF branch: {written}"
            );
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
        far_east_style(true, false),
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
    // Not normalized: `x{}^2` keeps its empty base, as in TeX.
    let result = crate::from_latex("x{}^2").unwrap();
    let first = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let back = crate::from_mtef_bytes(&first).unwrap();
    assert_eq!(back.to_latex().unwrap(), "x{}^{2}");
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

#[test]
fn unruled_native_matrix_and_pile_report_padding_loss() {
    let config = Config::default();
    for input in [
        r"\begin{array}{c}x\end{array}",
        r"\begin{array}{cc}x&y\end{array}",
    ] {
        let result = crate::reader::latex::LatexReader::parse_compact(input, &config).unwrap();
        let (_, diags) = MtefWriter::write_compact_with_diagnostics(&result.expr, &config).unwrap();
        assert!(
            diags.iter().any(|d| d.message.contains("outer padding")),
            "{input}: {diags:?}"
        );
    }
}

#[test]
fn fraktur_and_script_letters_use_the_euclid_fonts() {
    let result = crate::from_latex(r"\mathfrak{Bg}+\mathcal{N}").unwrap();
    let (data, diagnostics) = result
        .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::default())
        .unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    // MTCode U+F001 ('B') and U+F020 ('g') in Euclid Fraktur, U+F10D ('N')
    // in Euclid Math One, each with its font position.
    assert!(data.windows(4).any(|w| w == [0x01, 0xF0, b'B', 0x02]));
    assert!(data.windows(3).any(|w| w == [0x20, 0xF0, b'g']));
    assert!(data.windows(3).any(|w| w == [0x0D, 0xF1, b'N']));
    assert!(data.windows(14).any(|w| w == b"Euclid Fraktur"));
    let back = crate::from_mtef_bytes(&data).unwrap();
    let nodes = format!("{:?}", back.expr().nodes);
    assert!(
        nodes.contains(r#"Styled(Fraktur, [Identifier("B"), Identifier("g")])"#)
            && nodes.contains(r#"Styled(Script, [Identifier("N")])"#),
        "{nodes}"
    );
    let again = back.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert_eq!(again, data);
}

#[test]
fn alphabets_and_far_east_text_get_distinct_fonts() {
    let result =
        crate::from_mathml("<math><mi mathvariant=\"fraktur\">A</mi><mtext>中</mtext></math>")
            .unwrap();
    let data = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let back = crate::from_mtef_bytes(&data).unwrap();
    assert_eq!(back.to_mathml().unwrap(), result.to_mathml().unwrap());
    // The Far East font (FONT_DEF 7) uses its own encoding (ENCODING_DEF 8).
    assert!(data.windows(3).any(|w| w == [0x11, 8, b'S']));
}

#[test]
fn small_script_letters_are_a_reported_loss() {
    let result = crate::from_latex(r"\mathcal{n}").unwrap();
    let (data, diagnostics) = result
        .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::default())
        .unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("math alphabet"))
    );
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        "n"
    );
}

#[test]
fn empty_limits_are_a_reported_loss() {
    let result = crate::from_latex(r"\underset{\underset{}{}}{x}").unwrap();
    let (data, diagnostics) = result
        .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::default())
        .unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("empty limits")),
        "{diagnostics:?}"
    );
    let back = crate::from_mtef_bytes(&data).unwrap();
    let again = back.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    let third = crate::from_mtef_bytes(&again).unwrap();
    assert_eq!(
        third.to_mtef_bytes(&MtefWriteOptions::default()).unwrap(),
        again
    );
}

#[test]
fn mtef_3_has_no_math_alphabets() {
    let result = crate::from_latex(r"\mathfrak{B}").unwrap();
    let (_, diagnostics) = result
        .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(MtefVersion::V3))
        .unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("math alphabet"))
    );
}

#[test]
fn dashed_partition_lines_round_trip() {
    let result =
        crate::from_latex(r"\begin{array}{l:l|l} a & b & c \\ \hdashline d & e & f \end{array}")
            .unwrap();
    for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
        let bytes = result
            .to_mtef_bytes(&MtefWriteOptions::new(version))
            .unwrap();
        let back = crate::from_mtef_bytes(&bytes).unwrap();
        assert_eq!(back.to_latex().unwrap(), result.to_latex().unwrap());
    }
}

#[test]
fn partly_coloured_scripts_round_trip() {
    for latex in [
        r"{\textcolor[HTML]{ff0000}{x}}_{i}y",
        r"x_{\textcolor[HTML]{ff0000}{i}}y",
        r"\textcolor[HTML]{ff0000}{\frac{a}{\textcolor[HTML]{000000}{b}}}",
    ] {
        let first = crate::from_latex(latex).unwrap();
        let bytes = first.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
        let back = crate::from_mtef_bytes(&bytes).unwrap();
        assert_eq!(back.to_mathml().unwrap(), first.to_mathml().unwrap());
    }
}

#[test]
fn leading_scripts_survive_where_the_reader_moves_them() {
    for latex in [
        r"x{{}_{2}=}y",
        r"x{{}_{2}(}y)",
        r"x{{}_{2}\left(a\right)}",
        r"x{{}_{2}\log}a",
        r"x{{}_{2}y}",
    ] {
        let mut first = crate::reader::latex::LatexReader::parse_compact(latex, &Config::default())
            .unwrap()
            .expr;
        crate::ast::normalize::normalize_compact_expr(&mut first).unwrap();
        for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
            let bytes = MtefWriter::write_bytes(
                &first,
                &Config::default(),
                &MtefWriteOptions::new(version),
            )
            .unwrap();
            let back = crate::from_mtef_bytes(&bytes).unwrap();
            assert!(
                format!("{:?}", back.expr()).contains("PreScripts"),
                "{latex}"
            );
            let again = back.to_mtef_bytes(&MtefWriteOptions::new(version)).unwrap();
            assert_eq!(again, bytes, "{latex}");
        }
    }
}

#[test]
fn a_coloured_pile_is_stable_in_mtef_3() {
    let result =
        crate::from_latex(r"\textcolor{red}{\begin{array}{l} a \\ b \end{array}}").unwrap();
    let v3 = MtefWriteOptions::new(crate::MtefVersion::V3);
    let (bytes, diagnostics) = result.to_mtef_bytes_with_diagnostics(&v3).unwrap();
    assert!(diagnostics.iter().any(|d| d.message.contains("no colours")));
    let back = crate::from_mtef_bytes(&bytes).unwrap();
    assert_eq!(back.to_mtef_bytes(&v3).unwrap(), bytes);
}

/// Trees nested too deeply for normalization keep their zero spaces; writing
/// one back must not add another each time.
#[test]
fn zero_spaces_before_leading_scripts_do_not_accumulate() {
    let space = || MathNode::Space(crate::ast::Rational::zero());
    for base in [
        space(),
        MathNode::Row(Vec::new()),
        MathNode::Symbol(crate::ast::SymbolKind::Relation, "=".into()),
    ] {
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(crate::ast::SymbolKind::Open, "(".into()),
            MathNode::PreScripts {
                base: Box::new(base.clone()),
                pre_sub: Some(Box::new(MathNode::Number("1".into()))),
                pre_sup: None,
            },
            MathNode::Symbol(crate::ast::SymbolKind::BinaryOp, "+".into()),
        ]);
        for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
            let options = MtefWriteOptions::new(version);
            let config = Config::default();
            // Written and read without normalization.
            let mut compact = CompactExpr::from_math_expr(&expr).unwrap();
            let mut written = Vec::new();
            for _ in 0..4 {
                let bytes = MtefWriter::write_bytes(&compact, &config, &options).unwrap();
                compact = crate::reader::mtef::MtefReader::parse_bytes_compact(&bytes, &config)
                    .unwrap()
                    .expr;
                written.push(bytes);
            }
            assert_eq!(written[1], written[2], "{base:?}");
            assert_eq!(written[2], written[3], "{base:?}");
        }
    }
    // The space of a closed slot does not stand in for the one needed here.
    let fraction = MathExpr::inline(vec![
        MathNode::Fraction {
            kind: crate::ast::FractionKind::Normal,
            num: Box::new(MathNode::Identifier("a".into())),
            den: Box::new(MathNode::Row(vec![
                MathNode::Identifier("b".into()),
                space(),
            ])),
        },
        MathNode::PreScripts {
            base: Box::new(MathNode::Symbol(
                crate::ast::SymbolKind::Relation,
                "=".into(),
            )),
            pre_sub: Some(Box::new(MathNode::Number("1".into()))),
            pre_sup: None,
        },
    ]);
    let compact = CompactExpr::from_math_expr(&fraction).unwrap();
    let bytes = MtefWriter::write_bytes(&compact, &Config::default(), &MtefWriteOptions::default())
        .unwrap();
    let back = crate::from_mtef_bytes(&bytes).unwrap();
    assert!(
        format!("{:?}", back.expr()).contains("PreScripts"),
        "{:?}",
        back.expr()
    );
}

fn mtef_round_trip(latex: &str, version: crate::MtefVersion) -> (String, Vec<Diagnostic>) {
    let first = crate::from_latex(latex).unwrap();
    let options = MtefWriteOptions::new(version);
    let (bytes, diagnostics) = first.to_mtef_bytes_with_diagnostics(&options).unwrap();
    let back = crate::from_mtef_bytes(&bytes).unwrap();
    assert_eq!(back.to_mtef_bytes(&options).unwrap(), bytes, "{latex}");
    (back.to_latex().unwrap(), diagnostics)
}

#[test]
fn accents_on_styled_letters_stay_narrow() {
    use crate::MtefVersion::{V3, V5};
    for (latex, versions) in [
        (r"\tilde{\mathbf{A}}", &[V3, V5][..]),
        (r"\hat{\mathbf{A}}", &[V3, V5]),
        (r"\bar{\mathbf{A}}", &[V3, V5]),
        (r"\vec{\mathbf{A}}", &[V3, V5]),
        (r"\tilde{\mathrm{A}}", &[V3, V5]),
        (r"\hat{\boldsymbol{A}}", &[V3, V5]),
        (r"\hat{\hat{\mathbf{A}}}", &[V3, V5]),
        (r"\mathbf{Q}\hat{\mathbf{A}}\mathbf{B}", &[V3, V5]),
        (r"{\hat{\mathbf{A}}}_{1}^{2}", &[V3, V5]),
        (r"\hat{\mathfrak{A}}", &[V5]),
        (r"\hat{\mathcal{A}}", &[V5]),
        (r"\hat{\cancel{E}}", &[V3, V5]),
    ] {
        let expected = crate::from_latex(latex).unwrap().to_latex().unwrap();
        for &version in versions {
            let (back, diagnostics) = mtef_round_trip(latex, version);
            assert_eq!(back, expected, "{version:?}");
            assert!(diagnostics.is_empty(), "{latex}: {diagnostics:?}");
        }
    }
}

#[test]
fn wide_accents_stay_wide_and_narrow_ones_over_more_are_a_loss() {
    use crate::MtefVersion::V5;
    for latex in [
        r"\widetilde{\mathbf{A}}",
        r"\widehat{\mathbf{A}}",
        r"\overline{\mathbf{A}}",
        r"\overrightarrow{\mathbf{A}}",
        r"\widehat{\mathfrak{A}}",
        r"\widehat{xy}",
    ] {
        let (back, diagnostics) = mtef_round_trip(latex, V5);
        assert_eq!(back, latex);
        assert!(diagnostics.is_empty(), "{latex}: {diagnostics:?}");
    }
    for latex in [
        r"\hat{xy}",
        r"\tilde{xy}",
        r"\bar{xy}",
        r"\vec{xy}",
        r"\hat{\mathbf{A}_1}",
        r"\hat{\textcolor{red}{A}}",
    ] {
        let (_, diagnostics) = mtef_round_trip(latex, V5);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("narrow accent")),
            "{latex}: {diagnostics:?}"
        );
    }
    // A math alphabet MTEF lacks is its own loss; the accent still keeps.
    let (back, diagnostics) = mtef_round_trip(r"\hat{\mathbb{A}}", V5);
    assert_eq!(back, r"\hat{A}");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("math alphabet"))
    );
}

/// Primes alone on a styled letter keep the records they had.
#[test]
fn primes_on_styled_letters_are_not_embellishments() {
    let result = crate::from_latex(r"\mathbf{r}'").unwrap();
    let bytes = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert!(!bytes.windows(3).any(|w| w == [0x06, 0x00, 5]));
}

#[test]
fn stretchy_bar_accents_over_several_letters_are_not_a_loss() {
    for (tag, attribute, mark) in [
        ("mover", "accent", "&#xAF;"),
        ("mover", "accent", "&#x203E;"),
        ("mover", "accent", "&#x305;"),
        ("munder", "accentunder", "&#x203E;"),
        ("munder", "accentunder", "&#x332;"),
        ("munder", "accentunder", "_"),
    ] {
        let source = format!(
            "<math><{tag} {attribute}=\"true\"><mrow><mi>x</mi><mi>y</mi></mrow>\
             <mo>{mark}</mo></{tag}></math>"
        );
        let result = crate::from_mathml(&source).unwrap();
        let (_, diagnostics) = result
            .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::default())
            .unwrap();
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    }
    // The combining macron is `\bar`, which does not stretch.
    let (_, diagnostics) = mtef_round_trip(r"\bar{xy}", crate::MtefVersion::V5);
    assert!(!diagnostics.is_empty());
}

#[test]
fn strikes_under_an_accent_keep_their_direction() {
    for (latex, kind) in [
        (r"\hat{\cancel{E}}", 22),
        (r"\hat{\bcancel{E}}", 23),
        (r"\hat{\xcancel{E}}", 21),
        (r"\hat{\sout{E}}", 16),
    ] {
        let result = crate::from_latex(latex).unwrap();
        let bytes = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
        assert!(
            bytes.windows(6).any(|w| w == [0x06, 0, kind, 0x06, 0, 9]),
            "{latex}"
        );
        let (back, diagnostics) = mtef_round_trip(latex, crate::MtefVersion::V5);
        assert_eq!(back, result.to_latex().unwrap());
        assert!(diagnostics.is_empty(), "{latex}: {diagnostics:?}");
    }
}

#[test]
fn accents_around_styled_accents_are_byte_stable() {
    for latex in [
        r"\hat{\mathbf{\hat{\mathrm A}}}",
        r"\underleftarrow{\mathbf{\hat{\mathrm A}}}",
        r"\hat{\mathbf{\tilde{A}}}",
    ] {
        for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
            // Asserts that writing what was read gives the same bytes.
            mtef_round_trip(latex, version);
        }
    }
}

/// Reading can group what was written one level deeper (`x′` becomes a
/// superscript); a tree whose reading cannot be written under the same limit
/// is refused.
#[test]
fn whatever_is_written_can_be_written_again_at_the_depth_limit() {
    let (mut written, mut refused) = (0, 0);
    for roots in 0..60 {
        let mut node = MathNode::Row(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(crate::ast::SymbolKind::Ordinary, "\u{2032}".into()),
        ]);
        for _ in 0..roots {
            node = MathNode::Sqrt(Box::new(node));
        }
        let expr = CompactExpr::from_math_expr(&MathExpr::inline(vec![node])).unwrap();
        for limit in 1..=128 {
            let config = Config {
                max_depth: limit,
                ..Config::default()
            };
            for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
                let options = MtefWriteOptions::new(version);
                let Ok(bytes) = MtefWriter::write_bytes(&expr, &config, &options) else {
                    refused += 1;
                    continue;
                };
                written += 1;
                let back = crate::from_mtef_bytes_with(&bytes, &config).unwrap();
                if let Err(e) = back.to_mtef_bytes(&options) {
                    panic!("{version:?} limit {limit}, {roots} roots: {e}");
                }
            }
        }
    }
    assert!(written > 1000 && refused > 1000, "{written} {refused}");
}

/// A number with a script does not join a number in another colour: nothing
/// is written to keep them apart.
#[test]
fn numbers_in_two_colours_need_no_space() {
    let latex = r"\textcolor{red}{1}\textcolor{blue}{2}_a";
    let mtef = crate::convert(latex, crate::Format::LaTeX, crate::Format::Mtef).unwrap();
    assert!(!mtef.contains("9801ef"), "{mtef}");
    let mtef = crate::convert("1{2}_a", crate::Format::LaTeX, crate::Format::Mtef).unwrap();
    let mathml = crate::convert(&mtef, crate::Format::Mtef, crate::Format::MathML).unwrap();
    assert!(
        mathml.contains("<msub><mn>2</mn>") || mathml.contains("<msub><mn>12</mn>"),
        "{mathml}"
    );
}

/// Only the `s` of a possessive is kept apart from a letter: not that of a
/// function name, nor one of text.
#[test]
fn an_apostrophe_before_a_function_name_or_in_text_needs_no_space() {
    for latex in [
        r"x\char`'\sin y",
        r"\text{it'sy}",
        r"x\text{'sy}",
        r"\text{x'}sy",
    ] {
        let mtef = crate::convert(latex, crate::Format::LaTeX, crate::Format::Mtef).unwrap();
        assert!(!mtef.contains("9801ef"), "{latex}: {mtef}");
    }
    let mtef = crate::convert(r"x\char`'sy", crate::Format::LaTeX, crate::Format::Mtef).unwrap();
    assert!(mtef.contains("9801ef"), "{mtef}");
}

#[test]
fn smashed_content_is_written() {
    use crate::{DiagnosticKind, MtefVersion, MtefWriteOptions};
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let write = |latex: &str| {
            let (bytes, diagnostics) = crate::from_latex(latex)
                .unwrap()
                .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(version))
                .unwrap();
            let kinds: Vec<_> = diagnostics.iter().map(|d| d.kind).collect();
            let read = crate::from_mtef_bytes(&bytes).unwrap();
            (read.to_latex().unwrap(), kinds)
        };
        for smash in [r"\smash", r"\smash[b]", r"\smash[t]"] {
            let latex = format!(r"a{smash}{{\frac{{b}}{{c}}}}^{{2}}");
            let (read, kinds) = write(&latex);
            assert_eq!(read, r"a{\frac{b}{c}}^{2}", "{latex}");
            assert_eq!(kinds, [DiagnosticKind::Info], "{latex}");
        }
        let info = (String::from("x"), vec![DiagnosticKind::Info]);
        assert_eq!(write(r"x\vphantom{\frac{b}{c}}"), info);
        for phantom in [r"\phantom", r"\hphantom"] {
            let (read, kinds) = write(&format!("x{phantom}{{y}}"));
            assert_eq!((read.as_str(), kinds), ("x", vec![DiagnosticKind::Warning]));
        }
    }
}

/// All text, function names or upper-case Greek of one style are defined so in
/// MTEF 5; otherwise the loss is reported.
#[test]
fn styles_of_whole_equations_are_defined() {
    use crate::{DiagnosticKind, MtefVersion, MtefWriteOptions};
    let write = |latex: &str, version| {
        let (bytes, diagnostics) = crate::from_latex(latex)
            .unwrap()
            .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(version))
            .unwrap();
        let losses = diagnostics
            .iter()
            .filter(|d| d.kind == DiagnosticKind::Warning)
            .count();
        let read = crate::from_mtef_bytes(&bytes).unwrap();
        (read.to_latex().unwrap().trim().to_owned(), losses)
    };
    for (latex, plain) in [
        (r"\textbf{ab}", r"\text{ab}"),
        (r"\textit{ab}", r"\text{ab}"),
        (r"\mathbf{\sin}", r"\sin"),
        (r"\mathit{\sin}", r"\sin"),
        (r"\mathit{\Gamma}", r"\Gamma"),
    ] {
        assert_eq!(write(latex, MtefVersion::V5), (latex.to_owned(), 0));
        assert_eq!(
            write(latex, MtefVersion::V3),
            (plain.to_owned(), 1),
            "{latex}"
        );
    }
    for (latex, plain) in [
        (r"\textbf{ab}x\text{c}", r"\text{ab}x\text{c}"),
        (r"\mathbf{\sin}x+\cos x", r"\sin x+ \cos x"),
        (r"\mathit{\Gamma}\Delta", r"\Gamma\Delta"),
        (r"\mathit{12}", "12"),
    ] {
        assert_eq!(
            write(latex, MtefVersion::V5),
            (plain.to_owned(), 1),
            "{latex}"
        );
    }
    // Function names are upright in \boldsymbol.
    let bold = (r"\mathbf{\sin}".to_owned(), 0);
    assert_eq!(write(r"\boldsymbol{\sin}", MtefVersion::V5), bold);
}

#[test]
fn bold_numbers_and_symbols_read_back() {
    use crate::{MtefVersion, MtefWriteOptions};
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let write = |latex: &str| {
            crate::from_latex(latex)
                .unwrap()
                .to_mtef_bytes(&MtefWriteOptions::new(version))
                .unwrap()
        };
        let read = |latex: &str| {
            let result = crate::from_mtef_bytes(&write(latex)).unwrap();
            result.to_latex().unwrap().trim().replace("+ ", "+")
        };
        for latex in [
            r"\mathbf{12.5}",
            r"\mathbf{12}\mathbf{34}",
            r"1.5+\mathbf{12}.\mathbf{34}",
            r"\mathbf{12}.5",
            r"\mathbf{12}^{2}",
            r"\mathbf{7.8}12 .\mathbf{6}",
            r"\mathbf{7.8}9.6",
            r"\mathbf{x12}",
            r"\boldsymbol{+}",
        ] {
            assert_eq!(read(latex), latex, "{version:?}");
        }
        // The point in a bold number is not of the plain one after it.
        let split = r"\mathbf{7.8} 12 .6";
        assert_eq!(write(&read(split)), write(split));
        // A point at an end is not of the number.
        assert_eq!(read(r"\mathbf{.5}"), r".\mathbf{5}");
        assert_eq!(read(r"\mathbf{5.}"), r"\mathbf{5}.");
        // One digit is in the Vector-Matrix style (7, biased by 128).
        assert!(write(r"\mathbf{7}").contains(&135));
        assert!(write(r"\mathbf{\boldsymbol{12}34}").contains(&135));
        // Other symbols stay in their style, and no space is written.
        assert_eq!(write(r"\boldsymbol{\leq}"), write(r"\leq"));
        assert!(!write(r"x\mathbf{12}").contains(&152));
        // A minus, not a hyphen.
        let minus = write(r"\boldsymbol{-}");
        assert!(
            minus.windows(2).any(|code| code == [0x12, 0x22]),
            "{minus:?}"
        );
    }
    let options = MtefWriteOptions::new(MtefVersion::V5);
    for (content, read) in [
        ("<mn>.5</mn>", r".\mathbf{5}"),
        ("<mn>5.</mn>", r"\mathbf{5}."),
        ("<mn>1.2.3</mn>", r"\mathbf{1}.\mathbf{2}.\mathbf{3}"),
    ] {
        let mathml = format!(r#"<math><mstyle mathvariant="bold">{content}</mstyle></math>"#);
        let bytes = crate::from_mathml(&mathml)
            .unwrap()
            .to_mtef_bytes(&options)
            .unwrap();
        let back = crate::from_mtef_bytes(&bytes).unwrap();
        assert_eq!(back.to_latex().unwrap().trim(), read);
    }
    let greek = r"\mathit{\Gamma}\mathit{\Delta}";
    let bytes = crate::from_latex(greek)
        .unwrap()
        .to_mtef_bytes(&options)
        .unwrap();
    assert_eq!(
        crate::from_mtef_bytes(&bytes).unwrap().to_latex().unwrap(),
        greek
    );
}

/// How the characters of an MTEF 5 equation are drawn: the typeface, bits and
/// code of each CHAR record of a flat line, with the styles EQN_PREFS defines.
fn drawn(latex: &str) -> (Vec<(char, u8)>, usize) {
    let options = crate::MtefWriteOptions::new(crate::MtefVersion::V5);
    let (bytes, diagnostics) = crate::from_latex(latex)
        .unwrap()
        .to_mtef_bytes_with_diagnostics(&options)
        .unwrap();
    // The 12 styles end at the first size record; fnTEXT_FE may be undefined.
    let table = bytes
        .windows(3)
        .position(|w| w == [0x0F, 0x0C, 0x01])
        .unwrap()
        + 2;
    let mut at = table;
    let styles: Vec<u8> = (0..12)
        .map(|_| {
            let font = bytes[at];
            at += if font == 0 { 1 } else { 2 };
            if font == 0 { 0 } else { bytes[at - 1] }
        })
        .collect();
    let mut fonts = Vec::new();
    let mut chars = Vec::new();
    while at < bytes.len() {
        match bytes[at] {
            0 | 10..=14 => at += 1,
            1 => at += 2,
            8 => {
                fonts.push(bytes[at + 2]);
                at += 3;
            },
            2 => {
                let options = bytes[at + 1];
                let typeface = i32::from(bytes[at + 2]) - 128;
                let code = u16::from_le_bytes([bytes[at + 3], bytes[at + 4]]);
                at += if options & 4 != 0 { 6 } else { 5 };
                let bits = match typeface {
                    24 => continue,
                    1.. => styles[typeface as usize - 1],
                    _ => fonts[(-typeface) as usize - 1],
                };
                chars.push((char::from_u32(u32::from(code)).unwrap(), bits));
            },
            other => panic!("record {other} in {latex}"),
        }
    }
    let warnings = diagnostics
        .iter()
        .filter(|d| d.kind == crate::DiagnosticKind::Warning)
        .count();
    (chars, warnings)
}

/// Bytes, as the reader does not see the weight of punctuation: what is in a
/// style that is defined bold or italic is drawn so.
#[test]
fn defined_styles_draw_only_what_is_bold_or_italic() {
    let sin = |bits: u8| [('s', bits), ('i', bits), ('n', bits)];
    let line = |parts: &[&[(char, u8)]]| parts.concat();
    for (latex, chars, warnings) in [
        (
            r"\mathit{\sin}(x,y)",
            line(&[&sin(2), &[('(', 0), ('x', 2), (',', 0), ('y', 2), (')', 0)]]),
            0,
        ),
        (
            r"\mathbf{\sin}[x]",
            line(&[&sin(1), &[('[', 0), ('x', 2), (']', 0)]]),
            0,
        ),
        // Before the name, and bold punctuation.
        (
            r"!\mathbf{\sin}\boldsymbol{,}",
            line(&[&[('!', 0)], &sin(1), &[(',', 1)]]),
            0,
        ),
        (r"\sin;", line(&[&sin(0), &[(';', 0)]]), 0),
        // Names of two styles.
        (
            r"\mathbf{\sin}\sin;",
            line(&[&sin(0), &sin(0), &[(';', 0)]]),
            1,
        ),
        // Far East text has a style of its own, which is not defined.
        (r"\textbf{中}", vec![('中', 0)], 1),
        (r"\mathbf{中}", vec![('中', 0)], 1),
        (r"\mathrm{中}", vec![('中', 0)], 0),
        (r"\textbf{a中b}", vec![('a', 0), ('中', 0), ('b', 0)], 1),
        (
            r"\textbf{ab}\text{中}",
            vec![('a', 1), ('b', 1), ('中', 0)],
            0,
        ),
        // Letters of an alphabet MTEF has not, and an undefined command.
        (
            r"\mathit{\Gamma}\mathfrak{\Gamma}",
            vec![('Γ', 0), ('Γ', 0)],
            2,
        ),
        (r"\mathit{\Gamma}\Delta", vec![('Γ', 0), ('Δ', 0)], 1),
        (r"\mathit{\Gamma}x", vec![('Γ', 2), ('x', 2)], 0),
    ] {
        assert_eq!(drawn(latex), (chars, warnings), "{latex}");
    }
}
