use super::*;

use crate::unicode::symbols::is_integral_operator;

/// Check whether a MathNode is a large (n-ary) operator symbol.
pub(super) fn is_large_op(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(SymbolKind::LargeOp, _))
}

/// MathML lets a singleton row stand in for its core operator when deciding
/// whether under/over limits move to the side in compact styles.
pub(super) fn inline_limit_base(
    mut base: &MathNode,
    movable: bool,
    style: MathStyle,
) -> Option<&MathNode> {
    if !movable || style.is_display() {
        return None;
    }
    while let MathNode::Row(nodes) = base {
        let [child] = nodes.as_slice() else {
            return None;
        };
        base = child;
    }
    matches!(
        base,
        MathNode::Symbol(SymbolKind::LargeOp, _)
            | MathNode::Operator(OperatorAttr {
                limit_style: LimitStyle::Default,
                ..
            })
            | MathNode::StructuredOperator {
                form: OperatorNameForm::Starred(LimitStyle::Default),
                ..
            }
            | MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            }
    )
    .then_some(base)
}

pub(super) fn inline_limit_base_c(
    ast: &AstArena,
    mut base: NodeId,
    movable: bool,
    style: MathStyle,
) -> Option<NodeId> {
    if !movable || style.is_display() {
        return None;
    }
    while let CompactNode::Row(span) = *ast.node(base) {
        let &[child] = ast.children(span) else {
            return None;
        };
        base = child;
    }
    matches!(
        *ast.node(base),
        CompactNode::Symbol(SymbolKind::LargeOp, _)
            | CompactNode::Operator {
                limits: LimitStyle::Default,
                ..
            }
            | CompactNode::StructuredOperator {
                form: OperatorNameForm::Starred(LimitStyle::Default),
                ..
            }
            | CompactNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            }
    )
    .then_some(base)
}

/// Check whether a MathNode wants above/below limits in the given style.
/// Returns true for: Symbol(LargeOp) in display mode, Operator with AboveBelow limits,
/// or Operator with Default limits in display mode.
pub(super) fn wants_limits_above_below(node: &MathNode, style: MathStyle) -> bool {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, s) => {
            style.is_display() && !s.chars().next().is_some_and(is_integral_operator)
        },
        MathNode::Operator(OperatorAttr { limit_style, .. }) => match limit_style {
            LimitStyle::AboveBelow => true,
            LimitStyle::Default => style.is_display(),
            LimitStyle::SubSup => false,
        },
        MathNode::StructuredOperator { form, .. } => match form.limit_style() {
            LimitStyle::AboveBelow => true,
            LimitStyle::Default => style.is_display(),
            LimitStyle::SubSup => false,
        },
        // Decorated compounds fall through as ordinary bases; variable-limit
        // macros add an explicit movable MathAtom when outer scripts move.
        _ => false,
    }
}

/// Check whether a CompactNode is a large (n-ary) operator symbol.
pub(super) fn is_large_op_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(arena.node(id), CompactNode::Symbol(SymbolKind::LargeOp, _))
}

/// Extract the first character from a large-op symbol node.
pub(super) fn large_op_char(node: &MathNode) -> Option<char> {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, s) => s.chars().next(),
        _ => None,
    }
}

/// Extract the first character from a large-op compact symbol node.
pub(super) fn large_op_char_c(arena: &AstArena, id: NodeId) -> Option<char> {
    match arena.node(id) {
        CompactNode::Symbol(SymbolKind::LargeOp, sid) => arena.str_val(*sid).chars().next(),
        _ => None,
    }
}

/// Check whether a compact node wants above/below limits in the given style.
/// Returns true for: Symbol(LargeOp) in display mode, Operator with AboveBelow limits,
/// or Operator with Default limits in display mode.
pub(super) fn wants_limits_above_below_c(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    match *arena.node(id) {
        CompactNode::Symbol(SymbolKind::LargeOp, sid) => {
            style.is_display()
                && !arena
                    .str_val(sid)
                    .chars()
                    .next()
                    .is_some_and(is_integral_operator)
        },
        CompactNode::Operator { limits, .. } => match limits {
            LimitStyle::AboveBelow => true,
            LimitStyle::Default => style.is_display(),
            LimitStyle::SubSup => false,
        },
        CompactNode::StructuredOperator { form, .. } => match form.limit_style() {
            LimitStyle::AboveBelow => true,
            LimitStyle::Default => style.is_display(),
            LimitStyle::SubSup => false,
        },
        // Decorated compounds fall through as ordinary bases; variable-limit
        // macros add an explicit movable MathAtom when outer scripts move.
        _ => false,
    }
}

/// The canonical stretchy over/under symbol for `sym`, accepting the
/// vertical-presentation-form braces (`︷` U+FE37, `︸` U+FE38) that MathML
/// sources use for `⏞`/`⏟` (the font has no glyph for the former).
pub(super) fn canonical_stretchy_overunder(sym: &str) -> Option<&'static str> {
    Some(match sym {
        "\u{23DE}" | "\u{FE37}" => "\u{23DE}",
        "\u{23DF}" | "\u{FE38}" => "\u{23DF}",
        "\u{23B4}" => "\u{23B4}",
        "\u{23B5}" => "\u{23B5}",
        "\u{203E}" => "\u{203E}",
        _ => return None,
    })
}

