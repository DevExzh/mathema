use smallvec::SmallVec;
use super::RenderOptions;
use super::metrics::{self, GlyphMetrics};
use super::params::TexParams;
use super::spacing::{self, AtomType};
use super::style::MathStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
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
    #[inline]
    pub fn new(dx: f64, dy: f64, layout: LayoutBox) -> Self {
        Self { dx, dy, layout }
    }

    #[inline]
    pub fn at_origin(layout: LayoutBox) -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            layout,
        }
    }
}

impl LayoutBox {
    #[inline]
    pub fn kern(width: f64) -> Self {
        Self {
            width,
            height: 0.0,
            depth: 0.0,
            kind: BoxKind::Kern,
        }
    }

    #[inline]
    pub fn rule(width: f64, thickness: f64) -> Self {
        Self {
            width,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            kind: BoxKind::Rule { thickness },
        }
    }

    #[inline]
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

    #[inline]
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

/// Scale factor for big operators in display mode (TeX uses ~1.2x for display-size operators).
const BIG_OP_DISPLAY_SCALE: f64 = 1.2;

/// Check whether a MathNode is a large (n-ary) operator symbol.
fn is_large_op(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(SymbolKind::LargeOp, _))
}

/// Check whether a CompactNode is a large (n-ary) operator symbol.
fn is_large_op_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(arena.node(id), CompactNode::Symbol(SymbolKind::LargeOp, _))
}

/// Extract the first character from a large-op symbol node.
fn large_op_char(node: &MathNode) -> Option<char> {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, s) => s.chars().next(),
        _ => None,
    }
}

/// Extract the first character from a large-op compact symbol node.
fn large_op_char_c(arena: &AstArena, id: NodeId) -> Option<char> {
    match arena.node(id) {
        CompactNode::Symbol(SymbolKind::LargeOp, sid) => arena.str_val(*sid).chars().next(),
        _ => None,
    }
}

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

// ── Stretchy decoration SVG path helpers ──────────────────────────────

/// Returns `true` if the accent char should be drawn as a stretchy SVG path
/// that scales to the base width, rather than as a fixed glyph.
fn is_stretchy_accent(accent: char) -> bool {
    matches!(
        accent,
        '\u{0302}'  // hat / widehat (combining circumflex)
        | '\u{030C}' // check / widecheck (combining caron)
        | '\u{0303}' // tilde / widetilde (combining tilde)
        | '\u{20D7}' // vec / overrightarrow (combining right arrow above)
        | '\u{2192}' // overrightarrow (rightwards arrow)
        | '\u{2190}' // overleftarrow (leftwards arrow)
        | '\u{2194}' // overleftrightarrow (left right arrow)
        | '\u{21C0}' // overrightharpoon (rightwards harpoon)
        | '\u{21BC}' // overleftharpoon (leftwards harpoon)
    )
}

/// Returns `true` if the over/under symbol char is a stretchy decoration
/// (brace, bracket, overline) that should scale to the base width.
fn is_stretchy_overunder(sym: &str) -> bool {
    matches!(
        sym,
        "\u{23DE}"  // overbrace ⏞
        | "\u{23DF}" // underbrace ⏟
        | "\u{23B4}" // overbracket ⎴
        | "\u{23B5}" // underbracket ⎵
        | "\u{203E}" // overline ‾
    )
}

/// Generate an SVG path string for a stretchy accent at the given width and height.
/// Coordinates are in em units with (0,0) at top-left.
fn stretchy_accent_path(accent: char, w: f64, h: f64) -> String {
    let stroke = 0.04; // matches emit.rs stroke-width
    match accent {
        // Hat: ∧ shape centered above content
        '\u{0302}' => {
            let mid = w / 2.0;
            format!("M0,{h} L{mid},0 L{w},{h}")
        },
        // Check/caron: ∨ shape
        '\u{030C}' => {
            let mid = w / 2.0;
            format!("M0,0 L{mid},{h} L{w},0")
        },
        // Tilde: wavy S-curve
        '\u{0303}' => {
            let q1x = w * 0.25;
            let q2x = w * 0.75;
            let mid = h / 2.0;
            format!(
                "M0,{mid} C{q1x},0 {q2x},{h} {w},{mid}"
            )
        },
        // Right arrow (overrightarrow, vec)
        '\u{20D7}' | '\u{2192}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let tip = w;
            let shaft_end = w - head;
            format!(
                "M0,{mid} L{shaft_end},{mid} M{tip},{mid} L{se},{t} M{tip},{mid} L{se},{b}",
                se = shaft_end,
                t = mid - head,
                b = mid + head,
            )
        },
        // Left arrow (overleftarrow)
        '\u{2190}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_start = head;
            format!(
                "M{shaft_start},{mid} L{w},{mid} M0,{mid} L{shaft_start},{t} M0,{mid} L{shaft_start},{b}",
                t = mid - head,
                b = mid + head,
            )
        },
        // Left-right arrow
        '\u{2194}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.12).max(stroke * 3.0);
            let shaft_start = head;
            let shaft_end = w - head;
            format!(
                "M{shaft_start},{mid} L{shaft_end},{mid} \
                 M0,{mid} L{shaft_start},{t} M0,{mid} L{shaft_start},{b} \
                 M{w},{mid} L{shaft_end},{t} M{w},{mid} L{shaft_end},{b}",
                t = mid - head,
                b = mid + head,
            )
        },
        // Right harpoon (upper barb)
        '\u{21C0}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_end = w - head;
            format!(
                "M0,{mid} L{shaft_end},{mid} M{w},{mid} L{shaft_end},{t}",
                t = mid - head,
            )
        },
        // Left harpoon (upper barb)
        '\u{21BC}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_start = head;
            format!(
                "M{shaft_start},{mid} L{w},{mid} M0,{mid} L{shaft_start},{t}",
                t = mid - head,
            )
        },
        // Fallback: horizontal line
        _ => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
    }
}

