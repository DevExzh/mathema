//! Core math AST: the [`MathNode`] tree and its supporting types.
//!
//! This module defines the format-independent abstract syntax tree that all
//! readers parse into and all writers render from, plus the [`compact`] arena
//! representation, normalization passes, and traversal helpers.
#![deny(missing_docs)]

/// Arena-backed storage for the compact AST representation.
pub mod arena;
/// Typesetting attributes of nodes and expressions.
pub mod attrs;
/// Compact, `Copy`-able arena-indexed AST node representation.
pub mod compact;
mod dangerous;
mod decimal;
/// Structural normalization passes over the AST.
pub mod normalize;
/// TeX math styles and their transitions inside expressions.
pub mod style;
/// Undefined LaTeX control sequences kept in the AST.
pub mod undefined;
/// Traversal helpers for walking the AST.
pub mod visit;

use std::fmt;

pub use attrs::{ExprMeta, FontSize, NodeAttrs, TypeSize};

/// A complete math expression with display mode.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct MathExpr {
    /// Top-level sequence of nodes making up the expression, in reading order.
    pub nodes: Vec<MathNode>,
    /// Whether the expression renders as block (display) or inline math.
    pub display: DisplayStyle,
    /// Facts about the expression as a whole.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "ExprMeta::is_empty")
    )]
    pub meta: ExprMeta,
}

/// Render mode of a math expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DisplayStyle {
    /// Displayed (block) math, centered on its own line with full-size operators.
    Block,
    /// Inline math, flowed within surrounding text with compact operators.
    #[default]
    Inline,
}

/// Unit basis for an explicitly sized horizontal math space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SpaceBasis {
    /// An em of the text font at the current text size, including inside scripts.
    TextEm,
    /// The current math symbol font's quad (18mu).
    MathMu,
    /// A physical TeX dimension expressed in nominal ten-point ems.
    Physical,
}

/// Source environment for a matrix-like heap node.
///
/// This metadata is separate from [`MathNode::Array`] so existing array
/// constructors and serialized records retain their historical meaning. The
/// compact arena can recover this value for LaTeX matrix environments while
/// generic arrays continue to materialize as `Array`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MatrixKind {
    /// A full-size `matrix` environment.
    Matrix,
    /// A script-sized `smallmatrix` environment.
    SmallMatrix,
}

/// One ordered directive on the edge of a spanning table cell.
///
/// The sequence stored on each side is significant. Repeated directives stay
/// distinct so native LaTeX forms such as `@{}|r`, `|@{}r`, and `||r` retain
/// their source geometry through compact and heap conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SpanBoundary {
    /// Suppress the ordinary intercolumn glue at this boundary (`@{}`).
    SuppressGlue,
    /// Draw one native vertical rule at this boundary (`|`).
    Rule,
}

impl SpanBoundary {
    pub(crate) fn latex(self) -> &'static str {
        match self {
            Self::SuppressGlue => "@{}",
            Self::Rule => "|",
        }
    }
}

pub(crate) fn span_boundaries(mut spelling: &str) -> impl Iterator<Item = SpanBoundary> + '_ {
    std::iter::from_fn(move || {
        if let Some(rest) = spelling.strip_prefix("@{}") {
            spelling = rest;
            Some(SpanBoundary::SuppressGlue)
        } else if let Some(rest) = spelling.strip_prefix('|') {
            spelling = rest;
            Some(SpanBoundary::Rule)
        } else {
            None
        }
    })
}

pub(crate) fn parse_span_boundary_spelling(spelling: &str) -> Vec<SpanBoundary> {
    span_boundaries(spelling).collect()
}

pub(crate) fn span_boundary_has(spelling: &str, boundary: SpanBoundary) -> bool {
    span_boundaries(spelling).any(|value| value == boundary)
}

/// A span belongs to a whole cell. Wrappers among sibling nodes contribute
/// their content without changing the number of occupied columns.
pub(crate) fn table_cell_span(cell: &[MathNode]) -> usize {
    match cell {
        [MathNode::SpannedCell { span, .. }] => usize::from((*span).max(1)),
        _ => 1,
    }
}

pub(crate) fn table_row_width(row: &[Vec<MathNode>]) -> usize {
    row.iter().fold(0usize, |width, cell| {
        width.saturating_add(table_cell_span(cell))
    })
}

/// Preserve the existing heap representation for ordinary tables. When a
/// span expands the logical grid, apply the same size limit as the readers
/// before allocating column widths or emitting rectangular padding.
pub(crate) fn table_width(rows: &[Vec<Vec<MathNode>>], declared: usize) -> crate::Result<usize> {
    let mut width = declared;
    let mut expanded = false;
    for row in rows {
        let occupied = table_row_width(row);
        expanded |= occupied > row.len();
        width = width.max(occupied);
    }
    if expanded && width.saturating_mul(rows.len()) > arena::AstArena::MAX_CHILDREN {
        return Err(crate::Error::LimitExceeded {
            kind: crate::error::LimitKind::ListLength,
            limit: arena::AstArena::MAX_CHILDREN,
        });
    }
    Ok(width)
}

/// Only a wrapper occupying the whole cell supplies its alignment.
pub(crate) fn table_cell_parts(cell: &[MathNode]) -> (Option<ColumnAlign>, &[MathNode]) {
    match cell {
        [MathNode::AlignedCell { alignment, content }]
        | [
            MathNode::SpannedCell {
                alignment, content, ..
            },
        ] => {
            let nodes = match content.as_ref() {
                MathNode::Row(nodes) => nodes.as_slice(),
                node => std::slice::from_ref(node),
            };
            (Some(*alignment), nodes)
        },
        _ => (None, cell),
    }
}

