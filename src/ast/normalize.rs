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
    // Sizes around lists are those of what the lists hold, and lists are opened.
    if super::has_attributes(expr)
        && let Ok(compact) = CompactExpr::from_math_expr(expr)
    {
        *expr = compact.to_math_expr();
    }
    normalize_nodes_depth(&mut expr.nodes, 0);
}

fn normalize_nodes_depth(nodes: &mut Vec<MathNode>, depth: usize) {
    normalize_nodes_depth_with_identifiers(nodes, depth, true);
}

fn normalize_nodes_depth_with_identifiers(
    nodes: &mut Vec<MathNode>,
    depth: usize,
    merge_identifiers: bool,
) {
    normalize_nodes_keeping_breaks(nodes, depth, merge_identifiers, false);
}

/// Normalize a heap table cell. A line break in it stays inside a group, as a
/// bare `\\` would end the table row when written (see
/// [`normalize_array_cell`]).
fn normalize_array_cell_math(cell: &mut Vec<MathNode>, depth: usize, merge_identifiers: bool) {
    normalize_nodes_keeping_breaks(cell, depth, merge_identifiers, true);
    // The cell already scopes a lone group's size.
    if let [MathNode::Row(inner)] = cell.as_mut_slice()
        && inner
            .iter()
            .any(|node| matches!(node, MathNode::SizeSwitch(_)))
        && !inner.contains(&MathNode::LineBreak)
    {
        *cell = std::mem::take(inner);
    }
    for node in cell {
        if matches!(node, MathNode::LineBreak) {
            *node = MathNode::Row(vec![MathNode::LineBreak]);
        }
    }
}

fn normalize_nodes_keeping_breaks(
    nodes: &mut Vec<MathNode>,
    depth: usize,
    merge_identifiers: bool,
    keep_break_groups: bool,
) {
    // Phase 1: recursively normalize each child
    for node in nodes.iter_mut() {
        normalize_node_depth_with_identifiers(node, depth, merge_identifiers);
    }

    // Phase 2: flatten Row children into the parent list
    let mut flattened = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        match node {
            MathNode::Row(children) if children.is_empty() => {
                // Phase 4: remove empty Rows
            },
            MathNode::Row(children)
                if is_idotsint_math(&children)
                    || children.iter().any(is_declaration_math)
                    || keep_break_groups && children.contains(&MathNode::LineBreak) =>
            {
                // Explicit groups containing a direct style declaration must
                // remain grouped so the declaration cannot leak to siblings.
                flattened.push(MathNode::Row(children));
            },
            MathNode::Row(children) => {
                // Flatten
                for child in children {
                    flattened.push(child);
                }
            },
            other => flattened.push(other),
        }
    }

    // Phase 3: merge adjacent Text nodes with the same style
    let mut merged = Vec::with_capacity(flattened.len());
    for node in flattened {
        if let Some((MathNode::Text(prev_style, prev_text), MathNode::Text(style, text))) =
            same_sizes(merged.last_mut(), &node)
            && prev_style == style
        {
            prev_text.push_str(text);
            continue;
        }
        merged.push(node);
    }

    // Phase 3b: merge adjacent Number nodes
    let mut final_nodes = Vec::with_capacity(merged.len());
    let mut number_dots = None;
    for node in merged {
        let node_dots = match without_sizes(&node) {
            MathNode::Number(num) => number_decimal_points(num),
            _ => None,
        };
        if let Some((MathNode::Number(prev_num), MathNode::Number(num))) =
            same_sizes(final_nodes.last_mut(), &node)
            && let (Some(previous_dots), Some(node_dots)) = (number_dots, node_dots)
            && previous_dots + node_dots <= 1
        {
            prev_num.push_str(num);
            number_dots = Some(previous_dots + node_dots);
            continue;
        }
        number_dots = node_dots;
        final_nodes.push(node);
    }

    // Phase 3c: merge adjacent ASCII Identifier nodes (math-mode letter runs).
    let merged_ids = if merge_identifiers {
        let mut merged_ids = Vec::with_capacity(final_nodes.len());
        for node in final_nodes {
            if let Some((MathNode::Identifier(prev_text), MathNode::Identifier(text))) =
                same_sizes(merged_ids.last_mut(), &node)
                && prev_text.is_ascii()
                && text.is_ascii()
            {
                prev_text.push_str(text);
                continue;
            }
            merged_ids.push(node);
        }
        merged_ids
    } else {
        final_nodes
    };

    // Phase 4: an empty-base script is a limit of a big operator before it
    // (unlike TeX, also for `\lim{}_x`), else a leading script of the next
    // node (see `normalize_compact_span`).
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
    *nodes = coalesced;
}

/// Adjacent number tokens can be merged only when their concatenation is a
/// token accepted by the LaTeX number parser. Keeping a second decimal point
/// in one node makes the writer emit an ambiguous token that reparses with a
/// different grouping.
pub(crate) fn can_merge_number_text(first: &str, second: &str) -> bool {
    match (number_decimal_points(first), number_decimal_points(second)) {
        (Some(first_dots), Some(second_dots)) => first_dots + second_dots <= 1,
        _ => false,
    }
}

