use smallvec::SmallVec;

use super::arena::AstArena;
use super::{
    AccentPosition, ColumnAlign, DisplayStyle, FencedItem, FractionKind, LimitStyle, MathExpr,
    MathNode, OperatorAttr, Rational, StrokeKind, SymbolKind, TextStyle,
};

// ── Index types ────────────────────────────────────────────────────────

/// Index into `AstArena::nodes`. 4 bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Sentinel value representing "no node" (replaces `Option<Box<MathNode>>`).
    pub const NONE: Self = Self(u32::MAX);

    #[inline]
    pub(crate) fn new(raw: u32) -> Self {
        debug_assert!(
            raw < u32::MAX,
            "NodeId::new called with u32::MAX (reserved for NONE)"
        );
        Self(raw)
    }

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == u32::MAX
    }

    #[inline]
    pub fn is_some(self) -> bool {
        self.0 != u32::MAX
    }

    #[inline]
    pub fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Contiguous range of children in `AstArena::children`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NodeSpan {
    start: u32,
    len: u16,
}

impl NodeSpan {
    pub const EMPTY: Self = Self { start: 0, len: 0 };

    #[inline]
    pub(crate) fn new(start: u32, len: u16) -> Self {
        Self { start, len }
    }

    #[inline]
    pub fn start(self) -> u32 {
        self.start
    }

