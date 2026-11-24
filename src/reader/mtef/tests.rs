//! Focused tests of the MTEF reader. Byte layouts follow MTEFv3.md and
//! MTEFv5.md; whole equations extracted from real documents are tested in
//! `tests/mtef.rs`.

use super::*;
use crate::ast::compact::CompactNode;
use crate::ast::{LimitStyle, MathNode};
use crate::error::LimitKind;
use crate::writer::Writer;

// ── Builders ──────────────────────────────────────────────────────────────

/// MTEF 3 header (Windows, Equation Editor 3.0) followed by `body`.
fn v3(body: &[u8]) -> Vec<u8> {
    let mut data = vec![3, 1, 1, 3, 0];
    data.extend_from_slice(body);
    data
}

/// MTEF 5 header (Windows, MathType 6.0, key "DSMT6", display) and `body`.
fn v5(body: &[u8]) -> Vec<u8> {
    let mut data = vec![5, 1, 0, 6, 0];
    data.extend_from_slice(b"DSMT6\0");
    data.push(0);
    data.extend_from_slice(body);
    data
}

/// MTEF 3 CHAR record: tag with options, typeface + 128, 16-bit character.
fn c3(options: u8, typeface: u8, ch: char) -> Vec<u8> {
    let code = u16::try_from(u32::from(ch)).unwrap().to_le_bytes();
    vec![0x02 | (options << 4), 128 + typeface, code[0], code[1]]
}

/// MTEF 5 CHAR record with an MTCode value.
fn c5(options: u8, typeface: u8, ch: char) -> Vec<u8> {
    let code = u16::try_from(u32::from(ch)).unwrap().to_le_bytes();
    vec![0x02, options, 128 + typeface, code[0], code[1]]
}

/// An MTEF 3 LINE holding `content`.
fn line3(content: &[u8]) -> Vec<u8> {
    let mut data = vec![0x01];
    data.extend_from_slice(content);
    data.push(0);
    data
}

/// An MTEF 5 LINE holding `content`.
fn line5(content: &[u8]) -> Vec<u8> {
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(content);
    data.push(0);
    data
}

/// A whole MTEF 3 equation: FULL, one LINE, final END.
fn eq3(content: &[u8]) -> Vec<u8> {
    let mut body = vec![0x0A];
    body.extend(line3(content));
    body.push(0);
    v3(&body)
}

/// A whole MTEF 5 equation: FULL, one LINE, final END.
fn eq5(content: &[u8]) -> Vec<u8> {
    let mut body = vec![0x0A];
    body.extend(line5(content));
    body.push(0);
    v5(&body)
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn lenient() -> Config {
    Config::default().with_strictness(Strictness::Lenient)
}

fn latex(bytes: &[u8]) -> String {
    crate::from_mtef_bytes(bytes)
        .unwrap_or_else(|e| panic!("decode failed: {e}"))
        .to_latex()
        .unwrap()
}

fn nodes(bytes: &[u8]) -> Vec<MathNode> {
    MtefReader::parse_bytes(bytes, &Config::default())
        .unwrap_or_else(|e| panic!("decode failed: {e}"))
        .expr
        .nodes
}

fn strict_error(bytes: &[u8]) -> String {
    match MtefReader::parse_bytes(bytes, &Config::default()) {
        Err(Error::MtefParse { message }) => message,
        other => panic!("expected an MTEF parse error, got {other:?}"),
    }
}

fn has_warning(result: &ParseResult, needle: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|d| d.kind == DiagnosticKind::Warning && d.message.contains(needle))
}

// ── MTEF 3 ────────────────────────────────────────────────────────────────

#[test]
fn v3_function_letters_form_one_operator() {
    // r cos α: fnVARIABLE r, fnFUNCTION c o s (xfAUTO), fnLCGREEK α.
    let content = cat(&[
        &c3(1, 3, 'r'),
        &c3(1, 2, 'c'),
        &c3(1, 2, 'o'),
        &c3(1, 2, 's'),
        &c3(0, 4, '\u{3B1}'),
    ]);
    assert_eq!(latex(&eq3(&content)), r"r\cos \alpha");
    let nodes = nodes(&eq3(&content));
    assert!(matches!(&nodes[1], MathNode::Operator(op) if op.name == "cos"));
}

#[test]
fn v3_function_style_punctuation_is_not_an_operator() {
    let content = cat(&[
        &c3(0, 2, '('),
        &c3(1, 3, 'x'),
        &c3(0, 2, ','),
        &c3(1, 3, 'y'),
        &c3(0, 2, ')'),
    ]);
    assert_eq!(latex(&eq3(&content)), "(x,y)");
}

#[test]
fn v3_digits_merge_into_one_number() {
    let content = cat(&[&c3(0, 8, '1'), &c3(0, 8, '8'), &c3(0, 8, '0')]);
    assert!(matches!(&nodes(&eq3(&content))[0], MathNode::Number(n) if n == "180"));
}

#[test]
fn v3_superscript_attaches_to_the_preceding_item() {
    // R^2: tmSCRIPT (15) tvSUPER: SUB size, null subscript, superscript line.
    let script = cat(&[&[0x03, 15, 0, 0, 0x0B, 0x11], &line3(&c3(0, 8, '2')), &[0]]);
    let content = cat(&[&c3(1, 3, 'R'), &script, &[0x0A]]);
    assert_eq!(latex(&eq3(&content)), "R^{2}");
}

#[test]
fn v3_subscript_and_both_scripts() {
    let sub = cat(&[&[0x03, 15, 1, 0], &line3(&c3(0, 8, '1')), &[0x11, 0]]);
    assert_eq!(latex(&eq3(&cat(&[&c3(1, 3, 'z'), &sub]))), "z_{1}");
    let both = cat(&[
        &[0x03, 15, 2, 0],
        &line3(&c3(1, 3, 'j')),
        &line3(&c3(0, 2, '*')),
        &[0],
    ]);
    assert_eq!(
        latex(&eq3(&cat(&[&c3(0, 4, '\u{3B4}'), &both]))),
        r"\delta_j^*"
    );
}