/// Count decimal points in a number token whose text came from a parser or an
/// authored MathML `<mn>`. Non-decimal content stays in its own node and is
/// never merged with neighboring number nodes.
fn number_decimal_points(text: &str) -> Option<usize> {
    let mut dots = 0;
    for byte in text.bytes() {
        if byte == b'.' {
            dots += 1;
        } else if !byte.is_ascii_digit() {
            return None;
        }
    }
    Some(dots)
}

fn without_sizes(node: &MathNode) -> &MathNode {
    match node {
        MathNode::Attributed { content, .. } => content,
        node => node,
    }
}

/// Two neighbours without their typesetting attributes, if those are the same.
fn same_sizes<'a>(
    previous: Option<&'a mut MathNode>,
    node: &'a MathNode,
) -> Option<(&'a mut MathNode, &'a MathNode)> {
    match (previous?, node) {
        (
            MathNode::Attributed { attrs, content },
            MathNode::Attributed {
                attrs: own,
                content: next,
            },
        ) => (attrs == own).then_some((&mut **content, &**next)),
        (MathNode::Attributed { .. }, _) | (_, MathNode::Attributed { .. }) => None,
        (previous, node) => Some((previous, node)),
    }
}

fn is_effectively_empty_math(node: &MathNode) -> bool {
    match node {
        MathNode::Row(children) => children.iter().all(is_effectively_empty_math),
        // Empty styled and color wrappers are transparent when they occur as
        // script arguments. Explicit writable color bases are handled by the
        // separate empty-base predicate below.
        MathNode::Styled(_, children) => children.iter().all(is_effectively_empty_math),
        MathNode::Color { content, .. } => is_effectively_empty_math(content),
        MathNode::Scaled { content, .. } => is_effectively_empty_math(content),
        MathNode::Space(width) | MathNode::ExplicitSpace { width, .. } => width.num == 0,
        MathNode::Subscript { base, sub } => {
            is_effectively_empty_math(base) && is_effectively_empty_math(sub)
        },
        MathNode::Superscript { base, sup } => {
            is_effectively_empty_math(base) && is_effectively_empty_math(sup)
        },
        MathNode::SubSup { base, sub, sup } => {
            is_effectively_empty_math(base)
                && is_effectively_empty_math(sub)
                && is_effectively_empty_math(sup)
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            is_effectively_empty_math(base)
                && pre_sub.as_deref().is_none_or(is_effectively_empty_math)
                && pre_sup.as_deref().is_none_or(is_effectively_empty_math)
        },
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
            if is_empty_script_base_math(base)
    )
}

/// Whether a heap node is an implicit empty script base. Expressible colors
/// remain explicit atoms even when their content is empty, matching the
/// compact writer's color handling.
fn is_empty_script_base_math(node: &MathNode) -> bool {
    match node {
        MathNode::Row(children) => children.iter().all(is_empty_script_base_math),
        MathNode::Scaled { content, .. } => is_empty_script_base_math(content),
        MathNode::Identifier(name) => name.is_empty(),
        MathNode::Space(width) | MathNode::ExplicitSpace { width, .. } => width.num == 0,
        _ => false,
    }
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
    crate::unicode::symbols::operator_has_limits(name)
        || matches!(
            name,
            "lim inf"
                | "lim sup"
                | "arg\u{2009}max"
                | "arg\u{2009}min"
                | "arg max"
                | "arg min"
                | "inj lim"
                | "proj lim"
        )
}

/// Whether a function name such as sin or lim is spelled `name`.
pub(crate) fn is_function_name(name: &str) -> bool {
    is_limit_function(name) || crate::unicode::symbols::is_operator_command(name)
}

/// Whether a node takes a script that follows it: a big operator (as its
/// limit) or a function name (as its limit or its script). Mirrors
/// `takes_limits` for compact nodes.
fn takes_limits_math(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, _) => true,
        MathNode::Operator(_) | MathNode::StructuredOperator { .. } => true,
        MathNode::Identifier(name) | MathNode::Symbol(_, name) => is_function_name(name),
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        } => true,
        _ => false,
    }
}

fn is_plain_group(item: &FencedItem) -> bool {
    matches!(item, FencedItem::Element(MathNode::Row(nodes)) if is_plain_group_row(nodes))
}

/// A group that holds no declaration to keep inside it and is no `\idotsint`,
/// so that its nodes are nodes of the fence (none, for an empty one).
fn is_plain_group_row(nodes: &[MathNode]) -> bool {
    !nodes.iter().any(is_declaration_math) && !is_idotsint_math(nodes)
}

/// Heap form of `is_idotsint_row`.
fn is_idotsint_math(nodes: &[MathNode]) -> bool {
    let is = |node: &MathNode, kind, text: &str| matches!(node, MathNode::Symbol(k, t) if *k == kind && t == text);
    matches!(nodes, [first, dots, last]
        if is(first, SymbolKind::LargeOp, "\u{222B}")
            && is(dots, SymbolKind::Ordinary, "\u{22EF}")
            && is(last, SymbolKind::LargeOp, "\u{222B}"))
}

