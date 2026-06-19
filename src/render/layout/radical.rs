use super::*;
use crate::render::path_writer::PathWriter;

/// Generate a **filled** SVG path string for a radical sign (square root
/// symbol), closely matching the proportions of KaTeX's sqrtMain glyph.
///
/// The path is in local em coordinates where (0,0) is the top-left of the
/// bounding box.  `w` is the width of the radical symbol and `h` is the
/// total height it must cover.
///
/// The shape follows KaTeX's radical sign structure (from `svgGeometry.ts`):
///   - A small, thin tail/serif at the upper-left (~60% down from top)
///   - A steep diagonal downstroke to the V-bottom (~20% of width)
///   - A very steep, thin upstroke from V-bottom to the vinculum (~42% of w)
///   - A vinculum strip extending to the right edge
///   - An inner return path giving the shape proper thickness
///
/// Uses cubic bezier curves for smooth transitions between segments,
/// matching the quality of font-based radical glyphs.
///
/// The returned string is prefixed with "F:" so the emitter renders it as
/// a filled shape (`fill="currentColor"`) rather than a stroked path.
pub(crate) fn radical_path(w: f64, h: f64, t: f64) -> String {
    // These proportions are derived from KaTeX's sqrtMain glyph geometry.
    let tail_x = 0.10 * w; // right end of the small tail/serif
    let bottom_x = 0.20 * w; // x of the V-bottom (closer to left for sharper look)
    let top_x = 0.42 * w; // x where the upstroke meets the vinculum

    // --- Vertical position of the tail (60% down from top) ---
    let tail_y = 0.62 * h;

    // How far the tail slopes downward from start to end
    let tail_drop = 0.08 * h;

    // Upstroke half-width: the extended vertical stroke of a tall surd must read
    // as a solid stroke, so keep it close to (slightly above) the vinculum
    // thickness rather than a hairline. Thin upstrokes make tall radicals look
    // spindly and fail to visually enclose the radicand.
    let up_hw = t * 0.75;

    // === Outer edge (clockwise) ===
    // P0: tail start (left edge, upper edge of tail)
    let ox0 = 0.0;
    let oy0 = tail_y - t * 0.25;
    // P1: tail end — slopes downward steeply before the main diagonal
    let ox1 = tail_x;
    let oy1 = tail_y + tail_drop;
    // P2: V-bottom (outer, the deepest point of the radical)
    let ox2 = bottom_x;
    let oy2 = h;
    // P3: top of upstroke (outer left edge), meets vinculum at y=0
    let ox3 = top_x - up_hw;
    let oy3 = 0.0;
    // P4: vinculum right end, top edge
    let vx0 = w;
    let vy0 = 0.0;

    // === Inner edge (counter-clockwise, forming the return path) ===
    // P5: vinculum right end, bottom edge
    let vx1 = w;
    let vy1 = t;
    // P6: top of upstroke (inner right edge)
    let ix0 = top_x + up_hw;
    let iy0 = t;
    // P7: V-bottom inner — raised well above outer V for a sharp point.
    // Clamp so the inner contour stays below the vinculum even for thin radicals.
    let ix1 = bottom_x + t * 0.30;
    let iy1 = (h - t * 1.5).max(t * 2.0).min(h - t * 0.5);
    // P8: inner tail end
    let ix2 = tail_x + t * 0.15;
    let iy2 = tail_y + tail_drop - t * 0.7;
    // P9: inner tail start (left edge, lower edge of tail)
    let ix3 = 0.0;
    let iy3 = tail_y + t * 0.35;

    // Use cubic bezier curves for smooth transitions:
    // - Tail to downstroke: smooth curve through P0→P1→P2
    // - V-bottom to upstroke: smooth sharp turn at P2→P3
    // - Inner return: smooth curves for consistent stroke weight

    // Control points for outer V-bottom curve (P1→P2→P3)
    // The downstroke approaches the bottom nearly vertically
    let cp_down_x = ox1 + (ox2 - ox1) * 0.3;
    let cp_down_y = oy1 + (oy2 - oy1) * 0.7;
    // The upstroke leaves the bottom nearly vertically
    let cp_up_x = ox2 + (ox3 - ox2) * 0.05;
    let cp_up_y = oy2 - (oy2 - oy3) * 0.02;

    // Control points for inner V-bottom curve (ix0→ix1→ix2)
    let cp_idown_x = ix0 + (ix1 - ix0) * 0.05;
    let cp_idown_y = iy0 + (iy1 - iy0) * 0.5;
    let cp_iup_x = ix1 + (ix2 - ix1) * 0.3;
    let cp_iup_y = iy1 - (iy1 - iy2) * 0.1;

    // Build the path with PathWriter, which emits minimal `d` data directly
    // (trimmed numbers, minimal separators, collapsed repeated commands).
    let mut pw = PathWriter::with_prefix("F:");
    pw.cmd(b'M');
    pw.push_f64(ox0);
    pw.push_f64(oy0);
    pw.cmd(b'L');
    pw.push_f64(ox1);
    pw.push_f64(oy1);
    pw.cmd(b'C');
    pw.push_f64(cp_down_x);
    pw.push_f64(cp_down_y);
    pw.push_f64(ox2);
    pw.push_f64(oy2);
    pw.push_f64(ox2);
    pw.push_f64(oy2);
    pw.cmd(b'C');
    pw.push_f64(cp_up_x);
    pw.push_f64(cp_up_y);
    pw.push_f64(ox3);
    pw.push_f64(oy3);
    pw.push_f64(ox3);
    pw.push_f64(oy3);
    pw.cmd(b'L');
    pw.push_f64(vx0);
    pw.push_f64(vy0);
    pw.cmd(b'L');
    pw.push_f64(vx1);
    pw.push_f64(vy1);
    pw.cmd(b'L');
    pw.push_f64(ix0);
    pw.push_f64(iy0);
    pw.cmd(b'C');
    pw.push_f64(cp_idown_x);
    pw.push_f64(cp_idown_y);
    pw.push_f64(ix1);
    pw.push_f64(iy1);
    pw.push_f64(ix1);
    pw.push_f64(iy1);
    pw.cmd(b'C');
    pw.push_f64(cp_iup_x);
    pw.push_f64(cp_iup_y);
    pw.push_f64(ix2);
    pw.push_f64(iy2);
    pw.push_f64(ix2);
    pw.push_f64(iy2);
    pw.cmd(b'L');
    pw.push_f64(ix3);
    pw.push_f64(iy3);
    pw.close();
    pw.finish()
}

