use super::RenderOptions;
use super::metrics::{self, GlyphMetrics};
use super::params::TexParams;
use super::spacing::{self, AtomType};
use super::style::MathStyle;
/// Layout engine: converts MathExpr → LayoutBox tree.
/// Applies TeX typesetting rules for positioning.
use crate::ast::*;

/// A positioned layout box in the rendering tree.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub width: f64,
    pub height: f64, // above baseline
    pub depth: f64,  // below baseline
    pub kind: BoxKind,
}

/// The content of a layout box.
#[derive(Debug, Clone)]
pub enum BoxKind {
    /// A single glyph character.
    Glyph { ch: char, size: f64 },
    /// Horizontal box: children laid out left to right.
    HBox { children: Vec<PositionedBox> },
    /// Vertical box: children stacked top to bottom.
    VBox { children: Vec<PositionedBox> },
    /// A horizontal rule (fraction bar, vinculum).
    Rule { thickness: f64 },
    /// Invisible spacing.
    Kern,
    /// A stretchy delimiter.
    Delimiter { ch: char, target_height: f64 },
    /// SVG path data (radical signs, cancel lines).
    SvgPath(String),
    /// Color wrapper.
    Color {
        color: String,
        child: Box<PositionedBox>,
    },
    /// Invisible placeholder (takes space but doesn't render).
    Phantom,
    /// Box with border.
    Boxed {
        child: Box<PositionedBox>,
        padding: f64,
    },
}

/// A box with an offset from its parent's reference point.
#[derive(Debug, Clone)]
pub struct PositionedBox {
    pub dx: f64,
    pub dy: f64, // positive = up from baseline
    pub layout: LayoutBox,
}

impl PositionedBox {
    pub fn new(dx: f64, dy: f64, layout: LayoutBox) -> Self {
        Self { dx, dy, layout }
    }

    pub fn at_origin(layout: LayoutBox) -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            layout,
        }
    }
}

impl LayoutBox {
    pub fn kern(width: f64) -> Self {
        Self {
            width,
            height: 0.0,
            depth: 0.0,
            kind: BoxKind::Kern,
        }
    }

    pub fn rule(width: f64, thickness: f64) -> Self {
        Self {
            width,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            kind: BoxKind::Rule { thickness },
        }
    }

    fn glyph(ch: char, style: MathStyle) -> Self {
        let size = style.size_multiplier();
        let m = metrics::glyph_metrics(ch);
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph { ch, size },
        }
    }

    fn glyph_with_metrics(ch: char, m: GlyphMetrics, size: f64) -> Self {
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph { ch, size },
        }
    }
}

/// Maximum recursion depth for layout to prevent stack overflow on adversarial input.
const MAX_LAYOUT_DEPTH: u32 = 128;

struct LayoutCtx<'a> {
    style: MathStyle,
    params: TexParams,
    options: &'a RenderOptions,
    depth: u32,
}

impl<'a> LayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier()
    }

    fn with_style(&self, style: MathStyle) -> LayoutCtx<'a> {
        LayoutCtx {
            style,
            params: self.params,
            options: self.options,
            depth: self.depth,
        }
    }

    /// Return a new context with incremented depth for recursion tracking.
    fn deeper(&self) -> LayoutCtx<'a> {
        LayoutCtx {
            style: self.style,
            params: self.params,
            options: self.options,
            depth: self.depth + 1,
        }
    }
}

/// Entry point: layout an entire math expression.
pub fn layout_expr(expr: &MathExpr, style: MathStyle, options: &RenderOptions) -> LayoutBox {
    let ctx = LayoutCtx {
        style,
        params: TexParams::default(),
        options,
        depth: 0,
    };
    layout_row(&expr.nodes, &ctx)
}