/// Generate an SVG path string for a stretchy over/under decoration.
/// Coordinates are in em units with (0,0) at top-left.
fn stretchy_overunder_path(sym: &str, w: f64, h: f64) -> String {
    match sym {
        // Overbrace ⏞ — curly brace above, tip points up
        "\u{23DE}" => {
            let mid = w / 2.0;
            let q = w * 0.25;
            let tq = w * 0.75;
            format!(
                "M0,{h} Q{q},{h} {mid},0 Q{tq},{h} {w},{h}"
            )
        },
        // Underbrace ⏟ — curly brace below, tip points down
        "\u{23DF}" => {
            let mid = w / 2.0;
            let q = w * 0.25;
            let tq = w * 0.75;
            format!(
                "M0,0 Q{q},0 {mid},{h} Q{tq},0 {w},0"
            )
        },
        // Overbracket ⎴ — square bracket above
        "\u{23B4}" => {
            format!(
                "M0,{h} L0,0 L{w},0 L{w},{h}"
            )
        },
        // Underbracket ⎵ — square bracket below
        "\u{23B5}" => {
            format!(
                "M0,0 L0,{h} L{w},{h} L{w},0"
            )
        },
        // Overline ‾ — horizontal rule
        "\u{203E}" => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        _ => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
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
        MathNode::Symbol(kind, s) => {
            if let Some(ch) = s.chars().next() {
                if *kind == SymbolKind::LargeOp && ctx.style.is_display() {
                    let size = ctx.size() * BIG_OP_DISPLAY_SCALE;
                    let m = metrics::glyph_metrics(ch);
                    LayoutBox::glyph_with_metrics(ch, m, size)
                } else {
                    LayoutBox::glyph(ch, ctx.style)
                }
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
/// When the base is a large operator in display mode, place the superscript above as a limit.
fn layout_superscript(base: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, None, Some(sup), ctx);
    }
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
/// When the base is a large operator in display mode, place the subscript below as a limit.
fn layout_subscript(base: &MathNode, sub: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, Some(sub), None, ctx);
    }
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
/// When the base is a large operator in display mode, place limits above/below.
fn layout_subsup(base: &MathNode, sub: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place limits above/below
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, Some(sub), Some(sup), ctx);
    }
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

/// Layout a big operator (LargeOp symbol) with limits above/below (heap path).
/// Used when SubSup/Subscript/Superscript has a large-op base in display mode.
fn layout_big_op_limits_heap(
    base: &MathNode,
    sub: Option<&MathNode>,
    sup: Option<&MathNode>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char(base).unwrap_or('\u{2211}');
    let op_size = size * BIG_OP_DISPLAY_SCALE;
    let m = metrics::glyph_metrics(op_ch);
    let op_box = LayoutBox::glyph_with_metrics(op_ch, m, op_size);

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

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + p.big_op_spacing[0] * size;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + p.big_op_spacing[3] * size;
    }

    // Center op horizontally
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

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
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
        // As sub/superscripts — use the already-scaled op_box
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
            let result = layout_subsup_from_box_heap(op_box, sub_n, sup_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sub_n) = sub {
            let result = layout_subscript_from_box_heap(op_box, sub_n, ctx);
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

/// Helper: superscript layout where the base is an already-laid-out LayoutBox (heap path).
fn layout_superscript_from_box_heap(
    base_box: LayoutBox,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
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
fn layout_subscript_from_box_heap(
    base_box: LayoutBox,
    sub: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
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

/// Helper: combined sub+superscript layout where the base is an already-laid-out LayoutBox (heap path).
fn layout_subsup_from_box_heap(
    base_box: LayoutBox,
    sub: &MathNode,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;

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

    let mut shift_down = p.sub2.max(base_box.depth + p.sub_drop * ctx.size());

    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

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

/// Extract the symbol string from a MathNode if it is a stretchy overunder decoration.
fn get_stretchy_overunder_sym(node: &MathNode) -> Option<&str> {
    if let MathNode::Symbol(_, s) = node
        && is_stretchy_overunder(s)
    {
        return Some(s);
    }
    None
}

/// Create a stretchy overunder decoration LayoutBox at the given width.
fn make_stretchy_overunder_box(sym: &str, width: f64, size: f64) -> LayoutBox {
    let deco_h = 0.12 * size;
    LayoutBox {
        width,
        height: deco_h,
        depth: 0.0,
        kind: BoxKind::SvgPath(stretchy_overunder_path(sym, width, deco_h)),
    }
}

fn layout_overscript(base: &MathNode, over: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    // Check if the over decoration is a stretchy symbol that should scale to base width
    let over_box = if let Some(sym) = get_stretchy_overunder_sym(over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node(over, &over_ctx)
    };

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
    let base_w = base_box.width;

    // Check if the under decoration is a stretchy symbol that should scale to base width
    let under_box = if let Some(sym) = get_stretchy_overunder_sym(under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node(under, &under_ctx)
    };

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
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym(over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node(over, &over_ctx)
    };

    let under_box = if let Some(sym) = get_stretchy_overunder_sym(under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node(under, &under_ctx)
    };

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
    let base_width = base_box.width;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_box = if is_stretchy_accent(accent) && base_width > 0.0 {
        let accent_h = 0.12 * ctx.size(); // accent decoration height in em
        LayoutBox {
            width: base_width,
            height: accent_h,
            depth: 0.0,
            kind: BoxKind::SvgPath(stretchy_accent_path(accent, base_width, accent_h)),
        }
    } else {
        LayoutBox::glyph(accent, ctx.style)
    };

    let accent_dx = (base_width - accent_box.width) / 2.0;
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
    let box_height = half + axis;

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    LayoutBox {
        width: total_width,
        height: box_height,
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

fn layout_styled(style: TextStyle, children: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    let styled_children: Vec<MathNode> = children
        .iter()
        .map(|node| match node {
            MathNode::Identifier(s) => {
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                MathNode::Identifier(mapped)
            },
            MathNode::Number(s) => {
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                MathNode::Number(mapped)
            },
            other => other.clone(),
        })
        .collect();
    layout_row(&styled_children, ctx)
}

// ==========================================================================
// Compact arena-based layout functions (_c suffix)
// ==========================================================================

/// Entry point: layout an entire compact math expression.
pub fn layout_expr_c(expr: &CompactExpr, style: MathStyle, options: &RenderOptions) -> LayoutBox {
    let ctx = LayoutCtx {
        style,
        params: TexParams::default(),
        options,
        depth: 0,
    };
    layout_row_c(&expr.arena, expr.roots, &ctx)
}

fn layout_row_c(arena: &AstArena, span: NodeSpan, ctx: &LayoutCtx) -> LayoutBox {
    let ids = arena.children(span);
    if ids.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        // Inter-atom spacing
        let cur_atom = spacing::classify_node_c(arena, id);
        if let Some(prev) = prev_atom {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.style);
            if space > 0.0 {
                let kern_width = space * ctx.size();
                cursor_x += kern_width;
            }
        }

        let layout = layout_node_c(arena, id, ctx);
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);

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

fn layout_node_c(arena: &AstArena, id: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return LayoutBox::kern(0.0);
    }
    let ctx = &ctx.deeper();
    match *arena.node(id) {
        CompactNode::Number(sid) => layout_text_run(arena.str_val(sid), ctx, false),
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            if s.chars().count() == 1 {
                LayoutBox::glyph(s.chars().next().expect("non-empty checked"), ctx.style)
            } else {
                layout_text_run(s, ctx, false)
            }
        },
        CompactNode::Operator { name, .. } => layout_text_run(arena.str_val(name), ctx, false),
        CompactNode::Symbol(kind, sid) => {
            let s = arena.str_val(sid);
            if let Some(ch) = s.chars().next() {
                if kind == SymbolKind::LargeOp && ctx.style.is_display() {
                    let size = ctx.size() * BIG_OP_DISPLAY_SCALE;
                    let m = metrics::glyph_metrics(ch);
                    LayoutBox::glyph_with_metrics(ch, m, size)
                } else {
                    LayoutBox::glyph(ch, ctx.style)
                }
            } else {
                LayoutBox::kern(0.0)
            }
        },
        CompactNode::Text(_, sid) => layout_text_run(arena.str_val(sid), ctx, false),
        CompactNode::Space(r) => LayoutBox::kern(r.as_f64() * ctx.size()),
        CompactNode::Row(span) => layout_row_c(arena, span, ctx),
        CompactNode::Fenced {
            open,
            close,
            content,
        } => layout_fenced_c(arena, open, close, content, ctx),
        CompactNode::Subscript { base, sub } => layout_subscript_c(arena, base, sub, ctx),
        CompactNode::Superscript { base, sup } => layout_superscript_c(arena, base, sup, ctx),
        CompactNode::SubSup { base, sub, sup } => layout_subsup_c(arena, base, sub, sup, ctx),
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => layout_prescripts_c(arena, base, pre_sub, pre_sup, ctx),
        CompactNode::Overscript { base, over, .. } => layout_overscript_c(arena, base, over, ctx),
        CompactNode::Underscript { base, under, .. } => {
            layout_underscript_c(arena, base, under, ctx)
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => layout_underover_c(arena, base, under, over, ctx),
        CompactNode::Fraction { kind, num, den } => layout_fraction_c(arena, kind, num, den, ctx),
        CompactNode::Sqrt(inner) => layout_sqrt_c(arena, inner, None, ctx),
        CompactNode::Root { index, radicand } => layout_sqrt_c(arena, radicand, Some(index), ctx),
        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => layout_nary_c(arena, op, sub, sup, body, limits, ctx),
        CompactNode::Accent {
            base,
            accent,
            position,
        } => layout_accent_c(arena, base, accent, position, ctx),
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
        } => layout_array_c(arena, col_aligns, cells, num_cols, ctx),
        CompactNode::Phantom(inner) => layout_phantom_c(arena, inner, ctx),
        CompactNode::Boxed(inner) => layout_boxed_c(arena, inner, ctx),
        CompactNode::Cancel { stroke, content } => layout_cancel_c(arena, stroke, content, ctx),
        CompactNode::Scaled { factor, content } => layout_scaled_c(arena, factor, content, ctx),
        CompactNode::Styled(style, span) => layout_styled_c(arena, style, span, ctx),
        CompactNode::Color { color, content } => layout_color_c(arena, color, content, ctx),
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => LayoutBox::kern(0.0),
    }
}

fn layout_superscript_c(arena: &AstArena, base: NodeId, sup: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, None, Some(sup), ctx);
    }
    let base_box = layout_node_c(arena, base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);

    let p = &ctx.params;
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

fn layout_subscript_c(arena: &AstArena, base: NodeId, sub: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, Some(sub), None, ctx);
    }
    let base_box = layout_node_c(arena, base, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

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

fn layout_subsup_c(
    arena: &AstArena,
    base: NodeId,
    sub: NodeId,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Large operator in display mode: place limits above/below
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, Some(sub), Some(sup), ctx);
    }
    let base_box = layout_node_c(arena, base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;

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

    let mut shift_down = p.sub2.max(base_box.depth + p.sub_drop * ctx.size());

    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

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

/// Layout a big operator (LargeOp symbol) with limits above/below (compact path).
fn layout_big_op_limits_c(
    arena: &AstArena,
    base: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char_c(arena, base).unwrap_or('\u{2211}');
    let op_size = size * BIG_OP_DISPLAY_SCALE;
    let m = metrics::glyph_metrics(op_ch);
    let op_box = LayoutBox::glyph_with_metrics(op_ch, m, op_size);

    let mut children = Vec::new();
    let mut total_h = op_box.height;
    let mut total_d = op_box.depth;
    let mut max_w = op_box.width;

    let sup_box = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, s, &sup_ctx)
    });
    let sub_box = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, s, &sub_ctx)
    });

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + p.big_op_spacing[0] * size;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + p.big_op_spacing[3] * size;
    }

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

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_fraction_c(
    arena: &AstArena,
    kind: FractionKind,
    num: NodeId,
    den: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
    let num_box = layout_node_c(arena, num, &num_ctx);
    let den_box = layout_node_c(arena, den, &den_ctx);

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
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

        let num_clearance = (num_up - num_box.depth) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_box.height - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
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

    let num_dx = (frac_width - num_box.width) / 2.0;
    let den_dx = (frac_width - den_box.width) / 2.0;

    let mut children = vec![
        PositionedBox::new(num_dx, num_up, num_box),
        PositionedBox::new(den_dx, -den_down, den_box),
    ];

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

fn layout_sqrt_c(
    arena: &AstArena,
    radicand: NodeId,
    index: Option<NodeId>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let inner = layout_node_c(arena, radicand, ctx);

    let rule_thickness = p.rule_thickness * size;
    let extra_clearance = if ctx.style.is_display() {
        rule_thickness
    } else {
        rule_thickness / 2.0
    };

    let inner_total = inner.height + inner.depth + extra_clearance + rule_thickness;

    let radical_width = 0.833 * size;
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

    let vinculum = LayoutBox::rule(inner.width, rule_thickness);

    let shift_up = inner.height + extra_clearance + rule_thickness;

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

    if let Some(idx_id) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box = layout_node_c(arena, idx_id, &idx_ctx);
        let idx_shift = total_height * 0.6;
        let idx_width = idx_box.width;
        children.push(PositionedBox::new(0.0, idx_shift, idx_box));
        let idx_overflow = idx_width.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
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

fn layout_fenced_c(
    arena: &AstArena,
    open: StrId,
    close: StrId,
    content: NodeSpan,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Layout content, skipping Middle separator nodes
    let mut inner_children = Vec::new();
    for &child_id in arena.children(content) {
        match *arena.node(child_id) {
            CompactNode::Middle(_) => {},
            _ => {
                inner_children.push(layout_node_c(arena, child_id, ctx));
            },
        }
    }

    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for child in &inner_children {
        max_h = max_h.max(child.height);
        max_d = max_d.max(child.depth);
    }

    let target_h = max_h + max_d;

    let mut children = Vec::new();
    let mut cursor_x = 0.0;

    // Open delimiter
    let open_str = arena.str_val(open);
    if !open_str.is_empty()
        && open_str != "."
        && let Some(ch) = open_str.chars().next()
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

    for child in inner_children {
        let w = child.width;
        children.push(PositionedBox::new(cursor_x, 0.0, child));
        cursor_x += w;
    }

    // Close delimiter
    let close_str = arena.str_val(close);
    if !close_str.is_empty()
        && close_str != "."
        && let Some(ch) = close_str.chars().next()
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

fn layout_nary_c(
    arena: &AstArena,
    op: char,
    sub: NodeId,
    sup: NodeId,
    body: NodeId,
    limits: LimitStyle,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    let op_size = if ctx.style.is_display() {
        size * 1.2
    } else {
        size
    };
    let m = metrics::glyph_metrics(op);
    let op_box = LayoutBox::glyph_with_metrics(op, m, op_size);

    let body_box = layout_node_c(arena, body, ctx);
    let body_w = body_box.width;
    let body_h = body_box.height;
    let body_d = body_box.depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        LimitStyle::Default => ctx.style.is_display(),
    };

    if use_limits {
        let mut children = Vec::new();
        let mut total_h = op_box.height;
        let mut total_d = op_box.depth;
        let mut max_w = op_box.width;

        let sup_box = if sup.is_some() {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            Some(layout_node_c(arena, sup, &sup_ctx))
        } else {
            None
        };
        let sub_box = if sub.is_some() {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            Some(layout_node_c(arena, sub, &sub_ctx))
        } else {
            None
        };

        if let Some(ref sb) = sup_box {
            max_w = max_w.max(sb.width);
            total_h += sb.height + sb.depth + p.big_op_spacing[0] * size;
        }
        if let Some(ref sb) = sub_box {
            max_w = max_w.max(sb.width);
            total_d += sb.height + sb.depth + p.big_op_spacing[3] * size;
        }

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
        // As sub/superscripts — use the op glyph directly
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

        // Use the already-scaled op_box for the operator glyph
        let op_glyph_box = op_box;

        if sub.is_some() && sup.is_some() {
            let result = layout_subsup_from_box(op_glyph_box, arena, sub, sup, ctx);
            return build_hbox(result, body_box);
        } else if sub.is_some() {
            return build_hbox(
                layout_subscript_from_box(op_glyph_box, arena, sub, ctx),
                body_box,
            );
        } else if sup.is_some() {
            return build_hbox(
                layout_superscript_from_box(op_glyph_box, arena, sup, ctx),
                body_box,
            );
        }

        // Just op + body
        let op_w = op_glyph_box.width;
        let op_h = op_glyph_box.height;
        let op_d = op_glyph_box.depth;
        let children = vec![
            PositionedBox::at_origin(op_glyph_box),
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

/// Helper: superscript layout where the base is an already-laid-out LayoutBox.
fn layout_superscript_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);

    let p = &ctx.params;
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

/// Helper: subscript layout where the base is an already-laid-out LayoutBox.
fn layout_subscript_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sub: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

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

/// Helper: subsup layout where the base is an already-laid-out LayoutBox.
fn layout_subsup_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sub: NodeId,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;

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

    let mut shift_down = p.sub2.max(base_box.depth + p.sub_drop * ctx.size());

    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

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

/// Check if a CompactNode (referenced by NodeId) is a stretchy overunder decoration symbol.
/// Returns the symbol string if so.
fn get_stretchy_overunder_sym_c(arena: &AstArena, id: NodeId) -> Option<&str> {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        let s = arena.str_val(sid);
        if is_stretchy_overunder(s) {
            return Some(s);
        }
    }
    None
}

fn layout_overscript_c(arena: &AstArena, base: NodeId, over: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, over, &over_ctx)
    };

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_box.width.max(over_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let over_dy = base_box.height + gap + over_box.depth;

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

fn layout_underscript_c(
    arena: &AstArena,
    base: NodeId,
    under: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let under_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, under, &under_ctx)
    };

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(under_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;
    let under_dy = base_box.depth + gap + under_box.height;

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

fn layout_underover_c(
    arena: &AstArena,
    base: NodeId,
    under: NodeId,
    over: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, over, &over_ctx)
    };

    let under_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, under, &under_ctx)
    };

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(over_box.width).max(under_box.width);

    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;

    let over_dy = base_box.height + gap_above + over_box.depth;
    let under_dy = base_box.depth + gap_below + under_box.height;

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

