use super::*;

/// Returns true if `ch` is an extensible arrow character used by \xrightarrow etc.
pub(super) fn is_extensible_arrow(ch: char) -> bool {
    matches!(
        ch,
        '\u{2190}' // ← leftarrow
        | '\u{2192}' // → rightarrow
        | '\u{21D0}' // ⇐ Leftarrow
        | '\u{21D2}' // ⇒ Rightarrow
        | '\u{2194}' // ↔ leftrightarrow
        | '\u{21D4}' // ⇔ Leftrightarrow
        | '\u{21A9}' // ↩ hookleftarrow
        | '\u{21AA}' // ↪ hookrightarrow
        | '\u{21A6}' // ↦ mapsto
        | '\u{21CC}' // ⇌ rightleftharpoons
        | '\u{21CB}' // ⇋ leftrightharpoons
        | '\u{21C4}' // ⇄ rightleftarrows (xtofrom)
        | '\u{21A0}' // ↠ twoheadrightarrow
        | '\u{219E}' // ↞ twoheadleftarrow
        | '\u{21BC}' // ↼ leftharpoonup
        | '\u{21BD}' // ↽ leftharpoondown
        | '\u{21C0}' // ⇀ rightharpoonup
        | '\u{21C1}' // ⇁ rightharpoondown
        | '=' // \xlongequal — stretchy double-bar equals
    )
}

/// Check if a CompactNode (in AstArena) is an extensible arrow symbol and return the char.
pub(super) fn get_extensible_arrow_c(arena: &AstArena, id: NodeId) -> Option<char> {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        let s = arena.str_val(sid);
        if let Some(ch) = s.chars().next()
            && is_extensible_arrow(ch)
        {
            return Some(ch);
        }
    }
    None
}

/// Check if a heap MathNode is an extensible arrow symbol and return the char.
pub(super) fn get_extensible_arrow_heap(node: &MathNode) -> Option<char> {
    if let MathNode::Symbol(_, s) = node
        && let Some(ch) = s.chars().next()
        && is_extensible_arrow(ch)
    {
        return Some(ch);
    }
    None
}

/// Half-width of an extensible arrowhead, in em. This is also half the arrow's
/// total vertical extent (the path frame is `[0, 2·this]`), so the arrow is
/// centered on the math axis with this much head on each side of the shaft.
pub(super) const ARROW_HEAD_HALF_W_EM: f64 = 0.21;

/// Build the box for an extensible arrow of the given char and total width,
/// centered on the math axis. Shared by the heap and compact layout paths
/// (both use `LayoutBox`); the arena path uses [`extensible_arrow_dims`].
pub(super) fn make_extensible_arrow_box(ch: char, arrow_w: f64, size: f64, axis: f64) -> LayoutBox {
    let (w, h, d) = extensible_arrow_dims(arrow_w, size, axis);
    LayoutBox {
        width: w,
        height: h,
        depth: d,
        kind: BoxKind::SvgPath(stretchy_arrow_svg(ch, arrow_w, size)),
    }
}

/// `(width, height, depth)` for an extensible-arrow box whose shaft sits on the
/// math axis. The path frame spans `[0, 2·tw]` with its center (the shaft) at
/// `tw`; placing that center at `axis` above the baseline gives these extents.
pub(super) fn extensible_arrow_dims(arrow_w: f64, size: f64, axis: f64) -> (f64, f64, f64) {
    let tw = ARROW_HEAD_HALF_W_EM * size;
    (arrow_w, axis + tw, (tw - axis).max(0.0))
}

/// Generate a stretchy SVG path for an extensible arrow character at the given
/// width and font `size` (em). Proportions (shaft thickness, head length/width)
/// are absolute fractions of the em so the arrow keeps the robust, uniform line
/// weight of a fraction bar / relation operator at any length — matching
/// MathJax/LaTeX rather than scaling down with the box.
pub(super) fn stretchy_arrow_svg(ch: char, w: f64, em: f64) -> String {
    // Extensible arrows are infrequent (only `\xrightarrow` & friends), so the
    // verbose `format!` builder is kept and its output is minimized in one pass
    // (trailing/leading zeros trimmed, minimal separators, collapsed commands).
    crate::render::path_writer::minify_path_d(&stretchy_arrow_svg_verbose(ch, w, em))
}

