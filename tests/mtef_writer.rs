//! MTEF writer tests: real equations written back as the same kind of
//! object, and LaTeX written as MTEF 5 and MTEF 3, must read back to the same
//! AST.

use std::path::Path;

use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::{DisplayStyle, FencedItem, LimitStyle, MathNode};
use mathema::{
    CompactExpr, Config, MathExpr, MtefVersion, MtefWriteOptions, from_latex, from_latex_with,
    from_mtef_bytes,
};

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mtef");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect()
}

/// Every fixture, written with the version and product it declares, reads
/// back to the same AST and header identity, and writing it again gives the
/// same bytes.
#[test]
fn fixtures_round_trip_as_the_same_kind_of_object() {
    for (name, bytes) in fixtures() {
        let original = from_mtef_bytes(&bytes).unwrap();
        let info = original.mtef_info().unwrap();
        let options = MtefWriteOptions::from_info(info);
        let written = original.to_mtef_bytes(&options).unwrap();
        let back = from_mtef_bytes(&written).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back.expr(), original.expr(), "{name}");
        let back_info = back.mtef_info().unwrap();
        assert_eq!(back_info.version, info.version, "{name}");
        assert_eq!(back_info.product, info.product, "{name}");
        assert_eq!(back_info.product_version, info.product_version, "{name}");
        assert_eq!(
            back_info.product_subversion, info.product_subversion,
            "{name}"
        );
        assert_eq!(back_info.application_key, info.application_key, "{name}");
        assert_eq!(back_info.inline, info.inline, "{name}");
        assert_eq!(
            back_info.ole_header.is_some(),
            info.ole_header.is_some(),
            "{name}"
        );
        assert_eq!(back.to_mtef_bytes(&options).unwrap(), written, "{name}");
    }
}

/// Every fixture can also be written as the other MTEF version and read in
/// Strict mode.
#[test]
fn fixtures_convert_between_versions() {
    for (name, bytes) in fixtures() {
        let original = from_mtef_bytes(&bytes).unwrap();
        for version in [MtefVersion::V3, MtefVersion::V5] {
            let written = original
                .to_mtef_bytes(&MtefWriteOptions::new(version))
                .unwrap();
            let back = from_mtef_bytes(&written).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(back.mtef_info().unwrap().version, version, "{name}");
        }
    }
}

/// MTEF has no place for an operator's limit style, for the default limit
/// placement of a big operator (it is written as the operator's usual one),
/// or for array stretch and jot; drop them before comparing.
fn canonical(node: &mut MathNode) {
    match node {
        MathNode::Operator(attr) => attr.limit_style = LimitStyle::Default,
        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            if *limits == LimitStyle::Default {
                *limits = if matches!(op, '\u{222B}'..='\u{2233}') {
                    LimitStyle::SubSup
                } else {
                    LimitStyle::AboveBelow
                };
            }
            for n in [sub, sup].into_iter().flatten() {
                canonical(n);
            }
            canonical(body);
        },
        MathNode::Array {
            rows,
            arraystretch,
            add_jot,
            ..
        } => {
            *arraystretch = None;
            *add_jot = false;
            rows.iter_mut().flatten().flatten().for_each(canonical);
        },
        MathNode::Row(children) | MathNode::Styled(_, children) => {
            children.iter_mut().for_each(canonical);
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let FencedItem::Element(n) = item {
                    canonical(n);
                }
            }
        },
        MathNode::Subscript { base, sub: script }
        | MathNode::Superscript { base, sup: script }
        | MathNode::Overscript {
            base, over: script, ..
        }
        | MathNode::Underscript {
            base,
            under: script,
            ..
        } => {
            canonical(base);
            canonical(script);
        },
        MathNode::SubSup { base, sub, sup }
        | MathNode::UnderOver {
            base,
            under: sub,
            over: sup,
            ..
        } => {
            canonical(base);
            canonical(sub);
            canonical(sup);
        },
        MathNode::Fraction { num, den, .. } => {
            canonical(num);
            canonical(den);
        },
        MathNode::Root { index, radicand } => {
            canonical(index);
            canonical(radicand);
        },
        MathNode::Sqrt(n)
        | MathNode::Accent { base: n, .. }
        | MathNode::Cancel { content: n, .. }
        | MathNode::Boxed { content: n, .. } => canonical(n),
        _ => {},
    }
}

fn canonical_expr(mut expr: MathExpr) -> MathExpr {
    expr.nodes.iter_mut().for_each(canonical);
    expr
}

