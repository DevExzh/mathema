use super::*;

/// TeXBook Rule 18a: superscript positioning.
/// When the base is a large operator in display mode, place the superscript above as a limit.
pub(super) fn layout_superscript(base: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator or named operator with limits: place superscript above
    if wants_limits_above_below(base, ctx.branch_style()) {
        if is_large_op(base) {
            return layout_big_op_limits_heap(base, None, Some(sup), ctx);
        }
        return layout_op_limits_above_below_heap(base, None, Some(sup), ctx);
    }

    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop, scaled by child/parent size ratio
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    // Superscript shift: max of sup1/sup2/sup3, base.height - sup_drop, x_height/4 + sup.depth
    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(script_nucleus(base, &base_box).kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    let total_width = base_box.width + sup_box.width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    // Capture dimensions before moving base_box into children vec (avoids clone)
    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 18a: subscript positioning.
/// When the base is a large operator in display mode, place the subscript below as a limit.
pub(super) fn layout_subscript(base: &MathNode, sub: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator or named operator with limits: place subscript below
    if wants_limits_above_below(base, ctx.branch_style()) {
        if is_large_op(base) {
            return layout_big_op_limits_heap(base, Some(sub), None, ctx);
        }
        return layout_op_limits_above_below_heap(base, Some(sub), None, ctx);
    }

    let base_box = layout_node(base, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sub_drop, scaled by child/parent size ratio
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(script_nucleus(base, &base_box).kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height * size / 5.0));

    // Rule 18: a subscript goes where the base's advance ends, without the
    // italic correction its box includes.
    let ic = italic_correction(script_nucleus(base, &base_box));
    let sub_dx = (base_box.width - ic).max(0.0);
    let total_width = sub_dx + sub_box.width + SCRIPT_SPACE * size;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 18c: combined sub+superscript.
/// When the base is a large operator in display mode, place limits above/below.
pub(super) fn layout_subsup(
    base: &MathNode,
    sub: &MathNode,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Large operator or named operator with limits: place above/below
    if wants_limits_above_below(base, ctx.branch_style()) {
        if is_large_op(base) {
            return layout_big_op_limits_heap(base, Some(sub), Some(sup), ctx);
        }
        return layout_op_limits_above_below_heap(base, Some(sub), Some(sup), ctx);
    }
    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    let size = ctx.size();

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(script_nucleus(base, &base_box).kind, BoxKind::Glyph { .. });

    // Superscript shift
    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    // Subscript shift
    let mut shift_down = p.sub2 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    // KaTeX puts the entire gap adjustment into the subscript, then checks sup bottom vs xHeight
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        // Push subscript down to meet minimum gap (entire adjustment into sub)
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    // Then check if sup bottom is above 0.8 * xHeight
    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
        // Safety: ensure subscript stays below baseline
        shift_down = shift_down.max(0.0);
    }

    let ic = italic_correction(script_nucleus(base, &base_box));
    let sup_dx = base_box.width;
    let sub_dx = (base_box.width - ic).max(0.0);

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: superscript layout where the base is an already-laid-out LayoutBox (heap path).
pub(super) fn layout_superscript_from_box_heap(
    base_box: LayoutBox,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    let total_width = base_box.width + sup_box.width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: subscript layout where the base is an already-laid-out LayoutBox (heap path).
pub(super) fn layout_subscript_from_box_heap(
    base_box: LayoutBox,
    base_ic: f64,
    sub: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height * size / 5.0));

    let sub_dx = base_box.width - base_ic;

    let total_width = sub_dx + sub_box.width + SCRIPT_SPACE * size;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: combined sub+superscript layout where the base is an already-laid-out LayoutBox (heap path).
pub(super) fn layout_subsup_from_box_heap(
    base_box: LayoutBox,
    base_ic: f64,
    sub: &MathNode,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    let size = ctx.size();

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    let mut shift_down = p.sub2 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
        // Safety: ensure subscript stays below baseline
        shift_down = shift_down.max(0.0);
    }

    let sup_dx = base_box.width;
    let sub_dx = base_box.width - base_ic;

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// The box of the nucleus a leading script belongs to: `base` without the
/// trailing scripts beside it, whose box is the first child of the scripted
/// one. TeX sets `{}_a t_b` as two scripts, each placed by its own nucleus
/// alone. That holds for both shifts: where the trailing scripts of `t_b^c`
/// push each other apart, the leading one keeps its place, as in TeX, and is
/// not level with them.
fn nucleus_box<'a>(base: &MathNode, base_box: &'a LayoutBox, ctx: &LayoutCtx) -> &'a LayoutBox {
    let (mut node, mut current) = (base, base_box);
    loop {
        // Limits are stacked, not scripts beside the base.
        let inner = match node {
            MathNode::Subscript { base, .. }
            | MathNode::Superscript { base, .. }
            | MathNode::SubSup { base, .. } => base,
            _ => return current,
        };
        if wants_limits_above_below(inner, ctx.branch_style()) {
            return current;
        }
        let BoxKind::HBox { children } = &current.kind else {
            return current;
        };
        let Some(first) = children.first() else {
            return current;
        };
        (node, current) = (inner, &first.layout);
    }
}

/// The box of the one character a base is, which scripts and accents attach to:
/// TeX takes a group of one for its content (`\mathfrak{K}_1`; not a coloured
/// one), and TeXBook Rule 12 puts the scripts of an accented character on the
/// character.
pub(super) fn script_nucleus<'a>(base: &MathNode, base_box: &'a LayoutBox) -> &'a LayoutBox {
    let (mut node, mut current) = (base, base_box);
    loop {
        (node, current) = match (node, &current.kind) {
            (MathNode::Attributed { content, .. }, _)
            | (
                MathNode::MathAtom {
                    atom_type: MathAtomType::Ord,
                    content,
                },
                _,
            ) => (&**content, current),
            (
                MathNode::Accent {
                    base,
                    position: AccentPosition::Above,
                    ..
                },
                BoxKind::HBox { children },
            ) if !children.is_empty() => (&**base, &children[0].layout),
            (MathNode::Styled(_, nodes) | MathNode::Row(nodes), BoxKind::HBox { children })
                if nodes.len() == 1 && children.len() == 1 =>
            {
                (&nodes[0], &children[0].layout)
            },
            (_, BoxKind::Glyph { .. }) => return current,
            _ => return base_box,
        };
    }
}

