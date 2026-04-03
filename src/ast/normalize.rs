use smallvec::SmallVec;

use crate::ast::*;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};

/// Normalize a math expression in place.
///
/// Applies the following simplifications bottom-up:
/// 1. Flatten nested Rows: `Row([a, Row([b, c]), d])` → `Row([a, b, c, d])`
/// 2. Unwrap single-child Rows: `Row([x])` → `x`
/// 3. Merge adjacent Text nodes with the same style
/// 4. Remove empty Rows from node lists
pub fn normalize_expr(expr: &mut MathExpr) {
    normalize_nodes_depth(&mut expr.nodes, 0);
}

fn normalize_nodes_depth(nodes: &mut Vec<MathNode>, depth: usize) {
    // Phase 1: recursively normalize each child
    for node in nodes.iter_mut() {
        normalize_node_depth(node, depth);
    }

    // Phase 2: flatten Row children into the parent list
    let mut flattened = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        match node {
            MathNode::Row(children) if children.is_empty() => {
                // Phase 4: remove empty Rows
            },
            MathNode::Row(children) => {
                // Flatten
                flattened.extend(children);
            },
            other => flattened.push(other),
        }
    }

    // Phase 3: merge adjacent Text nodes with the same style
    let mut merged = Vec::with_capacity(flattened.len());
    for node in flattened {
        if let MathNode::Text(style, ref text) = node
            && let Some(MathNode::Text(prev_style, prev_text)) = merged.last_mut()
            && *prev_style == style
        {
            prev_text.push_str(text);
            continue;
        }
        merged.push(node);
    }

    *nodes = merged;
}

fn normalize_node_depth(node: &mut MathNode, depth: usize) {
    // Stop recursing if AST is pathologically deep
    if depth > super::MAX_AST_DEPTH {
        return;
    }
    let d = depth + 1;
    match node {
        MathNode::Row(children) => {
            normalize_nodes_depth(children, d);
            // Unwrap single-child Row
            if children.len() == 1 {
                // Safety: len checked to be exactly 1
                *node = children.pop().expect("len is 1");
            }
        },

        MathNode::Fenced { content, .. } => {
            for item in content.iter_mut() {
                if let FencedItem::Element(n) = item {
                    normalize_node_depth(n, d);
                }
            }
        },

        MathNode::Subscript { base, sub } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sub, d);
        },
        MathNode::Superscript { base, sup } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sup, d);
        },
        MathNode::SubSup { base, sub, sup } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sub, d);
            normalize_node_depth(sup, d);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_node_depth(base, d);
            if let Some(s) = pre_sub {
                normalize_node_depth(s, d);
            }
            if let Some(s) = pre_sup {
                normalize_node_depth(s, d);
            }
        },

        MathNode::Overscript { base, over, .. } => {
            normalize_node_depth(base, d);
            normalize_node_depth(over, d);
        },
        MathNode::Underscript { base, under, .. } => {
            normalize_node_depth(base, d);
            normalize_node_depth(under, d);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_node_depth(base, d);
            normalize_node_depth(under, d);
            normalize_node_depth(over, d);
        },

        MathNode::Fraction { num, den, .. } => {
            normalize_node_depth(num, d);
            normalize_node_depth(den, d);
        },
        MathNode::Sqrt(inner) => {
            normalize_node_depth(inner, d);
        },
        MathNode::Root { index, radicand } => {
            normalize_node_depth(index, d);
            normalize_node_depth(radicand, d);
        },

        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                normalize_node_depth(s, d);
            }
            if let Some(s) = sup {
                normalize_node_depth(s, d);
            }
            normalize_node_depth(body, d);
        },

        MathNode::Accent { base, .. } => {
            normalize_node_depth(base, d);
        },

        MathNode::Array { rows, .. } => {
            for row in rows.iter_mut() {
                for cell in row.iter_mut() {
                    normalize_nodes_depth(cell, d);
                }
            }
        },

        MathNode::Phantom(inner) | MathNode::Boxed(inner) => {
            normalize_node_depth(inner, d);
        },
        MathNode::Cancel { content, .. } => {
            normalize_node_depth(content, d);
        },
        MathNode::Scaled { content, .. } => {
            normalize_node_depth(content, d);
        },
        MathNode::Styled(_, children) => {
            normalize_nodes_depth(children, d);
        },

        // Leaf nodes: nothing to normalize
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_) => {},
    }
}