/// Layout a big operator (LargeOp symbol) with limits above/below (heap path).
/// Used when SubSup/Subscript/Superscript has a large-op base in display mode.
pub(super) fn layout_big_op_limits_heap(
    base: &MathNode,
    sub: Option<&MathNode>,
    sup: Option<&MathNode>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char(base).unwrap_or('\u{2211}');
    let op_size = size * metrics::display_op_scale(op_ch);
    let m = metrics::glyph_metrics_display(op_ch);
    let op_box = LayoutBox::glyph_with_metrics(op_ch, m, op_size, false);

    let mut children = Vec::new();
    let mut total_h = op_box.height;
    let mut total_d = op_box.depth;
    let mut max_w = op_box.width;

    let sup_box = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node(s, &sup_ctx)
    });
    let sub_box = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node(s, &sub_ctx)
    });

    // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
    let sup_gap = sup_box.as_ref().map_or(0.0, |sb| {
        (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sb.depth)
    });
    // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
    let sub_gap = sub_box.as_ref().map_or(0.0, |sb| {
        (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sb.height)
    });
    let padding = p.big_op_spacing[4] * size;

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + sup_gap + padding;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + sub_gap + padding;
    }

    // Center op horizontally
    let op_dx = (max_w - op_box.width) / 2.0;

    if let Some(sb) = sup_box {
        let sup_dx = (max_w - sb.width) / 2.0;
        let sup_dy = op_box.height + sup_gap + sb.depth;
        children.push(PositionedBox::new(sup_dx, sup_dy, sb));
    }
    let op_depth = op_box.depth;
    children.push(PositionedBox::new(op_dx, 0.0, op_box));
    if let Some(sb) = sub_box {
        let sub_dx = (max_w - sb.width) / 2.0;
        let sub_dy = -(op_depth + sub_gap + sb.height);
        children.push(PositionedBox::new(sub_dx, sub_dy, sb));
    }

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

/// Layout a named operator (e.g. \lim, \min, \max) with limits above/below (heap path).
/// Unlike `layout_big_op_limits_heap`, this does not apply large-op glyph scaling —
/// the operator is rendered as normal text with limits centered above/below.
pub(super) fn layout_op_limits_above_below_heap(
    base: &MathNode,
    sub: Option<&MathNode>,
    sup: Option<&MathNode>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Layout the operator base normally (e.g. "lim" as text)
    let op_box = layout_node(base, ctx);

    let mut children = Vec::new();
    let mut total_h = op_box.height;
    let mut total_d = op_box.depth;
    let mut max_w = op_box.width;

    let sup_box = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node(s, &sup_ctx)
    });
    let sub_box = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node(s, &sub_ctx)
    });

    // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
    let sup_gap = sup_box.as_ref().map_or(0.0, |sb| {
        (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sb.depth)
    });
    // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
    let sub_gap = sub_box.as_ref().map_or(0.0, |sb| {
        (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sb.height)
    });
    let padding = p.big_op_spacing[4] * size;

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + sup_gap + padding;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + sub_gap + padding;
    }

    let op_dx = (max_w - op_box.width) / 2.0;

    if let Some(sb) = sup_box {
        let sup_dx = (max_w - sb.width) / 2.0;
        let sup_dy = op_box.height + sup_gap + sb.depth;
        children.push(PositionedBox::new(sup_dx, sup_dy, sb));
    }
    let op_depth = op_box.depth;
    children.push(PositionedBox::new(op_dx, 0.0, op_box));
    if let Some(sb) = sub_box {
        let sub_dx = (max_w - sb.width) / 2.0;
        let sub_dy = -(op_depth + sub_gap + sb.height);
        children.push(PositionedBox::new(sub_dx, sub_dy, sb));
    }

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