#[test]
fn v3_fence_characters_after_the_slot_do_not_close_the_parent() {
    // tmBRACE (2) left only: main slot, then the brace glyph in fnEXPAND (22).
    // The equation continues after the template.
    let brace = cat(&[
        &[0x03, 2, 1, 0],
        &line3(&c3(1, 3, 'x')),
        &c3(0, 22, '{'),
        &[0],
    ]);
    let content = cat(&[&brace, &c3(0, 6, '='), &c3(0, 8, '1')]);
    assert_eq!(latex(&eq3(&content)), r"\left\{ x \right.= 1");
}

#[test]
fn v3_bars_with_private_use_glyphs() {
    let bars = cat(&[
        &[0x03, 4, 0, 0],
        &line3(&c3(1, 3, 'z')),
        &c3(0, 22, '\u{EC07}'),
        &c3(0, 22, '\u{EC08}'),
        &[0],
    ]);
    let result = MtefReader::parse_bytes(&eq3(&bars), &Config::default()).unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(latex(&eq3(&bars)), r"\left| z \right|");
}

#[test]
fn v3_root_slots_are_radicand_then_index() {
    let sqrt = cat(&[&[0x03, 13, 0, 0], &line3(&c3(0, 8, '2')), &[0x0B, 0x11, 0]]);
    assert_eq!(latex(&eq3(&sqrt)), r"\sqrt{2}");
    let nth = cat(&[
        &[0x03, 13, 1, 0],
        &line3(&c3(0, 8, '9')),
        &line3(&c3(0, 8, '3')),
        &[0],
    ]);
    assert_eq!(latex(&eq3(&nth)), r"\sqrt[3]{9}");
}

#[test]
fn v3_fraction_and_small_fraction() {
    let frac = cat(&[
        &[0x03, 14, 0, 0],
        &line3(&c3(1, 3, 'y')),
        &line3(&c3(1, 3, 'r')),
        &[0],
    ]);
    assert_eq!(latex(&eq3(&frac)), r"\frac{y}{r}");
    let small = cat(&[
        &[0x03, 14, 1, 0],
        &line3(&c3(0, 8, '1')),
        &line3(&c3(0, 8, '2')),
        &[0],
    ]);
    assert_eq!(latex(&eq3(&small)), r"\tfrac{1}{2}");
}

#[test]
fn v3_sum_slots_are_main_lower_upper_then_operator() {
    let sum = cat(&[
        &[0x03, 29, 1, 0],
        &line3(&c3(1, 3, 'a')),
        &line3(&c3(1, 3, 'k')),
        &line3(&c3(1, 3, 'n')),
        &[0x0D],
        &c3(0, 6, '\u{2211}'),
        &[0],
    ]);
    match &nodes(&eq3(&sum))[0] {
        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            assert_eq!(*op, '\u{2211}');
            assert_eq!(*limits, LimitStyle::AboveBelow);
            assert!(
                matches!(sub.as_deref(), Some(MathNode::Row(r)) if matches!(&r[..], [MathNode::Identifier(k)] if k == "k"))
            );
            assert!(
                matches!(sup.as_deref(), Some(MathNode::Row(r)) if matches!(&r[..], [MathNode::Identifier(n)] if n == "n"))
            );
            assert!(
                matches!(body.as_ref(), MathNode::Row(r) if matches!(&r[..], [MathNode::Identifier(a)] if a == "a"))
            );
        },
        other => panic!("expected Nary, got {other:?}"),
    }
}

#[test]
fn v3_limit_on_a_function_name() {
    let lim = cat(&[
        &[0x03, 39, 1, 0],
        &line3(&cat(&[&c3(1, 2, 'l'), &c3(1, 2, 'i'), &c3(1, 2, 'm')])),
        &line3(&cat(&[
            &c3(1, 3, 'n'),
            &c3(0, 6, '\u{2192}'),
            &c3(0, 6, '\u{221E}'),
        ])),
        &[0x11, 0],
    ]);
    assert_eq!(latex(&eq3(&lim)), r"\lim_{n\rightarrow \infty}");
}

#[test]
fn v3_embellishment_list_with_its_padding_byte() {
    // x with embTILDE: CHAR (xfAUTO | xfEMBELL), EMBELL 8, a zero byte, END.
    let content = cat(&[
        &c3(3, 3, 'x'),
        &[0x06, 8, 0, 0],
        &c3(0, 6, '+'),
        &c3(1, 3, 'y'),
    ]);
    assert_eq!(latex(&eq3(&content)), r"\tilde{x}+ y");
    let primed = cat(&[&c3(2, 3, 'f'), &[0x06, 5, 0, 0]]);
    assert_eq!(latex(&eq3(&primed)), r"f^{\prime}");
}

#[test]
fn v3_over_arrow_template() {
    let vector = cat(&[
        &[0x03, 47, 1, 0],
        &line3(&cat(&[&c3(1, 3, 'A'), &c3(1, 3, 'B')])),
        &[0],
    ]);
    assert_eq!(latex(&eq3(&vector)), r"\overrightarrow{AB}");
}

#[test]
fn v3_font_record_names_an_explicit_font() {
    // FONT 3 "Symbol" plain, then a CHAR in typeface -3.
    let font = cat(&[&[0x08, 3, 0], b"Symbol\0"]);
    let content = cat(&[&font, &[0x02, 128 - 3, 0xC0, 0x03]]);
    assert_eq!(latex(&eq3(&content)), r"\mathrm{\pi}");
}