    #[inline]
    pub fn len(self) -> u16 {
        self.len
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// Index into `AstArena`'s string pool. 4 bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StrId(u32);

impl StrId {
    #[inline]
    pub(crate) fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

// ── CompactNode ────────────────────────────────────────────────────────

/// Arena-backed AST node. 29 variants. All data lives in the owning `AstArena`.
///
/// This type is `Copy` — no heap allocations, no destructors. Nodes reference
/// each other via `NodeId` and store strings via `StrId`, both indexing into
/// the parent `AstArena`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompactNode {
    // ── Leaf nodes ─────────────────────────────────────────────────
    Number(StrId),
    Identifier(StrId),
    Operator {
        name: StrId,
        limits: LimitStyle,
    },
    Symbol(SymbolKind, StrId),
    Text(TextStyle, StrId),
    Space(Rational),

    // ── Grouping ───────────────────────────────────────────────────
    Row(NodeSpan),
    Fenced {
        open: StrId,
        close: StrId,
        content: NodeSpan,
    },

    // ── Scripts ────────────────────────────────────────────────────
    Subscript {
        base: NodeId,
        sub: NodeId,
    },
    Superscript {
        base: NodeId,
        sup: NodeId,
    },
    SubSup {
        base: NodeId,
        sub: NodeId,
        sup: NodeId,
    },
    PreScripts {
        base: NodeId,
        pre_sub: NodeId,
        pre_sup: NodeId,
    },

    // ── Over/Under ─────────────────────────────────────────────────
    Overscript {
        base: NodeId,
        over: NodeId,
        movable_limits: bool,
    },
    Underscript {
        base: NodeId,
        under: NodeId,
        movable_limits: bool,
    },
    UnderOver {
        base: NodeId,
        under: NodeId,
        over: NodeId,
        movable_limits: bool,
    },

    // ── Fractions/roots ────────────────────────────────────────────
    Fraction {
        kind: FractionKind,
        num: NodeId,
        den: NodeId,
    },
    Sqrt(NodeId),
    Root {
        index: NodeId,
        radicand: NodeId,
    },

    // ── N-ary ──────────────────────────────────────────────────────
    Nary {
        op: char,
        sub: NodeId,
        sup: NodeId,
        body: NodeId,
        limits: LimitStyle,
    },

    // ── Accent ─────────────────────────────────────────────────────
    Accent {
        base: NodeId,
        accent: char,
        position: AccentPosition,
    },

    // ── Array ──────────────────────────────────────────────────────
    /// Cells stored flat: `cells[row * num_cols + col]` gives the Row node for each cell.
    /// Column alignments stored as `ColAlign` nodes in the `col_aligns` span.
    Array {
        col_aligns: NodeSpan,
        cells: NodeSpan,
        num_cols: u16,
    },

    // ── Decorators ─────────────────────────────────────────────────
    Phantom(NodeId),
    Boxed(NodeId),
    Cancel {
        stroke: StrokeKind,
        content: NodeId,
    },
    Scaled {
        factor: Rational,
        content: NodeId,
    },
    Styled(TextStyle, NodeSpan),

    // ── Color ───────────────────────────────────────────────────────
    /// Color wrapper: applies a named color to its content.
    Color { color: StrId, content: NodeId },

    // ── Helpers (not in original MathNode) ──────────────────────────
    /// `FencedItem::Middle` replacement. Fenced content spans mix regular nodes and middles.
    Middle(StrId),
    /// Array column alignment storage node.
    ColAlign(ColumnAlign),

    /// `\rule[lift]{width}{height}` — visible filled rectangle.
    Rule { width: Rational, height: Rational },

    /// Math style switch: 0=Display, 1=Text, 2=Script, 3=ScriptScript.
    StyleSwitch(u8),
}

// Compile-time size check: CompactNode must fit in 32 bytes.
const _: () = assert!(
    size_of::<CompactNode>() <= 32,
    "CompactNode exceeds 32-byte limit"
);

// ── CompactExpr ────────────────────────────────────────────────────────

/// A complete math expression stored in a compact arena.
#[derive(Debug, Clone)]
pub struct CompactExpr {
    pub arena: AstArena,
    pub roots: NodeSpan,
    pub display: DisplayStyle,
}

impl CompactExpr {
    #[must_use]
    pub fn new(arena: AstArena, roots: NodeSpan, display: DisplayStyle) -> Self {
        Self {
            arena,
            roots,
            display,
        }
    }

    /// Build a `CompactExpr` from a `MathExpr`, interning all data into an arena.
    #[must_use]
    pub fn from_math_expr(expr: &MathExpr) -> Self {
        let mut arena = AstArena::new();
        let mut root_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for node in &expr.nodes {
            root_ids.push(push_math_node(&mut arena, node));
        }
        let roots = arena.push_children(&root_ids);
        Self {
            arena,
            roots,
            display: expr.display,
        }
    }

    /// Reconstruct a `MathExpr` from the arena. O(n) walk.
    #[must_use]
    pub fn to_math_expr(&self) -> MathExpr {
        let mut nodes = Vec::with_capacity(self.roots.len() as usize);
        for &id in self.arena.children(self.roots) {
            nodes.push(reconstruct_node(&self.arena, id));
        }
        MathExpr {
            nodes,
            display: self.display,
        }
    }
}

// ── MathNode → CompactNode (arena builder) ─────────────────────────────

fn push_math_node(arena: &mut AstArena, node: &MathNode) -> NodeId {
    match node {
        MathNode::Number(s) => {
            let sid = arena.push_str(s);
            arena.push_node(CompactNode::Number(sid))
        },
        MathNode::Identifier(s) => {
            let sid = arena.push_str(s);
            arena.push_node(CompactNode::Identifier(sid))
        },
        MathNode::Operator(attr) => {
            let name = arena.push_str(&attr.name);
            arena.push_node(CompactNode::Operator {
                name,
                limits: attr.limit_style,
            })
        },
        MathNode::Symbol(kind, s) => {
            let sid = arena.push_str(s);
            arena.push_node(CompactNode::Symbol(*kind, sid))
        },
        MathNode::Text(style, s) => {
            let sid = arena.push_str(s);
            arena.push_node(CompactNode::Text(*style, sid))
        },
        MathNode::Space(r) => arena.push_node(CompactNode::Space(*r)),

        MathNode::Row(children) => {
            let span = push_children(arena, children);
            arena.push_node(CompactNode::Row(span))
        },
        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let open_id = arena.push_str(open);
            let close_id = arena.push_str(close);
            let mut ids: SmallVec<[NodeId; 4]> = SmallVec::new();
            for item in content {
                match item {
                    FencedItem::Element(n) => ids.push(push_math_node(arena, n)),
                    FencedItem::Middle(s) => {
                        let sid = arena.push_str(s);
                        ids.push(arena.push_node(CompactNode::Middle(sid)));
                    },
                }
            }
            let content_span = arena.push_children(&ids);
            arena.push_node(CompactNode::Fenced {
                open: open_id,
                close: close_id,
                content: content_span,
            })
        },

        MathNode::Subscript { base, sub } => {
            let base_id = push_math_node(arena, base);
            let sub_id = push_math_node(arena, sub);
            arena.push_node(CompactNode::Subscript {
                base: base_id,
                sub: sub_id,
            })
        },
        MathNode::Superscript { base, sup } => {
            let base_id = push_math_node(arena, base);
            let sup_id = push_math_node(arena, sup);
            arena.push_node(CompactNode::Superscript {
                base: base_id,
                sup: sup_id,
            })
        },
        MathNode::SubSup { base, sub, sup } => {
            let base_id = push_math_node(arena, base);
            let sub_id = push_math_node(arena, sub);
            let sup_id = push_math_node(arena, sup);
            arena.push_node(CompactNode::SubSup {
                base: base_id,
                sub: sub_id,
                sup: sup_id,
            })
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            let base_id = push_math_node(arena, base);
            let pre_sub_id = push_opt(arena, pre_sub.as_deref());
            let pre_sup_id = push_opt(arena, pre_sup.as_deref());
            arena.push_node(CompactNode::PreScripts {
                base: base_id,
                pre_sub: pre_sub_id,
                pre_sup: pre_sup_id,
            })
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            let base_id = push_math_node(arena, base);
            let over_id = push_math_node(arena, over);
            arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: *movable_limits,
            })
        },
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            let base_id = push_math_node(arena, base);
            let under_id = push_math_node(arena, under);
            arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: *movable_limits,
            })
        },
        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            let base_id = push_math_node(arena, base);
            let under_id = push_math_node(arena, under);
            let over_id = push_math_node(arena, over);
            arena.push_node(CompactNode::UnderOver {
                base: base_id,
                under: under_id,
                over: over_id,
                movable_limits: *movable_limits,
            })
        },

        MathNode::Fraction { kind, num, den } => {
            let num_id = push_math_node(arena, num);
            let den_id = push_math_node(arena, den);
            arena.push_node(CompactNode::Fraction {
                kind: *kind,
                num: num_id,
                den: den_id,
            })
        },
        MathNode::Sqrt(inner) => {
            let inner_id = push_math_node(arena, inner);
            arena.push_node(CompactNode::Sqrt(inner_id))
        },
        MathNode::Root { index, radicand } => {
            let index_id = push_math_node(arena, index);
            let radicand_id = push_math_node(arena, radicand);
            arena.push_node(CompactNode::Root {
                index: index_id,
                radicand: radicand_id,
            })
        },

        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let sub_id = push_opt(arena, sub.as_deref());
            let sup_id = push_opt(arena, sup.as_deref());
            let body_id = push_math_node(arena, body);
            arena.push_node(CompactNode::Nary {
                op: *op,
                sub: sub_id,
                sup: sup_id,
                body: body_id,
                limits: *limits,
            })
        },

        MathNode::Accent {
            base,
            accent,
            position,
        } => {
            let base_id = push_math_node(arena, base);
            arena.push_node(CompactNode::Accent {
                base: base_id,
                accent: *accent,
                position: *position,
            })
        },

        MathNode::Array { columns, rows } => {
            // Push column alignments as ColAlign nodes
            let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
            for col in columns {
                align_ids.push(arena.push_node(CompactNode::ColAlign(*col)));
            }
            let col_aligns = arena.push_children(&align_ids);

            let num_cols = columns.len().max(1) as u16;

            // Push cells flat (row-major). Each cell Vec<MathNode> becomes a Row node.
            let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
            for row in rows {
                for cell in row {
                    let span = push_children(arena, cell);
                    cell_ids.push(arena.push_node(CompactNode::Row(span)));
                }
                // Pad short rows to num_cols
                for _ in row.len()..(num_cols as usize) {
                    cell_ids.push(arena.push_node(CompactNode::Row(NodeSpan::EMPTY)));
                }
            }
            let cells = arena.push_children(&cell_ids);

            arena.push_node(CompactNode::Array {
                col_aligns,
                cells,
                num_cols,
            })
        },

        MathNode::Phantom(inner) => {
            let inner_id = push_math_node(arena, inner);
            arena.push_node(CompactNode::Phantom(inner_id))
        },
        MathNode::Boxed(inner) => {
            let inner_id = push_math_node(arena, inner);
            arena.push_node(CompactNode::Boxed(inner_id))
        },
        MathNode::Cancel { stroke, content } => {
            let content_id = push_math_node(arena, content);
            arena.push_node(CompactNode::Cancel {
                stroke: *stroke,
                content: content_id,
            })
        },
        MathNode::Scaled { factor, content } => {
            let content_id = push_math_node(arena, content);
            arena.push_node(CompactNode::Scaled {
                factor: *factor,
                content: content_id,
            })
        },
        MathNode::Styled(style, children) => {
            let span = push_children(arena, children);
            arena.push_node(CompactNode::Styled(*style, span))
        },
    }
}

