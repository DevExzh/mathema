use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;

/// Normalize a math expression in place.
///
/// Applies the following simplifications bottom-up:
/// 1. Flatten nested Rows: `Row([a, Row([b, c]), d])` → `Row([a, b, c, d])`
/// 2. Unwrap single-child Rows: `Row([x])` → `x`
/// 3. Merge adjacent Text nodes with the same style
///    3b. Merge adjacent Number nodes
///    3c. Merge adjacent ASCII Identifier nodes (math-mode letter runs)
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

    // Phase 3b: merge adjacent Number nodes
    let mut final_nodes = Vec::with_capacity(merged.len());
    for node in merged {
        if let MathNode::Number(ref num) = node
            && let Some(MathNode::Number(prev_num)) = final_nodes.last_mut()
        {
            prev_num.push_str(num);
            continue;
        }
        final_nodes.push(node);
    }

    // Phase 3c: merge adjacent ASCII Identifier nodes (math-mode letter runs).
    let mut merged_ids = Vec::with_capacity(final_nodes.len());
    for node in final_nodes {
        if let MathNode::Identifier(ref text) = node
            && let Some(MathNode::Identifier(prev_text)) = merged_ids.last_mut()
            && prev_text.is_ascii()
            && text.is_ascii()
        {
            prev_text.push_str(text);
            continue;
        }
        merged_ids.push(node);
    }

    // Phase 4: attach empty-base scripts to the preceding atom.
    let mut coalesced = Vec::with_capacity(merged_ids.len());
    for node in merged_ids {
        if is_empty_base_script_math(&node)
            && let Some(prev) = coalesced.last()
            && can_carry_script_math(prev)
        {
            let prev = coalesced.pop().unwrap();
            coalesced.push(make_script_with_base_math(prev, node));
            continue;
        }
        coalesced.push(node);
    }

    *nodes = coalesced;
}

fn is_effectively_empty_math(node: &MathNode) -> bool {
    match node {
        MathNode::Row(children) => children.is_empty(),
        MathNode::Scaled { content, .. } => is_effectively_empty_math(content),
        _ => false,
    }
}

/// Collapse a script node whose subscript and/or superscript argument is
/// effectively empty.  This mirrors `flatten_empty_script` for the high-level
/// AST so the two normalization paths stay equivalent.
fn flatten_empty_script_math(node: &mut MathNode) {
    let is_candidate = match node {
        MathNode::Subscript { sub, .. } => is_effectively_empty_math(sub),
        MathNode::Superscript { sup, .. } => is_effectively_empty_math(sup),
        MathNode::SubSup { .. } => true,
        _ => false,
    };
    if !is_candidate {
        return;
    }

    // Take ownership via a temporary dummy value so we can inspect and rebuild.
    match std::mem::replace(node, MathNode::Row(vec![])) {
        MathNode::Subscript { base, .. } | MathNode::Superscript { base, .. } => {
            *node = *base;
        },
        MathNode::SubSup { base, sub, sup } => {
            let sub_empty = is_effectively_empty_math(&sub);
            let sup_empty = is_effectively_empty_math(&sup);
            *node = if sub_empty && sup_empty {
                *base
            } else if sub_empty {
                MathNode::Superscript {
                    base,
                    sup: Box::new(*sup),
                }
            } else if sup_empty {
                MathNode::Subscript {
                    base,
                    sub: Box::new(*sub),
                }
            } else {
                MathNode::SubSup { base, sub, sup }
            };
        },
        other => *node = other,
    }
}

fn is_empty_base_script_math(node: &MathNode) -> bool {
    matches!(
        node,
        MathNode::Subscript { base, .. }
            | MathNode::Superscript { base, .. }
            | MathNode::SubSup { base, .. }
            if is_effectively_empty_math(base)
    )
}

/// True for a node that can serve as the base of a sub/superscript in the
/// high-level AST. Mirrors `can_carry_script` for compact nodes.
fn can_carry_script_math(node: &MathNode) -> bool {
    !matches!(
        node,
        MathNode::Subscript { .. }
            | MathNode::Superscript { .. }
            | MathNode::SubSup { .. }
            | MathNode::PreScripts { .. }
            | MathNode::Symbol(_, _)
            | MathNode::Space(_)
    )
}

