use crate::error::{CompactParseResult, Config, ParseResult, Result};
use crate::reader::Reader;

pub struct LatexReader;

impl Reader for LatexReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        crate::latex::parse::parse(input, config)
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        crate::latex::parse::parse_compact(input, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::error::Config;

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
        assert!(matches!(node, MathNode::Phantom(_)));
    }

    #[test]
    fn boxed() {
        let node = parse_one("\\boxed{x}");
        assert!(matches!(node, MathNode::Boxed(_)));
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
        let node = parse_one("\\dbinom{n}{k}");
        assert!(matches!(node, MathNode::Fenced { .. }));
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
        assert!(matches!(node, MathNode::Identifier(_)));
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
}