/// Resolved geometry for drawing a radical surd, shared by the heap / compact /
/// arena `\sqrt` layouts so all three position the surd glyph identically.
pub(super) struct RadicalGeom {
    /// Radical (vinculum) rule thickness, in absolute units.
    thickness: f64,
    /// Height at which the vinculum / surd top sits above the row baseline.
    shift_up: f64,
    /// Horizontal width occupied by the surd glyph, in absolute units.
    surd_width: f64,
    /// Surd box height (above its own baseline).
    box_height: f64,
    /// Surd box depth (below its own baseline).
    box_depth: f64,
    /// Vertical offset (baseline shift) at which to place the surd box.
    surd_dy: f64,
    /// Target height passed to `BoxKind::Delimiter` so the emitters pick the
    /// same MATH-table variant the layout positioned against. `None` means no
    /// font variant is available — callers fall back to the synthetic path.
    surd_target: Option<f64>,
    /// Synthetic-path height, used only when `surd_target` is `None`.
    synthetic_height: f64,
}

/// Compute radical surd geometry for a radicand of the given height/depth.
///
/// When the Latin Modern Math font exposes `√` (U+221A) MATH-table size
/// variants (it does), the surd is rendered from the real font glyph selected
/// to enclose the radicand — matching KaTeX/MathJax optical sizing. The
/// vinculum top is aligned with the surd glyph top, and the surd descends to
/// cover the radicand depth. Falls back to a synthetic path otherwise.
pub(super) fn radical_geometry(
    inner_height: f64,
    inner_depth: f64,
    size: f64,
    rule_thickness_em: f64,
    x_height_em: f64,
    is_display: bool,
) -> RadicalGeom {
    let rule_thickness = rule_thickness_em * size;
    // Radical strokes thicker than fraction rules for better visibility.
    let thickness = math_table::math_constants()
        .map(|c| (c.radical_rule_thickness * size).max(0.06 * size))
        .unwrap_or((rule_thickness * 1.5).max(0.06 * size));

    // KaTeX-compatible clearance: theta + phi/4, where phi = xHeight in display
    // mode (≈ the MATH `radicalDisplayStyleVerticalGap`) and theta otherwise
    // (≈ the MATH `radicalVerticalGap`).
    let phi = if is_display {
        x_height_em * size
    } else {
        rule_thickness
    };
    let clearance = rule_thickness + phi / 4.0;

    // Total surd height needed to clear the radicand plus the vinculum.
    let min_height = inner_height + inner_depth + clearance + thickness;
    // The vinculum / surd top.
    let shift_up = inner_height + clearance + thickness;

    if let Some(v) = glyph_variants::delimiter_variant('\u{221A}', min_height) {
        // The emitters render the chosen variant scaled so its vertical advance
        // equals `target_height`; replicate that scale here for placement.
        let scale = min_height / v.advance_em;
        let surd_top = v.y_max_em * scale; // glyph top above its own baseline
        let surd_bottom = v.y_min_em * scale; // glyph bottom (≤ 0)
        let surd_width = (v.h_adv_em * scale).max(0.4 * size);
        RadicalGeom {
            thickness,
            shift_up,
            surd_width,
            box_height: surd_top,
            box_depth: -surd_bottom,
            // Place the box so the (scaled) glyph top lands exactly at `shift_up`.
            surd_dy: shift_up - surd_top,
            surd_target: Some(min_height),
            synthetic_height: min_height,
        }
    } else if let Some(asm) = glyph_variants::vertical_assembly('\u{221A}', min_height) {
        // Too tall for any discrete variant: build the surd from real font
        // assembly parts (checkmark + stretchy extender + top), like MathJax.
        // The renderer fills the box [top = vinculum, bottom = radicand foot]
        // using the same assembly, so layout only needs the box extent. We keep
        // the same box shape as the synthetic fallback: all ink above the box
        // baseline, height = min_height, depth = 0.
        let surd_width = (asm.width_em * size).max(0.4 * size);
        RadicalGeom {
            thickness,
            shift_up,
            surd_width,
            box_height: min_height,
            box_depth: 0.0,
            surd_dy: shift_up - min_height,
            surd_target: Some(min_height),
            synthetic_height: min_height,
        }
    } else {
        // Fallback: synthetic path sized to the exact required height.
        RadicalGeom {
            thickness,
            shift_up,
            surd_width: 0.833 * size,
            box_height: min_height,
            box_depth: 0.0,
            surd_dy: shift_up - min_height,
            surd_target: None,
            synthetic_height: min_height,
        }
    }
}