fn make_script_with_base_math(base: MathNode, script: MathNode) -> MathNode {
    match script {
        MathNode::Subscript { sub, .. } => MathNode::Subscript {
            base: Box::new(base),
            sub,
        },
        MathNode::Superscript { sup, .. } => MathNode::Superscript {
            base: Box::new(base),
            sup,
        },
        MathNode::SubSup { sub, sup, .. } => MathNode::SubSup {
            base: Box::new(base),
            sub,
            sup,
        },
        other => other,
    }
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
            if children.len() == 1
                && let Some(child) = children.pop()
            {
                *node = child;
            }
        },

        MathNode::Fenced { content, .. } => {
            for item in content.iter_mut() {
                if let FencedItem::Element(n) = item {
                    normalize_node_depth(n, d);
                }
            }
            // Coalesce empty-base scripts with the preceding element atom.
            // Middle delimiters naturally break the sequence because they are
            // not `Element` items.
            let mut coalesced: Vec<FencedItem> = Vec::with_capacity(content.len());
            for item in content.drain(..) {
                match item {
                    FencedItem::Element(node) if is_empty_base_script_math(&node) => {
                        if let Some(FencedItem::Element(prev)) = coalesced.last()
                            && can_carry_script_math(prev)
                        {
                            let FencedItem::Element(prev) = coalesced.pop().unwrap() else {
                                unreachable!()
                            };
                            coalesced
                                .push(FencedItem::Element(make_script_with_base_math(prev, node)));
                            continue;
                        }
                        coalesced.push(FencedItem::Element(node));
                    },
                    other => coalesced.push(other),
                }
            }
            *content = coalesced;
        },

        MathNode::Subscript { base, sub } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sub, d);
            flatten_empty_script_math(node);
        },
        MathNode::Superscript { base, sup } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sup, d);
            flatten_empty_script_math(node);
        },
        MathNode::SubSup { base, sub, sup } => {
            normalize_node_depth(base, d);
            normalize_node_depth(sub, d);
            normalize_node_depth(sup, d);
            flatten_empty_script_math(node);
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

        MathNode::Phantom(inner, _) | MathNode::Boxed { content: inner, .. } => {
            normalize_node_depth(inner, d);
        },
        MathNode::Cancel { content, .. } => {
            normalize_node_depth(content, d);
        },
        MathNode::Scaled { content, .. } | MathNode::RaiseBox { content, .. } => {
            normalize_node_depth(content, d);
        },
        MathNode::Styled(_, children) => {
            normalize_nodes_depth(children, d);
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            normalize_node_depth(display, d);
            normalize_node_depth(text, d);
            normalize_node_depth(script, d);
            normalize_node_depth(script_script, d);
        },
        MathNode::MathAtom { content, .. } => {
            normalize_node_depth(content, d);
        },

        // Leaf nodes: nothing to normalize
        MathNode::Number(_)
        | MathNode::Identifier(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_)
        | MathNode::Rule { .. } => {},
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
    // Keep the Row produced by `\idotsint` intact: the writer recognizes that
    // specific triple and emits the stable `\idotsint` command. Flattening it
    // into separate `\int \cdots \int` nodes (e.g. inside `\left\langle ...
    // \right\rangle`) makes the triple unrecognizable across a round-trip.
    let mut flattened: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &child_ids {
        match *arena.node(id) {
            CompactNode::Row(inner) if inner.is_empty() => {
                // Remove empty Row
            },
            CompactNode::Row(inner) if is_idotsint_row(arena, inner) => {
                flattened.push(id);
            },
            CompactNode::Row(inner) => {
                // Flatten: inline inner children
                for &inner_id in arena.children(inner) {
                    flattened.push(inner_id);
                }
            },
            _ => flattened.push(id),
        }
    }

    // Phase 2b: flatten Color wrappers whose color argument cannot be written
    // back verbatim.  The LaTeX writer drops the \textcolor{...} wrapper for
    // such colors and emits only the content, so keeping the wrapper in the
    // AST hides the content's edges from the spacing logic and produces output
    // like `m=m` instead of `m= m`.
    let mut splayed: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &flattened {
        if let CompactNode::Color { color, content } = *arena.node(id)
            && is_unwritable_color(arena.str_val(color))
        {
            if let CompactNode::Row(inner) = *arena.node(content) {
                for &inner_id in arena.children(inner) {
                    splayed.push(inner_id);
                }
            } else {
                splayed.push(content);
            }
        } else {
            splayed.push(id);
        }
    }

    // Phase 2c: drop zero-width spacing nodes. They produce no visible output
    // (e.g. a malformed/empty `\kern` or `\hspace{}`) and their presence can
    // block merges, suppress script attachment, or create/destroy separator
    // spaces in the writer.
    let splayed: SmallVec<[NodeId; 8]> = splayed
        .into_iter()
        .filter(|&id| !matches!(*arena.node(id), CompactNode::Space(r) if r == Rational::zero()))
        .collect();

    // Phase 3: merge adjacent Text nodes with the same style.
    let mut merged: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &splayed {
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

    // Phase 3b: merge adjacent Number nodes.
    let mut final_nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &merged {
        let node = *arena.node(id);
        if let CompactNode::Number(sid) = node
            && let Some(&prev_id) = final_nodes.last()
            && let CompactNode::Number(prev_sid) = *arena.node(prev_id)
        {
            let prev_s = arena.str_val(prev_sid).to_owned();
            let cur_s = arena.str_val(sid);
            let combined = format!("{prev_s}{cur_s}");
            let new_sid = arena.push_str(&combined);
            *arena.node_mut(prev_id) = CompactNode::Number(new_sid);
            continue;
        }
        final_nodes.push(id);
    }

    // Phase 3c: merge adjacent ASCII Identifier nodes.  In math mode a run of
    // ASCII letters is a single multi-letter identifier, but the parser emits
    // each letter as its own node.  Coalescing them keeps the writer from
    // adding unnecessary braces around braced single-letter runs (e.g. the
    // denominator of `\binom{x}{abc}` parsed from `{a}{b}{c}`).  Non-ASCII
    // identifiers are not merged because the LaTeX writer may emit the first
    // character as a `\command`, which cannot absorb subsequent characters
    // stably.
    let mut merged_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &final_nodes {
        let node = *arena.node(id);
        if let CompactNode::Identifier(sid) = node
            && let Some(&prev_id) = merged_ids.last()
            && let CompactNode::Identifier(prev_sid) = *arena.node(prev_id)
            && arena.str_val(prev_sid).is_ascii()
            && arena.str_val(sid).is_ascii()
        {
            let prev_s = arena.str_val(prev_sid).to_owned();
            let cur_s = arena.str_val(sid);
            let combined = format!("{prev_s}{cur_s}");
            let new_sid = arena.push_str(&combined);
            *arena.node_mut(prev_id) = CompactNode::Identifier(new_sid);
            continue;
        }
        merged_ids.push(id);
    }

    // Phase 4: attach empty-base scripts to the preceding atom.  In TeX a
    // sub/superscript marker immediately after an atom belongs to that atom;
    // if the parser left it as a separate empty-base script node, rewrite it
    // so the previous sibling becomes the base.  This prevents round-trip
    // divergence such as `op^{\prime}` → `o{p}^{\prime}`.
    let mut coalesced: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &merged_ids {
        if is_empty_base_script(arena, id)
            && let Some(&prev_id) = coalesced.last()
            && can_carry_script(*arena.node(prev_id))
        {
            coalesced.pop();
            coalesced.push(make_script_with_base(arena, prev_id, id));
            continue;
        }
        coalesced.push(id);
    }

    arena.push_children(&coalesced)
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
        },

        CompactNode::Styled(style, span) => {
            let new_span = normalize_compact_span(arena, span, d);
            *arena.node_mut(id) = CompactNode::Styled(style, new_span);
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            // Normalize the fenced content as a row: flatten nested rows,
            // remove empty rows, merge adjacent text/number atoms, and attach
            // empty-base scripts to the preceding atom.  Middle delimiters are
            // leaves and pass through untouched.
            let new_content = normalize_compact_span(arena, content, d);
            *arena.node_mut(id) = CompactNode::Fenced {
                open,
                close,
                content: new_content,
            };
        },

        CompactNode::Subscript { base, sub } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sub, d);
            flatten_empty_script(arena, id);
        },
        CompactNode::Superscript { base, sup } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sup, d);
            flatten_empty_script(arena, id);
        },
        CompactNode::SubSup { base, sub, sup } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, sub, d);
            normalize_compact_node(arena, sup, d);
            flatten_empty_script(arena, id);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_compact_node(arena, base, d);
            if pre_sub.is_some() {
                normalize_compact_node(arena, pre_sub, d);
            }
            if pre_sup.is_some() {
                normalize_compact_node(arena, pre_sup, d);
            }
            // The LaTeX writer cannot emit a prescript in a way that the
            // parser reliably reconstructs as PreScripts; the usual source
            // form is interpreted as an empty-base script followed by the
            // base.  Normalize to that shape early so the output is stable.
            let empty = arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            let script_id = match (pre_sub != NodeId::NONE, pre_sup != NodeId::NONE) {
                (true, true) => arena.push_node(CompactNode::SubSup {
                    base: empty,
                    sub: pre_sub,
                    sup: pre_sup,
                }),
                (true, false) => arena.push_node(CompactNode::Subscript {
                    base: empty,
                    sub: pre_sub,
                }),
                (false, true) => arena.push_node(CompactNode::Superscript {
                    base: empty,
                    sup: pre_sup,
                }),
                (false, false) => {
                    *arena.node_mut(id) = *arena.node(base);
                    return;
                },
            };
            let span = arena.push_children(&[script_id, base]);
            *arena.node_mut(id) = CompactNode::Row(span);
        },

        CompactNode::Overscript { base, over, .. } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, over, d);
        },
        CompactNode::Underscript { base, under, .. } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, under, d);
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_compact_node(arena, base, d);
            normalize_compact_node(arena, under, d);
            normalize_compact_node(arena, over, d);
        },

        CompactNode::Fraction { num, den, .. } => {
            normalize_compact_node(arena, num, d);
            normalize_compact_node(arena, den, d);
        },
        CompactNode::Sqrt(inner) => {
            normalize_compact_node(arena, inner, d);
        },
        CompactNode::Root { index, radicand } => {
            normalize_compact_node(arena, index, d);
            normalize_compact_node(arena, radicand, d);
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                normalize_compact_node(arena, sub, d);
            }
            if sup.is_some() {
                normalize_compact_node(arena, sup, d);
            }
            normalize_compact_node(arena, body, d);
        },

        CompactNode::Accent { base, .. } => {
            normalize_compact_node(arena, base, d);
        },

        CompactNode::Array { cells, .. } => {
            // Each cell is a Row node — normalize it, then trim leading/trailing
            // whitespace and replace any internal row separators with ordinary
            // space. A `\\`/`\cr` inside a cell is always a row separator on
            // re-parse, so keeping it as a node makes the written shape unstable.
            let cell_ids: SmallVec<[NodeId; 8]> = arena.children(cells).into();
            let mut new_cells: SmallVec<[NodeId; 8]> = SmallVec::new();
            for &cell_id in &cell_ids {
                normalize_compact_node(arena, cell_id, d);
                let new_id = normalize_array_cell(arena, cell_id);
                new_cells.push(new_id);
            }
            let new_cells_span = arena.push_children(&new_cells);
            if let CompactNode::Array { cells: c, .. } = arena.node_mut(id) {
                *c = new_cells_span;
            }
        },

        CompactNode::Phantom(inner, _) | CompactNode::Boxed { content: inner, .. } => {
            normalize_compact_node(arena, inner, d);
        },
        CompactNode::Cancel { content, .. } | CompactNode::RaiseBox { content, .. } => {
            normalize_compact_node(arena, content, d);
        },
        CompactNode::Scaled { content, .. } => {
            normalize_compact_node(arena, content, d);
            // Sizing commands only apply to delimiter symbols. If the parser
            // produced a wrapper around anything else, the writer will drop the
            // sizing command and just emit the content. Flatten the wrapper now
            // so that the emitted bracing matches the re-parsed shape.
            match *arena.node(content) {
                CompactNode::Symbol(
                    SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence,
                    _,
                ) => {
                    // Keep the sizing wrapper for delimiters.
                },
                CompactNode::Row(span) => {
                    let children: SmallVec<[NodeId; 8]> = arena.children(span).into();
                    let new_span = arena.push_children(&children);
                    *arena.node_mut(id) = CompactNode::Row(new_span);
                },
                _ => {
                    *arena.node_mut(id) = *arena.node(content);
                },
            }
        },
        CompactNode::Color { content, .. } => {
            normalize_compact_node(arena, content, d);
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            normalize_compact_node(arena, display, d);
            normalize_compact_node(arena, text, d);
            normalize_compact_node(arena, script, d);
            normalize_compact_node(arena, script_script, d);
        },
        CompactNode::MathAtom { content, .. } => {
            normalize_compact_node(arena, content, d);
        },

        // Leaf nodes: nothing to normalize
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
    }
}