/// A node in the shared mathematical expression tree.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
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
    /// Explicit space with its TeX unit basis preserved.
    ///
    /// This additive variant is used for parsed `\kern`, `\hskip`, `\mkern`,
    /// and `\mskip` nodes. Existing callers constructing [`MathNode::Space`]
    /// retain their historical current-em behavior.
    ExplicitSpace {
        /// Width expressed in the selected basis.
        width: Rational,
        /// Unit basis used to lay out and serialize this space.
        basis: SpaceBasis,
    },

    // === Grouping / delimiters ===
    /// Group acting as a unit (`{...}` in TeX, `<mrow>` in MathML)
    Row(Vec<MathNode>),
    /// Delimited group with open/close and optional mid-separators
    Fenced {
        /// Opening delimiter (e.g. `(`, `[`, `\{`); empty string for none.
        open: String,
        /// Closing delimiter (e.g. `)`, `]`, `\}`); empty string for none.
        close: String,
        /// Inner content, interleaving elements with `\middle` separators.
        content: Vec<FencedItem>,
    },

    // === Scripts ===
    /// Subscript: base_sub
    Subscript {
        /// Expression the subscript attaches to.
        base: Box<MathNode>,
        /// Subscript content.
        sub: Box<MathNode>,
    },
    /// Superscript: base^sup
    Superscript {
        /// Expression the superscript attaches to.
        base: Box<MathNode>,
        /// Superscript content.
        sup: Box<MathNode>,
    },
    /// Combined sub+superscript: base_sub^sup
    SubSup {
        /// Expression the scripts attach to.
        base: Box<MathNode>,
        /// Subscript content.
        sub: Box<MathNode>,
        /// Superscript content.
        sup: Box<MathNode>,
    },
    /// Pre-scripts (isotope notation): _{pre_sub}^{pre_sup}base
    PreScripts {
        /// Expression the pre-scripts attach to.
        base: Box<MathNode>,
        /// Pre-subscript content, if present.
        pre_sub: Option<Box<MathNode>>,
        /// Pre-superscript content, if present.
        pre_sup: Option<Box<MathNode>>,
    },

    // === Over/Under ===
    /// Content placed over base
    Overscript {
        /// Expression the overscript sits above.
        base: Box<MathNode>,
        /// Content drawn above the base.
        over: Box<MathNode>,
        /// Whether the overscript moves to a superscript position in inline style.
        movable_limits: bool,
    },
    /// Content placed under base
    Underscript {
        /// Expression the underscript sits below.
        base: Box<MathNode>,
        /// Content drawn below the base.
        under: Box<MathNode>,
        /// Whether the underscript moves to a subscript position in inline style.
        movable_limits: bool,
    },
    /// Content placed both over and under base
    UnderOver {
        /// Expression the scripts sit above and below.
        base: Box<MathNode>,
        /// Content drawn below the base.
        under: Box<MathNode>,
        /// Content drawn above the base.
        over: Box<MathNode>,
        /// Whether the scripts move to sub/superscript positions in inline style.
        movable_limits: bool,
    },

    // === Fractions and roots ===
    /// Fraction: num/den with configurable style
    Fraction {
        /// Fraction style (bar visibility and sizing).
        kind: FractionKind,
        /// Numerator expression.
        num: Box<MathNode>,
        /// Denominator expression.
        den: Box<MathNode>,
    },
    /// Square root
    Sqrt(Box<MathNode>),
    /// Nth root with explicit index
    Root {
        /// Root index (the small number in the radical's notch).
        index: Box<MathNode>,
        /// Expression under the radical sign.
        radicand: Box<MathNode>,
    },

    // === N-ary operations ===
    /// N-ary operator: summation, integral, product, etc.
    Nary {
        /// Operator character (e.g. `∑`, `∫`, `∏`).
        op: char,
        /// Lower limit, if present.
        sub: Option<Box<MathNode>>,
        /// Upper limit, if present.
        sup: Option<Box<MathNode>>,
        /// Operand the operator applies to.
        body: Box<MathNode>,
        /// Whether limits render above/below or as sub/superscripts.
        limits: LimitStyle,
    },

    // === Accent ===
    /// Accent mark above or below base
    Accent {
        /// Expression the accent decorates.
        base: Box<MathNode>,
        /// Accent character (e.g. `̂` for hat, `̃` for tilde).
        accent: char,
        /// Whether the accent is drawn above or below the base.
        position: AccentPosition,
        /// `true` for wide/stretchy accents like `\widehat`, `\widetilde`, `\widecheck`.
        /// `false` for fixed-size accents like `\hat`, `\tilde`, `\check`.
        #[cfg_attr(feature = "serde", serde(default))]
        wide: bool,
    },

    // === Tables/Arrays ===
    /// Matrix, equation array, cases, etc.
    Array {
        /// Per-column horizontal alignments.
        columns: Vec<ColumnAlign>,
        /// Rows of cells, where each cell is a sequence of nodes.
        rows: Vec<Vec<Vec<MathNode>>>,
        /// Optional `\arraystretch` row-height multiplier.
        arraystretch: Option<f64>,
        /// Whether to add `\jot` extra vertical spacing between rows.
        add_jot: bool,
    },
    // === Decorators ===
    /// Invisible but occupies space (kind controls which dimensions to keep)
    Phantom(Box<MathNode>, PhantomKind),
    /// Content reflected horizontally around its vertical centerline.
    Reflect {
        /// Reflected content.
        content: Box<MathNode>,
        /// Whether the wrapper uses LaTeX's text/hbox mode.
        text_mode: bool,
    },
    /// Visible filled rectangle: `\rule[lift]{width}{height}`.
    Rule {
        /// Rectangle width.
        width: Rational,
        /// Rectangle height.
        height: Rational,
        /// Vertical offset above the baseline.
        lift: Rational,
    },
    /// Bordered box around content (with optional background/border colors for \colorbox/\fcolorbox)
    Boxed {
        /// Boxed expression.
        content: Box<MathNode>,
        /// Optional background fill color (`\colorbox`/`\fcolorbox`).
        bg_color: Option<String>,
        /// Optional border color (`\fcolorbox`).
        border_color: Option<String>,
    },
    /// Actuarial angle enclosure with a top and right rule.
    Actuarial {
        /// Enclosed content.
        content: Box<MathNode>,
        /// Whether the enclosure came from a LaTeX hbox and forces text sizing.
        text_mode: bool,
    },
    /// Strikethrough/cancellation
    Cancel {
        /// Direction/shape of the cancellation stroke.
        stroke: StrokeKind,
        /// Expression being struck through.
        content: Box<MathNode>,
    },
    /// Scaled to a factor of normal size
    Scaled {
        /// Scale factor relative to normal size.
        factor: Rational,
        /// Expression being scaled.
        content: Box<MathNode>,
    },
    /// Vertically shifted box: `\raisebox{shift}[height][depth]{content}`.
    RaiseBox {
        /// Vertical shift (positive raises, negative lowers).
        shift: Rational,
        /// Optional overridden box height.
        height: Option<Rational>,
        /// Optional overridden box depth.
        depth: Option<Rational>,
        /// Expression placed in the raised box.
        content: Box<MathNode>,
    },
    /// Applies a text/font style to children
    Styled(TextStyle, Vec<MathNode>),
    /// Style-dependent content: \mathchoice{D}{T}{S}{SS}
    MathChoice {
        /// Variant used in display style.
        display: Box<MathNode>,
        /// Variant used in text (inline) style.
        text: Box<MathNode>,
        /// Variant used in script style.
        script: Box<MathNode>,
        /// Variant used in script-script style.
        script_script: Box<MathNode>,
    },
    /// Explicit TeX math-style declaration (`\displaystyle`, `\textstyle`,
    /// `\scriptstyle`, or `\scriptscriptstyle`). The compact representation
    /// stores the same value in [`compact::CompactNode::StyleSwitch`]. Values
    /// 4 and 5 are reserved for MathML's display style at script optical sizes.
    StyleSwitch(u8),
    /// Explicit atom-type override: \mathop, \mathrel, \mathbin, etc.
    MathAtom {
        /// TeX atom type controlling spacing.
        atom_type: MathAtomType,
        /// Expression reclassified with this atom type.
        content: Box<MathNode>,
    },
    /// Applies a foreground color to its content.
    ///
    /// This is additive to the compact arena's color wrapper and preserves
    /// foreground color when callers materialize the heap AST.
    Color {
        /// CSS/TeX color name or value.
        color: String,
        /// Colored content.
        content: Box<MathNode>,
    },
    /// Matrix-like table retaining its source environment and full heap
    /// representation. This variant is appended after all historical variants
    /// so ordinal serializers retain every released discriminant. Existing
    /// `Array` values remain the representation for generic arrays and legacy
    /// callers.
    Matrix {
        /// Matrix source environment.
        kind: MatrixKind,
        /// Uniform alignment applied to every matrix column.
        alignment: ColumnAlign,
        /// Rows of cells, where each cell is a sequence of nodes.
        rows: Vec<Vec<Vec<MathNode>>>,
    },
    /// A structured operatorname/operatorname-star whose content remains an
    /// expression, preserving decorations, spaces, and limit placement.
    StructuredOperator {
        /// Parsed operator content.
        content: Box<MathNode>,
        /// Effective placement for scripts attached to this operator.
        /// Source form and effective limit placement.
        form: OperatorNameForm,
    },
    /// Horizontal alignment of a whole array or matrix cell. Place this
    /// wrapper as the cell’s sole node. In other positions, its content is
    /// typeset with the surrounding expression.
    AlignedCell {
        /// Authored cell alignment.
        alignment: ColumnAlign,
        /// Cell content.
        content: Box<MathNode>,
    },
    /// A native LaTeX array retaining layout metadata that the historical
    /// [`MathNode::Array`] shape cannot represent.
    ///
    /// Readers use this variant for native TeX arrays with their padded or
    /// trimmed outer edges, and for MathML tables carrying rule geometry. Existing
    /// user-constructed [`MathNode::Array`] values keep their original shape
    /// and default layout semantics.
    NativeArray {
        /// Per-column horizontal alignments.
        columns: Vec<ColumnAlign>,
        /// Rows of cells, where each cell is a sequence of nodes.
        rows: Vec<Vec<Vec<MathNode>>>,
        /// Optional `\arraystretch` row-height multiplier.
        arraystretch: Option<f64>,
        /// Whether to add `\jot` extra vertical spacing between rows.
        add_jot: bool,
        /// Whether native array edge padding and intercolumn glue apply.
        array_style: bool,
        /// Vertical rule positions from the native column preamble.
        col_seps: u32,
        /// Horizontal rule positions from native `\hline` commands.
        row_seps: u32,
        /// The rules of `col_seps` drawn dashed (`:` in the preamble, MathML
        /// `dashed`). Bits without a rule in `col_seps` are ignored.
        #[cfg_attr(feature = "serde", serde(default))]
        dashed_col_seps: u32,
        /// The rules of `row_seps` drawn dashed (`\hdashline`, MathML
        /// `dashed`). Bits without a rule in `row_seps` are ignored.
        #[cfg_attr(feature = "serde", serde(default))]
        dashed_row_seps: u32,
    },
    /// A native table cell occupying multiple logical columns.
    SpannedCell {
        /// Authored alignment of the complete span.
        alignment: ColumnAlign,
        /// Number of logical columns occupied by this cell.
        span: u16,
        /// Ordered native preamble directives on the left edge.
        left_boundaries: Vec<SpanBoundary>,
        /// Ordered native preamble directives on the right edge.
        right_boundaries: Vec<SpanBoundary>,
        /// Cell content.
        content: Box<MathNode>,
    },
    /// A LaTeX control sequence that no table, package or macro defines,
    /// kept as written (KaTeX's `throwOnError: false` rendering) instead of
    /// being turned into identifier letters. See [`undefined`].
    UndefinedCommand {
        /// The command as written, backslash included (`\foo`).
        name: String,
        /// The brace groups written directly after it, which may be its
        /// arguments; its arity is unknown. Each is one group's content.
        arguments: Vec<Vec<MathNode>>,
    },
    /// Text-size declaration (`\tiny` … `\Huge`, MathML `mathsize="120%"`)
    /// that scales the nodes after it in the same row. The compact
    /// representation stores the same value in
    /// [`compact::CompactNode::SizeScale`]: 0 = `\tiny`, 1 = `\scriptsize`,
    /// 2 = `\footnotesize`, 3 = `\small`, 4 = `\normalsize`, 5 = `\large`,
    /// 6 = `\Large`, 7 = `\LARGE`, 8 = `\huge`, 9 = `\Huge`.
    SizeSwitch(u8),
    /// A line break (`\\` in an environment or at the top level, MathML
    /// `<mspace linebreak="newline"/>`). The compact representation is
    /// [`compact::CompactNode::Newline`].
    LineBreak,
    /// A node with typesetting attributes, which the compact representation
    /// keeps beside its nodes ([`AstArena::attrs`](crate::AstArena::attrs)). Attributes of a
    /// `Row` are those of its children there, so converting to the compact
    /// representation and back moves them to each child.
    Attributed {
        /// The attributes.
        attrs: NodeAttrs,
        /// The node they apply to.
        content: Box<MathNode>,
    },
}

