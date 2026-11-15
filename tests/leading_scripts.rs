//! Leading scripts (`{}^{14}_{6}C`, MathML `mprescripts`, OMML `m:sPre`,
//! MTEF scripts with tvSU_PRECEDES) belong to what follows them, in every
//! format and across conversions.

use mathema::{
    Format, MathNode, MtefWriteOptions, convert, from_latex, from_mathml, from_mtef_bytes,
    from_omml,
};

/// The base of the first leading-script node, as LaTeX.
fn prescript_bases(nodes: &[MathNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            MathNode::PreScripts { base, .. } => {
                out.push(format!("{base:?}"));
                prescript_bases(std::slice::from_ref(base), out);
            },
            MathNode::Row(children) | MathNode::Styled(_, children) => {
                prescript_bases(children, out)
            },
            MathNode::Array { rows, .. } => {
                for cell in rows.iter().flatten() {
                    prescript_bases(cell, out);
                }
            },
            MathNode::Fenced { content, .. } => {
                for item in content {
                    if let mathema::ast::FencedItem::Element(n) = item {
                        prescript_bases(std::slice::from_ref(n), out);
                    }
                }
            },
            MathNode::Subscript { base, sub: s } | MathNode::Superscript { base, sup: s } => {
                prescript_bases(std::slice::from_ref(base), out);
                prescript_bases(std::slice::from_ref(s), out);
            },
            MathNode::SubSup { base, sub, sup } => {
                for n in [base, sub, sup] {
                    prescript_bases(std::slice::from_ref(n), out);
                }
            },
            MathNode::Nary { body, sub, sup, .. } => {
                prescript_bases(std::slice::from_ref(body), out);
                for n in [sub, sup].into_iter().flatten() {
                    prescript_bases(std::slice::from_ref(n), out);
                }
            },
            MathNode::Fraction { num, den, .. } => {
                prescript_bases(std::slice::from_ref(num), out);
                prescript_bases(std::slice::from_ref(den), out);
            },
            _ => {},
        }
    }
}

fn bases(expr: &mathema::MathExpr) -> Vec<String> {
    let mut out = Vec::new();
    prescript_bases(&expr.nodes, &mut out);
    out
}

#[test]
fn latex_leading_scripts_belong_to_what_follows() {
    for (latex, base) in [
        (r"x={}^{14}_{6}C", "C"),
        (r"1+{}^2L", "L"),
        (r"a{}^2b", "b"),
        (r"{}_1^2X", "X"),
    ] {
        let expr = from_latex(latex).unwrap().expr();
        let mut compact = from_latex(latex).unwrap().compact_expr().clone();
        mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        let normalized = compact.to_math_expr();
        let want = vec![format!("{:?}", MathNode::Identifier(base.into()))];
        assert_eq!(
            bases(&normalized),
            want,
            "{latex}: {expr:?} -> {normalized:?}"
        );
        // Unchanged across conversion to every format and back.
        for format in [Format::LaTeX, Format::MathML, Format::Omml, Format::Mtef] {
            let written = convert(latex, Format::LaTeX, format).unwrap();
            let back = convert(&written, format, Format::LaTeX).unwrap();
            let again = from_latex(&back).unwrap();
            let mut compact = again.compact_expr().clone();
            mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
            assert_eq!(
                bases(&compact.to_math_expr()),
                want,
                "{latex} via {format:?}: {back}"
            );
        }
    }
}

#[test]
fn script_after_white_space_still_belongs_to_the_atom_before() {
    let result = from_latex("x '").unwrap();
    assert_eq!(result.to_latex().unwrap(), "x^{\\prime}");
    assert!(bases(&result.expr()).is_empty());
}

#[test]
fn mathml_and_omml_leading_scripts_keep_their_base() {
    let mathml = "<math><mi>x</mi><mo>=</mo><mmultiscripts><mi>C</mi><mprescripts/><mn>6</mn><mn>14</mn></mmultiscripts></math>";
    let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x=</m:t></m:r><m:sPre><m:sub><m:r><m:t>6</m:t></m:r></m:sub><m:sup><m:r><m:t>14</m:t></m:r></m:sup><m:e><m:r><m:t>C</m:t></m:r></m:e></m:sPre></m:oMath>"#;
    let from = [(Format::MathML, mathml), (Format::Omml, omml)];
    for (format, input) in from {
        let result = match format {
            Format::MathML => from_mathml(input).unwrap(),
            _ => from_omml(input).unwrap(),
        };
        assert_eq!(
            bases(&result.expr()).len(),
            1,
            "{format:?}: {:?}",
            result.expr()
        );
        for to in [Format::LaTeX, Format::MathML, Format::Omml, Format::Mtef] {
            let written = convert(input, format, to).unwrap();
            let back = convert(&written, to, Format::MathML).unwrap();
            assert!(
                back.contains("<mprescripts"),
                "{format:?} -> {to:?}: {back}"
            );
        }
    }
}

/// MathType's own leading scripts survive `from_mtef_bytes` and are written
/// back with tvSU_PRECEDES.
#[test]
fn mtef_leading_scripts_survive_read_and_write() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mtef-writer");
    for name in [
        "upstream-v5-leading-scripts.bin",
        "upstream-v5-leading-scripts-sums.bin",
    ] {
        let original = std::fs::read(dir.join(name)).unwrap();
        let count = |data: &[u8]| {
            data.windows(4)
                .filter(|w| w[0] == 3 && w[1] == 0 && (27..=29).contains(&w[2]) && w[3] == 1)
                .count()
        };
        let result = from_mtef_bytes(&original).unwrap();
        let expected = bases(&result.expr());
        assert_eq!(expected.len(), count(&original), "{name}");
        let options = MtefWriteOptions::from_info(result.mtef_info().unwrap());
        let written = result.to_mtef_bytes(&options).unwrap();
        assert_eq!(count(&written), count(&original), "{name}");
        let back = from_mtef_bytes(&written).unwrap();
        assert_eq!(bases(&back.expr()), expected, "{name}");
        assert_eq!(back.expr(), result.expr(), "{name}");
    }
}
