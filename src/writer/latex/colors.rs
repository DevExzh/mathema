//! Colors in LaTeX output. Text that is no color (see [`super::color_spec`])
//! is not written, and the node it colors gives way to its content.
//!
//! An expression that was just read is rewritten by [`to_groups`]: the node
//! becomes the group its content makes, which is what LaTeX without the color
//! reads as. That is done before the expression is normalized, if it is.
//! Nothing is known about an expression handed to the writer directly: a
//! group in a normalized one would be flattened on the next read, and joining
//! its content to the neighbours could leave it unnormalized. So
//! [`with_ordinary_atoms`] writes an explicit ordinary atom, the atom a color's
//! group is in TeX.

use super::LossReport;
use super::color_spec::{is_color, is_text_color};
use crate::ast::MathAtomType;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};

/// The argument of `\textcolor` for a color, with its model. Other text does
/// not get here; it must not be written whatever the caller did.
pub(super) fn text_color(spec: &str) -> (&'static str, &str) {
    match spec.strip_prefix('#') {
        _ if !is_text_color(spec) => ("", ""),
        Some(hex) => ("[HTML]", hex),
        None => ("", spec),
    }
}

/// The argument of `\colorbox`/`\fcolorbox` for a color, `""` for none. A
/// `#hex` color is written as it is: TeX needs the `HTML` model for it, which
/// KaTeX does not accept, and KaTeX is where such colors come from.
pub(super) fn box_color(spec: &str) -> &str {
    if is_color(spec) { spec } else { "" }
}

/// The content of a node that is written as its content alone: a color, or a
/// box that is only that background.
fn uncolored_content(arena: &AstArena, node: &CompactNode) -> Option<NodeId> {
    match *node {
        CompactNode::Color { color, content } => {
            (!is_text_color(arena.str_val(color))).then_some(content)
        },
        CompactNode::Boxed {
            content,
            bg_color,
            border_color,
        } => {
            let background = arena.str_val(bg_color);
            (!background.is_empty()
                && !is_color(background)
                && arena.str_val(border_color).is_empty())
            .then_some(content)
        },
        _ => None,
    }
}

/// Whether the expression has a node whose color is not written.
fn has_uncolored(expr: &CompactExpr) -> bool {
    // Most expressions have no color at all.
    expr.arena.nodes().iter().any(|node| {
        matches!(node, CompactNode::Color { .. } | CompactNode::Boxed { .. })
            && uncolored_content(&expr.arena, node).is_some()
    })
}

/// Rewrite the nodes whose color is not written as the groups of their
/// contents, as the reader makes them.
pub(crate) fn to_groups(expr: &mut CompactExpr) {
    if !has_uncolored(expr) {
        return;
    }
    let arena = &mut expr.arena;
    settle_attrs(arena);
    let nodes = arena.node_count() as u32;
    for index in 0..nodes {
        match *arena.node(NodeId::new(index)) {
            // A cell is a list of its own: a line break in a group that is the
            // whole cell would end the row.
            CompactNode::Array { cells, .. } => {
                for position in 0..arena.children(cells).len() {
                    let cell = arena.children(cells)[position];
                    let cell = match *arena.node(cell) {
                        CompactNode::AlignedCell { content, .. }
                        | CompactNode::SpannedCell { content, .. } => content,
                        _ => cell,
                    };
                    if let Some(content) = uncolored_content(arena, arena.node(cell)) {
                        *arena.node_mut_in_place(cell) = ordinary(content);
                    }
                }
            },
            // Under a script, the group of an operator is an ordinary atom.
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. } => {
                if let Some(content) = uncolored_content(arena, arena.node(base))
                    && matches!(
                        *arena.node(content),
                        CompactNode::Operator { .. }
                            | CompactNode::StructuredOperator { .. }
                            | CompactNode::MathAtom {
                                atom_type: MathAtomType::Op,
                                ..
                            }
                    )
                {
                    *arena.node_mut_in_place(base) = ordinary(content);
                }
            },
            _ => {},
        }
    }
    for index in 0..nodes {
        group(arena, NodeId::new(index));
    }
}

/// Give what uncolored nodes hold their typesetting attributes, which the
/// nodes lose with their place. The inner attributes win.
fn settle_attrs(arena: &mut AstArena) {
    // A color in a row in a color takes as many passes as they nest.
    while arena.has_attrs() {
        let mut held: Vec<(usize, NodeId)> = (0..arena.node_count() as u32)
            .map(NodeId::new)
            .filter(|&id| {
                arena.attrs(id).is_some() && uncolored_content(arena, arena.node(id)).is_some()
            })
            .map(|id| {
                let mut layers = 0;
                let mut inner = id;
                while let Some(content) = uncolored_content(arena, arena.node(inner)) {
                    inner = content;
                    layers += 1;
                }
                (layers, id)
            })
            .collect();
        if held.is_empty() {
            return;
        }
        held.sort_by_key(|&(layers, _)| layers);
        for (_, id) in held {
            if let (Some(content), Some(attrs)) = (innermost(arena, id), arena.attrs(id).cloned()) {
                arena.remove_attrs(id, &attrs);
                arena.add_attrs(content, &attrs);
            }
        }
    }
}