fn stretchy_arrow_svg_verbose(ch: char, w: f64, em: f64) -> String {
    let t = 0.027 * em; // half-thickness of arrow shaft (full ≈ rule thickness)
    let tip = 0.44 * em; // arrowhead length
    let tw = ARROW_HEAD_HALF_W_EM * em; // arrowhead half-width (perpendicular to shaft)
    let mid_y = tw; // vertical center → path frame is [0, 2·tw]

    match ch {
        // → rightarrow: shaft + right arrowhead with swept-back barbs
        '\u{2192}' => {
            let ax = w - tip; // back tips of the barbs
            let notch = ax + 0.42 * tip; // concave back of the head (toward the tip)
            format!(
                "F:M0,{y0:.4} L{notch:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{notch:.4},{y1:.4} L0,{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                notch = notch,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ← leftarrow: left arrowhead with swept-back barbs + shaft
        '\u{2190}' => {
            let notch = tip - 0.42 * tip; // concave back of the head (toward the tip)
            format!(
                "F:M{w:.4},{y0:.4} L{notch:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{notch:.4},{y1:.4} L{w:.4},{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                notch = notch,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ↔ leftrightarrow: arrowheads on both ends
        '\u{2194}' => {
            let ax = w - tip;
            format!(
                "F:M0,{mid:.4} L{tip:.4},{t1:.4} L{tip:.4},{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L{tip:.4},{y1:.4} L{tip:.4},{t2:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ⇒ double rightarrow: two shafts + right double arrowhead
        '\u{21D2}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!(
                "F:M0,{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{l1:.4} L0,{l1:.4} L0,{l0:.4} L{ax:.4},{l0:.4} L{ax:.4},{u1:.4} L0,{u1:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                l0 = lower - t,
                l1 = lower + t,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ⇐ double leftarrow
        '\u{21D0}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            format!(
                "F:M{w:.4},{u0:.4} L{tip:.4},{u0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{tip:.4},{l1:.4} L{w:.4},{l1:.4} L{w:.4},{l0:.4} L{tip:.4},{l0:.4} L{tip:.4},{u1:.4} L{w:.4},{u1:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                l0 = lower - t,
                l1 = lower + t,
                tip = tip,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ⇔ double leftrightarrow
        '\u{21D4}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!(
                "F:M0,{mid:.4} L{tip:.4},{t1:.4} L{tip:.4},{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{l1:.4} L{tip:.4},{l1:.4} L{tip:.4},{t2:.4} Z M{tip:.4},{u1:.4} L{ax:.4},{u1:.4} L{ax:.4},{l0:.4} L{tip:.4},{l0:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                l0 = lower - t,
                l1 = lower + t,
                tip = tip,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
        // ↦ mapsto: vertical bar + shaft + right arrowhead
        '\u{21A6}' => {
            let bar_w = 0.04 * em;
            let ax = w - tip;
            format!(
                "F:M0,{t1:.4} L0,{t2:.4} L{bw:.4},{t2:.4} L{bw:.4},{y1:.4} L{ax:.4},{y1:.4} L{ax:.4},{t2b:.4} L{w:.4},{mid:.4} L{ax:.4},{t1b:.4} L{ax:.4},{y0:.4} L{bw:.4},{y0:.4} L{bw:.4},{t1:.4} Z",
                bw = bar_w,
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw,
                t1b = mid_y - tw,
                t2b = mid_y + tw
            )
        },
        // ↪ hookrightarrow: shaft + right arrowhead + downward hook at tail
        '\u{21AA}' => {
            let ax = w - tip;
            let hx = t * 3.0; // hook offset from left edge
            let hr = t * 4.0; // hook depth below shaft
            format!(
                "F:M{hx:.4},{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L{hx:.4},{y1:.4} C0,{cb:.4} 0,{ct:.4} {hx:.4},{y0:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw,
                hx = hx,
                cb = mid_y + t + hr,
                ct = mid_y - t
            )
        },
        // ↩ hookleftarrow: shaft + left arrowhead + downward hook at tail
        '\u{21A9}' => {
            let hx = w - t * 3.0; // hook offset from right edge
            let hr = t * 4.0;
            format!(
                "F:M{hx:.4},{y0:.4} L{tip:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{tip:.4},{y1:.4} L{hx:.4},{y1:.4} C{w:.4},{cb:.4} {w:.4},{ct:.4} {hx:.4},{y0:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw,
                hx = hx,
                cb = mid_y + t + hr,
                ct = mid_y - t
            )
        },
        // ⇌ rightleftharpoons: upper right harpoon + lower left harpoon
        '\u{21CC}' => {
            let gap = 0.08 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!(
                "F:M0,{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{ut:.4} L{w:.4},{u1:.4} L{ax:.4},{u1:.4} L0,{u1:.4} Z M{w:.4},{l0:.4} L{tip:.4},{l0:.4} L{tip:.4},{lt:.4} L0,{l0:.4} L{tip:.4},{l1:.4} L{w:.4},{l1:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                ut = upper - tw,
                l0 = lower - t,
                l1 = lower + t,
                lt = lower + tw,
                tip = tip,
                ax = ax,
                w = w
            )
        },
        // ⇋ leftrightharpoons
        '\u{21CB}' => {
            let gap = 0.08 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!(
                "F:M{w:.4},{u0:.4} L{tip:.4},{u0:.4} L{tip:.4},{ut:.4} L0,{u1:.4} L{tip:.4},{u1:.4} L{w:.4},{u1:.4} Z M0,{l0:.4} L{ax:.4},{l0:.4} L{ax:.4},{lt:.4} L{w:.4},{l0:.4} L{ax:.4},{l1:.4} L0,{l1:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                ut = upper - tw,
                l0 = lower - t,
                l1 = lower + t,
                lt = lower + tw,
                tip = tip,
                ax = ax,
                w = w
            )
        },
        // ⇄ rightleftarrows: upper rightarrow + lower leftarrow (stacked full arrows)
        '\u{21C4}' => {
            let gap = 0.105 * em; // vertical offset of each arrow from center
            let htw = 0.09 * em; // arrowhead half-width of each sub-arrow
            let up = mid_y - gap;
            let lo = mid_y + gap;
            let ax = w - tip;
            let rnotch = ax + 0.42 * tip;
            let lnotch = tip - 0.42 * tip;
            format!(
                "F:M0,{u0:.4} L{rnotch:.4},{u0:.4} L{ax:.4},{ut:.4} L{w:.4},{up:.4} L{ax:.4},{ub:.4} L{rnotch:.4},{u1:.4} L0,{u1:.4} Z \
                 M{w:.4},{l0:.4} L{lnotch:.4},{l0:.4} L{tip:.4},{lt:.4} L0,{lo:.4} L{tip:.4},{lb:.4} L{lnotch:.4},{l1:.4} L{w:.4},{l1:.4} Z",
                u0 = up - t,
                u1 = up + t,
                ut = up - htw,
                ub = up + htw,
                l0 = lo - t,
                l1 = lo + t,
                lt = lo - htw,
                lb = lo + htw,
                up = up,
                lo = lo,
                ax = ax,
                rnotch = rnotch,
                lnotch = lnotch,
                tip = tip,
                w = w
            )
        },
        // ↠ twoheadrightarrow
        '\u{21A0}' => {
            let ax1 = w - tip;
            let ax2 = w - tip * 1.6;
            format!(
                "F:M0,{y0:.4} L{ax2:.4},{y0:.4} L{ax2:.4},{t1:.4} L{ax1:.4},{mid:.4} L{ax1:.4},{t1b:.4} L{w:.4},{mid:.4} L{ax1:.4},{t2b:.4} L{ax1:.4},{mid:.4} L{ax2:.4},{t2:.4} L{ax2:.4},{y1:.4} L0,{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax1 = ax1,
                ax2 = ax2,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw,
                t1b = mid_y - tw,
                t2b = mid_y + tw
            )
        },
        // ↞ twoheadleftarrow
        '\u{219E}' => {
            let t2x = tip * 1.6;
            format!(
                "F:M{w:.4},{y0:.4} L{t2x:.4},{y0:.4} L{t2x:.4},{t1:.4} L{tip:.4},{mid:.4} L{tip:.4},{t1b:.4} L0,{mid:.4} L{tip:.4},{t2b:.4} L{tip:.4},{mid:.4} L{t2x:.4},{t2:.4} L{t2x:.4},{y1:.4} L{w:.4},{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                t2x = t2x,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw,
                t1b = mid_y - tw,
                t2b = mid_y + tw
            )
        },
        // ↼ leftharpoonup: shaft + upper-left half arrowhead
        '\u{21BC}' => {
            format!(
                "F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{y1:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                w = w,
                t1 = mid_y - tw
            )
        },
        // ↽ leftharpoondown: shaft + lower-left half arrowhead
        '\u{21BD}' => {
            format!(
                "F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L0,{y0:.4} L{tip:.4},{t2:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                tip = tip,
                w = w,
                t2 = mid_y + tw
            )
        },
        // ⇀ rightharpoonup: shaft + upper-right half arrowhead
        '\u{21C0}' => {
            let ax = w - tip;
            format!(
                "F:M0,{y0:.4} L{ax:.4},{y0:.4} L{w:.4},{y0:.4} L{ax:.4},{t1:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                w = w,
                t1 = mid_y - tw
            )
        },
        // ⇁ rightharpoondown: shaft + lower-right half arrowhead
        '\u{21C1}' => {
            let ax = w - tip;
            format!(
                "F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t2:.4} L{w:.4},{y1:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                w = w,
                t2 = mid_y + tw
            )
        },
        // = long equals (\xlongequal): two horizontal bars spanning the width
        '=' => {
            let gap = 0.055 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            format!(
                "F:M0,{u0:.4} L{w:.4},{u0:.4} L{w:.4},{u1:.4} L0,{u1:.4} Z M0,{l0:.4} L{w:.4},{l0:.4} L{w:.4},{l1:.4} L0,{l1:.4} Z",
                u0 = upper - t,
                u1 = upper + t,
                l0 = lower - t,
                l1 = lower + t,
                w = w,
            )
        },
        // Fallback: simple right arrow
        _ => {
            let ax = w - tip;
            format!(
                "F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z",
                y0 = mid_y - t,
                y1 = mid_y + t,
                ax = ax,
                w = w,
                mid = mid_y,
                t1 = mid_y - tw,
                t2 = mid_y + tw
            )
        },
    }
}

// ---------------------------------------------------------------------------
// KaTeX-derived SVG path data for widehat, widetilde, and widecheck accents.
// Each accent has 4 tiers (progressively wider), selected by base width.
// Paths are from KaTeX's svgGeometry.ts (MIT license).
// ---------------------------------------------------------------------------

/// Fence contents are measured before their delimiters can be sized. Keep
/// each visible atom's spacing context alongside its measured box.
enum FencePart<'a, B> {
    Element {
        layout: B,
        atom: AtomType,
        style: MathStyle,
        size: f64,
    },
    Middle(&'a str),
}

fn fence_atom_gap(
    previous: &mut Option<AtomType>,
    atom: AtomType,
    style: MathStyle,
    size: f64,
) -> f64 {
    let (left, right) = spacing::adjust_bin_context(*previous, Some(atom));
    let current = right.unwrap_or(atom);
    *previous = Some(current);
    left.map_or(0.0, |left| {
        spacing::inter_atom_space(left, current, style) * size
    })
}

pub(super) fn layout_fenced(
    open: &str,
    close: &str,
    content: &[FencedItem],
    ctx: &LayoutCtx,
) -> LayoutBox {
    use crate::render::delimiter_paths;

    // Pass 1: Layout all Element items, compute max_h/max_d for delimiter sizing.
    // Store laid-out boxes for reuse in pass 2.
    let mut laid_out = Vec::with_capacity(content.len());
    let mut current_ctx = ctx.clone();
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    let mut element_count = 0usize;
    let mut sole_nobar_frac = false;

    for item in content {
        match item {
            FencedItem::Element(MathNode::StyleSwitch(value)) => {
                // A size, unlike a style, lasts across a `\middle`.
                let size_scale = current_ctx.size_scale;
                current_ctx = ctx.with_switch(*value);
                current_ctx.size_scale = size_scale;
            },
            FencedItem::Element(MathNode::SizeSwitch(idx)) => {
                current_ctx.size_scale = size_scale_factor(*idx);
            },
            FencedItem::Element(n) => {
                let child = layout_node(n, &current_ctx);
                max_h = max_h.max(child.height);
                max_d = max_d.max(child.depth);
                laid_out.push(FencePart::Element {
                    layout: child,
                    atom: spacing::classify_node_in(n, current_ctx.branch_style()),
                    style: current_ctx.effective_spacing_style(),
                    size: current_ctx.size(),
                });
                element_count += 1;
                sole_nobar_frac = element_count == 1
                    && matches!(
                        n,
                        MathNode::Fraction {
                            kind: FractionKind::NoBar,
                            ..
                        }
                    );
            },
            FencedItem::Middle(delim) => {
                laid_out.push(FencePart::Middle(delim.as_str()));
                let size_scale = current_ctx.size_scale;
                current_ctx = ctx.clone();
                current_ctx.size_scale = size_scale;
            },
        }
    }

    // Detect fraction-with-delimiters (e.g., \binom{n}{k}) — TeX Rule 15e
    let is_genfrac = element_count == 1 && sole_nobar_frac;

    let axis = ctx.params.axis_height * ctx.size();
    let target_h = if is_genfrac {
        // TeX Rule 15e: fixed minimum delimiter height for fractions
        let delim_min = if ctx.branch_style().is_display() {
            ctx.params.delim1 * ctx.size()
        } else {
            ctx.params.delim2 * ctx.size()
        };
        delim_min.max(max_h + max_d)
    } else {
        // TeX Rule 19: delimiterFactor/delimiterShortfall for \left...\right
        let max_dist_from_axis = (max_h - axis).max(max_d + axis);
        let delim_factor = 901.0_f64;
        let delim_shortfall = 0.5 * ctx.size();
        let target = (max_dist_from_axis / 500.0 * delim_factor)
            .max(2.0 * max_dist_from_axis - delim_shortfall);
        target.max(1.0 * ctx.size())
    };
    let target_h = quantize_delim_height(target_h);
    let half = target_h / 2.0;
    let delim_h = half + axis;
    let delim_d = half - axis;

    // Helper: compute delimiter width for a character at the computed target height
    let compute_delim_w = |ch: char| -> f64 {
        if let Some(v) = glyph_variants::delimiter_variant(ch, target_h) {
            v.h_adv_em * (target_h / v.advance_em)
        } else if let Some(asm) = glyph_variants::vertical_assembly(ch, target_h) {
            asm.width_em * ctx.size()
        } else if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        }
    };

    // Pass 2: Build positioned children with open, content, middle delimiters, close
    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<spacing::AtomType> = None;

    // Open delimiter
    if !open.is_empty()
        && open != "."
        && let Some(ch) = open.chars().next()
    {
        let delim = delimiter_box(
            ch,
            compute_delim_w(ch),
            delim_h,
            delim_d,
            target_h,
            ctx.size(),
        );
        let dw = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += dw;
        // Inner padding between open delimiter and content (TeX null-delimiter-space)
        cursor_x += 0.12 * ctx.size();
    }

    // Content + middle delimiters
    for part in laid_out {
        match part {
            FencePart::Element {
                layout: child,
                atom,
                style,
                size,
            } => {
                cursor_x += fence_atom_gap(&mut prev_atom, atom, style, size);
                let w = child.width;
                children.push(PositionedBox::new(cursor_x, 0.0, child));
                cursor_x += w;
            },
            FencePart::Middle(s) => {
                if s != "."
                    && let Some(ch) = s.chars().next()
                {
                    // Spacing before middle delimiter (treat as Inner atom)
                    if let Some(prev) = prev_atom {
                        let space = spacing::inter_atom_space(
                            prev,
                            spacing::AtomType::Inner,
                            ctx.effective_spacing_style(),
                        );
                        if space > 0.0 {
                            cursor_x += space * ctx.size();
                        }
                    }
                    let delim = delimiter_box(
                        ch,
                        compute_delim_w(ch),
                        delim_h,
                        delim_d,
                        target_h,
                        ctx.size(),
                    );
                    let dw = delim.width;
                    children.push(PositionedBox::new(cursor_x, 0.0, delim));
                    cursor_x += dw;
                    // Next element sees middle delimiter as Inner for spacing
                    prev_atom = Some(spacing::AtomType::Inner);
                }
            },
        }
    }

    // Close delimiter
    if !close.is_empty()
        && close != "."
        && let Some(ch) = close.chars().next()
    {
        // Inner padding between content and close delimiter
        cursor_x += 0.12 * ctx.size();
        let delim = delimiter_box(
            ch,
            compute_delim_w(ch),
            delim_h,
            delim_d,
            target_h,
            ctx.size(),
        );
        let dw = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += dw;
    }

    LayoutBox {
        width: cursor_x,
        height: max_h.max(delim_h),
        depth: max_d.max(delim_d),
        kind: BoxKind::HBox { children },
    }
}

pub(super) fn layout_fenced_arena(
    ast: &AstArena,
    open: StrId,
    close: StrId,
    content: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    use crate::render::delimiter_paths;

    // Pass 1: Layout all Element items, compute max_h/max_d for delimiter sizing.
    let content_children = ast.children(content);
    let mut laid_out = Vec::with_capacity(content_children.len());
    let mut current_ctx = ctx.clone();
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    let mut element_count = 0usize;
    let mut sole_nobar_frac = false;

    for &child_id in content_children {
        match *ast.node(child_id) {
            CompactNode::StyleSwitch(value) => {
                // A size, unlike a style, lasts across a `\middle`.
                let size_scale = current_ctx.size_scale;
                current_ctx = ctx.with_switch(value);
                current_ctx.size_scale = size_scale;
            },
            CompactNode::SizeScale(idx) => {
                current_ctx.size_scale = size_scale_factor(idx);
            },
            CompactNode::Middle(sid) => {
                laid_out.push(FencePart::Middle(ast.str_val(sid)));
                let size_scale = current_ctx.size_scale;
                current_ctx = ctx.clone();
                current_ctx.size_scale = size_scale;
            },
            _ => {
                let laid = layout_node_arena(ast, child_id, arena, &current_ctx);
                let b = arena.get(laid);
                max_h = max_h.max(b.height);
                max_d = max_d.max(b.depth);
                laid_out.push(FencePart::Element {
                    layout: laid,
                    atom: spacing::classify_node_c_in(ast, child_id, current_ctx.branch_style()),
                    style: current_ctx.effective_spacing_style(),
                    size: current_ctx.size(),
                });
                element_count += 1;
                sole_nobar_frac = element_count == 1
                    && matches!(
                        *ast.node(child_id),
                        CompactNode::Fraction {
                            kind: FractionKind::NoBar,
                            ..
                        }
                    );
            },
        }
    }

    // Detect fraction-with-delimiters (e.g., \binom) — TeX Rule 15e
    let is_genfrac = element_count == 1 && sole_nobar_frac;

    let axis = ctx.params.axis_height * ctx.size();
    let target_h = if is_genfrac {
        let delim_min = if ctx.branch_style().is_display() {
            ctx.params.delim1 * ctx.size()
        } else {
            ctx.params.delim2 * ctx.size()
        };
        delim_min.max(max_h + max_d)
    } else {
        let max_dist_from_axis = (max_h - axis).max(max_d + axis);
        let delim_factor = 901.0_f64;
        let delim_shortfall = 0.5 * ctx.size();
        let target = (max_dist_from_axis / 500.0 * delim_factor)
            .max(2.0 * max_dist_from_axis - delim_shortfall);
        target.max(1.0 * ctx.size())
    };
    let target_h = quantize_delim_height(target_h);
    // Recompute axis-centered delimiter box dimensions with updated target
    let half = target_h / 2.0;
    let delim_h = half + axis;
    let delim_d = half - axis;

    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<spacing::AtomType> = None;

    // Open delimiter
    let open_str = ast.str_val(open);
    if !open_str.is_empty()
        && open_str != "."
        && let Some(ch) = open_str.chars().next()
    {
        let delim_width = if let Some(v) = glyph_variants::delimiter_variant(ch, target_h) {
            v.h_adv_em * (target_h / v.advance_em)
        } else if let Some(asm) = glyph_variants::vertical_assembly(ch, target_h) {
            asm.width_em * ctx.size()
        } else if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim_id = alloc_delimiter(
            arena,
            ch,
            delim_width,
            delim_h,
            delim_d,
            target_h,
            ctx.size(),
        );
        let dw = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox {
            dx: cursor_x,
            dy: 0.0,
            id: delim_id,
        });
        cursor_x += dw;
        // Inner padding between open delimiter and content (TeX null-delimiter-space)
        cursor_x += 0.12 * ctx.size();
    }

    // Content + middle delimiters
    for part in laid_out {
        match part {
            FencePart::Middle(s) => {
                if s != "."
                    && let Some(ch) = s.chars().next()
                {
                    // Spacing before middle delimiter (treat as Inner atom)
                    if let Some(prev) = prev_atom {
                        let space = spacing::inter_atom_space(
                            prev,
                            spacing::AtomType::Inner,
                            ctx.effective_spacing_style(),
                        );
                        if space > 0.0 {
                            cursor_x += space * ctx.size();
                        }
                    }
                    let delim_width =
                        if let Some(v) = glyph_variants::delimiter_variant(ch, target_h) {
                            v.h_adv_em * (target_h / v.advance_em)
                        } else if let Some(asm) = glyph_variants::vertical_assembly(ch, target_h) {
                            asm.width_em * ctx.size()
                        } else if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
                            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
                        } else {
                            let base_delim_width = 0.389 * ctx.size();
                            let standard_height = 0.9 * ctx.size();
                            if target_h > standard_height {
                                base_delim_width * (target_h / standard_height)
                            } else {
                                base_delim_width
                            }
                        };
                    let delim_id = alloc_delimiter(
                        arena,
                        ch,
                        delim_width,
                        delim_h,
                        delim_d,
                        target_h,
                        ctx.size(),
                    );
                    let dw = arena.get(delim_id).width;
                    arena.push_child(ArenaPositionedBox {
                        dx: cursor_x,
                        dy: 0.0,
                        id: delim_id,
                    });
                    cursor_x += dw;
                    prev_atom = Some(spacing::AtomType::Inner);
                }
            },
            FencePart::Element {
                layout: laid,
                atom,
                style,
                size,
            } => {
                cursor_x += fence_atom_gap(&mut prev_atom, atom, style, size);
                let w = arena.get(laid).width;
                arena.push_child(ArenaPositionedBox {
                    dx: cursor_x,
                    dy: 0.0,
                    id: laid,
                });
                cursor_x += w;
            },
        }
    }

    // Close delimiter
    let close_str = ast.str_val(close);
    if !close_str.is_empty()
        && close_str != "."
        && let Some(ch) = close_str.chars().next()
    {
        // Inner padding between content and close delimiter
        cursor_x += 0.12 * ctx.size();
        let delim_width = if let Some(v) = glyph_variants::delimiter_variant(ch, target_h) {
            v.h_adv_em * (target_h / v.advance_em)
        } else if let Some(asm) = glyph_variants::vertical_assembly(ch, target_h) {
            asm.width_em * ctx.size()
        } else if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim_id = alloc_delimiter(
            arena,
            ch,
            delim_width,
            delim_h,
            delim_d,
            target_h,
            ctx.size(),
        );
        let dw = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox {
            dx: cursor_x,
            dy: 0.0,
            id: delim_id,
        });
        cursor_x += dw;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_h.max(delim_h),
        depth: max_d.max(delim_d),
        kind: ArenaBoxKind::HBox { children },
    })
}

/// A stretchy-delimiter box for `ch`, or — when no font glyph, MATH variant
/// or built-in path can draw `ch` — its [`glyph_fallback`] outline stretched to
/// the delimiter's extent, so the delimiter is never invisible.
pub(super) fn delimiter_box(
    ch: char,
    width: f64,
    height: f64,
    depth: f64,
    target_h: f64,
    size: f64,
) -> LayoutBox {
    if glyph_fallback::delimiter_needs_fallback(ch, target_h) {
        let s = glyph_fallback::delimiter_shape(ch, size, height, depth);
        return LayoutBox {
            width: s.width,
            height: s.height,
            depth: s.depth,
            kind: BoxKind::SvgPath(s.path),
        };
    }
    LayoutBox {
        width,
        height,
        depth,
        kind: BoxKind::Delimiter {
            ch,
            target_height: target_h,
            scale: 1.0,
        },
    }
}

/// Arena counterpart of [`delimiter_box`].
pub(super) fn alloc_delimiter(
    arena: &mut LayoutArena,
    ch: char,
    width: f64,
    height: f64,
    depth: f64,
    target_h: f64,
    size: f64,
) -> LayoutBoxId {
    if glyph_fallback::delimiter_needs_fallback(ch, target_h) {
        let s = glyph_fallback::delimiter_shape(ch, size, height, depth);
        return arena.alloc(ArenaLayoutBox {
            width: s.width,
            height: s.height,
            depth: s.depth,
            kind: ArenaBoxKind::SvgPath(s.path),
        });
    }
    arena.alloc(ArenaLayoutBox {
        width,
        height,
        depth,
        kind: ArenaBoxKind::Delimiter {
            ch,
            target_height: target_h,
            scale: 1.0,
        },
    })
}

/// Width of `\big.`: KaTeX's `.nulldelimiter` (1.2pt, the value of TeX's
/// `\nulldelimiterspace`). Plain TeX's and amsmath's `\big.` are zero-width,
/// since their macros set `\nulldelimiterspace` to 0 inside the box; KaTeX and
/// MathJax keep the space.
pub(super) const NULL_DELIMITER_SPACE_EM: f64 = 0.12;

/// Geometry of a `\big`-family delimiter drawn through the MATH table.
#[derive(Debug, Clone, Copy)]
pub(super) struct BigDelimiter {
    pub ch: char,
    /// Advance width of the drawn glyph.
    pub width: f64,
    /// Height and depth of the `Delimiter` box around its own baseline.
    pub height: f64,
    pub depth: f64,
    /// `target_height` of the `Delimiter` box. The outline emitters (embedded
    /// SVG, PNG) and the glyph subsetter pick the glyph from
    /// `target_height / scale` through [`glyph_variants::delimiter_variant`],
    /// or the assembly when that returns `None`, exactly as the layout does
    /// here. The text SVG draws its built-in path for the character, or the
    /// font glyph as `<text>` squeezed into `width`.
    pub target_height: f64,
    /// `scale` of the `Delimiter` box: the font size the glyph is drawn at.
    pub scale: f64,
    /// Raise applied to the box so the glyph is centered on the math axis,
    /// as TeX centers every `\left` delimiter.
    pub shift: f64,
}

/// The `\big`-family size whose `Scaled` factor is `factor`.
pub(super) fn is_big_delim_factor(factor: Rational) -> bool {
    crate::ast::BIG_DELIM_SIZES
        .iter()
        .any(|size| size.scale == factor)
}

/// Lay out the delimiter `ch` of a `\big`, `\Big`, `\bigg` or `\Bigg` whose
/// `Scaled` factor is `factor`, at font size `size`.
///
/// `size` is the text-style size (`\large`, `\Huge`, … included): TeX,
/// amsmath and KaTeX set these delimiters at text size even in a script.
///
/// The factors (1.2, 1.8, 2.4, 3.0) double as TeX's delimiter heights in em:
/// plain TeX's 8.5–17.5pt boxes pass through Rule 19 to requests that cmex
/// answers with its `size1`–`size4` glyphs, which KaTeX's `sizeToMaxHeight`
/// lists as 1.2/1.8/2.4/3.0 em. The glyph is the font's size variant for that
/// height (see [`glyph_variants::big_delimiter_glyph`]) drawn at its natural
/// size rather than a scaled base outline, or an extensible assembly when the
/// target is taller than every variant.
///
/// Returns `None` when `factor` is not a `\big` size or the font has no
/// variants for `ch`; the caller then keeps scaling the content.
pub(super) fn big_delimiter(
    factor: Rational,
    ch: char,
    size: f64,
    axis: f64,
) -> Option<BigDelimiter> {
    if !is_big_delim_factor(factor) || size.is_nan() || size <= 0.0 {
        return None;
    }
    let target_em = factor.as_f64();
    let target = match glyph_variants::big_delimiter_glyph(ch, target_em)? {
        glyph_variants::BigDelimiterGlyph::Variant(v) => v.advance_em,
        glyph_variants::BigDelimiterGlyph::Assembly => target_em,
    };
    let target_height = target * size;
    // The emitters' own choice for this box; for a variant it is the one
    // chosen above, drawn at its natural size.
    if let Some(v) = glyph_variants::delimiter_variant(ch, target) {
        let s = target_height / v.advance_em;
        let height = v.y_max_em * s;
        let depth = -v.y_min_em * s;
        return Some(BigDelimiter {
            ch,
            width: v.h_adv_em * s,
            height,
            depth,
            target_height,
            scale: size,
            shift: axis - (height - depth) / 2.0,
        });
    }
    let asm = glyph_variants::vertical_assembly(ch, target)?;
    // The emitters stretch the assembly over the whole box.
    Some(BigDelimiter {
        ch,
        width: asm.width_em * size,
        height: target_height / 2.0,
        depth: target_height / 2.0,
        target_height,
        scale: size,
        shift: axis,
    })
}

/// Heap layout of a [`BigDelimiter`]: the delimiter box raised onto the axis.
pub(super) fn big_delimiter_box(d: BigDelimiter) -> LayoutBox {
    let mut delim = delimiter_box(d.ch, d.width, d.height, d.depth, d.target_height, d.scale);
    if let BoxKind::Delimiter { scale, .. } = &mut delim.kind {
        *scale = d.scale;
    }
    LayoutBox {
        width: delim.width,
        height: (delim.height + d.shift).max(0.0),
        depth: (delim.depth - d.shift).max(0.0),
        kind: BoxKind::HBox {
            children: vec![PositionedBox::new(0.0, d.shift, delim)],
        },
    }
}

/// Arena counterpart of [`big_delimiter_box`].
pub(super) fn alloc_big_delimiter(arena: &mut LayoutArena, d: BigDelimiter) -> LayoutBoxId {
    let delim = alloc_delimiter(
        arena,
        d.ch,
        d.width,
        d.height,
        d.depth,
        d.target_height,
        d.scale,
    );
    if let ArenaBoxKind::Delimiter { scale, .. } = &mut arena.get_mut(delim).kind {
        *scale = d.scale;
    }
    let (width, height, depth) = {
        let b = arena.get(delim);
        (b.width, b.height, b.depth)
    };
    let start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: d.shift,
        id: delim,
    });
    let children = arena.finish_children(start);
    arena.alloc(ArenaLayoutBox {
        width,
        height: (height + d.shift).max(0.0),
        depth: (depth - d.shift).max(0.0),
        kind: ArenaBoxKind::HBox { children },
    })
}
