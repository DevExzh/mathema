//! SVG emission with embedded font glyph paths.
//!
//! Produces standalone SVGs that render without client-side fonts.
//! Glyph outlines are defined once in `<defs>` and referenced via `<use>`.

use std::fmt::Write;

use super::RenderOptions;
use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
use super::emit::{XmlAttrEscape, svg_x_bounds_px, svg_x_bounds_px_arena, write_escaped_char};
use super::fonts_registry::{self, FontId};
use super::layout::{BoxKind, LayoutBox};
use super::subset::GlyphSet;

/// Build the `<defs>` id for an embedded glyph.
///
/// The Math font keeps the historical `g{cp}` ids (so embedded references stay
/// stable); other faces are namespaced as `gf{font}_{cp}` to avoid collisions
/// when the same base codepoint is drawn from multiple faces.
fn glyph_id(font: FontId, cp: u32) -> String {
    if font.is_math() {
        format!("g{cp}")
    } else {
        format!("gf{}_{}", font.index(), cp)
    }
}

// ── Heap-based emission ──────────────────────────────────────────────

/// Emit SVG with embedded font paths from a heap-based layout tree.
pub fn emit_svg_with_fonts(root: &LayoutBox, options: &RenderOptions, glyphs: &GlyphSet) -> String {
    let em = options.font_size_px;
    let (view_x_px, width_px) = svg_x_bounds_px(root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;

    // Small padding so paths are not clipped at the exact bounding box edge
    // (matches the padding in emit.rs).
    let pad = 0.08 * em;

    let mut svg = String::with_capacity(4096);

    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.3}" height="{h:.3}" viewBox="{vx:.3} {vy:.3} {w:.3} {h:.3}" style="vertical-align: -{d:.3}px">"#,
        w = width_px + 2.0 * pad,
        h = height_px + 2.0 * pad,
        vx = view_x_px - pad,
        vy = -pad,
        d = depth_px + pad,
    );

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
            let _ = write!(
                svg,
                r#"<path id="{}" d="{}"/>"#,
                glyph_id(*font, *ch as u32),
                XmlAttrEscape(&outline.path_data),
            );
        }
    }

    // MATH-table size variants (delimiters / large operators), keyed by glyph id.
    let mut var_entries: Vec<_> = glyphs.variants.iter().collect();
    var_entries.sort_by_key(|(id, _)| **id);
    for (id, outline) in var_entries {
        let _ = write!(
            svg,
            r#"<path id="vg{}" d="{}"/>"#,
            *id,
            XmlAttrEscape(&outline.path_data),
        );
    }

    svg.push_str("</defs>");
}