/// TeX math class of a symbol, controlling spacing and stretchiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SymbolKind {
    /// Ordinary symbol with no special spacing (e.g. `\partial`, `\infty`).
    Ordinary,
    /// Large operator that may carry limits (e.g. `\sum`, `\int`).
    LargeOp,
    /// Binary operator with surrounding medium space (e.g. `+`, `\times`).
    BinaryOp,
    /// Relation with surrounding thick space (e.g. `=`, `\leq`).
    Relation,
    /// Opening delimiter (e.g. `(`, `[`).
    Open,
    /// Closing delimiter (e.g. `)`, `]`).
    Close,
    /// Punctuation with trailing space (e.g. `,`, `;`).
    Punctuation,
    /// Stretchable fence delimiter (e.g. `|`, `\|`).
    Fence,
    /// Alphabetic symbol treated as a letter (e.g. a Greek letter).
    Alphabetic,
}

/// Font variant (math alphabet) applied to text or symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TextStyle {
    /// Upright (roman) normal weight, e.g. `\mathrm`.
    Normal,
    /// Bold weight, e.g. `\mathbf`.
    Bold,
    /// Italic, e.g. `\mathit` (the default for variables).
    Italic,
    /// Bold italic.
    BoldItalic,
    /// Monospace (typewriter), e.g. `\mathtt`.
    Monospace,
    /// Sans-serif, e.g. `\mathsf`.
    SansSerif,
    /// Bold sans-serif.
    SansSerifBold,
    /// Italic sans-serif.
    SansSerifItalic,
    /// Bold italic sans-serif.
    SansSerifBoldItalic,
    /// Double-struck (blackboard bold), e.g. `\mathbb`.
    DoubleStruck,
    /// Script (calligraphic), e.g. `\mathcal`/`\mathscr`.
    Script,
    /// Bold script.
    BoldScript,
    /// Fraktur (blackletter), e.g. `\mathfrak`.
    Fraktur,
    /// Bold fraktur.
    BoldFraktur,
}

/// Style of a fraction, controlling bar visibility and sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum FractionKind {
    /// Standard `\frac`: bar shown, size follows current style.
    Normal,
    /// `\dfrac`: forced display-style sizing.
    Display,
    /// `\cfrac`: display-style fraction with TeX's continued-fraction strut
    /// and null-delimiter correction.
    Continued,
    /// `\tfrac`: forced text (inline) sizing.
    Inline,
    /// `\binom`-style fraction with no fraction bar.
    NoBar,
    /// `\sfrac`-style skewed (diagonal) fraction.
    Skewed,
}

