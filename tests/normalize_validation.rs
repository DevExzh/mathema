//! Public compact normalization validates caller-owned arena references first.

use mathema::ast::compact::{CompactExpr, CompactNode, NodeId};
use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::{DisplayStyle, MathExpr, MathNode, SymbolKind};
use mathema::{AstArena, Error};

#[test]
fn malformed_compact_tree_is_refused_without_mutation() {
    let mut arena = AstArena::new();
    let root = arena.push_node(CompactNode::Sqrt(NodeId::NONE));
    let roots = arena.push_children(&[root]).unwrap();
    let mut expr = CompactExpr::new(arena, roots, DisplayStyle::Inline);
    let before_roots = expr.roots;
    let before_children = expr.arena.children(roots).to_vec();
    let before_root = *expr.arena.node(root);

    let result = normalize_compact_expr(&mut expr);

    assert!(matches!(
        result,
        Err(Error::Unsupported(message)) if message.contains("invalid compact AST")
    ));
    assert_eq!(expr.roots, before_roots);
    assert_eq!(expr.arena.children(roots), before_children.as_slice());
    assert_eq!(*expr.arena.node(root), before_root);
}

#[test]
fn valid_compact_tree_keeps_normalization_semantics() {
    let source = MathExpr::inline(vec![
        MathNode::Symbol(SymbolKind::Ordinary, "a".into()),
        MathNode::Row(vec![
            MathNode::Symbol(SymbolKind::Ordinary, "b".into()),
            MathNode::Symbol(SymbolKind::Ordinary, "c".into()),
        ]),
    ]);
    let mut compact = CompactExpr::from_math_expr(&source).unwrap();

    normalize_compact_expr(&mut compact).unwrap();

    assert_eq!(
        compact.to_math_expr().nodes,
        vec![
            MathNode::Symbol(SymbolKind::Ordinary, "a".into()),
            MathNode::Symbol(SymbolKind::Ordinary, "b".into()),
            MathNode::Symbol(SymbolKind::Ordinary, "c".into()),
        ]
    );
}