pub(super) fn layout_nary(
    op: char,
    sub: &Option<Box<MathNode>>,
    sup: &Option<Box<MathNode>>,
    body: &MathNode,
    limits: LimitStyle,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    // The space between the operator and its operand.
    let thin_space = spacing::nary_body_atom(body, ctx.branch_style()).map_or(0.0, |atom| {
        spacing::inter_atom_space(AtomType::Op, atom, ctx.effective_spacing_style()) * size
    });

    // Large op in display mode: use per-operator scale matching KaTeX Size2/Size1
    let (op_size, m) = if ctx.branch_style().is_display() {
        (
            size * metrics::display_op_scale(op),
            metrics::glyph_metrics_display(op),
        )
    } else {
        (size, metrics::glyph_metrics(op))
    };
    let op_box = LayoutBox::glyph_with_metrics(op, m, op_size, false);

    // TeX: center the large operator on the math axis.
    // delta = axis_height - (height - depth) / 2
    let axis_delta = if ctx.branch_style().is_display() {
        p.axis_height * size - (op_box.height - op_box.depth) / 2.0
    } else {
        0.0
    };

    let body_box = layout_node(body, ctx);
    // Italic correction of the operator glyph (lost once it is wrapped in an
    // HBox), used to offset side-set scripts diagonally (e.g. integral limits).
    let op_ic = m.italic * op_size;
    // Capture body dimensions before any move
    let body_w = body_box.width;
    let body_h = body_box.height;
    let body_d = body_box.depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        // Integrals default to side-limits (nolimits); sums/products to above/below in display.
        LimitStyle::Default => ctx.branch_style().is_display() && !is_integral_operator(op),
    };

    if use_limits {
        // Limits above/below
        let mut children = Vec::new();
        // Account for axis centering shift in effective op dimensions
        let eff_op_h = op_box.height + axis_delta;
        let eff_op_d = (op_box.depth - axis_delta).max(0.0);
        let mut total_h = eff_op_h;
        let mut total_d = eff_op_d;
        let mut max_w = op_box.width;

        let sup_box = sup.as_ref().map(|s| {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            layout_node(s, &sup_ctx)
        });
        let sub_box = sub.as_ref().map(|s| {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            layout_node(s, &sub_ctx)
        });

        // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
        let sup_gap = sup_box.as_ref().map_or(0.0, |sb| {
            (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sb.depth)
        });
        // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
        let sub_gap = sub_box.as_ref().map_or(0.0, |sb| {
            (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sb.height)
        });
        let padding = p.big_op_spacing[4] * size;

        if let Some(ref sb) = sup_box {
            max_w = max_w.max(sb.width);
            total_h += sb.height + sb.depth + sup_gap + padding;
        }
        if let Some(ref sb) = sub_box {
            max_w = max_w.max(sb.width);
            total_d += sb.height + sb.depth + sub_gap + padding;
        }

        // Center op
        let op_dx = (max_w - op_box.width) / 2.0;

        if let Some(sb) = sup_box {
            // Center over the op; for a slanted operator (integral) nudge toward
            // the glyph's top by half its italic correction.
            let sup_dx = (max_w - sb.width) / 2.0 + op_ic / 2.0;
            let sup_dy = eff_op_h + sup_gap + sb.depth;
            children.push(PositionedBox::new(sup_dx, sup_dy, sb));
        }
        children.push(PositionedBox::new(op_dx, axis_delta, op_box));
        if let Some(sb) = sub_box {
            let sub_dx = (max_w - sb.width) / 2.0 - op_ic / 2.0;
            let sub_dy = -(eff_op_d + sub_gap + sb.height);
            children.push(PositionedBox::new(sub_dx, sub_dy, sb));
        }

        // Body follows
        let nary_width = max_w;
        children.push(PositionedBox::new(nary_width + thin_space, 0.0, body_box));
        LayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h.max(body_h),
            depth: total_d.max(body_d),
            kind: BoxKind::HBox { children },
        }
    } else {
        // As sub/superscripts — use the already-scaled op_box

        // Wrap large-op glyph in HBox so is_char_box returns false in subsup layout.
        // KaTeX does not treat large operator symbols as character boxes, allowing
        // the sup_drop term (base.height - supDrop) to position scripts proportionally.
        // Apply axis_delta to center the operator on the math axis (TeX Rule 13a/13b).
        let op_box = {
            let w = op_box.width;
            let h = op_box.height + axis_delta;
            let d = (op_box.depth - axis_delta).max(0.0);
            LayoutBox {
                width: w,
                height: h,
                depth: d,
                kind: BoxKind::HBox {
                    children: vec![PositionedBox::new(0.0, axis_delta, op_box)],
                },
            }
        };

        let build_hbox = |result: LayoutBox, body_box: LayoutBox| -> LayoutBox {
            let rw = result.width;
            let rh = result.height;
            let rd = result.depth;
            let bw = body_box.width;
            let bh = body_box.height;
            let bd = body_box.depth;
            let children = vec![
                PositionedBox::at_origin(result),
                PositionedBox::new(rw + thin_space, 0.0, body_box),
            ];
            LayoutBox {
                width: rw + thin_space + bw,
                height: rh.max(bh),
                depth: rd.max(bd),
                kind: BoxKind::HBox { children },
            }
        };

        if let (Some(sub_n), Some(sup_n)) = (sub, sup) {
            let result = layout_subsup_from_box_heap(op_box, op_ic, sub_n, sup_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sub_n) = sub {
            let result = layout_subscript_from_box_heap(op_box, op_ic, sub_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sup_n) = sup {
            let result = layout_superscript_from_box_heap(op_box, sup_n, ctx);
            return build_hbox(result, body_box);
        }

        // Just op + body
        let op_w = op_box.width;
        let op_h = op_box.height;
        let op_d = op_box.depth;
        let children = vec![
            PositionedBox::at_origin(op_box),
            PositionedBox::new(op_w + thin_space, 0.0, body_box),
        ];
        LayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: BoxKind::HBox { children },
        }
    }
}

/// Extract the symbol string from a MathNode if it is a stretchy overunder decoration.
pub(super) fn get_stretchy_overunder_sym(node: &MathNode) -> Option<&'static str> {
    if let MathNode::Symbol(_, s) = node {
        return canonical_stretchy_overunder(s);
    }
    None
}

/// Create a stretchy overunder decoration LayoutBox at the given width.
/// Quantize a stretchy-delimiter target height to a fine (1e-9 em) grid. The
/// heap and arena layout engines accumulate content height in different f64
/// orders, so their raw target heights can differ by ~1 ULP; without this, that
/// sub-ULP difference can straddle a rounding boundary and flip a 4-decimal
/// coordinate in the emitted delimiter path. The adjustment is ≤5e-10 em — far
/// below output precision — so it changes no visible or emitted geometry while
/// keeping the two engines byte-identical.
pub(super) fn quantize_delim_height(h: f64) -> f64 {
    (h * 1.0e9).round() / 1.0e9
}

/// Decoration box height (em·size) for a stretchy over/under symbol, matched to
/// KaTeX's `katexImagesData` (viewBoxHeight / 1000). Shared by the legacy and
/// arena over/under-script layouts so they stay in sync.
pub(super) fn stretchy_overunder_deco_h(sym: &str, size: f64) -> f64 {
    match sym {
        "\u{23DE}" | "\u{23DF}" => 0.548 * size, // overbrace/underbrace
        "\u{23B4}" | "\u{23B5}" => 0.44 * size,  // overbracket/underbracket
        "\u{203E}" => 0.08 * size,               // overline/underline (thin rule)
        _ => 0.40 * size,
    }
}

pub(super) fn make_stretchy_overunder_box(sym: &str, width: f64, size: f64) -> LayoutBox {
    let deco_h = stretchy_overunder_deco_h(sym, size);
    LayoutBox {
        width,
        height: deco_h,
        depth: 0.0,
        kind: BoxKind::SvgPath(stretchy_overunder_path(sym, width, deco_h)),
    }
}