/// Push a slice of MathNodes as children, returning their NodeSpan.
fn push_children(arena: &mut AstArena, nodes: &[MathNode]) -> NodeSpan {
    let mut ids: SmallVec<[NodeId; 2]> = SmallVec::new();
    for node in nodes {
        ids.push(push_math_node(arena, node));
    }
    arena.push_children(&ids)
}

/// Push an optional node, returning `NodeId::NONE` for `None`.
fn push_opt(arena: &mut AstArena, node: Option<&MathNode>) -> NodeId {
    match node {
        Some(n) => push_math_node(arena, n),
        None => NodeId::NONE,
    }
}

// ── CompactNode → MathNode (arena reader) ──────────────────────────────

fn reconstruct_node(arena: &AstArena, id: NodeId) -> MathNode {
    match *arena.node(id) {
        CompactNode::Number(sid) => MathNode::Number(arena.str_val(sid).to_owned()),
        CompactNode::Identifier(sid) => MathNode::Identifier(arena.str_val(sid).to_owned()),
        CompactNode::Operator { name, limits } => MathNode::Operator(OperatorAttr {
            name: arena.str_val(name).to_owned(),
            limit_style: limits,
        }),
        CompactNode::Symbol(kind, sid) => MathNode::Symbol(kind, arena.str_val(sid).to_owned()),
        CompactNode::Text(style, sid) => MathNode::Text(style, arena.str_val(sid).to_owned()),
        CompactNode::Space(r) => MathNode::Space(r),

        CompactNode::Row(span) => MathNode::Row(reconstruct_children(arena, span)),
        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            let mut items = Vec::with_capacity(content.len() as usize);
            for &child_id in arena.children(content) {
                match *arena.node(child_id) {
                    CompactNode::Middle(sid) => {
                        items.push(FencedItem::Middle(arena.str_val(sid).to_owned()));
                    },
                    _ => {
                        items.push(FencedItem::Element(reconstruct_node(arena, child_id)));
                    },
                }
            }
            MathNode::Fenced {
                open: arena.str_val(open).to_owned(),
                close: arena.str_val(close).to_owned(),
                content: items,
            }
        },

        CompactNode::Subscript { base, sub } => MathNode::Subscript {
            base: Box::new(reconstruct_node(arena, base)),
            sub: Box::new(reconstruct_node(arena, sub)),
        },
        CompactNode::Superscript { base, sup } => MathNode::Superscript {
            base: Box::new(reconstruct_node(arena, base)),
            sup: Box::new(reconstruct_node(arena, sup)),
        },
        CompactNode::SubSup { base, sub, sup } => MathNode::SubSup {
            base: Box::new(reconstruct_node(arena, base)),
            sub: Box::new(reconstruct_node(arena, sub)),
            sup: Box::new(reconstruct_node(arena, sup)),
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => MathNode::PreScripts {
            base: Box::new(reconstruct_node(arena, base)),
            pre_sub: reconstruct_opt(arena, pre_sub),
            pre_sup: reconstruct_opt(arena, pre_sup),
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => MathNode::Overscript {
            base: Box::new(reconstruct_node(arena, base)),
            over: Box::new(reconstruct_node(arena, over)),
            movable_limits,
        },
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => MathNode::Underscript {
            base: Box::new(reconstruct_node(arena, base)),
            under: Box::new(reconstruct_node(arena, under)),
            movable_limits,
        },
        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => MathNode::UnderOver {
            base: Box::new(reconstruct_node(arena, base)),
            under: Box::new(reconstruct_node(arena, under)),
            over: Box::new(reconstruct_node(arena, over)),
            movable_limits,
        },

        CompactNode::Fraction { kind, num, den } => MathNode::Fraction {
            kind,
            num: Box::new(reconstruct_node(arena, num)),
            den: Box::new(reconstruct_node(arena, den)),
        },
        CompactNode::Sqrt(inner) => MathNode::Sqrt(Box::new(reconstruct_node(arena, inner))),
        CompactNode::Root { index, radicand } => MathNode::Root {
            index: Box::new(reconstruct_node(arena, index)),
            radicand: Box::new(reconstruct_node(arena, radicand)),
        },

        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => MathNode::Nary {
            op,
            sub: reconstruct_opt(arena, sub),
            sup: reconstruct_opt(arena, sup),
            body: Box::new(reconstruct_node(arena, body)),
            limits,
        },

        CompactNode::Accent {
            base,
            accent,
            position,
        } => MathNode::Accent {
            base: Box::new(reconstruct_node(arena, base)),
            accent,
            position,
        },

        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
        } => {
            // Reconstruct column alignments
            let columns: Vec<ColumnAlign> = arena
                .children(col_aligns)
                .iter()
                .map(|&id| match *arena.node(id) {
                    CompactNode::ColAlign(a) => a,
                    _ => ColumnAlign::Center, // fallback
                })
                .collect();

            // Reconstruct rows from flat cells
            let cell_ids = arena.children(cells);
            let nc = num_cols as usize;
            let mut rows = Vec::new();
            for chunk in cell_ids.chunks(nc.max(1)) {
                let mut row = Vec::with_capacity(chunk.len());
                for &cell_id in chunk {
                    // Each cell is a Row node wrapping the cell contents
                    match *arena.node(cell_id) {
                        CompactNode::Row(span) => {
                            row.push(reconstruct_children(arena, span));
                        },
                        _ => {
                            // Fallback: wrap single node
                            row.push(vec![reconstruct_node(arena, cell_id)]);
                        },
                    }
                }
                rows.push(row);
            }

            MathNode::Array { columns, rows }
        },

        CompactNode::Phantom(inner) => MathNode::Phantom(Box::new(reconstruct_node(arena, inner))),
        CompactNode::Boxed(inner) => MathNode::Boxed(Box::new(reconstruct_node(arena, inner))),
        CompactNode::Cancel { stroke, content } => MathNode::Cancel {
            stroke,
            content: Box::new(reconstruct_node(arena, content)),
        },
        CompactNode::Scaled { factor, content } => MathNode::Scaled {
            factor,
            content: Box::new(reconstruct_node(arena, content)),
        },
        CompactNode::Styled(style, span) => {
            MathNode::Styled(style, reconstruct_children(arena, span))
        },

        // Color wrapper — no MathNode::Color exists, so reconstruct content only.
        // Color is handled directly in the compact/arena layout paths.
        CompactNode::Color { content, .. } => reconstruct_node(arena, content),

        // Helper nodes — should not appear as standalone roots, but handle gracefully
        CompactNode::Middle(sid) => {
            MathNode::Symbol(SymbolKind::Fence, arena.str_val(sid).to_owned())
        },
        CompactNode::ColAlign(_) => MathNode::Space(Rational::zero()),
        CompactNode::Rule { width, height: _ } => {
            // Reconstruct as a Space with the width dimension (best approximation)
            MathNode::Space(width)
        },
        CompactNode::StyleSwitch(_) => {
            // Style switches are layout-only directives; reconstruct as empty row
            MathNode::Row(Vec::new())
        },
    }
}