#[test]
fn v3_matrix_with_partition_lines() {
    // 2 x 2, row partitions none, column partitions: solid line between the
    // columns (line 1 → bits 2-3 of the first byte).
    let cells = cat(&[
        &line3(&c3(1, 3, 'a')),
        &line3(&c3(1, 3, 'b')),
        &line3(&c3(1, 3, 'c')),
        &line3(&c3(1, 3, 'd')),
    ]);
    let matrix = cat(&[&[0x05, 1, 2, 1, 2, 2, 0x00, 0x04], &cells, &[0]]);
    let result = crate::from_mtef_bytes(&eq3(&matrix)).unwrap();
    let expr = result.compact_expr();
    let root = expr.arena.children(expr.roots)[0];
    match *expr.arena.node(root) {
        CompactNode::Array {
            num_cols,
            col_seps,
            row_seps,
            cells,
            array_style,
            ..
        } => {
            assert_eq!(num_cols, 2);
            assert_eq!(col_seps, 0b010);
            assert_eq!(row_seps, 0);
            assert!(array_style);
            assert_eq!(expr.arena.children(cells).len(), 4);
        },
        ref other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn v3_matrix_with_horizontal_partition_line_keeps_native_padding() {
    let cells = cat(&[
        &line3(&c3(1, 3, 'a')),
        &line3(&c3(1, 3, 'b')),
        &line3(&c3(1, 3, 'c')),
        &line3(&c3(1, 3, 'd')),
    ]);
    // 2 x 2, one solid horizontal partition between the rows, no vertical
    // partitions. MTEF has no separate edge-padding bit, so a partitioned
    // matrix projects to the padded native-array geometry.
    let matrix = cat(&[&[0x05, 1, 2, 1, 2, 2, 0x04, 0x00], &cells, &[0]]);
    let result = crate::from_mtef_bytes(&eq3(&matrix)).unwrap();
    let expr = result.compact_expr();
    let root = expr.arena.children(expr.roots)[0];
    assert!(matches!(
        *expr.arena.node(root),
        CompactNode::Array {
            array_style: true,
            col_seps: 0,
            row_seps: 0b010,
            ..
        }
    ));
}

#[test]
fn v3_pile_at_top_level() {
    let body = cat(&[
        &[0x0A, 0x04, 1, 1],
        &line3(&c3(1, 3, 'a')),
        &line3(&c3(1, 3, 'b')),
        &[0, 0],
    ]);
    assert_eq!(latex(&v3(&body)), r"\begin{array}{l} a \\ b \end{array}");
}

// ── MTEF 5 ────────────────────────────────────────────────────────────────

/// Definitions and preferences as MathType 5 writes them, before FULL.
fn v5_preamble() -> Vec<u8> {
    let mut data = Vec::new();
    data.push(0x13);
    data.extend_from_slice(b"WinAllBasicCodePages\0");
    data.extend_from_slice(&[0x11, 5]);
    data.extend_from_slice(b"Times New Roman\0");
    data.extend_from_slice(&[0x11, 3]);
    data.extend_from_slice(b"Symbol\0");
    // EQN_PREFS: options, 2 sizes ("12" pt, "58" %), 1 space ("0.2" in),
    // 3 styles (font 1 plain, font 1 plain, font 1 italic).
    data.extend_from_slice(&[0x12, 0x00]);
    data.extend_from_slice(&[2, 0x21, 0x2F, 0x45, 0x8F]);
    data.extend_from_slice(&[1, 0x00, 0xA2, 0xF0]);
    data.extend_from_slice(&[3, 1, 0, 1, 0, 1, 2]);
    data
}

#[test]
fn v5_definitions_and_preferences_do_not_hide_the_equation() {
    let mut body = v5_preamble();
    body.push(0x0A);
    body.extend(line5(&cat(&[
        &c5(0x04, 4, '\u{3B1}'),
        &[0x61],
        &c5(0x04, 6, '='),
        &[0x3D],
        &c5(0, 3, 'r'),
    ])));
    body.push(0);
    assert_eq!(latex(&v5(&body)), r"\alpha= r");
}

#[test]
fn v5_function_start_splits_adjacent_names() {
    let content = cat(&[
        &c5(0x02, 2, 's'),
        &c5(0, 2, 'i'),
        &c5(0, 2, 'n'),
        &c5(0x02, 2, 'c'),
        &c5(0, 2, 'o'),
        &c5(0, 2, 's'),
        &c5(0, 3, 'x'),
    ]);
    assert_eq!(latex(&eq5(&content)), r"\sin \cos x");
}

#[test]
fn v5_scripts_and_leading_scripts() {
    let sup = cat(&[
        &[0x03, 0, 28, 0, 0, 0x0B, 0x01, 0x01],
        &line5(&c5(0, 8, '2')),
        &[0],
    ]);
    assert_eq!(latex(&eq5(&cat(&[&c5(0, 3, 'x'), &sup]))), "x^{2}");
    // tvSU_PRECEDES: the subscript goes before the item that follows.
    let pre = cat(&[
        &[0x03, 0, 27, 1, 0],
        &line5(&c5(0, 8, '1')),
        &[0x01, 0x01, 0],
    ]);
    let nodes = nodes(&eq5(&cat(&[&pre, &c5(0, 3, 'X')])));
    assert!(
        matches!(&nodes[0], MathNode::PreScripts { base, pre_sub: Some(_), pre_sup: None }
            if matches!(base.as_ref(), MathNode::Identifier(x) if x == "X")),
        "{nodes:?}"
    );
}

#[test]
fn v5_integral_limits_use_bits_0x10_and_0x20() {
    // tmINTEG single, lower + upper, integral-style: variation 0x31.
    let integral = cat(&[
        &[0x03, 0, 15, 0x31, 0],
        &line5(&c5(0, 3, 'f')),
        &line5(&c5(0, 8, '0')),
        &line5(&c5(0, 8, '1')),
        &c5(0, 6, '\u{222B}'),
        &[0],
    ]);
    assert_eq!(latex(&eq5(&integral)), r"\int\nolimits_{0}^{1} f");
    // Two-byte variation: 0x81 0x01 = 0x0101 (single sign, expanding).
    let expanding = cat(&[
        &[0x03, 0, 15, 0x81, 0x01, 0],
        &line5(&c5(0, 3, 'g')),
        &[0x01, 0x01, 0x01, 0x01, 0],
    ]);
    assert!(matches!(
        &nodes(&eq5(&expanding))[0],
        MathNode::Nary {
            op: '\u{222B}',
            sub: None,
            sup: None,
            ..
        }
    ));
}

#[test]
fn v5_sum_style_operator_in_its_own_slot() {
    // tmSUMOP: null main slot, lower, upper, then the operator as a line.
    let sumop = cat(&[
        &[0x03, 0, 22, 0x70, 0, 0x01, 0x01],
        &line5(&c5(0, 3, 'i')),
        &line5(&c5(0, 3, 'n')),
        &line5(&cat(&[&c5(0x04, 6, '\u{2211}'), &[0xE5]])),
        &[0],
    ]);
    assert!(matches!(
        &nodes(&eq5(&sumop))[0],
        MathNode::Nary {
            op: '\u{2211}',
            limits: LimitStyle::AboveBelow,
            ..
        }
    ));
}

#[test]
fn v5_interval_fences() {
    let interval = cat(&[
        &[0x03, 0, 9, 0x30, 0],
        &line5(&c5(0, 8, '1')),
        &c5(0, 22, '('),
        &c5(0, 22, ']'),
        &[0],
    ]);
    assert_eq!(latex(&eq5(&interval)), r"\left( 1 \right]");
}

#[test]
fn v5_dirac_bra_ket() {
    let braket = cat(&[
        &[0x03, 0, 30, 3, 0],
        &line5(&c5(0, 3, 'a')),
        &line5(&c5(0, 3, 'b')),
        &[0],
    ]);
    assert_eq!(
        latex(&eq5(&braket)),
        r"\left\langle a \middle| b \right\rangle"
    );
}

#[test]
fn v5_horizontal_brace_with_label() {
    let brace = cat(&[
        &[0x03, 0, 24, 1, 0],
        &line5(&c5(0, 3, 'a')),
        &line5(&c5(0, 3, 'b')),
        &c5(0, 22, '\u{FE37}'),
        &[0],
    ]);
    assert_eq!(latex(&eq5(&brace)), r"{\overbrace{a}}^b");
}

#[test]
fn v5_embellishments() {
    let dot = cat(&[&c5(0x01, 3, 'x'), &[0x06, 0, 2, 0]]);
    assert_eq!(latex(&eq5(&dot)), r"\dot{x}");
    let not = cat(&[&c5(0x01, 6, '='), &[0x06, 0, 10, 0]]);
    assert_eq!(latex(&eq5(&not)), r"\neq");
    let under = cat(&[&c5(0x01, 3, 'y'), &[0x06, 0, 30, 0]]);
    assert_eq!(latex(&eq5(&under)), r"\utilde{y}");
}

#[test]
fn v5_explicit_font_keeps_upright_letters() {
    // FONT_DEF 1 (encoding 5), FONT_STYLE_DEF 1 → font 1 plain, then a CHAR
    // in typeface -1 (written as 127).
    let mut body = vec![0x13];
    body.extend_from_slice(b"WinAllCodePages\0");
    body.extend_from_slice(&[0x11, 5]);
    body.extend_from_slice(b"SimSun\0");
    body.push(0x0A);
    body.extend(line5(&cat(&[
        &[0x08, 1, 0],
        &[0x02, 0, 127, b'A', 0],
        &[0x02, 0, 127, b'H', 0],
        &c5(0, 6, '='),
    ])));
    body.push(0);
    assert_eq!(latex(&v5(&body)), r"\mathrm{AH}=");
}

#[test]
fn v5_font_position_without_mtcode_uses_the_font_encoding() {
    // mtefOPT_CHAR_ENC_NO_MTCODE | mtefOPT_CHAR_ENC_CHAR_8 in the Symbol
    // style: font position 'a' is α in the Symbol encoding.
    let content = [0x02, 0x24, 128 + 4, b'a'];
    assert_eq!(latex(&eq5(&content)), r"\alpha");
}

#[test]
fn v5_text_runs_and_spaces() {
    let content = cat(&[
        &c5(0, 1, 'i'),
        &c5(0, 1, 'f'),
        &c5(0, 24, '\u{EF04}'),
        &c5(0, 3, 'x'),
    ]);
    assert_eq!(latex(&eq5(&content)), r"\text{if}\;x");
}

#[test]
fn v5_size_color_ruler_and_future_records_are_consumed() {
    let content = cat(&[
        // SIZE: explicit point size, large delta, small delta.
        &[
            0x09,
            101,
            0x80,
            0x01,
            0x09,
            100,
            1,
            0x10,
            0x27,
            0x09,
            1,
            128 + 2,
        ],
        // COLOR_DEF: CMYK with a name, RGB red; COLOR selects the red one.
        &[0x10, 0x05, 0, 0, 0, 0, 0, 0, 0xE8, 0x03],
        b"Black\0",
        &[0x10, 0x00, 0xE8, 0x03, 0, 0, 0, 0, 0x0F, 2],
        // A future-expansion record of 3 bytes.
        &[100, 3, 1, 2, 3],
        &c5(0, 3, 'x'),
    ]);
    // A LINE with a ruler: two tab stops of 3 bytes each.
    let mut body = vec![0x0A, 0x01, 0x02, 0x07, 2, 0, 0x10, 0, 3, 0x20, 0];
    body.extend_from_slice(&content);
    body.extend_from_slice(&[0, 0]);
    let result = MtefReader::parse_bytes(&v5(&body), &Config::default()).unwrap();
    assert_eq!(
        result.expr.nodes,
        [MathNode::Color {
            color: "#ff0000".into(),
            content: Box::new(MathNode::Identifier("x".into())),
        }]
    );
    let infos: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Info)
        .collect();
    // The future-expansion record only.
    assert_eq!(infos.len(), 1, "{infos:?}");
}

/// COLOR_DEF (RGB red) as definition 1, then `content` in one line.
fn coloured(content: &[&[u8]]) -> crate::ast::MathExpr {
    let mut body = vec![0x10, 0, 0xE8, 0x03, 0, 0, 0, 0, 0x0A, 1, 0];
    body.extend(cat(content));
    body.extend_from_slice(&[0, 0]);
    MtefReader::parse_bytes(&v5(&body), &Config::default())
        .unwrap()
        .expr
}

fn red(content: MathNode) -> MathNode {
    MathNode::Color {
        color: "#ff0000".into(),
        content: Box::new(content),
    }
}

/// tmSUB (27) holding one character, with `before` just inside the template.
fn sub5(before: &[u8], ch: char) -> Vec<u8> {
    cat(&[
        &[3, 0, 27, 0, 0, 1, 0],
        before,
        &c5(0, 3, ch),
        &[0, 1, 1, 0],
    ])
}

#[test]
fn a_script_keeps_its_own_colour() {
    const RED: &[u8] = &[0x0F, 1];
    const BLACK: &[u8] = &[0x0F, 0];
    let id = |s: &str| MathNode::Identifier(s.into());
    let sub = |base, sub| MathNode::Subscript {
        base: Box::new(base),
        sub: Box::new(sub),
    };
    let row = |n| MathNode::Row(vec![n]);
    let x = c5(0, 3, 'x');
    // Black script on a red base, and red script on a black one.
    let expr = coloured(&[RED, &x, BLACK, &sub5(&[], 'i')]);
    assert_eq!(expr.nodes, [sub(red(id("x")), row(id("i")))]);
    let expr = coloured(&[&x, RED, &sub5(&[], 'i'), BLACK]);
    assert_eq!(expr.nodes, [sub(id("x"), red(row(id("i"))))]);
    // The colour changes inside the slot.
    let expr = coloured(&[&x, &sub5(RED, 'i'), BLACK]);
    assert_eq!(expr.nodes, [sub(id("x"), row(red(id("i"))))]);
    // One colour for both is one colour node.
    let expr = coloured(&[RED, &x, &sub5(&[], 'i'), BLACK, &c5(0, 3, 'y')]);
    assert_eq!(expr.nodes, [red(sub(id("x"), row(id("i")))), id("y")]);
}

fn latex5(content: &[&[u8]]) -> String {
    let mut body = vec![0x0A, 1, 0];
    body.extend(cat(content));
    body.extend_from_slice(&[0, 0]);
    crate::from_mtef_bytes(&v5(&body))
        .unwrap()
        .to_latex()
        .unwrap()
}

/// tmSUP (28) with `tvSU_PRECEDES`, holding one character.
fn pre_sup5(ch: char) -> Vec<u8> {
    cat(&[&[3, 0, 28, 1, 0, 1, 1, 1, 0], &c5(0, 8, ch), &[0, 0]])
}

#[test]
fn leading_scripts_lead_letters_and_follow_otherwise() {
    let (x, c) = (c5(0, 3, 'x'), c5(0, 3, 'C'));
    let plus = c5(0, 6, '+');
    assert_eq!(latex5(&[&pre_sup5('2'), &c]), "{{}^{2}C}");
    assert_eq!(latex5(&[&plus, &pre_sup5('2'), &c]), "+ {{}^{2}C}");
    assert_eq!(latex5(&[&x, &pre_sup5('2'), &plus, &c]), "x^{2}+ C");
    assert_eq!(latex5(&[&x, &pre_sup5('2')]), "x^{2}");
    // An operator before it carries no script.
    assert_eq!(latex5(&[&x, &plus, &pre_sup5('2')]), "x+{}^{2}");
}

#[test]
fn decimals_primes_and_quotes() {
    let n = |ch| c5(0, 8, ch);
    let f = |ch| c5(0, 2, ch);
    let t = |ch| c5(0, 1, ch);
    let x = c5(0, 3, 'x');
    assert_eq!(latex5(&[&n('3'), &f('.'), &n('6')]), "3.6");
    // Not a decimal: no digit follows, or the number has a point already.
    assert_eq!(latex5(&[&n('3'), &f('.')]), "3 .");
    let result = latex5(&[&n('1'), &f('.'), &n('2'), &f('.'), &n('3')]);
    assert!(result.starts_with("1.2"), "{result}");
    assert_eq!(latex5(&[&x, &f('\''), &f('\'')]), r"x^{\prime\prime}");
    assert_eq!(latex5(&[&x, &f('\''), &sub5(&[], 'i')]), r"x_i^{\prime}");
    assert_eq!(latex5(&[&x, &f('~'), &x, &f('-'), &x]), r"x\sim x- x");
    // Text keeps its characters; a double quote may be a quotation mark.
    assert_eq!(latex5(&[&t('a'), &t('-'), &t('\'')]), r"\text{a-'}");
    assert_eq!(latex5(&[&f('"'), &x, &f('"')]), "\\char`\"x\\char`\"");
}

#[test]
fn letters_stay_separate_identifiers() {
    let mut body = vec![0x0A, 1, 0];
    body.extend(cat(&[&c5(0, 3, 'd'), &c5(0, 3, 't')]));
    body.extend_from_slice(&[0, 0]);
    let result = crate::from_mtef_bytes(&v5(&body)).unwrap();
    assert!(result.to_mathml().unwrap().contains("<mi>d</mi><mi>t</mi>"));
}

// ── Headers ───────────────────────────────────────────────────────────────

fn ole(mtef: &[u8], version: u32, declared: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&28u16.to_le_bytes());
    data.extend_from_slice(&version.to_le_bytes());
    data.extend_from_slice(&0xC1A5u16.to_le_bytes());
    data.extend_from_slice(&declared.to_le_bytes());
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(mtef);
    data
}