pub(super) fn layout_overscript(base: &MathNode, over: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node(over, &over_ctx);
        if let Some(arrow_w) = extensible_arrow_width(arrow_ch, over_box.width, size) {
            let arrow_box =
                make_extensible_arrow_box(arrow_ch, arrow_w, size, p.axis_height * size);
            let arrow_d = arrow_box.depth;

            let gap = p.big_op_spacing[2] * size;
            let max_w = arrow_box.width.max(over_box.width);
            let base_dx = (max_w - arrow_box.width) / 2.0;
            let over_dx = (max_w - over_box.width) / 2.0;
            let over_dy = arrow_box.height + gap + over_box.depth;
            let over_h = over_box.height;

            let children = vec![
                PositionedBox::new(base_dx, 0.0, arrow_box),
                PositionedBox::new(over_dx, over_dy, over_box),
            ];

            return LayoutBox {
                width: max_w,
                height: over_dy + over_h,
                depth: arrow_d,
                kind: BoxKind::VBox { children },
            };
        }
        label = Some(over_box);
    }

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    // Check if the over decoration is a stretchy symbol that should scale to base width
    let (over_box, is_line_accent) = if let Some(sym) = get_stretchy_overunder_sym(over) {
        (
            make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size),
            sym == "\u{203E}",
        )
    } else {
        let over_box = label.unwrap_or_else(|| {
            let over_ctx = ctx.with_style(ctx.style.sup());
            layout_node(over, &over_ctx)
        });
        (over_box, false)
    };

    // KaTeX Rule 9: overline uses 3 × defaultRuleThickness gap
    let gap = if is_line_accent {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[2] * size
    };
    stack_single_script(base_box, over_box, AccentPosition::Above, gap)
}

pub(super) fn layout_underscript(base: &MathNode, under: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node(under, &under_ctx);
        if let Some(arrow_w) = extensible_arrow_width(arrow_ch, under_box.width, size) {
            let arrow_box =
                make_extensible_arrow_box(arrow_ch, arrow_w, size, p.axis_height * size);
            let arrow_a = arrow_box.height;

            let gap = p.big_op_spacing[4] * size;
            let max_w = arrow_box.width.max(under_box.width);
            let base_dx = (max_w - arrow_box.width) / 2.0;
            let under_dx = (max_w - under_box.width) / 2.0;
            let under_dy = arrow_box.depth + gap + under_box.height;
            let under_d = under_box.depth;

            let children = vec![
                PositionedBox::new(base_dx, 0.0, arrow_box),
                PositionedBox::new(under_dx, -under_dy, under_box),
            ];

            return LayoutBox {
                width: max_w,
                height: arrow_a,
                depth: under_dy + under_d,
                kind: BoxKind::VBox { children },
            };
        }
        label = Some(under_box);
    }

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    // Check if the under decoration is a stretchy symbol that should scale to base width
    let under_box = if let Some(sym) = get_stretchy_overunder_sym(under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_box = label.unwrap_or_else(|| {
            let under_ctx = ctx.with_style(ctx.style.sub());
            layout_node(under, &under_ctx)
        });
        under_box
    };

    let gap = p.big_op_spacing[4] * size;
    stack_single_script(base_box, under_box, AccentPosition::Below, gap)
}

pub(super) fn stack_single_script(
    base: LayoutBox,
    script: LayoutBox,
    position: AccentPosition,
    gap: f64,
) -> LayoutBox {
    let width = base.width.max(script.width);
    let y = match position {
        AccentPosition::Above => base.height + gap + script.depth,
        AccentPosition::Below => -(base.depth + gap + script.height),
    };
    let (height, depth) = match position {
        AccentPosition::Above => (y + script.height, base.depth),
        AccentPosition::Below => (base.height, script.depth - y),
    };
    let children = vec![
        PositionedBox::new((width - base.width) / 2.0, 0.0, base),
        PositionedBox::new((width - script.width) / 2.0, y, script),
    ];
    LayoutBox {
        width,
        height,
        depth,
        kind: BoxKind::VBox { children },
    }
}

pub(super) fn layout_underover(
    base: &MathNode,
    under: &MathNode,
    over: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node(over, &over_ctx);
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node(under, &under_ctx);
        if let Some(arrow_w) =
            extensible_arrow_width(arrow_ch, over_box.width.max(under_box.width), size)
        {
            let arrow_box =
                make_extensible_arrow_box(arrow_ch, arrow_w, size, p.axis_height * size);

            let gap_above = p.big_op_spacing[2] * size;
            let gap_below = p.big_op_spacing[4] * size;
            let max_w = arrow_box.width.max(over_box.width).max(under_box.width);
            let base_dx = (max_w - arrow_box.width) / 2.0;
            let over_dx = (max_w - over_box.width) / 2.0;
            let under_dx = (max_w - under_box.width) / 2.0;
            let over_dy = arrow_box.height + gap_above + over_box.depth;
            let under_dy = arrow_box.depth + gap_below + under_box.height;
            let over_h = over_box.height;
            let under_d = under_box.depth;

            let children = vec![
                PositionedBox::new(over_dx, over_dy, over_box),
                PositionedBox::new(base_dx, 0.0, arrow_box),
                PositionedBox::new(under_dx, -under_dy, under_box),
            ];

            return LayoutBox {
                width: max_w,
                height: over_dy + over_h,
                depth: under_dy + under_d,
                kind: BoxKind::VBox { children },
            };
        }
        label = Some((over_box, under_box));
    }
    let (label_over, label_under) = label.unzip();

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    let (over_box, is_over_line) = if let Some(sym) = get_stretchy_overunder_sym(over) {
        (
            make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size),
            sym == "\u{203E}",
        )
    } else {
        let over_box = label_over.unwrap_or_else(|| {
            let over_ctx = ctx.with_style(ctx.style.sup());
            layout_node(over, &over_ctx)
        });
        (over_box, false)
    };

    let (under_box, is_under_line) = if let Some(sym) = get_stretchy_overunder_sym(under) {
        (
            make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size),
            sym == "\u{203E}",
        )
    } else {
        let under_box = label_under.unwrap_or_else(|| {
            let under_ctx = ctx.with_style(ctx.style.sub());
            layout_node(under, &under_ctx)
        });
        (under_box, false)
    };

    let gap_above = if is_over_line {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[2] * size
    };
    let gap_below = if is_under_line {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[4] * size
    };
    let max_w = base_box.width.max(over_box.width).max(under_box.width);

    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;

    let over_dy = base_box.height + gap_above + over_box.depth;
    let under_dy = base_box.depth + gap_below + under_box.height;

    // Capture dimensions before moving into children
    let over_h = over_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(over_dx, over_dy, over_box),
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