/// Compact-AST version of `script_nucleus`.
pub(super) fn script_nucleus_arena(
    ast: &AstArena,
    base: NodeId,
    base_id: LayoutBoxId,
    arena: &LayoutArena,
) -> LayoutBoxId {
    let (mut node, mut current) = (base, base_id);
    loop {
        (node, current) = match (*ast.node(node), &arena.get(current).kind) {
            (
                CompactNode::MathAtom {
                    atom_type: MathAtomType::Ord,
                    content,
                },
                _,
            ) => (content, current),
            (
                CompactNode::Accent {
                    base,
                    position: AccentPosition::Above,
                    ..
                },
                &ArenaBoxKind::HBox { children },
            ) if !arena.children(children).is_empty() => (base, arena.children(children)[0].id),
            (
                CompactNode::Styled(_, span) | CompactNode::Row(span),
                &ArenaBoxKind::HBox { children },
            ) if ast.children(span).len() == 1 && arena.children(children).len() == 1 => {
                (ast.children(span)[0], arena.children(children)[0].id)
            },
            (_, ArenaBoxKind::Glyph { .. }) => return current,
            _ => return base_id,
        };
    }
}

/// Compact-AST version of `nucleus_box`.
fn nucleus_box_arena(
    ast: &AstArena,
    base: NodeId,
    base_id: LayoutBoxId,
    arena: &LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let (mut node, mut current) = (base, base_id);
    loop {
        let inner = match *ast.node(node) {
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. } => base,
            _ => return current,
        };
        if wants_limits_above_below_c(ast, inner, ctx.branch_style()) {
            return current;
        }
        let ArenaBoxKind::HBox { children } = arena.get(current).kind else {
            return current;
        };
        let Some(first) = arena.children(children).first() else {
            return current;
        };
        (node, current) = (inner, first.id);
    }
}