/// TeXBook Rule 11: square root.
pub(super) fn layout_sqrt(
    radicand: &MathNode,
    index: Option<&MathNode>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let cramped_ctx = ctx.with_style(ctx.style.cramped());
    let inner = layout_node(radicand, &cramped_ctx);

    let g = radical_geometry(
        inner.height,
        inner.depth,
        size,
        ctx.params.rule_thickness,
        ctx.params.x_height,
        ctx.style.is_display(),
    );
    let radical_thickness = g.thickness;
    let radical_width = g.surd_width;

    // Radical sign: prefer the real font surd glyph (MATH-table variant), so it
    // scales optically to the radicand; fall back to the synthetic path.
    let radical = if let Some(target) = g.surd_target {
        LayoutBox {
            width: radical_width,
            height: g.box_height,
            depth: g.box_depth,
            kind: BoxKind::Delimiter {
                ch: '\u{221A}',
                target_height: target,
            },
        }
    } else {
        LayoutBox {
            width: radical_width,
            height: g.synthetic_height,
            depth: 0.0,
            kind: BoxKind::SvgPath(radical_path(
                radical_width,
                g.synthetic_height,
                radical_thickness,
            )),
        }
    };

    // Vinculum (horizontal rule on top) — match radical_thickness for seamless junction
    let vinculum = LayoutBox::rule(inner.width, radical_thickness);

    let shift_up = g.shift_up;

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_depth = inner.depth;
    let mut children = vec![
        PositionedBox::new(0.0, g.surd_dy, radical),
        PositionedBox::new(radical_width, shift_up, vinculum),
        PositionedBox::new(radical_width, 0.0, inner),
    ];

    let mut total_width = radical_width + inner_width;
    let extra_ascender = math_table::math_constants()
        .map(|c| c.radical_extra_ascender * size)
        .unwrap_or(0.0);
    let total_height = shift_up + radical_thickness + extra_ascender;
    let total_depth = inner_depth.max(g.box_depth - g.surd_dy);

    // Optional root index
    if let Some(idx) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box = layout_node(idx, &idx_ctx);
        // Use MATH table radical_degree_bottom_raise_percent (default 60%)
        let raise_pct = math_table::math_constants()
            .map(|c| c.radical_degree_bottom_raise_percent as f64 / 100.0)
            .unwrap_or(0.6);
        // TeX `\r@@t`: raise the index's *baseline* by 0.6·(height − depth) of
        // the radical body, so it rides high in the surd's crook.
        let idx_shift = (raise_pct * (total_height - total_depth)).max(0.0);
        let idx_width = idx_box.width;

        // Use MATH table kern values for degree positioning (OpenType MATH spec):
        // `\mkern5mu` before, `\mkern-10mu` after. The negative `kern_after`
        // tucks the index into the surd glyph's notch — correct now that the
        // surd is a real font glyph (variant or assembly) rather than a
        // synthetic path.
        let kern_before = math_table::math_constants()
            .map(|c| c.radical_kern_before_degree * size)
            .unwrap_or(0.2778 * size);
        let kern_after = math_table::math_constants()
            .map(|c| c.radical_kern_after_degree * size)
            .unwrap_or(-0.5556 * size);

        let degree_x = kern_before;
        let radical_start = (degree_x + idx_width + kern_after).max(0.0);

        // Place degree
        children.push(PositionedBox::new(degree_x, idx_shift, idx_box));

        // Shift the radical (surd, vinculum, radicand) right to make room.
        for child in &mut children[..3] {
            child.dx += radical_start;
        }
        total_width = (radical_start + radical_width + inner_width).max(degree_x + idx_width);
    }

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

