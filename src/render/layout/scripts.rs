use super::*;

/// Returns true if the superscript node consists entirely of prime symbols (U+2032).
/// TeX skips italic correction for prime superscripts so they sit tight against the base.
pub(super) fn is_prime_sup(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(_, s) => s.chars().all(|c| c == '\u{2032}'),
        MathNode::Row(children) => !children.is_empty() && children.iter().all(is_prime_sup),
        _ => false,
    }
}

/// Compact-AST version of `is_prime_sup`.
pub(super) fn is_prime_sup_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Symbol(_, sid) => arena.str_val(sid).chars().all(|c| c == '\u{2032}'),
        CompactNode::Row(span) => {
            let ids = arena.children(span);
            !ids.is_empty() && ids.iter().all(|&nid| is_prime_sup_c(arena, nid))
        },
        _ => false,
    }
}

/// TeXBook Rule 18a: superscript positioning.
/// When the base is a large operator in display mode, place the superscript above as a limit.
pub(super) fn layout_superscript(base: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator or named operator with limits: place superscript above
    if wants_limits_above_below(base, ctx.style) {
        if is_large_op(base) {
            return layout_big_op_limits_heap(base, None, Some(sup), ctx);
        }
        return layout_op_limits_above_below_heap(base, None, Some(sup), ctx);
    }

    // Overbrace annotation: Superscript { base: Overscript { over: "⏞" }, sup: annotation }
    // Place annotation centered above the brace, not as a regular superscript.
    if let MathNode::Overscript {
        base: inner_base,
        over,
        ..
    } = base
        && node_is_overbrace(over)
    {
        return layout_overbrace_annotated(inner_base, over, sup, ctx);
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
    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
    } else {
        p.sup2 * size
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    // Italic correction: shift superscript right by the base glyph's italic correction.
    // TeX skips italic correction for prime superscripts — primes sit tight against the base.
    let ic = if is_prime_sup(sup) {
        0.0
    } else {
        italic_correction(&base_box)
    };

    let total_width = base_box.width + ic + sup_box.width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    // Capture dimensions before moving base_box into children vec (avoids clone)
    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
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
    if wants_limits_above_below(base, ctx.style) {
        if is_large_op(base) {
            return layout_big_op_limits_heap(base, Some(sub), None, ctx);
        }
        return layout_op_limits_above_below_heap(base, Some(sub), None, ctx);
    }

    // Underbrace annotation: Subscript { base: Underscript { under: "⏟" }, sub: annotation }
    // Place annotation centered below the brace, not as a regular subscript.
    if let MathNode::Underscript {
        base: inner_base,
        under,
        ..
    } = base
        && node_is_underbrace(under)
    {
        return layout_underbrace_annotated(inner_base, under, sub, ctx);
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
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1 * size;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height * size / 5.0));

    // Italic correction: shift subscript left by the base glyph's italic correction
    let ic = italic_correction(&base_box);
    let sub_dx = (base_box.width - ic).max(0.0);

    // Ensure total width is at least base width — large italic correction
    // can shift the subscript far left, but the base itself still occupies space.
    let total_width = (sub_dx + sub_box.width).max(base_box.width) + SCRIPT_SPACE * size;
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
    if wants_limits_above_below(base, ctx.style) {
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
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    // Superscript shift
    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
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

    // TeX skips italic correction for prime superscripts.
    let ic = if is_prime_sup(sup) {
        0.0
    } else {
        italic_correction(&base_box)
    };
    let sup_dx = base_box.width + ic;
    let sub_dx = (base_box.width - ic / 2.0).max(0.0);

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
    base_ic: f64,
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

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
    } else {
        p.sup2 * size
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_box.depth);

    let ic = if is_prime_sup(sup) { 0.0 } else { base_ic };

    let total_width = base_box.width + ic + sup_box.width + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
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

    let _ = base_ic;
    let sub_dx = base_box.width;

    let total_width = (sub_dx + sub_box.width).max(base_box.width) + SCRIPT_SPACE * size;
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

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
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

    let ic = if is_prime_sup(sup) { 0.0 } else { base_ic };
    let sup_dx = base_box.width + ic;
    let sub_dx = base_box.width;

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

    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
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
            shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio * size);
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
            shift_down =
                shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio * size);
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
    if wants_limits_above_below_c(ast, base, ctx.style) {
        if is_large_op_c(ast, base) {
            return layout_big_op_limits_arena(ast, base, None, Some(sup), arena, ctx);
        }
        return layout_op_limits_above_below_arena(ast, base, None, Some(sup), arena, ctx);
    }

    // Overbrace annotation: Superscript { base: Overscript { over: "⏞" }, sup: annotation }
    if let CompactNode::Overscript {
        base: inner_base,
        over,
        ..
    } = *ast.node(base)
        && node_is_overbrace_c(ast, over)
    {
        return layout_overbrace_annotated_arena(ast, inner_base, sup, arena, ctx);
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

    // TeX skips italic correction for prime superscripts.
    let ic = if is_prime_sup_c(ast, sup) {
        0.0
    } else {
        italic_correction_arena(base_id, arena)
    };

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let total_width = base_w + ic + sup_w + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: base_w + ic,
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
    if wants_limits_above_below_c(ast, base, ctx.style) {
        if is_large_op_c(ast, base) {
            return layout_big_op_limits_arena(ast, base, Some(sub), None, arena, ctx);
        }
        return layout_op_limits_above_below_arena(ast, base, Some(sub), None, arena, ctx);
    }

    // Underbrace annotation: Subscript { base: Underscript { under: "⏟" }, sub: annotation }
    if let CompactNode::Underscript {
        base: inner_base,
        under,
        ..
    } = *ast.node(base)
        && node_is_underbrace_c(ast, under)
    {
        return layout_underbrace_annotated_arena(ast, inner_base, sub, arena, ctx);
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

    let ic = italic_correction_arena(base_id, arena);
    let sub_dx = (base_w - ic).max(0.0);

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

    let total_width = (sub_dx + sub_w).max(base_w) + SCRIPT_SPACE * size;
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
    if wants_limits_above_below_c(ast, base, ctx.style) {
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

    let ic = if is_prime_sup_c(ast, sup) {
        0.0
    } else {
        italic_correction_arena(base_id, arena)
    };

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

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
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

    let sup_dx = base_w + ic;
    let sub_dx = (base_w - ic / 2.0).max(0.0);

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
    base_ic: f64,
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

    let ic = if is_prime_sup_c(ast, sup) {
        0.0
    } else {
        base_ic
    };

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    let size = ctx.size();
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
    } else {
        p.sup2 * size
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
    }
    let shift_up = shift_up.max(p.x_height * size / 4.0 + sup_d);

    let total_width = base_w + ic + sup_w + SCRIPT_SPACE * size;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: base_w + ic,
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

    let _ = base_ic;
    let sub_dx = base_w;

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

    let total_width = (sub_dx + sub_w).max(base_w) + SCRIPT_SPACE * size;
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

    let ic = if is_prime_sup_c(ast, sup) {
        0.0
    } else {
        base_ic
    };

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

    let mut shift_up = if ctx.style.is_display() {
        p.sup1 * size
    } else if ctx.style.is_cramped() {
        p.sup3 * size
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

    let sup_dx = base_w + ic;
    let sub_dx = base_w;

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

    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });
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
            shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio * size);
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
            shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio * size);
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
