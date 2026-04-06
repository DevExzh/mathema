use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::*;

/// Immutable visitor for AST traversal.
///
/// Default implementations recurse into child nodes. Override specific
/// methods to inspect nodes of interest.
pub trait Visitor {
    fn visit_expr(&mut self, expr: &MathExpr) {
        walk_expr(self, expr);
    }

    fn visit_node(&mut self, node: &MathNode) {
        walk_node(self, node);
    }
}

/// Walk the children of a `MathExpr`.
pub fn walk_expr<V: Visitor + ?Sized>(v: &mut V, expr: &MathExpr) {
    for node in &expr.nodes {
        v.visit_node(node);
    }
}

/// Walk the children of a `MathNode`.
pub fn walk_node<V: Visitor + ?Sized>(v: &mut V, node: &MathNode) {
    match node {
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_) => {},

        MathNode::Row(children) => {
            for child in children {
                v.visit_node(child);
            }
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let FencedItem::Element(node) = item {
                    v.visit_node(node);
                }
            }
        },

        MathNode::Subscript { base, sub } => {
            v.visit_node(base);
            v.visit_node(sub);
        },
        MathNode::Superscript { base, sup } => {
            v.visit_node(base);
            v.visit_node(sup);
        },
        MathNode::SubSup { base, sub, sup } => {
            v.visit_node(base);
            v.visit_node(sub);
            v.visit_node(sup);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            v.visit_node(base);
            if let Some(s) = pre_sub {
                v.visit_node(s);
            }
            if let Some(s) = pre_sup {
                v.visit_node(s);
            }
        },

        MathNode::Overscript { base, over, .. } => {
            v.visit_node(base);
            v.visit_node(over);
        },
        MathNode::Underscript { base, under, .. } => {
            v.visit_node(base);
            v.visit_node(under);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            v.visit_node(base);
            v.visit_node(under);
            v.visit_node(over);
        },

        MathNode::Fraction { num, den, .. } => {
            v.visit_node(num);
            v.visit_node(den);
        },
        MathNode::Sqrt(inner) => {
            v.visit_node(inner);
        },
        MathNode::Root { index, radicand } => {
            v.visit_node(index);
            v.visit_node(radicand);
        },

        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                v.visit_node(s);
            }
            if let Some(s) = sup {
                v.visit_node(s);
            }
            v.visit_node(body);
        },

        MathNode::Accent { base, .. } => {
            v.visit_node(base);
        },

        MathNode::Array { rows, .. } => {
            for row in rows {
                for cell in row {
                    for node in cell {
                        v.visit_node(node);
                    }
                }
            }
        },

        MathNode::Phantom(inner) | MathNode::Boxed(inner) => {
            v.visit_node(inner);
        },
        MathNode::Cancel { content, .. } => {
            v.visit_node(content);
        },
        MathNode::Scaled { content, .. } => {
            v.visit_node(content);
        },
        MathNode::Styled(_, children) => {
            for child in children {
                v.visit_node(child);
            }
        },
    }
}

/// Mutable visitor for AST transformation.
///
/// Each method receives a mutable reference and can modify nodes in place.
pub trait MutVisitor {
    fn visit_expr_mut(&mut self, expr: &mut MathExpr) {
        walk_expr_mut(self, expr);
    }

    fn visit_node_mut(&mut self, node: &mut MathNode) {
        walk_node_mut(self, node);
    }
}

/// Walk and mutably visit children of a `MathExpr`.
pub fn walk_expr_mut<V: MutVisitor + ?Sized>(v: &mut V, expr: &mut MathExpr) {
    for node in &mut expr.nodes {
        v.visit_node_mut(node);
    }
}

