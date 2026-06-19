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

pub(super) fn layout_fenced(
    open: &str,
    close: &str,
    content: &[FencedItem],
    ctx: &LayoutCtx,
) -> LayoutBox {
    use crate::render::delimiter_paths;

    // Pass 1: Layout all Element items, compute max_h/max_d for delimiter sizing.
    // Store laid-out boxes for reuse in pass 2.
    let mut laid_out: Vec<Option<LayoutBox>> = Vec::with_capacity(content.len());
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    let mut element_count = 0usize;
    let mut sole_nobar_frac = false;

    for item in content {
        match item {
            FencedItem::Element(n) => {
                let child = layout_node(n, ctx);
                max_h = max_h.max(child.height);
                max_d = max_d.max(child.depth);
                laid_out.push(Some(child));
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
            FencedItem::Middle(_) => {
                laid_out.push(None);
            },
        }
    }

    // Detect fraction-with-delimiters (e.g., \binom{n}{k}) — TeX Rule 15e
    let is_genfrac = element_count == 1 && sole_nobar_frac;

    let axis = ctx.params.axis_height * ctx.size();
    let target_h = if is_genfrac {
        // TeX Rule 15e: fixed minimum delimiter height for fractions
        let delim_min = if ctx.style.is_display() {
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
    let mut laid_iter = laid_out.into_iter();

    // Open delimiter
    if !open.is_empty()
        && open != "."
        && let Some(ch) = open.chars().next()
    {
        let dw = compute_delim_w(ch);
        children.push(PositionedBox::new(
            cursor_x,
            0.0,
            LayoutBox {
                width: dw,
                height: delim_h,
                depth: delim_d,
                kind: BoxKind::Delimiter {
                    ch,
                    target_height: target_h,
                },
            },
        ));
        cursor_x += dw;
        // Inner padding between open delimiter and content (TeX null-delimiter-space)
        cursor_x += 0.12 * ctx.size();
    }

    // Content + middle delimiters
    for item in content {
        match item {
            FencedItem::Element(n) => {
                // Safety: laid_out has one entry per content item
                let child = laid_iter
                    .next()
                    .expect("fenced layout: iterator exhausted")
                    .expect("fenced layout: element was None");
                // Inter-atom spacing
                let raw_atom = spacing::classify_node(n);
                let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
                let cur_atom = adj_cur.unwrap(); // safe: we passed Some
                if let Some(prev) = adj_prev {
                    let space =
                        spacing::inter_atom_space(prev, cur_atom, ctx.effective_spacing_style());
                    if space > 0.0 {
                        cursor_x += space * ctx.size();
                    }
                }
                let w = child.width;
                children.push(PositionedBox::new(cursor_x, 0.0, child));
                cursor_x += w;
                prev_atom = Some(cur_atom);
            },
            FencedItem::Middle(s) => {
                laid_iter.next(); // consume the None placeholder
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
                    let dw = compute_delim_w(ch);
                    children.push(PositionedBox::new(
                        cursor_x,
                        0.0,
                        LayoutBox {
                            width: dw,
                            height: delim_h,
                            depth: delim_d,
                            kind: BoxKind::Delimiter {
                                ch,
                                target_height: target_h,
                            },
                        },
                    ));
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
        let dw = compute_delim_w(ch);
        children.push(PositionedBox::new(
            cursor_x,
            0.0,
            LayoutBox {
                width: dw,
                height: delim_h,
                depth: delim_d,
                kind: BoxKind::Delimiter {
                    ch,
                    target_height: target_h,
                },
            },
        ));
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
    let mut laid_out: Vec<Option<LayoutBoxId>> = Vec::with_capacity(content_children.len());
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    let mut element_count = 0usize;
    let mut sole_nobar_frac = false;

    for &child_id in content_children {
        match *ast.node(child_id) {
            CompactNode::Middle(_) => {
                laid_out.push(None);
            },
            _ => {
                let laid = layout_node_arena(ast, child_id, arena, ctx);
                let b = arena.get(laid);
                max_h = max_h.max(b.height);
                max_d = max_d.max(b.depth);
                laid_out.push(Some(laid));
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
        let delim_min = if ctx.style.is_display() {
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
    let mut laid_iter = laid_out.into_iter();

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
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
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
    for &child_id in content_children {
        match *ast.node(child_id) {
            CompactNode::Middle(sid) => {
                laid_iter.next(); // consume the None placeholder
                let s = ast.str_val(sid);
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
                    let delim_id = arena.alloc(ArenaLayoutBox {
                        width: delim_width,
                        height: delim_h,
                        depth: delim_d,
                        kind: ArenaBoxKind::Delimiter {
                            ch,
                            target_height: target_h,
                        },
                    });
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
            _ => {
                // Safety: laid_iter yields one item per non-delimiter child; unwrap safe because
                // the iterator was built from the same child list we're iterating
                let laid = laid_iter.next().unwrap().unwrap();
                // Inter-atom spacing
                let raw_atom = spacing::classify_node_c(ast, child_id);
                let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
                let cur_atom = adj_cur.unwrap(); // safe: we passed Some(raw_atom)
                if let Some(prev) = adj_prev {
                    let space =
                        spacing::inter_atom_space(prev, cur_atom, ctx.effective_spacing_style());
                    if space > 0.0 {
                        cursor_x += space * ctx.size();
                    }
                }
                let w = arena.get(laid).width;
                arena.push_child(ArenaPositionedBox {
                    dx: cursor_x,
                    dy: 0.0,
                    id: laid,
                });
                cursor_x += w;
                prev_atom = Some(cur_atom);
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
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
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