/// Horizontal alignment of an array/matrix column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ColumnAlign {
    /// Left-aligned (`l`).
    Left,
    /// Centered (`c`).
    Center,
    /// Right-aligned (`r`).
    Right,
}

/// Placement of an accent relative to its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AccentPosition {
    /// Accent drawn above the base (e.g. `\hat`, `\bar`).
    Above,
    /// Accent drawn below the base (e.g. `\underbar`).
    Below,
}

/// Direction/shape of a cancellation stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum StrokeKind {
    /// Diagonal line from bottom-left to top-right (`\cancel`).
    Forward,
    /// Diagonal line from top-left to bottom-right (`\bcancel`).
    Backward,
    /// Both diagonals forming an X (`\xcancel`).
    Cross,
    /// Horizontal strike-through line.
    Horizontal,
    /// Vertical strike-through line.
    Vertical,
}

/// Distinguishes `\phantom` from `\vphantom`, `\hphantom`, and `\smash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PhantomKind {
    /// `\phantom`: keep all dimensions (width, height, depth).
    #[default]
    Full,
    /// `\vphantom`: zero width, keep height and depth.
    Vertical,
    /// `\hphantom`: zero height and depth, keep width.
    Horizontal,
    /// `\smash` or `\smash[bt]`: render content, zero height AND depth.
    SmashBoth,
    /// `\smash[b]`: render content, zero depth only.
    SmashBottom,
    /// `\smash[t]`: render content, zero height only.
    SmashTop,
}

/// One size of the `\big` family as stored in a `Scaled` node.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BigDelimSize {
    /// Scale factor of the `Scaled` node.
    pub scale: Rational,
    /// LaTeX command name without the class suffix.
    pub command: &'static str,
    /// The `minsize`/`maxsize` MathJax writes (TeX's 8.5pt, 11.5pt, 14.5pt
    /// and 17.5pt boxes, relative to `\big`).
    pub mathjax: &'static str,
    /// `mathjax` as a number of em.
    pub mathjax_em: f64,
}

/// `\big`, `\Big`, `\bigg` and `\Bigg`.
pub(crate) const BIG_DELIM_SIZES: [BigDelimSize; 4] = [
    BigDelimSize {
        scale: Rational { num: 6, den: 5 },
        command: "big",
        mathjax: "1.2em",
        mathjax_em: 1.2,
    },
    BigDelimSize {
        scale: Rational { num: 9, den: 5 },
        command: "Big",
        mathjax: "1.623em",
        mathjax_em: 1.623,
    },
    BigDelimSize {
        scale: Rational { num: 12, den: 5 },
        command: "bigg",
        mathjax: "2.047em",
        mathjax_em: 2.047,
    },
    BigDelimSize {
        scale: Rational { num: 3, den: 1 },
        command: "Bigg",
        mathjax: "2.470em",
        mathjax_em: 2.47,
    },
];

/// TeX atom type for spacing classification (\mathord, \mathop, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MathAtomType {
    /// Ordinary atom (`\mathord`): no extra spacing.
    Ord,
    /// Large operator atom (`\mathop`): may take limits.
    Op,
    /// Binary operator atom (`\mathbin`): medium space on both sides.
    Bin,
    /// Relation atom (`\mathrel`): thick space on both sides.
    Rel,
    /// Opening atom (`\mathopen`): delimiter-like opening spacing.
    Open,
    /// Closing atom (`\mathclose`): delimiter-like closing spacing.
    Close,
    /// Punctuation atom (`\mathpunct`): trailing space.
    Punct,
    /// Inner atom (`\mathinner`): treated as a self-contained subformula.
    Inner,
}

/// Whether operator limits sit above/below or as sub/superscripts.
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

/// One element inside a [`MathNode::Fenced`] group's content list.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FencedItem {
    /// A normal content node.
    Element(MathNode),
    /// A `\middle` separator with the given delimiter string.
    Middle(String),
}

/// A named math operator together with its limit-placement behavior.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OperatorAttr {
    /// Operator name as it appears in the formula (e.g. `"sin"`, `"lim"`).
    pub name: String,
    /// How limits attach to this operator.
    pub limit_style: LimitStyle,
}

/// Source form of a structured operator-name expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OperatorNameForm {
    /// An unstarred operator name. Its scripts stay at the side.
    Unstarred,
    /// A starred operator name with its effective placement.
    Starred(LimitStyle),
}

impl OperatorNameForm {
    /// Return the effective placement used by renderers and writers.
    #[inline]
    pub const fn limit_style(self) -> LimitStyle {
        match self {
            Self::Unstarred => LimitStyle::SubSup,
            Self::Starred(style) => style,
        }
    }

    /// Return whether this form came from a starred operator name.
    #[inline]
    pub const fn is_starred(self) -> bool {
        matches!(self, Self::Starred(_))
    }
}

/// Exact rational number for spacing (avoids floating-point imprecision).
/// Stored in lowest terms: `gcd(|num|, den) == 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rational {
    /// Signed numerator.
    pub num: i32,
    /// Strictly positive denominator.
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
    /// Creates a rational reduced to lowest terms.
    ///
    /// # Panics
    /// Panics if `den` is zero.
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

    /// The rational `0/1`.
    #[must_use]
    pub fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    /// The rational `1/1`.
    #[must_use]
    pub fn one() -> Self {
        Self { num: 1, den: 1 }
    }

    /// Converts to an `f64` (`num / den`).
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
    /// Creates an expression from nodes with the given display mode.
    #[must_use]
    pub fn new(nodes: Vec<MathNode>, display: DisplayStyle) -> Self {
        Self {
            nodes,
            display,
            meta: ExprMeta::default(),
        }
    }

    /// Creates an inline expression from the given nodes.
    #[must_use]
    pub fn inline(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Inline,
            meta: ExprMeta::default(),
        }
    }

    /// Creates a block (display) expression from the given nodes.
    #[must_use]
    pub fn block(nodes: Vec<MathNode>) -> Self {
        Self {
            nodes,
            display: DisplayStyle::Block,
            meta: ExprMeta::default(),
        }
    }
}

/// Scale factors, in thousandths of the normal size, of the text-size
/// declarations stored in [`MathNode::SizeSwitch`] /
/// [`compact::CompactNode::SizeScale`], indexed `\tiny` (0) … `\Huge` (9).
pub(crate) const SIZE_SCALE_PER_MILLE: [u16; 10] =
    [500, 700, 800, 900, 1000, 1200, 1440, 1728, 2074, 2488];

/// The size declaration closest to `scale` (a multiple of the normal size)
/// and whether it matches within 1%. `None` for a non-positive or non-finite
/// scale.
pub(crate) fn nearest_size_scale(scale: f64) -> Option<(u8, bool)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let log = scale.ln();
    let (index, per_mille) = SIZE_SCALE_PER_MILLE
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (f64::from(**a) / 1000.0).ln() - log;
            let db = (f64::from(**b) / 1000.0).ln() - log;
            da.abs().total_cmp(&db.abs())
        })?;
    let exact = (f64::from(*per_mille) / 1000.0 / scale - 1.0).abs() <= 0.01;
    Some((index as u8, exact))
}

