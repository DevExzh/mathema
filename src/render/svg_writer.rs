//! SVG emission with embedded font glyph paths.
//!
//! Produces standalone SVGs that render without client-side fonts.
//! Glyph outlines are defined once in `<defs>` and referenced via `<use>`.

use super::RenderOptions;
use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
use super::emit::{
    push_f3, push_u64, push_xml_escaped, svg_x_bounds_px, svg_x_bounds_px_arena, write_escaped_char,
};
use super::fonts_registry::{self, FontId};
use super::layout::{BoxKind, LayoutBox};
use super::subset::GlyphSet;

/// Write the `<defs>` id for an embedded glyph directly into `out`.
///
/// The Math font keeps the historical `g{cp}` ids (so embedded references stay
/// stable); other faces are namespaced as `gf{font}_{cp}` to avoid collisions
/// when the same base codepoint is drawn from multiple faces. Streaming into the
/// buffer avoids the per-glyph `format!` allocation the previous emitter used.
#[inline]
fn push_glyph_id(out: &mut String, font: FontId, cp: u32) {
    if font.is_math() {
        out.push('g');
        push_u64(out, u64::from(cp));
    } else {
        out.push_str("gf");
        push_u64(out, font.index() as u64);
        out.push('_');
        push_u64(out, u64::from(cp));
    }
}

/// Write a float with exactly `places` decimal digits into `out`, avoiding the
/// `std::fmt` machinery. Mirrors [`emit::push_f3`] (3-5x faster than `write!`)
/// but for the higher precisions the embedded path needs (`.4` stroke widths,
/// `.6` glyph scales). Output is byte-identical to the previous
/// `write!(out, "{v:.N}")` for the finite values produced here.
#[inline]
fn push_fixed(out: &mut String, v: f64, places: u32) {
    if !v.is_finite() {
        out.push_str("0.");
        for _ in 0..places {
            out.push('0');
        }
        return;
    }

    let v = if v < 0.0 {
        out.push('-');
        -v
    } else {
        v
    };

    let pow = 10u64.pow(places);
    let scaled = (v * pow as f64).round().min(u64::MAX as f64) as u64;
    let integer_part = scaled / pow;
    let frac_part = scaled % pow;

    push_u64(out, integer_part);
    out.push('.');

    // Emit the fractional part zero-padded to exactly `places` digits.
    let mut buf = [b'0'; 20];
    let n_digits = places as usize;
    let mut n = frac_part;
    let mut i = n_digits;
    while i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    // SAFETY: buf[..n_digits] contains only ASCII digits.
    out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..n_digits]) });
}

// ── Heap-based emission ──────────────────────────────────────────────