// ── Compact arena-based normalization ──────────────────────────────────

/// Normalize a compact expression in place.
///
/// Same semantics as `normalize_expr`: flatten nested Rows, unwrap
/// single-child Rows, merge adjacent Text nodes, remove empty Rows.
/// Operates directly on the arena — dead nodes become unreachable garbage.
pub fn normalize_compact_expr(expr: &mut CompactExpr) {
    expr.roots = normalize_compact_span(&mut expr.arena, expr.roots, 0);
}

/// Normalize a span of child nodes. Returns a (possibly new) span.
fn normalize_compact_span(arena: &mut AstArena, span: NodeSpan, depth: usize) -> NodeSpan {
    if depth > super::MAX_AST_DEPTH {
        return span;
    }
    let d = depth + 1;

    // Phase 1: recursively normalize each child node.
    // Collect IDs first to avoid borrow conflicts with arena.
    let child_ids: SmallVec<[NodeId; 8]> = arena.children(span).into();
    for &id in &child_ids {
        normalize_compact_node(arena, id, d);
    }

    // Phase 2: flatten Rows and remove empty Rows.
    // Re-read nodes because Phase 1 may have replaced them in place.
    let mut flattened: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &child_ids {
        match *arena.node(id) {
            CompactNode::Row(inner) if inner.is_empty() => {
                // Remove empty Row
            }
            CompactNode::Row(inner) => {
                // Flatten: inline inner children
                for &inner_id in arena.children(inner) {
                    flattened.push(inner_id);
                }
            }
            _ => flattened.push(id),
        }
    }

    // Phase 3: merge adjacent Text nodes with the same style.
    let mut merged: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &flattened {
        let node = *arena.node(id);
        if let CompactNode::Text(style, sid) = node
            && let Some(&prev_id) = merged.last()
            && let CompactNode::Text(prev_style, prev_sid) = *arena.node(prev_id)
            && prev_style == style
        {
            let prev_s = arena.str_val(prev_sid).to_owned();
            let cur_s = arena.str_val(sid);
            let combined = format!("{prev_s}{cur_s}");
            let new_sid = arena.push_str(&combined);
            *arena.node_mut(prev_id) = CompactNode::Text(style, new_sid);
            continue;
        }
        merged.push(id);
    }

    arena.push_children(&merged)
}