/// If a script node's subscript and/or superscript argument is effectively
/// empty, collapse it to its base (or to the remaining non-empty script kind).
/// An empty script marker carries no visual or semantic information, but
/// preserving the wrapper makes the writer emit extra braces that disappear on
/// re-parse.
fn flatten_empty_script(arena: &mut AstArena, id: NodeId) {
    let node = *arena.node(id);
    let base = match node {
        CompactNode::Subscript { base, sub } if is_effectively_empty_node(arena, sub) => base,
        CompactNode::Superscript { base, sup } if is_effectively_empty_node(arena, sup) => base,
        CompactNode::SubSup { base, sub, sup } => {
            let sub_empty = is_effectively_empty_node(arena, sub);
            let sup_empty = is_effectively_empty_node(arena, sup);
            if sub_empty && sup_empty {
                base
            } else if sub_empty {
                let new_id = arena.push_node(CompactNode::Superscript { base, sup });
                *arena.node_mut(id) = *arena.node(new_id);
                return;
            } else if sup_empty {
                let new_id = arena.push_node(CompactNode::Subscript { base, sub });
                *arena.node_mut(id) = *arena.node(new_id);
                return;
            } else {
                return;
            }
        },
        _ => return,
    };
    *arena.node_mut(id) = *arena.node(base);
}