/// Whether any node of a heap tree has attributes, as deep as trees are
/// written. (No deeper one is.)
pub(crate) fn has_attributes(expr: &MathExpr) -> bool {
    struct Finder {
        found: bool,
        depth: usize,
    }
    impl visit::Visitor for Finder {
        fn visit_node(&mut self, node: &MathNode) {
            if matches!(node, MathNode::Attributed { .. }) {
                self.found = true;
            }
            if !self.found && self.depth <= MAX_AST_DEPTH {
                self.depth += 1;
                visit::walk_node(self, node);
                self.depth -= 1;
            }
        }
    }
    let mut finder = Finder {
        found: false,
        depth: 0,
    };
    visit::Visitor::visit_expr(&mut finder, expr);
    finder.found
}

/// Maximum allowed AST nesting depth for safe recursive traversal.
pub const MAX_AST_DEPTH: usize = 128;

/// Validate the nesting depth and the logical grid size of spanning tables.
/// Call before any recursive AST traversal (writing, rendering, normalization)
/// to prevent stack overflow on maliciously deep or programmatically-constructed trees.
pub fn validate_ast_depth(expr: &MathExpr) -> crate::error::Result<()> {
    validate_ast_depth_with(expr, MAX_AST_DEPTH)
}

/// Validate a heap AST against a caller's depth limit.
///
/// The effective limit is clamped to [`MAX_AST_DEPTH`], which keeps every
/// recursive validation and writer traversal bounded even when a caller uses
/// a larger configuration value. A root node is at depth zero.
pub fn validate_ast_depth_with(expr: &MathExpr, limit: usize) -> crate::error::Result<()> {
    let limit = limit.min(MAX_AST_DEPTH);
    for node in &expr.nodes {
        check_node_depth(node, 0, limit)?;
    }
    Ok(())
}

/// Validate compact AST nesting depth against [`MAX_AST_DEPTH`].
pub fn validate_compact_depth(
    arena: &arena::AstArena,
    roots: compact::NodeSpan,
) -> crate::error::Result<()> {
    validate_compact_depth_with(arena, roots, MAX_AST_DEPTH)
}

/// Validate compact AST nesting depth against `limit` (clamped to
/// [`MAX_AST_DEPTH`]). A root node is at depth 0; a tree whose deepest node
/// lies below `limit` fails with [`crate::Error::LimitExceeded`] and
/// [`crate::LimitKind::Depth`]. An arena that refused a list or outgrew its
/// indices fails with the corresponding [`crate::LimitKind`]. Missing or
/// out-of-bounds node references on the traversed path return an error.
/// Handles are local indices, so an in-bounds index taken from another arena
/// refers to the node at that index in this arena.
pub fn validate_compact_depth_with(
    arena: &arena::AstArena,
    roots: compact::NodeSpan,
    limit: usize,
) -> crate::error::Result<()> {
    arena.check_limits()?;
    let limit = limit.min(MAX_AST_DEPTH);
    let roots = arena
        .try_children(roots)
        .ok_or_else(|| invalid_compact("root span is outside its arena"))?;
    for &id in roots {
        check_compact_depth_checked(arena, id, 0, limit)?;
    }
    Ok(())
}

/// Validate a compact tree that may have been assembled by a public caller.
///
/// Unlike [`validate_compact_depth_with`], this also checks every stored arena
/// reference and rejects cycles in unreachable slots. Some direct writer
/// lowering paths inspect all arena nodes, so they use this stricter boundary
/// before traversing caller-owned data.
pub(crate) fn validate_compact_for_write(
    arena: &arena::AstArena,
    roots: compact::NodeSpan,
    limit: usize,
) -> crate::error::Result<()> {
    arena.check_limits()?;
    let limit = limit.min(MAX_AST_DEPTH);
    validate_compact_acyclic_with_depth(arena, roots, limit)
}

fn depth_error(limit: usize) -> crate::error::Error {
    crate::error::Error::LimitExceeded {
        kind: crate::error::LimitKind::Depth,
        limit,
    }
}

/// The side of a horizontal brace or bracket over or under its base
/// (`\overbrace`, `\underbracket`), an operator whose scripts are limits.
pub(crate) fn brace_side(arena: &arena::AstArena, id: compact::NodeId) -> Option<AccentPosition> {
    use compact::CompactNode;
    let is = |id, brace| matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == brace);
    let (position, brace) = match *arena.node(id) {
        CompactNode::Overscript { over, .. } => (AccentPosition::Above, is(over, "\u{23DE}")),
        CompactNode::Underscript { under, .. } => (AccentPosition::Below, is(under, "\u{23DF}")),
        CompactNode::Accent {
            accent,
            position,
            wide,
            ..
        } => (
            position,
            matches!(
                (accent, position, wide),
                ('\u{23DE}', AccentPosition::Above, _)
                    | ('\u{23DF}', AccentPosition::Below, _)
                    | ('\u{23B4}', AccentPosition::Above, true)
                    | ('\u{23B5}', AccentPosition::Below, true)
            ),
        ),
        _ => return None,
    };
    brace.then_some(position)
}

/// The direct children of a compact node, in the order the depth validator
/// visits them (fence `Middle` separators and absent optional slots are
/// skipped; array column alignments are not children).
pub(crate) fn compact_children(
    arena: &arena::AstArena,
    node: &compact::CompactNode,
) -> smallvec::SmallVec<[compact::NodeId; 4]> {
    use compact::CompactNode;
    let mut out = smallvec::SmallVec::new();
    let mut push = |id: compact::NodeId| {
        if id.is_some() {
            out.push(id);
        }
    };
    match *node {
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
        | CompactNode::SizeScale(_) => {},
        CompactNode::Row(span)
        | CompactNode::Styled(_, span)
        | CompactNode::UndefinedCommand {
            arguments: span, ..
        } => {
            arena.children(span).iter().for_each(|&id| push(id));
        },
        CompactNode::Fenced { content, .. } => {
            for &id in arena.children(content) {
                if !matches!(*arena.node(id), CompactNode::Middle(_)) {
                    push(id);
                }
            }
        },
        CompactNode::Array { cells, .. } => {
            arena.children(cells).iter().for_each(|&id| push(id));
        },
        CompactNode::Subscript { base, sub } => [base, sub].into_iter().for_each(push),
        CompactNode::Superscript { base, sup } => [base, sup].into_iter().for_each(push),
        CompactNode::SubSup { base, sub, sup } => [base, sub, sup].into_iter().for_each(push),
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => [base, pre_sub, pre_sup].into_iter().for_each(push),
        CompactNode::Overscript { base, over, .. } => [base, over].into_iter().for_each(push),
        CompactNode::Underscript { base, under, .. } => [base, under].into_iter().for_each(push),
        CompactNode::UnderOver {
            base, under, over, ..
        } => [base, under, over].into_iter().for_each(push),
        CompactNode::Fraction { num, den, .. } => [num, den].into_iter().for_each(push),
        CompactNode::Sqrt(inner) => push(inner),
        CompactNode::Root { index, radicand } => [index, radicand].into_iter().for_each(push),
        CompactNode::Nary { sub, sup, body, .. } => [sub, sup, body].into_iter().for_each(push),
        CompactNode::Accent { base, .. } => push(base),
        CompactNode::Phantom(inner, _)
        | CompactNode::Boxed { content: inner, .. }
        | CompactNode::Actuarial { content: inner, .. } => push(inner),
        CompactNode::Reflect { content: inner, .. } => push(inner),
        CompactNode::Cancel { content, .. }
        | CompactNode::Scaled { content, .. }
        | CompactNode::RaiseBox { content, .. }
        | CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. }
        | CompactNode::AlignedCell { content, .. }
        | CompactNode::SpannedCell { content, .. } => push(content),
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => [display, text, script, script_script]
            .into_iter()
            .for_each(push),
    }
    out
}