fn layout_accent_c(
    arena: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node_c(arena, base, ctx);
    let base_width = base_box.width;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_box = if is_stretchy_accent(accent) && base_width > 0.0 {
        let accent_h = 0.12 * ctx.size();
        LayoutBox {
            width: base_width,
            height: accent_h,
            depth: 0.0,
            kind: BoxKind::SvgPath(stretchy_accent_path(accent, base_width, accent_h)),
        }
    } else {
        LayoutBox::glyph(accent, ctx.style)
    };

    let accent_dx = (base_width - accent_box.width) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_box.height + 0.02 * ctx.size();
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

#[allow(clippy::needless_range_loop)]
fn layout_array_c(
    arena: &AstArena,
    col_aligns: NodeSpan,
    cells: NodeSpan,
    num_cols: u16,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let row_gap = 0.6 * size;
    let col_gap = 1.0 * size;

    // Extract column alignments
    let columns: Vec<ColumnAlign> = arena
        .children(col_aligns)
        .iter()
        .map(|&id| match *arena.node(id) {
            CompactNode::ColAlign(a) => a,
            _ => ColumnAlign::Center,
        })
        .collect();

    // Layout all cells (flat storage, row-major, stride = num_cols)
    let cell_ids = arena.children(cells);
    let nc = num_cols as usize;
    let n_rows = if nc > 0 { cell_ids.len() / nc } else { 0 };

    let mut cell_layouts: Vec<LayoutBox> = Vec::with_capacity(cell_ids.len());
    for &cell_id in cell_ids {
        // Each cell is a Row node
        match *arena.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layouts.push(layout_row_c(arena, span, ctx));
            },
            _ => {
                cell_layouts.push(layout_node_c(arena, cell_id, ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![0.0_f64; n_rows];
    let mut row_depths = vec![0.0_f64; n_rows];

    for i in 0..n_rows {
        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layouts.len() {
                col_widths[j] = col_widths[j].max(cell_layouts[idx].width);
                row_heights[i] = row_heights[i].max(cell_layouts[idx].height);
                row_depths[i] = row_depths[i].max(cell_layouts[idx].depth);
            }
        }
    }

    // Position cells
    let mut children = Vec::new();
    let mut cursor_y = 0.0;

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layouts.len() {
                let col_w = col_widths[j];
                // Steal the cell layout to avoid clone
                let cell = std::mem::replace(&mut cell_layouts[idx], LayoutBox::kern(0.0));
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
                cursor_x += col_w;
            } else {
                cursor_x += col_widths.get(j).copied().unwrap_or(0.0);
            }
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
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    LayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: BoxKind::VBox { children },
    }
}

fn layout_prescripts_c(
    arena: &AstArena,
    base: NodeId,
    pre_sub: NodeId,
    pre_sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node_c(arena, base, ctx);
    let p = &ctx.params;

    let mut children = Vec::new();
    let mut pre_width: f64 = 0.0;
    let mut total_h = base_box.height;
    let mut total_d = base_box.depth;

    if pre_sup.is_some() {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        let sup_box = layout_node_c(arena, pre_sup, &sup_ctx);
        let shift_up = p.sup2.max(base_box.height - p.sup_drop * ctx.size());
        pre_width = pre_width.max(sup_box.width);
        total_h = total_h.max(shift_up + sup_box.height);
        children.push(PositionedBox::new(0.0, shift_up, sup_box));
    }
    if pre_sub.is_some() {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        let sub_box = layout_node_c(arena, pre_sub, &sub_ctx);
        let shift_down = p.sub1.max(base_box.depth + p.sub_drop * ctx.size());
        pre_width = pre_width.max(sub_box.width);
        total_d = total_d.max(shift_down + sub_box.depth);
        children.push(PositionedBox::new(0.0, -shift_down, sub_box));
    }

    let base_width = base_box.width;
    children.push(PositionedBox::new(pre_width, 0.0, base_box));

    LayoutBox {
        width: pre_width + base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_phantom_c(arena: &AstArena, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
    LayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: BoxKind::Phantom,
    }
}

fn layout_boxed_c(arena: &AstArena, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner = layout_node_c(arena, content, ctx);

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

fn layout_color_c(arena: &AstArena, color: StrId, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let color_str = arena.str_val(color).to_owned();
    let inner = layout_node_c(arena, content, ctx);
    let child_box = PositionedBox::new(0.0, 0.0, inner);
    LayoutBox {
        width: child_box.layout.width,
        height: child_box.layout.height,
        depth: child_box.layout.depth,
        kind: BoxKind::Color {
            color: color_str,
            child: Box::new(child_box),
        },
    }
}

fn layout_cancel_c(
    arena: &AstArena,
    stroke: StrokeKind,
    content: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
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

fn layout_scaled_c(
    arena: &AstArena,
    factor: Rational,
    content: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: inner.kind,
    }
}

fn layout_styled_c(arena: &AstArena, style: TextStyle, span: NodeSpan, ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    let ids = arena.children(span);
    if ids.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        let layout = match *arena.node(id) {
            CompactNode::Identifier(sid) => {
                let s = arena.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                if mapped.chars().count() == 1 {
                    LayoutBox::glyph(mapped.chars().next().expect("non-empty checked"), ctx.style)
                } else {
                    layout_text_run(&mapped, ctx, false)
                }
            },
            CompactNode::Number(sid) => {
                let s = arena.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                layout_text_run(&mapped, ctx, false)
            },
            _ => layout_node_c(arena, id, ctx),
        };
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);
        let layout_width = layout.width;
        children.push(PositionedBox::new(cursor_x, 0.0, layout));
        cursor_x += layout_width;
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children },
    }
}

// ==========================================================================
// Arena-based layout functions
// ==========================================================================

use crate::render::arena::{
    ArenaBoxKind, ArenaLayoutBox, ArenaPositionedBox, LayoutArena, LayoutBoxId,
};

/// Builder for arena-based layout.
pub struct LayoutBuilder<'a> {
    style: MathStyle,
    options: &'a RenderOptions,
}

