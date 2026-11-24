//! Typesetting attributes: what a document says about how a node is set that
//! does not change its mathematics.
//!
//! The compact arena keeps them beside its nodes
//! ([`AstArena::attrs`](super::arena::AstArena::attrs)), so a tree with
//! attributes has the same nodes as one without. The heap AST wraps a node in
//! [`MathNode::Attributed`](super::MathNode::Attributed).
//!
//! An attribute applies to a node and everything inside it that has no such
//! attribute of its own.

use super::Rational;

/// The attributes of one node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct NodeAttrs {
    /// Font size.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub size: Option<FontSize>,
}

impl NodeAttrs {
    /// Attributes holding only a font size.
    #[must_use]
    pub fn with_size(size: FontSize) -> Self {
        Self { size: Some(size) }
    }

    /// Whether no attribute is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size.is_none()
    }

    /// Take from `outer` what is not set here: the inner attribute wins.
    #[must_use]
    pub fn or(self, outer: &Self) -> Self {
        Self {
            size: self.size.or(outer.size),
        }
    }
}

/// A font size that is not relative to the size around it, unlike the
/// declarations [`MathNode::SizeSwitch`](super::MathNode::SizeSwitch) stands
/// for.
///
/// It is the size of the node's own text, also inside a script (MathML's rule
/// for `mathsize`); scripts inside the node are smaller in proportion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum FontSize {
    /// A size in points (1/72 inch).
    Points(Rational),
    /// A MathType logical size, plus an amount in points.
    Logical {
        /// The logical size.
        level: TypeSize,
        /// Points added to it, usually zero.
        delta_pt: Rational,
    },
}

/// A MathType logical size ("typesize").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TypeSize {
    /// Full size: the size of ordinary text in the equation.
    Full,
    /// The size of subscripts and superscripts.
    Sub,
    /// The size of scripts on scripts.
    Sub2,
    /// The size of big operators.
    Sym,
    /// The size of big operators in scripts.
    SubSym,
    /// The first user-defined size.
    User1,
    /// The second user-defined size.
    User2,
}

impl TypeSize {
    /// The size in thousandths of full size. Scripts have the sizes of this
    /// crate's layout, so that text set at a script's size matches a script;
    /// the others are MathType's factory settings.
    pub(crate) fn per_mille(self) -> i32 {
        match self {
            Self::Full | Self::SubSym => 1000,
            Self::Sub => 700,
            Self::Sub2 => 500,
            Self::Sym | Self::User2 => 1500,
            Self::User1 => 750,
        }
    }
}

/// The base size of an expression that does not say its own, in points: the
/// factory setting of MathType, and the size of CSS `medium` text.
pub(crate) const DEFAULT_BASE_SIZE_PT: i32 = 12;

/// Facts about an expression as a whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct ExprMeta {
    /// The size of its ordinary text in points, where the source says it.
    /// [`FontSize::Points`] is seen against it; 12 points when unknown.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub base_size_pt: Option<Rational>,
}

impl ExprMeta {
    /// Whether nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base_size_pt.is_none()
    }
}

/// The base size that `meta` gives or implies, in points.
pub(crate) fn base_size_pt(meta: Option<&ExprMeta>) -> f64 {
    meta.and_then(|meta| meta.base_size_pt)
        .map(super::Rational::as_f64)
        .filter(|size| *size > 0.0)
        .unwrap_or_else(|| f64::from(DEFAULT_BASE_SIZE_PT))
}

impl FontSize {
    /// The size as a multiple of a base size in points.
    pub(crate) fn scale(self, base_pt: f64) -> f64 {
        let scale = match self {
            Self::Points(points) => points.as_f64() / base_pt,
            Self::Logical { level, delta_pt } => {
                f64::from(level.per_mille()) / 1000.0 + delta_pt.as_f64() / base_pt
            },
        };
        // A document can name any size; one that draws nothing or overflows
        // is none.
        if scale.is_finite() {
            scale.clamp(0.05, 100.0)
        } else {
            1.0
        }
    }
}