/// Walk and mutably visit children of a `MathNode`.
pub fn walk_node_mut<V: MutVisitor + ?Sized>(v: &mut V, node: &mut MathNode) {
    match node {
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_) => {},

        MathNode::Row(children) => {
            for child in children {
                v.visit_node_mut(child);
            }
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let FencedItem::Element(node) = item {
                    v.visit_node_mut(node);
                }
            }
        },

        MathNode::Subscript { base, sub } => {
            v.visit_node_mut(base);
            v.visit_node_mut(sub);
        },
        MathNode::Superscript { base, sup } => {
            v.visit_node_mut(base);
            v.visit_node_mut(sup);
        },
        MathNode::SubSup { base, sub, sup } => {
            v.visit_node_mut(base);
            v.visit_node_mut(sub);
            v.visit_node_mut(sup);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            v.visit_node_mut(base);
            if let Some(s) = pre_sub {
                v.visit_node_mut(s);
            }
            if let Some(s) = pre_sup {
                v.visit_node_mut(s);
            }
        },

        MathNode::Overscript { base, over, .. } => {
            v.visit_node_mut(base);
            v.visit_node_mut(over);
        },
        MathNode::Underscript { base, under, .. } => {
            v.visit_node_mut(base);
            v.visit_node_mut(under);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            v.visit_node_mut(base);
            v.visit_node_mut(under);
            v.visit_node_mut(over);
        },

        MathNode::Fraction { num, den, .. } => {
            v.visit_node_mut(num);
            v.visit_node_mut(den);
        },
        MathNode::Sqrt(inner) => {
            v.visit_node_mut(inner);
        },
        MathNode::Root { index, radicand } => {
            v.visit_node_mut(index);
            v.visit_node_mut(radicand);
        },

        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                v.visit_node_mut(s);
            }
            if let Some(s) = sup {
                v.visit_node_mut(s);
            }
            v.visit_node_mut(body);
        },

        MathNode::Accent { base, .. } => {
            v.visit_node_mut(base);
        },

        MathNode::Array { rows, .. } => {
            for row in rows {
                for cell in row {
                    for node in cell {
                        v.visit_node_mut(node);
                    }
                }
            }
        },

        MathNode::Phantom(inner) | MathNode::Boxed(inner) => {
            v.visit_node_mut(inner);
        },
        MathNode::Cancel { content, .. } => {
            v.visit_node_mut(content);
        },
        MathNode::Scaled { content, .. } => {
            v.visit_node_mut(content);
        },
        MathNode::Styled(_, children) => {
            for child in children {
                v.visit_node_mut(child);
            }
        },
    }
}

// ── Compact AST visitors ───────────────────────────────────────────────

/// Immutable visitor for compact arena-based AST traversal.
pub trait CompactVisitor {
    fn visit_expr(&mut self, expr: &CompactExpr) {
        walk_compact_expr(self, expr);
    }

    fn visit_node(&mut self, arena: &AstArena, id: NodeId) {
        walk_compact_node(self, arena, id);
    }
}

pub fn walk_compact_expr<V: CompactVisitor + ?Sized>(v: &mut V, expr: &CompactExpr) {
    for &id in expr.arena.children(expr.roots) {
        v.visit_node(&expr.arena, id);
    }
}

pub fn walk_compact_node<V: CompactVisitor + ?Sized>(v: &mut V, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Number(_)
        | CompactNode::Identifier(_)
        | CompactNode::Operator { .. }
        | CompactNode::Symbol(_, _)
        | CompactNode::Text(_, _)
        | CompactNode::Space(_)
        | CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_) => {},

        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            for &child in arena.children(span) {
                v.visit_node(arena, child);
            }
        },
        CompactNode::Fenced { content, .. } => {
            for &child in arena.children(content) {
                if !matches!(*arena.node(child), CompactNode::Middle(_)) {
                    v.visit_node(arena, child);
                }
            }
        },

        CompactNode::Subscript { base, sub } => {
            v.visit_node(arena, base);
            v.visit_node(arena, sub);
        },
        CompactNode::Superscript { base, sup } => {
            v.visit_node(arena, base);
            v.visit_node(arena, sup);
        },
        CompactNode::SubSup { base, sub, sup } => {
            v.visit_node(arena, base);
            v.visit_node(arena, sub);
            v.visit_node(arena, sup);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            v.visit_node(arena, base);
            if pre_sub.is_some() {
                v.visit_node(arena, pre_sub);
            }
            if pre_sup.is_some() {
                v.visit_node(arena, pre_sup);
            }
        },

        CompactNode::Overscript { base, over, .. } => {
            v.visit_node(arena, base);
            v.visit_node(arena, over);
        },
        CompactNode::Underscript { base, under, .. } => {
            v.visit_node(arena, base);
            v.visit_node(arena, under);
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            v.visit_node(arena, base);
            v.visit_node(arena, under);
            v.visit_node(arena, over);
        },

        CompactNode::Fraction { num, den, .. } => {
            v.visit_node(arena, num);
            v.visit_node(arena, den);
        },
        CompactNode::Sqrt(inner) => {
            v.visit_node(arena, inner);
        },
        CompactNode::Root { index, radicand } => {
            v.visit_node(arena, index);
            v.visit_node(arena, radicand);
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                v.visit_node(arena, sub);
            }
            if sup.is_some() {
                v.visit_node(arena, sup);
            }
            v.visit_node(arena, body);
        },

        CompactNode::Accent { base, .. } => {
            v.visit_node(arena, base);
        },

        CompactNode::Array { cells, .. } => {
            for &cell_id in arena.children(cells) {
                v.visit_node(arena, cell_id);
            }
        },

        CompactNode::Phantom(inner) | CompactNode::Boxed(inner) => {
            v.visit_node(arena, inner);
        },
        CompactNode::Cancel { content, .. } | CompactNode::Scaled { content, .. } => {
            v.visit_node(arena, content);
        },
        CompactNode::Color { content, .. } => {
            v.visit_node(arena, content);
        },
    }
}