fn assert_round_trip(latex: &str, version: MtefVersion) {
    let result = from_latex(latex).unwrap();
    let mut compact: CompactExpr = result.compact_expr().clone();
    normalize_compact_expr(&mut compact).unwrap();
    let expected = canonical_expr(compact.to_math_expr());
    let bytes = result
        .to_mtef_bytes(&MtefWriteOptions::new(version))
        .unwrap();
    let back = from_mtef_bytes(&bytes).unwrap_or_else(|e| panic!("{latex} ({version:?}): {e}"));
    assert_eq!(back.mtef_info().unwrap().version, version);
    assert_eq!(
        canonical_expr(back.expr()),
        expected,
        "{latex} ({version:?})"
    );
}

/// Expressions both MTEF 3 and MTEF 5 represent exactly.
const BOTH: &[&str] = &[
    r"\frac{a}{b}",
    r"\frac{1}{2}+\frac{x+1}{x-1}",
    r"\frac{\frac{a}{b}}{c}",
    r"\tfrac{1}{2}",
    r"a/b",
    r"\sqrt{x}",
    r"\sqrt[3]{x+1}",
    r"\sqrt[n]{\frac{a}{b}}",
    r"x^2",
    r"x_i",
    r"x_i^2",
    r"x^{2n}",
    r"e^{-x^2}",
    r"a_{i,j}",
    r"x^{y^z}",
    r"x'",
    r"f''(x)",
    r"{}_1^2X",
    r"{}^{14}_{6}C",
    r"12^3",
    r"\sin^2 x + \cos^2 x = 1",
    r"\left( x \right)",
    r"\left[ x \right]",
    r"\left\{ x \right\}",
    r"\left| x \right|",
    r"\left\| x \right\|",
    r"\left\langle x \right\rangle",
    r"\left\lfloor x \right\rfloor",
    r"\left\lceil x \right\rceil",
    r"\left( x \right]",
    r"\left[ x \right)",
    r"\left. x \right|",
    r"\left( x \right.",
    r"\left\langle a \middle| b \right\rangle",
    r"(a+b)^2",
    r"\sum_{i=1}^{n} x_i",
    r"\sum_{i} x",
    r"\sum x",
    r"\sum^{n} x",
    r"\prod_{k=1}^{n} k",
    r"\coprod_i A_i",
    r"\bigcup_{i} A_i",
    r"\bigcap_{i=1}^{n} A_i",
    r"\int_0^1 f(x)\,dx",
    r"\int x\,dx",
    r"\iint_D f",
    r"\iiint_V f",
    r"\oint_C f",
    r"\int\limits_0^1 f",
    r"\sum\nolimits_{i} x",
    r"\bigoplus_{i} V_i",
    r"\lim_{x\to 0} \frac{\sin x}{x}",
    r"\max_{a} b",
    r"\limsup_{n} a_n",
    r"\liminf_{n} a_n",
    r"\injlim_{i} A_i",
    r"\projlim_{i} A_i",
    r"\limsup",
    r"\liminf",
    r"\injlim",
    r"\projlim",
    r"\operatorname*{lim\,sup}",
    r"\operatorname*{lim\,inf}_{n} a_n",
    r"\lim\,\sup_{n} a_n",
    r"\lim \sup",
    r"\begin{matrix} a & b \\ c & d \end{matrix}",
    r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}",
    r"\begin{bmatrix} a & b & c \end{bmatrix}",
    r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
    r"f(x)=\begin{cases} 1 & x>0 \\ 0 & \text{otherwise} \end{cases}",
    r"\begin{array}{c} a \\ b \end{array}",
    r"\begin{array}{|c|c|} \hline a & b \\ \hline c & d \\ \hline \end{array}",
    r"\hat{x}",
    r"\tilde{y}",
    r"\bar{z}",
    r"\vec{v}",
    r"\dot{a}",
    r"\ddot{b}",
    r"\overline{ab}",
    r"\underline{ab}",
    r"\overrightarrow{AB}",
    r"\overleftarrow{AB}",
    r"\hat{\dot{x}}",
    r"\overbrace{a+b}^{n}",
    r"\underbrace{c+d}_{m}",
    r"\overbrace{a+b}",
    r"\xrightarrow[g]{f}",
    r"\xrightarrow{f}",
    r"\xleftarrow{f}",
    r"\overset{a}{b}",
    r"\underset{a}{b}",
    r"\cancel{x}",
    r"\text{if } x > 0",
    r"\text{otherwise}",
    r"\alpha + \beta = \gamma",
    r"\Gamma \Delta \Theta \Lambda \Xi \Pi \Sigma \Phi \Psi \Omega",
    r"\varepsilon \vartheta \varphi \varrho",
    r"\sin x \cos y \tan z \log w \ln v \exp u",
    r"\sin\cos x",
    r"\operatorname{rank} A",
    r"\mathbf{v} + \mathbf{w}",
    r"\mathrm{d}x",
    r"\mathbf{12}",
    r"x\mathbf{y}",
    r"3.14159",
    r"1+2=3",
    r"a \le b \ge c \ne d",
    r"\infty \partial \nabla \forall \exists",
    r"a \cdot b \times c \div d \pm e \mp f",
    r"a \quad b \qquad c",
    r"a\,b\;c\:d\ e",
    r"x \in A \subset B \cup C \cap D",
    r"\angle ABC = 90^\circ",
    r"a \to b \Rightarrow c \Leftrightarrow d",
    r"\ldots \cdots",
    r"\sqrt{x^2+y^2}",
    r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}",
];