/// Whether a node can be the base of leading scripts. Mirrors
/// `can_take_prescripts` for compact nodes.
fn can_take_prescripts_math(node: &MathNode) -> bool {
    match node {
        MathNode::PreScripts { .. }
        | MathNode::Space(_)
        | MathNode::ExplicitSpace { .. }
        | MathNode::StyleSwitch(_)
        | MathNode::SizeSwitch(_)
        | MathNode::LineBreak => false,
        MathNode::Row(children) => !children.iter().any(is_declaration_math),
        _ => true,
    }
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

fn normalize_node_depth_with_identifiers(
    node: &mut MathNode,
    depth: usize,
    merge_identifiers: bool,
) {
    // Stop recursing if AST is pathologically deep
    if depth > super::MAX_AST_DEPTH {
        return;
    }
    let d = depth + 1;
    match node {
        // What carries attributes stays apart from its neighbours.
        MathNode::Attributed { content, .. } => {
            normalize_node_depth_with_identifiers(content, d, merge_identifiers);
        },
        MathNode::Row(children) => {
            normalize_nodes_depth_with_identifiers(children, d, merge_identifiers);
            // Unwrap single-child Row, except for an empty declaration group:
            // `{"\scriptstyle"}` must not leak into the following sibling.
            if children.len() == 1
                && !children.first().is_some_and(is_declaration_math)
                && let Some(child) = children.pop()
            {
                *node = child;
            }
        },

        MathNode::Fenced { content, .. } => {
            for item in content.iter_mut() {
                if let FencedItem::Element(n) = item {
                    normalize_node_depth_with_identifiers(n, d, merge_identifiers);
                }
            }
            // As in the compact normalizer, a group is flattened into the
            // fenced row.
            if content.iter().any(is_plain_group) {
                *content = std::mem::take(content)
                    .into_iter()
                    .flat_map(|item| match item {
                        FencedItem::Element(MathNode::Row(nodes)) if is_plain_group_row(&nodes) => {
                            nodes.into_iter().map(FencedItem::Element).collect()
                        },
                        item => vec![item],
                    })
                    .collect();
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
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(sub, d, merge_identifiers);
            flatten_empty_script_math(node);
        },
        MathNode::Superscript { base, sup } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(sup, d, merge_identifiers);
            flatten_empty_script_math(node);
        },
        MathNode::SubSup { base, sub, sup } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(sub, d, merge_identifiers);
            normalize_node_depth_with_identifiers(sup, d, merge_identifiers);
            flatten_empty_script_math(node);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            if let Some(s) = pre_sub {
                normalize_node_depth_with_identifiers(s, d, merge_identifiers);
            }
            if let Some(s) = pre_sup {
                normalize_node_depth_with_identifiers(s, d, merge_identifiers);
            }
        },

        MathNode::Overscript { base, over, .. } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(over, d, merge_identifiers);
        },
        MathNode::Underscript { base, under, .. } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(under, d, merge_identifiers);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
            normalize_node_depth_with_identifiers(under, d, merge_identifiers);
            normalize_node_depth_with_identifiers(over, d, merge_identifiers);
        },

        MathNode::Fraction { num, den, .. } => {
            normalize_node_depth_with_identifiers(num, d, merge_identifiers);
            normalize_node_depth_with_identifiers(den, d, merge_identifiers);
        },
        MathNode::Sqrt(inner) => {
            normalize_node_depth_with_identifiers(inner, d, merge_identifiers);
        },
        MathNode::Root { index, radicand } => {
            normalize_node_depth_with_identifiers(index, d, merge_identifiers);
            normalize_node_depth_with_identifiers(radicand, d, merge_identifiers);
        },

        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                normalize_node_depth_with_identifiers(s, d, merge_identifiers);
            }
            if let Some(s) = sup {
                normalize_node_depth_with_identifiers(s, d, merge_identifiers);
            }
            normalize_node_depth_with_identifiers(body, d, merge_identifiers);
        },

        MathNode::Accent { base, .. } => {
            normalize_node_depth_with_identifiers(base, d, merge_identifiers);
        },

        MathNode::Array { rows, .. }
        | MathNode::Matrix { rows, .. }
        | MathNode::NativeArray { rows, .. } => {
            for row in rows.iter_mut() {
                for cell in row.iter_mut() {
                    normalize_array_cell_math(cell, d, merge_identifiers);
                }
            }
        },
        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            // Preserve identifier atom boundaries while retaining every other
            // normalization phase for the cell's math list.
            match content.as_mut() {
                MathNode::Row(children) => {
                    normalize_nodes_depth_with_identifiers(children, d, false);
                },
                node => normalize_node_depth_with_identifiers(node, d, false),
            }
        },

        MathNode::Phantom(inner, _)
        | MathNode::Boxed { content: inner, .. }
        | MathNode::Actuarial { content: inner, .. } => {
            normalize_node_depth_with_identifiers(inner, d, merge_identifiers);
        },
        MathNode::Reflect { content: inner, .. } => {
            normalize_node_depth_with_identifiers(inner, d, merge_identifiers)
        },
        MathNode::Cancel { content, .. } => {
            normalize_node_depth_with_identifiers(content, d, merge_identifiers);
        },
        MathNode::Scaled { content, .. } | MathNode::RaiseBox { content, .. } => {
            normalize_node_depth_with_identifiers(content, d, merge_identifiers);
        },
        MathNode::Styled(_, children) => {
            normalize_nodes_depth_with_identifiers(children, d, merge_identifiers);
            // The font command's braces scope a lone style group (see the
            // compact normalizer).
            if let [MathNode::Row(inner)] = children.as_mut_slice()
                && inner.iter().any(is_declaration_math)
            {
                *children = std::mem::take(inner);
            }
        },
        MathNode::Color { content, .. } => {
            normalize_node_depth_with_identifiers(content, d, merge_identifiers)
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            normalize_node_depth_with_identifiers(display, d, merge_identifiers);
            normalize_node_depth_with_identifiers(text, d, merge_identifiers);
            normalize_node_depth_with_identifiers(script, d, merge_identifiers);
            normalize_node_depth_with_identifiers(script_script, d, merge_identifiers);
        },
        MathNode::MathAtom { content, .. } | MathNode::StructuredOperator { content, .. } => {
            normalize_node_depth_with_identifiers(content, d, merge_identifiers);
        },
        // The command itself is kept as written; its argument groups are
        // normalized as groups, never merged into each other or the row.
        MathNode::UndefinedCommand { arguments, .. } => {
            for argument in arguments {
                normalize_nodes_depth_with_identifiers(argument, d, merge_identifiers);
            }
        },

        // Leaf nodes: nothing to normalize
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
        | MathNode::LineBreak => {},
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