/// Mutable visitor for compact arena-based AST transformation.
///
/// Because `CompactNode` is `Copy`, the mutable visitor copies the node,
/// recurses into children, and can replace nodes via `arena.node_mut(id)`.
pub trait CompactMutVisitor {
    fn visit_node_mut(&mut self, arena: &mut AstArena, id: NodeId) {
        walk_compact_node_mut(self, arena, id);
    }
}

pub fn walk_compact_node_mut<V: CompactMutVisitor + ?Sized>(
    v: &mut V,
    arena: &mut AstArena,
    id: NodeId,
) {
    let node = *arena.node(id); // Copy — cheap, avoids borrow conflict
    match node {
        CompactNode::Number(_)
        | CompactNode::Identifier(_)
        | CompactNode::Operator { .. }
        | CompactNode::Symbol(_, _)
        | CompactNode::Text(_, _)
        | CompactNode::Space(_)
        | CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_) => {},

        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            let ids: smallvec::SmallVec<[NodeId; 8]> = arena.children(span).into();
            for &child in &ids {
                v.visit_node_mut(arena, child);
            }
        },
        CompactNode::Fenced { content, .. } => {
            let ids: smallvec::SmallVec<[NodeId; 8]> = arena.children(content).into();
            for &child in &ids {
                if !matches!(*arena.node(child), CompactNode::Middle(_)) {
                    v.visit_node_mut(arena, child);
                }
            }
        },

        CompactNode::Subscript { base, sub } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, sub);
        },
        CompactNode::Superscript { base, sup } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, sup);
        },
        CompactNode::SubSup { base, sub, sup } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, sub);
            v.visit_node_mut(arena, sup);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            v.visit_node_mut(arena, base);
            if pre_sub.is_some() {
                v.visit_node_mut(arena, pre_sub);
            }
            if pre_sup.is_some() {
                v.visit_node_mut(arena, pre_sup);
            }
        },

        CompactNode::Overscript { base, over, .. } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, over);
        },
        CompactNode::Underscript { base, under, .. } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, under);
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            v.visit_node_mut(arena, base);
            v.visit_node_mut(arena, under);
            v.visit_node_mut(arena, over);
        },

        CompactNode::Fraction { num, den, .. } => {
            v.visit_node_mut(arena, num);
            v.visit_node_mut(arena, den);
        },
        CompactNode::Sqrt(inner) => {
            v.visit_node_mut(arena, inner);
        },
        CompactNode::Root { index, radicand } => {
            v.visit_node_mut(arena, index);
            v.visit_node_mut(arena, radicand);
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                v.visit_node_mut(arena, sub);
            }
            if sup.is_some() {
                v.visit_node_mut(arena, sup);
            }
            v.visit_node_mut(arena, body);
        },

        CompactNode::Accent { base, .. } => {
            v.visit_node_mut(arena, base);
        },

        CompactNode::Array { cells, .. } => {
            let ids: smallvec::SmallVec<[NodeId; 8]> = arena.children(cells).into();
            for &cell_id in &ids {
                v.visit_node_mut(arena, cell_id);
            }
        },

        CompactNode::Phantom(inner) | CompactNode::Boxed(inner) => {
            v.visit_node_mut(arena, inner);
        },
        CompactNode::Cancel { content, .. } | CompactNode::Scaled { content, .. } => {
            v.visit_node_mut(arena, content);
        },
        CompactNode::Color { content, .. } => {
            v.visit_node_mut(arena, content);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NodeCounter {
        count: usize,
    }

    impl Visitor for NodeCounter {
        fn visit_node(&mut self, node: &MathNode) {
            self.count += 1;
            walk_node(self, node);
        }
    }

    #[test]
    fn count_nodes_simple() {
        let expr = MathExpr::inline(vec![
            MathNode::Number("1".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("2".into()),
        ]);
        let mut counter = NodeCounter { count: 0 };
        counter.visit_expr(&expr);
        assert_eq!(counter.count, 3);
    }

    #[test]
    fn count_nodes_nested() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Identifier("a".into())),
            den: Box::new(MathNode::Row(vec![
                MathNode::Identifier("b".into()),
                MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                MathNode::Identifier("c".into()),
            ])),
        }]);
        let mut counter = NodeCounter { count: 0 };
        counter.visit_expr(&expr);
        // fraction(1) + a(1) + row(1) + b(1) + +(1) + c(1) = 6
        assert_eq!(counter.count, 6);
    }

    #[test]
    fn mut_visitor_transforms() {
        struct DoubleNumbers;
        impl MutVisitor for DoubleNumbers {
            fn visit_node_mut(&mut self, node: &mut MathNode) {
                if let MathNode::Number(s) = node
                    && let Ok(n) = s.parse::<i64>()
                {
                    *s = (n * 2).to_string();
                }
                walk_node_mut(self, node);
            }
        }

        let mut expr = MathExpr::inline(vec![
            MathNode::Number("3".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("5".into()),
        ]);
        DoubleNumbers.visit_expr_mut(&mut expr);
        assert_eq!(expr.nodes[0], MathNode::Number("6".into()));
        assert_eq!(expr.nodes[2], MathNode::Number("10".into()));
    }

    // ── Compact visitor tests ──────────────────────────────────────────

    struct CompactNodeCounter {
        count: usize,
    }

    impl CompactVisitor for CompactNodeCounter {
        fn visit_node(&mut self, arena: &AstArena, id: NodeId) {
            self.count += 1;
            walk_compact_node(self, arena, id);
        }
    }

    #[test]
    fn compact_count_nodes_simple() {
        let expr = MathExpr::inline(vec![
            MathNode::Number("1".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("2".into()),
        ]);
        let compact = CompactExpr::from_math_expr(&expr);
        let mut counter = CompactNodeCounter { count: 0 };
        counter.visit_expr(&compact);
        assert_eq!(counter.count, 3);
    }

    #[test]
    fn compact_count_nodes_nested() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Identifier("a".into())),
            den: Box::new(MathNode::Row(vec![
                MathNode::Identifier("b".into()),
                MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                MathNode::Identifier("c".into()),
            ])),
        }]);
        let compact = CompactExpr::from_math_expr(&expr);
        let mut counter = CompactNodeCounter { count: 0 };
        counter.visit_expr(&compact);
        // fraction(1) + a(1) + row(1) + b(1) + +(1) + c(1) = 6
        assert_eq!(counter.count, 6);
    }

    #[test]
    fn compact_mut_visitor_transforms() {
        struct DoubleCompactNumbers;
        impl CompactMutVisitor for DoubleCompactNumbers {
            fn visit_node_mut(&mut self, arena: &mut AstArena, id: NodeId) {
                if let CompactNode::Number(sid) = *arena.node(id) {
                    let s = arena.str_val(sid).to_owned();
                    if let Ok(n) = s.parse::<i64>() {
                        let doubled = (n * 2).to_string();
                        let new_sid = arena.push_str(&doubled);
                        *arena.node_mut(id) = CompactNode::Number(new_sid);
                    }
                }
                walk_compact_node_mut(self, arena, id);
            }
        }

        let expr = MathExpr::inline(vec![
            MathNode::Number("3".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("5".into()),
        ]);
        let mut compact = CompactExpr::from_math_expr(&expr);
        let root_ids: smallvec::SmallVec<[NodeId; 8]> =
            compact.arena.children(compact.roots).into();
        for &id in &root_ids {
            DoubleCompactNumbers.visit_node_mut(&mut compact.arena, id);
        }
        let restored = compact.to_math_expr();
        assert_eq!(restored.nodes[0], MathNode::Number("6".into()));
        assert_eq!(restored.nodes[2], MathNode::Number("10".into()));
    }
}
