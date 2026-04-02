use mathema::ast::*;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;
use proptest::prelude::*;

// =========================================================================
// Strategies for generating AST types
// =========================================================================

fn arb_text_style() -> impl Strategy<Value = TextStyle> {
    prop_oneof![
        Just(TextStyle::Normal),
        Just(TextStyle::Bold),
        Just(TextStyle::Italic),
        Just(TextStyle::Monospace),
        Just(TextStyle::DoubleStruck),
        Just(TextStyle::Script),
        Just(TextStyle::Fraktur),
    ]
}

fn arb_fraction_kind() -> impl Strategy<Value = FractionKind> {
    prop_oneof![
        Just(FractionKind::Normal),
        Just(FractionKind::Display),
        Just(FractionKind::Inline),
    ]
}

fn arb_limit_style() -> impl Strategy<Value = LimitStyle> {
    prop_oneof![
        Just(LimitStyle::Default),
        Just(LimitStyle::AboveBelow),
        Just(LimitStyle::SubSup),
    ]
}

fn arb_identifier() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("x".to_string()),
        Just("y".to_string()),
        Just("z".to_string()),
        Just("a".to_string()),
        Just("n".to_string()),
        Just("\u{03B1}".to_string()), // alpha
        Just("\u{03B2}".to_string()), // beta
    ]
}

fn arb_number() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("0".to_string()),
        Just("1".to_string()),
        Just("2".to_string()),
        Just("42".to_string()),
    ]
}

fn arb_operator_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("sin".to_string()),
        Just("cos".to_string()),
        Just("lim".to_string()),
    ]
}

fn arb_symbol_char() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("+".to_string()),
        Just("-".to_string()),
        Just("=".to_string()),
    ]
}

fn arb_nary_op() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('\u{2211}'), // sum
        Just('\u{222B}'), // integral
        Just('\u{220F}'), // product
    ]
}

/// Generate a MathNode tree with bounded depth using prop_recursive.
fn arb_math_node() -> impl Strategy<Value = MathNode> {
    let leaf = prop_oneof![
        arb_number().prop_map(MathNode::Number),
        arb_identifier().prop_map(MathNode::Identifier),
        arb_operator_name().prop_map(|name| MathNode::Operator(OperatorAttr {
            name,
            limit_style: LimitStyle::Default,
        })),
        arb_symbol_char().prop_map(|s| MathNode::Symbol(SymbolKind::BinaryOp, s)),
    ];

    // depth=3, desired_size=15, expected_branch_size=3
    leaf.prop_recursive(3, 15, 3, |inner| {
        prop_oneof![
            // Fraction
            2 => (arb_fraction_kind(), inner.clone(), inner.clone())
                .prop_map(|(kind, num, den)| MathNode::Fraction {
                    kind,
                    num: Box::new(num),
                    den: Box::new(den),
                }),
            // Superscript
            2 => (inner.clone(), inner.clone())
                .prop_map(|(base, sup)| MathNode::Superscript {
                    base: Box::new(base),
                    sup: Box::new(sup),
                }),
            // Subscript
            2 => (inner.clone(), inner.clone())
                .prop_map(|(base, sub)| MathNode::Subscript {
                    base: Box::new(base),
                    sub: Box::new(sub),
                }),
            // SubSup
            1 => (inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(base, sub, sup)| MathNode::SubSup {
                    base: Box::new(base),
                    sub: Box::new(sub),
                    sup: Box::new(sup),
                }),
            // Sqrt
            2 => inner.clone().prop_map(|inner| MathNode::Sqrt(Box::new(inner))),
            // Root
            1 => (inner.clone(), inner.clone())
                .prop_map(|(index, radicand)| MathNode::Root {
                    index: Box::new(index),
                    radicand: Box::new(radicand),
                }),
            // Fenced
            1 => inner.clone().prop_map(|inner| MathNode::Fenced {
                open: "(".to_string(),
                close: ")".to_string(),
                content: vec![FencedItem::Element(inner)],
            }),
            // Nary
            1 => (arb_nary_op(), arb_limit_style(), inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(op, limits, sub, sup, body)| MathNode::Nary {
                    op,
                    sub: Some(Box::new(sub)),
                    sup: Some(Box::new(sup)),
                    body: Box::new(body),
                    limits,
                }),
            // Row
            1 => prop::collection::vec(inner.clone(), 1..=3)
                .prop_map(MathNode::Row),
            // Accent
            1 => inner.clone().prop_map(|base| MathNode::Accent {
                base: Box::new(base),
                accent: '\u{0302}', // hat
                position: AccentPosition::Above,
            }),
            // Styled
            1 => (arb_text_style(), inner.clone())
                .prop_map(|(style, inner)| MathNode::Styled(style, vec![inner])),
        ]
    })
}

fn arb_math_expr() -> impl Strategy<Value = MathExpr> {
    prop::collection::vec(arb_math_node(), 1..=3).prop_map(MathExpr::inline)
}