#[test]
fn equation_native_header_is_detected_and_reported() {
    let mtef = eq3(&c3(0, 8, '7'));
    let len = u32::try_from(mtef.len()).unwrap();
    let result = crate::from_mtef_bytes(&ole(&mtef, 0x0002_0000, len)).unwrap();
    assert_eq!(result.to_latex().unwrap(), "7");
    let info = result.mtef_info().unwrap();
    assert_eq!(info.version, MtefVersion::V3);
    assert_eq!(info.product, MtefProduct::EquationEditor);
    assert_eq!(info.platform, MtefPlatform::Windows);
    let header = info.ole_header.unwrap();
    assert_eq!(header.object_size, len);
    assert_eq!(header.clipboard_format, 0xC1A5);
}

#[test]
fn equation_native_header_is_validated() {
    let mtef = eq3(&c3(0, 8, '7'));
    let len = u32::try_from(mtef.len()).unwrap();
    // Declared length larger than the data: refused in both modes.
    assert!(strict_error(&ole(&mtef, 0x0002_0000, len + 1)).contains("declares"));
    let bad = MtefReader::parse_bytes(&ole(&mtef, 0x0002_0000, len + 1), &lenient()).unwrap();
    assert!(bad.expr.nodes.is_empty() && bad.has_warnings());
    // Unknown header version: Strict refuses, Lenient warns and decodes.
    assert!(strict_error(&ole(&mtef, 0x0003_0000, len)).contains("header version"));
    let warned = MtefReader::parse_bytes(&ole(&mtef, 0x0003_0000, len), &lenient()).unwrap();
    assert!(has_warning(&warned, "header version"));
    assert_eq!(warned.expr.nodes.len(), 1);
    // Data after the declared object is ignored with a note.
    let mut padded = ole(&mtef, 0x0002_0000, len);
    padded.extend_from_slice(&[0xFF; 4]);
    let result = MtefReader::parse_bytes(&padded, &Config::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Info)
    );
    // A header cut short.
    assert!(strict_error(&ole(&[], 0x0002_0000, 0)[..20]).contains("truncated"));
}