/// Validate all stored nodes iteratively, including unreachable nodes. A
/// compact arena retains dead nodes after normalization, and a few writer
/// lowering passes inspect those storage slots while looking for metadata. The
/// cycle check and subtree-height computation keep malformed caller-owned
/// arenas off the call stack and make shared children depth-sensitive for each
/// written root.
fn validate_compact_acyclic_with_depth(
    arena: &arena::AstArena,
    roots: compact::NodeSpan,
    limit: usize,
) -> crate::error::Result<()> {
    let mut state = vec![0_u8; arena.node_count()];
    let mut heights = vec![0_usize; arena.node_count()];
    check_compact_span(arena, roots)?;
    let roots = arena
        .try_children(roots)
        .ok_or_else(|| invalid_compact("root span is outside its arena"))?;
    for &root in roots {
        visit_compact_graph(arena, root, &mut state, &mut heights, Some(limit))?;
        if heights[root.index()] > limit {
            return Err(depth_error(limit));
        }
    }
    // Dead nodes remain in arenas after normalization. Validate their handles,
    // metadata and cycles, but do not charge their depth to the written roots.
    for raw in 0..arena.node_count() as u32 {
        let id = compact::NodeId::new(raw);
        if state[id.index()] == 0 {
            visit_compact_graph(arena, id, &mut state, &mut heights, None)?;
        }
    }
    Ok(())
}

/// Visit one arena graph with an explicit child list per frame. The cached
/// subtree height makes a previously visited shared child count again when a
/// later root reaches it at a greater depth.
fn visit_compact_graph(
    arena: &arena::AstArena,
    start: compact::NodeId,
    state: &mut [u8],
    heights: &mut [usize],
    limit: Option<usize>,
) -> crate::error::Result<()> {
    if state[start.index()] == 2 {
        if let Some(limit) = limit
            && heights[start.index()] > limit
        {
            return Err(depth_error(limit));
        }
        return Ok(());
    }
    if state[start.index()] == 1 {
        return Err(invalid_compact("cyclic node reference"));
    }
    if !start.is_some() || arena.try_node(start).is_none() {
        return Err(invalid_compact("node reference belongs to another arena"));
    }

    struct Frame {
        id: compact::NodeId,
        children: smallvec::SmallVec<[compact::NodeId; 4]>,
        next: usize,
        depth: usize,
    }

    check_compact_node_data(arena, arena.node(start))?;
    state[start.index()] = 1;
    let mut stack = vec![Frame {
        id: start,
        children: compact_children(arena, arena.node(start)),
        next: 0,
        depth: 0,
    }];
    while let Some(frame) = stack.last_mut() {
        if frame.next < frame.children.len() {
            let child = frame.children[frame.next];
            frame.next += 1;
            let child_depth = frame.depth.saturating_add(1);
            match state[child.index()] {
                0 => {
                    if let Some(limit) = limit
                        && child_depth > limit
                    {
                        return Err(depth_error(limit));
                    }
                    check_compact_node_data(arena, arena.node(child))?;
                    state[child.index()] = 1;
                    stack.push(Frame {
                        id: child,
                        children: compact_children(arena, arena.node(child)),
                        next: 0,
                        depth: child_depth,
                    });
                },
                1 => return Err(invalid_compact("cyclic node reference")),
                2 => {
                    if let Some(limit) = limit
                        && child_depth.saturating_add(heights[child.index()]) > limit
                    {
                        return Err(depth_error(limit));
                    }
                },
                _ => unreachable!("compact validation state is a byte"),
            }
        } else {
            let height = frame
                .children
                .iter()
                .map(|&child| heights[child.index()].saturating_add(1))
                .max()
                .unwrap_or(0);
            heights[frame.id.index()] = height;
            state[frame.id.index()] = 2;
            stack.pop();
        }
    }
    Ok(())
}

fn invalid_compact(message: &str) -> crate::error::Error {
    crate::error::Error::Unsupported(format!("invalid compact AST: {message}"))
}

fn check_compact_id(
    arena: &arena::AstArena,
    id: compact::NodeId,
    required: bool,
) -> crate::error::Result<()> {
    if id.is_none() {
        return if required {
            Err(invalid_compact("missing node reference"))
        } else {
            Ok(())
        };
    }
    arena
        .try_node(id)
        .map(|_| ())
        .ok_or_else(|| invalid_compact("node reference belongs to another arena"))
}

fn check_compact_span(
    arena: &arena::AstArena,
    span: compact::NodeSpan,
) -> crate::error::Result<()> {
    let children = arena
        .try_children(span)
        .ok_or_else(|| invalid_compact("child span is outside its arena"))?;
    for &id in children {
        check_compact_id(arena, id, true)?;
    }
    Ok(())
}

fn check_compact_string(arena: &arena::AstArena, id: compact::StrId) -> crate::error::Result<()> {
    arena
        .try_str_val(id)
        .map(|_| ())
        .ok_or_else(|| invalid_compact("string reference belongs to another arena"))
}