// =========================================================================
// Property: LaTeX write never panics
// =========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn latex_write_never_panics(expr in arb_math_expr()) {
        let config = Config::default();
        let _ = LatexWriter::write(&expr, &config);
    }

    #[test]
    fn mathml_write_never_panics(expr in arb_math_expr()) {
        let config = Config::default();
        let _ = MathmlWriter::write(&expr, &config);
    }

    #[test]
    fn omml_write_never_panics(expr in arb_math_expr()) {
        let config = Config::default();
        let _ = OmmlWriter::write(&expr, &config);
    }

    #[test]
    fn unicode_math_write_never_panics(expr in arb_math_expr()) {
        let config = Config::default();
        let _ = UnicodeMathWriter::write(&expr, &config);
    }
}

// =========================================================================
// Property: LaTeX round-trip is idempotent (write→parse→write = write)
// =========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn latex_roundtrip_idempotent(expr in arb_math_expr()) {
        let config = Config::default();
        if let Ok(latex1) = LatexWriter::write(&expr, &config)
            && let Ok(parsed) = LatexReader::parse(&latex1, &config)
            && let Ok(latex2) = LatexWriter::write(&parsed.expr, &config)
        {
            // Second parse should produce the same output
            let reparsed = LatexReader::parse(&latex2, &config).unwrap();
            let latex3 = LatexWriter::write(&reparsed.expr, &config).unwrap();
            assert_eq!(latex2, latex3, "LaTeX not idempotent:\n  first:  {latex2}\n  second: {latex3}");
        }
    }

    #[test]
    fn mathml_roundtrip_idempotent(expr in arb_math_expr()) {
        let config = Config::default();
        if let Ok(mathml1) = MathmlWriter::write(&expr, &config)
            && let Ok(parsed) = MathmlReader::parse(&mathml1, &config)
            && let Ok(mathml2) = MathmlWriter::write(&parsed.expr, &config)
        {
            let reparsed = MathmlReader::parse(&mathml2, &config).unwrap();
            let mathml3 = MathmlWriter::write(&reparsed.expr, &config).unwrap();
            assert_eq!(mathml2, mathml3, "MathML not idempotent:\n  first:  {mathml2}\n  second: {mathml3}");
        }
    }
}

// =========================================================================
// Property: cross-format doesn't panic (LaTeX → X → LaTeX)
// =========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn cross_format_latex_mathml_no_panic(expr in arb_math_expr()) {
        let config = Config::default();
        if let Ok(latex) = LatexWriter::write(&expr, &config)
            && let Ok(parsed) = LatexReader::parse(&latex, &config)
        {
            let _ = MathmlWriter::write(&parsed.expr, &config);
        }
    }

    #[test]
    fn cross_format_latex_omml_no_panic(expr in arb_math_expr()) {
        let config = Config::default();
        if let Ok(latex) = LatexWriter::write(&expr, &config)
            && let Ok(parsed) = LatexReader::parse(&latex, &config)
        {
            let _ = OmmlWriter::write(&parsed.expr, &config);
        }
    }

    #[test]
    fn cross_format_latex_unicode_no_panic(expr in arb_math_expr()) {
        let config = Config::default();
        if let Ok(latex) = LatexWriter::write(&expr, &config)
            && let Ok(parsed) = LatexReader::parse(&latex, &config)
        {
            let _ = UnicodeMathWriter::write(&parsed.expr, &config);
        }
    }
}

// =========================================================================
// Property: generated LaTeX strings parse without panic
// =========================================================================

fn arb_latex_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple expressions
        Just("x".to_string()),
        Just("x^2".to_string()),
        Just("x_i".to_string()),
        Just("x_i^2".to_string()),
        Just("\\frac{a}{b}".to_string()),
        Just("\\sqrt{x}".to_string()),
        Just("\\sqrt[3]{x}".to_string()),
        // Composed expressions
        (arb_identifier(), arb_number()).prop_map(|(id, num)| format!("{id}^{{{num}}}")),
        (arb_identifier(), arb_number()).prop_map(|(id, num)| format!("{id}_{{{num}}}")),
        (arb_number(), arb_number()).prop_map(|(a, b)| format!("\\frac{{{a}}}{{{b}}}")),
        (arb_identifier(),).prop_map(|(id,)| format!("\\sqrt{{{id}}}")),
        // Nested
        (arb_number(), arb_number(), arb_number(), arb_number()).prop_map(|(a, b, c, d)| format!(
            "\\frac{{\\frac{{{a}}}{{{b}}}}}{{\\frac{{{c}}}{{{d}}}}}"
        )),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn latex_parse_never_panics(input in arb_latex_fragment()) {
        let config = Config::default();
        let _ = LatexReader::parse(&input, &config);
    }

    #[test]
    fn latex_fragment_roundtrip(input in arb_latex_fragment()) {
        let config = Config::default();
        if let Ok(parsed) = LatexReader::parse(&input, &config) {
            let output = LatexWriter::write(&parsed.expr, &config).unwrap();
            let reparsed = LatexReader::parse(&output, &config).unwrap();
            assert_eq!(parsed.expr, reparsed.expr,
                "Round-trip failed: {input} → {output}");
        }
    }
}
