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
fn v3_operator_character_is_not_latex_syntax() {
    // The operator of a sum or integral template may be any character.
    let operator = |selector: u8, ch: char, lower: &str| {
        let lower = cat(&lower
            .chars()
            .map(|c| c3(1, 3, c))
            .collect::<Vec<_>>()
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>());
        latex(&eq3(&cat(&[
            &[0x03, selector, 2, 0],
            &line3(&c3(1, 3, 'a')),
            &line3(&lower),
            &line3(&[]),
            &[0x0D],
            &c3(0, 6, ch),
            &[0],
        ])))
    };
    for (ch, sum, sum_without_limits) in [
        ('}', r"\mathop{\}}", r"\mathop{\}}a"),
        ('{', r"\mathop{\{}", r"\mathop{\{}a"),
        ('8', r"\mathop{8}", r"\mathop{8}a"),
        ('%', r"\mathop{\%}", r"\mathop{\%}a"),
        ('\\', r"\mathop{\setminus}", r"\mathop{\setminus}a"),
        ('^', r"\mathop{\char`\^}", r"\mathop{\char`\^}a"),
        ('砀', r"\mathop{砀}", r"\mathop{砀}a"),
    ] {
        assert_eq!(operator(43, ch, "k"), format!(r"{sum}\limits_ka"));
        assert_eq!(operator(43, ch, ""), sum_without_limits);
        assert_eq!(operator(42, ch, "k"), format!(r"{sum}\nolimits_ka"));
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
    assert_eq!(latex(&eq5(&brace)), r"\overbrace{a}^b");
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
fn v5_pile_ruler_has_no_tag_byte() {
    // PILE with mtefOPT_LP_RULER: halign, valign, then one tab stop (count,
    // type, 16-bit offset) directly, then the pile's lines.
    let content = cat(&[
        &[0x04, 0x02, 1, 1, 1, 0, 0x68, 0x0D],
        &line5(&c5(0, 3, 'x')),
        &line5(&c5(0, 3, 'y')),
        &[0],
    ]);
    assert_eq!(
        latex(&eq5(&content)),
        latex(&eq5(&cat(&[
            &[0x04, 0, 1, 1],
            &line5(&c5(0, 3, 'x')),
            &line5(&c5(0, 3, 'y')),
            &[0]
        ])))
    );
}

#[test]
fn v5_spacing_style_bare_space_codes() {
    // MathType 5.0 writes Spacing-style characters as bare codes: 0x02 thin,
    // 0x04 thick, 0x08 one-point, 0x06 two-em; they are spaces, not control
    // characters.
    let content = cat(&[
        &c5(0, 3, 'x'),
        &c5(0, 24, '\u{2}'),
        &c5(0, 3, 'y'),
        &c5(0, 24, '\u{4}'),
        &c5(0, 3, 'z'),
    ]);
    let result = MtefReader::parse_bytes(&eq5(&content), &Config::default()).unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        latex(&eq5(&content)),
        latex(&eq5(&cat(&[
            &c5(0, 3, 'x'),
            &c5(0, 24, '\u{EF02}'),
            &c5(0, 3, 'y'),
            &c5(0, 24, '\u{EF04}'),
            &c5(0, 3, 'z'),
        ])))
    );
    // Outside the Spacing style the same code is still a control character.
    assert!(strict_error(&eq5(&c5(0, 3, '\u{2}'))).contains("control character"));
}

#[test]
fn v5_mtcode_math_alphabets() {
    // A letter in font 1 (`font`, style bits `style`), with a font position.
    let letter = |font: &str, style: u8, code: u16, position: Option<u8>| {
        let code = code.to_le_bytes();
        let options = if position.is_some() { 0x04 } else { 0 };
        let mut record = cat(&[
            &[0x11, 2],
            font.as_bytes(),
            &[0, 0x08, 1, style, 0x02, options, 0x7F],
            &code,
        ]);
        record.extend(position);
        eq5(&record)
    };
    let fraktur = "Euclid Fraktur";
    let script = "Euclid Math One";
    assert_eq!(
        latex(&letter(fraktur, 0, 0xF001, Some(b'B'))),
        r"\mathfrak{B}"
    );
    assert_eq!(
        latex(&letter(fraktur, 0, 0xF01E, Some(b'e'))),
        r"\mathfrak{e}"
    );
    assert_eq!(
        latex(&letter(script, 0, 0xF10D, Some(b'N'))),
        r"\mathcal{N}"
    );
    let bold = nodes(&letter(fraktur, 1, 0xF001, Some(b'B')));
    assert!(format!("{bold:?}").contains("BoldFraktur"), "{bold:?}");
    // No position, another position, another font, or a small script letter:
    // the code point is kept and reported.
    for (bytes, code) in [
        (letter(fraktur, 0, 0xF021, None), "U+F021"),
        (letter(fraktur, 0, 0xF001, Some(b'Q')), "U+F001"),
        (letter("Wingdings", 0, 0xF001, Some(b'B')), "U+F001"),
        (letter(script, 0, 0xF127, Some(b'n')), "U+F127"),
    ] {
        let result = MtefReader::parse_bytes(&bytes, &lenient()).unwrap();
        assert!(has_warning(&result, code), "{:?}", result.diagnostics);
    }
}

#[test]
fn v3_ruler_has_its_tag() {
    let tagged = [
        0x03, 1, 1, 3, 0, 0x0A, 0x21, 0x07, 1, 0, 0x10, 0, 0x02, 0x83, 0x78, 0, 0, 0,
    ];
    assert_eq!(latex(&tagged), "x");
    let mut untagged = tagged.to_vec();
    untagged.remove(7);
    assert!(strict_error(&untagged).contains("RULER"));
}

#[test]
fn equation_data_in_the_tail_is_not_slack() {
    let refuse = |tail: &[u8], total: Option<usize>| {
        let mut mtef = eq3(&c3(0, 3, 'x'));
        mtef.extend_from_slice(tail);
        let total = total.unwrap_or(mtef.len().next_multiple_of(4));
        mtef.resize(total, 0x01);
        let declared = u32::try_from(total).unwrap();
        let data = ole(&mtef, 0x0002_0000, declared);
        assert!(strict_error(&data).contains("follow the end"), "{tail:?}");
        let recovered = MtefReader::parse_bytes(&data, &lenient()).unwrap();
        assert!(has_warning(&recovered, "follow the end"));
    };
    // Records without content before a character; two lines decoding to
    // the end.
    refuse(&cat(&[&[0x0A], &c3(0, 3, 'y')]), Some(400));
    refuse(&cat(&[&[0x09, 1, 130], &c3(0, 3, 'y')]), Some(400));
    let two_lines = cat(&[&line3(&c3(0, 3, 'y')), &line3(&c3(0, 3, 'z')), &[0]]);
    let mut mtef = eq3(&c3(0, 3, 'x'));
    mtef.extend_from_slice(&two_lines);
    mtef.resize(400, 0);
    assert!(strict_error(&ole(&mtef, 0x0002_0000, 400)).contains("follow the end"));
    // The same lines, then a record cut off by the end of the buffer.
    mtef[399] = 0x03;
    assert_eq!(latex(&ole(&mtef, 0x0002_0000, 400)), "x");
    // A complete character; the rest of a line; a second equation.
    refuse(&c3(0, 3, 'y'), None);
    refuse(&cat(&[&c3(0, 3, 'y'), &[0, 0]]), Some(400));
    refuse(&cat(&[&[0x0A], &line3(&c3(0, 3, 'y')), &[0]]), Some(400));
}

#[test]
fn nested_empty_limits_draw_nothing() {
    let mut data = vec![
        0x03, 1, 0x0A, 1, 0, 0x03, 0x27, 1, 0, 1, 0, 0x03, 0x27, 1, 0, 1, 0x10, 5, 0x83, 0x35, 0x0A,
    ];
    data.extend([0; 7]);
    let result = crate::from_mtef_bytes_with(&data, &lenient()).unwrap();
    let options = crate::MtefWriteOptions::default();
    let first = result.to_mtef_bytes(&options).unwrap();
    let back = crate::from_mtef_bytes(&first).unwrap();
    assert_eq!(back.to_mtef_bytes(&options).unwrap(), first);
}

#[test]
fn adobe_symbol_private_use_pieces_are_mapped() {
    // arrowvertex, as MathType writes it for Symbol position 0xBD.
    assert_eq!(
        nodes(&eq5(&c5(0, 6, '\u{F8E6}'))),
        nodes(&eq5(&c5(0, 6, '\u{23D0}')))
    );
    assert_eq!(chars::normalize_code_point('\u{F8F5}'), '\u{23AE}');
    assert_eq!(chars::normalize_code_point('\u{F8F6}'), '\u{239E}');
    assert_eq!(chars::normalize_code_point('\u{F8FE}'), '\u{23AD}');
    assert_eq!(chars::normalize_code_point('\u{F8E8}'), '\u{00AE}');
    assert_eq!(chars::normalize_code_point('\u{F8E5}'), '\u{F8E5}');
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
    // A LINE with a ruler: two tab stops of 3 bytes each, without the RULER
    // tag byte, as MathType writes it.
    let mut body = vec![0x0A, 0x01, 0x02, 2, 0, 0x10, 0, 3, 0x20, 0];
    body.extend_from_slice(&content);
    body.extend_from_slice(&[0, 0]);
    let result = MtefReader::parse_bytes(&v5(&body), &Config::default()).unwrap();
    assert_eq!(
        result.expr.nodes,
        [MathNode::Color {
            color: "#ff0000".into(),
            // The last SIZE record: the subscript size and 2/32 of a point.
            content: Box::new(MathNode::Attributed {
                attrs: crate::ast::NodeAttrs::with_size(crate::ast::FontSize::Logical {
                    level: crate::ast::TypeSize::Sub,
                    delta_pt: crate::ast::Rational::new(1, 16),
                }),
                content: Box::new(MathNode::Identifier("x".into())),
            }),
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

/// A slash takes no scripts or primes, like a binary operator, though its class
/// is ordinary.
#[test]
fn a_slash_takes_scripts_and_primes_as_an_operator_does() {
    let f = |ch| c5(0, 2, ch);
    let (x, y, prime) = (c5(0, 3, 'x'), c5(0, 3, 'y'), f('\''));
    let alike = |parts: &dyn Fn(&[u8]) -> Vec<Vec<u8>>| {
        let written = |operator| {
            let parts = parts(&c5(0, 6, operator));
            latex5(&parts.iter().map(Vec::as_slice).collect::<Vec<_>>())
        };
        // Only the spacing of the spelling differs.
        let slash = written('/').replace(['/', ' '], "");
        assert_eq!(slash, written('+').replace(['+', ' '], ""));
    };
    alike(&|op| vec![x.clone(), op.to_vec(), prime.clone(), y.clone()]);
    alike(&|op| vec![x.clone(), op.to_vec(), pre_sup5('2'), y.clone()]);
    alike(&|op| vec![x.clone(), pre_sup5('2'), op.to_vec(), y.clone()]);
    alike(&|op| vec![x.clone(), op.to_vec(), pre_sup5('2')]);
    assert_eq!(latex5(&[&x, &c5(0, 6, '/'), &pre_sup5('2')]), "x/{}^{2}");
    assert_eq!(latex5(&[&x, &pre_sup5('2'), &c5(0, 6, '/'), &y]), "x^{2}/y");
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
    assert_eq!(latex5(&[&f('"'), &x, &f('"')]), r#""x""#);
}

#[test]
fn possessive_apostrophes_are_not_primes() {
    let (x, s, i) = (c5(0, 3, 'x'), c5(0, 3, 's'), c5(0, 3, 'i'));
    let a = c5(0, 2, '\'');
    assert_eq!(latex5(&[&x, &a, &s]), "x\\char`'s");
    // A product and a function name after a prime.
    assert_eq!(latex5(&[&x, &a, &s, &i]), r"x^{\prime}si");
    assert_eq!(latex5(&[&x, &a, &s, &sub5(&[], 'i')]), r"x^{\prime}s_i");
    let sin = cat(&[&c5(2, 2, 's'), &c5(0, 2, 'i'), &c5(0, 2, 'n')]);
    assert_eq!(latex5(&[&x, &a, &sin, &i]), r"x^{\prime}\sin i");
}

#[test]
fn leading_scripts_before_fences_and_functions_follow_the_item_before() {
    // tmPAREN (1) with both sides, holding Q.
    let fence = cat(&[&[3, 0, 1, 3, 0, 1, 0], &c5(0, 3, 'Q'), &[0, 0]]);
    let d = c5(0, 3, 'D');
    let log = cat(&[&c5(2, 2, 'l'), &c5(0, 2, 'o'), &c5(0, 2, 'g')]);
    let follow = latex5(&[&d, &pre_sup5('2'), &fence]);
    assert!(follow.starts_with(r"D^{2}\left("), "{follow}");
    assert_eq!(latex5(&[&d, &pre_sup5('2'), &log]), r"D^{2}\log");
    // Nothing that carries it is before the function name: it leads (¹⁸⁸Re).
    let lead = latex5(&[&c5(0, 6, '\u{2192}'), &pre_sup5('2'), &log]);
    assert!(
        lead.contains("{}^{2}") && !lead.contains("arrow^"),
        "{lead}"
    );
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
    // Buffer slack inside the declared object, after the final END: ignored
    // with a note in Strict mode too (MathType declares its whole buffer).
    let mut slack = mtef.clone();
    let filler = 4 - slack.len() % 4;
    slack.extend(std::iter::repeat_n(0xEF, filler));
    let slack_len = u32::try_from(slack.len()).unwrap();
    let result =
        MtefReader::parse_bytes(&ole(&slack, 0x0002_0000, slack_len), &Config::default()).unwrap();
    assert_eq!(result.expr.nodes.len(), 1);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Info && d.message.contains("slack"))
    );
    // A longer tail, or a size that is not a multiple of 4, is not slack.
    let mut long = slack.clone();
    long.extend_from_slice(&[0xEF; 8]);
    let odd = [mtef.as_slice(), &vec![0xEF; filler + 1]].concat();
    for tail in [long, odd] {
        let declared = u32::try_from(tail.len()).unwrap();
        assert!(strict_error(&ole(&tail, 0x0002_0000, declared)).contains("follow the end"));
    }
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
    assert!(strict_error(&[1, 0x0A]).contains("version 1 is not supported"));
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

#[test]
fn leading_scripts_stay_outside_trailing_ones() {
    // tmSUB with tvSU_PRECEDES, `d`, then a trailing tmSUB: one node with
    // both scripts, which writing and reading again must not change.
    let sub = |variation: u8, digit: char| {
        cat(&[
            &[0x03, 0, 27, variation, 0],
            &line5(&c5(0, 8, digit)),
            &[0x01, 0x01, 0],
        ])
    };
    let equation = eq5(&cat(&[&sub(1, '1'), &c5(0, 3, 'd'), &sub(0, '2')]));
    let decoded = nodes(&equation);
    assert!(
        matches!(&decoded[..], [MathNode::PreScripts { base, .. }]
            if matches!(**base, MathNode::Subscript { .. })),
        "{decoded:?}"
    );
    let result = crate::from_mtef_bytes(&equation).unwrap();
    let written = result
        .to_mtef_bytes(&crate::MtefWriteOptions::default())
        .unwrap();
    assert_eq!(
        crate::from_mtef_bytes(&written).unwrap().expr(),
        result.expr()
    );
}

#[test]
fn a_leading_script_leads_the_first_item_of_a_template_row() {
    // tmSUP with tvSU_PRECEDES, then a tmINTOP whose operator is empty and
    // whose body starts with a space, which it cannot lead.
    let equation = eq5(&cat(&[
        &[0x03, 0, 28, 1, 0, 0x01, 0x01],
        &line5(&c5(0, 3, 'l')),
        &[0],
        &[0x03, 0, 21, 0, 0],
        &line5(&cat(&[&c5(0, 24, '\u{EF02}'), &c5(0, 3, 'x')])),
        &[0x01, 0x01, 0x01, 0x01],
        &line5(&[]),
        &[0],
    ]));
    let decoded = format!("{:?}", nodes(&equation));
    assert!(
        decoded.contains("Superscript { base: Row([])") && decoded.contains("Space"),
        "{decoded}"
    );
}

#[test]
fn labelled_arrow_pairs_keep_both_arrows() {
    for arrow in ["⇌", "⇄"] {
        let source = format!("<math><mover><mo>{arrow}</mo><mi>a</mi></mover></math>");
        let result = crate::from_mathml(&source).unwrap();
        let data = result
            .to_mtef_bytes(&crate::MtefWriteOptions::default())
            .unwrap();
        let back = crate::from_mtef_bytes(&data).unwrap();
        let mathml = back.to_mathml().unwrap();
        assert!(mathml.contains(arrow), "{mathml}");
    }
}

#[test]
fn a_prime_after_a_group_stays_inside_its_leading_scripts() {
    // A leading script, an embellished letter (not a lone character) and an
    // apostrophe.
    let equation = eq5(&cat(&[
        &[0x03, 0, 27, 1, 0],
        &line5(&c5(0, 8, '1')),
        &[0x01, 0x01, 0],
        &c5(0x01, 3, 'A'),
        &[0x06, 0, 13, 0],
        &c5(0, 6, '\''),
    ]));
    let decoded = nodes(&equation);
    assert!(
        matches!(&decoded[..], [MathNode::PreScripts { base, .. }]
            if matches!(**base, MathNode::Superscript { .. })),
        "{decoded:?}"
    );
}

#[test]
fn only_arrow_pair_variations_are_drawn_with_two_characters() {
    let arrow = |variation: u8, characters: &str| {
        let mut template = vec![0x03, 0, 14, variation, 0];
        template.extend(line5(&c5(0, 3, 'a')));
        template.extend([0x01, 0x01]);
        for ch in characters.chars() {
            template.extend(c5(0, 22, ch));
        }
        template.push(0);
        eq5(&template)
    };
    let drawn = |bytes: &[u8], config: &Config| {
        let result = MtefReader::parse_bytes(bytes, config).unwrap();
        (format!("{:?}", result.expr.nodes), result.has_warnings())
    };
    for (variation, characters, expected) in [
        (0x05, "→←", "⇄"),
        (0x06, "⇀↽", "⇌"),
        (0x24, "→", "→"),
        (0x24, "", "→"),
        (0x05, "", "⇄"),
    ] {
        for config in [Config::default(), lenient()] {
            let (nodes, warned) = drawn(&arrow(variation, characters), &config);
            assert!(nodes.contains(expected) && !warned, "{characters}: {nodes}");
        }
    }
    // More characters than the variation draws: refused, or the first.
    for (variation, characters) in [
        (0x00, "→←"),
        (0x10, "→←"),
        (0x24, "→←"),
        (0x05, "→xy"),
        (0x06, "→x"),
    ] {
        let bytes = arrow(variation, characters);
        assert!(strict_error(&bytes).contains("arrow template holds"));
        let (nodes, warned) = drawn(&bytes, &lenient());
        assert!(nodes.contains('→') && warned, "{characters}: {nodes}");
    }
}

/// Writing an equation and reading it back reaches a fixed point after one
/// pass, with the content unchanged.
#[track_caller]
fn settles(data: &[u8]) {
    let first = crate::from_mtef_bytes_with(data, &lenient()).unwrap();
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let options = crate::MtefWriteOptions::new(version);
        let one = crate::from_mtef_bytes(&first.to_mtef_bytes(&options).unwrap()).unwrap();
        let written = one.to_mtef_bytes(&options).unwrap();
        let two = crate::from_mtef_bytes(&written).unwrap();
        assert_eq!(one.to_mathml().unwrap(), two.to_mathml().unwrap());
        assert_eq!(written, two.to_mtef_bytes(&options).unwrap());
    }
}

/// An MTEF 3 INTOP template without limits: `operator` drawn before `body`.
fn intop(operator: &[u8], body: &[u8]) -> Vec<u8> {
    let mut data = vec![0x03, 42, 0, 0];
    data.extend(line3(body));
    data.extend_from_slice(&[0x11, 0x11]);
    data.extend(line3(operator));
    data.push(0);
    data
}

#[test]
fn a_prime_operator_without_limits_is_the_prime_of_the_item_before_it() {
    let prime = intop(&c3(0, 2, '\''), &[]);
    let data = eq3(&[c3(0, 3, 'x'), prime.clone()].concat());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"x^{\prime}"
    );
    settles(&data);
    // After a body, and nested in another one.
    let body = intop(&c3(0, 3, 'y'), &c3(0, 8, '0'));
    settles(&eq3(&[body, c3(0, 2, '\'')].concat()));
    settles(&eq3(&intop(&c3(0, 3, 'y'), &prime)));
    // An operator that draws nothing does not come between.
    let nothing = intop(&c3(0, 24, '\u{EB01}'), &[]);
    let data = eq3(&[c3(0, 3, 'x'), nothing, c3(0, 2, '\'')].concat());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"x^{\prime}"
    );
    settles(&data);
}

#[test]
fn a_slot_holding_only_a_zero_space_is_empty() {
    let zero = c3(0, 24, '\u{EB01}');
    // Root with a zero space for its index.
    let mut root = vec![0x03, 13, 1, 0];
    root.extend(line3(&c3(0, 3, 'x')));
    root.extend(line3(&zero));
    root.push(0);
    let data = eq3(&root);
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"\sqrt{x}"
    );
    settles(&data);
    // An accent over a zero space.
    let mut accent = c3(2, 24, '\u{EB01}');
    accent.extend_from_slice(&[0x06, 19, 0, 0]);
    settles(&eq3(&accent));
}

#[test]
fn a_point_written_after_a_number_stays_its_own_node() {
    let result = crate::from_latex(r"6{.}6+1.2{.}3").unwrap();
    for version in [MtefVersion::V3, MtefVersion::V5] {
        let options = crate::MtefWriteOptions::new(version);
        let back = crate::from_mtef_bytes(&result.to_mtef_bytes(&options).unwrap()).unwrap();
        let mathml = back.to_mathml().unwrap();
        assert!(
            mathml.contains("<mn>6</mn><mo>.</mo><mn>6</mn>"),
            "{mathml}"
        );
        assert!(
            mathml.contains("<mn>1.2</mn><mo>.</mo><mn>3</mn>"),
            "{mathml}"
        );
    }
}

#[test]
fn leading_and_trailing_scripts_keep_their_colour() {
    // COLOR_DEF blue, black. tmSUB with the `tvSU_PRECEDES` variation leads.
    let define = [
        0x10, 0, 0, 0, 0, 0, 0xE8, 0x03, 0x10, 0, 0, 0, 0, 0, 0, 0, 0x0F, 1,
    ];
    let sub = |variation: u8, ch: char| {
        let mut data = vec![0x03, 0x00, 0x1B, variation, 0x00];
        data.extend(line5(&c5(0, 3, ch)));
        data.extend_from_slice(&[0x01, 0x01, 0x00]);
        data
    };
    let parts = |middle: &[u8]| {
        let body: [&[u8]; 5] = [&define, &sub(1, 'a'), &c5(0, 3, 'x'), middle, &sub(0, 'b')];
        eq5(&body.concat())
    };
    let mathml = |data: &[u8]| crate::from_mtef_bytes(data).unwrap().to_mathml().unwrap();
    // All of it blue.
    let blue = parts(&[]);
    let out = mathml(&blue);
    assert!(
        out.contains(r##"<mstyle mathcolor="#0000ff"><mmultiscripts>"##),
        "{out}"
    );
    settles(&blue);
    // Blue with a black trailing subscript.
    let mixed = parts(&[0x0F, 2]);
    let out = mathml(&mixed);
    assert_eq!(out.matches("#0000ff").count(), 2, "{out}");
    settles(&mixed);
}

#[test]
fn a_scripted_text_stays_apart_from_the_text_before_it_across_colour_records() {
    // Blue `a`, and blue `b` with a blue subscript, as two runs.
    let result = crate::from_latex(
        r"{\color{blue}\text{a}}{\color{blue}\text{b}_c}+{\color{blue}\text{a}}\text{b}_c",
    )
    .unwrap();
    let written = result
        .to_mtef_bytes(&crate::MtefWriteOptions::default())
        .unwrap();
    let mathml = crate::from_mtef_bytes(&written)
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(!mathml.contains("<mtext>ab</mtext>"), "{mathml}");
    assert_eq!(mathml.matches("<msub>").count(), 2, "{mathml}");
    settles(&written);
}

#[test]
fn primes_and_leading_scripts_around_items_that_draw_nothing_settle() {
    let zero = c3(0, 24, '\u{EB01}');
    // A zero space before a prime.
    let data = eq3(&[c3(0, 3, 'x'), zero.clone(), c3(0, 2, '\'')].concat());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"x^{\prime}"
    );
    settles(&data);
    // A leading subscript template (tmLSCRIPT, subscript).
    let leading = |ch: char| {
        let mut data = vec![0x03, 44, 1, 0];
        data.extend(line3(&c3(0, 3, ch)));
        data.extend_from_slice(&[0x11, 0]);
        data
    };
    let subscript = |ch: char| {
        let mut data = leading(ch);
        data[1] = 15;
        data
    };
    let nothing = intop(&zero, &[]);
    // Before an operator that draws nothing, then a prime.
    settles(&eq3(
        &[leading('a'), nothing.clone(), c3(0, 2, '\'')].concat()
    ));
    // Before a subscript on nothing, and before another leading script.
    settles(&eq3(&[leading('a'), intop(&zero, &subscript('b'))].concat()));
    let led = [leading('b'), c3(0, 3, 'x')].concat();
    settles(&eq3(&[leading('a'), intop(&zero, &led)].concat()));
    // A prime with a subscript of its own.
    let primed = [c3(0, 2, '\''), subscript('b')].concat();
    settles(&eq3(&[c3(0, 3, 'x'), intop(&zero, &primed)].concat()));
}

/// The colours of scripts and their bases survive rewriting in one spelling.
#[test]
fn scripts_in_several_colours_settle() {
    let define = [
        0x10, 0, 0, 0, 0, 0, 0xE8, 0x03, 0x10, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 0xE8, 0x03, 0, 0, 0, 0,
    ];
    let (blue, black, red) = ([0x0F, 1], [0x0F, 2], [0x0F, 3]);
    let script = |variation: u8, content: &[u8]| {
        let mut data = vec![0x03, 0x00, 0x1B, variation, 0x00];
        data.extend(line5(content));
        data.extend_from_slice(&[0x01, 0x01, 0x00]);
        data
    };
    let x = |ch: char| c5(0, 3, ch);
    let cases: [Vec<Vec<u8>>; 4] = [
        // A script on nothing before a coloured run.
        vec![script(0, &x('a')), blue.to_vec(), x('x'), x('y')],
        // Before a coloured letter with a subscript.
        vec![
            script(0, &x('a')),
            blue.to_vec(),
            x('x'),
            script(0, &x('b')),
        ],
        // The same after another item.
        vec![
            x('w'),
            script(1, &x('a')),
            c5(0, 24, '\u{EF01}'),
            blue.to_vec(),
            x('x'),
            x('y'),
        ],
        // A black base of a blue leading script in a red line.
        vec![
            red.to_vec(),
            script(
                0,
                &[
                    x('m'),
                    blue.to_vec(),
                    script(1, &x('a')),
                    black.to_vec(),
                    x('x'),
                ]
                .concat(),
            ),
            x('z'),
        ],
    ];
    for parts in cases {
        let data = eq5(&[define.to_vec(), parts.concat()].concat());
        let mathml = crate::from_mtef_bytes(&data).unwrap().to_mathml().unwrap();
        assert!(mathml.contains("mathcolor"), "{mathml}");
        let written = crate::from_mtef_bytes(&data)
            .unwrap()
            .to_mtef_bytes(&crate::MtefWriteOptions::default())
            .unwrap();
        let back = crate::from_mtef_bytes(&written).unwrap();
        assert_eq!(back.to_mathml().unwrap(), mathml);
        settles(&data);
    }
}

/// The MathML both of `data` and of every rewrite of it.
#[track_caller]
fn settled_mathml(data: &[u8]) -> String {
    settles(data);
    let first = crate::from_mtef_bytes(data).unwrap();
    let written = first
        .to_mtef_bytes(&crate::MtefWriteOptions::default())
        .unwrap();
    let mathml = first.to_mathml().unwrap();
    let back = crate::from_mtef_bytes(&written).unwrap();
    assert_eq!(back.to_mathml().unwrap(), mathml);
    mathml
}

/// An MTEF 5 template of three slots, `None` a null line.
fn template5(selector: u8, variation: u8, slots: &[Option<&[u8]>]) -> Vec<u8> {
    let mut data = vec![0x03, 0, selector, variation, 0];
    for slot in slots {
        match slot {
            Some(content) => data.extend(line5(content)),
            None => data.extend_from_slice(&[0x01, 0x01]),
        }
    }
    data.push(0);
    data
}

fn script5(variation: u8, content: &[u8]) -> Vec<u8> {
    template5(27, variation, &[Some(content), None])
}

/// A limit template without limits is its content, in the line's runs.
#[test]
fn a_limit_template_without_limits_joins_its_line() {
    let x = [&[0x0F, 1][..], &c5(0, 3, 'x')].concat();
    let limit = template5(23, 0x30, &[Some(&x), None, None]);
    let data = eq5(&[colours(), vec![0x0F, 2], limit, c5(0, 3, 'y')].concat());
    let mathml = settled_mathml(&data);
    assert_eq!(mathml.matches("#0000ff").count(), 1, "{mathml}");
}

/// A prime belongs to the nucleus in its colour, whatever the colours of the
/// scripts between.
#[test]
fn a_prime_belongs_to_the_nucleus_in_its_colour() {
    let (blue, black) = (vec![0x0F, 1], vec![0x0F, 2]);
    let (x, prime) = (c5(0, 3, 'x'), c5(0, 2, '\''));
    let a = script5(0, &c5(0, 3, 'a'));
    // Blue x and prime, black subscript; and the same after a black leading
    // script.
    let plain = [&blue[..], &x, &black, &a, &blue, &prime].concat();
    let led = [
        &black[..],
        &script5(1, &c5(0, 3, 'b')),
        &blue,
        &x,
        &black,
        &a,
        &blue,
        &prime,
    ]
    .concat();
    for content in [plain, led] {
        let mathml = settled_mathml(&eq5(&[colours(), content].concat()));
        assert!(mathml.contains("<msup>"), "{mathml}");
        assert!(mathml.contains("′"), "{mathml}");
    }
}

/// Primes typed as embellishments of a prime count one by one.
#[test]
fn a_prime_with_prime_embellishments_is_so_many_primes() {
    let mut embellished = c5(0, 2, '\'');
    embellished[1] |= 0x01;
    embellished.extend_from_slice(&[0x06, 0, 5, 0]);
    let template = template5(21, 0, &[Some(&[]), None, None, Some(&embellished)]);
    let subscript = script5(0, &c5(0, 3, 'a'));
    let prime = c5(0, 2, '\'');
    let x = c5(0, 3, 'x');
    // In the line and drawn by a template, after `x`, `x′` and `x_a`.
    for item in [&embellished, &template] {
        for (before, marks) in [
            (x.clone(), 2),
            ([&x[..], &prime].concat(), 3),
            ([&x[..], &subscript].concat(), 2),
        ] {
            let data = eq5(&[&before[..], item, &subscript, &prime].concat());
            let mathml = settled_mathml(&data);
            assert_eq!(mathml.matches('′').count(), marks + 1, "{mathml}");
        }
    }
}

/// Leading scripts of nothing are scripts on nothing, also below the depth
/// normalization reaches.
#[test]
fn leading_scripts_of_nothing_settle_in_deep_tables() {
    let zero = c5(0, 24, '\u{EF01}');
    let mut content = [script5(0, &c5(0, 3, 'b')), script5(1, &c5(0, 3, 'a')), zero].concat();
    // Every depth up to the limit of the reader.
    while let Ok(result) = crate::from_mtef_bytes(&eq5(&content)) {
        if result
            .to_mtef_bytes(&crate::MtefWriteOptions::default())
            .is_err()
        {
            break;
        }
        assert!(!result.to_mathml().unwrap().contains("mprescripts"));
        settles(&eq5(&content));
        let mut pile = vec![0x04, 0, 2, 1];
        pile.extend(line5(&content));
        pile.push(0);
        content = pile;
    }
    assert!(content.len() > 200);
}

/// A possessive `'s` ends before another letter in a zero space.
#[test]
fn a_possessive_and_zero_spaces_settle() {
    let zero = c5(0, 24, '\u{EF01}');
    let (a, quote, s) = (c5(0, 3, 'a'), c5(0, 2, '\''), c5(0, 3, 's'));
    // The zero space before the s does not count; the one after it does.
    for parts in [
        vec![&a, &quote, &zero, &s],
        vec![&a, &quote, &s, &zero, &a],
        vec![&a, &quote, &s],
    ] {
        let data = eq5(&parts.into_iter().flatten().copied().collect::<Vec<_>>());
        let mathml = settled_mathml(&data);
        assert!(!mathml.contains("<msup>"), "{mathml}");
    }
    // A template's letter after the s counts: a variable, not a possessive.
    let limit = template5(23, 0x30, &[Some(&a), None, None]);
    let data = eq5(&[a, quote, s, limit].concat());
    assert!(settled_mathml(&data).contains("<msup>"));
}

/// `y′` and a subscript in another colour are one node.
#[test]
fn a_prime_and_a_subscript_in_two_colours_are_one_node() {
    let prime = template5(
        28,
        0,
        &[None, Some(&[&[0x0F, 1][..], &c5(0, 2, '\'')].concat())],
    );
    let content = [
        &[0x0F, 1][..],
        &c5(0, 3, 'y'),
        &prime,
        &[0x0F, 2],
        &script5(0, &c5(0, 3, 'a')),
    ]
    .concat();
    let mathml = settled_mathml(&eq5(&[colours(), content].concat()));
    assert!(mathml.contains("<msubsup>"), "{mathml}");
}

/// Primes that join one by one take linear time.
#[test]
fn many_primes_in_template_runs_are_cheap() {
    let mut data = c5(0, 3, 'x');
    let prime = template5(21, 0, &[Some(&[]), None, None, Some(&c5(0, 2, '\''))]);
    for _ in 0..20_000 {
        data.extend_from_slice(&prime);
    }
    let start = std::time::Instant::now();
    let mathml = crate::from_mtef_bytes(&eq5(&data))
        .unwrap()
        .to_mathml()
        .unwrap();
    assert_eq!(mathml.matches('′').count(), 20_000);
    // 20 ms in release; copying the primes each time took a second there.
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// Runs of one colour in the content of a script are one run, in whatever
/// templates the colour was selected.
#[test]
fn runs_of_one_colour_in_a_script_are_one_run() {
    // A generated equation, reduced.
    let digits = concat!(
        "05534d54360000100000000000e803100000000000000010000000000a010003001b00",
        "01000f0103001b0000010003001c010001010100010002008831000f02020081610000",
        "00000300173000010002009801ef03001b000001000f02730002008273000001010002",
        "0082270003001b0100010101010000010101010002008632200101000f020000",
    );
    let data: Vec<u8> = (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
        .collect();
    settles(&data);
}

/// Generated equations, reduced, that each need one rule.
#[test]
fn reduced_generated_equations_settle() {
    for digits in [
        // a prime after scripts on nothing that a big operator takes
        concat!(
            "05010006000000100000000000e80310000000000000001003000000000a0200",
            "0001000200861122030015000001000200822700030000010100030017300001",
            "000f0100000001010101010003001b0000010002000100000000000000",
        ),
        // scripts after leading scripts of nothing
        concat!(
            "05534d54360000100000000000e803100000000000000010000000000a010003",
            "00816100000100030015000001000300150000010003001c0100010101000200",
            "883100020200837802008273000000000101010101000200837800000003001b",
            "000001000300150000018611220200863d000f02020088310000010101010198",
            "01ef0f0100010002008378000001010002008227000001010002010600050000",
            "010101010100030086280003001c000001010100020082730001010101000001",
            "0100000002008227000f02020082270003001d00000100010101000200837900",
            "000001010000",
        ),
        // scripts on nothing as the base of scripts in another colour
        concat!(
            "05000654360000100000000000000a010401000200822700010003001b010001",
            "0003000b00010100020082270003000001010181010002008161000288310002",
            "008273000000000100020083790000000200862c000f01020082730000000300",
            "1c00000300150000010002008632200300173000010002008227000200863d00",
            "000100020083730002008373000f030001010002008632200001010101010003",
            "001d00010003001c010101860101023220000000000300000200000000000000",
            "0000",
        ),
        // a script on a number in a row beside a number
        concat!(
            "05000654360000100000000000000a01000f01000200863d0002008831000100",
            "0200883100020082270002008227000003001b0100030001010001010000",
        ),
    ] {
        let data: Vec<u8> = (0..digits.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
            .collect();
        let mathml = crate::from_mtef_bytes_with(&data, &lenient())
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(mathml.contains("<mo"), "{mathml}");
        settles(&data);
    }
}

/// A prime glyph with prime embellishments is the glyph once and as many
/// primes, for every glyph and embellishment.
#[test]
fn prime_glyphs_with_prime_embellishments_keep_their_marks() {
    let marks = |mathml: &str| -> Vec<char> {
        let is_mark = |ch: &char| matches!(ch, '\u{2032}'..='\u{2034}' | '\u{2057}');
        mathml.chars().filter(is_mark).collect()
    };
    for glyph in ['\'', '\u{2032}', '\u{2033}', '\u{2034}', '\u{2057}'] {
        for (kind, count) in [(5, 1), (6, 2), (18, 3)] {
            let typeface = if glyph == '\'' { 2 } else { 6 };
            let mut five = c5(0, typeface, glyph);
            five[1] |= 0x01;
            five.extend_from_slice(&[0x06, 0, kind, 0]);
            let mut three = c3(0, typeface, glyph);
            three[0] |= 0x20;
            three.extend_from_slice(&[0x06, kind, 0, 0]);
            let mut expected = vec!['\u{2032}'; count];
            expected.insert(0, if glyph == '\'' { '\u{2032}' } else { glyph });
            for data in [
                eq5(&[c5(0, 3, 'x'), five].concat()),
                eq3(&[c3(0, 3, 'x'), three].concat()),
            ] {
                let result = crate::from_mtef_bytes(&data).unwrap();
                let mathml = result.to_mathml().unwrap();
                assert_eq!(marks(&mathml), expected, "{mathml}");
                assert!(mathml.contains("<msup>"), "{mathml}");
                settles(&data);
                for version in [MtefVersion::V3, MtefVersion::V5] {
                    let written = result
                        .to_mtef_bytes(&crate::MtefWriteOptions::new(version))
                        .unwrap();
                    let again = crate::from_mtef_bytes(&written).unwrap();
                    assert_eq!(marks(&again.to_mathml().unwrap()), expected);
                }
            }
        }
    }
    let mtef = crate::convert("x″^{\\prime}", crate::Format::LaTeX, crate::Format::Mtef).unwrap();
    let mathml = crate::convert(&mtef, crate::Format::Mtef, crate::Format::MathML).unwrap();
    assert_eq!(marks(&mathml), ['\u{2033}', '\u{2032}'], "{mathml}");
}

/// Also one that a template drew, with embellishments on embellishments.
#[test]
fn an_embellished_prime_in_a_template_is_so_many_primes() {
    let mut embellished = c5(0, 2, '\'');
    embellished[1] |= 0x01;
    embellished.extend_from_slice(&[0x06, 0, 5, 0x06, 0, 18, 0]);
    let template = template5(21, 0, &[Some(&[]), None, None, Some(&embellished)]);
    let mathml = settled_mathml(&eq5(&[c5(0, 3, 'x'), template].concat()));
    assert!(mathml.contains("<msup><mi>x</mi>"), "{mathml}");
    assert_eq!(mathml.matches("<msup>").count(), 1, "{mathml}");
    assert_eq!(mathml.matches('′').count(), 5, "{mathml}");
}

/// What a leading script leads is coloured part by part, also where the line
/// it was read in joins one in another colour.
#[test]
fn a_led_item_is_coloured_part_by_part() {
    let (blue, red) = (vec![0x0F, 1], vec![0x0F, 3]);
    let define = [
        0x10, 0, 0, 0, 0, 0, 0xE8, 0x03, 0x10, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 0xE8, 0x03, 0, 0, 0, 0,
    ];
    let led = [
        blue.clone(),
        script5(1, &c5(0, 3, 'a')),
        red.clone(),
        c5(0, 3, 'x'),
        c5(0, 2, '\''),
    ]
    .concat();
    let drawn = template5(23, 0x30, &[Some(&led), None, None]);
    let content = [&red[..], &drawn, &blue, &c5(0, 6, '=')].concat();
    let script = template5(28, 0, &[None, Some(&content)]);
    let data = eq5(&[&define[..], &blue, &c5(0, 3, 'y'), &script].concat());
    let mathml = settled_mathml(&data);
    assert_eq!(mathml.matches("#ff0000").count(), 2, "{mathml}");
}

/// A prime in another colour that its item does not take changes nothing.
#[test]
fn a_prime_that_is_not_taken_leaves_the_colours() {
    let (blue, black) = (vec![0x0F, 1], vec![0x0F, 2]);
    let sin = [c5(0, 2, 's'), c5(0, 2, 'i'), c5(0, 2, 'n')].concat();
    let prime = c5(0, 2, '\'');
    let script = script5(0, &c5(0, 3, 'a'));
    let drawn = template5(23, 0x30, &[Some(&script), None, None]);
    let data = eq5(&[
        colours(),
        black.clone(),
        sin,
        drawn,
        blue.clone(),
        prime.clone(),
    ]
    .concat());
    let mathml = settled_mathml(&data);
    assert_eq!(mathml.matches("#0000ff").count(), 1, "{mathml}");
    assert!(
        mathml.contains("<mstyle mathcolor=\"#0000ff\"><mo>′"),
        "{mathml}"
    );
    // Blue template, black x with a leading script, black prime.
    let content = [script5(1, &c5(0, 3, 'a')), black.clone(), c5(0, 3, 'x')].concat();
    let template = template5(23, 0x30, &[Some(&content), None, None]);
    let data = eq5(&[colours(), blue, template, black, prime].concat());
    let mathml = settled_mathml(&data);
    let after = mathml.rsplit_once("#0000ff").unwrap().1;
    assert!(!after.contains('′'), "{mathml}");
}

/// A possessive after a script on an empty fence, which is not written.
#[test]
fn a_possessive_after_a_script_on_an_empty_fence_settles() {
    let fence = template5(1, 0, &[Some(&[])]);
    let content = [
        fence,
        script5(0, &c5(0, 3, 'c')),
        c5(0, 2, '\''),
        c5(0, 3, 's'),
        line5(&c5(0, 3, 'x')),
    ]
    .concat();
    let data = eq5(&content);
    settles(&data);
    let mut result = crate::from_mtef_bytes(&data).unwrap();
    for _ in 0..3 {
        let mathml = result.to_mathml().unwrap();
        assert!(!mathml.contains('′'), "{mathml}");
        let written = result
            .to_mtef_bytes(&crate::MtefWriteOptions::default())
            .unwrap();
        result = crate::from_mtef_bytes(&written).unwrap();
    }
}

/// Generated equations, reduced: how coloured parts are grouped.
#[test]
fn reduced_generated_equations_are_grouped_by_colour() {
    for (digits, expected) in [
        // Scripts on nothing that a leading script leads are coloured part by part.
        (
            concat!(
                "05000654360000100000000000e803100000000000000010e80300000000",
                "0a010003001b01000100020083780000000f0302009801ef03001b000001",
                "00010002000f0100009801ef02000082270000010100",
            ),
            r##"<mmultiscripts><msub><mspace width="0em"/><mstyle"##,
        ),
        // Two coloured nodes side by side are one.
        (
            concat!(
                "05534d54360000100000000000010101010100021c010001010100010002",
                "008227000f010300000001000200863d0002003d000000000200000000",
            ),
            r##"<mo>=</mo><mo>"##,
        ),
        // A function name takes no scripts on nothing in another colour.
        (
            concat!(
                "05000654360000100000000000000a010002000000010002008273000f01",
                "0300150000010003001c0101000200863220020083780003000001010100",
                "0200837300020083780000000001010101010003001b0100010002000300",
                "1500000000",
            ),
            r##"<mmultiscripts><mo>′</mo>"##,
        ),
        // What was painted apart in one colour is one.
        (
            concat!(
                "05000654360000100000000000e80310000000000000001000e803000000",
                "00030000010002008831000f03000200863d0003001b0100010001000f01",
                "0201822700060012000001000200863d000000009801ef0200000000",
            ),
            r##"<mprescripts/><mstyle mathcolor="#0000ff"><mrow>"##,
        ),
    ] {
        let data: Vec<u8> = (0..digits.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
            .collect();
        let result = crate::from_mtef_bytes_with(&data, &lenient()).unwrap();
        let mathml = result.to_mathml().unwrap();
        assert!(mathml.contains(expected), "{expected}: {mathml}");
        settles(&data);
    }
    // Two coloured nodes are one, not only three.
    let digits = "05534d54360000100000000000010101010100021c010001010100010002008227000f010300000001000200863d0002003d000000000200000000";
    let data: Vec<u8> = (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
        .collect();
    let result = crate::from_mtef_bytes_with(&data, &lenient()).unwrap();
    let mathml = result.to_mathml().unwrap();
    assert_eq!(mathml.matches("mathcolor").count(), 1, "{mathml}");
}

/// The zero spaces at the ends of slots are dropped, in the tree as it is read.
#[test]
fn zero_spaces_at_the_ends_of_slots_are_dropped() {
    let zero = c5(0, 24, '\u{EF01}');
    let x = c5(0, 3, 'x');
    let y = c5(0, 3, 'y');
    let top = [&x[..], &y, &zero].concat();
    let bottom = [&zero[..], &x, &y, &zero, &zero].concat();
    for content in [&top, &bottom, &zero] {
        let fraction = template5(11, 0, &[Some(content), Some(&x)]);
        let tree = MtefReader::parse_bytes(&eq5(&fraction), &Config::default()).unwrap();
        let tree = format!("{:?}", tree.expr);
        assert!(!tree.contains("Space"), "{tree}");
    }
    // Primes before one are primes.
    let prime = c5(0, 2, '\'');
    let sup = template5(
        28,
        0,
        &[None, Some(&[&prime[..], &prime, &zero, &prime].concat())],
    );
    let mathml = settled_mathml(&eq5(&[&x[..], &sup, &script5(0, &y)].concat()));
    assert!(mathml.contains("<msubsup>"), "{mathml}");
}

/// Primes on primes join an item in the order in which they are drawn.
#[test]
fn primes_of_a_template_keep_their_order() {
    let mark = |ch: char| c5(0, 6, ch);
    let sup = |ch: char| template5(28, 0, &[None, Some(&mark(ch))]);
    let content = [mark('\u{2032}'), sup('\u{2033}'), sup('\u{2034}')].concat();
    let template = template5(21, 0, &[Some(&[]), None, None, Some(&content)]);
    let data = eq5(&[c5(0, 3, 'x'), template].concat());
    let mathml = settled_mathml(&data);
    let marks: String = mathml
        .chars()
        .filter(|ch| ('\u{2032}'..='\u{2034}').contains(ch))
        .collect();
    assert_eq!(marks, "\u{2032}\u{2033}\u{2034}", "{mathml}");
}

/// The zero space that keeps an apostrophe is written before whatever draws a
/// letter, and not at the start of the next line.
#[test]
fn a_possessive_keeps_its_apostrophe() {
    for rest in [
        r"\hat{y}",
        "y_1",
        r"\mathbf{y}",
        r"\textcolor{red}{y}",
        r"\vec{y}",
        "yz",
    ] {
        for latex in [
            format!(r"x\char`'s{rest}"),
            format!(r"{{\color{{blue}}x\char`'s}}{rest}"),
        ] {
            let mtef = crate::convert(&latex, crate::Format::LaTeX, crate::Format::Mtef).unwrap();
            let mathml = crate::convert(&mtef, crate::Format::Mtef, crate::Format::MathML).unwrap();
            assert!(
                mathml.contains("&apos;") && !mathml.contains('′'),
                "{latex}: {mathml}"
            );
        }
    }
    let latex = r"\begin{gathered} x\char`'s \\ y \end{gathered}";
    let mtef = crate::convert(latex, crate::Format::LaTeX, crate::Format::Mtef).unwrap();
    assert!(!mtef.contains("9801ef"), "{mtef}");
}

/// The style byte of FONT is 1 bold and 2 italic in MTEF 2 and 3; in MTEF 4,
/// without evidence against it, as its description says.
#[test]
fn font_records_give_bold_and_italic() {
    let equation = |version: u8, style: u8| {
        let mut data = vec![version, 1, 1, 3, 0, 10, 1, 8, 1, style];
        data.extend_from_slice(b"Arial\0");
        data.extend_from_slice(&[2, 127, b'x']);
        if version == 3 {
            data.push(0);
        }
        data.extend_from_slice(&[0, 0]);
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap()
    };
    for version in [2, 3] {
        assert_eq!(equation(version, 0), r"\mathrm{x}");
        assert_eq!(equation(version, 1), r"\mathbf{x}");
        assert_eq!(equation(version, 2), "x");
        assert_eq!(equation(version, 3), r"\boldsymbol{x}");
    }
    assert_eq!(equation(4, 1), "x");
    assert_eq!(equation(4, 2), r"\mathbf{x}");
}

/// COLOR_DEF blue, black; COLOR `n` selects one.
fn colours() -> Vec<u8> {
    vec![0x10, 0, 0, 0, 0, 0, 0xE8, 0x03, 0x10, 0, 0, 0, 0, 0, 0, 0]
}

/// The equation's own line is assembled when the outer list sees it: a prime
/// in another colour stays beside the coloured run.
#[test]
fn a_prime_after_a_coloured_run_is_not_the_runs() {
    let prime = c5(0, 2, '\'');
    let run =
        |items: &[Vec<u8>]| [vec![0x0F, 1], items.concat(), vec![0x0F, 2], prime.clone()].concat();
    let (x, equals) = (c5(0, 3, 'x'), c5(0, 6, '='));
    for items in [
        vec![x.clone()],
        vec![x.clone(), equals.clone()],
        vec![c5(0, 1, 'a'), c5(0, 1, 'b')],
        vec![c5(0, 2, 's'), c5(0, 2, 'i'), c5(0, 2, 'n')],
    ] {
        let run = run(&items);
        // On the main line, in a fraction and in a subscript.
        let mut fraction = vec![0x03, 0, 11, 0, 0];
        fraction.extend(line5(&run));
        fraction.extend(line5(&x));
        fraction.push(0);
        let mut subscript = [x.clone(), vec![0x03, 0, 27, 0, 0]].concat();
        subscript.extend(line5(&run));
        subscript.extend_from_slice(&[0x01, 0x01, 0]);
        for content in [run.clone(), fraction, subscript] {
            let data = eq5(&[colours(), content].concat());
            let mathml = crate::from_mtef_bytes(&data).unwrap().to_mathml().unwrap();
            assert!(!mathml.contains("<msup>"), "{mathml}");
            assert!(mathml.contains("#0000ff"), "{mathml}");
            settles(&data);
        }
    }
}

/// Lines inside a line only group: their items were assembled on their own.
#[test]
fn a_prime_in_a_line_of_its_own_stays_there() {
    let data = eq3(&[c3(0, 3, 'x'), line3(&c3(0, 2, '\''))].concat());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"x\prime"
    );
    let data = eq5(&[c5(0, 3, 'x'), line5(&c5(0, 2, '\''))].concat());
    assert_eq!(
        crate::from_mtef_bytes(&data).unwrap().to_latex().unwrap(),
        r"x\prime"
    );
    // A template's run is the line's.
    let data = eq3(&intop(&c3(0, 3, 'x'), &c3(0, 3, 'y')));
    settles(&data);
}

/// Coloured zero spaces, primes and scripts on nothing from other formats.
#[test]
fn coloured_latex_settles() {
    for latex in [
        r"{\color{red}\hspace{0pt}}\hspace{0pt}",
        r"{\color{blue}x=}'",
        r"\frac{{\color{blue}x=}'}{y}",
        r"x_{{\color{blue}ab}'}",
        r"{\color{blue}'\hspace{0pt}}{\color{red}v}",
        r"{\color{blue}^2\hspace{0pt}}{\color{red}x}",
        r"{\color{red}'r\prime}'",
        r"^b{\color{blue}\hspace{0pt}\sin'}",
        r"_t{\color{blue}\sin_9}\prime",
        r"{\color{blue}^{0}u\prime_{b}}_q",
        r"'{\color{blue})_\}}\prime",
        r"_,{\color{red},_n}",
        r"^x{\color{red}\hspace{0pt}\sin}^8",
        r"\sin{\color{black}{\color{black}_b}}\prime",
    ] {
        let first = crate::from_latex_with(latex, &lenient()).unwrap();
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let options = crate::MtefWriteOptions::new(version);
            let one = crate::from_mtef_bytes(&first.to_mtef_bytes(&options).unwrap()).unwrap();
            let written = one.to_mtef_bytes(&options).unwrap();
            let two = crate::from_mtef_bytes(&written).unwrap();
            assert_eq!(
                two.to_mtef_bytes(&options).unwrap(),
                written,
                "{version:?} {latex}"
            );
        }
    }
}