/// The content under every uncolored node that starts at `id`.
fn innermost(arena: &AstArena, id: NodeId) -> Option<NodeId> {
    let mut content = uncolored_content(arena, arena.node(id))?;
    while let Some(inner) = uncolored_content(arena, arena.node(content)) {
        content = inner;
    }
    Some(content)
}

/// Make the node at `id`, if its color is not written, the group of its
/// content.
fn group(arena: &mut AstArena, id: NodeId) {
    let Some(mut content) = innermost(arena, id) else {
        return;
    };
    if let CompactNode::Row(span) = *arena.node(content) {
        let mut children = arena
            .children(span)
            .iter()
            .filter(|&&child| !super::is_empty_row(arena, child));
        if let (Some(&only), None) = (children.next(), children.next()) {
            content = innermost(arena, only).unwrap_or(only);
        }
    }
    match *arena.node(content) {
        // Alone, it would apply to what follows.
        CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_) | CompactNode::Newline => {
            *arena.node_mut_in_place(id) = ordinary(content);
        },
        _ => arena.replace_from(id, content),
    }
}

/// A copy of `expr` rewritten by [`to_groups`], or `None` when that changes
/// nothing.
pub(super) fn grouped(expr: &CompactExpr) -> Option<CompactExpr> {
    if !has_uncolored(expr) {
        return None;
    }
    let mut expr = expr.clone();
    to_groups(&mut expr);
    Some(expr)
}

/// `expr` with an ordinary atom in the place of each node whose color is not
/// written, or `None` when it has none. One that is all of its list or
/// argument needs no atom.
pub(super) fn with_ordinary_atoms(expr: &CompactExpr) -> Option<CompactExpr> {
    if !has_uncolored(expr) {
        return None;
    }
    let mut expr = expr.clone();
    let roots = expr.roots;
    let arena = &mut expr.arena;
    settle_attrs(arena);
    let nodes = arena.node_count() as u32;
    // Whether a node has neighbours its content could meet.
    let mut beside = vec![false; nodes as usize];
    let mut mark = |ids: &[NodeId], list: bool| {
        // A single node can stand for all of a list, a row of nodes cannot.
        let alone = list
            && matches!(*ids, [only] if innermost(arena, only)
                .is_none_or(|content| !matches!(*arena.node(content), CompactNode::Row(_))));
        if !alone {
            ids.iter().for_each(|id| beside[id.index()] = true);
        }
    };
    mark(arena.children(roots), true);
    for node in arena.nodes() {
        match *node {
            CompactNode::Row(span) | CompactNode::Styled(_, span) => {
                mark(arena.children(span), true)
            },
            CompactNode::Fenced { content, .. } => mark(arena.children(content), true),
            // A script makes its base one atom; a cell is a list of its own.
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. }
            | CompactNode::PreScripts { base, .. }
            | CompactNode::Nary { body: base, .. } => mark(&[base], false),
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                mark(&[content], false);
            },
            CompactNode::Array { cells, .. } => {
                mark(arena.children(cells), false);
            },
            _ => {},
        }
    }
    // All of a list: its content is the list.
    expr.roots = own_list(arena, roots);
    for index in 0..nodes {
        let id = NodeId::new(index);
        match *arena.node(id) {
            CompactNode::Row(span) => {
                *arena.node_mut_in_place(id) = CompactNode::Row(own_list(arena, span))
            },
            CompactNode::Styled(style, span) => {
                *arena.node_mut_in_place(id) = CompactNode::Styled(style, own_list(arena, span));
            },
            CompactNode::Fenced {
                open,
                close,
                content,
            } => {
                *arena.node_mut_in_place(id) = CompactNode::Fenced {
                    open,
                    close,
                    content: own_list(arena, content),
                };
            },
            _ => {},
        }
    }
    for index in 0..nodes {
        let id = NodeId::new(index);
        if !beside[id.index()] {
            group(arena, id);
        } else if let Some(content) = innermost(arena, id) {
            *arena.node_mut_in_place(id) = ordinary(content);
        }
    }
    Some(expr)
}

/// The list inside the one uncolored node of `span`, if it holds no line
/// break (which would end the row of a table cell).
fn own_list(arena: &AstArena, span: NodeSpan) -> NodeSpan {
    if let [only] = *arena.children(span)
        && let Some(content) = innermost(arena, only)
        && let CompactNode::Row(list) = *arena.node(content)
        && !arena
            .children(list)
            .iter()
            .any(|&id| matches!(*arena.node(id), CompactNode::Newline))
    {
        list
    } else {
        span
    }
}

fn ordinary(content: NodeId) -> CompactNode {
    CompactNode::MathAtom {
        atom_type: MathAtomType::Ord,
        content,
    }
}

/// Report the colors dropped from the expression (the arena may also hold
/// nodes that are not part of it).
pub(super) fn report_dropped(expr: &CompactExpr, report: &mut LossReport) {
    if !report.is_collecting() {
        return;
    }
    let arena = &expr.arena;
    let mut stack = arena.children(expr.roots).to_vec();
    while let Some(id) = stack.pop() {
        let node = arena.node(id);
        if let CompactNode::Color { color, .. }
        | CompactNode::Boxed {
            bg_color: color, ..
        } = *node
            && uncolored_content(arena, node).is_some()
        {
            report.color(arena.str_val(color));
        }
        stack.extend(crate::ast::compact_children(arena, node));
    }
}
