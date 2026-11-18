use crate::error::{CompactParseResult, Config, ParseResult, Result};
use crate::reader::Reader;

pub struct LatexReader;

impl Reader for LatexReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        config.check_input_size(input)?;
        crate::latex::parse::parse(input, config)
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        config.check_input_size(input)?;
        crate::latex::parse::parse_compact(input, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::compact::CompactNode;
    use crate::ast::*;
    use crate::error::Config;
    use crate::writer::Writer;
    use crate::writer::latex::LatexWriter;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        LatexReader::parse(input, &config).unwrap().expr.nodes
    }

    fn parse_one(input: &str) -> MathNode {
        let mut nodes = parse(input);
        assert_eq!(nodes.len(), 1, "expected 1 node, got {}", nodes.len());
        nodes.remove(0)
    }

    #[test]
    fn simple_number() {
        assert_eq!(parse_one("42"), MathNode::Number("42".into()));
        assert_eq!(parse_one("3.14"), MathNode::Number("3.14".into()));
    }

    #[test]
    fn simple_identifier() {
        assert_eq!(parse_one("x"), MathNode::Identifier("x".into()));
    }

    #[test]
    fn greek_letter() {
        assert_eq!(
            parse_one("\\alpha"),
            MathNode::Identifier("\u{03B1}".into())
        );
    }

    #[test]
    fn binary_op() {
        let nodes = parse("x + y");
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[1], MathNode::Symbol(SymbolKind::BinaryOp, "+".into()));
    }

    #[test]
    fn unicode_symbols_match_commands() {
        for (literal, command) in [("±", "\\pm"), ("×", "\\times"), ("≤", "\\leq")] {
            assert_eq!(parse(literal), parse(command), "symbol: {literal}");
        }
    }

    #[test]
    fn fraction() {
        let node = parse_one("\\frac{a}{b}");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        ));
    }

    #[test]
    fn sqrt_simple() {
        let node = parse_one("\\sqrt{x}");
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn sqrt_with_index() {
        let node = parse_one("\\sqrt[3]{x}");
        assert!(matches!(node, MathNode::Root { .. }));
    }

    #[test]
    fn superscript() {
        let node = parse_one("x^2");
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn subscript() {
        let node = parse_one("x_i");
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn subsup() {
        let node = parse_one("x_i^2");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn subsup_reversed() {
        let node = parse_one("x^2_i");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn named_operator() {
        let node = parse_one("\\sin");
        assert!(matches!(node, MathNode::Operator(_)));
    }

    #[test]
    fn nary_sum() {
        let nodes = parse("\\sum_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], MathNode::Nary { .. }));
    }

    #[test]
    fn left_right_parens() {
        let node = parse_one("\\left( x \\right)");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn text_command() {
        let node = parse_one("\\text{hello world}");
        assert_eq!(
            node,
            MathNode::Text(TextStyle::Normal, "hello world".into())
        );
    }

    #[test]
    fn char_primitive_preserves_literal_math_symbols() {
        for input in [r"\char`\~", r#"\char"7e"#, r"\char126"] {
            assert_eq!(
                parse_one(input),
                MathNode::Symbol(SymbolKind::Ordinary, "~".into()),
                "{input}"
            );
        }
        assert_eq!(
            parse_one(r"\char`\^"),
            MathNode::Symbol(SymbolKind::Ordinary, "^".into())
        );
        assert_eq!(
            parse_one(r"\char`'"),
            MathNode::Symbol(SymbolKind::Ordinary, "'".into())
        );
        assert_eq!(
            parse_one(r"\char`\\"),
            MathNode::Symbol(SymbolKind::Ordinary, "\\".into())
        );
    }

    #[test]
    fn text_takes_one_unexpanded_token() {
        for (input, braced) in [
            (r"\sqrt \text x", r"\sqrt{\text{x}}"),
            (r"x_\text x", r"x_{\text{x}}"),
            (r"\textbf 𝔸b", r"\textbf{𝔸}b"),
            ("\\text e\u{0301}x", "\\text{e\u{0301}}x"),
            (r"\text\%x", r"\text{\%}x"),
            (r"\text\{x", r"\text{\{}x"),
            (r"\textit\alpha x", r"\textit{α}x"),
            (
                r"\newcommand{\word}{alpha}\textbf\word y",
                r"\textbf{alpha}y",
            ),
        ] {
            assert_eq!(parse(input), parse(braced), "{input}");
        }
        for invalid in [r"\text", r"{\text}", r"\text&", r"\text{unclosed"] {
            assert!(
                LatexReader::parse(invalid, &Config::default()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn emphasis_toggles_slant_and_preserves_the_surrounding_weight() {
        for (input, equivalent) in [
            (
                r"\emph{one \emph{two} three}",
                r"\textit{one \textup{two} three}",
            ),
            (
                r"\textit{\emph{a\emph{b}}}",
                r"\textit{\textup{a\textit{b}}}",
            ),
            (
                r"\textsf{\textbf{\emph{x}\textmd{y}}}",
                r"\textsf{\textbf{\textit{x}}y}",
            ),
        ] {
            assert_eq!(parse(input), parse(equivalent), "{input}");
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            assert_eq!(parse(&first), parse(input), "{first}");
        }
    }

    #[test]
    fn mathbb_command() {
        let node = parse_one("\\mathbb{R}");
        assert!(matches!(node, MathNode::Styled(TextStyle::DoubleStruck, _)));
    }

    #[test]
    fn hat_accent() {
        let node = parse_one("\\hat{x}");
        assert!(matches!(
            node,
            MathNode::Accent {
                position: AccentPosition::Above,
                ..
            }
        ));
    }

    #[test]
    fn braced_group() {
        let node = parse_one("{a + b}");
        assert!(matches!(node, MathNode::Row(_)));
    }

    #[test]
    fn phantom() {
        let node = parse_one("\\phantom{x}");
        assert!(matches!(node, MathNode::Phantom(_, _)));
    }

    #[test]
    fn boxed() {
        let node = parse_one("\\boxed{x}");
        assert!(matches!(node, MathNode::Boxed { .. }));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = LatexReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn spacing_commands() {
        let node = parse_one("\\quad");
        assert!(matches!(node, MathNode::Space(_)));
    }

    #[test]
    fn nested_fraction() {
        let node = parse_one("\\frac{\\frac{a}{b}}{c}");
        if let MathNode::Fraction { num, .. } = &node {
            assert!(matches!(**num, MathNode::Fraction { .. }));
        } else {
            panic!("expected fraction");
        }
    }

    #[test]
    fn matrix_pmatrix() {
        let node = parse_one("\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn heap_matrix_retains_environment_and_compact_geometry() {
        for (source, kind, arraystretch) in [
            (
                r"\begin{matrix}a&b\\c&d\end{matrix}",
                MatrixKind::Matrix,
                None,
            ),
            (
                r"\begin{smallmatrix}a&b\\c&d\end{smallmatrix}",
                MatrixKind::SmallMatrix,
                Some(0.5),
            ),
        ] {
            let config = Config::default();
            let heap = LatexReader::parse(source, &config).unwrap().expr;
            assert!(matches!(
                heap.nodes.as_slice(),
                [MathNode::Matrix {
                    kind: actual,
                    arraystretch: actual_stretch,
                    ..
                }] if *actual == kind && *actual_stretch == arraystretch
            ));
            let compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            assert_eq!(heap, compact.to_math_expr());
            assert_eq!(
                LatexWriter::write(&heap, &config).unwrap(),
                LatexWriter::write_compact(&compact, &config).unwrap()
            );
        }
    }

    #[test]
    fn heap_starred_matrix_retains_alignment_on_write() {
        for source in [
            r"\begin{matrix*}[l]a&b\\c&d\end{matrix*}",
            r"\begin{matrix*}[r]a&b\\c&d\end{matrix*}",
        ] {
            let config = Config::default();
            let heap = LatexReader::parse(source, &config).unwrap().expr;
            let written = LatexWriter::write(&heap, &config).unwrap();
            assert!(written.contains(r"\begin{matrix*}"), "{written}");
            assert!(written.contains(if source.contains("[l]") { "[l]" } else { "[r]" }));
            let compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            assert_eq!(
                LatexWriter::write_compact(&compact, &config).unwrap(),
                written
            );
        }
    }

    #[test]
    fn array_row_separator_after_nary_operator_is_preserved() {
        let node = parse_one(r"\begin{array}{l}\int\\x\end{array}");
        match node {
            MathNode::Array { rows, .. } => {
                assert_eq!(
                    rows.len(),
                    2,
                    "row separator after n-ary operator should create a second row"
                );
            },
            other => panic!("expected Array node, got {other:?}"),
        }
    }

    #[test]
    fn quadratic_formula() {
        let input = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
        let config = Config::default();
        let result = LatexReader::parse(input, &config);
        assert!(result.is_ok());
        let nodes = result.unwrap().expr.nodes;
        assert!(nodes.len() >= 3);
    }

    #[test]
    fn operatorname() {
        let node = parse_one("\\operatorname{Tr}");
        if let MathNode::Operator(attr) = &node {
            assert_eq!(attr.name, "Tr");
        } else {
            panic!("expected Operator, got {node:?}");
        }
    }

    #[test]
    fn overset() {
        let node = parse_one("\\overset{\\sim}{=}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn underset() {
        let node = parse_one("\\underset{n}{\\max}");
        assert!(matches!(node, MathNode::Underscript { .. }));
    }

    #[test]
    fn stackrel() {
        let node = parse_one("\\stackrel{\\text{def}}{=}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn not_equals() {
        let nodes = parse("\\not=");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MathNode::Symbol(SymbolKind::Relation, s) if s == "\u{2260}"));
    }

    #[test]
    fn not_in() {
        let nodes = parse("\\not\\in");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MathNode::Symbol(SymbolKind::Relation, s) if s == "\u{2209}"));
    }

    #[test]
    fn sum_limits() {
        let nodes = parse("\\sum\\limits_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        if let MathNode::Nary { limits, .. } = &nodes[0] {
            assert_eq!(*limits, LimitStyle::AboveBelow);
        } else {
            panic!("expected Nary");
        }
    }

    #[test]
    fn sum_nolimits() {
        let nodes = parse("\\sum\\nolimits_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        if let MathNode::Nary { limits, .. } = &nodes[0] {
            assert_eq!(*limits, LimitStyle::SubSup);
        } else {
            panic!("expected Nary");
        }
    }

    #[test]
    fn lim_limits() {
        let nodes = parse("\\lim\\limits_{x \\to 0}");
        assert!(!nodes.is_empty());
    }

    #[test]
    fn big_paren() {
        let node = parse_one("\\big(");
        assert!(matches!(node, MathNode::Scaled { .. }));
    }

    #[test]
    fn bigg_bracket() {
        let node = parse_one("\\Bigg[");
        if let MathNode::Scaled { factor, .. } = &node {
            assert_eq!(*factor, Rational::new(3, 1));
        } else {
            panic!("expected Scaled");
        }
    }

    #[test]
    fn pmod() {
        let node = parse_one("\\pmod{n}");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn bmod() {
        let node = parse_one("\\bmod");
        assert!(matches!(node, MathNode::Operator(_)));
    }

    #[test]
    fn dbinom() {
        let node = parse_one(r"\dbinom{n}{k}");
        let MathNode::Row(nodes) = node else {
            panic!("missing display-style scope")
        };
        assert!(
            matches!(&nodes[..], [MathNode::StyleSwitch(0), MathNode::Fenced { content, .. }]
            if matches!(&content[..], [crate::ast::FencedItem::Element(MathNode::Fraction {
                kind: crate::ast::FractionKind::NoBar, ..
            })]))
        );
    }

    #[test]
    fn xrightarrow() {
        let node = parse_one("\\xrightarrow{f}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn xrightarrow_with_below() {
        let node = parse_one("\\xrightarrow[g]{f}");
        assert!(matches!(node, MathNode::UnderOver { .. }));
    }

    #[test]
    fn substack() {
        let node = parse_one("\\substack{i < n \\\\ j > 0}");
        assert!(matches!(node, MathNode::Array { .. }));
    }

    #[test]
    fn color_ignored() {
        let nodes = parse("\\color{red} x");
        assert!(!nodes.is_empty());
    }

    #[test]
    fn textcolor() {
        let node = parse_one("\\textcolor{blue}{x}");
        assert_eq!(
            node,
            MathNode::Color {
                color: "blue".into(),
                content: Box::new(MathNode::Identifier("x".into())),
            }
        );
    }

    #[test]
    fn displaystyle_ignored() {
        let nodes = parse("\\displaystyle x");
        assert!(!nodes.is_empty());
    }

    #[test]
    fn gather_env() {
        let node = parse_one("\\begin{gather} a \\\\ b \\end{gather}");
        assert!(matches!(node, MathNode::Array { .. }));
    }

    #[test]
    fn equation_env() {
        let node = parse_one("\\begin{equation} x = 1 \\end{equation}");
        assert!(!matches!(node, MathNode::Array { .. }));
    }

    // =======================================================================
    // Tests for newly implemented LaTeX commands
    // =======================================================================

    #[test]
    fn cfrac() {
        let node = parse_one("\\cfrac{1}{2}");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Display,
                ..
            }
        ));
    }

    #[test]
    fn cfrac_nested() {
        let node = parse_one("\\cfrac{1}{1+\\cfrac{1}{2}}");
        if let MathNode::Fraction { kind, den, .. } = &node {
            assert_eq!(*kind, FractionKind::Display);
            // den should be a Row containing "1 + cfrac{1}{2}"
            if let MathNode::Row(children) = den.as_ref() {
                assert!(children.iter().any(|n| matches!(
                    n,
                    MathNode::Fraction {
                        kind: FractionKind::Display,
                        ..
                    }
                )));
            }
        } else {
            panic!("expected Display fraction, got {node:?}");
        }
    }

    #[test]
    fn ensuremath() {
        let nodes = parse("\\ensuremath{\\pi^2}");
        assert_eq!(nodes.len(), 1);
        // Should parse as superscript of pi^2, not literal "ensuremath"
        assert!(matches!(nodes[0], MathNode::Superscript { .. }));
    }

    #[test]
    fn over_infix() {
        let node = parse_one("{a \\over b}");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        ));
    }

    #[test]
    fn atop_infix() {
        let node = parse_one("{a \\atop b}");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::NoBar,
                ..
            }
        ));
    }

    #[test]
    fn choose_infix() {
        let node = parse_one("{n \\choose k}");
        // Should produce a Fenced (parenthesized) node containing a NoBar fraction
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn brace_infix() {
        let node = parse_one("{a \\brace b}");
        if let MathNode::Fenced { open, close, .. } = &node {
            assert_eq!(open, "{");
            assert_eq!(close, "}");
        } else {
            panic!("expected Fenced, got {node:?}");
        }
    }

    #[test]
    fn brack_infix() {
        let node = parse_one("{a \\brack b}");
        if let MathNode::Fenced { open, close, .. } = &node {
            assert_eq!(open, "[");
            assert_eq!(close, "]");
        } else {
            panic!("expected Fenced, got {node:?}");
        }
    }

    // Regression: unbraced infix operators at the TOP level must build a
    // fraction too (previously leaked a `__infix_*__` sentinel into output).
    #[test]
    fn over_infix_top_level() {
        let node = parse_one("a \\over b");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        ));
    }

    #[test]
    fn atop_infix_top_level() {
        let node = parse_one("a \\atop b");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::NoBar,
                ..
            }
        ));
    }

    #[test]
    fn above_infix_top_level() {
        // `\above <dim>` → fraction at top level (dimension currently dropped).
        let node = parse_one("a \\above 1pt b");
        assert!(matches!(node, MathNode::Fraction { .. }));
    }

    #[test]
    fn choose_infix_top_level() {
        let node = parse_one("n \\choose k");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn no_infix_sentinel_leaks_to_output() {
        // The `__infix_*__` sentinel must never survive into rendered output.
        for tex in [
            "a \\over b",
            "a \\atop b",
            "a \\above 1pt b",
            "n \\choose k",
        ] {
            let mathml = crate::convert::convert(
                tex,
                crate::error::Format::LaTeX,
                crate::error::Format::MathML,
            )
            .unwrap();
            assert!(
                !mathml.contains("__infix"),
                "sentinel leaked for {tex:?}: {mathml}"
            );
            assert!(
                mathml.contains("<mfrac"),
                "no <mfrac> for {tex:?}: {mathml}"
            );
        }
    }

    #[test]
    fn font_sizing_noop() {
        // Font sizing commands should not appear as literal text
        for cmd in &[
            "\\Huge",
            "\\huge",
            "\\LARGE",
            "\\Large",
            "\\large",
            "\\normalsize",
            "\\small",
            "\\footnotesize",
            "\\scriptsize",
            "\\tiny",
        ] {
            let input = format!("{} x", cmd);
            let nodes = parse(&input);
            // Should have exactly one identifier "x" (sizing is no-op)
            assert!(
                nodes
                    .iter()
                    .any(|n| matches!(n, MathNode::Identifier(s) if s == "x")),
                "font sizing command {cmd} did not parse as no-op: {nodes:?}"
            );
            // Should NOT have an identifier with the command name
            let cmd_name = &cmd[1..]; // strip backslash
            assert!(
                !nodes
                    .iter()
                    .any(|n| matches!(n, MathNode::Identifier(s) if s == cmd_name)),
                "font sizing command {cmd} appeared as literal text"
            );
        }
    }

    #[test]
    fn mathop_wrapper() {
        let nodes = parse("\\mathop{\\text{li}}");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(
            &nodes[0],
            MathNode::MathAtom { atom_type: MathAtomType::Op, content }
            if matches!(content.as_ref(), MathNode::Text(TextStyle::Normal, _))
        ));
    }

    #[test]
    fn mathrel_wrapper() {
        let nodes = parse("\\mathrel{\\#}");
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn mathbin_wrapper() {
        let nodes = parse("\\mathbin{+}");
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn mathord_wrapper() {
        let nodes = parse("\\mathord{x}");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(
            &nodes[0],
            MathNode::MathAtom { atom_type: MathAtomType::Ord, content }
            if matches!(content.as_ref(), MathNode::Identifier(_))
        ));
    }

    #[test]
    fn kern_with_pt() {
        let nodes = parse("a\\kern5pt b");
        assert!(
            nodes
                .iter()
                .any(|n| matches!(n, MathNode::Space(_) | MathNode::ExplicitSpace { .. })),
            "\\kern should produce a Space node: {nodes:?}"
        );
    }

    #[test]
    fn kern_with_em() {
        let nodes = parse("a\\kern1em b");
        assert!(
            nodes.iter().any(|n| {
                matches!(n, MathNode::ExplicitSpace { width, .. } if width.as_f64() > 0.0)
            }),
            "\\kern 1em should produce positive space"
        );
    }

    #[test]
    fn mkern_with_mu() {
        let nodes = parse("a\\mkern18mu b");
        assert!(
            nodes.iter().any(|n| {
                matches!(
                    n,
                    MathNode::ExplicitSpace {
                        basis: SpaceBasis::MathMu,
                        ..
                    }
                )
            }),
            "\\mkern should produce a Space node"
        );
    }

    #[test]
    fn hskip_and_mskip_share_their_kern_spacing_nodes() {
        let hskip = parse("a\\hskip{1em}b");
        let kern = parse("a\\kern1em b");
        assert_eq!(
            hskip, kern,
            "hskip and kern should have the same fixed spacing"
        );

        let mskip = parse("a\\mskip{18mu}b");
        let mkern = parse("a\\mkern18mu b");
        assert_eq!(
            mskip, mkern,
            "mskip and mkern should have the same fixed spacing"
        );
    }

    #[test]
    fn explicit_spacing_preserves_text_em_and_math_mu_bases() {
        let text = LatexReader::parse_compact(r"\hskip1em", &Config::default()).unwrap();
        let math = LatexReader::parse_compact(r"\mskip18mu", &Config::default()).unwrap();
        let text_id = text.expr.arena.children(text.expr.roots)[0];
        let math_id = math.expr.arena.children(math.expr.roots)[0];
        assert_eq!(
            text.expr.arena.space_basis(text_id),
            Some(SpaceBasis::TextEm)
        );
        assert_eq!(
            math.expr.arena.space_basis(math_id),
            Some(SpaceBasis::MathMu)
        );
        assert!(matches!(
            text.expr.to_math_expr().nodes.as_slice(),
            [MathNode::ExplicitSpace { width, basis: SpaceBasis::TextEm }]
                if *width == Rational::one()
        ));
        assert!(matches!(
            math.expr.to_math_expr().nodes.as_slice(),
            [MathNode::ExplicitSpace { width, basis: SpaceBasis::MathMu }]
                if *width == Rational::one()
        ));

        let text_mathml = crate::from_latex(r"x^{a\hskip1em b}")
            .unwrap()
            .to_mathml()
            .unwrap();
        let mu_mathml = crate::from_latex(r"x^{a\mskip18mu b}")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(text_mathml.contains(r#"width="1.428"#), "{text_mathml}");
        assert!(mu_mathml.contains(r#"width="1.171"#), "{mu_mathml}");
    }

    #[test]
    fn kern_glue_reports_the_unrepresentable_component() {
        let parsed =
            LatexReader::parse_compact(r"a\hskip{1em plus .5em}b", &Config::default()).unwrap();
        assert!(parsed.has_warnings());
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("stretch and shrink") })
        );

        let parsed = LatexReader::parse_compact(r"a\hskip1em + b", &Config::default()).unwrap();
        assert!(!parsed.has_warnings());
        let expr = parsed.to_parse_result().expr;
        assert!(
            expr.nodes
                .iter()
                .any(|node| { matches!(node, MathNode::Symbol(_, text) if text == "+") })
        );
        assert!(
            expr.nodes
                .iter()
                .any(|node| { matches!(node, MathNode::Identifier(text) if text == "b") })
        );
    }

    #[test]
    fn unbraced_glue_suffixes_are_consumed_without_swallowing_following_math() {
        let parsed =
            LatexReader::parse_compact(r"a\hskip 1em plus .5em minus .2em b", &Config::default())
                .unwrap();
        assert!(parsed.has_warnings());
        let expr = parsed.to_parse_result().expr;
        assert!(
            expr.nodes
                .iter()
                .any(|node| matches!(node, MathNode::Identifier(text) if text == "b"))
        );
        assert!(expr.nodes.iter().any(
            |node| matches!(node, MathNode::Space(width) if width.as_f64() > 0.09)
                || matches!(node, MathNode::ExplicitSpace { width, .. } if width.as_f64() > 0.09)
        ));

        let parsed =
            LatexReader::parse_compact(r"a\hskip 1em plus 1fill minus 2pt b", &Config::default())
                .unwrap();
        assert!(parsed.has_warnings());
        let expr = parsed.to_parse_result().expr;
        assert!(
            expr.nodes
                .iter()
                .any(|node| matches!(node, MathNode::Identifier(text) if text == "b"))
        );
    }

    #[test]
    fn glue_keywords_belong_only_to_skip_primitives() {
        let full = r"a\hskip1em plus 1fil minus 2pt b";
        let fixed = r"a\kern1em b";
        let to_mathml = |x| crate::from_latex(x).unwrap().to_mathml().unwrap();
        assert_eq!(to_mathml(full), to_mathml(fixed));
        assert_eq!(to_mathml(r"a\mskip18mu plus 3fill b"), to_mathml(fixed));
        let kern =
            LatexReader::parse_compact(r"a\kern1em plus 1fil b", &Config::default()).unwrap();
        assert!(!kern.has_warnings());
        assert!(
            crate::from_latex(r"a\kern1em plus 1fil b")
                .unwrap()
                .to_latex()
                .unwrap()
                .contains("plus")
        );
        let incomplete = crate::from_latex(r"a\hskip1em plus 2pt minus b").unwrap();
        let tex = incomplete.to_latex().unwrap();
        assert!(tex.contains("minus") && !tex.contains("plus"), "{tex}");
    }

    #[test]
    fn incomplete_glue_is_left_for_the_outer_parser() {
        let parsed = LatexReader::parse_compact(r"a\hskip 1em plus b", &Config::default()).unwrap();
        assert!(parsed.has_warnings());
        let expr = parsed.to_parse_result().expr;
        assert!(
            expr.nodes
                .iter()
                .any(|node| matches!(node, MathNode::Identifier(text) if text == "b"))
        );
    }

    #[test]
    fn glue_fixed_width_matches_kern_and_consumes_infinite_orders() {
        for (glue, kern) in [
            (r"a\hskip 1em plus .5em minus .2em b", r"a\kern1em b"),
            (r"a\hskip 1em plus 1fill minus 2pt b", r"a\kern1em b"),
            (r"a\mskip 18mu plus 1fil b", r"a\mkern18mu b"),
        ] {
            let glue = parse(glue);
            let kern = parse(kern);
            let glue_space = glue
                .iter()
                .find_map(|node| match node {
                    MathNode::Space(value) | MathNode::ExplicitSpace { width: value, .. } => {
                        Some(value.as_f64())
                    },
                    _ => None,
                })
                .expect("glue should retain a fixed space");
            let kern_space = kern
                .iter()
                .find_map(|node| match node {
                    MathNode::Space(value) | MathNode::ExplicitSpace { width: value, .. } => {
                        Some(value.as_f64())
                    },
                    _ => None,
                })
                .expect("kern should retain a fixed space");
            assert!((glue_space - kern_space).abs() < f64::EPSILON, "{glue:?}");
            assert!(
                glue.iter()
                    .any(|node| matches!(node, MathNode::Identifier(text) if text == "b"))
            );
        }
    }

    #[test]
    fn explicit_spacing_observes_the_tex_dimension_range() {
        for (command, unit) in [(r"\hskip", "pt"), (r"\mskip", "mu")] {
            let normal = format!("a{command}16383{unit} b");
            let large = format!("a{command}16384{unit} b");
            assert!(crate::from_latex(&normal).is_ok(), "{normal}");
            assert!(crate::from_latex(&large).is_err(), "{large}");
            let huge = format!("a{command}{}{unit} b", "9".repeat(350));
            assert!(crate::from_latex(&huge).is_err());
        }
    }

    #[test]
    fn math_and_text_spacing_units_are_reported_without_rejecting_input() {
        let text_unit = LatexReader::parse_compact(r"a\hskip1mu b", &Config::default()).unwrap();
        assert!(text_unit.has_warnings());
        assert!(
            text_unit
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("does not use a mu unit") })
        );

        let math_unit = LatexReader::parse_compact(r"a\mskip1em b", &Config::default()).unwrap();
        assert!(math_unit.has_warnings());
        assert!(
            math_unit
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("expects a mu unit") })
        );
    }

    #[test]
    fn hspace_braced() {
        let nodes = parse("a\\hspace{1em}b");
        assert!(
            nodes.iter().any(|n| matches!(n, MathNode::Space(_))),
            "\\hspace should produce a Space node"
        );
    }

    #[test]
    fn hspace_star() {
        let nodes = parse("a\\hspace*{2pt}b");
        assert!(
            nodes.iter().any(|n| matches!(n, MathNode::Space(_))),
            "\\hspace* should produce a Space node"
        );
    }

    #[test]
    fn rule_consumed() {
        let nodes = parse("a\\rule{1cm}{2cm}b");
        // \rule should be consumed, not produce literal "rule"
        assert!(
            !nodes
                .iter()
                .any(|n| matches!(n, MathNode::Identifier(s) if s == "rule")),
            "\\rule should not appear as literal text"
        );
    }

    #[test]
    fn rule_with_depth() {
        let nodes = parse("a\\rule[1pt]{1cm}{2cm}b");
        assert!(
            !nodes
                .iter()
                .any(|n| matches!(n, MathNode::Identifier(s) if s == "rule")),
            "\\rule with optional depth should not appear as literal text"
        );
    }

    #[test]
    fn rule_retains_dimensions_in_heap_ast() {
        let nodes = parse(r"\rule[1em]{2em}{3em}");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MathNode::Rule {
                width,
                height,
                lift,
            } => {
                assert_eq!(*width, Rational::new(2, 1));
                assert_eq!(*height, Rational::new(3, 1));
                assert_eq!(*lift, Rational::new(1, 1));
            },
            other => panic!("expected Rule node, got {other:?}"),
        }
    }

    #[test]
    fn top_level_newline_parse_compact() {
        let result = LatexReader::parse_compact(r"a\\b", &Config::default()).unwrap();
        let roots = result.expr.arena.children(result.expr.roots);
        assert_eq!(roots.len(), 3, "expected a, newline, b roots");
        assert!(
            roots
                .iter()
                .any(|&id| matches!(result.expr.arena.node(id), CompactNode::Newline)),
            "top-level line break should be retained in compact roots"
        );
    }

    #[test]
    fn top_level_source_newline_parse_compact() {
        let result = LatexReader::parse_compact("a+b=\nc+d", &Config::default()).unwrap();
        let roots = result.expr.arena.children(result.expr.roots);
        assert!(
            roots
                .iter()
                .any(|&id| matches!(result.expr.arena.node(id), CompactNode::Newline)),
            "top-level source newline should be retained in compact roots"
        );
    }

    #[test]
    fn raisebox_retains_shift_in_heap_ast() {
        let nodes = parse(r"a\raisebox{1em}{b}");
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], MathNode::Identifier(ref s) if s == "a"));
        match &nodes[1] {
            MathNode::RaiseBox {
                shift,
                height,
                depth,
                content,
            } => {
                assert_eq!(*shift, Rational::new(1, 1));
                assert_eq!(*height, None);
                assert_eq!(*depth, None);
                assert!(matches!(content.as_ref(), MathNode::Identifier(s) if s == "b"));
            },
            other => panic!("expected RaiseBox node, got {other:?}"),
        }
    }

    #[test]
    fn raisebox_optional_extents_do_not_leak_into_content() {
        let nodes = parse(r"\raisebox{1em}[2em][3em]{b}");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MathNode::RaiseBox {
                shift,
                height,
                depth,
                content,
            } => {
                assert_eq!(*shift, Rational::new(1, 1));
                assert_eq!(*height, Some(Rational::new(2, 1)));
                assert_eq!(*depth, Some(Rational::new(3, 1)));
                assert!(matches!(content.as_ref(), MathNode::Identifier(s) if s == "b"));
            },
            other => panic!("expected RaiseBox node, got {other:?}"),
        }
    }

    #[test]
    fn middle_outside_leftright() {
        // \middle outside \left..\right should still parse as a fence symbol
        let nodes = parse("\\middle|");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], MathNode::Symbol(SymbolKind::Fence, _)));
    }

    #[test]
    fn color_shorthand_blue() {
        let nodes = parse("\\blue{x}");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0],
            MathNode::Color {
                color: "blue".into(),
                content: Box::new(MathNode::Identifier("x".into())),
            }
        );
    }

    #[test]
    fn color_shorthand_red_single_token() {
        let nodes = parse("\\red x");
        // Should wrap x in color
        assert!(!nodes.is_empty());
        assert!(matches!(
            &nodes[..],
            [MathNode::Color { color, content }]
                if color == "red" && matches!(content.as_ref(), MathNode::Identifier(x) if x == "x")
        ));
    }
}