/// True if `id` refers to a node that produces no output when written.
/// Currently covers empty Rows and sizing wrappers (\big, \Big, ...) whose
/// content is itself empty — both render as nothing in the output stream.
fn is_effectively_empty_node(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    match *arena.node(id) {
        CompactNode::Row(span) => arena.children(span).is_empty(),
        CompactNode::Scaled { content, .. } => is_effectively_empty_node(arena, content),
        CompactNode::Space(r) => r == Rational::zero(),
        _ => false,
    }
}

/// True for a Row span that is the canonical `\idotsint` expansion
/// `\int \cdots \int`.
fn is_idotsint_row(arena: &AstArena, span: NodeSpan) -> bool {
    let children = arena.children(span);
    if children.len() != 3 {
        return false;
    }
    let is_int = |id: NodeId| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::LargeOp, sid)
                if arena.str_val(sid) == "\u{222B}"
        )
    };
    let is_dots = |id: NodeId| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::Ordinary, sid)
                if arena.str_val(sid) == "\u{22EF}"
        )
    };
    is_int(children[0]) && is_dots(children[1]) && is_int(children[2])
}

/// Normalize an array/matrix cell. Trims leading/trailing whitespace and
/// converts any internal row separator (`Newline`) into ordinary space, because
/// `\\`/`\cr` inside a cell is always interpreted as ending the row on
/// re-parse. Keeping it as a node makes the written shape flip between
/// `...t\\ +...` and `...t \\\\ +...`.
fn normalize_array_cell(arena: &mut AstArena, cell_id: NodeId) -> NodeId {
    let span = match *arena.node(cell_id) {
        CompactNode::Row(span) => span,
        _ => return cell_id,
    };
    let children: SmallVec<[NodeId; 8]> = arena.children(span).into();
    let is_ws = |arena: &AstArena, id: NodeId| {
        matches!(
            *arena.node(id),
            CompactNode::Space(_) | CompactNode::Newline
        )
    };
    // Trim leading whitespace.
    let mut start = 0;
    while start < children.len() && is_ws(arena, children[start]) {
        start += 1;
    }
    // Trim trailing whitespace.
    let mut end = children.len();
    while end > start && is_ws(arena, children[end - 1]) {
        end -= 1;
    }
    let mut new_children: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &child_id in &children[start..end] {
        if matches!(*arena.node(child_id), CompactNode::Newline) {
            new_children.push(arena.push_node(CompactNode::Space(Rational::new(1, 4))));
        } else {
            new_children.push(child_id);
        }
    }
    let new_span = arena.push_children(&new_children);
    arena.push_node(CompactNode::Row(new_span))
}