impl<'a> LayoutBuilder<'a> {
    pub fn new(style: MathStyle, options: &'a RenderOptions) -> Self {
        Self { style, options }
    }

    /// Layout a compact expression into the arena. Returns the root box ID.
    pub fn layout(
        &self,
        arena: &mut LayoutArena,
        expr: &CompactExpr,
    ) -> LayoutBoxId {
        let ctx = ArenaLayoutCtx {
            style: self.style,
            params: TexParams::default(),
            options: self.options,
            depth: 0,
        };
        layout_row_arena(&expr.arena, expr.roots, arena, &ctx)
    }
}

struct ArenaLayoutCtx<'a> {
    style: MathStyle,
    params: TexParams,
    options: &'a RenderOptions,
    depth: u32,
}

impl<'a> ArenaLayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier()
    }

    fn with_style(&self, style: MathStyle) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style,
            params: self.params,
            options: self.options,
            depth: self.depth,
        }
    }

    fn deeper(&self) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style: self.style,
            params: self.params,
            options: self.options,
            depth: self.depth + 1,
        }
    }
}

// ── Leaf helpers ──────────────────────────────────────────────────────

fn alloc_glyph(arena: &mut LayoutArena, ch: char, style: MathStyle) -> LayoutBoxId {
    let size = style.size_multiplier();
    let m = metrics::glyph_metrics(ch);
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph { ch, size },
    })
}

fn alloc_glyph_with_metrics(
    arena: &mut LayoutArena,
    ch: char,
    m: metrics::GlyphMetrics,
    size: f64,
) -> LayoutBoxId {
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph { ch, size },
    })
}

