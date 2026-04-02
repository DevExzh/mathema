use crate::ast::*;

/// Normalize a math expression in place.
///
/// Applies the following simplifications bottom-up:
/// 1. Flatten nested Rows: `Row([a, Row([b, c]), d])` → `Row([a, b, c, d])`
/// 2. Unwrap single-child Rows: `Row([x])` → `x`
/// 3. Merge adjacent Text nodes with the same style
/// 4. Remove empty Rows from node lists
pub fn normalize_expr(expr: &mut MathExpr) {
    normalize_nodes(&mut expr.nodes);
}

fn normalize_nodes(nodes: &mut Vec<MathNode>) {
    // Phase 1: recursively normalize each child
    for node in nodes.iter_mut() {
        normalize_node_recursive(node);
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

fn normalize_node_recursive(node: &mut MathNode) {
    match node {
        MathNode::Row(children) => {
            normalize_nodes(children);
            // Unwrap single-child Row
            if children.len() == 1 {
                *node = children.pop().unwrap();
            }
        },

        MathNode::Fenced { content, .. } => {
            for item in content.iter_mut() {
                if let FencedItem::Element(n) = item {
                    normalize_node_recursive(n);
                }
            }
        },

        MathNode::Subscript { base, sub } => {
            normalize_node_recursive(base);
            normalize_node_recursive(sub);
        },
        MathNode::Superscript { base, sup } => {
            normalize_node_recursive(base);
            normalize_node_recursive(sup);
        },
        MathNode::SubSup { base, sub, sup } => {
            normalize_node_recursive(base);
            normalize_node_recursive(sub);
            normalize_node_recursive(sup);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_node_recursive(base);
            if let Some(s) = pre_sub {
                normalize_node_recursive(s);
            }
            if let Some(s) = pre_sup {
                normalize_node_recursive(s);
            }
        },

        MathNode::Overscript { base, over, .. } => {
            normalize_node_recursive(base);
            normalize_node_recursive(over);
        },
        MathNode::Underscript { base, under, .. } => {
            normalize_node_recursive(base);
            normalize_node_recursive(under);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_node_recursive(base);
            normalize_node_recursive(under);
            normalize_node_recursive(over);
        },

        MathNode::Fraction { num, den, .. } => {
            normalize_node_recursive(num);
            normalize_node_recursive(den);
        },
        MathNode::Sqrt(inner) => {
            normalize_node_recursive(inner);
        },
        MathNode::Root { index, radicand } => {
            normalize_node_recursive(index);
            normalize_node_recursive(radicand);
        },

        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                normalize_node_recursive(s);
            }
            if let Some(s) = sup {
                normalize_node_recursive(s);
            }
            normalize_node_recursive(body);
        },

        MathNode::Accent { base, .. } => {
            normalize_node_recursive(base);
        },

        MathNode::Array { rows, .. } => {
            for row in rows.iter_mut() {
                for cell in row.iter_mut() {
                    normalize_nodes(cell);
                }
            }
        },

        MathNode::Phantom(inner) | MathNode::Boxed(inner) => {
            normalize_node_recursive(inner);
        },
        MathNode::Cancel { content, .. } => {
            normalize_node_recursive(content);
        },
        MathNode::Scaled { content, .. } => {
            normalize_node_recursive(content);
        },
        MathNode::Styled(_, children) => {
            normalize_nodes(children);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