/// [`normalize_compact_expr`] without merging adjacent ASCII identifiers.
///
/// For readers whose identifiers are atoms of their own, such as MTEF, where
/// each character of a variable-style run is a separate italic variable: a
/// merged run would become one multi-letter (upright) identifier.
pub(crate) fn normalize_compact_expr_keeping_identifiers(
    expr: &mut CompactExpr,
) -> crate::error::Result<()> {
    expr.roots = normalize_compact_span_with_identifiers(&mut expr.arena, expr.roots, 0, false)?;
    expr.arena.check_limits()
}

/// Normalize a span of child nodes. Returns a (possibly new) span.
fn normalize_compact_span(
    arena: &mut AstArena,
    span: NodeSpan,
    depth: usize,
) -> crate::error::Result<NodeSpan> {
    normalize_compact_span_with_identifiers(arena, span, depth, true)
}

/// Normalize a span while preserving separate ASCII identifier atoms. Native
/// TeX groups supply one atom per letter, while a MathML multi-letter `<mi>`
/// supplies one upright identifier. The distinction matters only inside an
/// authored alignment cell.
fn normalize_compact_span_with_identifiers(
    arena: &mut AstArena,
    span: NodeSpan,
    depth: usize,
    merge_identifiers: bool,
) -> crate::error::Result<NodeSpan> {
    normalize_span_keeping_breaks(arena, span, depth, merge_identifiers, false)
}