fn layout_text_run_arena(
    text: &str,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        let m = metrics::glyph_metrics(ch);
        let glyph_id = alloc_glyph_with_metrics(arena, ch, m, size);
        let b = arena.get(glyph_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        arena.push_child(ArenaPositionedBox {
            dx: cursor_x,
            dy: 0.0,
            id: glyph_id,
        });
        cursor_x += m.width * size;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_row_arena(
    ast: &AstArena,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let ids = ast.children(span);
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    // Phase 1: layout all children and collect (box_id, dx) pairs.
    // We must NOT call begin_children/push_child while nested layout calls
    // are still in progress, because they also push into the same flat array.
    // SmallVec avoids heap allocation for rows with ≤16 children (the common case).
    let mut laid_out: SmallVec<[(LayoutBoxId, f64); 16]> = SmallVec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        let cur_atom = spacing::classify_node_c(ast, id);
        if let Some(prev) = prev_atom {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.style);
            if space > 0.0 {
                cursor_x += space * ctx.size();
            }
        }

        let box_id = layout_node_arena(ast, id, arena, ctx);
        let b = arena.get(box_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        let w = b.width;

        laid_out.push((box_id, cursor_x));
        cursor_x += w;
        prev_atom = Some(cur_atom);
    }

    // Phase 2: now push all children into the arena's children array.
    let children_start = arena.begin_children();
    for &(box_id, dx) in &laid_out {
        arena.push_child(ArenaPositionedBox { dx, dy: 0.0, id: box_id });
    }
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_node_arena(
    ast: &AstArena,
    id: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }
    let ctx = &ctx.deeper();
    match *ast.node(id) {
        CompactNode::Number(sid) => layout_text_run_arena(ast.str_val(sid), arena, ctx),
        CompactNode::Identifier(sid) => {
            let s = ast.str_val(sid);
            if s.chars().count() == 1 {
                alloc_glyph(arena, s.chars().next().expect("non-empty checked"), ctx.style)
            } else {
                layout_text_run_arena(s, arena, ctx)
            }
        },
        CompactNode::Operator { name, .. } => layout_text_run_arena(ast.str_val(name), arena, ctx),
        CompactNode::Symbol(kind, sid) => {
            let s = ast.str_val(sid);
            if let Some(ch) = s.chars().next() {
                if kind == SymbolKind::LargeOp && ctx.style.is_display() {
                    let size = ctx.size() * BIG_OP_DISPLAY_SCALE;
                    let m = metrics::glyph_metrics(ch);
                    alloc_glyph_with_metrics(arena, ch, m, size)
                } else {
                    alloc_glyph(arena, ch, ctx.style)
                }
            } else {
                arena.alloc(ArenaLayoutBox::kern(0.0))
            }
        },
        CompactNode::Text(_, sid) => layout_text_run_arena(ast.str_val(sid), arena, ctx),
        CompactNode::Space(r) => arena.alloc(ArenaLayoutBox::kern(r.as_f64() * ctx.size())),
        CompactNode::Row(span) => layout_row_arena(ast, span, arena, ctx),
        CompactNode::Fenced { open, close, content } => layout_fenced_arena(ast, open, close, content, arena, ctx),
        CompactNode::Subscript { base, sub } => layout_subscript_arena(ast, base, sub, arena, ctx),
        CompactNode::Superscript { base, sup } => layout_superscript_arena(ast, base, sup, arena, ctx),
        CompactNode::SubSup { base, sub, sup } => layout_subsup_arena(ast, base, sub, sup, arena, ctx),
        CompactNode::PreScripts { base, pre_sub, pre_sup } => layout_prescripts_arena(ast, base, pre_sub, pre_sup, arena, ctx),
        CompactNode::Overscript { base, over, .. } => layout_overscript_arena(ast, base, over, arena, ctx),
        CompactNode::Underscript { base, under, .. } => layout_underscript_arena(ast, base, under, arena, ctx),
        CompactNode::UnderOver { base, under, over, .. } => layout_underover_arena(ast, base, under, over, arena, ctx),
        CompactNode::Fraction { kind, num, den } => layout_fraction_arena(ast, kind, num, den, arena, ctx),
        CompactNode::Sqrt(inner) => layout_sqrt_arena(ast, inner, None, arena, ctx),
        CompactNode::Root { index, radicand } => layout_sqrt_arena(ast, radicand, Some(index), arena, ctx),
        CompactNode::Nary { op, sub, sup, body, limits } => layout_nary_arena(ast, op, sub, sup, body, limits, arena, ctx),
        CompactNode::Accent { base, accent, position } => layout_accent_arena(ast, base, accent, position, arena, ctx),
        CompactNode::Array { col_aligns, cells, num_cols } => layout_array_arena(ast, col_aligns, cells, num_cols, arena, ctx),
        CompactNode::Phantom(inner) => layout_phantom_arena(ast, inner, arena, ctx),
        CompactNode::Boxed(inner) => layout_boxed_arena(ast, inner, arena, ctx),
        CompactNode::Cancel { stroke, content } => layout_cancel_arena(ast, stroke, content, arena, ctx),
        CompactNode::Scaled { factor, content } => layout_scaled_arena(ast, factor, content, arena, ctx),
        CompactNode::Styled(style, span) => layout_styled_arena(ast, style, span, arena, ctx),
        CompactNode::Color { color, content } => layout_color_arena(ast, color, content, arena, ctx),
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
    }
}

// ── Script and fraction arena layout ─────────────────────────────────

fn layout_superscript_arena(
    ast: &AstArena,
    base: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, None, Some(sup), arena, ctx);
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

    let p = &ctx.params;
    let shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    let shift_up = shift_up
        .max(base_h - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_d);

    let total_width = base_w + sup_w;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: shift_up, id: sup_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_subscript_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, Some(sub), None, arena, ctx);
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

    let p = &ctx.params;
    let shift_down = p
        .sub1
        .max(base_d + p.sub_drop * ctx.size())
        .max(sub_h - (4.0 * p.x_height / 5.0));

    let total_width = base_w + sub_w;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_subsup_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place limits above/below
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, Some(sub), Some(sup), arena, ctx);
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

    let p = &ctx.params;

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    shift_up = shift_up
        .max(base_h - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_d);

    let mut shift_down = p.sub2.max(base_d + p.sub_drop * ctx.size());

    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

    let sup_bottom = shift_up - sup_d;
    if sup_bottom < (4.0 * p.x_height / 5.0) {
        let delta = (4.0 * p.x_height / 5.0) - sup_bottom;
        shift_up += delta;
    }

    let script_width = sup_w.max(sub_w);
    let total_width = base_w + script_width;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: shift_up, id: sup_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Layout a big operator (LargeOp symbol) with limits above/below (arena path).
fn layout_big_op_limits_arena(
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
    let op_size = size * BIG_OP_DISPLAY_SCALE;
    let m = metrics::glyph_metrics(op_ch);
    let op_id = alloc_glyph_with_metrics(arena, op_ch, m, op_size);

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

    if sup_id_opt.is_some() {
        max_w = max_w.max(sup_w_opt);
        total_h += sup_h_opt + sup_d_opt + p.big_op_spacing[0] * size;
    }
    if sub_id_opt.is_some() {
        max_w = max_w.max(sub_w_opt);
        total_d += sub_h_opt + sub_d_opt + p.big_op_spacing[3] * size;
    }

    let op_dx = (max_w - op_w) / 2.0;

    let children_start = arena.begin_children();

    if let Some(sid) = sup_id_opt {
        let sup_dx = (max_w - sup_w_opt) / 2.0;
        let sup_dy = op_h + p.big_op_spacing[0] * size + sup_d_opt;
        arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: sup_dy, id: sid });
    }
    arena.push_child(ArenaPositionedBox { dx: op_dx, dy: 0.0, id: op_id });
    if let Some(sid) = sub_id_opt {
        let sub_dx = (max_w - sub_w_opt) / 2.0;
        let sub_dy = -(op_d + p.big_op_spacing[3] * size + sub_h_opt);
        arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: sub_dy, id: sid });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_fraction_arena(
    ast: &AstArena,
    kind: FractionKind,
    num: NodeId,
    den: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
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
    let total_height = num_up + num_h;
    let total_depth = den_down + den_d;

    let num_dx = (frac_width - num_w) / 2.0;
    let den_dx = (frac_width - den_w) / 2.0;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: num_dx, dy: num_up, id: num_id });
    arena.push_child(ArenaPositionedBox { dx: den_dx, dy: -den_down, id: den_id });

    if bar_thickness > 0.0 {
        let bar_id = arena.alloc(ArenaLayoutBox::rule(frac_width, bar_thickness));
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: axis, id: bar_id });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: frac_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

// ── Remaining arena layout functions ─────────────────────────────────

fn layout_sqrt_arena(
    ast: &AstArena,
    radicand: NodeId,
    index: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    let inner_id = layout_node_arena(ast, radicand, arena, ctx);

    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let rule_thickness = p.rule_thickness * size;
    let extra_clearance = if ctx.style.is_display() {
        rule_thickness
    } else {
        rule_thickness / 2.0
    };

    let inner_total = inner_h + inner_d + extra_clearance + rule_thickness;

    let radical_width = 0.833 * size;
    let radical_id = arena.alloc(ArenaLayoutBox {
        width: radical_width,
        height: inner_total,
        depth: 0.0,
        kind: ArenaBoxKind::SvgPath(format!(
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
    });

    let vinculum_id = arena.alloc(ArenaLayoutBox::rule(inner_w, rule_thickness));

    let shift_up = inner_h + extra_clearance + rule_thickness;

    // Build children: radical, vinculum, inner
    // We may need to adjust dx values if there's an index, so we track the span start
    // and positions separately.
    let mut radical_dx = 0.0_f64;
    let mut vinculum_dx = radical_width;
    let mut inner_dx = radical_width;
    let mut total_width = radical_width + inner_w;
    let total_height = shift_up + rule_thickness;
    let total_depth = inner_d;

    // Optional root index
    if let Some(idx_id_node) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box_id = layout_node_arena(ast, idx_id_node, arena, &idx_ctx);
        let idx_w = arena.get(idx_box_id).width;
        let idx_shift = total_height * 0.6;
        let idx_overflow = idx_w.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
            radical_dx += extra;
            vinculum_dx += extra;
            inner_dx += extra;
            total_width += extra;
        }

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: radical_dx, dy: 0.0, id: radical_id });
        arena.push_child(ArenaPositionedBox { dx: vinculum_dx, dy: shift_up, id: vinculum_id });
        arena.push_child(ArenaPositionedBox { dx: inner_dx, dy: 0.0, id: inner_id });
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: idx_shift, id: idx_box_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: radical_dx, dy: 0.0, id: radical_id });
        arena.push_child(ArenaPositionedBox { dx: vinculum_dx, dy: shift_up, id: vinculum_id });
        arena.push_child(ArenaPositionedBox { dx: inner_dx, dy: 0.0, id: inner_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    }
}