#[test]
fn v5_header_fields_are_reported() {
    let result = crate::from_mtef_bytes(&eq5(&c5(0, 3, 'x'))).unwrap();
    let info = result.mtef_info().unwrap();
    assert_eq!(info.version, MtefVersion::V5);
    assert_eq!(info.product, MtefProduct::MathType);
    assert_eq!((info.product_version, info.product_subversion), (6, 0));
    assert_eq!(info.application_key.as_deref(), Some("DSMT6"));
    assert_eq!(info.inline, Some(false));
    assert!(info.ole_header.is_none());
}

#[test]
fn unsupported_versions_are_refused() {
    assert!(strict_error(&[4, 1, 0, 3, 5, 0x0A]).contains("version 4 is not supported"));
    assert!(strict_error(&[0x41, 0x42]).contains("not MTEF data"));
}

#[test]
fn hex_input_still_works() {
    let hex: String = eq5(&c5(0, 3, 'x'))
        .iter()
        .map(|b| format!("{b:02X} "))
        .collect();
    assert_eq!(crate::from_mtef(&hex).unwrap().to_latex().unwrap(), "x");
    assert!(MtefReader::parse("zz", &Config::default()).is_err());
    assert!(MtefReader::parse("zz", &lenient()).unwrap().has_warnings());
}