/// Check if a CompactNode (referenced by NodeId) is a stretchy overunder decoration symbol.
/// Returns the symbol string if so.
pub(super) fn get_stretchy_overunder_sym_c(arena: &AstArena, id: NodeId) -> Option<&'static str> {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        return canonical_stretchy_overunder(arena.str_val(sid));
    }
    None
}

/// Layout a big operator (LargeOp symbol) with limits above/below (arena path).
pub(super) fn layout_big_op_limits_arena(
    ast: &AstArena,
    base: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char_c(ast, base).unwrap_or('\u{2211}');
    let op_size = size * metrics::display_op_scale(op_ch);
    let m = metrics::glyph_metrics_display(op_ch);
    let op_id = alloc_glyph_with_metrics(arena, op_ch, m, op_size, false);

    let op_w = arena.get(op_id).width;
    let op_h = arena.get(op_id).height;
    let op_d = arena.get(op_id).depth;

    let mut total_h = op_h;
    let mut total_d = op_d;
    let mut max_w = op_w;

    let sup_id_opt = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, s, arena, &sup_ctx)
    });
    let sub_id_opt = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, s, arena, &sub_ctx)
    });

    let (sup_w_opt, sup_h_opt, sup_d_opt) = if let Some(sid) = sup_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };
    let (sub_w_opt, sub_h_opt, sub_d_opt) = if let Some(sid) = sub_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };

    // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
    let sup_gap = if sup_id_opt.is_some() {
        (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sup_d_opt)
    } else {
        0.0
    };
    // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
    let sub_gap = if sub_id_opt.is_some() {
        (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sub_h_opt)
    } else {
        0.0
    };
    let padding = p.big_op_spacing[4] * size;

    if sup_id_opt.is_some() {
        max_w = max_w.max(sup_w_opt);
        total_h += sup_h_opt + sup_d_opt + sup_gap + padding;
    }
    if sub_id_opt.is_some() {
        max_w = max_w.max(sub_w_opt);
        total_d += sub_h_opt + sub_d_opt + sub_gap + padding;
    }

    let op_dx = (max_w - op_w) / 2.0;

    let children_start = arena.begin_children();

    if let Some(sid) = sup_id_opt {
        let sup_dx = (max_w - sup_w_opt) / 2.0;
        let sup_dy = op_h + sup_gap + sup_d_opt;
        arena.push_child(ArenaPositionedBox {
            dx: sup_dx,
            dy: sup_dy,
            id: sid,
        });
    }
    arena.push_child(ArenaPositionedBox {
        dx: op_dx,
        dy: 0.0,
        id: op_id,
    });
    if let Some(sid) = sub_id_opt {
        let sub_dx = (max_w - sub_w_opt) / 2.0;
        let sub_dy = -(op_d + sub_gap + sub_h_opt);
        arena.push_child(ArenaPositionedBox {
            dx: sub_dx,
            dy: sub_dy,
            id: sid,
        });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Layout a named operator (e.g. \lim, \min, \max) with limits above/below (arena path).
pub(super) fn layout_op_limits_above_below_arena(
    ast: &AstArena,
    base: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Layout the operator base normally (e.g. "lim" as text)
    let op_id = layout_node_arena(ast, base, arena, ctx);

    let op_w = arena.get(op_id).width;
    let op_h = arena.get(op_id).height;
    let op_d = arena.get(op_id).depth;

    let mut total_h = op_h;
    let mut total_d = op_d;
    let mut max_w = op_w;

    let sup_id_opt = sup.filter(|s| s.is_some()).map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, s, arena, &sup_ctx)
    });
    let sub_id_opt = sub.filter(|s| s.is_some()).map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, s, arena, &sub_ctx)
    });

    let (sup_w_opt, sup_h_opt, sup_d_opt) = if let Some(sid) = sup_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };
    let (sub_w_opt, sub_h_opt, sub_d_opt) = if let Some(sid) = sub_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };

    // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
    let sup_gap = if sup_id_opt.is_some() {
        (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sup_d_opt)
    } else {
        0.0
    };
    // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
    let sub_gap = if sub_id_opt.is_some() {
        (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sub_h_opt)
    } else {
        0.0
    };
    let padding = p.big_op_spacing[4] * size;

    if sup_id_opt.is_some() {
        max_w = max_w.max(sup_w_opt);
        total_h += sup_h_opt + sup_d_opt + sup_gap + padding;
    }
    if sub_id_opt.is_some() {
        max_w = max_w.max(sub_w_opt);
        total_d += sub_h_opt + sub_d_opt + sub_gap + padding;
    }

    let op_dx = (max_w - op_w) / 2.0;

    let children_start = arena.begin_children();

    if let Some(sid) = sup_id_opt {
        let sup_dx = (max_w - sup_w_opt) / 2.0;
        let sup_dy = op_h + sup_gap + sup_d_opt;
        arena.push_child(ArenaPositionedBox {
            dx: sup_dx,
            dy: sup_dy,
            id: sid,
        });
    }
    arena.push_child(ArenaPositionedBox {
        dx: op_dx,
        dy: 0.0,
        id: op_id,
    });
    if let Some(sid) = sub_id_opt {
        let sub_dx = (max_w - sub_w_opt) / 2.0;
        let sub_dy = -(op_d + sub_gap + sub_h_opt);
        arena.push_child(ArenaPositionedBox {
            dx: sub_dx,
            dy: sub_dy,
            id: sid,
        });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn layout_nary_arena(
    ast: &AstArena,
    op: char,
    sub: NodeId,
    sup: NodeId,
    body: NodeId,
    limits: LimitStyle,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    // The space between the operator and its operand.
    let thin_space = spacing::nary_body_atom_c(ast, body, ctx.branch_style()).map_or(0.0, |atom| {
        spacing::inter_atom_space(AtomType::Op, atom, ctx.effective_spacing_style()) * size
    });

    // Large op in display mode: use per-operator scale matching KaTeX Size2/Size1
    let (op_size, m) = if ctx.branch_style().is_display() {
        (
            size * metrics::display_op_scale(op),
            metrics::glyph_metrics_display(op),
        )
    } else {
        (size, metrics::glyph_metrics(op))
    };
    let op_size = op_size * ctx.glyph_scale;
    let op_id = alloc_glyph_with_metrics(arena, op, m, op_size, false);

    // TeX: center the large operator on the math axis.
    let op_h_raw = arena.get(op_id).height;
    let op_d_raw = arena.get(op_id).depth;
    let axis_delta = if ctx.branch_style().is_display() {
        p.axis_height * size - (op_h_raw - op_d_raw) / 2.0
    } else {
        0.0
    };

    let body_id = layout_node_arena(ast, body, arena, ctx);
    // Italic correction of the operator glyph (lost once it is wrapped in an
    // HBox), used to offset side-set scripts diagonally (e.g. integral limits).
    let op_ic = m.italic * op_size;
    let body_w = arena.get(body_id).width;
    let body_h = arena.get(body_id).height;
    let body_d = arena.get(body_id).depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        // Integrals default to side-limits (nolimits); sums/products to above/below in display.
        LimitStyle::Default => ctx.branch_style().is_display() && !is_integral_operator(op),
    };

    if use_limits {
        // Limits above/below
        let op_w = arena.get(op_id).width;
        let op_h = arena.get(op_id).height;
        let op_d = arena.get(op_id).depth;

        let eff_op_h = op_h + axis_delta;
        let eff_op_d = (op_d - axis_delta).max(0.0);
        let mut total_h = eff_op_h;
        let mut total_d = eff_op_d;
        let mut max_w = op_w;

        let sup_id_opt = if sup.is_some() {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            let sid = layout_node_arena(ast, sup, arena, &sup_ctx);
            Some(sid)
        } else {
            None
        };
        let sub_id_opt = if sub.is_some() {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            let sid = layout_node_arena(ast, sub, arena, &sub_ctx);
            Some(sid)
        } else {
            None
        };

        // Read dimensions for sup/sub
        let (sup_w_opt, sup_h_opt, sup_d_opt) = if let Some(sid) = sup_id_opt {
            let b = arena.get(sid);
            (b.width, b.height, b.depth)
        } else {
            (0.0, 0.0, 0.0)
        };
        let (sub_w_opt, sub_h_opt, sub_d_opt) = if let Some(sid) = sub_id_opt {
            let b = arena.get(sid);
            (b.width, b.height, b.depth)
        } else {
            (0.0, 0.0, 0.0)
        };

        // KaTeX assembleSupSub: gap = max(bigOpSpacing1, bigOpSpacing3 - sup.depth)
        let sup_gap = if sup_id_opt.is_some() {
            (p.big_op_spacing[0] * size).max(p.big_op_spacing[2] * size - sup_d_opt)
        } else {
            0.0
        };
        // KaTeX assembleSupSub: gap = max(bigOpSpacing2, bigOpSpacing4 - sub.height)
        let sub_gap = if sub_id_opt.is_some() {
            (p.big_op_spacing[1] * size).max(p.big_op_spacing[3] * size - sub_h_opt)
        } else {
            0.0
        };
        let padding = p.big_op_spacing[4] * size;

        if sup_id_opt.is_some() {
            max_w = max_w.max(sup_w_opt);
            total_h += sup_h_opt + sup_d_opt + sup_gap + padding;
        }
        if sub_id_opt.is_some() {
            max_w = max_w.max(sub_w_opt);
            total_d += sub_h_opt + sub_d_opt + sub_gap + padding;
        }

        let op_dx = (max_w - op_w) / 2.0;

        let children_start = arena.begin_children();

        if let Some(sid) = sup_id_opt {
            let sup_dx = (max_w - sup_w_opt) / 2.0 + op_ic / 2.0;
            let sup_dy = eff_op_h + sup_gap + sup_d_opt;
            arena.push_child(ArenaPositionedBox {
                dx: sup_dx,
                dy: sup_dy,
                id: sid,
            });
        }
        arena.push_child(ArenaPositionedBox {
            dx: op_dx,
            dy: axis_delta,
            id: op_id,
        });
        if let Some(sid) = sub_id_opt {
            let sub_dx = (max_w - sub_w_opt) / 2.0 - op_ic / 2.0;
            let sub_dy = -(eff_op_d + sub_gap + sub_h_opt);
            arena.push_child(ArenaPositionedBox {
                dx: sub_dx,
                dy: sub_dy,
                id: sid,
            });
        }

        // Body follows
        let nary_width = max_w;
        arena.push_child(ArenaPositionedBox {
            dx: nary_width + thin_space,
            dy: 0.0,
            id: body_id,
        });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h.max(body_h),
            depth: total_d.max(body_d),
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        // As sub/superscripts

        // Wrap large-op glyph in HBox so is_char_box returns false in subsup layout.
        // KaTeX does not treat large operator symbols as character boxes, allowing
        // the sup_drop term (base.height - supDrop) to position scripts proportionally.
        // Apply axis_delta to center the operator on the math axis (TeX Rule 13a/13b).
        let op_glyph_id = {
            let b = arena.get(op_id);
            let w = b.width;
            let h = b.height + axis_delta;
            let d = (b.depth - axis_delta).max(0.0);
            let cs = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: 0.0,
                dy: axis_delta,
                id: op_id,
            });
            let children = arena.finish_children(cs);
            arena.alloc(ArenaLayoutBox {
                width: w,
                height: h,
                depth: d,
                kind: ArenaBoxKind::HBox { children },
            })
        };

        if sub.is_some() && sup.is_some() {
            let result_id =
                layout_subsup_from_box_arena(op_glyph_id, op_ic, ast, sub, sup, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        } else if sub.is_some() {
            let result_id =
                layout_subscript_from_box_arena(op_glyph_id, op_ic, ast, sub, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        } else if sup.is_some() {
            let result_id = layout_superscript_from_box_arena(op_glyph_id, ast, sup, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        }

        // Just op + body
        let op_w = arena.get(op_glyph_id).width;
        let op_h = arena.get(op_glyph_id).height;
        let op_d = arena.get(op_glyph_id).depth;

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: 0.0,
            id: op_glyph_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: op_w + thin_space,
            dy: 0.0,
            id: body_id,
        });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: ArenaBoxKind::HBox { children },
        })
    }
}