fn check_compact_node_data(
    arena: &arena::AstArena,
    node: &compact::CompactNode,
) -> crate::error::Result<()> {
    use compact::CompactNode;

    match *node {
        CompactNode::Number(text) | CompactNode::Identifier(text) | CompactNode::Middle(text) => {
            check_compact_string(arena, text)?
        },
        CompactNode::Operator { name, .. }
        | CompactNode::Symbol(_, name)
        | CompactNode::Text(_, name) => check_compact_string(arena, name)?,
        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            check_compact_string(arena, open)?;
            check_compact_string(arena, close)?;
            check_compact_span(arena, content)?;
        },
        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            check_compact_span(arena, span)?;
        },
        CompactNode::UndefinedCommand { name, arguments } => {
            check_compact_string(arena, name)?;
            check_compact_span(arena, arguments)?;
        },
        CompactNode::Array {
            col_aligns, cells, ..
        } => {
            check_compact_span(arena, col_aligns)?;
            check_compact_span(arena, cells)?;
        },
        CompactNode::SpannedCell {
            left_boundaries,
            right_boundaries,
            ..
        } => {
            check_compact_string(arena, left_boundaries)?;
            check_compact_string(arena, right_boundaries)?;
        },
        CompactNode::Boxed {
            bg_color,
            border_color,
            ..
        } => {
            check_compact_string(arena, bg_color)?;
            check_compact_string(arena, border_color)?;
        },
        CompactNode::Color { color, .. } => check_compact_string(arena, color)?,
        _ => {},
    }

    let required = |id| check_compact_id(arena, id, true);
    let optional = |id| check_compact_id(arena, id, false);
    match *node {
        CompactNode::Subscript { base, sub }
        | CompactNode::Superscript { base, sup: sub }
        | CompactNode::Fraction {
            num: base,
            den: sub,
            ..
        } => {
            required(base)?;
            required(sub)?;
        },
        CompactNode::SubSup { base, sub, sup }
        | CompactNode::UnderOver {
            base,
            under: sub,
            over: sup,
            ..
        } => {
            required(base)?;
            required(sub)?;
            required(sup)?;
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            required(base)?;
            optional(pre_sub)?;
            optional(pre_sup)?;
        },
        CompactNode::Overscript { base, over, .. }
        | CompactNode::Underscript {
            base, under: over, ..
        } => {
            required(base)?;
            required(over)?;
        },
        CompactNode::Accent { base, .. } => required(base)?,
        CompactNode::Sqrt(inner)
        | CompactNode::Phantom(inner, _)
        | CompactNode::Reflect { content: inner, .. }
        | CompactNode::Actuarial { content: inner, .. }
        | CompactNode::Cancel { content: inner, .. }
        | CompactNode::Scaled { content: inner, .. }
        | CompactNode::RaiseBox { content: inner, .. }
        | CompactNode::Color { content: inner, .. }
        | CompactNode::MathAtom { content: inner, .. }
        | CompactNode::StructuredOperator { content: inner, .. }
        | CompactNode::AlignedCell { content: inner, .. } => required(inner)?,
        CompactNode::Root { index, radicand } => {
            required(index)?;
            required(radicand)?;
        },
        CompactNode::Nary { sub, sup, body, .. } => {
            optional(sub)?;
            optional(sup)?;
            required(body)?;
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            required(display)?;
            required(text)?;
            required(script)?;
            required(script_script)?;
        },
        CompactNode::Boxed { content, .. } => required(content)?,
        CompactNode::SpannedCell { content, .. } => required(content)?,
        _ => {},
    }
    Ok(())
}

/// Checked counterpart of [`compact_children`] for the cheap depth validator.
/// It follows only the reachable graph and therefore avoids scanning dead
/// arena slots on the normal parsed-tree path.
fn compact_children_checked(
    arena: &arena::AstArena,
    node: &compact::CompactNode,
) -> crate::error::Result<smallvec::SmallVec<[compact::NodeId; 4]>> {
    use compact::CompactNode;
    let mut out = smallvec::SmallVec::new();
    match *node {
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
        | CompactNode::SizeScale(_) => {},
        CompactNode::Row(span_)
        | CompactNode::Styled(_, span_)
        | CompactNode::UndefinedCommand {
            arguments: span_, ..
        } => append_compact_span_checked(arena, &mut out, span_)?,
        CompactNode::Fenced { content, .. } => {
            let children = arena
                .try_children(content)
                .ok_or_else(|| invalid_compact("child span is outside its arena"))?;
            for &id in children {
                if matches!(arena.try_node(id), Some(CompactNode::Middle(_))) {
                    continue;
                }
                append_compact_id_checked(arena, &mut out, id, true)?;
            }
        },
        CompactNode::Array { cells, .. } => append_compact_span_checked(arena, &mut out, cells)?,
        CompactNode::Subscript { base, sub }
        | CompactNode::Superscript { base, sup: sub }
        | CompactNode::Fraction {
            num: base,
            den: sub,
            ..
        } => {
            append_compact_id_checked(arena, &mut out, base, true)?;
            append_compact_id_checked(arena, &mut out, sub, true)?;
        },
        CompactNode::SubSup { base, sub, sup }
        | CompactNode::UnderOver {
            base,
            under: sub,
            over: sup,
            ..
        } => {
            append_compact_id_checked(arena, &mut out, base, true)?;
            append_compact_id_checked(arena, &mut out, sub, true)?;
            append_compact_id_checked(arena, &mut out, sup, true)?;
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            append_compact_id_checked(arena, &mut out, base, true)?;
            append_compact_id_checked(arena, &mut out, pre_sub, false)?;
            append_compact_id_checked(arena, &mut out, pre_sup, false)?;
        },
        CompactNode::Overscript { base, over, .. }
        | CompactNode::Underscript {
            base, under: over, ..
        } => {
            append_compact_id_checked(arena, &mut out, base, true)?;
            append_compact_id_checked(arena, &mut out, over, true)?;
        },
        CompactNode::Accent { base, .. } => append_compact_id_checked(arena, &mut out, base, true)?,
        CompactNode::Sqrt(inner)
        | CompactNode::Phantom(inner, _)
        | CompactNode::Reflect { content: inner, .. }
        | CompactNode::Boxed { content: inner, .. }
        | CompactNode::Actuarial { content: inner, .. }
        | CompactNode::Cancel { content: inner, .. }
        | CompactNode::Scaled { content: inner, .. }
        | CompactNode::RaiseBox { content: inner, .. }
        | CompactNode::Color { content: inner, .. }
        | CompactNode::MathAtom { content: inner, .. }
        | CompactNode::StructuredOperator { content: inner, .. }
        | CompactNode::AlignedCell { content: inner, .. }
        | CompactNode::SpannedCell { content: inner, .. } => {
            append_compact_id_checked(arena, &mut out, inner, true)?
        },
        CompactNode::Root { index, radicand } => {
            append_compact_id_checked(arena, &mut out, index, true)?;
            append_compact_id_checked(arena, &mut out, radicand, true)?;
        },
        CompactNode::Nary { sub, sup, body, .. } => {
            append_compact_id_checked(arena, &mut out, sub, false)?;
            append_compact_id_checked(arena, &mut out, sup, false)?;
            append_compact_id_checked(arena, &mut out, body, true)?;
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            append_compact_id_checked(arena, &mut out, display, true)?;
            append_compact_id_checked(arena, &mut out, text, true)?;
            append_compact_id_checked(arena, &mut out, script, true)?;
            append_compact_id_checked(arena, &mut out, script_script, true)?;
        },
    }
    Ok(out)
}

fn append_compact_id_checked(
    arena: &arena::AstArena,
    out: &mut smallvec::SmallVec<[compact::NodeId; 4]>,
    id: compact::NodeId,
    required: bool,
) -> crate::error::Result<()> {
    if id.is_none() {
        return if required {
            Err(invalid_compact("missing node reference"))
        } else {
            Ok(())
        };
    }
    if arena.try_node(id).is_none() {
        return Err(invalid_compact("node reference belongs to another arena"));
    }
    out.push(id);
    Ok(())
}

fn append_compact_span_checked(
    arena: &arena::AstArena,
    out: &mut smallvec::SmallVec<[compact::NodeId; 4]>,
    span: compact::NodeSpan,
) -> crate::error::Result<()> {
    let children = arena
        .try_children(span)
        .ok_or_else(|| invalid_compact("child span is outside its arena"))?;
    for &id in children {
        append_compact_id_checked(arena, out, id, true)?;
    }
    Ok(())
}

fn check_compact_depth_checked(
    arena: &arena::AstArena,
    id: compact::NodeId,
    depth: usize,
    limit: usize,
) -> crate::error::Result<()> {
    if depth > limit {
        return Err(depth_error(limit));
    }
    let node = arena
        .try_node(id)
        .ok_or_else(|| invalid_compact("node reference belongs to another arena"))?;
    for child in compact_children_checked(arena, node)? {
        check_compact_depth_checked(arena, child, depth + 1, limit)?;
    }
    Ok(())
}