#[test]
fn empty_input_is_an_empty_expression() {
    assert!(nodes(&[]).is_empty());
}

// ── Strictness and hostile input ──────────────────────────────────────────

#[test]
fn unknown_record_types_are_refused_in_strict_mode() {
    let v5_unknown = eq5(&cat(&[&c5(0, 3, 'x'), &[50]]));
    assert!(strict_error(&v5_unknown).contains("record type 50"));
    let lenient_result = MtefReader::parse_bytes(&v5_unknown, &lenient()).unwrap();
    assert!(lenient_result.expr.nodes.is_empty() && lenient_result.has_warnings());
    let v3_unknown = eq3(&cat(&[&c3(0, 3, 'x'), &[0x0F]]));
    assert!(strict_error(&v3_unknown).contains("record type 15"));
}

#[test]
fn unknown_templates_are_refused_in_strict_mode() {
    let unknown = cat(&[&[0x03, 0, 60, 0, 0], &line5(&c5(0, 3, 'x')), &[0]]);
    assert!(strict_error(&eq5(&unknown)).contains("selector 60"));
    let result = MtefReader::parse_bytes(&eq5(&unknown), &lenient()).unwrap();
    assert!(has_warning(&result, "selector 60"));
    assert!(format!("{:?}", result.expr.nodes).contains("\"x\""));
}

#[test]
fn unknown_embellishments_and_typefaces_are_refused_in_strict_mode() {
    let embell = cat(&[&c5(0x01, 3, 'x'), &[0x06, 0, 99, 0]]);
    assert!(strict_error(&eq5(&embell)).contains("embellishment type 99"));
    let result = MtefReader::parse_bytes(&eq5(&embell), &lenient()).unwrap();
    assert!(has_warning(&result, "embellishment type 99"));
    assert!(strict_error(&eq5(&c5(0, 15, 'x'))).contains("typeface style 15"));
}

#[test]
fn extra_template_slots_are_refused_in_strict_mode() {
    let fraction = cat(&[
        &[0x03, 0, 11, 0, 0],
        &line5(&c5(0, 8, '1')),
        &line5(&c5(0, 8, '2')),
        &line5(&c5(0, 8, '3')),
        &[0],
    ]);
    assert!(strict_error(&eq5(&fraction)).contains("more than its 2 slot"));
    let result = MtefReader::parse_bytes(&eq5(&fraction), &lenient()).unwrap();
    assert!(format!("{:?}", result.expr.nodes).contains("\"3\""));
}

#[test]
fn data_after_the_equation() {
    let mut padded = eq5(&c5(0, 3, 'x'));
    padded.extend_from_slice(&[0, 0, 0]);
    assert_eq!(latex(&padded), "x");
    let mut garbage = eq5(&c5(0, 3, 'x'));
    garbage.extend_from_slice(&[0, 7, 0]);
    assert!(strict_error(&garbage).contains("follow the end"));
    assert!(
        MtefReader::parse_bytes(&garbage, &lenient())
            .unwrap()
            .has_warnings()
    );
}

#[test]
fn missing_final_end_is_refused_in_strict_mode() {
    let mut data = eq5(&c5(0, 3, 'x'));
    data.pop();
    assert!(strict_error(&data).contains("without the END record"));
    // Lenient mode keeps the complete line and warns.
    let result = MtefReader::parse_bytes(&data, &lenient()).unwrap();
    assert_eq!(result.expr.nodes.len(), 1);
    assert!(has_warning(&result, "without the END record"));
    data.pop();
    assert!(strict_error(&data).contains("missing its END"));
}

#[test]
fn every_truncation_is_refused_without_panicking() {
    let fraction = cat(&[
        &[0x03, 0, 11, 0, 0],
        &line5(&cat(&[&c5(0x01, 3, 'x'), &[0x06, 0, 2, 0]])),
        &line5(&c5(0, 8, '2')),
        &[0],
    ]);
    let mut full = v5_preamble();
    full.push(0x0A);
    full.extend(line5(&fraction));
    full.push(0);
    let full = v5(&full);
    if let Err(e) = MtefReader::parse_bytes(&full, &Config::default()) {
        panic!("the complete stream must decode: {e}");
    }
    // Every prefix that cuts a record must fail in Strict mode; the only
    // accepted prefix is the one that drops just the final END.
    for len in 1..full.len() - 1 {
        let prefix = &full[..len];
        assert!(
            MtefReader::parse_bytes(prefix, &Config::default()).is_err(),
            "prefix of {len} bytes was accepted"
        );
        let lenient = MtefReader::parse_bytes(prefix, &lenient()).unwrap();
        assert!(lenient.has_warnings(), "prefix of {len} bytes: no warning");
    }
}