/// The span normalization. With `keep_break_groups` (a table cell list), a
/// group that holds a line break is neither flattened nor unwrapped: in the
/// cell list the `\\` would be a row separator, which
/// `normalize_array_cell` replaces with a space.
fn normalize_span_keeping_breaks(
    arena: &mut AstArena,
    span: NodeSpan,
    depth: usize,
    merge_identifiers: bool,
    keep_break_groups: bool,
) -> crate::error::Result<NodeSpan> {
    if depth > super::MAX_AST_DEPTH {
        return Ok(span);
    }
    let d = depth + 1;

    // Phase 1: recursively normalize each child node.
    // Collect IDs first to avoid borrow conflicts with arena.
    let child_ids: SmallVec<[NodeId; 8]> = arena.children(span).into();
    for &id in &child_ids {
        normalize_compact_node_with_identifiers(arena, id, d, merge_identifiers)?;
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
            CompactNode::Row(inner) if has_direct_declaration(arena, inner) => {
                // Explicit groups containing a style declaration must remain
                // grouped so the declaration cannot leak into following
                // siblings when LaTeX is written back.
                flattened.push(id);
            },
            CompactNode::Row(inner) if keep_break_groups && has_direct_newline(arena, inner) => {
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

    // Phase 2b: drop zero-width spacing nodes. They produce no visible output
    // (e.g. a malformed/empty `\kern` or `\hspace{}`) and their presence can
    // block merges, suppress script attachment, or create/destroy separator
    // spaces in the writer.
    let splayed: SmallVec<[NodeId; 8]> = flattened
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
    let merged_ids = if merge_identifiers {
        merge_runs(arena, &final_nodes, |arena, node| match node {
            CompactNode::Identifier(sid) if arena.str_val(sid).is_ascii() => {
                Some((MergeKind::Identifier, sid))
            },
            _ => None,
        })
    } else {
        final_nodes
    };

    // Phase 4: an empty-base script belongs to what follows it (`{}^{14}C`,
    // an MTEF script template with tvSU_PRECEDES, MathML `mprescripts`,
    // OMML `m:sPre`): make it a leading script of the next node. Scripts the
    // LaTeX parser left without a base after white space (`| '`) are
    // attached to the preceding atom by the parser itself. After a big
    // operator or a function that takes limits the script is its limit
    // (`<mo>∫</mo><msub><mrow/><mi>C</mi></msub>`, as TeX-to-MathML
    // converters write `\int_C`). This is the one place an empty-base script
    // joins the node before it, and it departs from TeX: `\lim{}_x`,
    // `\max{}_x` and `\operatorname{f}{}_1` also become `\lim_x`, … (TeX
    // sets x after an empty atom). Any other script keeps its empty base
    // (`x{}_1`); the MTEF writer's `attaches_to_previous` mirrors this.
    let mut with_limits: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in &merged_ids {
        if is_empty_base_script(arena, id)
            && let Some(previous) = with_limits.last_mut()
            && takes_limits(arena, *arena.node(*previous))
        {
            *previous = make_script_with_base(arena, *previous, id);
            continue;
        }
        with_limits.push(id);
    }
    let mut coalesced: SmallVec<[NodeId; 8]> = SmallVec::new();
    for &id in with_limits.iter().rev() {
        if let Some(&next_id) = coalesced.last()
            && let Some(prescripts) = leading_scripts_on(arena, id, next_id)?
        {
            coalesced.pop();
            coalesced.push(prescripts);
            continue;
        }
        coalesced.push(id);
    }
    coalesced.reverse();

    arena.push_children(&coalesced)
}

/// The node that results from attaching the empty-base script `script` to
/// the following node `next`, or `None` when `next` cannot take leading
/// scripts.
///
/// A group is transparent to a leading script: a flattened group's scripts
/// lead its first node. A group that holds a direct line break may be kept
/// as a group instead: one in a table cell list (see
/// `normalize_span_keeping_breaks`), and one that also holds a style switch,
/// anywhere. The scripts lead the first node of such a kept group too, even
/// with a style switch (which refuses leading scripts on the group itself),
/// as the line break is checked first. So `{}^{a}{b\\c}` in a cell means
/// `{{}^{a}b}\\c` in one group; the writer spells it `{{{}^{a}b}\\ c}`. As
/// the base of a leading script the group would be written inside the
/// script's own group, where it is flattened on reading back.
fn leading_scripts_on(
    arena: &mut AstArena,
    script: NodeId,
    next: NodeId,
) -> crate::error::Result<Option<NodeId>> {
    let Some((pre_sub, pre_sup)) = empty_base_scripts(arena, script) else {
        return Ok(None);
    };
    if let CompactNode::Row(span) = *arena.node(next)
        && has_direct_newline(arena, span)
    {
        let mut children: SmallVec<[NodeId; 8]> = arena.children(span).into();
        let Some(&first) = children.first() else {
            return Ok(None);
        };
        let Some(led) = leading_scripts_on(arena, script, first)? else {
            return Ok(None);
        };
        children[0] = led;
        let span = arena.push_children(&children)?;
        return Ok(Some(arena.push_node(CompactNode::Row(span))));
    }
    let takes = can_take_prescripts(*arena.node(next))
        && !matches!(*arena.node(next), CompactNode::Row(span) if has_direct_declaration(arena, span))
        && !is_empty_base_script(arena, next);
    Ok(takes.then(|| {
        for part in [pre_sub, pre_sup].into_iter().filter(|part| part.is_some()) {
            arena.move_attrs(script, part);
        }
        arena.push_node(CompactNode::PreScripts {
            base: next,
            pre_sub,
            pre_sup,
        })
    }))
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
        let mut number_dots = (kind == MergeKind::Number)
            .then(|| number_decimal_points(arena.str_val(first_sid)))
            .flatten();
        while end < ids.len() {
            let Some((next_kind, next_sid)) = key(arena, *arena.node(ids[end])) else {
                break;
            };
            if next_kind != kind || arena.attrs(ids[end]) != arena.attrs(first) {
                break;
            }
            if kind == MergeKind::Number {
                let Some(next_dots) = number_decimal_points(arena.str_val(next_sid)) else {
                    break;
                };
                let Some(previous_dots) = number_dots else {
                    break;
                };
                if previous_dots + next_dots > 1 {
                    break;
                }
                number_dots = Some(previous_dots + next_dots);
            }
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
            *arena.node_mut_in_place(first) = match kind {
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
fn normalize_compact_node_with_identifiers(
    arena: &mut AstArena,
    id: NodeId,
    depth: usize,
    merge_identifiers: bool,
) -> crate::error::Result<()> {
    if depth > super::MAX_AST_DEPTH {
        return Ok(());
    }
    let d = depth + 1;

    let node = *arena.node(id);
    match node {
        CompactNode::Row(span) => {
            normalize_row_node(arena, id, span, d, merge_identifiers, false)?;
        },

        CompactNode::Styled(style, span) => {
            let new_span =
                normalize_compact_span_with_identifiers(arena, span, d, merge_identifiers)?;
            // The font command's braces already scope a declaration, and the
            // parser reads `\mathtt{{\displaystyle x}}` without the inner
            // group. Drop a lone style group here too (`\tt\dbinom ab`).
            let children = arena.children(new_span);
            let new_span = match children {
                [only] => match *arena.node(*only) {
                    CompactNode::Row(inner) if has_direct_declaration(arena, inner) => inner,
                    _ => new_span,
                },
                _ => new_span,
            };
            *arena.node_mut_in_place(id) = CompactNode::Styled(style, new_span);
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
            let new_content =
                normalize_compact_span_with_identifiers(arena, content, d, merge_identifiers)?;
            *arena.node_mut_in_place(id) = CompactNode::Fenced {
                open,
                close,
                content: new_content,
            };
        },

        CompactNode::Subscript { base, sub } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, sub, d, merge_identifiers)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::Superscript { base, sup } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, sup, d, merge_identifiers)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::SubSup { base, sub, sup } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, sub, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, sup, d, merge_identifiers)?;
            flatten_empty_script(arena, id);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            if pre_sub.is_some() {
                normalize_compact_node_with_identifiers(arena, pre_sub, d, merge_identifiers)?;
            }
            if pre_sup.is_some() {
                normalize_compact_node_with_identifiers(arena, pre_sup, d, merge_identifiers)?;
            }
            // Leading scripts stay leading scripts of their base. Without
            // scripts the node is its base; without a base it is an
            // empty-base script (one with nothing to precede).
            let sub = (pre_sub.is_some() && !is_effectively_empty_node(arena, pre_sub))
                .then_some(pre_sub);
            let sup = (pre_sup.is_some() && !is_effectively_empty_node(arena, pre_sup))
                .then_some(pre_sup);
            let replacement = match (sub, sup) {
                (None, None) => {
                    arena.replace_from(id, base);
                    return Ok(());
                },
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
            *arena.node_mut_in_place(id) = replacement;
        },

        CompactNode::Overscript { base, over, .. } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, over, d, merge_identifiers)?;
        },
        CompactNode::Underscript { base, under, .. } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, under, d, merge_identifiers)?;
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, under, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, over, d, merge_identifiers)?;
        },

        CompactNode::Fraction { num, den, .. } => {
            normalize_compact_node_with_identifiers(arena, num, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, den, d, merge_identifiers)?;
        },
        CompactNode::Sqrt(inner) => {
            normalize_compact_node_with_identifiers(arena, inner, d, merge_identifiers)?;
        },
        CompactNode::Root { index, radicand } => {
            normalize_compact_node_with_identifiers(arena, index, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, radicand, d, merge_identifiers)?;
        },

        CompactNode::Nary { sub, sup, body, .. } => {
            if sub.is_some() {
                normalize_compact_node_with_identifiers(arena, sub, d, merge_identifiers)?;
            }
            if sup.is_some() {
                normalize_compact_node_with_identifiers(arena, sup, d, merge_identifiers)?;
            }
            normalize_compact_node_with_identifiers(arena, body, d, merge_identifiers)?;
        },

        CompactNode::Accent { base, .. } => {
            normalize_compact_node_with_identifiers(arena, base, d, merge_identifiers)?;
        },

        CompactNode::Array { cells, .. } => {
            // Each cell is a Row node — normalize it, then keep internal line
            // breaks inside groups. A bare `\\`/`\cr` in a cell list is always
            // a row separator on re-parse, so a braced line break (`{a\\b}`),
            // a break within the cell, keeps its group, and a direct break is
            // put in one (`normalize_array_cell`).
            let cell_ids: SmallVec<[NodeId; 8]> = arena.children(cells).into();
            let mut new_cells: SmallVec<[NodeId; 8]> = SmallVec::new();
            for &cell_id in &cell_ids {
                if let CompactNode::Row(span) = *arena.node(cell_id) {
                    // The depth a nested `normalize_compact_node` call uses.
                    normalize_row_node(arena, cell_id, span, d + 1, merge_identifiers, true)?;
                } else {
                    normalize_compact_node_with_identifiers(arena, cell_id, d, merge_identifiers)?;
                }
                let new_id = normalize_array_cell(arena, cell_id)?;
                new_cells.push(new_id);
            }
            let new_cells_span = arena.push_children(&new_cells)?;
            if let CompactNode::Array { cells: c, .. } = arena.node_mut_in_place(id) {
                *c = new_cells_span;
            }
        },
        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            // Preserve separate TeX identifier atoms within the whole cell
            // math list. Other normalization phases still run, including
            // nested row flattening, number/text merging and script
            // attachment.
            if let CompactNode::Row(span) = *arena.node(content) {
                let new_span = normalize_compact_span_with_identifiers(arena, span, d, false)?;
                *arena.node_mut_in_place(content) = CompactNode::Row(new_span);
            } else {
                normalize_compact_node_with_identifiers(arena, content, d, false)?;
            }
        },

        CompactNode::Phantom(inner, _)
        | CompactNode::Boxed { content: inner, .. }
        | CompactNode::Actuarial { content: inner, .. } => {
            normalize_compact_node_with_identifiers(arena, inner, d, merge_identifiers)?;
        },
        CompactNode::Reflect { content: inner, .. } => {
            normalize_compact_node_with_identifiers(arena, inner, d, merge_identifiers)?;
        },
        CompactNode::Cancel { content, .. } | CompactNode::RaiseBox { content, .. } => {
            normalize_compact_node_with_identifiers(arena, content, d, merge_identifiers)?;
        },
        CompactNode::Scaled { content, .. } => {
            normalize_compact_node_with_identifiers(arena, content, d, merge_identifiers)?;
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
                    *arena.node_mut_in_place(id) = CompactNode::Row(new_span);
                    arena.settle_attrs(id);
                },
                _ => {
                    arena.replace_from(id, content);
                },
            }
        },
        CompactNode::Color { content, .. } => {
            normalize_compact_node_with_identifiers(arena, content, d, merge_identifiers)?;
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            normalize_compact_node_with_identifiers(arena, display, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, text, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, script, d, merge_identifiers)?;
            normalize_compact_node_with_identifiers(arena, script_script, d, merge_identifiers)?;
        },
        CompactNode::MathAtom { content, .. } | CompactNode::StructuredOperator { content, .. } => {
            normalize_compact_node_with_identifiers(arena, content, d, merge_identifiers)?;
        },
        // The command itself is kept as written; its argument groups are
        // normalized as groups, never merged into each other or the row.
        CompactNode::UndefinedCommand { arguments, .. } => {
            let groups: SmallVec<[NodeId; 4]> = arena.children(arguments).into();
            for group in groups {
                if let CompactNode::Row(span) = *arena.node(group) {
                    let span =
                        normalize_compact_span_with_identifiers(arena, span, d, merge_identifiers)?;
                    *arena.node_mut_in_place(group) = CompactNode::Row(span);
                }
            }
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
                arena.replace_from(id, new_id);
                return;
            } else if sup_empty {
                let new_id = arena.push_node(CompactNode::Subscript { base, sub });
                arena.replace_from(id, new_id);
                return;
            } else {
                return;
            }
        },
        _ => return,
    };
    arena.replace_from(id, base);
}