fn check_node_depth(node: &MathNode, depth: usize, limit: usize) -> crate::error::Result<()> {
    if depth > limit {
        return Err(depth_error(limit));
    }
    let d = depth + 1;
    match node {
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_)
        | MathNode::ExplicitSpace { .. }
        | MathNode::Rule { .. }
        | MathNode::StyleSwitch(_)
        | MathNode::SizeSwitch(_)
        | MathNode::LineBreak => Ok(()),
        // Not a level of its own, as in the arena; only attributes nested
        // directly, which no reader builds, count.
        MathNode::Attributed { content, .. } => {
            let nested = matches!(**content, MathNode::Attributed { .. });
            check_node_depth(content, if nested { d } else { depth }, limit)
        },

        MathNode::Row(children) | MathNode::Styled(_, children) => {
            for child in children {
                check_node_depth(child, d, limit)?;
            }
            Ok(())
        },
        MathNode::UndefinedCommand { arguments, .. } => {
            // Each argument is a group, one level down as in the arena.
            for child in arguments.iter().flatten() {
                check_node_depth(child, d + 1, limit)?;
            }
            Ok(())
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let FencedItem::Element(n) = item {
                    check_node_depth(n, d, limit)?;
                }
            }
            Ok(())
        },
        MathNode::Subscript { base, sub } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(sub, d, limit)
        },
        MathNode::Superscript { base, sup } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(sup, d, limit)
        },
        MathNode::SubSup { base, sub, sup } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(sub, d, limit)?;
            check_node_depth(sup, d, limit)
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            check_node_depth(base, d, limit)?;
            if let Some(s) = pre_sub {
                check_node_depth(s, d, limit)?;
            }
            if let Some(s) = pre_sup {
                check_node_depth(s, d, limit)?;
            }
            Ok(())
        },
        MathNode::Overscript { base, over, .. } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(over, d, limit)
        },
        MathNode::Underscript { base, under, .. } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(under, d, limit)
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            check_node_depth(base, d, limit)?;
            check_node_depth(under, d, limit)?;
            check_node_depth(over, d, limit)
        },
        MathNode::Fraction { num, den, .. } => {
            check_node_depth(num, d, limit)?;
            check_node_depth(den, d, limit)
        },
        MathNode::Sqrt(inner) => check_node_depth(inner, d, limit),
        MathNode::Root { index, radicand } => {
            check_node_depth(index, d, limit)?;
            check_node_depth(radicand, d, limit)
        },
        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                check_node_depth(s, d, limit)?;
            }
            if let Some(s) = sup {
                check_node_depth(s, d, limit)?;
            }
            check_node_depth(body, d, limit)
        },
        MathNode::Accent { base, .. } => check_node_depth(base, d, limit),
        MathNode::Array { rows, .. }
        | MathNode::Matrix { rows, .. }
        | MathNode::NativeArray { rows, .. } => {
            let declared = match node {
                MathNode::Array { columns, .. } | MathNode::NativeArray { columns, .. } => {
                    columns.len()
                },
                _ => 1,
            };
            table_width(rows, declared)?;
            for row in rows {
                for cell in row {
                    for n in cell {
                        check_node_depth(n, d, limit)?;
                    }
                }
            }
            Ok(())
        },
        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            check_node_depth(content, d, limit)
        },
        MathNode::Phantom(inner, _)
        | MathNode::Boxed { content: inner, .. }
        | MathNode::Actuarial { content: inner, .. } => check_node_depth(inner, d, limit),
        MathNode::Reflect { content: inner, .. } => check_node_depth(inner, d, limit),
        MathNode::Cancel { content, .. }
        | MathNode::Scaled { content, .. }
        | MathNode::RaiseBox { content, .. }
        | MathNode::Color { content, .. }
        | MathNode::StructuredOperator { content, .. } => check_node_depth(content, d, limit),
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            check_node_depth(display, d, limit)?;
            check_node_depth(text, d, limit)?;
            check_node_depth(script, d, limit)?;
            check_node_depth(script_script, d, limit)
        },
        MathNode::MathAtom { content, .. } => check_node_depth(content, d, limit),
    }
}

/// Fast depth check for parser-owned compact nodes. Public boundaries use the
/// checked variants above; parser construction has already established arena
/// handle validity and uses this hot path while closing nested groups.
pub(crate) fn check_compact_depth(
    arena: &arena::AstArena,
    id: compact::NodeId,
    depth: usize,
    limit: usize,
) -> crate::error::Result<()> {
    if depth > limit {
        return Err(depth_error(limit));
    }
    for child in compact_children(arena, arena.node(id)) {
        check_compact_depth(arena, child, depth + 1, limit)?;
    }
    Ok(())
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
    fn math_node_variants_are_constructible() {
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
            MathNode::ExplicitSpace {
                width: Rational::one(),
                basis: SpaceBasis::MathMu,
            },
            MathNode::Reflect {
                content: Box::new(MathNode::Identifier("x".into())),
                text_mode: false,
            },
            MathNode::Actuarial {
                content: Box::new(MathNode::Identifier("x".into())),
                text_mode: false,
            },
            MathNode::StyleSwitch(1),
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
                wide: false,
            },
            MathNode::Array {
                columns: vec![ColumnAlign::Center],
                rows: vec![vec![vec![MathNode::Number("1".into())]]],
                arraystretch: None,
                add_jot: false,
            },
            MathNode::Phantom(Box::new(MathNode::Number("0".into())), PhantomKind::Full),
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
            MathNode::Color {
                color: "red".into(),
                content: Box::new(MathNode::Identifier("x".into())),
            },
            MathNode::MathChoice {
                display: Box::new(MathNode::Identifier("D".into())),
                text: Box::new(MathNode::Identifier("T".into())),
                script: Box::new(MathNode::Identifier("S".into())),
                script_script: Box::new(MathNode::Identifier("SS".into())),
            },
            MathNode::Rule {
                width: Rational::new(1, 2),
                height: Rational::new(2, 1),
                lift: Rational::new(0, 1),
            },
            MathNode::RaiseBox {
                shift: Rational::new(1, 4),
                height: None,
                depth: None,
                content: Box::new(MathNode::Identifier("x".into())),
            },
            MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                content: Box::new(MathNode::Identifier("f".into())),
            },
            MathNode::NativeArray {
                columns: vec![ColumnAlign::Left],
                rows: vec![vec![vec![MathNode::Identifier("x".into())]]],
                arraystretch: None,
                add_jot: false,
                array_style: true,
                col_seps: 0,
                row_seps: 0,
                dashed_col_seps: 0,
                dashed_row_seps: 0,
            },
            MathNode::StructuredOperator {
                content: Box::new(MathNode::Identifier("f".into())),
                form: OperatorNameForm::Starred(LimitStyle::Default),
            },
        ];
        assert_eq!(nodes.len(), 37);
        #[cfg(feature = "serde")]
        for node in nodes {
            let json = serde_json::to_value(&node).unwrap();
            assert_eq!(node, serde_json::from_value(json).unwrap());
        }
    }
}