#[test]
fn matrix_cells_are_never_allocated_beyond_the_input() {
    // A 255 x 255 matrix with no cells, repeated: a reader that allocates the
    // declared cells would create about 65 000 nodes per 9 input bytes.
    let one = [0x05, 0, 1, 1, 1, 255, 255, 0, 0, 0];
    let mut bomb = Vec::new();
    let partition_bytes = 2 * 64; // (255 + 1) lines, 2 bits each
    for _ in 0..1000 {
        bomb.extend_from_slice(&one[..7]);
        bomb.extend(std::iter::repeat_n(0u8, partition_bytes));
        bomb.push(0);
    }
    let data = eq5(&bomb);
    assert!(data.len() < 140 * 1024);
    assert!(strict_error(&data).contains("255 x 255"));
    let result = MtefReader::parse_bytes_compact(&data, &lenient()).unwrap();
    assert!(
        result.expr.arena.node_count() < data.len(),
        "{} nodes for {} bytes",
        result.expr.arena.node_count(),
        data.len()
    );
}

#[test]
fn matrix_cell_count_must_match_in_strict_mode() {
    let matrix = cat(&[
        &[0x05, 0, 0, 1, 1, 2, 2, 0, 0],
        &line5(&c5(0, 3, 'a')),
        &line5(&c5(0, 3, 'b')),
        &line5(&c5(0, 3, 'c')),
        &[0],
    ]);
    assert!(strict_error(&eq5(&matrix)).contains("2 x 2"));
    // Lenient mode completes the last row only.
    let result = MtefReader::parse_bytes_compact(&eq5(&matrix), &lenient()).unwrap();
    let expr = &result.expr;
    let root = expr.arena.children(expr.roots)[0];
    assert!(
        matches!(*expr.arena.node(root), CompactNode::Array { cells, .. } if expr.arena.children(cells).len() == 4)
    );
}

#[test]
fn node_limit_is_enforced() {
    let content: Vec<u8> = (0..200).flat_map(|_| c5(0, 3, 'x')).collect();
    let config = Config::default().with_max_nodes(Some(50));
    match MtefReader::parse_bytes(&eq5(&content), &config) {
        Err(Error::LimitExceeded {
            kind: LimitKind::Nodes,
            limit: 50,
        }) => {},
        other => panic!("expected the node limit, got {other:?}"),
    }
    // The limit holds in Lenient mode too.
    let lenient = lenient().with_max_nodes(Some(50));
    assert!(matches!(
        MtefReader::parse_bytes(&eq5(&content), &lenient),
        Err(Error::LimitExceeded { .. })
    ));
    assert!(MtefReader::parse_bytes(&eq5(&content), &Config::default()).is_ok());
}

/// `depth` nested fractions, each with the next one in its numerator.
fn nested_fractions(depth: usize) -> Vec<u8> {
    let mut inner = c5(0, 8, '1');
    for _ in 0..depth {
        inner = cat(&[
            &[0x03, 0, 11, 0, 0],
            &line5(&inner),
            &line5(&c5(0, 8, '2')),
            &[0],
        ]);
    }
    eq5(&inner)
}

#[test]
fn depth_limit_is_enforced() {
    assert!(MtefReader::parse_bytes(&nested_fractions(20), &Config::default()).is_ok());
    let config = Config::default().with_max_depth(10);
    assert!(matches!(
        MtefReader::parse_bytes(&nested_fractions(20), &config),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            limit: 10
        })
    ));
    // Deep input is refused, never a stack overflow, even with a large limit.
    let config = Config::default().with_max_depth(usize::MAX);
    assert!(matches!(
        MtefReader::parse_bytes(&nested_fractions(5000), &config),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            ..
        })
    ));
}

#[test]
fn input_size_limit_is_enforced() {
    let config = Config::default().with_max_input_bytes(Some(8));
    assert!(matches!(
        crate::from_mtef_bytes_with(&eq5(&c5(0, 3, 'x')), &config),
        Err(Error::InputTooLarge { limit: 8, .. })
    ));
}

#[test]
fn embell_outside_a_character_is_refused_in_strict_mode() {
    let stray = cat(&[&c5(0, 3, 'x'), &[0x06, 0, 2]]);
    assert!(strict_error(&eq5(&stray)).contains("EMBELL"));
}

#[test]
fn control_characters_are_refused_in_strict_mode() {
    assert!(strict_error(&eq5(&c5(0, 3, '\u{1}'))).contains("control character"));
    let result = MtefReader::parse_bytes(&eq5(&c5(0, 3, '\u{1}')), &lenient()).unwrap();
    assert!(has_warning(&result, "control character"));
}

/// MTEF 5 superscript template with an empty-line superscript of `x`.
fn superscript() -> Vec<u8> {
    cat(&[
        &[0x03, 0, 28, 0, 0, 0x01, 0x01],
        &line5(&c5(0, 8, '2')),
        &[0],
    ])
}

#[test]
fn script_chains_are_bounded_by_the_ast_depth() {
    // Every superscript template wraps the item before it, so a run of them
    // in one line nests without nesting object lists.
    // 30 000 templates overflowed the stack before the bound existed; under
    // Miri a chain just past the limit exercises the same check in seconds.
    let length = if cfg!(miri) { 200 } else { 30_000 };
    let chain: Vec<u8> = cat(&[&c5(0, 3, 'x'), &superscript().repeat(length)]);
    let data = eq5(&chain);
    assert!(data.len() < 500 * 1024);
    for result in [
        MtefReader::parse_bytes(&data, &Config::default()).map(|_| ()),
        MtefReader::parse_bytes(&data, &lenient()).map(|_| ()),
        crate::from_mtef_bytes(&data).map(|_| ()),
    ] {
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::Depth,
                    limit: 128
                })
            ),
            "{result:?}"
        );
    }
    let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
    assert!(matches!(
        MtefReader::parse(&hex, &Config::default()),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            ..
        })
    ));
    // Up to the limit the chain decodes.
    let short = eq5(&cat(&[&c5(0, 3, 'x'), &superscript().repeat(100)]));
    assert!(crate::from_mtef_bytes(&short).is_ok());
}

#[test]
fn embellishment_chains_are_bounded_by_the_ast_depth() {
    let mut chain = c5(0x01, 3, 'x');
    for _ in 0..if cfg!(miri) { 200 } else { 1000 } {
        chain.extend_from_slice(&[0x06, 0, 2]);
    }
    chain.push(0);
    assert!(matches!(
        MtefReader::parse_bytes(&eq5(&chain), &Config::default()),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            ..
        })
    ));
}