/// True if `id` refers to a node that produces no output when written.
/// Currently covers empty Rows and sizing wrappers (\big, \Big, ...) whose
/// content is itself empty — both render as nothing in the output stream.
pub(crate) fn is_effectively_empty_node(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    match *arena.node(id) {
        CompactNode::Row(span) => arena
            .children(span)
            .iter()
            .all(|&child| is_effectively_empty_node(arena, child)),
        CompactNode::Scaled { content, .. } => is_effectively_empty_node(arena, content),
        CompactNode::Space(r) => r == Rational::zero(),
        CompactNode::Styled(_, span) => arena
            .children(span)
            .iter()
            .all(|&child| is_effectively_empty_node(arena, child)),
        // A color wrapper with no visible content is transparent to every
        // writer. Recognize it here so an empty colored prescript is removed
        // before a following empty-base script is classified.
        CompactNode::Color { content, .. } => is_effectively_empty_node(arena, content),
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

fn has_direct_declaration(arena: &AstArena, span: NodeSpan) -> bool {
    arena
        .children(span)
        .iter()
        .any(|&id| is_declaration(*arena.node(id)))
}

/// A style or size switch, whose group must not be opened: it would apply to
/// the siblings after the group.
fn is_declaration(node: CompactNode) -> bool {
    matches!(
        node,
        CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_)
    )
}

/// Heap counterpart of [`is_declaration`].
fn is_declaration_math(node: &MathNode) -> bool {
    matches!(node, MathNode::StyleSwitch(_) | MathNode::SizeSwitch(_))
}

/// Normalize a `Row` node in place: normalize its list, then unwrap a single
/// child. With `keep_break_groups` (see `normalize_span_keeping_breaks`), a
/// lone group holding a line break stays a group.
fn normalize_row_node(
    arena: &mut AstArena,
    id: NodeId,
    span: NodeSpan,
    depth: usize,
    merge_identifiers: bool,
    keep_break_groups: bool,
) -> crate::error::Result<()> {
    let new_span =
        normalize_span_keeping_breaks(arena, span, depth, merge_identifiers, keep_break_groups)?;
    let children = arena.children(new_span);
    let keep_group = |child: NodeId| match *arena.node(child) {
        CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_) => true,
        CompactNode::Row(inner) => keep_break_groups && has_direct_newline(arena, inner),
        _ => false,
    };
    if let [only] = children
        && !keep_group(*only)
    {
        // Unwrap single-child Row: replace this node with its child
        arena.replace_from(id, *only);
    } else {
        *arena.node_mut_in_place(id) = CompactNode::Row(new_span);
    }
    Ok(())
}

