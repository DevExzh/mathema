use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
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

    // Phase 4: an empty-base script is a limit of a big operator before it,
    // else a leading script of the next node (see `normalize_compact_span`).
    let mut with_limits: Vec<MathNode> = Vec::with_capacity(merged_ids.len());
    for node in merged_ids {
        if is_empty_base_script_math(&node) && with_limits.last().is_some_and(takes_limits_math) {
            let prev = with_limits.pop().unwrap_or(MathNode::Row(Vec::new()));
            with_limits.push(make_script_with_base_math(prev, node));
            continue;
        }
        with_limits.push(node);
    }
    let mut coalesced: Vec<MathNode> = Vec::with_capacity(with_limits.len());
    for node in with_limits.into_iter().rev() {
        if is_empty_base_script_math(&node)
            && coalesced.last().is_some_and(|next| {
                can_take_prescripts_math(next) && !is_empty_base_script_math(next)
            })
        {
            let next = coalesced.pop().unwrap_or(MathNode::Row(Vec::new()));
            coalesced.push(make_prescripts_math(node, next));
            continue;
        }
        coalesced.push(node);
    }
    coalesced.reverse();
    let content_end = coalesced
        .iter()
        .rposition(|node| !matches!(node, MathNode::Space(_)))
        .map_or(0, |i| i + 1);
    if content_end >= 2
        && is_empty_base_script_math(&coalesced[content_end - 1])
        && can_carry_script_math(&coalesced[content_end - 2])
    {
        let last = coalesced.remove(content_end - 1);
        let prev = std::mem::replace(&mut coalesced[content_end - 2], MathNode::Row(Vec::new()));
        coalesced[content_end - 2] = make_script_with_base_math(prev, last);
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

/// Function names whose scripts are limits, set under the name in display
/// style (UTN #28 lists det, gcd, inf, lim, lim inf, lim sup, max, min, Pr
/// and sup).
pub(crate) fn is_limit_function(name: &str) -> bool {
    matches!(
        name,
        "lim"
            | "liminf"
            | "limsup"
            | "lim inf"
            | "lim sup"
            | "max"
            | "min"
            | "sup"
            | "inf"
            | "det"
            | "gcd"
            | "Pr"
            | "argmax"
            | "argmin"
            | "arg max"
            | "arg min"
            | "inj lim"
            | "proj lim"
    )
}

/// Whether a function name such as sin or lim is spelled `name`.
fn is_function_name(name: &str) -> bool {
    is_limit_function(name) || crate::unicode::symbols::is_operator_command(name)
}

/// Whether a node takes a script that follows it: a big operator (as its
/// limit) or a function name (as its limit or its script). Mirrors
/// `takes_limits` for compact nodes.
fn takes_limits_math(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, _) => true,
        MathNode::Operator(_) => true,
        MathNode::Identifier(name) | MathNode::Symbol(_, name) => is_function_name(name),
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        } => true,
        _ => false,
    }
}

/// Whether a node can be the base of leading scripts. Mirrors
/// `can_take_prescripts` for compact nodes.
fn can_take_prescripts_math(node: &MathNode) -> bool {
    !matches!(node, MathNode::PreScripts { .. } | MathNode::Space(_))
}