fn layout_fenced_arena(
    ast: &AstArena,
    open: StrId,
    close: StrId,
    content: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Layout content, skipping Middle separator nodes
    // We need to collect IDs first, then measure
    let mut inner_ids: Vec<LayoutBoxId> = Vec::new();
    for &child_id in ast.children(content) {
        match *ast.node(child_id) {
            CompactNode::Middle(_) => {},
            _ => {
                let laid = layout_node_arena(ast, child_id, arena, ctx);
                inner_ids.push(laid);
            },
        }
    }

    // Compute max height/depth of content
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for &id in &inner_ids {
        let b = arena.get(id);
        max_h = max_h.max(b.height);
        max_d = max_d.max(b.depth);
    }

    let target_h = max_h + max_d;

    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;

    // Open delimiter
    let open_str = ast.str_val(open);
    if !open_str.is_empty() && open_str != "."
        && let Some(ch) = open_str.chars().next()
    {
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: 0.389 * ctx.size(),
            height: max_h,
            depth: max_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
        let delim_w = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id: delim_id });
        cursor_x += delim_w;
    }

    // Content
    for &id in &inner_ids {
        let w = arena.get(id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id });
        cursor_x += w;
    }

    // Close delimiter
    let close_str = ast.str_val(close);
    if !close_str.is_empty() && close_str != "."
        && let Some(ch) = close_str.chars().next()
    {
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: 0.389 * ctx.size(),
            height: max_h,
            depth: max_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
        let delim_w = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id: delim_id });
        cursor_x += delim_w;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_h,
        depth: max_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[allow(clippy::too_many_arguments)]
fn layout_nary_arena(
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

    // Large op in display mode
    let op_size = if ctx.style.is_display() {
        size * 1.2
    } else {
        size
    };
    let m = metrics::glyph_metrics(op);
    let op_id = alloc_glyph_with_metrics(arena, op, m, op_size);

    let body_id = layout_node_arena(ast, body, arena, ctx);
    let body_w = arena.get(body_id).width;
    let body_h = arena.get(body_id).height;
    let body_d = arena.get(body_id).depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        LimitStyle::Default => ctx.style.is_display(),
    };

    if use_limits {
        // Limits above/below
        let op_w = arena.get(op_id).width;
        let op_h = arena.get(op_id).height;
        let op_d = arena.get(op_id).depth;

        let mut total_h = op_h;
        let mut total_d = op_d;
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

        if sup_id_opt.is_some() {
            max_w = max_w.max(sup_w_opt);
            total_h += sup_h_opt + sup_d_opt + p.big_op_spacing[0] * size;
        }
        if sub_id_opt.is_some() {
            max_w = max_w.max(sub_w_opt);
            total_d += sub_h_opt + sub_d_opt + p.big_op_spacing[3] * size;
        }

        let op_dx = (max_w - op_w) / 2.0;

        let children_start = arena.begin_children();

        if let Some(sid) = sup_id_opt {
            let sup_dx = (max_w - sup_w_opt) / 2.0;
            let sup_dy = op_h + p.big_op_spacing[0] * size + sup_d_opt;
            arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: sup_dy, id: sid });
        }
        arena.push_child(ArenaPositionedBox { dx: op_dx, dy: 0.0, id: op_id });
        if let Some(sid) = sub_id_opt {
            let sub_dx = (max_w - sub_w_opt) / 2.0;
            let sub_dy = -(op_d + p.big_op_spacing[3] * size + sub_h_opt);
            arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: sub_dy, id: sid });
        }

        // Body follows
        let nary_width = max_w;
        let thin_space = 3.0 / 18.0 * size;
        arena.push_child(ArenaPositionedBox { dx: nary_width + thin_space, dy: 0.0, id: body_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h,
            depth: total_d,
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        // As sub/superscripts
        let thin_space = 3.0 / 18.0 * size;

        // Use the already-scaled op_id for the operator glyph
        let op_glyph_id = op_id;

        if sub.is_some() && sup.is_some() {
            let result_id = layout_subsup_from_box_arena(op_glyph_id, ast, sub, sup, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        } else if sub.is_some() {
            let result_id = layout_subscript_from_box_arena(op_glyph_id, ast, sub, arena, ctx);
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
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: op_glyph_id });
        arena.push_child(ArenaPositionedBox { dx: op_w + thin_space, dy: 0.0, id: body_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: ArenaBoxKind::HBox { children },
        })
    }
}

