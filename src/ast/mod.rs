pub mod visit;

use std::fmt;

/// A complete math expression with display mode.
#[derive(Debug, Clone, PartialEq)]
pub struct MathExpr {
    pub nodes: Vec<MathNode>,
    pub display: DisplayStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayStyle {
    Block,
    Inline,
}

impl Default for DisplayStyle {
    fn default() -> Self {
        DisplayStyle::Inline
    }
}

/// The core math AST node. 26 variants covering the union of all target formats.
#[derive(Debug, Clone, PartialEq)]
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
    },

    // === Decorators ===
    /// Invisible but occupies space
    Phantom(Box<MathNode>),
    /// Bordered box around content
    Boxed(Box<MathNode>),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractionKind {
    Normal,
    Display,
    Inline,
    NoBar,
    Skewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentPosition {
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeKind {
    Forward,
    Backward,
    Cross,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitStyle {
    /// Context-dependent (display vs inline)
    Default,
    /// Always above/below the operator
    AboveBelow,
    /// Always as sub/superscripts
    SubSup,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FencedItem {
    Element(MathNode),
    Middle(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperatorAttr {
    pub name: String,
    pub limit_style: LimitStyle,
}

/// Exact rational number for spacing (avoids floating-point imprecision).
/// Invariant: `den` must be non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    pub num: i32,
    pub den: u32,
}

impl Rational {
    pub fn new(num: i32, den: u32) -> Self {
        assert!(den > 0, "Rational denominator must be non-zero");
        Self { num, den }
    }

    pub fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    pub fn one() -> Self {
        Self { num: 1, den: 1 }
    }

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
    pub fn new(nodes: Vec<MathNode>, display: DisplayStyle) -> Self {
        Self { nodes, display }
    }

    pub fn inline(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Inline,
        }
    }

    pub fn block(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Block,
        }
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
        Rational::new(1, 0);
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
    fn all_26_variants_constructible() {
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
            },
            MathNode::Phantom(Box::new(MathNode::Number("0".into()))),
            MathNode::Boxed(Box::new(MathNode::Number("1".into()))),
            MathNode::Cancel {
                stroke: StrokeKind::Forward,
                content: Box::new(MathNode::Number("0".into())),
            },
            MathNode::Scaled {
                factor: Rational::new(2, 1),
                content: Box::new(MathNode::Number("1".into())),
            },
            MathNode::Styled(TextStyle::Bold, vec![MathNode::Identifier("x".into())]),
        ];
        assert_eq!(nodes.len(), 26);
    }
}
