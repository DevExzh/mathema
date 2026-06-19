use super::*;

/// TeXBook Rule 15: fraction layout.
pub(super) fn layout_fraction(
    kind: FractionKind,
    num: &MathNode,
    den: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Override context style for \dfrac (Display) and \tfrac (Inline)
    let override_ctx = match kind {
        FractionKind::Display => Some(ctx.with_style(MathStyle::Display)),
        FractionKind::Inline => Some(ctx.with_style(MathStyle::Text)),
        _ => None,
    };
    let ctx = override_ctx.as_ref().unwrap_or(ctx);

    let p = &ctx.params;
    let size = ctx.size();

    let num_style = ctx.style.fracnum();
    let den_style = ctx.style.fracden();
    // Fraction content should always preserve at least text-level inter-atom
    // spacing (medium and thick spaces).  Without this floor, inline fractions
    // (Text→Script) suppress binary-operator spacing, producing visually
    // cramped numerators/denominators.  The layout SIZE still follows the
    // normal fracnum/fracden rules; only the spacing calculation is upgraded.
    let mut num_ctx = ctx.with_style(num_style);
    num_ctx.spacing_style = Some(num_ctx.effective_spacing_style().min_text_level());
    let mut den_ctx = ctx.with_style(den_style);
    den_ctx.spacing_style = Some(den_ctx.effective_spacing_style().min_text_level());
    let num_box = layout_node(num, &num_ctx);
    let den_box = layout_node(den, &den_ctx);

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    // Shifts from axis (num3 used for no-bar fractions in non-display styles)
    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
    } else if bar_thickness == 0.0 {
        (p.num3 * size, p.denom2 * size)
    } else {
        (p.num2 * size, p.denom2 * size)
    };

    // Ensure minimum clearance
    let axis = p.axis_height * size;
    let mut num_up = num_shift;
    let mut den_down = den_shift;

    if bar_thickness > 0.0 {
        // Clearance from bar: at least 3*bar_thickness in display, 1*bar_thickness otherwise
        let min_clearance = if ctx.style.is_display() {
            3.0 * bar_thickness
        } else {
            bar_thickness
        };

        let num_clearance = (num_up - num_box.depth) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_box.height - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
        // No bar: ensure gap between num bottom and den top
        let min_gap = if ctx.style.is_display() {
            7.0 * p.rule_thickness * size
        } else {
            3.0 * p.rule_thickness * size
        };
        let gap = (num_up - num_box.depth) - (den_box.height - den_down);
        if gap < min_gap {
            let adjust = (min_gap - gap) / 2.0;
            num_up += adjust;
            den_down += adjust;
        }
    }

    let frac_width = num_box.width.max(den_box.width);
    // Add null-delimiter-space side padding to prevent content from touching adjacent glyphs.
    // Standard TeX adds ~0.12em on each side of a fraction.
    let side_padding = 0.12 * size;
    let padded_frac_width = frac_width + 2.0 * side_padding;
    let total_height = num_up + num_box.height;
    let total_depth = den_down + den_box.depth;

    // Center numerator and denominator within the padded width
    let num_dx = side_padding + (frac_width - num_box.width) / 2.0;
    let den_dx = side_padding + (frac_width - den_box.width) / 2.0;

    let mut children = vec![
        PositionedBox::new(num_dx, num_up, num_box),
        PositionedBox::new(den_dx, -den_down, den_box),
    ];

    // Add the fraction bar
    if bar_thickness > 0.0 {
        let bar = LayoutBox::rule(padded_frac_width, bar_thickness);
        children.push(PositionedBox::new(0.0, axis, bar));
    }

    LayoutBox {
        width: padded_frac_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

pub(super) fn layout_fraction_arena(
    ast: &AstArena,
    kind: FractionKind,
    num: NodeId,
    den: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Override context style for \dfrac (Display) and \tfrac (Inline)
    let override_ctx = match kind {
        FractionKind::Display => Some(ctx.with_style(MathStyle::Display)),
        FractionKind::Inline => Some(ctx.with_style(MathStyle::Text)),
        _ => None,
    };
    let ctx = override_ctx.as_ref().unwrap_or(ctx);

    let p = &ctx.params;
    let size = ctx.size();

    let num_style = ctx.style.fracnum();
    let den_style = ctx.style.fracden();
    // Preserve at least text-level spacing in fraction content (see heap path).
    let mut num_ctx = ctx.with_style(num_style);
    num_ctx.spacing_style = Some(num_ctx.effective_spacing_style().min_text_level());
    let mut den_ctx = ctx.with_style(den_style);
    den_ctx.spacing_style = Some(den_ctx.effective_spacing_style().min_text_level());
    let num_id = layout_node_arena(ast, num, arena, &num_ctx);
    let den_id = layout_node_arena(ast, den, arena, &den_ctx);

    // Read dimensions before any further arena mutation
    let num_w = arena.get(num_id).width;
    let num_h = arena.get(num_id).height;
    let num_d = arena.get(num_id).depth;
    let den_w = arena.get(den_id).width;
    let den_h = arena.get(den_id).height;
    let den_d = arena.get(den_id).depth;

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
    } else if bar_thickness == 0.0 {
        (p.num3 * size, p.denom2 * size)
    } else {
        (p.num2 * size, p.denom2 * size)
    };

    let axis = p.axis_height * size;
    let mut num_up = num_shift;
    let mut den_down = den_shift;

    if bar_thickness > 0.0 {
        let min_clearance = if ctx.style.is_display() {
            3.0 * bar_thickness
        } else {
            bar_thickness
        };

        let num_clearance = (num_up - num_d) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_h - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
        let min_gap = if ctx.style.is_display() {
            7.0 * p.rule_thickness * size
        } else {
            3.0 * p.rule_thickness * size
        };
        let gap = (num_up - num_d) - (den_h - den_down);
        if gap < min_gap {
            let adjust = (min_gap - gap) / 2.0;
            num_up += adjust;
            den_down += adjust;
        }
    }

    let frac_width = num_w.max(den_w);
    // Add null-delimiter-space side padding to prevent content from touching adjacent glyphs
    let side_padding = 0.12 * size;
    let padded_frac_width = frac_width + 2.0 * side_padding;
    let total_height = num_up + num_h;
    let total_depth = den_down + den_d;

    let num_dx = side_padding + (frac_width - num_w) / 2.0;
    let den_dx = side_padding + (frac_width - den_w) / 2.0;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: num_dx,
        dy: num_up,
        id: num_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: den_dx,
        dy: -den_down,
        id: den_id,
    });

    if bar_thickness > 0.0 {
        let bar_id = arena.alloc(ArenaLayoutBox::rule(padded_frac_width, bar_thickness));
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: axis,
            id: bar_id,
        });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: padded_frac_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

// ── Remaining arena layout functions ─────────────────────────────────
