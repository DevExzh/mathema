//! Leading scripts (`{}^{14}_{6}C`, MathML `mprescripts`, OMML `m:sPre`,
//! MTEF scripts with tvSU_PRECEDES) belong to what follows them, in every
//! format and across conversions.

use mathema::{
    Format, MathNode, MtefWriteOptions, convert, from_latex, from_mathml, from_mtef_bytes,
    from_omml, from_unicode_math,
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
        (r"x^2{}^3y", "y"),
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
        for format in [
            Format::LaTeX,
            Format::MathML,
            Format::Omml,
            Format::Mtef,
            Format::UnicodeMath,
        ] {
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
        for to in [
            Format::LaTeX,
            Format::MathML,
            Format::Omml,
            Format::Mtef,
            Format::UnicodeMath,
        ] {
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
        "upstream-v5-leading-space-script.bin",
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
        // And through UnicodeMath (`(_b^a)c`); the other files use notation
        // the UnicodeMath reader does not read back (primes as `^'`).
        if name != "upstream-v5-leading-scripts.bin" {
            continue;
        }
        let unicode = result.to_unicode_math().unwrap();
        let reread = from_unicode_math(&unicode).unwrap();
        let mut compact = reread.compact_expr().clone();
        mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        assert_eq!(
            bases(&compact.to_math_expr()).len(),
            expected.len(),
            "{name}: {unicode}"
        );
    }
}

#[test]
fn unicode_math_leading_scripts_use_the_parenthesized_form() {
    // UTN #28 section 3.3: `(_c^b)a`.
    let written = convert(r"x={}^{14}_{6}C", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert_eq!(written, "x=(_6^14)C");
    let back = from_unicode_math(&written).unwrap();
    assert_eq!(
        bases(&back.expr()),
        vec![format!("{:?}", MathNode::Identifier("C".into()))]
    );
    let half = from_unicode_math("(_1^)F_1").unwrap();
    assert_eq!(bases(&half.expr()).len(), 1, "{:?}", half.expr());
}

/// A script after a big operator is its limit, as TeX-to-MathML converters
/// write `\int_C`: `<mo>∫</mo><msub><mrow/><mi>C</mi></msub>`.
#[test]
fn empty_base_script_after_a_big_operator_is_its_limit() {
    let normalized = |mathml: &str| {
        let mut compact = from_mathml(mathml).unwrap().compact_expr().clone();
        mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        compact.to_math_expr()
    };
    let integral = "<math><mo>∫</mo><msub><mrow/><mi>C</mi></msub><mrow><mo>(</mo><mi>z</mi><mo>)</mo></mrow></math>";
    assert!(bases(&normalized(integral)).is_empty());
    assert_eq!(
        convert(integral, Format::MathML, Format::LaTeX).unwrap(),
        r"{\int}_C(z)"
    );
    let power =
        "<math><mo>∫</mo><msub><mrow/><mi>C</mi></msub><msup><mi>z</mi><mi>n</mi></msup></math>";
    assert!(bases(&normalized(power)).is_empty());
    assert!(
        convert(power, Format::MathML, Format::LaTeX)
            .unwrap()
            .starts_with(r"{\int}_C")
    );
    let lim =
        "<math><mi>lim</mi><msub><mrow/><mi>n</mi></msub><msup><mi>a</mi><mi>n</mi></msup></math>";
    assert!(bases(&normalized(lim)).is_empty());
}

fn normalized(result: &mathema::ConvertResult) -> mathema::MathExpr {
    let mut compact = result.compact_expr().clone();
    mathema::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
    compact.to_math_expr()
}

/// A script after a function name is that function's script (or limit).
#[test]
fn empty_base_script_after_a_function_name_is_its_script() {
    let latex = [
        r"\sin {}^2 x",
        r"\log {}_2 x",
        r"\operatorname{rank} {}_x f",
        r"\lim {}_n a",
    ];
    for input in latex {
        let result = from_latex(input).unwrap();
        assert!(bases(&normalized(&result)).is_empty(), "{input}");
        for format in [
            Format::LaTeX,
            Format::MathML,
            Format::Omml,
            Format::Mtef,
            Format::UnicodeMath,
        ] {
            let written = convert(input, Format::LaTeX, format).unwrap();
            let back = convert(&written, format, Format::MathML).unwrap();
            assert!(
                !back.contains("mprescripts"),
                "{input} via {format:?}: {back}"
            );
        }
    }
    let mathml = [
        "<math><mi>sin</mi><msup><mrow/><mn>2</mn></msup><mi>x</mi></math>",
        "<math><mo>log</mo><msub><mrow/><mn>2</mn></msub><mi>x</mi></math>",
    ];
    for input in mathml {
        assert!(
            bases(&normalized(&from_mathml(input).unwrap())).is_empty(),
            "{input}"
        );
        let omml = convert(input, Format::MathML, Format::Omml).unwrap();
        assert!(!omml.contains("m:sPre"), "{input}: {omml}");
        assert!(
            bases(&normalized(&from_omml(&omml).unwrap())).is_empty(),
            "{omml}"
        );
    }
    let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:func><m:fName><m:r><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func></m:oMath>"#;
    assert!(
        convert(omml, Format::Omml, Format::LaTeX)
            .unwrap()
            .contains("sin")
    );
}

/// A word written as one MathML operator is kept whole.
#[test]
fn operator_words_are_written_whole_in_latex() {
    let cases = [
        (
            "<math><msub><mo>lim</mo><mi>n</mi></msub><mi>a</mi></math>",
            r"\lim",
        ),
        ("<math><mo>lim</mo><mi>a</mi></math>", r"\lim a"),
        (
            "<math><mo>rank</mo><mi>A</mi></math>",
            r"\operatorname{rank}A",
        ),
    ];
    for (mathml, expected) in cases {
        let latex = convert(mathml, Format::MathML, Format::LaTeX).unwrap();
        assert!(latex.contains(expected), "{mathml}: {latex}");
        // Reading the LaTeX back keeps the word.
        let back = convert(&latex, Format::LaTeX, Format::LaTeX).unwrap();
        let word = if expected.contains("rank") {
            r"\operatorname{rank}"
        } else {
            r"\lim"
        };
        assert!(back.contains(word), "{mathml}: {back}");
        assert_eq!(
            convert(&back, Format::LaTeX, Format::LaTeX).unwrap(),
            back,
            "{mathml}"
        );
    }
}

/// A leading script after a big operator keeps its base in LaTeX.
#[test]
fn leading_script_after_an_integral_keeps_its_base_in_latex() {
    let mathml = "<math><mo>∫</mo><mmultiscripts><mi>f</mi><mprescripts/><mi>C</mi><none/></mmultiscripts></math>";
    let latex = convert(mathml, Format::MathML, Format::LaTeX).unwrap();
    assert_eq!(latex, r"\int{{}_{C}f}");
    assert_eq!(bases(&normalized(&from_latex(&latex).unwrap())).len(), 1);
    // A non-ASCII character before the scripts is no command name.
    let after_text = "<math><mtext>中</mtext><mmultiscripts><mi>f</mi><mprescripts/><mi>C</mi><none/></mmultiscripts></math>";
    assert!(convert(after_text, Format::MathML, Format::LaTeX).is_ok());
}

/// Writing a parsed but not normalized expression gives the bytes its
/// re-read writes.
#[test]
fn mtef_output_of_empty_base_scripts_is_stable() {
    for input in [
        r"\lim {}_x f",
        "x^2 ^3",
        "{}^2{}^3x",
        "x^{2}_{3} '",
        r"\sin {}^2 x",
        "x{}^2",
        r"\operatorname*{foo}{}_x f",
    ] {
        for version in [mathema::MtefVersion::V5, mathema::MtefVersion::V3] {
            let options = MtefWriteOptions::new(version);
            let first = from_latex(input).unwrap().to_mtef_bytes(&options).unwrap();
            let second = from_mtef_bytes(&first)
                .unwrap()
                .to_mtef_bytes(&options)
                .unwrap();
            assert_eq!(first, second, "{input} ({version:?})");
        }
    }
}

/// LaTeX names only the function commands LaTeX and amsmath define.
#[test]
fn latex_function_names_use_real_commands() {
    for (latex, expected) in [
        (r"\lcm(a,b)", r"\operatorname{lcm}"),
        (r"\sgn x", r"\operatorname{sgn}"),
        (r"\argmax_x f", r"\operatorname*{argmax}"),
        (r"\sin x", r"\sin"),
    ] {
        let written = convert(latex, Format::LaTeX, Format::LaTeX).unwrap();
        assert!(written.contains(expected), "{latex}: {written}");
        assert_eq!(
            convert(&written, Format::LaTeX, Format::LaTeX).unwrap(),
            written,
            "{latex}"
        );
    }
}

/// An explicit leading script after a function name stays on its base.
#[test]
fn leading_script_after_a_function_name_keeps_its_base() {
    let mathml = "<math><mo>sin</mo><mmultiscripts><mi>x</mi><mprescripts/><none/><mn>2</mn></mmultiscripts></math>";
    let result = from_mathml(mathml).unwrap();
    assert_eq!(bases(&normalized(&result)).len(), 1);
    for format in [
        Format::LaTeX,
        Format::Omml,
        Format::Mtef,
        Format::UnicodeMath,
    ] {
        let written = convert(mathml, Format::MathML, format).unwrap();
        let back = convert(&written, format, Format::MathML).unwrap();
        assert!(
            back.contains("mprescripts"),
            "{format:?}: {written} -> {back}"
        );
    }
}

/// Symbol commands inside `\text` keep their characters.
#[test]
fn symbol_commands_in_text_are_kept() {
    assert_eq!(
        convert(r"\text{\int}", Format::LaTeX, Format::LaTeX).unwrap(),
        r"\text{∫}"
    );
}
