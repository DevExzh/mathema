//! Colors in LaTeX output. Text that is no color (see [`crate::ast::color`]) is not
//! written; a node that is only that color gives way to its content before
//! anything is written, so the writer sees the tree it will read back.

use super::LossReport;
use crate::ast::MathAtomType;
use crate::ast::arena::AstArena;
use crate::ast::color::{is_color, is_text_color};
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::normalize::unwritable_color_content;

/// The argument of `\textcolor` for a color, with its optional model.
pub(super) fn text_color(spec: &str) -> Option<(&'static str, &str)> {
    if !is_text_color(spec) {
        None
    } else if let Some(hex) = spec.strip_prefix('#') {
        Some(("[HTML]", hex))
    } else {
        Some(("", spec))
    }
}

/// The argument of `\colorbox`/`\fcolorbox` for a color, `""` for none. A
/// `#hex` color is written as it is: TeX needs the `HTML` model for it, which
/// KaTeX does not accept, and KaTeX is where such colors come from.
pub(super) fn box_color(spec: &str) -> &str {
    if is_color(spec) { spec } else { "" }
}

/// Whether the node is written as its content alone.
fn is_dropped(arena: &AstArena, node: &CompactNode) -> bool {
    unwritable_color_content(arena, node).is_some()
}

/// `expr` without the nodes whose color cannot be written, or `None` when it
/// has none.
pub(super) fn without_dropped_colors(
    expr: &CompactExpr,
    report: &mut LossReport,
) -> Option<CompactExpr> {
    let nodes = expr.arena.nodes();
    let dropped = |node: &CompactNode| {
        matches!(node, CompactNode::Color { .. } | CompactNode::Boxed { .. })
            && is_dropped(&expr.arena, node)
    };
    if !nodes.iter().any(dropped) {
        return None;
    }
    if report.is_collecting() {
        report_dropped(expr, report);
    }
    let mut expr = expr.clone();
    let arena = &mut expr.arena;
    expr.roots = splice(arena, expr.roots);
    for index in 0..arena.node_count() as u32 {
        let id = NodeId::new(index);
        match *arena.node(id) {
            CompactNode::Row(span) => {
                let span = splice(arena, span);
                *arena.node_mut(id) = CompactNode::Row(span);
            },
            CompactNode::Styled(style, span) => {
                let span = splice(arena, span);
                *arena.node_mut(id) = CompactNode::Styled(style, span);
            },
            CompactNode::Fenced {
                open,
                close,
                content,
            } => {
                let content = splice(arena, content);
                *arena.node_mut(id) = CompactNode::Fenced {
                    open,
                    close,
                    content,
                };
            },
            // The group of a color is an ordinary atom under a script.
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. }
            | CompactNode::PreScripts { base, .. }
                if is_dropped(arena, arena.node(base)) =>
            {
                let mut content = base;
                while let Some(inner) = unwritable_color_content(arena, arena.node(content)) {
                    content = inner;
                }
                if matches!(
                    *arena.node(content),
                    CompactNode::Operator { .. }
                        | CompactNode::StructuredOperator { .. }
                        | CompactNode::MathAtom {
                            atom_type: MathAtomType::Op,
                            ..
                        }
                ) {
                    *arena.node_mut(base) = CompactNode::MathAtom {
                        atom_type: MathAtomType::Ord,
                        content,
                    };
                }
            },
            _ => {},
        }
    }
    // What is left is the only content of some argument.
    for index in 0..arena.node_count() as u32 {
        let id = NodeId::new(index);
        let mut content = id;
        while let Some(inner) = unwritable_color_content(arena, arena.node(content)) {
            content = inner;
        }
        if content == id {
            continue;
        }
        *arena.node_mut(id) = match *arena.node(content) {
            row @ CompactNode::Row(_) => row,
            _ => CompactNode::Row(arena.push_children_deferred(&[content])),
        };
    }
    Some(expr)
}

/// Report the colors dropped from the expression (the arena may also hold
/// nodes that are not part of it).
fn report_dropped(expr: &CompactExpr, report: &mut LossReport) {
    let arena = &expr.arena;
    let mut stack = arena.children(expr.roots).to_vec();
    while let Some(id) = stack.pop() {
        let node = arena.node(id);
        if let CompactNode::Color { color, .. }
        | CompactNode::Boxed {
            bg_color: color, ..
        } = *node
            && is_dropped(arena, node)
        {
            report.color(arena.str_val(color));
        }
        stack.extend(crate::ast::compact_children(arena, node));
    }
}

/// `span` with the content of each dropped color in its place. Content that
/// holds a declaration keeps its group.
fn splice(arena: &mut AstArena, span: NodeSpan) -> NodeSpan {
    if !arena
        .children(span)
        .iter()
        .any(|&id| is_dropped(arena, arena.node(id)))
    {
        return span;
    }
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = arena.children(span).iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let Some(content) = unwritable_color_content(arena, arena.node(id)) else {
            out.push(id);
            continue;
        };
        match *arena.node(content) {
            CompactNode::Row(inner) if !super::row_contains_style_switch(arena, inner, 0) => {
                stack.extend(arena.children(inner).iter().rev());
            },
            _ => stack.push(content),
        }
    }
    arena.push_children_deferred(&out)
}