#[test]
fn ast_depth_limit_is_configurable() {
    let data = eq5(&cat(&[&c5(0, 3, 'x'), &superscript().repeat(20)]));
    assert!(crate::from_mtef_bytes(&data).is_ok());
    let config = Config::default().with_max_depth(5);
    assert!(matches!(
        crate::from_mtef_bytes_with(&data, &config),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            limit: 5
        })
    ));
}

#[test]
fn long_lists_are_refused_not_truncated() {
    // 70 000 characters in one line: more than a node can hold.
    let content = c5(0, 3, 'x').repeat(70_000);
    let data = eq5(&content);
    for config in [Config::default(), lenient()] {
        assert!(matches!(
            MtefReader::parse_bytes(&data, &config),
            Err(Error::LimitExceeded {
                kind: LimitKind::ListLength,
                limit: 65_535
            })
        ));
    }
    // Spaces and markers that produce no node still count as records.
    let markers = c5(0, 23, '\u{EF00}').repeat(70_000);
    assert!(matches!(
        MtefReader::parse_bytes(&eq5(&markers), &Config::default()),
        Err(Error::LimitExceeded {
            kind: LimitKind::ListLength,
            ..
        })
    ));
}

#[test]
fn box_without_sides_is_its_content() {
    let boxed = |var: u8| cat(&[&[0x03, 0, 37, var, 0], &line5(&c5(0, 3, 'x')), &[0]]);
    assert_eq!(latex(&eq5(&boxed(0x1E))), r"\boxed{x}");
    assert_eq!(latex(&eq5(&boxed(0))), "x");
    let partial = MtefReader::parse_bytes(&eq5(&boxed(0x06)), &Config::default()).unwrap();
    assert!(has_warning(&partial, "only some of its sides"));
}

#[test]
fn empty_limit_lines_are_dropped() {
    // tmLIM with an upper limit present but empty, as MathType 6 writes it.
    let lim = cat(&[
        &[0x03, 0, 23, 0x20, 0],
        &line5(&cat(&[&c5(0, 3, 'C'), &c5(0, 3, 'D')])),
        &[0x01, 0x01],
        &line5(&[]),
        &[0],
    ]);
    assert_eq!(latex(&eq5(&lim)), "CD");
}

#[test]
fn partition_lines_beyond_the_ast_are_retained_for_mtef_roundtrip() {
    // 1 x 40 matrix with a solid line before column 35. MTEF's partition
    // field carries it even though the historical u32 AST mask does not.
    let mut col_parts = [0u8; 11];
    col_parts[35 / 4] = 1 << (2 * (35 % 4));
    let cells5: Vec<u8> = (0..40).flat_map(|_| line5(&c5(0, 3, 'a'))).collect();
    let cells3: Vec<u8> = (0..40).flat_map(|_| line3(&c3(0, 3, 'a'))).collect();
    let matrix5 = cat(&[&[0x05, 0, 0, 1, 1, 1, 40, 0], &col_parts, &cells5, &[0]]);
    let matrix3 = cat(&[&[0x05, 1, 2, 1, 1, 40, 0], &col_parts, &cells3, &[0]]);
    for (bytes, version) in [
        (eq5(&matrix5), MtefVersion::V5),
        (eq3(&matrix3), MtefVersion::V3),
    ] {
        let result = MtefReader::parse_bytes_compact(&bytes, &Config::default()).unwrap();
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("partition line"))
        );
        assert!(
            crate::writer::latex::LatexWriter::write_compact(&result.expr, &Config::default())
                .unwrap()
                .contains("|")
        );
        let mut heap = result.expr.to_math_expr();
        let MathNode::NativeArray { rows, .. } = &mut heap.nodes[0] else {
            panic!("expected a ruled array")
        };
        assert_eq!(rows[0].len(), 40);
        assert!(matches!(rows[0][34].as_slice(),
            [MathNode::SpannedCell {span:1,right_boundaries,..}] if right_boundaries == &[crate::ast::SpanBoundary::Rule]));
        let compact = crate::CompactExpr::from_math_expr(&heap).unwrap();
        let (encoded, diagnostics) = crate::writer::mtef::MtefWriter::write_bytes_with_diagnostics(
            &compact,
            &Config::default(),
            &crate::MtefWriteOptions::new(version),
        )
        .unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let imported = MtefReader::parse_bytes(&encoded, &Config::default())
            .unwrap()
            .expr;
        assert_eq!(imported, heap);
        // Editing the public cell changes the encoded partition as well.
        let MathNode::NativeArray { rows, .. } = &mut heap.nodes[0] else {
            unreachable!()
        };
        rows[0][34] = vec![MathNode::Identifier("x".into())];
        let compact = crate::CompactExpr::from_math_expr(&heap).unwrap();
        let (changed, _) = crate::writer::mtef::MtefWriter::write_bytes_with_diagnostics(
            &compact,
            &Config::default(),
            &crate::MtefWriteOptions::new(version),
        )
        .unwrap();
        assert!(
            !crate::from_mtef_bytes(&changed)
                .unwrap()
                .to_latex()
                .unwrap()
                .contains('|')
        );
    }
}

#[test]
fn arena_refuses_oversized_child_lists() {
    let mut arena = crate::AstArena::new();
    let ids = vec![crate::NodeId::NONE; crate::AstArena::MAX_CHILDREN + 1];
    assert!(matches!(
        arena.push_children(&ids),
        Err(Error::LimitExceeded {
            kind: LimitKind::ListLength,
            ..
        })
    ));
    assert!(arena.push_children(&ids[1..]).is_ok());
}

#[test]
fn mt_extra_font_positions_without_mtcode() {
    // fnMTEXTRA characters written with only their 8-bit font position:
    // midline, vertical and diagonal ellipses and the coproduct.
    let content: Vec<u8> = [0x4C, 0x4D, 0x4F, 0x43]
        .iter()
        .flat_map(|&position| [0x02, 0x24, 128 + 11, position])
        .collect();
    let nodes = nodes(&eq5(&content));
    let text = format!("{nodes:?}");
    for ch in ['\u{22EF}', '\u{22EE}', '\u{22F1}', '\u{2210}'] {
        assert!(text.contains(ch), "{ch:?} missing from {text}");
    }
}