fn reconstruct_children(arena: &AstArena, span: NodeSpan) -> Vec<MathNode> {
    arena
        .children(span)
        .iter()
        .map(|&id| reconstruct_node(arena, id))
        .collect()
}

fn reconstruct_opt(arena: &AstArena, id: NodeId) -> Option<Box<MathNode>> {
    if id.is_none() {
        None
    } else {
        Some(Box::new(reconstruct_node(arena, id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{FencedItem, MathExpr, MathNode, OperatorAttr};

    #[test]
    fn compact_node_is_copy() {
        let s = StrId::new(0);
        let node = CompactNode::Number(s);
        let copy = node; // Copy
        assert_eq!(node, copy);
    }

    #[test]
    fn compact_node_size() {
        let size = size_of::<CompactNode>();
        // Should be well under 32 bytes. Print for visibility.
        eprintln!("CompactNode size: {size} bytes");
        assert!(size <= 32);
    }

    #[test]
    fn node_id_size() {
        assert_eq!(size_of::<NodeId>(), 4);
    }

    #[test]
    fn str_id_size() {
        assert_eq!(size_of::<StrId>(), 4);
    }

    #[test]
    fn node_span_size() {
        // u32 (4) + u16 (2) + padding (2) = 8 bytes
        assert!(size_of::<NodeSpan>() <= 8);
    }

    #[test]
    fn node_id_sentinel() {
        assert!(NodeId::NONE.is_none());
        assert!(!NodeId::NONE.is_some());
        assert_eq!(NodeId::NONE.raw(), u32::MAX);
    }

    #[test]
    fn node_span_empty() {
        let span = NodeSpan::EMPTY;
        assert!(span.is_empty());
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn compact_expr_construction() {
        let mut arena = AstArena::new();
        let s = arena.push_str("42");
        let n = arena.push_node(CompactNode::Number(s));
        let roots = arena.push_children(&[n]);
        let expr = CompactExpr::new(arena, roots, DisplayStyle::Inline);
        assert_eq!(expr.display, DisplayStyle::Inline);
        assert_eq!(expr.roots.len(), 1);
    }

    #[test]
    fn all_28_variants_constructible() {
        let mut arena = AstArena::new();
        let s = arena.push_str("x");
        let id = arena.push_node(CompactNode::Identifier(s));
        let span = arena.push_children(&[id]);

        let variants: Vec<CompactNode> = vec![
            CompactNode::Number(s),
            CompactNode::Identifier(s),
            CompactNode::Operator {
                name: s,
                limits: LimitStyle::Default,
            },
            CompactNode::Symbol(SymbolKind::BinaryOp, s),
            CompactNode::Text(TextStyle::Normal, s),
            CompactNode::Space(Rational::new(1, 6)),
            CompactNode::Row(span),
            CompactNode::Fenced {
                open: s,
                close: s,
                content: span,
            },
            CompactNode::Subscript { base: id, sub: id },
            CompactNode::Superscript { base: id, sup: id },
            CompactNode::SubSup {
                base: id,
                sub: id,
                sup: id,
            },
            CompactNode::PreScripts {
                base: id,
                pre_sub: id,
                pre_sup: id,
            },
            CompactNode::Overscript {
                base: id,
                over: id,
                movable_limits: false,
            },
            CompactNode::Underscript {
                base: id,
                under: id,
                movable_limits: false,
            },
            CompactNode::UnderOver {
                base: id,
                under: id,
                over: id,
                movable_limits: true,
            },
            CompactNode::Fraction {
                kind: FractionKind::Normal,
                num: id,
                den: id,
            },
            CompactNode::Sqrt(id),
            CompactNode::Root {
                index: id,
                radicand: id,
            },
            CompactNode::Nary {
                op: '\u{2211}',
                sub: id,
                sup: id,
                body: id,
                limits: LimitStyle::AboveBelow,
            },
            CompactNode::Accent {
                base: id,
                accent: '\u{0302}',
                position: AccentPosition::Above,
            },
            CompactNode::Array {
                col_aligns: span,
                cells: span,
                num_cols: 1,
            },
            CompactNode::Phantom(id),
            CompactNode::Boxed(id),
            CompactNode::Cancel {
                stroke: StrokeKind::Forward,
                content: id,
            },
            CompactNode::Scaled {
                factor: Rational::new(2, 1),
                content: id,
            },
            CompactNode::Styled(TextStyle::Bold, span),
            CompactNode::Color { color: s, content: id },
            CompactNode::Middle(s),
            CompactNode::ColAlign(ColumnAlign::Center),
        ];
        assert_eq!(variants.len(), 29);
    }

    /// Helper: round-trip a MathExpr through CompactExpr and assert equality.
    fn assert_roundtrip(expr: &MathExpr) {
        let compact = CompactExpr::from_math_expr(expr);
        let restored = compact.to_math_expr();
        assert_eq!(expr, &restored, "round-trip failed");
    }

    #[test]
    fn roundtrip_leaf_nodes() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::Number("42".into()),
            MathNode::Identifier("x".into()),
            MathNode::Operator(OperatorAttr {
                name: "sin".into(),
                limit_style: LimitStyle::Default,
            }),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Text(TextStyle::Normal, "hello".into()),
            MathNode::Space(Rational::new(1, 6)),
        ]));
    }

    #[test]
    fn roundtrip_row_and_styled() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::Row(vec![
                MathNode::Number("1".into()),
                MathNode::Number("2".into()),
            ]),
            MathNode::Styled(TextStyle::Bold, vec![MathNode::Identifier("x".into())]),
        ]));
    }

    #[test]
    fn roundtrip_fenced_with_middles() {
        assert_roundtrip(&MathExpr::inline(vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![
                FencedItem::Element(MathNode::Number("1".into())),
                FencedItem::Middle("|".into()),
                FencedItem::Element(MathNode::Number("2".into())),
            ],
        }]));
    }

    #[test]
    fn roundtrip_scripts() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::Subscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sub: Box::new(MathNode::Number("2".into())),
            },
            MathNode::Superscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sup: Box::new(MathNode::Number("3".into())),
            },
            MathNode::SubSup {
                base: Box::new(MathNode::Identifier("x".into())),
                sub: Box::new(MathNode::Number("1".into())),
                sup: Box::new(MathNode::Number("2".into())),
            },
        ]));
    }

    #[test]
    fn roundtrip_prescripts_with_none() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::PreScripts {
                base: Box::new(MathNode::Identifier("C".into())),
                pre_sub: Some(Box::new(MathNode::Number("6".into()))),
                pre_sup: None,
            },
            MathNode::PreScripts {
                base: Box::new(MathNode::Identifier("C".into())),
                pre_sub: None,
                pre_sup: Some(Box::new(MathNode::Number("14".into()))),
            },
        ]));
    }

    #[test]
    fn roundtrip_over_under() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::Overscript {
                base: Box::new(MathNode::Identifier("x".into())),
                over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{0302}".into())),
                movable_limits: false,
            },
            MathNode::Underscript {
                base: Box::new(MathNode::Identifier("x".into())),
                under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "_".into())),
                movable_limits: true,
            },
            MathNode::UnderOver {
                base: Box::new(MathNode::Identifier("x".into())),
                under: Box::new(MathNode::Number("0".into())),
                over: Box::new(MathNode::Number("1".into())),
                movable_limits: true,
            },
        ]));
    }

    #[test]
    fn roundtrip_fraction_and_roots() {
        assert_roundtrip(&MathExpr::inline(vec![
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
        ]));
    }

    #[test]
    fn roundtrip_nary_with_none() {
        assert_roundtrip(&MathExpr::inline(vec![
            MathNode::Nary {
                op: '\u{2211}',
                sub: Some(Box::new(MathNode::Number("0".into()))),
                sup: Some(Box::new(MathNode::Identifier("n".into()))),
                body: Box::new(MathNode::Identifier("i".into())),
                limits: LimitStyle::AboveBelow,
            },
            MathNode::Nary {
                op: '\u{222B}',
                sub: None,
                sup: None,
                body: Box::new(MathNode::Identifier("f".into())),
                limits: LimitStyle::SubSup,
            },
        ]));
    }

    #[test]
    fn roundtrip_accent() {
        assert_roundtrip(&MathExpr::inline(vec![MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '\u{0302}',
            position: AccentPosition::Above,
        }]));
    }

    #[test]
    fn roundtrip_array() {
        assert_roundtrip(&MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Left, ColumnAlign::Right],
            rows: vec![
                vec![
                    vec![MathNode::Number("1".into())],
                    vec![MathNode::Number("2".into())],
                ],
                vec![
                    vec![MathNode::Number("3".into())],
                    vec![MathNode::Number("4".into())],
                ],
            ],
        }]));
    }

    #[test]
    fn roundtrip_decorators() {
        assert_roundtrip(&MathExpr::inline(vec![
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
        ]));
    }

    #[test]
    fn roundtrip_nested_expression() {
        // x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
        let expr = MathExpr::block(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::Relation, "=".into()),
            MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(MathNode::Row(vec![
                    MathNode::Symbol(SymbolKind::Ordinary, "\u{2212}".into()),
                    MathNode::Identifier("b".into()),
                    MathNode::Symbol(SymbolKind::BinaryOp, "\u{00B1}".into()),
                    MathNode::Sqrt(Box::new(MathNode::Row(vec![
                        MathNode::Superscript {
                            base: Box::new(MathNode::Identifier("b".into())),
                            sup: Box::new(MathNode::Number("2".into())),
                        },
                        MathNode::Symbol(SymbolKind::BinaryOp, "\u{2212}".into()),
                        MathNode::Number("4".into()),
                        MathNode::Identifier("a".into()),
                        MathNode::Identifier("c".into()),
                    ]))),
                ])),
                den: Box::new(MathNode::Row(vec![
                    MathNode::Number("2".into()),
                    MathNode::Identifier("a".into()),
                ])),
            },
        ]);
        assert_roundtrip(&expr);
    }

    #[test]
    fn roundtrip_display_style() {
        let block = MathExpr::block(vec![MathNode::Number("1".into())]);
        let inline = MathExpr::inline(vec![MathNode::Number("1".into())]);
        let compact_block = CompactExpr::from_math_expr(&block);
        let compact_inline = CompactExpr::from_math_expr(&inline);
        assert_eq!(compact_block.to_math_expr().display, DisplayStyle::Block);
        assert_eq!(compact_inline.to_math_expr().display, DisplayStyle::Inline);
    }

    #[test]
    fn roundtrip_empty_expr() {
        assert_roundtrip(&MathExpr::inline(vec![]));
    }

    #[test]
    fn memory_report() {
        use crate::error::Config;
        use crate::reader::Reader;
        use crate::reader::latex::LatexReader;

        let compact_size = size_of::<CompactNode>();
        let legacy_size = size_of::<MathNode>();
        let ratio = legacy_size as f64 / compact_size as f64;

        eprintln!("=== Memory Report ===");
        eprintln!("MathNode  size: {legacy_size} bytes");
        eprintln!("CompactNode size: {compact_size} bytes");
        eprintln!("Ratio (legacy/compact): {ratio:.1}x");

        assert!(
            compact_size <= 32,
            "CompactNode should be <= 32 bytes, got {compact_size}"
        );

        let quadratic = r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}";
        let config = Config::default();

        let compact_result = LatexReader::parse_compact(quadratic, &config).unwrap();
        let arena = &compact_result.expr.arena;

        let node_count = arena.node_count();
        let children_count = arena.children_count();
        let string_count = arena.string_count();
        let heap_bytes = arena.heap_bytes();

        eprintln!("--- Quadratic formula arena stats ---");
        eprintln!("Nodes:    {node_count}");
        eprintln!("Children: {children_count}");
        eprintln!("Strings:  {string_count}");
        eprintln!("Arena heap bytes: {heap_bytes}");
        eprintln!(
            "Bytes/node (arena): {:.1}",
            heap_bytes as f64 / node_count as f64
        );

        // Sanity checks
        assert!(node_count > 0, "quadratic formula should have nodes");
        assert!(heap_bytes > 0, "arena should use heap memory");
        assert!(
            (heap_bytes as f64 / node_count as f64) < 100.0,
            "bytes per node should be reasonable"
        );
    }
}