/// Helper: build an HBox of [result, thin_space, body] for nary non-limits mode.
fn build_hbox_arena(
    result_id: LayoutBoxId,
    body_id: LayoutBoxId,
    thin_space: f64,
    arena: &mut LayoutArena,
) -> LayoutBoxId {
    let rw = arena.get(result_id).width;
    let rh = arena.get(result_id).height;
    let rd = arena.get(result_id).depth;
    let bw = arena.get(body_id).width;
    let bh = arena.get(body_id).height;
    let bd = arena.get(body_id).depth;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: result_id });
    arena.push_child(ArenaPositionedBox { dx: rw + thin_space, dy: 0.0, id: body_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: rw + thin_space + bw,
        height: rh.max(bh),
        depth: rd.max(bd),
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: superscript layout where the base is an already-laid-out arena box.
fn layout_superscript_from_box_arena(
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

    let p = &ctx.params;
    let shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    let shift_up = shift_up
        .max(base_h - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_d);

    let total_width = base_w + sup_w;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: shift_up, id: sup_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subscript layout where the base is an already-laid-out arena box.
fn layout_subscript_from_box_arena(
    base_id: LayoutBoxId,
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

    let p = &ctx.params;
    let shift_down = p
        .sub1
        .max(base_d + p.sub_drop * ctx.size())
        .max(sub_h - (4.0 * p.x_height / 5.0));

    let total_width = base_w + sub_w;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subsup layout where the base is an already-laid-out arena box.
fn layout_subsup_from_box_arena(
    base_id: LayoutBoxId,
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

    let p = &ctx.params;

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    shift_up = shift_up
        .max(base_h - p.sup_drop * ctx.size())
        .max(p.x_height / 4.0 + sup_d);

    let mut shift_down = p.sub2.max(base_d + p.sub_drop * ctx.size());

    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    if gap < 4.0 * p.rule_thickness {
        let adjust = (4.0 * p.rule_thickness - gap) / 2.0;
        shift_up += adjust;
        shift_down += adjust;
    }

    let sup_bottom = shift_up - sup_d;
    if sup_bottom < (4.0 * p.x_height / 5.0) {
        let delta = (4.0 * p.x_height / 5.0) - sup_bottom;
        shift_up += delta;
    }

    let script_width = sup_w.max(sub_w);
    let total_width = base_w + script_width;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: shift_up, id: sup_id });
    arena.push_child(ArenaPositionedBox { dx: base_w, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_overscript_arena(
    ast: &AstArena,
    base: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    // Check if the over node is a stretchy decoration symbol
    let over_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.12 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, over, arena, &over_ctx)
    };

    let over_w = arena.get(over_id).width;
    let over_h = arena.get(over_id).height;
    let over_d = arena.get(over_id).depth;

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_w.max(over_w);
    let base_dx = (max_w - base_w) / 2.0;
    let over_dx = (max_w - over_w) / 2.0;
    let over_dy = base_h + gap + over_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: base_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_underscript_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let under_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.12 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, under, arena, &under_ctx)
    };

    let under_w = arena.get(under_id).width;
    let under_h = arena.get(under_id).height;
    let under_d = arena.get(under_id).depth;

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_w.max(under_w);
    let base_dx = (max_w - base_w) / 2.0;
    let under_dx = (max_w - under_w) / 2.0;
    let under_dy = base_d + gap + under_h;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: base_h,
        depth: under_dy + under_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_underover_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let over_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.12 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, over, arena, &over_ctx)
    };

    let under_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.12 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, under, arena, &under_ctx)
    };

    let over_w = arena.get(over_id).width;
    let over_h = arena.get(over_id).height;
    let over_d = arena.get(over_id).depth;
    let under_w = arena.get(under_id).width;
    let under_h = arena.get(under_id).height;
    let under_d = arena.get(under_id).depth;

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
    let max_w = base_w.max(over_w).max(under_w);

    let base_dx = (max_w - base_w) / 2.0;
    let over_dx = (max_w - over_w) / 2.0;
    let under_dx = (max_w - under_w) / 2.0;

    let over_dy = base_h + gap_above + over_d;
    let under_dy = base_d + gap_below + under_h;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_accent_arena(
    ast: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_id = if is_stretchy_accent(accent) && base_w > 0.0 {
        let accent_h = 0.12 * ctx.size();
        arena.alloc(ArenaLayoutBox {
            width: base_w,
            height: accent_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_accent_path(accent, base_w, accent_h)),
        })
    } else {
        alloc_glyph(arena, accent, ctx.style)
    };

    let accent_w = arena.get(accent_id).width;
    let accent_h = arena.get(accent_id).height;

    let accent_dx = (base_w - accent_w) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_h + 0.02 * ctx.size();
            (dy, dy + accent_h, base_d)
        },
        AccentPosition::Below => {
            let dy = -(base_d + accent_h + 0.02 * ctx.size());
            (
                dy,
                base_h,
                base_d + accent_h + 0.02 * ctx.size(),
            )
        },
    };

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: accent_dx, dy: accent_dy, id: accent_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[allow(clippy::needless_range_loop)]
fn layout_array_arena(
    ast: &AstArena,
    col_aligns: NodeSpan,
    cells: NodeSpan,
    num_cols: u16,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let row_gap = 0.6 * size;
    let col_gap = 1.0 * size;

    // Extract column alignments
    let columns: Vec<ColumnAlign> = ast
        .children(col_aligns)
        .iter()
        .map(|&id| match *ast.node(id) {
            CompactNode::ColAlign(a) => a,
            _ => ColumnAlign::Center,
        })
        .collect();

    // Layout all cells (flat storage, row-major, stride = num_cols)
    let cell_ids_ast = ast.children(cells);
    let nc = num_cols as usize;
    let n_rows = if nc > 0 { cell_ids_ast.len() / nc } else { 0 };

    // Layout each cell and store the layout box IDs
    let mut cell_layout_ids: Vec<LayoutBoxId> = Vec::with_capacity(cell_ids_ast.len());
    for &cell_id in cell_ids_ast {
        match *ast.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layout_ids.push(layout_row_arena(ast, span, arena, ctx));
            },
            _ => {
                cell_layout_ids.push(layout_node_arena(ast, cell_id, arena, ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![0.0_f64; n_rows];
    let mut row_depths = vec![0.0_f64; n_rows];

    for i in 0..n_rows {
        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let b = arena.get(cell_layout_ids[idx]);
                col_widths[j] = col_widths[j].max(b.width);
                row_heights[i] = row_heights[i].max(b.height);
                row_depths[i] = row_depths[i].max(b.depth);
            }
        }
    }

    // Pre-compute total extent and box_height so we can convert top-down
    // cursor_y to VBox dy (positive = up from baseline) when pushing children.
    let total_extent: f64 = row_heights.iter().sum::<f64>()
        + row_depths.iter().sum::<f64>()
        + row_gap * (n_rows.saturating_sub(1) as f64);
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Position cells
    let children_start = arena.begin_children();
    let mut cursor_y = 0.0;

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let col_w = col_widths[j];
                let cell_w = arena.get(cell_layout_ids[idx]).width;
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell_w) / 2.0,
                    ColumnAlign::Right => col_w - cell_w,
                };
                arena.push_child(ArenaPositionedBox {
                    dx: cursor_x + dx,
                    dy: box_height - cursor_y,
                    id: cell_layout_ids[idx],
                });
                cursor_x += col_w;
            } else {
                cursor_x += col_widths.get(j).copied().unwrap_or(0.0);
            }
            if j < n_cols - 1 {
                cursor_x += col_gap;
            }
        }

        cursor_y += row_depths[i];
        if i < n_rows - 1 {
            cursor_y += row_gap;
        }
    }

    let children = arena.finish_children(children_start);
    let total_width: f64 =
        col_widths.iter().sum::<f64>() + col_gap * (n_cols.saturating_sub(1) as f64);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_prescripts_arena(
    ast: &AstArena,
    base: NodeId,
    pre_sub: NodeId,
    pre_sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let base_id = layout_node_arena(ast, base, arena, ctx);
    let p = &ctx.params;

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let mut pre_width: f64 = 0.0;
    let mut total_h = base_h;
    let mut total_d = base_d;

    // Layout pre-superscript and pre-subscript
    let pre_sup_id = if pre_sup.is_some() {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        let sid = layout_node_arena(ast, pre_sup, arena, &sup_ctx);
        let b = arena.get(sid);
        let shift_up = p.sup2.max(base_h - p.sup_drop * ctx.size());
        pre_width = pre_width.max(b.width);
        total_h = total_h.max(shift_up + b.height);
        Some((sid, shift_up))
    } else {
        None
    };

    let pre_sub_id = if pre_sub.is_some() {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        let sid = layout_node_arena(ast, pre_sub, arena, &sub_ctx);
        let b = arena.get(sid);
        let shift_down = p.sub1.max(base_d + p.sub_drop * ctx.size());
        pre_width = pre_width.max(b.width);
        total_d = total_d.max(shift_down + b.depth);
        Some((sid, shift_down))
    } else {
        None
    };

    let children_start = arena.begin_children();
    if let Some((sid, shift_up)) = pre_sup_id {
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: shift_up, id: sid });
    }
    if let Some((sid, shift_down)) = pre_sub_id {
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: -shift_down, id: sid });
    }
    arena.push_child(ArenaPositionedBox { dx: pre_width, dy: 0.0, id: base_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: pre_width + base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_phantom_arena(
    ast: &AstArena,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: inner_w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::Phantom,
    })
}

fn layout_boxed_arena(
    ast: &AstArena,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: inner_w + 2.0 * padding,
        height: inner_h + padding,
        depth: inner_d + padding,
        kind: ArenaBoxKind::Boxed {
            child: inner_id,
            padding,
        },
    })
}

fn layout_color_arena(
    ast: &AstArena,
    color: StrId,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let color_str = ast.str_val(color).to_owned();
    let child_id = layout_node_arena(ast, content, arena, ctx);
    let w = arena.get(child_id).width;
    let h = arena.get(child_id).height;
    let d = arena.get(child_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: w,
        height: h,
        depth: d,
        kind: ArenaBoxKind::Color {
            color: color_str,
            child: child_id,
        },
    })
}

fn layout_cancel_arena(
    ast: &AstArena,
    stroke: StrokeKind,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let w = inner_w;
    let h = inner_h + inner_d;

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

    let path_id = arena.alloc(ArenaLayoutBox {
        width: w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::SvgPath(path),
    });

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: inner_id });
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: path_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: inner_w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_scaled_arena(
    ast: &AstArena,
    factor: Rational,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;
    let inner_kind = arena.get(inner_id).kind.clone();
    let f = factor.as_f64();

    arena.alloc(ArenaLayoutBox {
        width: inner_w * f,
        height: inner_h * f,
        depth: inner_d * f,
        kind: inner_kind,
    })
}