pub(super) fn layout_prescripts(
    base: &MathNode,
    pre_sub: &Option<Box<MathNode>>,
    pre_sup: &Option<Box<MathNode>>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let p = &ctx.params;
    let size = ctx.size();

    let mut pre_width: f64 = 0.0;
    let mut total_h = base_box.height;
    let mut total_d = base_box.depth;

    let nucleus = nucleus_box(base, &base_box, ctx);
    let (nucleus_h, nucleus_d) = (nucleus.height, nucleus.depth);
    let is_char_box = matches!(nucleus.kind, BoxKind::Glyph { .. });
    let has_both = pre_sup.is_some() && pre_sub.is_some();

    // Layout pre-superscript
    let sup_data = if let Some(sup) = pre_sup {
        let sup_style = ctx.style.sup();
        let sup_params = TexParams::for_size_class(sup_style.size_class());
        let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
        let sup_ctx = ctx.with_style(sup_style);
        let sup_box = layout_node(sup, &sup_ctx);
        let mut shift_up = p.sup2 * size;
        if !is_char_box {
            shift_up = shift_up.max(nucleus_h - sup_params.sup_drop * sup_size_ratio * size);
        }
        pre_width = pre_width.max(sup_box.width);
        Some((sup_box, shift_up))
    } else {
        None
    };

    // Layout pre-subscript; use sub2 when both scripts present (Rule 18d)
    let sub_data = if let Some(sub) = pre_sub {
        let sub_style = ctx.style.sub();
        let sub_params = TexParams::for_size_class(sub_style.size_class());
        let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();
        let sub_ctx = ctx.with_style(sub_style);
        let sub_box = layout_node(sub, &sub_ctx);
        let mut shift_down = if has_both { p.sub2 } else { p.sub1 } * size;
        if !is_char_box {
            shift_down = shift_down.max(nucleus_d + sub_params.sub_drop * sub_size_ratio * size);
        }
        pre_width = pre_width.max(sub_box.width);
        Some((sub_box, shift_down))
    } else {
        None
    };

    // Rule 18e: ensure minimum gap between pre-sub top and pre-sup bottom
    let (sup_data, sub_data) = match (sup_data, sub_data) {
        (Some((sup_box, mut shift_up)), Some((sub_box, mut shift_down))) => {
            let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
            let min_gap = 4.0 * p.rule_thickness * size;
            if gap < min_gap {
                shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
            }
            let psi = 0.8 * p.x_height * size - (shift_up - sup_box.depth);
            if psi > 0.0 {
                shift_up += psi;
                shift_down -= psi;
                shift_down = shift_down.max(0.0);
            }
            (Some((sup_box, shift_up)), Some((sub_box, shift_down)))
        },
        other => other,
    };

    let mut children = Vec::new();
    // Right-align pre-scripts within the pre-column (TeX convention)
    if let Some((sup_box, shift_up)) = sup_data {
        total_h = total_h.max(shift_up + sup_box.height);
        let dx = pre_width - sup_box.width;
        children.push(PositionedBox::new(dx, shift_up, sup_box));
    }
    if let Some((sub_box, shift_down)) = sub_data {
        total_d = total_d.max(shift_down + sub_box.depth);
        let dx = pre_width - sub_box.width;
        children.push(PositionedBox::new(dx, -shift_down, sub_box));
    }

    let base_width = base_box.width;
    let kern = SCRIPT_SPACE * size;
    children.push(PositionedBox::new(pre_width + kern, 0.0, base_box));

    LayoutBox {
        width: pre_width + kern + base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

pub(super) fn layout_superscript_arena(
    ast: &AstArena,
    base: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator or named operator with limits: place superscript above
    if wants_limits_above_below_c(ast, base, ctx.branch_style()) {
        if is_large_op_c(ast, base) {
            return layout_big_op_limits_arena(ast, base, None, Some(sup), arena, ctx);
        }
        return layout_op_limits_above_below_arena(ast, base, None, Some(sup), arena, ctx);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;

    // Bug 2 fix: check if base is a single character
    let nucleus = script_nucleus_arena(ast, base, base_id, arena);
    let is_char_box = matches!(arena.get(nucleus).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let total_width = base_w + sup_w + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: base_w,
        dy: shift_up,
        id: sup_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

pub(super) fn layout_subscript_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator or named operator with limits: place subscript below
    if wants_limits_above_below_c(ast, base, ctx.branch_style()) {
        if is_large_op_c(ast, base) {
            return layout_big_op_limits_arena(ast, base, Some(sub), None, arena, ctx);
        }
        return layout_op_limits_above_below_arena(ast, base, Some(sub), None, arena, ctx);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(script_nucleus_arena(ast, base, base_id, arena), arena);
    let sub_dx = (base_w - ic).max(0.0);

    // Bug 2 fix: check if base is a single character
    let nucleus = script_nucleus_arena(ast, base, base_id, arena);
    let is_char_box = matches!(arena.get(nucleus).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_down = p.sub1 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio * size);
    }
    let shift_down = shift_down.max(sub_h - (4.0 * p.x_height * size / 5.0));

    let total_width = sub_dx + sub_w + SCRIPT_SPACE * size;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sub_dx,
        dy: -shift_down,
        id: sub_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

pub(super) fn layout_subsup_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator or named operator with limits: place above/below
    if wants_limits_above_below_c(ast, base, ctx.branch_style()) {
        if is_large_op_c(ast, base) {
            return layout_big_op_limits_arena(ast, base, Some(sub), Some(sup), arena, ctx);
        }
        return layout_op_limits_above_below_arena(ast, base, Some(sub), Some(sup), arena, ctx);
    }
    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(script_nucleus_arena(ast, base, base_id, arena), arena);
    // Bug 2 fix: check if base is a single character
    let nucleus = script_nucleus_arena(ast, base, base_id, arena);
    let is_char_box = matches!(arena.get(nucleus).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let mut shift_down = p.sub2 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio * size);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_d) + sub_h;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_d);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
        // Safety: ensure subscript stays below baseline
        shift_down = shift_down.max(0.0);
    }

    let sup_dx = base_w;
    let sub_dx = (base_w - ic).max(0.0);

    let script_width = (sup_dx - base_w + sup_w)
        .max(sub_dx - base_w + sub_w)
        .max(0.0);
    let total_width = base_w + script_width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sup_dx,
        dy: shift_up,
        id: sup_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sub_dx,
        dy: -shift_down,
        id: sub_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: superscript layout where the base is an already-laid-out arena box.
pub(super) fn layout_superscript_from_box_arena(
    base_id: LayoutBoxId,
    ast: &AstArena,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let total_width = base_w + sup_w + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: base_w,
        dy: shift_up,
        id: sup_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subscript layout where the base is an already-laid-out arena box.
pub(super) fn layout_subscript_from_box_arena(
    base_id: LayoutBoxId,
    base_ic: f64,
    ast: &AstArena,
    sub: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let sub_dx = base_w - base_ic;

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_down = p.sub1 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio * size);
    }
    let shift_down = shift_down.max(sub_h - (4.0 * p.x_height * size / 5.0));

    let total_width = sub_dx + sub_w + SCRIPT_SPACE * size;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sub_dx,
        dy: -shift_down,
        id: sub_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subsup layout where the base is an already-laid-out arena box.
pub(super) fn layout_subsup_from_box_arena(
    base_id: LayoutBoxId,
    base_ic: f64,
    ast: &AstArena,
    sub: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_cramped() {
        p.sup3 * size
    } else if ctx.branch_style().is_display() {
        p.sup1 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let mut shift_down = p.sub2 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio * size);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_d) + sub_h;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_d);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
        // Safety: ensure subscript stays below baseline
        shift_down = shift_down.max(0.0);
    }

    let sup_dx = base_w;
    let sub_dx = base_w - base_ic;

    let script_width = (sup_dx - base_w + sup_w)
        .max(sub_dx - base_w + sub_w)
        .max(0.0);
    let total_width = base_w + script_width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sup_dx,
        dy: shift_up,
        id: sup_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: sub_dx,
        dy: -shift_down,
        id: sub_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