fn has_direct_newline(arena: &AstArena, span: NodeSpan) -> bool {
    arena
        .children(span)
        .iter()
        .any(|&id| matches!(*arena.node(id), CompactNode::Newline))
}

/// Source whitespace is consumed by the parser. Keep authored math spaces,
/// including edge kerns. A line break stored as direct cell content would be
/// a row separator when written, so it becomes a braced break (`{\\}`, which
/// the parser reads back as such a direct break) within the cell.
fn normalize_array_cell(arena: &mut AstArena, cell_id: NodeId) -> crate::error::Result<NodeId> {
    if matches!(arena.node(cell_id), CompactNode::Newline) {
        // A cell that is only a break, unwrapped from its one-child list.
        let group = arena.push_children(&[cell_id])?;
        let group = arena.push_node(CompactNode::Row(group));
        let cell = arena.push_children(&[group])?;
        return Ok(arena.push_node(CompactNode::Row(cell)));
    }
    let CompactNode::Row(span) = *arena.node(cell_id) else {
        return Ok(cell_id);
    };
    let children = arena.children(span);
    let Some(first_break) = children
        .iter()
        .position(|&id| matches!(arena.node(id), CompactNode::Newline))
    else {
        return Ok(cell_id);
    };
    let mut children: SmallVec<[NodeId; 8]> = children.into();
    for id in &mut children[first_break..] {
        if matches!(arena.node(*id), CompactNode::Newline) {
            let span = arena.push_children(&[*id])?;
            *id = arena.push_node(CompactNode::Row(span));
        }
    }
    let span = arena.push_children(&children)?;
    Ok(arena.push_node(CompactNode::Row(span)))
}