pub(super) fn layout_overscript_arena(
    ast: &AstArena,
    base: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_id = layout_node_arena(ast, over, arena, &over_ctx);
        let over_w = arena.get(over_id).width;
        let over_h = arena.get(over_id).height;
        let over_d = arena.get(over_id).depth;

        if let Some(arrow_w) = extensible_arrow_width(arrow_ch, over_w, size) {
            let (_aw, arrow_a, arrow_d) =
                extensible_arrow_dims(arrow_w, size, p.axis_height * size);
            let arrow_id = arena.alloc(ArenaLayoutBox {
                width: arrow_w,
                height: arrow_a,
                depth: arrow_d,
                kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, size)),
            });

            let gap = p.big_op_spacing[2] * size;
            let max_w = arrow_w.max(over_w);
            let base_dx = (max_w - arrow_w) / 2.0;
            let over_dx = (max_w - over_w) / 2.0;
            let over_dy = arrow_a + gap + over_d;

            let children_start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: base_dx,
                dy: 0.0,
                id: arrow_id,
            });
            arena.push_child(ArenaPositionedBox {
                dx: over_dx,
                dy: over_dy,
                id: over_id,
            });
            let children = arena.finish_children(children_start);

            return arena.alloc(ArenaLayoutBox {
                width: max_w,
                height: over_dy + over_h,
                depth: arrow_d,
                kind: ArenaBoxKind::VBox { children },
            });
        }
        label = Some(over_id);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;

    // Check if the over node is a stretchy decoration symbol
    let (over_id, is_line_accent) = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = stretchy_overunder_deco_h(sym, size);
        (
            arena.alloc(ArenaLayoutBox {
                width: deco_w,
                height: deco_h,
                depth: 0.0,
                kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
            }),
            sym == "\u{203E}",
        )
    } else {
        let over_box = label.unwrap_or_else(|| {
            let over_ctx = ctx.with_style(ctx.style.sup());
            layout_node_arena(ast, over, arena, &over_ctx)
        });
        (over_box, false)
    };

    let gap = if is_line_accent {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[2] * size
    };
    stack_single_script_arena(base_id, over_id, AccentPosition::Above, gap, arena)
}