/// Normalize a single compact node. May replace it in-place via `arena.node_mut`.
fn normalize_compact_node(arena: &mut AstArena, id: NodeId, depth: usize) {
    if depth > super::MAX_AST_DEPTH {
        return;
    }
    let d = depth + 1;

    let node = *arena.node(id);
    match node {
        CompactNode::Row(span) => {
            let new_span = normalize_compact_span(arena, span, d);
            let children = arena.children(new_span);
            if children.len() == 1 {
                // Unwrap single-child Row: replace this node with its child
                let child_node = *arena.node(children[0]);
                *arena.node_mut(id) = child_node;
            } else {
                *arena.node_mut(id) = CompactNode::Row(new_span);
            }
        }

        CompactNode::Styled(style, span) => {
            let new_span = normalize_compact_span(arena, span, d);
            *arena.node_mut(id) = CompactNode::Styled(style, new_span);
        }

        CompactNode::Fenced { open, close, content } => {
            // Normalize elements within fenced content (skip Middles)
            let content_ids: SmallVec<[NodeId; 8]> = arena.children(content).into();
            for &cid in &content_ids {
                if !matches!(*arena.node(cid), CompactNode::Middle(_)) {
                    normalize_compact_node(arena, cid, d);
                }
            }
            // Re-read open/close since they haven't changed, but content children may have
            let _ = (open, close); // suppress unused warning
        }

        CompactNode::Subscript { base, sub } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sub, d);
        }
        CompactNode::Superscript { base, sup } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sup, d);
        }
        CompactNode::SubSup { base, sub, sup } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sub, d);
            normalize_compact_node(arena, sup, d);
        }
        CompactNode::PreScripts { base, pre_sub, pre_sup } => {
            normalize_compact_node(arena, base, d);
            if pre_sub.is_some() {
                normalize_compact_node(arena, pre_sub, d);
            }
            if pre_sup.is_some() {
                normalize_compact_node(arena, pre_sup, d);
            }
        }

        CompactNode::Overscript { base, over, .. } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, over, d);
        }
        CompactNode::Underscript { base, under, .. } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, under, d);
        }
        CompactNode::UnderOver { base, under, over, .. } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, under, d);
            normalize_compact_node(arena, over, d);
        }

        CompactNode::Fraction { num, den, .. } => {
            normalize_compact_node(arena, num, d);
            normalize_compact_node(arena, den, d);
        }
        CompactNode::Sqrt(inner) => {
            normalize_compact_node(arena, inner, d);
        }
        CompactNode::Root { index, radicand } => {
            normalize_compact_node(arena, index, d);
            normalize_compact_node(arena, radicand, d);
        }

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                normalize_compact_node(arena, sub, d);
            }
            if sup.is_some() {
                normalize_compact_node(arena, sup, d);
            }
            normalize_compact_node(arena, body, d);
        }

        CompactNode::Accent { base, .. } => {
            normalize_compact_node(arena, base, d);
        }

        CompactNode::Array { cells, .. } => {
            // Each cell is a Row node — normalize it
            let cell_ids: SmallVec<[NodeId; 8]> = arena.children(cells).into();
            for &cell_id in &cell_ids {
                normalize_compact_node(arena, cell_id, d);
            }
        }

        CompactNode::Phantom(inner) | CompactNode::Boxed(inner) => {
            normalize_compact_node(arena, inner, d);
        }
        CompactNode::Cancel { content, .. } | CompactNode::Scaled { content, .. } => {
            normalize_compact_node(arena, content, d);
        }

        // Leaf nodes: nothing to normalize
        CompactNode::Number(_)
        | CompactNode::Identifier(_)
        | CompactNode::Operator { .. }
        | CompactNode::Symbol(_, _)
        | CompactNode::Text(_, _)
        | CompactNode::Space(_)
        | CompactNode::Middle(_)
        | CompactNode::ColAlign(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::compact::CompactExpr;

    #[test]
    fn flatten_nested_rows() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Identifier("a".into()),
            MathNode::Row(vec![
                MathNode::Identifier("b".into()),
                MathNode::Identifier("c".into()),
            ]),
            MathNode::Identifier("d".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 4);
        assert_eq!(expr.nodes[1], MathNode::Identifier("b".into()));
        assert_eq!(expr.nodes[2], MathNode::Identifier("c".into()));
    }

    #[test]
    fn unwrap_single_child_row() {
        let mut expr = MathExpr::inline(vec![MathNode::Row(vec![MathNode::Number("42".into())])]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 1);
        assert_eq!(expr.nodes[0], MathNode::Number("42".into()));
    }

    #[test]
    fn remove_empty_rows() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Row(vec![]),
            MathNode::Identifier("y".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 2);
        assert_eq!(expr.nodes[0], MathNode::Identifier("x".into()));
        assert_eq!(expr.nodes[1], MathNode::Identifier("y".into()));
    }

    #[test]
    fn merge_adjacent_text() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Text(TextStyle::Normal, "hello".into()),
            MathNode::Text(TextStyle::Normal, " world".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 1);
        assert_eq!(
            expr.nodes[0],
            MathNode::Text(TextStyle::Normal, "hello world".into())
        );
    }

    #[test]
    fn no_merge_different_styles() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Text(TextStyle::Normal, "a".into()),
            MathNode::Text(TextStyle::Bold, "b".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 2);
    }

    #[test]
    fn deeply_nested_rows() {
        let mut expr = MathExpr::inline(vec![MathNode::Row(vec![MathNode::Row(vec![
            MathNode::Row(vec![MathNode::Identifier("x".into())]),
        ])])]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 1);
        assert_eq!(expr.nodes[0], MathNode::Identifier("x".into()));
    }

    #[test]
    fn normalize_inside_fraction() {
        let mut expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Row(vec![MathNode::Number("1".into())])),
            den: Box::new(MathNode::Row(vec![MathNode::Number("2".into())])),
        }]);
        normalize_expr(&mut expr);
        if let MathNode::Fraction { num, den, .. } = &expr.nodes[0] {
            assert_eq!(**num, MathNode::Number("1".into()));
            assert_eq!(**den, MathNode::Number("2".into()));
        } else {
            panic!("expected Fraction");
        }
    }

    #[test]
    fn normalize_preserves_non_trivial_structure() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("y".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 3);
    }

    // ── Compact normalize equivalence tests ────────────────────────────

    /// Assert that normalizing via MathExpr and via CompactExpr produce the same result.
    fn assert_normalize_equiv(input: MathExpr) {
        // Path A: normalize MathExpr directly
        let mut path_a = input.clone();
        normalize_expr(&mut path_a);

        // Path B: MathExpr → CompactExpr → normalize → MathExpr
        let mut compact = CompactExpr::from_math_expr(&input);
        normalize_compact_expr(&mut compact);
        let path_b = compact.to_math_expr();

        assert_eq!(path_a, path_b, "normalize equivalence failed");
    }

    #[test]
    fn compact_flatten_nested_rows() {
        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Identifier("a".into()),
            MathNode::Row(vec![
                MathNode::Identifier("b".into()),
                MathNode::Identifier("c".into()),
            ]),
            MathNode::Identifier("d".into()),
        ]));
    }

    #[test]
    fn compact_unwrap_single_child_row() {
        assert_normalize_equiv(MathExpr::inline(vec![MathNode::Row(vec![
            MathNode::Number("42".into()),
        ])]));
    }

    #[test]
    fn compact_remove_empty_rows() {
        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Row(vec![]),
            MathNode::Identifier("y".into()),
        ]));
    }

    #[test]
    fn compact_merge_adjacent_text() {
        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Text(TextStyle::Normal, "hello".into()),
            MathNode::Text(TextStyle::Normal, " world".into()),
        ]));
    }

    #[test]
    fn compact_no_merge_different_styles() {
        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Text(TextStyle::Normal, "a".into()),
            MathNode::Text(TextStyle::Bold, "b".into()),
        ]));
    }

    #[test]
    fn compact_deeply_nested_rows() {
        assert_normalize_equiv(MathExpr::inline(vec![MathNode::Row(vec![
            MathNode::Row(vec![MathNode::Row(vec![MathNode::Identifier("x".into())])]),
        ])]));
    }

    #[test]
    fn compact_normalize_inside_fraction() {
        assert_normalize_equiv(MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Row(vec![MathNode::Number("1".into())])),
            den: Box::new(MathNode::Row(vec![MathNode::Number("2".into())])),
        }]));
    }

    #[test]
    fn compact_normalize_preserves_structure() {
        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("y".into()),
        ]));
    }

    #[test]
    fn compact_normalize_complex_expression() {
        // Nested expression with multiple normalizable patterns
        assert_normalize_equiv(MathExpr::block(vec![
            MathNode::Row(vec![MathNode::Identifier("x".into())]), // unwrap
            MathNode::Symbol(SymbolKind::Relation, "=".into()),
            MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(MathNode::Row(vec![
                    // flatten inner Row + merge Text
                    MathNode::Text(TextStyle::Normal, "a".into()),
                    MathNode::Text(TextStyle::Normal, "b".into()),
                    MathNode::Row(vec![
                        MathNode::Identifier("c".into()),
                        MathNode::Row(vec![]), // remove empty
                    ]),
                ])),
                den: Box::new(MathNode::Row(vec![MathNode::Number("2".into())])), // unwrap
            },
        ]));
    }
}
