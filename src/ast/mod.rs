pub mod arena;
pub mod compact;
pub mod normalize;
pub mod visit;

use std::fmt;

/// A complete math expression with display mode.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MathExpr {
    pub nodes: Vec<MathNode>,
    pub display: DisplayStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DisplayStyle {
    Block,
    #[default]
    Inline,
}

/// The core math AST node. 27 variants covering the union of all target formats.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MathNode {
    // === Leaf nodes ===
    /// Numeric literal: "3.14", "42"
    Number(String),
    /// Variable or identifier: "x", "alpha"
    Identifier(String),
    /// Named math operator: "sin", "lim", "det"
    Operator(OperatorAttr),
    /// Classified symbol: +, =, (, ), etc.
    Symbol(SymbolKind, String),
    /// Normal text within math mode
    Text(TextStyle, String),
    /// Explicit space (width in em as rational number)
    Space(Rational),

    // === Grouping / delimiters ===
    /// Group acting as a unit ({...} in TeX, <mrow> in MathML)
    Row(Vec<MathNode>),
    /// Delimited group with open/close and optional mid-separators
    Fenced {
        open: String,
        close: String,
        content: Vec<FencedItem>,
    },

    // === Scripts ===
    /// Subscript: base_sub
    Subscript {
        base: Box<MathNode>,
        sub: Box<MathNode>,
    },
    /// Superscript: base^sup
    Superscript {
        base: Box<MathNode>,
        sup: Box<MathNode>,
    },
    /// Combined sub+superscript: base_sub^sup
    SubSup {
        base: Box<MathNode>,
        sub: Box<MathNode>,
        sup: Box<MathNode>,
    },
    /// Pre-scripts (isotope notation): _{pre_sub}^{pre_sup}base
    PreScripts {
        base: Box<MathNode>,
        pre_sub: Option<Box<MathNode>>,
        pre_sup: Option<Box<MathNode>>,
    },

    // === Over/Under ===
    /// Content placed over base
    Overscript {
        base: Box<MathNode>,
        over: Box<MathNode>,
        movable_limits: bool,
    },
    /// Content placed under base
    Underscript {
        base: Box<MathNode>,
        under: Box<MathNode>,
        movable_limits: bool,
    },
    /// Content placed both over and under base
    UnderOver {
        base: Box<MathNode>,
        under: Box<MathNode>,
        over: Box<MathNode>,
        movable_limits: bool,
    },

    // === Fractions and roots ===
    /// Fraction: num/den with configurable style
    Fraction {
        kind: FractionKind,
        num: Box<MathNode>,
        den: Box<MathNode>,
    },
    /// Square root
    Sqrt(Box<MathNode>),
    /// Nth root with explicit index
    Root {
        index: Box<MathNode>,
        radicand: Box<MathNode>,
    },

    // === N-ary operations ===
    /// N-ary operator: summation, integral, product, etc.
    Nary {
        op: char,
        sub: Option<Box<MathNode>>,
        sup: Option<Box<MathNode>>,
        body: Box<MathNode>,
        limits: LimitStyle,
    },

    // === Accent ===
    /// Accent mark above or below base
    Accent {
        base: Box<MathNode>,
        accent: char,
        position: AccentPosition,
    },

    // === Tables/Arrays ===
    /// Matrix, equation array, cases, etc.
    Array {
        columns: Vec<ColumnAlign>,
        rows: Vec<Vec<Vec<MathNode>>>,
        arraystretch: Option<f64>,
        add_jot: bool,
    },

    // === Decorators ===
    /// Invisible but occupies space
    Phantom(Box<MathNode>),
    /// Bordered box around content (with optional background/border colors for \colorbox/\fcolorbox)
    Boxed {
        content: Box<MathNode>,
        bg_color: Option<String>,
        border_color: Option<String>,
    },
    /// Strikethrough/cancellation
    Cancel {
        stroke: StrokeKind,
        content: Box<MathNode>,
    },
    /// Scaled to a factor of normal size
    Scaled {
        factor: Rational,
        content: Box<MathNode>,
    },
    /// Applies a text/font style to children
    Styled(TextStyle, Vec<MathNode>),
    /// Style-dependent content: \mathchoice{D}{T}{S}{SS}
    MathChoice {
        display: Box<MathNode>,
        text: Box<MathNode>,
        script: Box<MathNode>,
        script_script: Box<MathNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SymbolKind {
    Ordinary,
    LargeOp,
    BinaryOp,
    Relation,
    Open,
    Close,
    Punctuation,
    Fence,
    Alphabetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextStyle {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Monospace,
    SansSerif,
    SansSerifBold,
    SansSerifItalic,
    SansSerifBoldItalic,
    DoubleStruck,
    Script,
    BoldScript,
    Fraktur,
    BoldFraktur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FractionKind {
    Normal,
    Display,
    Inline,
    NoBar,
    Skewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AccentPosition {
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeKind {
    Forward,
    Backward,
    Cross,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LimitStyle {
    /// Context-dependent (display vs inline)
    Default,
    /// Always above/below the operator
    AboveBelow,
    /// Always as sub/superscripts
    SubSup,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FencedItem {
    Element(MathNode),
    Middle(String),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OperatorAttr {
    pub name: String,
    pub limit_style: LimitStyle,
}

/// Exact rational number for spacing (avoids floating-point imprecision).
/// Stored in lowest terms: `gcd(|num|, den) == 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rational {
    pub num: i32,
    pub den: u32,
}

/// Greatest common divisor (Euclidean algorithm).
const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl Rational {
    #[must_use]
    pub fn new(num: i32, den: u32) -> Self {
        assert!(den > 0, "Rational denominator must be non-zero");
        if num == 0 {
            return Self { num: 0, den: 1 };
        }
        let g = gcd(num.unsigned_abs(), den);
        let reduced_abs = num.unsigned_abs() / g;
        // Negate via i64 to handle the i32::MIN edge case safely.
        let reduced_num = if num >= 0 {
            reduced_abs as i32
        } else {
            (-(reduced_abs as i64)) as i32
        };
        Self {
            num: reduced_num,
            den: den / g,
        }
    }

    #[must_use]
    pub fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    #[must_use]
    pub fn one() -> Self {
        Self { num: 1, den: 1 }
    }

    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl MathExpr {
    #[must_use]
    pub fn new(nodes: Vec<MathNode>, display: DisplayStyle) -> Self {
        Self { nodes, display }
    }

    #[must_use]
    pub fn inline(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Inline,
        }
    }

    #[must_use]
    pub fn block(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Block,
        }
    }
}

/// Maximum allowed AST nesting depth for safe recursive traversal.
pub const MAX_AST_DEPTH: usize = 128;

/// Validate that AST nesting does not exceed the maximum depth.
/// Call before any recursive AST traversal (writing, rendering, normalization)
/// to prevent stack overflow on maliciously deep or programmatically-constructed trees.
pub fn validate_ast_depth(expr: &MathExpr) -> crate::error::Result<()> {
    for node in &expr.nodes {
        check_node_depth(node, 0)?;
    }
    Ok(())
}

/// Validate compact AST nesting depth.
pub fn validate_compact_depth(
    arena: &arena::AstArena,
    roots: compact::NodeSpan,
) -> crate::error::Result<()> {
    for &id in arena.children(roots) {
        check_compact_depth(arena, id, 0)?;
    }
    Ok(())
}

fn check_node_depth(node: &MathNode, depth: usize) -> crate::error::Result<()> {
    if depth > MAX_AST_DEPTH {
        return Err(crate::error::Error::Unsupported(format!(
            "AST nesting exceeds maximum depth of {MAX_AST_DEPTH}"
        )));
    }
    let d = depth + 1;
    match node {
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_) => Ok(()),

        MathNode::Row(children) | MathNode::Styled(_, children) => {
            for child in children {
                check_node_depth(child, d)?;
            }
            Ok(())
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let FencedItem::Element(n) = item {
                    check_node_depth(n, d)?;
                }
            }
            Ok(())
        },
        MathNode::Subscript { base, sub } => {
            check_node_depth(base, d)?;
            check_node_depth(sub, d)
        },
        MathNode::Superscript { base, sup } => {
            check_node_depth(base, d)?;
            check_node_depth(sup, d)
        },
        MathNode::SubSup { base, sub, sup } => {
            check_node_depth(base, d)?;
            check_node_depth(sub, d)?;
            check_node_depth(sup, d)
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            check_node_depth(base, d)?;
            if let Some(s) = pre_sub {
                check_node_depth(s, d)?;
            }
            if let Some(s) = pre_sup {
                check_node_depth(s, d)?;
            }
            Ok(())
        },
        MathNode::Overscript { base, over, .. } => {
            check_node_depth(base, d)?;
            check_node_depth(over, d)
        },
        MathNode::Underscript { base, under, .. } => {
            check_node_depth(base, d)?;
            check_node_depth(under, d)
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            check_node_depth(base, d)?;
            check_node_depth(under, d)?;
            check_node_depth(over, d)
        },
        MathNode::Fraction { num, den, .. } => {
            check_node_depth(num, d)?;
            check_node_depth(den, d)
        },
        MathNode::Sqrt(inner) => check_node_depth(inner, d),
        MathNode::Root { index, radicand } => {
            check_node_depth(index, d)?;
            check_node_depth(radicand, d)
        },
        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                check_node_depth(s, d)?;
            }
            if let Some(s) = sup {
                check_node_depth(s, d)?;
            }
            check_node_depth(body, d)
        },
        MathNode::Accent { base, .. } => check_node_depth(base, d),
        MathNode::Array { rows, .. } => {
            for row in rows {
                for cell in row {
                    for n in cell {
                        check_node_depth(n, d)?;
                    }
                }
            }
            Ok(())
        },
        MathNode::Phantom(inner) | MathNode::Boxed { content: inner, .. } => {
            check_node_depth(inner, d)
        },
        MathNode::Cancel { content, .. } | MathNode::Scaled { content, .. } => {
            check_node_depth(content, d)
        },
        MathNode::MathChoice { display, text, script, script_script } => {
            check_node_depth(display, d)?;
            check_node_depth(text, d)?;
            check_node_depth(script, d)?;
            check_node_depth(script_script, d)
        },
    }
}

fn check_compact_depth(
    arena: &arena::AstArena,
    id: compact::NodeId,
    depth: usize,
) -> crate::error::Result<()> {
    use compact::CompactNode;

    if depth > MAX_AST_DEPTH {
        return Err(crate::error::Error::Unsupported(format!(
            "AST nesting exceeds maximum depth of {MAX_AST_DEPTH}"
        )));
    }
    let d = depth + 1;
    match *arena.node(id) {
        CompactNode::Number(_)
        | CompactNode::Identifier(_)
        | CompactNode::Operator { .. }
        | CompactNode::Symbol(_, _)
        | CompactNode::Text(_, _)
        | CompactNode::Space(_)
        | CompactNode::Newline
        | CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_) => Ok(()),

        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            for &child in arena.children(span) {
                check_compact_depth(arena, child, d)?;
            }
            Ok(())
        },
        CompactNode::Fenced { content, .. } => {
            for &child in arena.children(content) {
                if !matches!(*arena.node(child), CompactNode::Middle(_)) {
                    check_compact_depth(arena, child, d)?;
                }
            }
            Ok(())
        },

        CompactNode::Subscript { base, sub } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, sub, d)
        },
        CompactNode::Superscript { base, sup } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, sup, d)
        },
        CompactNode::SubSup { base, sub, sup } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, sub, d)?;
            check_compact_depth(arena, sup, d)
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            check_compact_depth(arena, base, d)?;
            if pre_sub.is_some() {
                check_compact_depth(arena, pre_sub, d)?;
            }
            if pre_sup.is_some() {
                check_compact_depth(arena, pre_sup, d)?;
            }
            Ok(())
        },

        CompactNode::Overscript { base, over, .. } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, over, d)
        },
        CompactNode::Underscript { base, under, .. } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, under, d)
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            check_compact_depth(arena, base, d)?;
            check_compact_depth(arena, under, d)?;
            check_compact_depth(arena, over, d)
        },

        CompactNode::Fraction { num, den, .. } => {
            check_compact_depth(arena, num, d)?;
            check_compact_depth(arena, den, d)
        },
        CompactNode::Sqrt(inner) => check_compact_depth(arena, inner, d),
        CompactNode::Root { index, radicand } => {
            check_compact_depth(arena, index, d)?;
            check_compact_depth(arena, radicand, d)
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                check_compact_depth(arena, sub, d)?;
            }
            if sup.is_some() {
                check_compact_depth(arena, sup, d)?;
            }
            check_compact_depth(arena, body, d)
        },

        CompactNode::Accent { base, .. } => check_compact_depth(arena, base, d),

        CompactNode::Array { cells, .. } => {
            for &cell_id in arena.children(cells) {
                check_compact_depth(arena, cell_id, d)?;
            }
            Ok(())
        },

        CompactNode::Phantom(inner) | CompactNode::Boxed { content: inner, .. } => {
            check_compact_depth(arena, inner, d)
        },
        CompactNode::Cancel { content, .. } | CompactNode::Scaled { content, .. } => {
            check_compact_depth(arena, content, d)
        },
        CompactNode::Color { content, .. } => check_compact_depth(arena, content, d),
        CompactNode::MathChoice { display, text, script, script_script } => {
            check_compact_depth(arena, display, d)?;
            check_compact_depth(arena, text, d)?;
            check_compact_depth(arena, script, d)?;
            check_compact_depth(arena, script_script, d)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_basics() {
        let r = Rational::new(3, 4);
        assert_eq!(r.num, 3);
        assert_eq!(r.den, 4);
        assert!((r.as_f64() - 0.75).abs() < f64::EPSILON);
        assert_eq!(format!("{r}"), "3/4");
    }

    #[test]
    fn rational_whole() {
        let r = Rational::new(5, 1);
        assert_eq!(format!("{r}"), "5");
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn rational_zero_den() {
        let _ = Rational::new(1, 0);
    }

    #[test]
    fn math_expr_constructors() {
        let expr = MathExpr::inline(vec![MathNode::Number("42".into())]);
        assert_eq!(expr.display, DisplayStyle::Inline);
        assert_eq!(expr.nodes.len(), 1);

        let expr = MathExpr::block(vec![]);
        assert_eq!(expr.display, DisplayStyle::Block);
    }

    #[test]
    fn all_27_variants_constructible() {
        // Verify every variant can be constructed without panic
        let nodes: Vec<MathNode> = vec![
            MathNode::Number("1".into()),
            MathNode::Identifier("x".into()),
            MathNode::Operator(OperatorAttr {
                name: "sin".into(),
                limit_style: LimitStyle::Default,
            }),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Text(TextStyle::Normal, "hello".into()),
            MathNode::Space(Rational::new(1, 6)),
            MathNode::Row(vec![]),
            MathNode::Fenced {
                open: "(".into(),
                close: ")".into(),
                content: vec![],
            },
            MathNode::Subscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sub: Box::new(MathNode::Number("2".into())),
            },
            MathNode::Superscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sup: Box::new(MathNode::Number("2".into())),
            },
            MathNode::SubSup {
                base: Box::new(MathNode::Identifier("x".into())),
                sub: Box::new(MathNode::Number("1".into())),
                sup: Box::new(MathNode::Number("2".into())),
            },
            MathNode::PreScripts {
                base: Box::new(MathNode::Identifier("C".into())),
                pre_sub: Some(Box::new(MathNode::Number("6".into()))),
                pre_sup: Some(Box::new(MathNode::Number("14".into()))),
            },
            MathNode::Overscript {
                base: Box::new(MathNode::Identifier("x".into())),
                over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{0302}".into())),
                movable_limits: false,
            },
            MathNode::Underscript {
                base: Box::new(MathNode::Identifier("x".into())),
                under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "_".into())),
                movable_limits: false,
            },
            MathNode::UnderOver {
                base: Box::new(MathNode::Identifier("x".into())),
                under: Box::new(MathNode::Number("0".into())),
                over: Box::new(MathNode::Number("1".into())),
                movable_limits: true,
            },
            MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(MathNode::Number("1".into())),
                den: Box::new(MathNode::Number("2".into())),
            },
            MathNode::Sqrt(Box::new(MathNode::Number("2".into()))),
            MathNode::Root {
                index: Box::new(MathNode::Number("3".into())),
                radicand: Box::new(MathNode::Number("8".into())),
            },
            MathNode::Nary {
                op: '\u{2211}',
                sub: Some(Box::new(MathNode::Number("0".into()))),
                sup: Some(Box::new(MathNode::Identifier("n".into()))),
                body: Box::new(MathNode::Identifier("i".into())),
                limits: LimitStyle::AboveBelow,
            },
            MathNode::Accent {
                base: Box::new(MathNode::Identifier("x".into())),
                accent: '\u{0302}',
                position: AccentPosition::Above,
            },
            MathNode::Array {
                columns: vec![ColumnAlign::Center],
                rows: vec![vec![vec![MathNode::Number("1".into())]]],
                arraystretch: None,
                add_jot: false,
            },
            MathNode::Phantom(Box::new(MathNode::Number("0".into()))),
            MathNode::Boxed {
                content: Box::new(MathNode::Number("1".into())),
                bg_color: None,
                border_color: None,
            },
            MathNode::Cancel {
                stroke: StrokeKind::Forward,
                content: Box::new(MathNode::Number("0".into())),
            },
            MathNode::Scaled {
                factor: Rational::new(2, 1),
                content: Box::new(MathNode::Number("1".into())),
            },
            MathNode::Styled(TextStyle::Bold, vec![MathNode::Identifier("x".into())]),
            MathNode::MathChoice {
                display: Box::new(MathNode::Identifier("D".into())),
                text: Box::new(MathNode::Identifier("T".into())),
                script: Box::new(MathNode::Identifier("S".into())),
                script_script: Box::new(MathNode::Identifier("SS".into())),
            },
        ];
        assert_eq!(nodes.len(), 27);
    }
}