pub(super) fn layout_underscript_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_id = layout_node_arena(ast, under, arena, &under_ctx);
        let under_w = arena.get(under_id).width;
        let under_h = arena.get(under_id).height;
        let under_d = arena.get(under_id).depth;

        if let Some(arrow_w) = extensible_arrow_width(arrow_ch, under_w, size) {
            let (_aw, arrow_a, arrow_d) =
                extensible_arrow_dims(arrow_w, size, p.axis_height * size);
            let arrow_id = arena.alloc(ArenaLayoutBox {
                width: arrow_w,
                height: arrow_a,
                depth: arrow_d,
                kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, size)),
            });

            let gap = p.big_op_spacing[4] * size;
            let max_w = arrow_w.max(under_w);
            let base_dx = (max_w - arrow_w) / 2.0;
            let under_dx = (max_w - under_w) / 2.0;
            let under_dy = arrow_d + gap + under_h;

            let children_start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: base_dx,
                dy: 0.0,
                id: arrow_id,
            });
            arena.push_child(ArenaPositionedBox {
                dx: under_dx,
                dy: -under_dy,
                id: under_id,
            });
            let children = arena.finish_children(children_start);

            return arena.alloc(ArenaLayoutBox {
                width: max_w,
                height: arrow_a,
                depth: under_dy + under_d,
                kind: ArenaBoxKind::VBox { children },
            });
        }
        label = Some(under_id);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;

    let under_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = stretchy_overunder_deco_h(sym, size);
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let under_box = label.unwrap_or_else(|| {
            let under_ctx = ctx.with_style(ctx.style.sub());
            layout_node_arena(ast, under, arena, &under_ctx)
        });
        under_box
    };

    let gap = p.big_op_spacing[4] * size;
    stack_single_script_arena(base_id, under_id, AccentPosition::Below, gap, arena)
}