/// Stream the standard `<svg …>` opening tag using the fast float formatter.
///
/// Byte-identical to the previous `write!(… "{:.3}" …)` header. Shared by the
/// heap and arena entry points.
#[inline]
fn push_svg_open(
    svg: &mut String,
    view_x_px: f64,
    width_px: f64,
    height_px: f64,
    depth_px: f64,
    pad: f64,
) {
    let w = width_px + 2.0 * pad;
    let h = height_px + 2.0 * pad;
    svg.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width=""#);
    push_f3(svg, w);
    svg.push_str(r#"" height=""#);
    push_f3(svg, h);
    svg.push_str(r#"" viewBox=""#);
    push_f3(svg, view_x_px - pad);
    svg.push(' ');
    push_f3(svg, -pad);
    svg.push(' ');
    push_f3(svg, w);
    svg.push(' ');
    push_f3(svg, h);
    svg.push_str(r#"" style="vertical-align: -"#);
    push_f3(svg, depth_px + pad);
    svg.push_str(r#"px">"#);
}

/// Estimate the output buffer size from the glyph defs, with a 4096-byte floor.
///
/// The `<defs>` path strings dominate the byte count and are cheap to sum, so a
/// single pass over them gives a good initial allocation and avoids repeated
/// `String` regrowth on larger formulas.
fn estimate_capacity(glyphs: &GlyphSet) -> usize {
    let mut cap = 256usize; // header + closing tag
    for fg in glyphs.fonts.values() {
        for outline in fg.glyphs.values() {
            // `<path id="…" d="…"/>` plus a reference (`<use …/>`) per glyph.
            cap += outline.path_data.len() + 96;
        }
    }
    for outline in glyphs.variants.values() {
        cap += outline.path_data.len() + 96;
    }
    cap.max(4096)
}

/// Emit SVG with embedded font paths from a heap-based layout tree.
pub fn emit_svg_with_fonts(root: &LayoutBox, options: &RenderOptions, glyphs: &GlyphSet) -> String {
    let em = options.font_size_px;
    let (view_x_px, width_px) = svg_x_bounds_px(root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;

    // Small padding so paths are not clipped at the exact bounding box edge
    // (matches the padding in emit.rs).
    let pad = 0.08 * em;

    let mut svg = String::with_capacity(estimate_capacity(glyphs));

    push_svg_open(&mut svg, view_x_px, width_px, height_px, depth_px, pad);

    emit_defs(&mut svg, glyphs);

    let baseline_y = root.height * em;
    emit_box_embedded(&mut svg, root, 0.0, baseline_y, em, options, glyphs);

    svg.push_str("</svg>");
    svg
}

/// Write `<defs>` block with glyph path definitions, sorted for determinism.
fn emit_defs(svg: &mut String, glyphs: &GlyphSet) {
    let has_glyphs = glyphs.fonts.values().any(|f| !f.glyphs.is_empty());
    if !has_glyphs && glyphs.variants.is_empty() {
        return;
    }

    svg.push_str("<defs>");

    // Per-font glyph outlines, ordered by font index then codepoint.
    let mut font_entries: Vec<_> = glyphs.fonts.iter().collect();
    font_entries.sort_by_key(|(font, _)| font.index());
    for (font, fg) in font_entries {
        let mut entries: Vec<_> = fg.glyphs.iter().collect();
        entries.sort_by_key(|(ch, _)| **ch as u32);
        for (ch, outline) in entries {
            svg.push_str(r#"<path id=""#);
            push_glyph_id(svg, *font, *ch as u32);
            svg.push_str(r#"" d=""#);
            push_xml_escaped(svg, &outline.path_data);
            svg.push_str(r#""/>"#);
        }
    }

    // MATH-table size variants (delimiters / large operators), keyed by glyph id.
    let mut var_entries: Vec<_> = glyphs.variants.iter().collect();
    var_entries.sort_by_key(|(id, _)| **id);
    for (id, outline) in var_entries {
        svg.push_str(r#"<path id="vg"#);
        push_u64(svg, u64::from(*id));
        svg.push_str(r#"" d=""#);
        push_xml_escaped(svg, &outline.path_data);
        svg.push_str(r#""/>"#);
    }

    svg.push_str("</defs>");
}

/// Append the shared `<use>` transform/fill tail.
///
/// Call after the `<use href="#…id` opening has been streamed. `skew` is an
/// optional trailing transform token (e.g. `"skewX(-12)"`). Byte-identical to
/// the previous `transform="translate({x:.3},{y:.3}) scale({s:.6})…"` output.
#[inline]
fn push_use_tail(svg: &mut String, x: f64, y: f64, scale: f64, skew: Option<&str>, color: &str) {
    svg.push_str(r#"" transform="translate("#);
    push_f3(svg, x);
    svg.push(',');
    push_f3(svg, y);
    svg.push_str(r#") scale("#);
    push_fixed(svg, scale, 6);
    svg.push(')');
    if let Some(s) = skew {
        svg.push(' ');
        svg.push_str(s);
    }
    svg.push_str(r#"" fill=""#);
    push_xml_escaped(svg, color);
    svg.push_str(r#""/>"#);
}

/// Stream a `<use href="#…glyph-id…">` reference followed by the transform/fill
/// tail. Convenience wrapper over [`push_glyph_id`] + [`push_use_tail`].
#[inline]
#[allow(clippy::too_many_arguments)]
fn push_use_glyph(
    svg: &mut String,
    font: FontId,
    cp: u32,
    x: f64,
    y: f64,
    scale: f64,
    skew: Option<&str>,
    color: &str,
) {
    svg.push_str(r##"<use href="#"##);
    push_glyph_id(svg, font, cp);
    push_use_tail(svg, x, y, scale, skew, color);
}
/// scale(s)">`. Shared by delimiter and free-path emission.
#[inline]
fn push_g_open(svg: &mut String, tx: f64, ty: f64, scale: f64) {
    svg.push_str(r#"<g transform="translate("#);
    push_f3(svg, tx);
    svg.push(',');
    push_f3(svg, ty);
    svg.push_str(r#") scale("#);
    push_f3(svg, scale);
    svg.push_str(r#")">"#);
}

/// Emit a filled `<path>` inside a transformed group (parens, braces, accents).
#[inline]
fn push_fill_path(svg: &mut String, tx: f64, ty: f64, scale: f64, path: &str, color: &str) {
    push_g_open(svg, tx, ty, scale);
    svg.push_str(r#"<path d=""#);
    push_xml_escaped(svg, path);
    svg.push_str(r#"" fill=""#);
    push_xml_escaped(svg, color);
    svg.push_str(r#"" stroke="none"/></g>"#);
}

/// Emit a stroked delimiter `<path>` (rounded caps, `.4`-precision stroke width).
#[inline]
fn push_stroke_delim_path(
    svg: &mut String,
    tx: f64,
    ty: f64,
    scale: f64,
    path: &str,
    color: &str,
    stroke_width: f64,
) {
    push_g_open(svg, tx, ty, scale);
    svg.push_str(r#"<path d=""#);
    push_xml_escaped(svg, path);
    svg.push_str(r#"" stroke=""#);
    push_xml_escaped(svg, color);
    svg.push_str(r#"" stroke-width=""#);
    push_fixed(svg, stroke_width, 4);
    svg.push_str(r#"" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>"#);
}

/// Emit a stroked free `<path>` (KaTeX-style accents/arrows, fixed `0.04` width).
#[inline]
fn push_stroke_free_path(svg: &mut String, tx: f64, ty: f64, scale: f64, path: &str, color: &str) {
    push_g_open(svg, tx, ty, scale);
    svg.push_str(r#"<path d=""#);
    push_xml_escaped(svg, path);
    svg.push_str(r#"" stroke=""#);
    push_xml_escaped(svg, color);
    svg.push_str(r#"" stroke-width="0.04" fill="none"/></g>"#);
}

/// Emit the border/background `<rect>` for a `\boxed`/`\fbox` (`.3`-precision).
#[inline]
#[allow(clippy::too_many_arguments)]
fn push_boxed_rect(
    svg: &mut String,
    x: f64,
    ry: f64,
    w: f64,
    h: f64,
    stroke: &str,
    fill: &str,
    stroke_width: f64,
) {
    svg.push_str(r#"<rect x=""#);
    push_f3(svg, x);
    svg.push_str(r#"" y=""#);
    push_f3(svg, ry);
    svg.push_str(r#"" width=""#);
    push_f3(svg, w);
    svg.push_str(r#"" height=""#);
    push_f3(svg, h);
    svg.push_str(r#"" stroke=""#);
    push_xml_escaped(svg, stroke);
    svg.push_str(r#"" fill=""#);
    push_xml_escaped(svg, fill);
    svg.push_str(r#"" stroke-width=""#);
    push_f3(svg, stroke_width);
    svg.push_str(r#""/>"#);
}

/// Emit a `<use>` referencing an embedded MATH-table variant glyph outline.
fn emit_variant_glyph(
    svg: &mut String,
    glyph_id: u16,
    font_size_px: f64,
    x: f64,
    y: f64,
    upem: f64,
    color: &str,
) {
    let scale = font_size_px / upem;
    svg.push_str(r##"<use href="#vg"##);
    push_u64(svg, u64::from(glyph_id));
    push_use_tail(svg, x, y, scale, None, color);
}

/// Emit a stretchy vertical delimiter/radical from its OpenType MATH glyph
/// assembly parts (mirrors `raster::draw_vertical_assembly`). Returns `false`
/// (emitting nothing) when no assembly exists, so the caller can fall back.
#[allow(clippy::too_many_arguments)]
fn emit_vertical_assembly_embedded(
    svg: &mut String,
    glyphs: &GlyphSet,
    ch: char,
    fill_height_em: f64,
    box_top: f64,
    x: f64,
    em: f64,
    upem: f64,
    color: &str,
) -> bool {
    let Some(asm) = super::glyph_variants::vertical_assembly(ch, fill_height_em) else {
        return false;
    };
    let h_nat = asm.ink_top_em - asm.ink_bottom_em;
    if h_nat <= 0.001 {
        return false;
    }
    // Bail if any part outline is missing from the embedded set (keeps SVG and
    // PNG identical: the PNG path would also have nothing to draw).
    if asm
        .placements
        .iter()
        .any(|p| !glyphs.variants.contains_key(&p.glyph_id))
    {
        return false;
    }
    let k = fill_height_em / h_nat;
    let fs = em * k;
    for p in &asm.placements {
        let baseline_y = box_top + (asm.ink_top_em - p.shift_em) * fs;
        emit_variant_glyph(svg, p.glyph_id, fs, x, baseline_y, upem, color);
    }
    true
}

fn emit_box_embedded(
    svg: &mut String,
    layout: &LayoutBox,
    x: f64,
    y: f64,
    em: f64,
    options: &RenderOptions,
    glyphs: &GlyphSet,
) {
    match &layout.kind {
        BoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => {
            // Display-size large operators (∑, ∫, …) use a designed MATH-table
            // variant glyph when available, matching the raster backend.
            if *size > 1.2
                && super::glyph_variants::is_large_op_char(*ch)
                && let Some(v) = super::glyph_variants::display_operator_variant(*ch)
                && glyphs.variants.contains_key(&v.glyph_id)
            {
                let target_px = em * size;
                let fs = if v.advance_em > 0.01 {
                    target_px / v.advance_em
                } else {
                    target_px
                };
                emit_variant_glyph(
                    svg,
                    v.glyph_id,
                    fs,
                    x,
                    y,
                    glyphs.font_info.units_per_em,
                    &options.fg_color,
                );
                return;
            }
            emit_glyph(svg, *ch, *size, x, y, em, options, glyphs, *italic, *font);
        },

        BoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => {
            // For font-embed, we still need per-character glyph paths
            let font = fonts_registry::font_for_text_run(*text_style);
            let mut cx = x;
            for ch in text.chars() {
                emit_glyph(svg, ch, *size, cx, y, em, options, glyphs, *italic, font);
                // Use the same metric lookup as layout_text_run to keep
                // glyph positions consistent with the layout pass.
                let m = fonts_registry::text_metrics(ch, *italic, font);
                cx += m.width * size * em;
            }
        },

        BoxKind::HBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
            }
        },

        BoxKind::VBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
            }
        },

        BoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            let ry = y - layout.height * em;
            svg.push_str(r#"<rect x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, ry);
            svg.push_str(r#"" width=""#);
            push_f3(svg, w_px);
            svg.push_str(r#"" height=""#);
            push_f3(svg, th_px);
            svg.push_str(r#"" fill=""#);
            push_xml_escaped(svg, &options.fg_color);
            svg.push_str(r#""/>"#);
        },

        BoxKind::Kern => {},

        BoxKind::Delimiter { ch, target_height } => {
            // Prefer a MATH-table variant glyph (matches the raster backend).
            if let Some(v) = super::glyph_variants::delimiter_variant(*ch, *target_height)
                && glyphs.variants.contains_key(&v.glyph_id)
            {
                let target_px = target_height * em;
                let fs = if v.advance_em > 0.01 {
                    target_px / v.advance_em
                } else {
                    em
                };
                emit_variant_glyph(
                    svg,
                    v.glyph_id,
                    fs,
                    x,
                    y,
                    glyphs.font_info.units_per_em,
                    &options.fg_color,
                );
                return;
            }
            // Too tall for a discrete variant: build from the font's stretchy
            // assembly parts before falling back to a synthetic path.
            if emit_vertical_assembly_embedded(
                svg,
                glyphs,
                *ch,
                layout.height + layout.depth,
                y - layout.height * em,
                x,
                em,
                glyphs.font_info.units_per_em,
                &options.fg_color,
            ) {
                return;
            }
            // Use SVG path for tall delimiters
            if let Some(dp) = super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0) {
                let tx = x;
                let ty = y - layout.height * em;
                // "F:" prefix indicates a filled path (parentheses, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    push_fill_path(svg, tx, ty, em, fill_path, &options.fg_color);
                } else {
                    push_stroke_delim_path(
                        svg,
                        tx,
                        ty,
                        em,
                        &dp.path_data,
                        &options.fg_color,
                        dp.stroke_width,
                    );
                }
            } else {
                // Fallback to glyph rendering
                emit_glyph(
                    svg,
                    *ch,
                    *target_height,
                    x,
                    y,
                    em,
                    options,
                    glyphs,
                    false,
                    FontId::Math,
                );
            }
        },

        BoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            // "F:" prefix indicates a filled path (KaTeX-style accents, arrows)
            if let Some(fill_path) = path.strip_prefix("F:") {
                push_fill_path(svg, tx, ty, em, fill_path, &options.fg_color);
            } else {
                push_stroke_free_path(svg, tx, ty, em, path, &options.fg_color);
            }
        },

        BoxKind::Color { color, child } => {
            let mut color_opts = options.clone();
            color_opts.fg_color = color.clone();
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box_embedded(svg, &child.layout, cx, cy, em, &color_opts, glyphs);
        },

        BoxKind::Phantom => {},

        BoxKind::Boxed {
            child,
            padding: _,
            bg_color,
            border_color,
        } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;
            let stroke_color = border_color.as_deref().unwrap_or(&options.fg_color);
            let fill_color = bg_color.as_deref().unwrap_or("none");
            let sw = 0.04 * em; // proportional stroke width
            push_boxed_rect(svg, x, ry, w_px, h_px, stroke_color, fill_color, sw);
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
        },
    }
}

/// Emit a single glyph: `<use>` if outline exists, `<text>` fallback otherwise.
#[allow(clippy::too_many_arguments)]
fn emit_glyph(
    svg: &mut String,
    ch: char,
    size: f64,
    x: f64,
    y: f64,
    em: f64,
    options: &RenderOptions,
    glyphs: &GlyphSet,
    italic: bool,
    font: FontId,
) {
    // Text-face glyphs (e.g. \mathbf, \mathsf, \mathtt) carry their slant/weight
    // in the outline itself: draw the base codepoint with no remap or skew.
    // Falls back to the Math font (handled by `GlyphSet::outline`) when absent.
    if !font.is_math() {
        if let Some((outline, used_font, info)) = glyphs.outline(font, ch) {
            let _ = outline;
            let scale = size * em / info.units_per_em;
            push_use_glyph(
                svg,
                used_font,
                ch as u32,
                x,
                y,
                scale,
                None,
                &options.fg_color,
            );
            return;
        }
        // No outline in the text face nor the Math fallback → <text>.
        emit_text_fallback(svg, ch, size, x, y, em, options, italic);
        return;
    }

    // Math font: try the actual Math Italic glyph first when italic.
    if italic
        && let Some(italic_ch) = super::layout::math_italic_codepoint(ch)
        && let Some((_, used_font, info)) = glyphs.outline(FontId::Math, italic_ch)
    {
        let scale = size * em / info.units_per_em;
        push_use_glyph(
            svg,
            used_font,
            italic_ch as u32,
            x,
            y,
            scale,
            None,
            &options.fg_color,
        );
        return;
    }
    if let Some((_, used_font, info)) = glyphs.outline(FontId::Math, ch) {
        let scale = size * em / info.units_per_em;
        if italic {
            // Apply synthetic italic via skewX(-12) to the glyph outline
            push_use_glyph(
                svg,
                used_font,
                ch as u32,
                x,
                y,
                scale,
                Some("skewX(-12)"),
                &options.fg_color,
            );
        } else if ch == '\u{2202}' {
            // Give the nearly-upright Latin Modern Math `∂` the slight forward
            // italic lean MathJax uses. Mirrors the raster backend's shear so the
            // SVG and PNG outputs stay in parity.
            let skew = format!("skewX({})", super::layout::PARTIAL_DESLANT_DEG);
            push_use_glyph(
                svg,
                used_font,
                ch as u32,
                x,
                y,
                scale,
                Some(&skew),
                &options.fg_color,
            );
        } else {
            push_use_glyph(
                svg,
                used_font,
                ch as u32,
                x,
                y,
                scale,
                None,
                &options.fg_color,
            );
        }
    } else {
        emit_text_fallback(svg, ch, size, x, y, em, options, italic);
    }
}

/// `<text>` fallback when no embedded outline is available.
#[allow(clippy::too_many_arguments)]
fn emit_text_fallback(
    svg: &mut String,
    ch: char,
    size: f64,
    x: f64,
    y: f64,
    em: f64,
    options: &RenderOptions,
    italic: bool,
) {
    let font_size = em * size;
    svg.push_str(r#"<text x=""#);
    push_f3(svg, x);
    svg.push_str(r#"" y=""#);
    push_f3(svg, y);
    svg.push_str(r#"" font-size=""#);
    push_f3(svg, font_size);
    svg.push_str(r#"" font-family=""#);
    push_xml_escaped(svg, &options.font_family);
    if italic {
        svg.push_str(r#"" font-style="italic" fill=""#);
    } else {
        svg.push_str(r#"" fill=""#);
    }
    push_xml_escaped(svg, &options.fg_color);
    svg.push_str(r#"">"#);
    write_escaped_char(svg, ch);
    svg.push_str("</text>");
}

// ── Arena-based emission ─────────────────────────────────────────────

/// Emit SVG with embedded font paths from an arena-based layout tree.
pub fn emit_svg_with_fonts_arena(
    arena: &LayoutArena,
    root: LayoutBoxId,
    options: &RenderOptions,
    glyphs: &GlyphSet,
) -> String {
    let root_box = arena.get(root);
    let em = options.font_size_px;
    let (view_x_px, width_px) = svg_x_bounds_px_arena(arena, root, em);
    let height_px = (root_box.height + root_box.depth) * em;
    let depth_px = root_box.depth * em;

    // Small padding so paths are not clipped at the exact bounding box edge
    // (matches the padding in emit.rs).
    let pad = 0.08 * em;

    let mut svg = String::with_capacity(estimate_capacity(glyphs));

    push_svg_open(&mut svg, view_x_px, width_px, height_px, depth_px, pad);

    emit_defs(&mut svg, glyphs);

    let baseline_y = root_box.height * em;
    emit_box_arena_embedded(&mut svg, arena, root, 0.0, baseline_y, em, options, glyphs);

    svg.push_str("</svg>");
    svg
}

#[allow(clippy::too_many_arguments)]
fn emit_box_arena_embedded(
    svg: &mut String,
    arena: &LayoutArena,
    id: LayoutBoxId,
    x: f64,
    y: f64,
    em: f64,
    options: &RenderOptions,
    glyphs: &GlyphSet,
) {
    let layout = arena.get(id);
    match &layout.kind {
        ArenaBoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => {
            if *size > 1.2
                && super::glyph_variants::is_large_op_char(*ch)
                && let Some(v) = super::glyph_variants::display_operator_variant(*ch)
                && glyphs.variants.contains_key(&v.glyph_id)
            {
                let target_px = em * size;
                let fs = if v.advance_em > 0.01 {
                    target_px / v.advance_em
                } else {
                    target_px
                };
                emit_variant_glyph(
                    svg,
                    v.glyph_id,
                    fs,
                    x,
                    y,
                    glyphs.font_info.units_per_em,
                    &options.fg_color,
                );
                return;
            }
            emit_glyph(svg, *ch, *size, x, y, em, options, glyphs, *italic, *font);
        },

        ArenaBoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => {
            // For font-embed, we still need per-character glyph paths
            let font = fonts_registry::font_for_text_run(*text_style);
            let mut cx = x;
            for ch in text.chars() {
                emit_glyph(svg, ch, *size, cx, y, em, options, glyphs, *italic, font);
                // Use the same metric lookup as layout_text_run to keep
                // glyph positions consistent with the layout pass.
                let m = fonts_registry::text_metrics(ch, *italic, font);
                cx += m.width * size * em;
            }
        },

        ArenaBoxKind::HBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena_embedded(svg, arena, child.id, cx, cy, em, options, glyphs);
            }
        },

        ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena_embedded(svg, arena, child.id, cx, cy, em, options, glyphs);
            }
        },

        ArenaBoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            let ry = y - layout.height * em;
            svg.push_str(r#"<rect x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, ry);
            svg.push_str(r#"" width=""#);
            push_f3(svg, w_px);
            svg.push_str(r#"" height=""#);
            push_f3(svg, th_px);
            svg.push_str(r#"" fill=""#);
            push_xml_escaped(svg, &options.fg_color);
            svg.push_str(r#""/>"#);
        },

        ArenaBoxKind::Kern => {},

        ArenaBoxKind::Delimiter { ch, target_height } => {
            // Prefer a MATH-table variant glyph (matches the raster backend).
            if let Some(v) = super::glyph_variants::delimiter_variant(*ch, *target_height)
                && glyphs.variants.contains_key(&v.glyph_id)
            {
                let target_px = target_height * em;
                let fs = if v.advance_em > 0.01 {
                    target_px / v.advance_em
                } else {
                    em
                };
                emit_variant_glyph(
                    svg,
                    v.glyph_id,
                    fs,
                    x,
                    y,
                    glyphs.font_info.units_per_em,
                    &options.fg_color,
                );
                return;
            }
            // Too tall for a discrete variant: build from the font's stretchy
            // assembly parts before falling back to a synthetic path.
            if emit_vertical_assembly_embedded(
                svg,
                glyphs,
                *ch,
                layout.height + layout.depth,
                y - layout.height * em,
                x,
                em,
                glyphs.font_info.units_per_em,
                &options.fg_color,
            ) {
                return;
            }
            // Use SVG path for tall delimiters
            if let Some(dp) = super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0) {
                let tx = x;
                let ty = y - layout.height * em;
                // "F:" prefix indicates a filled path (parentheses, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    push_fill_path(svg, tx, ty, em, fill_path, &options.fg_color);
                } else {
                    push_stroke_delim_path(
                        svg,
                        tx,
                        ty,
                        em,
                        &dp.path_data,
                        &options.fg_color,
                        dp.stroke_width,
                    );
                }
            } else {
                // Fallback to glyph rendering
                emit_glyph(
                    svg,
                    *ch,
                    *target_height,
                    x,
                    y,
                    em,
                    options,
                    glyphs,
                    false,
                    FontId::Math,
                );
            }
        },

        ArenaBoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            // "F:" prefix indicates a filled path (KaTeX-style accents, arrows)
            if let Some(fill_path) = path.strip_prefix("F:") {
                push_fill_path(svg, tx, ty, em, fill_path, &options.fg_color);
            } else {
                push_stroke_free_path(svg, tx, ty, em, path, &options.fg_color);
            }
        },

        ArenaBoxKind::Color { color, child } => {
            let mut color_opts = options.clone();
            color_opts.fg_color = color.clone();
            emit_box_arena_embedded(svg, arena, *child, x, y, em, &color_opts, glyphs);
        },

        ArenaBoxKind::Phantom => {},

        ArenaBoxKind::Boxed {
            child,
            padding,
            bg_color,
            border_color,
        } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;
            let stroke_color = border_color.as_deref().unwrap_or(&options.fg_color);
            let fill_color = bg_color.as_deref().unwrap_or("none");
            let sw = 0.04 * em; // proportional stroke width
            push_boxed_rect(svg, x, ry, w_px, h_px, stroke_color, fill_color, sw);
            emit_box_arena_embedded(svg, arena, *child, x + padding * em, y, em, options, glyphs);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::render::layout::layout_expr;
    use crate::render::style::MathStyle;
    use crate::render::subset::{build_glyph_set, collect_chars};
    use std::collections::HashSet;

    fn render_embedded(latex_nodes: Vec<MathNode>) -> String {
        let expr = MathExpr::inline(latex_nodes);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let chars = collect_chars(&root);
        let glyphs = build_glyph_set(&chars);
        emit_svg_with_fonts(&root, &options, &glyphs)
    }

    #[test]
    fn svg_has_defs_section() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(svg.contains("<defs>"), "should have <defs> section");
        assert!(svg.contains("</defs>"), "should close </defs>");
    }

    #[test]
    fn svg_has_glyph_path_in_defs() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(
            svg.contains("id=\"g120\""),
            "should define glyph for 'x' (cp 120)"
        );
        assert!(svg.contains("<path"), "should have path element in defs");
    }

    #[test]
    fn svg_uses_use_elements_for_glyphs() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(svg.contains("<use"), "should use <use> to reference glyph");
        // 'x' is italic in math mode, so it uses Math Italic U+1D465 (119909)
        assert!(
            svg.contains("href=\"#g119909\""),
            "should reference Math Italic glyph definition"
        );
    }

    #[test]
    fn svg_has_no_text_elements_when_all_glyphs_found() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(
            !svg.contains("<text"),
            "should not fall back to <text> when glyph is in font"
        );
    }

    #[test]
    fn svg_has_valid_structure() {
        let svg = render_embedded(vec![MathNode::Number("42".into())]);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("xmlns"));
        assert!(svg.contains("viewBox"));
    }

    #[test]
    fn svg_fraction_still_has_rect() {
        let svg = render_embedded(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(
            svg.contains("<rect"),
            "fraction bar should still use <rect>"
        );
    }

    #[test]
    fn svg_deduplicates_glyphs() {
        let svg = render_embedded(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("x".into()),
        ]);
        let defs_start = svg.find("<defs>").unwrap();
        let defs_end = svg.find("</defs>").unwrap();
        let defs_section = &svg[defs_start..defs_end];
        // 'x' is italic in math mode → Math Italic U+1D465 (119909)
        assert_eq!(
            defs_section.matches("g119909").count(),
            1,
            "Math Italic glyph 'x' should be defined exactly once in defs"
        );
        let body = &svg[defs_end..];
        assert_eq!(
            body.matches("#g119909").count(),
            2,
            "Math Italic 'x' should be referenced twice"
        );
    }

    #[test]
    fn svg_uses_correct_fill_color() {
        let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let options = RenderOptions {
            fg_color: "#ff0000".into(),
            ..RenderOptions::default()
        };
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let chars = collect_chars(&root);
        let glyphs = build_glyph_set(&chars);
        let svg = emit_svg_with_fonts(&root, &options, &glyphs);
        assert!(svg.contains("fill=\"#ff0000\""));
    }

    #[test]
    fn svg_use_has_transform_with_scale() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(svg.contains("scale("), "<use> should have scale transform");
        assert!(
            svg.contains("translate("),
            "<use> should have translate transform"
        );
    }

    #[test]
    fn svg_fallback_to_text_for_missing_glyph() {
        let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let empty_chars = HashSet::new();
        let glyphs = build_glyph_set(&empty_chars);
        let svg = emit_svg_with_fonts(&root, &options, &glyphs);
        assert!(
            svg.contains("<text"),
            "should fall back to <text> when glyph not in set"
        );
    }

    #[test]
    fn arena_emit_matches_heap() {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::layout::{LayoutBuilder, layout_expr};
        use crate::render::subset::collect_chars_arena;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2 + 1", &config).unwrap();
        let options = RenderOptions::default();

        // Heap path
        let heap_root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &options);
        let heap_chars = collect_chars(&heap_root);
        let heap_glyphs = build_glyph_set(&heap_chars);
        let heap_svg = emit_svg_with_fonts(&heap_root, &options, &heap_glyphs);

        // Arena path
        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Text, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_chars = collect_chars_arena(&arena, arena_root);
        let arena_glyphs = build_glyph_set(&arena_chars);
        let arena_svg = emit_svg_with_fonts_arena(&arena, arena_root, &options, &arena_glyphs);

        assert_eq!(heap_svg, arena_svg, "arena must match heap");
    }
}