/// Make an empty-base script a leading script of `base`.
fn make_prescripts_math(script: MathNode, base: MathNode) -> MathNode {
    let (pre_sub, pre_sup) = match script {
        MathNode::Subscript { sub, .. } => (Some(sub), None),
        MathNode::Superscript { sup, .. } => (None, Some(sup)),
        MathNode::SubSup { sub, sup, .. } => (Some(sub), Some(sup)),
        other => return MathNode::Row(vec![other, base]),
    };
    MathNode::PreScripts {
        base: Box::new(base),
        pre_sub,
        pre_sup,
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
            // Empty-base scripts become leading scripts of the next element;
            // middle delimiters are not elements and break the sequence.
            let mut with_limits: Vec<FencedItem> = Vec::with_capacity(content.len());
            for item in content.drain(..) {
                match item {
                    FencedItem::Element(node)
                        if is_empty_base_script_math(&node)
                            && matches!(with_limits.last(), Some(FencedItem::Element(prev)) if takes_limits_math(prev)) =>
                    {
                        let Some(FencedItem::Element(prev)) = with_limits.pop() else {
                            unreachable!()
                        };
                        with_limits
                            .push(FencedItem::Element(make_script_with_base_math(prev, node)));
                    },
                    other => with_limits.push(other),
                }
            }
            let mut coalesced: Vec<FencedItem> = Vec::with_capacity(with_limits.len());
            for item in with_limits.into_iter().rev() {
                match item {
                    FencedItem::Element(node)
                        if is_empty_base_script_math(&node)
                            && matches!(coalesced.last(), Some(FencedItem::Element(next))
                                if can_take_prescripts_math(next) && !is_empty_base_script_math(next)) =>
                    {
                        let Some(FencedItem::Element(next)) = coalesced.pop() else {
                            unreachable!()
                        };
                        coalesced.push(FencedItem::Element(make_prescripts_math(node, next)));
                    },
                    other => coalesced.push(other),
                }
            }
            coalesced.reverse();
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

        MathNode::Phantom(inner, _)
        | MathNode::Boxed { content: inner, .. }
        | MathNode::Actuarial { content: inner, .. } => {
            normalize_node_depth(inner, d);
        },
        MathNode::Reflect { content: inner, .. } => normalize_node_depth(inner, d),
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
///
/// # Errors
///
/// [`Error::LimitExceeded`](crate::Error::LimitExceeded) with
/// [`LimitKind::ListLength`](crate::LimitKind::ListLength) when flattening
/// nested rows would give one node more children than the arena can store,
/// or [`LimitKind::ArenaSize`](crate::LimitKind::ArenaSize) when the arena
/// would outgrow its 32-bit indices.
pub fn normalize_compact_expr(expr: &mut CompactExpr) -> crate::error::Result<()> {
    expr.roots = normalize_compact_span(&mut expr.arena, expr.roots, 0)?;
    expr.arena.check_limits()
}

/// Normalize a span of child nodes. Returns a (possibly new) span.
fn normalize_compact_span(
    arena: &mut AstArena,
    span: NodeSpan,
    depth: usize,
) -> crate::error::Result<NodeSpan> {
    if depth > super::MAX_AST_DEPTH {
        return Ok(span);
    }
    let d = depth + 1;

    // Phase 1: recursively normalize each child node.
    // Collect IDs first to avoid borrow conflicts with arena.
    let child_ids: SmallVec<[NodeId; 8]> = arena.children(span).into();
    for &id in &child_ids {
        normalize_compact_node(arena, id, d)?;
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
    // Phase 3b: merge adjacent Number nodes.
    // Phase 3c: merge adjacent ASCII Identifier nodes.  In math mode a run of
    // ASCII letters is a single multi-letter identifier, but the parser emits
    // each letter as its own node.  Coalescing them keeps the writer from
    // adding unnecessary braces around braced single-letter runs (e.g. the
    // denominator of `\binom{x}{abc}` parsed from `{a}{b}{c}`).  Non-ASCII
    // identifiers are not merged because the LaTeX writer may emit the first
    // character as a `\command`, which cannot absorb subsequent characters
    // stably.
    //
    // Each run is collected first and its string interned once, so merging
    // stays linear in the length of the run.
    let merged = merge_runs(arena, &splayed, |_, node| match node {
        CompactNode::Text(style, sid) => Some((MergeKind::Text(style), sid)),
        _ => None,
    });
    let final_nodes = merge_runs(arena, &merged, |_, node| match node {
        CompactNode::Number(sid) => Some((MergeKind::Number, sid)),
        _ => None,
    });
    let merged_ids = merge_runs(arena, &final_nodes, |arena, node| match node {
        CompactNode::Identifier(sid) if arena.str_val(sid).is_ascii() => {
            Some((MergeKind::Identifier, sid))
        },
        _ => None,
    });

    // Phase 4: an empty-base script belongs to what follows it (`{}^{14}C`,
    // an MTEF script template with tvSU_PRECEDES, MathML `mprescripts`,
    // OMML `m:sPre`): make it a leading script of the next node. Scripts the
    // LaTeX parser left without a base after white space (`| '`) are
    // attached to the preceding atom by the parser itself. After a big
    // operator or a function that takes limits the script is its limit
    // (`<mo>∫</mo><msub><mrow/><mi>C</mi></msub>`, as TeX-to-MathML
    // converters write `\int_C`).
    let mut with_limits: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &merged_ids {
        if is_empty_base_script(arena, id)
            && let Some(&prev_id) = with_limits.last()
            && takes_limits(arena, *arena.node(prev_id))
        {
            with_limits.pop();
            with_limits.push(make_script_with_base(arena, prev_id, id));
            continue;
        }
        with_limits.push(id);
    }
    let mut coalesced: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in with_limits.iter().rev() {
        if let Some(&next_id) = coalesced.last()
            && let Some((sub, sup)) = empty_base_scripts(arena, id)
            && can_take_prescripts(*arena.node(next_id))
            && !is_empty_base_script(arena, next_id)
        {
            coalesced.pop();
            coalesced.push(arena.push_node(CompactNode::PreScripts {
                base: next_id,
                pre_sub: sub,
                pre_sup: sup,
            }));
            continue;
        }
        coalesced.push(id);
    }
    coalesced.reverse();
    // With nothing but space after it, a script belongs to the atom before
    // it, as in TeX (`x'` written with white space or a group between).
    let content_end = coalesced
        .iter()
        .rposition(|&id| !is_blank(*arena.node(id)))
        .map_or(0, |i| i + 1);
    if content_end >= 2
        && is_empty_base_script(arena, coalesced[content_end - 1])
        && can_carry_script(*arena.node(coalesced[content_end - 2]))
    {
        let (prev, last) = (coalesced[content_end - 2], coalesced[content_end - 1]);
        let joined = make_script_with_base(arena, prev, last);
        coalesced.remove(content_end - 1);
        coalesced[content_end - 2] = joined;
    }

    arena.push_children(&coalesced)
}

/// The node kinds whose adjacent runs normalization merges.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MergeKind {
    Text(TextStyle),
    Number,
    Identifier,
}

/// Merge each run of adjacent nodes that `key` gives the same kind into the
/// run's first node, interning the run's string once.
fn merge_runs(
    arena: &mut AstArena,
    ids: &[NodeId],
    key: impl Fn(&AstArena, CompactNode) -> Option<(MergeKind, StrId)>,
) -> SmallVec<[NodeId; 8]> {
    let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
    let mut i = 0;
    while i < ids.len() {
        let first = ids[i];
        let Some((kind, first_sid)) = key(arena, *arena.node(first)) else {
            out.push(first);
            i += 1;
            continue;
        };
        let mut end = i + 1;
        while end < ids.len()
            && matches!(key(arena, *arena.node(ids[end])), Some((k, _)) if k == kind)
        {
            end += 1;
        }
        if end - i > 1 {
            let mut combined = String::from(arena.str_val(first_sid));
            for &id in &ids[i + 1..end] {
                if let Some((_, sid)) = key(arena, *arena.node(id)) {
                    combined.push_str(arena.str_val(sid));
                }
            }
            let sid = arena.push_str(&combined);
            *arena.node_mut(first) = match kind {
                MergeKind::Text(style) => CompactNode::Text(style, sid),
                MergeKind::Number => CompactNode::Number(sid),
                MergeKind::Identifier => CompactNode::Identifier(sid),
            };
        }
        out.push(first);
        i = end;
    }
    out
}

/// Normalize a single compact node. May replace it in-place via `arena.node_mut`.
fn normalize_compact_node(
    arena: &mut AstArena,
    id: NodeId,
    depth: usize,
) -> crate::error::Result<()> {
    if depth > super::MAX_AST_DEPTH {
        return Ok(());
    }
    let d = depth + 1;

    let node = *arena.node(id);
    match node {
        CompactNode::Row(span) => {
            let new_span = normalize_compact_span(arena, span, d)?;
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
            let new_span = normalize_compact_span(arena, span, d)?;
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
            let new_content = normalize_compact_span(arena, content, d)?;
            *arena.node_mut(id) = CompactNode::Fenced {
                open,
                close,
                content: new_content,
            };
        },

        CompactNode::Subscript { base, sub } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, sub, d)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::Superscript { base, sup } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, sup, d)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::SubSup { base, sub, sup } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, sub, d)?;
            normalize_compact_node(arena, sup, d)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_compact_node(arena, base, d)?;
            if pre_sub.is_some() {
                normalize_compact_node(arena, pre_sub, d)?;
            }
            if pre_sup.is_some() {
                normalize_compact_node(arena, pre_sup, d)?;
            }
            // Leading scripts stay leading scripts of their base. Without
            // scripts the node is its base; without a base it is an
            // empty-base script (one with nothing to precede).
            let sub = (pre_sub.is_some() && !is_effectively_empty_node(arena, pre_sub))
                .then_some(pre_sub);
            let sup = (pre_sup.is_some() && !is_effectively_empty_node(arena, pre_sup))
                .then_some(pre_sup);
            let replacement = match (sub, sup) {
                (None, None) => *arena.node(base),
                _ if !is_effectively_empty_node(arena, base) => CompactNode::PreScripts {
                    base,
                    pre_sub: sub.unwrap_or(NodeId::NONE),
                    pre_sup: sup.unwrap_or(NodeId::NONE),
                },
                // An effectively empty base (a zero-width space, an empty
                // row) is the empty row an empty-base script has.
                (Some(sub), Some(sup)) => CompactNode::SubSup {
                    base: arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    sub,
                    sup,
                },
                (Some(sub), None) => CompactNode::Subscript {
                    base: arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    sub,
                },
                (None, Some(sup)) => CompactNode::Superscript {
                    base: arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    sup,
                },
            };
            *arena.node_mut(id) = replacement;
        },

        CompactNode::Overscript { base, over, .. } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, over, d)?;
        },
        CompactNode::Underscript { base, under, .. } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, under, d)?;
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_compact_node(arena, base, d)?;
            normalize_compact_node(arena, under, d)?;
            normalize_compact_node(arena, over, d)?;
        },

        CompactNode::Fraction { num, den, .. } => {
            normalize_compact_node(arena, num, d)?;
            normalize_compact_node(arena, den, d)?;
        },
        CompactNode::Sqrt(inner) => {
            normalize_compact_node(arena, inner, d)?;
        },
        CompactNode::Root { index, radicand } => {
            normalize_compact_node(arena, index, d)?;
            normalize_compact_node(arena, radicand, d)?;
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                normalize_compact_node(arena, sub, d)?;
            }
            if sup.is_some() {
                normalize_compact_node(arena, sup, d)?;
            }
            normalize_compact_node(arena, body, d)?;
        },

        CompactNode::Accent { base, .. } => {
            normalize_compact_node(arena, base, d)?;
        },

        CompactNode::Array { cells, .. } => {
            // Each cell is a Row node — normalize it, then trim leading/trailing
            // whitespace and replace any internal row separators with ordinary
            // space. A `\\`/`\cr` inside a cell is always a row separator on
            // re-parse, so keeping it as a node makes the written shape unstable.
            let cell_ids: SmallVec<[NodeId; 8]> = arena.children(cells).into();
            let mut new_cells: SmallVec<[NodeId; 8]> = SmallVec::new();
            for &cell_id in &cell_ids {
                normalize_compact_node(arena, cell_id, d)?;
                let new_id = normalize_array_cell(arena, cell_id)?;
                new_cells.push(new_id);
            }
            let new_cells_span = arena.push_children(&new_cells)?;
            if let CompactNode::Array { cells: c, .. } = arena.node_mut(id) {
                *c = new_cells_span;
            }
        },

        CompactNode::Phantom(inner, _)
        | CompactNode::Boxed { content: inner, .. }
        | CompactNode::Actuarial { content: inner, .. } => {
            normalize_compact_node(arena, inner, d)?;
        },
        CompactNode::Reflect { content: inner, .. } => normalize_compact_node(arena, inner, d)?,
        CompactNode::Cancel { content, .. } | CompactNode::RaiseBox { content, .. } => {
            normalize_compact_node(arena, content, d)?;
        },
        CompactNode::Scaled { content, .. } => {
            normalize_compact_node(arena, content, d)?;
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
                    let new_span = arena.push_children(&children)?;
                    *arena.node_mut(id) = CompactNode::Row(new_span);
                },
                _ => {
                    *arena.node_mut(id) = *arena.node(content);
                },
            }
        },
        CompactNode::Color { content, .. } => {
            normalize_compact_node(arena, content, d)?;
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            normalize_compact_node(arena, display, d)?;
            normalize_compact_node(arena, text, d)?;
            normalize_compact_node(arena, script, d)?;
            normalize_compact_node(arena, script_script, d)?;
        },
        CompactNode::MathAtom { content, .. } => {
            normalize_compact_node(arena, content, d)?;
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
    Ok(())
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
fn normalize_array_cell(arena: &mut AstArena, cell_id: NodeId) -> crate::error::Result<NodeId> {
    let span = match *arena.node(cell_id) {
        CompactNode::Row(span) => span,
        _ => return Ok(cell_id),
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
    let new_span = arena.push_children(&new_children)?;
    Ok(arena.push_node(CompactNode::Row(new_span)))
}

/// True for color strings that the LaTeX writer cannot emit verbatim inside
/// `\textcolor{...}` without breaking brace balance or being reinterpreted.
/// Such wrappers are dropped during writing, so the normalizer flattens them
/// early so spacing decisions see the wrapped content.
fn is_unwritable_color(color: &str) -> bool {
    // `#RRGGBB` is written as `\textcolor[HTML]{RRGGBB}`.
    if color
        .strip_prefix('#')
        .is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return false;
    }
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
pub(crate) fn can_carry_script(node: CompactNode) -> bool {
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
/// The scripts (`NodeId::NONE` when absent) of a script node whose base is
/// empty.
fn empty_base_scripts(arena: &AstArena, id: NodeId) -> Option<(NodeId, NodeId)> {
    if !is_empty_base_script(arena, id) {
        return None;
    }
    match *arena.node(id) {
        CompactNode::Subscript { sub, .. } => Some((sub, NodeId::NONE)),
        CompactNode::Superscript { sup, .. } => Some((NodeId::NONE, sup)),
        CompactNode::SubSup { sub, sup, .. } => Some((sub, sup)),
        _ => None,
    }
}

/// Whether a node takes a script that follows it: a big operator (as its
/// limit) or a function name (as its limit or its script).
pub(crate) fn takes_limits(arena: &AstArena, node: CompactNode) -> bool {
    match node {
        CompactNode::Symbol(SymbolKind::LargeOp, _) => true,
        // Function names take the script as their limit (lim) or as an
        // ordinary script (`\sin {}^2 x` is sin squared).
        CompactNode::Operator { .. } => true,
        // MathML writes function names as `<mi>lim</mi>` or `<mo>sin</mo>`.
        CompactNode::Identifier(name) | CompactNode::Symbol(_, name) => {
            is_function_name(arena.str_val(name))
        },
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        } => true,
        _ => false,
    }
}

/// A node that only spaces (white space or a line break).
pub(crate) fn is_blank(node: CompactNode) -> bool {
    matches!(node, CompactNode::Space(_) | CompactNode::Newline)
}

/// Whether a node can be the base of leading scripts.
pub(crate) fn can_take_prescripts(node: CompactNode) -> bool {
    !matches!(
        node,
        CompactNode::PreScripts { .. }
            | CompactNode::Middle(_)
            | CompactNode::ColAlign(_)
            | CompactNode::StyleSwitch(_)
            | CompactNode::SizeScale(_)
            | CompactNode::Newline
            | CompactNode::Space(_)
    )
}

pub(crate) fn make_script_with_base(
    arena: &mut AstArena,
    base: NodeId,
    script_id: NodeId,
) -> NodeId {
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
        let mut compact = CompactExpr::from_math_expr(&input).unwrap();
        normalize_compact_expr(&mut compact).unwrap();
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
