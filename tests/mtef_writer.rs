//! MTEF writer tests: real equations written back as the same kind of
//! object, and LaTeX written as MTEF 5 and MTEF 3, must read back to the same
//! AST.

use std::path::Path;

use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::{FencedItem, LimitStyle, MathNode};
use mathema::{CompactExpr, MathExpr, MtefVersion, MtefWriteOptions, from_latex, from_mtef_bytes};

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