pub(super) fn stack_single_script_arena(
    base: LayoutBoxId,
    script: LayoutBoxId,
    position: AccentPosition,
    gap: f64,
    arena: &mut LayoutArena,
) -> LayoutBoxId {
    let b = arena.get(base);
    let s = arena.get(script);
    let width = b.width.max(s.width);
    let y = match position {
        AccentPosition::Above => b.height + gap + s.depth,
        AccentPosition::Below => -(b.depth + gap + s.height),
    };
    let (height, depth) = match position {
        AccentPosition::Above => (y + s.height, b.depth),
        AccentPosition::Below => (b.height, s.depth - y),
    };
    let base_x = (width - b.width) / 2.0;
    let script_x = (width - s.width) / 2.0;
    let start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: base_x,
        dy: 0.0,
        id: base,
    });
    arena.push_child(ArenaPositionedBox {
        dx: script_x,
        dy: y,
        id: script,
    });
    let children = arena.finish_children(start);
    arena.alloc(ArenaLayoutBox {
        width,
        height,
        depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

pub(super) fn layout_underover_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // An extensible arrow with a label that does not fit on the glyph.
    let mut label = None;
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_id = layout_node_arena(ast, over, arena, &over_ctx);
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_id = layout_node_arena(ast, under, arena, &under_ctx);

        let over_w = arena.get(over_id).width;
        let over_h = arena.get(over_id).height;
        let over_d = arena.get(over_id).depth;
        let under_w = arena.get(under_id).width;
        let under_h = arena.get(under_id).height;
        let under_d = arena.get(under_id).depth;

        if let Some(arrow_w) = extensible_arrow_width(arrow_ch, over_w.max(under_w), size) {
            let (_aw, arrow_a, arrow_d) =
                extensible_arrow_dims(arrow_w, size, p.axis_height * size);
            let arrow_id = arena.alloc(ArenaLayoutBox {
                width: arrow_w,
                height: arrow_a,
                depth: arrow_d,
                kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, size)),
            });

            let gap_above = p.big_op_spacing[2] * size;
            let gap_below = p.big_op_spacing[4] * size;
            let max_w = arrow_w.max(over_w).max(under_w);
            let base_dx = (max_w - arrow_w) / 2.0;
            let over_dx = (max_w - over_w) / 2.0;
            let under_dx = (max_w - under_w) / 2.0;
            let over_dy = arrow_a + gap_above + over_d;
            let under_dy = arrow_d + gap_below + under_h;

            let children_start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: over_dx,
                dy: over_dy,
                id: over_id,
            });
            arena.push_child(ArenaPositionedBox {
                dx: base_dx,
                dy: 0.0,
                id: arrow_id,
            });
            arena.push_child(ArenaPositionedBox {
                dx: under_dx,
                dy: -under_dy,
                id: under_id,
            });
            let children = arena.finish_children(children_start);

            return arena.alloc(ArenaLayoutBox {
                width: max_w,
                height: over_dy + over_h,
                depth: under_dy + under_d,
                kind: ArenaBoxKind::VBox { children },
            });
        }
        label = Some((over_id, under_id));
    }
    let (label_over, label_under) = label.unzip();

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let (over_id, is_over_line) = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = stretchy_overunder_deco_h(sym, size);
        (
            arena.alloc(ArenaLayoutBox {
                width: deco_w,
                height: deco_h,
                depth: 0.0,
                kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
            }),
            sym == "\u{203E}",
        )
    } else {
        let over_box = label_over.unwrap_or_else(|| {
            let over_ctx = ctx.with_style(ctx.style.sup());
            layout_node_arena(ast, over, arena, &over_ctx)
        });
        (over_box, false)
    };

    let (under_id, is_under_line) = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = stretchy_overunder_deco_h(sym, size);
        (
            arena.alloc(ArenaLayoutBox {
                width: deco_w,
                height: deco_h,
                depth: 0.0,
                kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
            }),
            sym == "\u{203E}",
        )
    } else {
        let under_box = label_under.unwrap_or_else(|| {
            let under_ctx = ctx.with_style(ctx.style.sub());
            layout_node_arena(ast, under, arena, &under_ctx)
        });
        (under_box, false)
    };

    let over_w = arena.get(over_id).width;
    let over_h = arena.get(over_id).height;
    let over_d = arena.get(over_id).depth;
    let under_w = arena.get(under_id).width;
    let under_h = arena.get(under_id).height;
    let under_d = arena.get(under_id).depth;

    let gap_above = if is_over_line {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[2] * size
    };
    let gap_below = if is_under_line {
        3.0 * p.rule_thickness * size
    } else {
        p.big_op_spacing[4] * size
    };
    let max_w = base_w.max(over_w).max(under_w);

    let base_dx = (max_w - base_w) / 2.0;
    let over_dx = (max_w - over_w) / 2.0;
    let under_dx = (max_w - under_w) / 2.0;

    let over_dy = base_h + gap_above + over_d;
    let under_dy = base_d + gap_below + under_h;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: over_dx,
        dy: over_dy,
        id: over_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: base_dx,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: under_dx,
        dy: -under_dy,
        id: under_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: ArenaBoxKind::VBox { children },
    })
}