pub(super) fn layout_prescripts_arena(
    ast: &AstArena,
    base: NodeId,
    pre_sub: NodeId,
    pre_sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let base_id = layout_node_arena(ast, base, arena, ctx);
    let p = &ctx.params;
    let size = ctx.size();

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let mut pre_width: f64 = 0.0;
    let mut total_h = base_h;
    let mut total_d = base_d;

    let nucleus = arena.get(nucleus_box_arena(ast, base, base_id, arena, ctx));
    let (nucleus_h, nucleus_d) = (nucleus.height, nucleus.depth);
    let is_char_box = matches!(nucleus.kind, ArenaBoxKind::Glyph { .. });
    let has_both = pre_sup.is_some() && pre_sub.is_some();

    // Layout pre-superscript
    let mut pre_sup_id = if pre_sup.is_some() {
        let sup_style = ctx.style.sup();
        let sup_params = TexParams::for_size_class(sup_style.size_class());
        let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
        let sup_ctx = ctx.with_style(sup_style);
        let sid = layout_node_arena(ast, pre_sup, arena, &sup_ctx);
        let b = arena.get(sid);
        let mut shift_up = p.sup2 * size;
        if !is_char_box {
            shift_up = shift_up.max(nucleus_h - sup_params.sup_drop * sup_size_ratio * size);
        }
        pre_width = pre_width.max(b.width);
        Some((sid, shift_up))
    } else {
        None
    };

    // Layout pre-subscript; use sub2 when both scripts present (Rule 18d)
    let mut pre_sub_id = if pre_sub.is_some() {
        let sub_style = ctx.style.sub();
        let sub_params = TexParams::for_size_class(sub_style.size_class());
        let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();
        let sub_ctx = ctx.with_style(sub_style);
        let sid = layout_node_arena(ast, pre_sub, arena, &sub_ctx);
        let b = arena.get(sid);
        let mut shift_down = if has_both { p.sub2 } else { p.sub1 } * size;
        if !is_char_box {
            shift_down = shift_down.max(nucleus_d + sub_params.sub_drop * sub_size_ratio * size);
        }
        pre_width = pre_width.max(b.width);
        Some((sid, shift_down))
    } else {
        None
    };

    // Rule 18e: ensure minimum gap between pre-sub top and pre-sup bottom
    if let (Some((sup_sid, shift_up)), Some((sub_sid, shift_down))) =
        (&mut pre_sup_id, &mut pre_sub_id)
    {
        let sup_depth = arena.get(*sup_sid).depth;
        let sub_height = arena.get(*sub_sid).height;
        let gap = (*shift_up - sup_depth) - (sub_height - *shift_down);
        let min_gap = 4.0 * p.rule_thickness * size;
        if gap < min_gap {
            *shift_down = min_gap - (*shift_up - sup_depth) + sub_height;
        }
        let psi = 0.8 * p.x_height * size - (*shift_up - sup_depth);
        if psi > 0.0 {
            *shift_up += psi;
            *shift_down -= psi;
            *shift_down = (*shift_down).max(0.0);
        }
    }

    let children_start = arena.begin_children();
    // Right-align pre-scripts within the pre-column (TeX convention)
    if let Some((sid, shift_up)) = pre_sup_id {
        total_h = total_h.max(shift_up + arena.get(sid).height);
        let dx = pre_width - arena.get(sid).width;
        arena.push_child(ArenaPositionedBox {
            dx,
            dy: shift_up,
            id: sid,
        });
    }
    if let Some((sid, shift_down)) = pre_sub_id {
        total_d = total_d.max(shift_down + arena.get(sid).depth);
        let dx = pre_width - arena.get(sid).width;
        arena.push_child(ArenaPositionedBox {
            dx,
            dy: -shift_down,
            id: sid,
        });
    }
    let kern = SCRIPT_SPACE * size;
    arena.push_child(ArenaPositionedBox {
        dx: pre_width + kern,
        dy: 0.0,
        id: base_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: pre_width + kern + base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}