/// Emit a `<use>` referencing an embedded MATH-table variant glyph outline.
///
/// Matches `raster::draw_glyph_variant`: the variant outline (y-negated font
/// units) is scaled by `font_size_px / units_per_em` and placed at the baseline
/// `(x, y)`, so the standalone SVG and the PNG draw the identical glyph.
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
    let _ = write!(
        svg,
        r##"<use href="#vg{id}" transform="translate({x:.3},{y:.3}) scale({s:.6})" fill="{color}"/>"##,
        id = glyph_id,
        x = x,
        y = y,
        s = scale,
        color = XmlAttrEscape(color),
    );
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
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" fill="{color}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = th_px,
                color = XmlAttrEscape(&options.fg_color),
            );
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
            // Use SVG path for tall delimiters
            if let Some(dp) = super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0) {
                let tx = x;
                let ty = y - layout.height * em;
                // "F:" prefix indicates a filled path (parentheses, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    let _ = write!(
                        svg,
                        r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" fill="{color}" stroke="none"/></g>"#,
                        tx = tx,
                        ty = ty,
                        s = em,
                        path = XmlAttrEscape(fill_path),
                        color = XmlAttrEscape(&options.fg_color),
                    );
                } else {
                    let _ = write!(
                        svg,
                        r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="{sw:.4}" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>"#,
                        tx = tx,
                        ty = ty,
                        s = em,
                        path = XmlAttrEscape(&dp.path_data),
                        color = XmlAttrEscape(&options.fg_color),
                        sw = dp.stroke_width,
                    );
                }
            } else {
                // Fallback to glyph rendering
                emit_glyph(svg, *ch, *target_height, x, y, em, options, glyphs, false, FontId::Math);
            }
        },

        BoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            // "F:" prefix indicates a filled path (KaTeX-style accents, arrows)
            if let Some(fill_path) = path.strip_prefix("F:") {
                let _ = write!(
                    svg,
                    r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" fill="{color}" stroke="none"/></g>"#,
                    tx = tx,
                    ty = ty,
                    s = em,
                    path = XmlAttrEscape(fill_path),
                    color = XmlAttrEscape(&options.fg_color),
                );
            } else {
                let _ = write!(
                    svg,
                    r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="0.04" fill="none"/></g>"#,
                    tx = tx,
                    ty = ty,
                    s = em,
                    path = XmlAttrEscape(path),
                    color = XmlAttrEscape(&options.fg_color),
                );
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
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{stroke}" fill="{fill}" stroke-width="{sw:.3}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                stroke = XmlAttrEscape(stroke_color),
                fill = XmlAttrEscape(fill_color),
                sw = sw,
            );
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
            let _ = write!(
                svg,
                r##"<use href="#{id}" transform="translate({x:.3},{y:.3}) scale({s:.6})" fill="{color}"/>"##,
                id = glyph_id(used_font, ch as u32),
                x = x,
                y = y,
                s = scale,
                color = XmlAttrEscape(&options.fg_color),
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
        let _ = write!(
            svg,
            r##"<use href="#{id}" transform="translate({x:.3},{y:.3}) scale({s:.6})" fill="{color}"/>"##,
            id = glyph_id(used_font, italic_ch as u32),
            x = x,
            y = y,
            s = scale,
            color = XmlAttrEscape(&options.fg_color),
        );
        return;
    }
    if let Some((_, used_font, info)) = glyphs.outline(FontId::Math, ch) {
        let scale = size * em / info.units_per_em;
        if italic {
            // Apply synthetic italic via skewX(-12) to the glyph outline
            let _ = write!(
                svg,
                r##"<use href="#{id}" transform="translate({x:.3},{y:.3}) scale({s:.6}) skewX(-12)" fill="{color}"/>"##,
                id = glyph_id(used_font, ch as u32),
                x = x,
                y = y,
                s = scale,
                color = XmlAttrEscape(&options.fg_color),
            );
        } else {
            let _ = write!(
                svg,
                r##"<use href="#{id}" transform="translate({x:.3},{y:.3}) scale({s:.6})" fill="{color}"/>"##,
                id = glyph_id(used_font, ch as u32),
                x = x,
                y = y,
                s = scale,
                color = XmlAttrEscape(&options.fg_color),
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
    if italic {
        let _ = write!(
            svg,
            r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" font-style="italic" fill="{color}">"#,
            x = x,
            y = y,
            fs = font_size,
            ff = XmlAttrEscape(&options.font_family),
            color = XmlAttrEscape(&options.fg_color),
        );
    } else {
        let _ = write!(
            svg,
            r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" fill="{color}">"#,
            x = x,
            y = y,
            fs = font_size,
            ff = XmlAttrEscape(&options.font_family),
            color = XmlAttrEscape(&options.fg_color),
        );
    }
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

    let mut svg = String::with_capacity(4096);

    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.3}" height="{h:.3}" viewBox="{vx:.3} {vy:.3} {w:.3} {h:.3}" style="vertical-align: -{d:.3}px">"#,
        w = width_px + 2.0 * pad,
        h = height_px + 2.0 * pad,
        vx = view_x_px - pad,
        vy = -pad,
        d = depth_px + pad,
    );

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
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" fill="{color}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = th_px,
                color = XmlAttrEscape(&options.fg_color),
            );
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
            // Use SVG path for tall delimiters
            if let Some(dp) = super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0) {
                let tx = x;
                let ty = y - layout.height * em;
                // "F:" prefix indicates a filled path (parentheses, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    let _ = write!(
                        svg,
                        r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" fill="{color}" stroke="none"/></g>"#,
                        tx = tx,
                        ty = ty,
                        s = em,
                        path = XmlAttrEscape(fill_path),
                        color = XmlAttrEscape(&options.fg_color),
                    );
                } else {
                    let _ = write!(
                        svg,
                        r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="{sw:.4}" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>"#,
                        tx = tx,
                        ty = ty,
                        s = em,
                        path = XmlAttrEscape(&dp.path_data),
                        color = XmlAttrEscape(&options.fg_color),
                        sw = dp.stroke_width,
                    );
                }
            } else {
                // Fallback to glyph rendering
                emit_glyph(svg, *ch, *target_height, x, y, em, options, glyphs, false, FontId::Math);
            }
        },

        ArenaBoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            // "F:" prefix indicates a filled path (KaTeX-style accents, arrows)
            if let Some(fill_path) = path.strip_prefix("F:") {
                let _ = write!(
                    svg,
                    r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" fill="{color}" stroke="none"/></g>"#,
                    tx = tx,
                    ty = ty,
                    s = em,
                    path = XmlAttrEscape(fill_path),
                    color = XmlAttrEscape(&options.fg_color),
                );
            } else {
                let _ = write!(
                    svg,
                    r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="0.04" fill="none"/></g>"#,
                    tx = tx,
                    ty = ty,
                    s = em,
                    path = XmlAttrEscape(path),
                    color = XmlAttrEscape(&options.fg_color),
                );
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
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{stroke}" fill="{fill}" stroke-width="{sw:.3}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                stroke = XmlAttrEscape(stroke_color),
                fill = XmlAttrEscape(fill_color),
                sw = sw,
            );
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
        use crate::render::layout::{LayoutBuilder, layout_expr_c};
        use crate::render::subset::collect_chars_arena;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2 + 1", &config).unwrap();
        let options = RenderOptions::default();

        // Heap path
        let heap_root = layout_expr_c(&result.expr, MathStyle::Text, &options);
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