fn layout_row(nodes: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    if nodes.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for node in nodes {
        // Inter-atom spacing
        let cur_atom = spacing::classify_node(node);
        if let Some(prev) = prev_atom {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.style);
            if space > 0.0 {
                let kern_width = space * ctx.size();
                cursor_x += kern_width;
            }
        }

        let layout = layout_node(node, ctx);
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);

        // Capture width before moving layout into children vec
        let layout_width = layout.width;
        children.push(PositionedBox::new(cursor_x, 0.0, layout));
        cursor_x += layout_width;
        prev_atom = Some(cur_atom);
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_node(node: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Guard against stack overflow from deeply nested input (e.g. \frac{\frac{...}{...}}{...})
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return LayoutBox::kern(0.0);
    }
    let ctx = &ctx.deeper();
    match node {
        MathNode::Number(s) => layout_text_run(s, ctx, false),
        MathNode::Identifier(s) => {
            if s.chars().count() == 1 {
                // Single-char identifiers are italic; chars().next() is safe
                // because chars().count() == 1 guarantees at least one char
                LayoutBox::glyph(s.chars().next().expect("non-empty checked"), ctx.style)
            } else {
                layout_text_run(s, ctx, false)
            }
        },
        MathNode::Operator(op) => layout_operator(op, ctx),
        MathNode::Symbol(_, s) => {
            if let Some(ch) = s.chars().next() {
                LayoutBox::glyph(ch, ctx.style)
            } else {
                LayoutBox::kern(0.0)
            }
        },
        MathNode::Text(_, s) => layout_text_run(s, ctx, false),
        MathNode::Space(r) => LayoutBox::kern(r.as_f64() * ctx.size()),
        MathNode::Row(children) => layout_row(children, ctx),
        MathNode::Fenced {
            open,
            close,
            content,
        } => layout_fenced(open, close, content, ctx),
        MathNode::Subscript { base, sub } => layout_subscript(base, sub, ctx),
        MathNode::Superscript { base, sup } => layout_superscript(base, sup, ctx),
        MathNode::SubSup { base, sub, sup } => layout_subsup(base, sub, sup, ctx),
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => layout_prescripts(base, pre_sub, pre_sup, ctx),
        MathNode::Overscript { base, over, .. } => layout_overscript(base, over, ctx),
        MathNode::Underscript { base, under, .. } => layout_underscript(base, under, ctx),
        MathNode::UnderOver {
            base, under, over, ..
        } => layout_underover(base, under, over, ctx),
        MathNode::Fraction { kind, num, den } => layout_fraction(*kind, num, den, ctx),
        MathNode::Sqrt(radicand) => layout_sqrt(radicand, None, ctx),
        MathNode::Root { index, radicand } => layout_sqrt(radicand, Some(index), ctx),
        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => layout_nary(*op, sub, sup, body, *limits, ctx),
        MathNode::Accent {
            base,
            accent,
            position,
        } => layout_accent(base, *accent, *position, ctx),
        MathNode::Array { columns, rows } => layout_array(columns, rows, ctx),
        MathNode::Phantom(content) => layout_phantom(content, ctx),
        MathNode::Boxed(content) => layout_boxed(content, ctx),
        MathNode::Cancel { stroke, content } => layout_cancel(*stroke, content, ctx),
        MathNode::Scaled { factor, content } => layout_scaled(*factor, content, ctx),
        MathNode::Styled(style, children) => layout_styled(*style, children, ctx),
    }
}

// === Leaf node layouts ===