/// Expressions only MTEF 5 has templates for (wide accents, strikes, boxes).
const MTEF5_ONLY: &[&str] = &[
    r"\widehat{xy}",
    r"\widetilde{abc}",
    r"\cancel{x+y}",
    r"\bcancel{y}",
    r"\xcancel{z}",
    r"\boxed{x+1}",
];

#[test]
fn latex_round_trips_through_mtef5() {
    for latex in BOTH.iter().chain(MTEF5_ONLY) {
        assert_round_trip(latex, MtefVersion::V5);
    }
}

#[test]
fn latex_round_trips_through_mtef3() {
    for latex in BOTH {
        assert_round_trip(latex, MtefVersion::V3);
    }
}

/// A write that succeeds reads back, in every mode; an AST the reader would
/// rebuild too deep is refused with a limit error instead.
#[test]
fn written_output_always_reads_back() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mtef-writer/fuzz-nested-piles.bin");
    let data = std::fs::read(path).unwrap();
    let lenient = mathema::Config::default().with_strictness(mathema::Strictness::Lenient);
    let result = mathema::from_mtef_bytes_with(&data, &lenient).unwrap();
    for version in [MtefVersion::V5, MtefVersion::V3] {
        match result.to_mtef_bytes(&MtefWriteOptions::new(version)) {
            Ok(bytes) => {
                from_mtef_bytes(&bytes).unwrap();
                mathema::from_mtef_bytes_with(&bytes, &lenient).unwrap();
            },
            Err(err) => assert!(
                matches!(
                    err,
                    mathema::Error::LimitExceeded {
                        kind: mathema::LimitKind::Depth,
                        ..
                    }
                ),
                "{err}"
            ),
        }
    }
}

/// The read-back check ignores the caller's input-size limit, which is for
/// reading, not writing.
#[test]
fn read_back_check_ignores_the_input_size_limit() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mtef-writer/fuzz-nested-piles.bin");
    let data = std::fs::read(path).unwrap();
    let lenient = mathema::Config::default().with_strictness(mathema::Strictness::Lenient);
    let result = mathema::from_mtef_bytes_with(&data, &lenient).unwrap();
    let compact = result.compact_expr();
    let small = lenient.with_max_input_bytes(Some(100));
    for version in [MtefVersion::V5, MtefVersion::V3] {
        let written = mathema::writer::mtef::MtefWriter::write_bytes(
            compact,
            &small,
            &MtefWriteOptions::new(version),
        );
        match written {
            Ok(bytes) => {
                from_mtef_bytes(&bytes).unwrap();
            },
            Err(err) => assert!(
                matches!(
                    err,
                    mathema::Error::LimitExceeded {
                        kind: mathema::LimitKind::Depth,
                        ..
                    }
                ),
                "{err}"
            ),
        }
    }
}

#[test]
fn mtef5_horizontal_bracket_templates_keep_their_operand_and_label() {
    let options = MtefWriteOptions::new(MtefVersion::V5);
    for input in [
        r"\overbracket{a+b}",
        r"\underbracket{x}",
        r"\overbracket{a+b}^n",
        r"\underbracket{x}_b",
    ] {
        let original = from_latex(input).unwrap();
        let bytes = original.to_mtef_bytes(&options).unwrap();
        let decoded = from_mtef_bytes(&bytes).unwrap();
        assert_eq!(
            original.to_latex().unwrap(),
            decoded.to_latex().unwrap(),
            "{input}"
        );
        assert_eq!(decoded.to_mtef_bytes(&options).unwrap(), bytes, "{input}");
    }
}