/// True for color strings that the LaTeX writer cannot emit verbatim inside
/// `\textcolor{...}` without breaking brace balance or being reinterpreted.
/// Such wrappers are dropped during writing, so the normalizer flattens them
/// early so spacing decisions see the wrapped content.
fn is_unwritable_color(color: &str) -> bool {
    color.chars().any(|c| {
        matches!(
            c,
            '{' | '}' | '\\' | '~' | '#' | '$' | '%' | '&' | '^' | '_'
        )
    })
}

/// True if `id` is a script node whose base is effectively empty.  Such nodes
/// arise when a sub/superscript marker is not attached to an explicit base
/// during parsing (e.g. `sm\Big'` where `\Big` has no content).
fn is_empty_base_script(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => is_effectively_empty_node(arena, base),
        _ => false,
    }
}

/// True for a node that can serve as the base of a sub/superscript.
///
/// Script-marker nodes and spacing/helpers are excluded because they cannot
/// serve as a stable base. Symbols are allowed: `=^{\prime}`, `+^{2}`,
/// `)^{\prime}`, and `.*^{\prime}` are all valid TeX and round-trip stably
/// when the empty-base script is attached to the preceding atom.
fn can_carry_script(node: CompactNode) -> bool {
    !matches!(
        node,
        CompactNode::Subscript { .. }
            | CompactNode::Superscript { .. }
            | CompactNode::SubSup { .. }
            | CompactNode::PreScripts { .. }
            | CompactNode::Middle(_)
            | CompactNode::ColAlign(_)
            | CompactNode::StyleSwitch(_)
            | CompactNode::SizeScale(_)
            | CompactNode::Newline
            | CompactNode::Space(_)
    )
}