fn layout_text_run(text: &str, ctx: &LayoutCtx, _italic: bool) -> LayoutBox {
    let size = ctx.size();
    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        let m = metrics::glyph_metrics(ch);
        let glyph = LayoutBox::glyph_with_metrics(ch, m, size);
        max_height = max_height.max(glyph.height);
        max_depth = max_depth.max(glyph.depth);
        children.push(PositionedBox::new(cursor_x, 0.0, glyph));
        cursor_x += m.width * size;
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_operator(op: &OperatorAttr, ctx: &LayoutCtx) -> LayoutBox {
    layout_text_run(&op.name, ctx, false)
}

// === Structure node layouts ===

/// TeXBook Rule 18a: superscript positioning.
fn layout_superscript(base: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
    // Superscript shift: max of sup1/sup2/sup3, base.height - sup_drop, x_height/4 + sup.depth
    let shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    let shift_up = shift_up
        .max(base_box.height - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_box.depth);

    let total_width = base_box.width + sup_box.width;
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
fn layout_subscript(base: &MathNode, sub: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    let shift_down = p
        .sub1
        .max(base_box.depth + p.sub_drop * ctx.size())
        .max(sub_box.height - (4.0 * p.x_height / 5.0));

    let total_width = base_box.width + sub_box.width;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 18c: combined sub+superscript.
fn layout_subsup(base: &MathNode, sub: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;

    // Superscript shift
    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    shift_up = shift_up
        .max(base_box.height - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_box.depth);

    // Subscript shift
    let mut shift_down = p.sub2.max(base_box.depth + p.sub_drop * ctx.size());

    // Ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

    // Ensure sup doesn't go above (4/5)x_height
    let sup_bottom = shift_up - sup_box.depth;
    if sup_bottom < (4.0 * p.x_height / 5.0) {
        let delta = (4.0 * p.x_height / 5.0) - sup_bottom;
        shift_up += delta;
    }

    let script_width = sup_box.width.max(sub_box.width);
    let total_width = base_box.width + script_width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width, shift_up, sup_box),
        PositionedBox::new(base_width, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 15: fraction layout.
fn layout_fraction(
    kind: FractionKind,
    num: &MathNode,
    den: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
    let num_box = layout_node(num, &num_ctx);
    let den_box = layout_node(den, &den_ctx);

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    // Shifts from axis
    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
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
    let total_height = num_up + num_box.height;
    let total_depth = den_down + den_box.depth;

    // Center numerator and denominator
    let num_dx = (frac_width - num_box.width) / 2.0;
    let den_dx = (frac_width - den_box.width) / 2.0;

    let mut children = vec![
        PositionedBox::new(num_dx, num_up, num_box),
        PositionedBox::new(den_dx, -den_down, den_box),
    ];

    // Add the fraction bar
    if bar_thickness > 0.0 {
        let bar = LayoutBox::rule(frac_width, bar_thickness);
        children.push(PositionedBox::new(0.0, axis, bar));
    }

    LayoutBox {
        width: frac_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

/// TeXBook Rule 11: square root.
fn layout_sqrt(radicand: &MathNode, index: Option<&MathNode>, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let inner = layout_node(radicand, ctx);

    let rule_thickness = p.rule_thickness * size;
    let extra_clearance = if ctx.style.is_display() {
        rule_thickness
    } else {
        rule_thickness / 2.0
    };

    // Total height the radical must cover
    let inner_total = inner.height + inner.depth + extra_clearance + rule_thickness;

    // Radical sign
    let radical_width = 0.833 * size; // approximate radical sign width
    let radical = LayoutBox {
        width: radical_width,
        height: inner_total,
        depth: 0.0,
        kind: BoxKind::SvgPath(format!(
            "M{x0},{y0} L{x1},{y1} L{x2},{y2} L{x3},{y3}",
            x0 = 0.0,
            y0 = inner_total * 0.6,
            x1 = radical_width * 0.35,
            y1 = inner_total,
            x2 = radical_width * 0.65,
            y2 = 0.0,
            x3 = radical_width,
            y3 = 0.0,
        )),
    };

    // Vinculum (horizontal rule on top)
    let vinculum = LayoutBox::rule(inner.width, rule_thickness);

    let shift_up = inner.height + extra_clearance + rule_thickness;

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_depth = inner.depth;
    let mut children = vec![
        PositionedBox::new(0.0, 0.0, radical),
        PositionedBox::new(radical_width, shift_up, vinculum),
        PositionedBox::new(radical_width, 0.0, inner),
    ];

    let mut total_width = radical_width + inner_width;
    let total_height = shift_up + rule_thickness;
    let total_depth = inner_depth;

    // Optional root index
    if let Some(idx) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box = layout_node(idx, &idx_ctx);
        let idx_shift = total_height * 0.6; // raise index
        let idx_width = idx_box.width;
        children.push(PositionedBox::new(0.0, idx_shift, idx_box));
        // Adjust if index overflows to the left
        let idx_overflow = idx_width.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
            // Shift everything right by extra
            for child in &mut children[..3] {
                child.dx += extra;
            }
            total_width += extra;
        }
    }

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

// === Remaining node layouts ===

fn layout_fenced(open: &str, close: &str, content: &[FencedItem], ctx: &LayoutCtx) -> LayoutBox {
    // Layout content
    let inner_nodes: Vec<&MathNode> = content
        .iter()
        .filter_map(|item| match item {
            FencedItem::Element(n) => Some(n),
            FencedItem::Middle(_) => None,
        })
        .collect();

    let mut inner_children = Vec::new();
    for n in &inner_nodes {
        inner_children.push(layout_node(n, ctx));
    }

    // Compute max height/depth of content
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for child in &inner_children {
        max_h = max_h.max(child.height);
        max_d = max_d.max(child.depth);
    }

    let target_h = max_h + max_d;

    // Build positioned children
    let mut children = Vec::new();
    let mut cursor_x = 0.0;

    // Open delimiter
    if !open.is_empty()
        && open != "."
        && let Some(ch) = open.chars().next()
    {
        let delim = LayoutBox {
            width: 0.389 * ctx.size(),
            height: max_h,
            depth: max_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    // Content — capture width before moving each child
    for child in inner_children {
        let w = child.width;
        children.push(PositionedBox::new(cursor_x, 0.0, child));
        cursor_x += w;
    }

    // Close delimiter
    if !close.is_empty()
        && close != "."
        && let Some(ch) = close.chars().next()
    {
        let delim = LayoutBox {
            width: 0.389 * ctx.size(),
            height: max_h,
            depth: max_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    LayoutBox {
        width: cursor_x,
        height: max_h,
        depth: max_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_nary(
    op: char,
    sub: &Option<Box<MathNode>>,
    sup: &Option<Box<MathNode>>,
    body: &MathNode,
    limits: LimitStyle,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Large op in display mode
    let op_size = if ctx.style.is_display() {
        size * 1.2
    } else {
        size
    };
    let m = metrics::glyph_metrics(op);
    let op_box = LayoutBox::glyph_with_metrics(op, m, op_size);

    let body_box = layout_node(body, ctx);
    // Capture body dimensions before any move
    let body_w = body_box.width;
    let body_h = body_box.height;
    let body_d = body_box.depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        LimitStyle::Default => ctx.style.is_display(),
    };

    if use_limits {
        // Limits above/below
        let mut children = Vec::new();
        let mut total_h = op_box.height;
        let mut total_d = op_box.depth;
        let mut max_w = op_box.width;

        let sup_box = sup.as_ref().map(|s| {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            layout_node(s, &sup_ctx)
        });
        let sub_box = sub.as_ref().map(|s| {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            layout_node(s, &sub_ctx)
        });

        if let Some(ref sb) = sup_box {
            max_w = max_w.max(sb.width);
            total_h += sb.height + sb.depth + p.big_op_spacing[0] * size;
        }
        if let Some(ref sb) = sub_box {
            max_w = max_w.max(sb.width);
            total_d += sb.height + sb.depth + p.big_op_spacing[3] * size;
        }

        // Center op
        let op_dx = (max_w - op_box.width) / 2.0;

        if let Some(sb) = sup_box {
            let sup_dx = (max_w - sb.width) / 2.0;
            let sup_dy = op_box.height + p.big_op_spacing[0] * size + sb.depth;
            children.push(PositionedBox::new(sup_dx, sup_dy, sb));
        }
        let op_depth = op_box.depth;
        children.push(PositionedBox::new(op_dx, 0.0, op_box));
        if let Some(sb) = sub_box {
            let sub_dx = (max_w - sb.width) / 2.0;
            let sub_dy = -(op_depth + p.big_op_spacing[3] * size + sb.height);
            children.push(PositionedBox::new(sub_dx, sub_dy, sb));
        }

        // Body follows
        let nary_width = max_w;
        let thin_space = 3.0 / 18.0 * size;
        children.push(PositionedBox::new(nary_width + thin_space, 0.0, body_box));
        LayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h,
            depth: total_d,
            kind: BoxKind::HBox { children },
        }
    } else {
        // As sub/superscripts — helper to build "result + thin_space + body" hbox
        let node = MathNode::Symbol(SymbolKind::LargeOp, op.to_string());
        let thin_space = 3.0 / 18.0 * size;

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
            let result = layout_subsup(&node, sub_n, sup_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sub_n) = sub {
            let result = layout_subscript(&node, sub_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sup_n) = sup {
            let result = layout_superscript(&node, sup_n, ctx);
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

fn layout_overscript(base: &MathNode, over: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node(base, ctx);
    let over_ctx = ctx.with_style(ctx.style.sup());
    let over_box = layout_node(over, &over_ctx);

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_box.width.max(over_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let over_dy = base_box.height + gap + over_box.depth;

    // Capture dimensions before moving into children
    let over_h = over_box.height;
    let base_d = base_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(over_dx, over_dy, over_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: base_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underscript(base: &MathNode, under: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node(base, ctx);
    let under_ctx = ctx.with_style(ctx.style.sub());
    let under_box = layout_node(under, &under_ctx);

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(under_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;
    let under_dy = base_box.depth + gap + under_box.height;

    // Capture dimensions before moving into children
    let base_h = base_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: base_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underover(
    base: &MathNode,
    under: &MathNode,
    over: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node(base, ctx);
    let over_ctx = ctx.with_style(ctx.style.sup());
    let under_ctx = ctx.with_style(ctx.style.sub());
    let over_box = layout_node(over, &over_ctx);
    let under_box = layout_node(under, &under_ctx);

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
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

fn layout_accent(
    base: &MathNode,
    accent: char,
    position: AccentPosition,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let accent_box = LayoutBox::glyph(accent, ctx.style);

    let accent_dx = (base_box.width - accent_box.width) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_box.height + 0.02 * ctx.size(); // small gap
            (dy, dy + accent_box.height, base_box.depth)
        },
        AccentPosition::Below => {
            let dy = -(base_box.depth + accent_box.height + 0.02 * ctx.size());
            (
                dy,
                base_box.height,
                base_box.depth + accent_box.height + 0.02 * ctx.size(),
            )
        },
    };

    // Capture width before moving base_box into children vec
    let base_width = base_box.width;

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(accent_dx, accent_dy, accent_box),
    ];

    LayoutBox {
        width: base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_array(
    columns: &[ColumnAlign],
    rows: &[Vec<Vec<MathNode>>],
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let row_gap = 0.6 * size; // jot
    let col_gap = 1.0 * size; // column separation

    // Layout all cells
    let mut cell_layouts: Vec<Vec<LayoutBox>> = Vec::new();
    for row in rows {
        let mut row_layouts = Vec::new();
        for cell in row {
            row_layouts.push(layout_row(cell, ctx));
        }
        cell_layouts.push(row_layouts);
    }

    let n_cols = columns
        .len()
        .max(cell_layouts.iter().map(|r| r.len()).max().unwrap_or(0));
    let n_rows = cell_layouts.len();

    // Compute column widths and row heights/depths
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![0.0_f64; n_rows];
    let mut row_depths = vec![0.0_f64; n_rows];

    for (i, row) in cell_layouts.iter().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            col_widths[j] = col_widths[j].max(cell.width);
            row_heights[i] = row_heights[i].max(cell.height);
            row_depths[i] = row_depths[i].max(cell.depth);
        }
    }

    // Position cells — consume cell_layouts by ownership to avoid cloning
    let mut children = Vec::new();
    let mut cursor_y = 0.0;

    // Start from top
    for (i, row) in cell_layouts.into_iter().enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut row_iter = row.into_iter();

        for (j, &col_w) in col_widths.iter().enumerate() {
            if let Some(cell) = row_iter.next() {
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            }
            cursor_x += col_w;
            if j < n_cols - 1 {
                cursor_x += col_gap;
            }
        }

        cursor_y += row_depths[i];
        if i < n_rows - 1 {
            cursor_y += row_gap;
        }
    }

    let total_width: f64 =
        col_widths.iter().sum::<f64>() + col_gap * (n_cols.saturating_sub(1) as f64);
    let total_extent = cursor_y;
    // Center vertically around the axis
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;

    LayoutBox {
        width: total_width,
        height: half + axis,
        depth: half - axis,
        kind: BoxKind::VBox { children },
    }
}

fn layout_prescripts(
    base: &MathNode,
    pre_sub: &Option<Box<MathNode>>,
    pre_sup: &Option<Box<MathNode>>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let p = &ctx.params;

    let mut children = Vec::new();
    let mut pre_width: f64 = 0.0;
    let mut total_h = base_box.height;
    let mut total_d = base_box.depth;

    if let Some(sup) = pre_sup {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        let sup_box = layout_node(sup, &sup_ctx);
        let shift_up = p.sup2.max(base_box.height - p.sup_drop * ctx.size());
        pre_width = pre_width.max(sup_box.width);
        total_h = total_h.max(shift_up + sup_box.height);
        children.push(PositionedBox::new(0.0, shift_up, sup_box));
    }
    if let Some(sub) = pre_sub {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        let sub_box = layout_node(sub, &sub_ctx);
        let shift_down = p.sub1.max(base_box.depth + p.sub_drop * ctx.size());
        pre_width = pre_width.max(sub_box.width);
        total_d = total_d.max(shift_down + sub_box.depth);
        children.push(PositionedBox::new(0.0, -shift_down, sub_box));
    }

    // Capture width before moving base_box into children vec
    let base_width = base_box.width;
    children.push(PositionedBox::new(pre_width, 0.0, base_box));

    LayoutBox {
        width: pre_width + base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_phantom(content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    LayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: BoxKind::Phantom,
    }
}

fn layout_boxed(content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner = layout_node(content, ctx);

    LayoutBox {
        width: inner.width + 2.0 * padding,
        height: inner.height + padding,
        depth: inner.depth + padding,
        kind: BoxKind::Boxed {
            child: Box::new(PositionedBox::new(padding, 0.0, inner)),
            padding,
        },
    }
}

fn layout_cancel(stroke: StrokeKind, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let w = inner.width;
    let h = inner.height + inner.depth;

    let path = match stroke {
        StrokeKind::Forward => format!("M0,{h} L{w},0"),
        StrokeKind::Backward => format!("M0,0 L{w},{h}"),
        StrokeKind::Cross => format!("M0,{h} L{w},0 M0,0 L{w},{h}"),
        StrokeKind::Horizontal => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        StrokeKind::Vertical => {
            let mid = w / 2.0;
            format!("M{mid},0 L{mid},{h}")
        },
    };

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_height = inner.height;
    let inner_depth = inner.depth;

    let children = vec![
        PositionedBox::at_origin(inner),
        PositionedBox::at_origin(LayoutBox {
            width: w,
            height: inner_height,
            depth: inner_depth,
            kind: BoxKind::SvgPath(path),
        }),
    ];

    LayoutBox {
        width: inner_width,
        height: inner_height,
        depth: inner_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_scaled(factor: Rational, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: inner.kind,
    }
}

fn layout_styled(_style: TextStyle, children: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    // For now, just layout children normally.
    // Text style affects glyph selection (bold/italic) — full implementation
    // would map styled chars through unicode::fonts::styled_char.
    layout_row(children, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> RenderOptions {
        RenderOptions::default()
    }

    #[test]
    fn layout_simple_number() {
        let expr = MathExpr::inline(vec![MathNode::Number("42".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_fraction_has_depth() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_superscript_raises() {
        let expr = MathExpr::inline(vec![MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Width should be base + sup
        assert!(root.width > 0.5);
    }

    #[test]
    fn empty_row_has_zero_width() {
        let expr = MathExpr::inline(vec![]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert_eq!(root.width, 0.0);
    }

    #[test]
    fn layout_subscript_lowers() {
        let expr = MathExpr::inline(vec![MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.5);
        assert!(root.depth > 0.0, "subscript should add depth");
    }

    #[test]
    fn layout_subsup_combined() {
        let expr = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.5);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0, "subsup should have both height and depth");
    }

    #[test]
    fn layout_fraction_display_wider_than_text() {
        let frac = vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }];
        let expr_d = MathExpr::inline(frac.clone());
        let expr_t = MathExpr::inline(frac);
        let opts = default_options();
        let display = layout_expr(&expr_d, MathStyle::Display, &opts);
        let text = layout_expr(&expr_t, MathStyle::Text, &opts);
        // Display fractions have larger numerator/denominator spacing
        assert!(
            display.height + display.depth >= text.height + text.depth,
            "display fraction should be at least as tall as text fraction"
        );
    }

    #[test]
    fn layout_sqrt_has_positive_dimensions() {
        let expr = MathExpr::inline(vec![MathNode::Sqrt(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_sqrt_with_index() {
        let expr = MathExpr::inline(vec![MathNode::Root {
            index: Box::new(MathNode::Number("3".into())),
            radicand: Box::new(MathNode::Identifier("x".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_fenced_wider_than_content() {
        let content_nodes = vec![MathNode::Identifier("x".into())];
        let content_expr = MathExpr::inline(content_nodes.clone());
        let content_only = layout_expr(&content_expr, MathStyle::Text, &default_options());

        let fenced_nodes = vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        }];
        let fenced_expr = MathExpr::inline(fenced_nodes);
        let fenced = layout_expr(&fenced_expr, MathStyle::Text, &default_options());

        assert!(
            fenced.width > content_only.width,
            "fenced expression should be wider than content alone (delimiters add width)"
        );
    }

    #[test]
    fn layout_array_grid() {
        let expr = MathExpr::inline(vec![MathNode::Array {
            rows: vec![
                vec![
                    vec![MathNode::Identifier("a".into())],
                    vec![MathNode::Identifier("b".into())],
                ],
                vec![
                    vec![MathNode::Identifier("c".into())],
                    vec![MathNode::Identifier("d".into())],
                ],
            ],
            columns: vec![ColumnAlign::Center, ColumnAlign::Center],
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_nary_display_has_limits() {
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_accent_over_base() {
        let expr = MathExpr::inline(vec![MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '\u{0302}', // combining circumflex
            position: AccentPosition::Above,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_overscript() {
        let expr = MathExpr::inline(vec![MathNode::Overscript {
            base: Box::new(MathNode::Identifier("x".into())),
            over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "→".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_underscript() {
        let expr = MathExpr::inline(vec![MathNode::Underscript {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "_".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_underover() {
        let expr = MathExpr::inline(vec![MathNode::UnderOver {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(MathNode::Number("1".into())),
            over: Box::new(MathNode::Number("2".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_phantom_has_dimensions_but_invisible() {
        let content = MathNode::Identifier("x".into());
        let visible = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Phantom(Box::new(content.clone()))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(
            (root.width - visible.width).abs() < 0.01,
            "phantom should have same width as visible content"
        );
        if let BoxKind::HBox { children } = &root.kind {
            let phantom_kind = &children[0].layout.kind;
            assert!(matches!(phantom_kind, BoxKind::Phantom));
        }
    }

    #[test]
    fn layout_boxed_adds_padding() {
        let content = MathNode::Identifier("x".into());
        let inner = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Boxed(Box::new(content.clone()))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Boxed should be wider than inner due to padding
        assert!(
            root.width > inner.width,
            "boxed should add padding to width"
        );
    }

    #[test]
    fn layout_cancel_preserves_dimensions() {
        let expr = MathExpr::inline(vec![MathNode::Cancel {
            stroke: StrokeKind::Forward,
            content: Box::new(MathNode::Identifier("x".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_scaled_doubles_dimensions() {
        let content = MathNode::Identifier("x".into());
        let normal = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Scaled {
            factor: Rational::new(2, 1),
            content: Box::new(content.clone()),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Width should be approximately double (within the HBox wrapper)
        if let BoxKind::HBox { children } = &root.kind {
            let scaled = &children[0].layout;
            assert!(
                (scaled.width - normal.width * 2.0).abs() < 0.01,
                "scaled by 2 should double the width"
            );
        }
    }

    #[test]
    fn layout_text_upright() {
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, "abc".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_space_kern() {
        let expr = MathExpr::inline(vec![MathNode::Space(Rational::new(1, 2))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0, "space should produce width");
    }

    #[test]
    fn layout_identifier_single_char() {
        let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_operator_large_in_display() {
        let expr = MathExpr::inline(vec![MathNode::Operator(OperatorAttr {
            name: "∑".into(),
            limit_style: LimitStyle::Default,
        })]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        assert!(
            display.height >= text.height,
            "large op in display should be at least as tall as in text"
        );
    }

    #[test]
    fn layout_row_with_spacing() {
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("y".into()),
        ]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Row with binary op spacing should be wider than just x and y
        let x_expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let y_expr = MathExpr::inline(vec![MathNode::Identifier("y".into())]);
        let x_w = layout_expr(&x_expr, MathStyle::Text, &default_options()).width;
        let y_w = layout_expr(&y_expr, MathStyle::Text, &default_options()).width;
        assert!(
            root.width > x_w + y_w,
            "row with operator should include spacing"
        );
    }

    #[test]
    fn layout_prescripts_before_base() {
        let expr = MathExpr::inline(vec![MathNode::PreScripts {
            base: Box::new(MathNode::Identifier("C".into())),
            pre_sub: Some(Box::new(MathNode::Number("6".into()))),
            pre_sup: Some(Box::new(MathNode::Number("12".into()))),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_styled_children() {
        let expr = MathExpr::inline(vec![MathNode::Styled(
            TextStyle::Bold,
            vec![MathNode::Identifier("x".into())],
        )]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_symbol_various_kinds() {
        for kind in [
            SymbolKind::Ordinary,
            SymbolKind::BinaryOp,
            SymbolKind::Relation,
            SymbolKind::Open,
            SymbolKind::Close,
            SymbolKind::Punctuation,
        ] {
            let expr = MathExpr::inline(vec![MathNode::Symbol(kind, "+".into())]);
            let root = layout_expr(&expr, MathStyle::Text, &default_options());
            assert!(root.width > 0.0, "symbol kind {:?} should have width", kind);
        }
    }

    #[test]
    fn layout_deeply_nested_fraction() {
        let inner_frac = MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        };
        let outer_frac = MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(inner_frac),
            den: Box::new(MathNode::Number("3".into())),
        };
        let expr = MathExpr::inline(vec![outer_frac]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_fraction_nobar() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::NoBar,
            num: Box::new(MathNode::Number("n".into())),
            den: Box::new(MathNode::Number("k".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn script_style_shrinks() {
        let base_expr = MathExpr::inline(vec![MathNode::Number("2".into())]);
        let opts = default_options();
        let text_box = layout_expr(&base_expr, MathStyle::Text, &opts);
        let script_box = layout_expr(&base_expr, MathStyle::Script, &opts);
        assert!(
            script_box.width < text_box.width,
            "script style should produce smaller glyphs"
        );
    }
}