#[test]
fn mtef5_compound_operator_limits_use_native_bigop_slots() {
    let options = MtefWriteOptions::new(MtefVersion::V5);
    for (input, selector, expected_marker) in [
        (r"\mathop{a+b}\limits^⎴", 22u8, r"\mathop{a+ b}\limits^{⎴}"),
        (
            r"\mathop{a+b}\nolimits^⎴",
            21u8,
            r"\mathop{a+ b}\nolimits^{⎴}",
        ),
        (
            r"\mathop{a+b}\limits_⎵^⎴",
            22u8,
            r"\mathop{a+ b}\limits_{⎵}^{⎴}",
        ),
    ] {
        let original = from_latex(input).unwrap();
        let bytes = original.to_mtef_bytes(&options).unwrap();
        assert!(
            bytes
                .windows(4)
                .any(|window| window == [0x03, 0x00, selector, 0x60]
                    || window == [0x03, 0x00, selector, 0x20]
                    || window == [0x03, 0x00, selector, 0x70]),
            "{input} did not use tmINTOP/tmSUMOP selector {selector}: {:02x?}",
            bytes
        );
        let decoded = from_mtef_bytes(&bytes).unwrap();
        assert_eq!(decoded.to_latex().unwrap(), expected_marker, "{input}");
        assert_eq!(decoded.to_mtef_bytes(&options).unwrap(), bytes, "{input}");
    }

    for operator in ["A", r"\alpha", "+"] {
        for modifier in [r"\limits", r"\nolimits"] {
            let input = format!(r"\mathop{{{operator}}}{modifier}_i^j");
            let original = from_latex(&input).unwrap();
            let bytes = original.to_mtef_bytes(&options).unwrap();
            let decoded = from_mtef_bytes(&bytes).unwrap();
            assert_eq!(
                decoded.to_latex().unwrap(),
                original.to_latex().unwrap(),
                "{input}"
            );
            assert_eq!(decoded.to_mtef_bytes(&options).unwrap(), bytes, "{input}");
        }
    }
    // A typed bracket accent remains the tmHBRACK decoration template and
    // never becomes a general operator-limit template.
    let bracket = from_latex(r"\overbracket{\mathop{a+b}}").unwrap();
    let bytes = bracket.to_mtef_bytes(&options).unwrap();
    assert!(
        bytes
            .windows(4)
            .any(|window| window == [0x03, 0x00, 25, 0x01])
    );
    assert!(
        !bytes
            .windows(4)
            .any(|window| window == [0x03, 0x00, 21, 0x20])
    );
    assert_eq!(
        from_mtef_bytes(&bytes).unwrap().to_latex().unwrap(),
        r"\overbracket{a+ b}"
    );
}

#[test]
fn movable_compound_limits_follow_equation_display_style() {
    let options = MtefWriteOptions::new(MtefVersion::V5);
    let block = from_latex(r"\mathop{a+b}^i").unwrap();
    let block_bytes = block.to_mtef_bytes(&options).unwrap();
    assert!(
        block_bytes
            .windows(4)
            .any(|window| window[0..3] == [3, 0, 22])
    );
    let (_, block_diagnostics) = block.to_mtef_bytes_with_diagnostics(&options).unwrap();
    assert!(
        block_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("movable limits")),
        "MTEF should report the loss of movable compound placement"
    );

    let inline = from_latex_with(
        r"\mathop{a+b}^i",
        &Config::default().with_display(DisplayStyle::Inline),
    )
    .unwrap();
    let inline_bytes = inline.to_mtef_bytes(&options).unwrap();
    assert!(
        inline_bytes
            .windows(4)
            .any(|window| window[0..3] == [3, 0, 21])
    );
    assert!(
        !inline_bytes
            .windows(4)
            .any(|window| window[0..3] == [3, 0, 22])
    );

    let decoded = from_mtef_bytes(&inline_bytes).unwrap();
    assert!(decoded.to_latex().unwrap().contains(r"\mathop{a+ b}"));
    assert_eq!(decoded.to_mtef_bytes(&options).unwrap(), inline_bytes);
}

#[test]
fn mtef3_compound_operator_limits_use_general_bigop_slots() {
    let options = MtefWriteOptions::new(MtefVersion::V3);
    for (input, selector, expected) in [
        (r"\mathop{a+b}\limits^⎴", 43u8, r"\mathop{a+ b}\limits^{⎴}"),
        (
            r"\mathop{a+b}\nolimits^⎴",
            42u8,
            r"\mathop{a+ b}\nolimits^{⎴}",
        ),
    ] {
        let original = from_latex(input).unwrap();
        let bytes = original.to_mtef_bytes(&options).unwrap();
        assert!(
            bytes.windows(3).any(|window| window == [0x03, selector, 0]),
            "{input} did not use the native MTEF 3 general operator slot"
        );
        let decoded = from_mtef_bytes(&bytes).unwrap();
        assert_eq!(decoded.to_latex().unwrap(), expected, "{input}");
        assert_eq!(decoded.to_mtef_bytes(&options).unwrap(), bytes, "{input}");
    }
}