/// Build a new script node whose base is `base` and whose sub/superscripts are
/// taken from the empty-base script node `script_id`.
fn make_script_with_base(arena: &mut AstArena, base: NodeId, script_id: NodeId) -> NodeId {
    match *arena.node(script_id) {
        CompactNode::Subscript { sub, .. } => arena.push_node(CompactNode::Subscript { base, sub }),
        CompactNode::Superscript { sup, .. } => {
            arena.push_node(CompactNode::Superscript { base, sup })
        },
        CompactNode::SubSup { sub, sup, .. } => {
            arena.push_node(CompactNode::SubSup { base, sub, sup })
        },
        _ => script_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::compact::CompactExpr;

    #[test]
    fn flatten_nested_rows() {
        // Use Symbols rather than Identifiers here so the test isolates Row
        // flattening from the Identifier-merging phase.
        let mut expr = MathExpr::inline(vec![
            MathNode::Symbol(SymbolKind::Ordinary, "a".into()),
            MathNode::Row(vec![
                MathNode::Symbol(SymbolKind::Ordinary, "b".into()),
                MathNode::Symbol(SymbolKind::Ordinary, "c".into()),
            ]),
            MathNode::Symbol(SymbolKind::Ordinary, "d".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 4);
        assert_eq!(
            expr.nodes[1],
            MathNode::Symbol(SymbolKind::Ordinary, "b".into())
        );
        assert_eq!(
            expr.nodes[2],
            MathNode::Symbol(SymbolKind::Ordinary, "c".into())
        );
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
        // Use Symbols rather than Identifiers here so the test isolates empty
        // Row removal from the Identifier-merging phase.
        let mut expr = MathExpr::inline(vec![
            MathNode::Symbol(SymbolKind::Ordinary, "x".into()),
            MathNode::Row(vec![]),
            MathNode::Symbol(SymbolKind::Ordinary, "y".into()),
        ]);
        normalize_expr(&mut expr);
        assert_eq!(expr.nodes.len(), 2);
        assert_eq!(
            expr.nodes[0],
            MathNode::Symbol(SymbolKind::Ordinary, "x".into())
        );
        assert_eq!(
            expr.nodes[1],
            MathNode::Symbol(SymbolKind::Ordinary, "y".into())
        );
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
        assert_normalize_equiv(MathExpr::inline(vec![MathNode::Row(vec![MathNode::Row(
            vec![MathNode::Row(vec![MathNode::Identifier("x".into())])],
        )])]));
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