fn layout_styled_arena(
    ast: &AstArena,
    style: TextStyle,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    use crate::unicode::fonts::styled_char;

    let ids = ast.children(span);
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        let layout_id = match *ast.node(id) {
            CompactNode::Identifier(sid) => {
                let s = ast.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                if mapped.chars().count() == 1 {
                    alloc_glyph(arena, mapped.chars().next().expect("non-empty checked"), ctx.style)
                } else {
                    layout_text_run_arena(&mapped, arena, ctx)
                }
            },
            CompactNode::Number(sid) => {
                let s = ast.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                layout_text_run_arena(&mapped, arena, ctx)
            },
            _ => layout_node_arena(ast, id, arena, ctx),
        };
        let b = arena.get(layout_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        let w = b.width;
        arena.push_child(ArenaPositionedBox {
            dx: cursor_x,
            dy: 0.0,
            id: layout_id,
        });
        cursor_x += w;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
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

    #[test]
    fn large_op_symbol_scales_in_display() {
        // Symbol(LargeOp, "∑") should be larger in display mode than text mode
        let expr = MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        assert!(
            display.height > text.height,
            "LargeOp symbol in display should be taller: display={}, text={}",
            display.height,
            text.height,
        );
        assert!(
            display.width > text.width,
            "LargeOp symbol in display should be wider: display={}, text={}",
            display.width,
            text.width,
        );
    }

    #[test]
    fn subsup_with_large_op_uses_limits_in_display() {
        // SubSup with LargeOp base in display mode should place scripts above/below
        let expr_display = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr_display, MathStyle::Display, &opts);
        // In limits mode, the total height should be larger because sup is above
        // and sub is below the operator, adding to overall vertical extent
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.height > just_op.height,
            "SubSup with limits should be taller than just the op: display={}, op={}",
            display.height,
            just_op.height,
        );
        assert!(
            display.depth > just_op.depth,
            "SubSup with limits should have more depth than just the op: display={}, op={}",
            display.depth,
            just_op.depth,
        );
    }

    #[test]
    fn subscript_with_large_op_uses_limits_in_display() {
        // Subscript with LargeOp base in display mode should place sub below
        let expr = MathExpr::inline(vec![MathNode::Subscript {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.depth > just_op.depth,
            "Subscript limit should add depth below op"
        );
    }

    #[test]
    fn superscript_with_large_op_uses_limits_in_display() {
        // Superscript with LargeOp base in display mode should place sup above
        let expr = MathExpr::inline(vec![MathNode::Superscript {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.height > just_op.height,
            "Superscript limit should add height above op"
        );
    }

    #[test]
    fn subsup_with_large_op_no_limits_in_text() {
        // SubSup with LargeOp base in text mode should NOT use limits
        let expr = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Text,
            &opts,
        );
        // In non-limits mode, the width should be wider (scripts to the side)
        assert!(
            text.width > just_op.width,
            "SubSup in text mode should be wider than just the op (scripts beside, not above/below)"
        );
    }

    #[test]
    fn nary_nolimits_still_scales_in_display() {
        // Nary with SubSup limit style in display mode should still scale the op glyph
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text_expr = MathExpr::inline(vec![MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let text = layout_expr(&text_expr, MathStyle::Text, &opts);
        assert!(
            display.height > text.height,
            "Nary with SubSup style in display should still be taller (scaled op): display={}, text={}",
            display.height,
            text.height,
        );
    }

    // === Arena layout tests ===

    #[test]
    fn arena_layout_simple_number() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("42", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_superscript() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5, "superscript should have positive width");
    }

    #[test]
    fn arena_layout_subscript() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x_i", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5);
        assert!(b.depth > 0.0, "subscript should add depth");
    }

    #[test]
    fn arena_layout_subsup() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x_i^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5);
        assert!(b.height > 0.0);
        assert!(b.depth > 0.0);
    }

    #[test]
    fn arena_layout_fraction() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\frac{1}{2}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Display, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.height > 0.0);
        assert!(b.depth > 0.0);
    }

    #[test]
    fn arena_layout_sqrt() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\sqrt{x}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_fenced() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\left(x\\right)", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_nary_display() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\sum_{i=0}^{n} x", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Display, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_overunder() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\overset{a}{b}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_array() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            &config,
        ).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_accent() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\hat{x}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_depth_limit() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let mut latex = String::new();
        for _ in 0..150 {
            latex.push_str("\\frac{");
        }
        latex.push('x');
        for _ in 0..150 {
            latex.push_str("}{1}");
        }
        let config = crate::error::Config::default();
        if let Ok(result) = crate::reader::latex::LatexReader::parse_compact(&latex, &config) {
            let mut arena = LayoutArena::new();
            let opts = default_options();
            let builder = LayoutBuilder::new(MathStyle::Text, &opts);
            // Must not panic or overflow stack
            let _root = builder.layout(&mut arena, &result.expr);
        }
    }

    #[test]
    fn arena_clear_and_reuse() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);

        let root1 = builder.layout(&mut arena, &result.expr);
        let w1 = arena.get(root1).width;

        arena.clear();
        let root2 = builder.layout(&mut arena, &result.expr);
        let w2 = arena.get(root2).width;

        assert!((w1 - w2).abs() < f64::EPSILON, "reused arena should produce identical layout");
    }

    // === Arena-heap equivalence tests ===

    /// Helper: verify arena layout produces identical SVG to heap layout for a LaTeX string.
    fn assert_arena_heap_equivalent(latex: &str, style: MathStyle) {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::emit::{emit_svg, emit_svg_arena};

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config)
            .unwrap_or_else(|e| panic!("parse failed for {latex:?}: {e}"));
        let options = default_options();

        let heap_root = layout_expr_c(&result.expr, style, &options);
        let heap_svg = emit_svg(&heap_root, &options);

        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(style, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_svg = emit_svg_arena(&arena, arena_root, &options);

        assert_eq!(heap_svg, arena_svg, "mismatch for {latex:?}");
    }

    #[test]
    fn equivalence_simple() {
        assert_arena_heap_equivalent("x^2 + 1", MathStyle::Text);
    }

    #[test]
    fn equivalence_quadratic() {
        assert_arena_heap_equivalent(
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            MathStyle::Display,
        );
    }

    #[test]
    fn equivalence_nested_frac() {
        assert_arena_heap_equivalent("\\frac{\\frac{a}{b}}{\\frac{c}{d}}", MathStyle::Display);
    }

    #[test]
    fn equivalence_sum() {
        assert_arena_heap_equivalent("\\sum_{i=0}^{n} \\frac{1}{i^2}", MathStyle::Display);
    }

    #[test]
    fn equivalence_sqrt_with_index() {
        assert_arena_heap_equivalent("\\sqrt[3]{x}", MathStyle::Text);
    }

    #[test]
    fn equivalence_fenced() {
        assert_arena_heap_equivalent("\\left(\\frac{a}{b}\\right)", MathStyle::Display);
    }

    #[test]
    fn equivalence_matrix() {
        assert_arena_heap_equivalent(
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            MathStyle::Text,
        );
    }

    #[test]
    fn equivalence_accent() {
        assert_arena_heap_equivalent("\\hat{x} + \\vec{v}", MathStyle::Text);
    }

    #[test]
    fn equivalence_text() {
        assert_arena_heap_equivalent("\\text{hello}", MathStyle::Text);
    }

    #[test]
    fn equivalence_navier_stokes() {
        assert_arena_heap_equivalent(
            "\\rho\\left(\\frac{\\partial \\mathbf{u}}{\\partial t} + \\mathbf{u} \\cdot \\nabla \\mathbf{u}\\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{u} + \\mathbf{f}",
            MathStyle::Display,
        );
    }

    // =====================================================================
    // Aligned / multi-column alignment tests
    // =====================================================================

    /// Simple 2-column aligned environment: columns should be laid out
    /// side by side rather than stacked vertically.
    #[test]
    fn aligned_two_column_has_grid_layout() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        // With 2 columns, the layout width must be > single-column width.
        // A single character is ~0.5 units, so 2 columns + gap should be > 1.5.
        assert!(
            root.width > 1.5,
            "aligned block should be wider than a single column, got {}",
            root.width
        );
    }

    /// Multi-pair aligned environment: 4 columns (rl rl).
    #[test]
    fn aligned_four_column_has_grid_layout() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();

        // Verify the compact AST has num_cols = 4
        fn find_num_cols(arena: &AstArena, id: crate::ast::compact::NodeId) -> Option<u16> {
            match arena.node(id) {
                crate::ast::compact::CompactNode::Array { num_cols, .. } => Some(*num_cols),
                crate::ast::compact::CompactNode::Row(span) => {
                    for &child_id in arena.children(*span) {
                        if let Some(nc) = find_num_cols(arena, child_id) {
                            return Some(nc);
                        }
                    }
                    None
                },
                _ => None,
            }
        }
        let nc = {
            let arena = &result.expr.arena;
            let mut found = None;
            for &root_id in arena.children(result.expr.roots) {
                found = find_num_cols(arena, root_id);
                if found.is_some() {
                    break;
                }
            }
            found.expect("should find Array node")
        };
        assert_eq!(nc, 4, "aligned with 3 &'s per row should produce 4 columns");

        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        // 4 columns with gaps should be significantly wider than 2 columns
        assert!(
            root.width > 3.0,
            "4-column aligned block should be wide, got {}",
            root.width
        );
    }

    /// aligned environment: heap/compact/arena paths should all agree.
    #[test]
    fn equivalence_aligned() {
        assert_arena_heap_equivalent(
            r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}",
            MathStyle::Text,
        );
    }

    /// Multi-pair aligned: heap/compact/arena paths should all agree.
    #[test]
    fn equivalence_aligned_multi_pair() {
        assert_arena_heap_equivalent(
            r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}",
            MathStyle::Text,
        );
    }

    /// alignedat environment parses and lays out correctly.
    #[test]
    fn alignedat_parses_and_renders() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{alignedat}{2} x &= 1 & \quad y &= 2 \end{alignedat}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        assert!(root.width > 1.0, "alignedat should render with meaningful width");
    }
}