/// True if `id` is a script node whose base is effectively empty.  Such nodes
/// arise when a sub/superscript marker is not attached to an explicit base
/// during parsing (e.g. `sm\Big'` where `\Big` has no content).
pub(crate) fn is_empty_base_script(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => is_empty_script_base(arena, base),
        _ => false,
    }
}

/// True for an implicit empty script base. An expressible color remains
/// an explicit atom even when its body is empty.
fn is_empty_script_base(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    match *arena.node(id) {
        CompactNode::Row(span) => arena
            .children(span)
            .iter()
            .all(|&child| is_empty_script_base(arena, child)),
        CompactNode::Scaled { content, .. } => is_empty_script_base(arena, content),
        // An empty `<mi/>` base writes nothing and reads back as `{}`.
        CompactNode::Identifier(sid) => arena.str_val(sid).is_empty(),
        CompactNode::Space(r) => r == Rational::zero(),
        _ => false,
    }
}

/// True for a node that can serve as the base of a sub/superscript.
///
/// Script-marker nodes and spacing helpers are excluded because they cannot
/// serve as a stable base. Fence separators are allowed here because a
/// following empty-base script is a script on that separator in the current
/// row, and retaining the helper node would make writer passes disagree.
pub(crate) fn can_carry_script(node: CompactNode) -> bool {
    !matches!(
        node,
        CompactNode::Subscript { .. }
            | CompactNode::Superscript { .. }
            | CompactNode::SubSup { .. }
            | CompactNode::PreScripts { .. }
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
        CompactNode::Operator { .. } | CompactNode::StructuredOperator { .. } => true,
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
    // The base keeps its own size: the script's is its scripts'.
    if let Some((sub, sup)) = empty_base_scripts(arena, script_id) {
        for part in [sub, sup].into_iter().filter(|part| part.is_some()) {
            arena.move_attrs(script_id, part);
        }
    }
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
    fn normalization_copies_explicit_space_basis_with_the_node() {
        let space = MathNode::ExplicitSpace {
            width: super::super::Rational::new(1, 2),
            basis: super::super::SpaceBasis::MathMu,
        };
        for n in [
            MathNode::Row(vec![space.clone()]),
            MathNode::Superscript {
                base: Box::new(space.clone()),
                sup: Box::new(MathNode::Row(Vec::new())),
            },
            MathNode::PreScripts {
                base: Box::new(space.clone()),
                pre_sub: None,
                pre_sup: None,
            },
        ] {
            let mut compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![n])).unwrap();
            normalize_compact_expr(&mut compact).unwrap();
            assert_eq!(compact.to_math_expr().nodes, vec![space.clone()]);
        }
    }

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
    #[test]
    fn normalize_heap_elides_empty_styled_prescript_before_following_script() {
        let mut expr = MathExpr::inline(vec![
            MathNode::Superscript {
                base: Box::new(MathNode::Row(Vec::new())),
                sup: Box::new(MathNode::Styled(TextStyle::Normal, Vec::new())),
            },
            MathNode::Identifier("x".into()),
            MathNode::Superscript {
                base: Box::new(MathNode::Row(Vec::new())),
                sup: Box::new(MathNode::Number("2".into())),
            },
        ]);
        normalize_expr(&mut expr);
        // The trailing `{}^2` keeps its own empty base (TeX's `x{}^2`).
        assert_eq!(
            expr.nodes,
            vec![
                MathNode::Identifier("x".into()),
                MathNode::Superscript {
                    base: Box::new(MathNode::Row(Vec::new())),
                    sup: Box::new(MathNode::Number("2".into())),
                }
            ]
        );

        assert_normalize_equiv(MathExpr::inline(vec![
            MathNode::Superscript {
                base: Box::new(MathNode::Row(Vec::new())),
                sup: Box::new(MathNode::Styled(TextStyle::Normal, Vec::new())),
            },
            MathNode::Identifier("x".into()),
            MathNode::Superscript {
                base: Box::new(MathNode::Row(Vec::new())),
                sup: Box::new(MathNode::Number("2".into())),
            },
        ]));
    }

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