// === Remaining node layouts ===

pub(super) fn layout_sqrt_arena(
    ast: &AstArena,
    radicand: NodeId,
    index: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let cramped_ctx = ctx.with_style(ctx.style.cramped());
    let inner_id = layout_node_arena(ast, radicand, arena, &cramped_ctx);

    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let g = radical_geometry(
        inner_h,
        inner_d,
        size,
        ctx.params.rule_thickness,
        ctx.params.x_height,
        ctx.style.is_display(),
    );
    let radical_thickness = g.thickness;
    let radical_width = g.surd_width;

    let radical_id = if let Some(target) = g.surd_target {
        arena.alloc(ArenaLayoutBox {
            width: radical_width,
            height: g.box_height,
            depth: g.box_depth,
            kind: ArenaBoxKind::Delimiter {
                ch: '\u{221A}',
                target_height: target,
            },
        })
    } else {
        arena.alloc(ArenaLayoutBox {
            width: radical_width,
            height: g.synthetic_height,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(radical_path(
                radical_width,
                g.synthetic_height,
                radical_thickness,
            )),
        })
    };

    let vinculum_id = arena.alloc(ArenaLayoutBox::rule(inner_w, radical_thickness));

    let shift_up = g.shift_up;

    // Build children: radical, vinculum, inner
    // We may need to adjust dx values if there's an index, so we track the span start
    // and positions separately.
    let mut radical_dx = 0.0_f64;
    let mut vinculum_dx = radical_width;
    let mut inner_dx = radical_width;
    let mut total_width = radical_width + inner_w;
    let extra_ascender = math_table::math_constants()
        .map(|c| c.radical_extra_ascender * size)
        .unwrap_or(0.0);
    let total_height = shift_up + radical_thickness + extra_ascender;
    // Radical surd baseline offset (positions the glyph top at the vinculum).
    let radical_dy = g.surd_dy;
    let total_depth = inner_d.max(g.box_depth - g.surd_dy);

    // Optional root index
    if let Some(idx_id_node) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box_id = layout_node_arena(ast, idx_id_node, arena, &idx_ctx);
        let idx_w = arena.get(idx_box_id).width;
        // Use MATH table radical_degree_bottom_raise_percent (default 60%)
        let raise_pct = math_table::math_constants()
            .map(|c| c.radical_degree_bottom_raise_percent as f64 / 100.0)
            .unwrap_or(0.6);
        // TeX `\r@@t`: raise the index's baseline by 0.6·(height − depth).
        let idx_shift = (raise_pct * (total_height - total_depth)).max(0.0);

        // OpenType MATH kerns (`\mkern5mu` / `\mkern-10mu`): the negative
        // `kern_after` tucks the index into the surd glyph's notch.
        let kern_before = math_table::math_constants()
            .map(|c| c.radical_kern_before_degree * size)
            .unwrap_or(0.2778 * size);
        let kern_after = math_table::math_constants()
            .map(|c| c.radical_kern_after_degree * size)
            .unwrap_or(-0.5556 * size);

        let degree_x = kern_before;
        let radical_start = (degree_x + idx_w + kern_after).max(0.0);

        radical_dx += radical_start;
        vinculum_dx += radical_start;
        inner_dx += radical_start;
        total_width = (radical_start + radical_width + inner_w).max(degree_x + idx_w);

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: radical_dx,
            dy: radical_dy,
            id: radical_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: vinculum_dx,
            dy: shift_up,
            id: vinculum_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: inner_dx,
            dy: 0.0,
            id: inner_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: degree_x,
            dy: idx_shift,
            id: idx_box_id,
        });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: radical_dx,
            dy: radical_dy,
            id: radical_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: vinculum_dx,
            dy: shift_up,
            id: vinculum_id,
        });
        arena.push_child(ArenaPositionedBox {
            dx: inner_dx,
            dy: 0.0,
            id: inner_id,
        });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    }
}
