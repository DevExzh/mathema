/// Direct raster rendering: converts a LayoutBox tree into PNG or JPEG image bytes.
///
/// Pipeline: MathExpr → LayoutBox tree → tiny_skia::Pixmap → PNG/JPEG bytes.
/// Glyphs are rasterized with `fontdue` using an embedded Latin Modern Math font.
use std::collections::HashMap;
use std::sync::LazyLock;

use fontdue::{Font, FontSettings};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

use super::RenderOptions;
use super::fonts_registry::{self, FontId};
use super::layout::{BoxKind, LayoutBox};
use super::style::MathStyle;
use crate::ast::MathExpr;
use crate::ast::compact::CompactExpr;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Embedded font (Latin Modern Math, GUST Font License)
// ---------------------------------------------------------------------------

static MATH_FONT: LazyLock<Font> = LazyLock::new(|| {
    Font::from_bytes(super::FONT_DATA, FontSettings::default())
        .expect("embedded font must be valid")
});

/// Per-id lazily-initialized `fontdue` fonts for the registered text faces.
/// `FontId::Math` is served by [`MATH_FONT`]; text faces are parsed on first use.
static FONTDUE_CELLS: [std::sync::OnceLock<Font>; 13] = [const { std::sync::OnceLock::new() }; 13];

/// `fontdue` font for a font identity (used to rasterize text-face glyphs).
fn fontdue_font(id: FontId) -> &'static Font {
    if id.is_math() {
        return &MATH_FONT;
    }
    FONTDUE_CELLS[id.index()].get_or_init(|| {
        Font::from_bytes(fonts_registry::font_bytes(id), FontSettings::default())
            .expect("embedded font must be valid")
    })
}

/// Lazily-parsed ttf-parser face for MATH-table variant outline extraction.
static TTF_FACE: LazyLock<ttf_parser::Face<'static>> = LazyLock::new(|| {
    ttf_parser::Face::parse(super::FONT_DATA, 0).expect("embedded font data must be valid")
});

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Supported raster image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

/// Options for rasterized image output.
#[derive(Debug, Clone)]
pub struct RasterOptions {
    /// SVG/layout rendering options (font size, display mode, colors).
    pub render: RenderOptions,
    /// Output image format.
    pub format: ImageFormat,
    /// DPI scale factor. 1.0 = 96 DPI, 2.0 = Retina/192 DPI.
    pub scale: f64,
    /// Background color as RGBA. `None` = transparent (PNG) or white (JPEG).
    pub background: Option<[u8; 4]>,
    /// JPEG quality (1–100). Ignored for PNG.
    pub quality: u8,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            render: RenderOptions::default(),
            format: ImageFormat::Png,
            scale: 1.0,
            background: None,
            quality: 90,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render a math expression directly to raster image bytes (PNG or JPEG).
pub fn render_to_image(expr: &MathExpr, options: &RasterOptions) -> Result<Vec<u8>> {
    crate::ast::validate_ast_depth(expr)?;
    let style = if options.render.is_display(expr) {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let root = super::layout::layout_expr(expr, style, &options.render);
    rasterize_layout(&root, options)
}

/// Render a compact math expression directly to raster image bytes (PNG or JPEG).
pub fn render_to_image_compact(expr: &CompactExpr, options: &RasterOptions) -> Result<Vec<u8>> {
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .render
        .display_mode
        .unwrap_or(expr.display == crate::ast::DisplayStyle::Block)
    {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let root = super::layout::layout_expr_c(expr, style, &options.render);
    rasterize_layout(&root, options)
}

/// Recursively compute the horizontal extent of all visible content in a layout
/// tree.  Returns `(min_x, max_x)` in **em units** relative to the tree root's
/// origin.  Invisible nodes (`Kern`, `Phantom`) are skipped.
#[cfg(test)]
fn compute_x_bounds(layout: &LayoutBox, x: f64) -> (f64, f64) {
    match &layout.kind {
        // Visible leaf nodes: occupy [x, x + width].
        BoxKind::Glyph { .. }
        | BoxKind::TextRun { .. }
        | BoxKind::Rule { .. }
        | BoxKind::Delimiter { .. }
        | BoxKind::SvgPath(_) => (x, x + layout.width),

        // Container nodes: recurse into children.
        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for child in children {
                let (cmin, cmax) = compute_x_bounds(&child.layout, x + child.dx);
                if cmin < lo {
                    lo = cmin;
                }
                if cmax > hi {
                    hi = cmax;
                }
            }
            // If there are no children, fall back to the box's own extent.
            if lo > hi {
                (x, x + layout.width)
            } else {
                (lo, hi)
            }
        },

        // Color wrapper: recurse into the single child.
        BoxKind::Color { child, .. } => compute_x_bounds(&child.layout, x + child.dx),

        // Boxed wrapper: recurse into the single child but also account for the
        // box border itself.
        BoxKind::Boxed { child, .. } => {
            let (cmin, cmax) = compute_x_bounds(&child.layout, x + child.dx);
            (cmin.min(x), cmax.max(x + layout.width))
        },

        // Invisible nodes contribute no visible content.
        BoxKind::Kern | BoxKind::Phantom => (f64::INFINITY, f64::NEG_INFINITY),
    }
}

fn rasterize_layout(root: &LayoutBox, options: &RasterOptions) -> Result<Vec<u8>> {
    let em = options.render.font_size_px * options.scale;
    let padding = 2.0 * options.scale; // 2px padding at 1x

    // Guard against excessive dimensions that could cause OOM
    const MAX_PIXMAP_DIMENSION: u32 = 8192;

    // Use the same horizontal bounds policy as SVG so leading negative space
    // does not clip content and trailing layout width remains represented.
    let (view_x_px, content_width_px) = super::emit::svg_x_bounds_px(root, em);
    let x_shift_px = if view_x_px < 0.0 { -view_x_px } else { 0.0 };

    let width = (content_width_px + padding * 2.0).ceil().max(1.0) as u32;
    let height = ((root.height + root.depth) * em + padding * 2.0)
        .ceil()
        .max(1.0) as u32;

    if width > MAX_PIXMAP_DIMENSION || height > MAX_PIXMAP_DIMENSION {
        return Err(crate::error::Error::Render {
            message: format!(
                "image dimensions {width}x{height} exceed maximum of {MAX_PIXMAP_DIMENSION}x{MAX_PIXMAP_DIMENSION}"
            ),
        });
    }

    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| crate::error::Error::Render {
        message: format!("failed to create {width}x{height} pixmap"),
    })?;

    // Fill background
    let bg = match options.background {
        Some(rgba) => Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]),
        None => match options.format {
            ImageFormat::Png => Color::from_rgba8(0, 0, 0, 0), // transparent
            ImageFormat::Jpeg => Color::from_rgba8(255, 255, 255, 255), // white
        },
    };
    pixmap.fill(bg);

    let fg = parse_color(&options.render.fg_color);

    let mut ctx = RasterCtx {
        em,
        fg,
        cache: HashMap::new(),
        glyph_rgba_buf: Vec::new(),
    };

    // Shift content right by |view_x_px| so negatively-positioned glyphs are inside the pixmap.
    let baseline_y = root.height * em + padding;
    emit_raster_box(
        &mut pixmap,
        &mut ctx,
        root,
        padding + x_shift_px,
        baseline_y,
    );

    encode_image(&pixmap, options)
}

// ---------------------------------------------------------------------------
// Recursive tree walker
// ---------------------------------------------------------------------------

struct RasterCtx {
    em: f64,
    fg: [u8; 3],
    cache: HashMap<(FontId, char, u32), (fontdue::Metrics, Vec<u8>)>,
    /// Reusable RGBA buffer for glyph compositing (avoids per-glyph Pixmap allocation).
    glyph_rgba_buf: Vec<u8>,
}

fn emit_raster_box(
    pixmap: &mut Pixmap,
    ctx: &mut RasterCtx,
    layout: &LayoutBox,
    x: f64,
    y: f64, // baseline position (y-down)
) {
    let em = ctx.em;
    let fg = ctx.fg;
    match &layout.kind {
        BoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => {
            // For large operators at display size, use MATH table variant glyphs
            // for better quality (properly designed display glyphs vs scaled base).
            let used_variant = if *size > 1.2 && is_large_op_char(*ch) {
                if let Some(variant) = super::glyph_variants::display_operator_variant(*ch) {
                    let target_px = em * size;
                    let font_size_px = if variant.advance_em > 0.01 {
                        target_px / variant.advance_em
                    } else {
                        target_px
                    };
                    draw_glyph_variant(pixmap, fg, variant.glyph_id, font_size_px, x, y);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !used_variant {
                let font_size_px = (em * size).max(1.0) as f32;
                draw_glyph(pixmap, ctx, *ch, font_size_px, x, y, *italic, *font);
            }
        },

        BoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => {
            let font = fonts_registry::font_for_text_run(*text_style);
            let font_size_px = (em * size).max(1.0) as f32;
            let mut cx = x;
            for ch in text.chars() {
                // Use the same metric lookup as layout_text_run to keep
                // glyph positions consistent with the layout pass.
                let m = fonts_registry::text_metrics(ch, *italic, font);
                draw_glyph(pixmap, ctx, ch, font_size_px, cx, y, *italic, font);
                cx += m.width * size * em;
            }
        },

        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em; // dy positive = up in TeX, subtract for y-down
                emit_raster_box(pixmap, ctx, &child.layout, cx, cy);
            }
        },

        BoxKind::Rule { thickness } => {
            let th_px = (thickness * em) as f32;
            let w_px = (layout.width * em) as f32;
            let ry = (y - layout.height * em) as f32;
            // Guard against degenerate rects.
            let rw = w_px.max(0.5);
            let rh = th_px.max(0.5);
            if rw.is_finite()
                && rh.is_finite()
                && ry.is_finite()
                && (x as f32).is_finite()
                && rw > 0.0
                && rh > 0.0
            {
                // Use fill_path instead of fill_rect to get proper anti-aliasing
                // without triggering tiny-skia's hairline_aa assertion on thin rects.
                let mut pb = PathBuilder::new();
                let rx = x as f32;
                pb.move_to(rx, ry);
                pb.line_to(rx + rw, ry);
                pb.line_to(rx + rw, ry + rh);
                pb.line_to(rx, ry + rh);
                pb.close();
                if let Some(path) = pb.finish() {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                    paint.anti_alias = true;
                    pixmap.fill_path(
                        &path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        },

        BoxKind::Kern | BoxKind::Phantom => {
            // Invisible — no output
        },

        BoxKind::Delimiter { ch, target_height } => {
            // Try MATH table glyph variant first for better quality.
            let variant_rendered = if let Some(variant) =
                super::glyph_variants::delimiter_variant(*ch, *target_height)
            {
                // Render the variant glyph scaled to match the layout's target height.
                // font_size_px = target_height_pixels / advance_em
                let target_px = target_height * em;
                let font_size_px = if variant.advance_em > 0.01 {
                    target_px / variant.advance_em
                } else {
                    em
                };
                draw_glyph_variant(pixmap, fg, variant.glyph_id, font_size_px, x, y);
                true
            } else {
                false
            };

            if !variant_rendered {
                // Too tall for a discrete variant: try the font's stretchy
                // assembly (real parts: hooks + extenders) before the synthetic
                // path, so tall surds/braces stay proportional like MathJax.
                let box_top = y - layout.height * em;
                let fill_h = layout.height + layout.depth;
                let assembled = draw_vertical_assembly(pixmap, fg, *ch, fill_h, box_top, x, em);
                if assembled {
                    // rendered from assembly parts
                } else if let Some(dp) =
                    super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0)
                {
                    if let Some((path, is_fill)) = parse_svg_path_ex(&dp.path_data) {
                        let tx = x as f32;
                        let ty = (y - layout.height * em) as f32;
                        let scale = em as f32;

                        let mut paint = Paint::default();
                        paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                        paint.anti_alias = true;

                        let transform = Transform::from_translate(tx, ty).pre_scale(scale, scale);
                        if is_fill {
                            pixmap.fill_path(
                                &path,
                                &paint,
                                tiny_skia::FillRule::Winding,
                                transform,
                                None,
                            );
                        } else if dp.stroke_width > 0.0 {
                            let stroke = Stroke {
                                width: dp.stroke_width as f32,
                                line_cap: tiny_skia::LineCap::Round,
                                line_join: tiny_skia::LineJoin::Round,
                                ..Stroke::default()
                            };
                            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                        }
                    }
                } else {
                    // Final fallback: render as text glyph
                    let font_size_px = (target_height * em).max(1.0) as f32;
                    draw_glyph(pixmap, ctx, *ch, font_size_px, x, y, false, FontId::Math);
                }
            }
        },

        BoxKind::SvgPath(path_data) => {
            if let Some((path, is_fill)) = parse_svg_path_ex(path_data) {
                let tx = x as f32;
                let ty = (y - layout.height * em) as f32;
                let scale = em as f32;

                let mut paint = Paint::default();
                paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                paint.anti_alias = true;

                let transform = Transform::from_translate(tx, ty).pre_scale(scale, scale);
                if is_fill {
                    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
                } else {
                    let stroke = Stroke {
                        width: 0.04,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
        },

        BoxKind::Color { color, child } => {
            let new_fg = parse_color(color);
            let old_fg = ctx.fg;
            ctx.fg = new_fg;
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_raster_box(pixmap, ctx, &child.layout, cx, cy);
            ctx.fg = old_fg;
        },

        BoxKind::Boxed {
            child,
            padding: _,
            bg_color,
            border_color,
        } => {
            // Background fill
            if let Some(bg) = bg_color {
                let w_px = (layout.width * em) as f32;
                let h_px = ((layout.height + layout.depth) * em) as f32;
                let ry = (y - layout.height * em) as f32;

                if let Some(rect) = Rect::from_xywh(x as f32, ry, w_px, h_px) {
                    let mut paint = Paint::default();
                    let rgb = parse_color(bg);
                    paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
                    paint.anti_alias = true;
                    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                }
            }

            // Border rectangle
            let w_px = (layout.width * em) as f32;
            let h_px = ((layout.height + layout.depth) * em) as f32;
            let ry = (y - layout.height * em) as f32;

            let mut pb = PathBuilder::new();
            pb.move_to(x as f32, ry);
            pb.line_to(x as f32 + w_px, ry);
            pb.line_to(x as f32 + w_px, ry + h_px);
            pb.line_to(x as f32, ry + h_px);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                if let Some(bc) = border_color {
                    let rgb = parse_color(bc);
                    paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
                } else {
                    paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                }
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: (0.04 * em) as f32, // proportional to font size, matches SVG path
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // Content
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_raster_box(pixmap, ctx, &child.layout, cx, cy);
        },
    }
}

// ---------------------------------------------------------------------------
// Glyph rasterization
// ---------------------------------------------------------------------------

/// Check whether a character is a large (n-ary) operator that may have display variants.
#[inline]
fn is_large_op_char(ch: char) -> bool {
    super::glyph_variants::is_large_op_char(ch)
}

#[allow(clippy::too_many_arguments)]
/// Forward italic shear (tangent) applied to `∂` to match MathJax's slant.
#[inline]
fn partial_deslant_skew() -> f32 {
    super::layout::PARTIAL_DESLANT_DEG.to_radians().tan()
}

fn draw_glyph(
    pixmap: &mut Pixmap,
    ctx: &mut RasterCtx,
    ch: char,
    font_size_px: f32,
    x: f64,
    baseline_y: f64,
    italic: bool,
    font: FontId,
) {
    if font_size_px < 0.5 {
        return;
    }

    let bits = font_size_px.to_bits();

    // Resolve which face + codepoint to actually rasterize, and whether to apply
    // a synthetic italic shear. Text faces carry their slant in the outline, so
    // they draw the base codepoint with no shear; a glyph missing from the text
    // face falls back to the Math font (mirroring the embedded-SVG fallback).
    let (used_font, render_ch, use_shear) = if font.is_math() {
        if italic {
            if let Some(italic_ch) = super::layout::math_italic_codepoint(ch) {
                let key = (FontId::Math, italic_ch, bits);
                let (m, bm) = &*ctx
                    .cache
                    .entry(key)
                    .or_insert_with(|| MATH_FONT.rasterize(italic_ch, font_size_px));
                if m.width > 0 && m.height > 0 && !bm.is_empty() {
                    (FontId::Math, italic_ch, false)
                } else {
                    (FontId::Math, ch, true)
                }
            } else {
                (FontId::Math, ch, true)
            }
        } else {
            (FontId::Math, ch, false)
        }
    } else {
        let key = (font, ch, bits);
        let (m, bm) = &*ctx
            .cache
            .entry(key)
            .or_insert_with(|| fontdue_font(font).rasterize(ch, font_size_px));
        if m.width > 0 && m.height > 0 && !bm.is_empty() {
            (font, ch, false)
        } else {
            (FontId::Math, ch, false)
        }
    };

    let key = (used_font, render_ch, bits);
    let fg = ctx.fg;
    let (metrics, bitmap) = &*ctx
        .cache
        .entry(key)
        .or_insert_with(|| fontdue_font(used_font).rasterize(render_ch, font_size_px));
    if metrics.width == 0 || metrics.height == 0 || bitmap.is_empty() {
        return;
    }

    // Position the glyph bitmap using sub-pixel coordinates for spacing accuracy.
    // fontdue metrics: ymin is y-up from baseline to bottom of glyph bounding box.
    // Keep fractional positions; tiny-skia's transform handles sub-pixel placement.
    let gx = (x as f32) + metrics.xmin as f32;
    let gy = (baseline_y as f32) - metrics.ymin as f32 - metrics.height as f32;

    // Reuse the RGBA buffer from ctx to avoid per-glyph Pixmap allocation.
    let gw = metrics.width as u32;
    let gh = metrics.height as u32;
    let needed = (gw as usize) * (gh as usize) * 4;
    let buf = &mut ctx.glyph_rgba_buf;
    buf.resize(needed, 0);
    for (i, &coverage) in bitmap.iter().enumerate() {
        let idx = i * 4;
        if idx + 3 < buf.len() {
            // Premultiplied RGBA: RGB values are pre-multiplied by alpha
            let a = coverage as u16;
            buf[idx] = ((fg[0] as u16 * a) / 255) as u8;
            buf[idx + 1] = ((fg[1] as u16 * a) / 255) as u8;
            buf[idx + 2] = ((fg[2] as u16 * a) / 255) as u8;
            buf[idx + 3] = coverage;
        }
    }
    if let Some(glyph_ref) = tiny_skia::PixmapRef::from_bytes(buf, gw, gh) {
        // Apply synthetic italic shear (≈12° slant) only when real italic glyph unavailable.
        // Pivot the shear about the baseline (not the glyph-bitmap top) so it matches the
        // embedded-SVG emitter, which applies `skewX(-12)` about the baseline. Without this
        // the two paths slant identically but offset horizontally, diverging in PNG vs SVG.
        //
        // Separately, the Latin Modern Math `∂` (U+2202) outline is nearly
        // upright; MathJax gives `∂` the slight forward italic lean standard in
        // math typography. Apply that forward shear (negative angle).
        let skew = if use_shear {
            -0.21_f32
        } else if used_font == FontId::Math && render_ch == '\u{2202}' {
            partial_deslant_skew()
        } else {
            0.0
        };
        let transform = if skew != 0.0 {
            let tx = gx - skew * (baseline_y as f32 - gy);
            Transform::from_row(1.0, 0.0, skew, 1.0, tx, gy)
        } else {
            Transform::from_translate(gx, gy)
        };
        pixmap.draw_pixmap(0, 0, glyph_ref, &PixmapPaint::default(), transform, None);
    }
}

/// Render a glyph by its font glyph index (for MATH table variants).
///
/// Uses ttf-parser for outline extraction and tiny-skia for path rendering,
/// because fontdue cannot rasterize non-cmap glyph IDs (MATH table variants
/// are typically not mapped to Unicode codepoints).
fn draw_glyph_variant(
    pixmap: &mut Pixmap,
    fg: [u8; 3],
    glyph_id: u16,
    font_size_px: f64,
    x: f64,
    baseline_y: f64,
) {
    if font_size_px < 0.5 {
        return;
    }
    let face = &*TTF_FACE;
    let upem = face.units_per_em() as f64;
    let scale = font_size_px / upem;

    let gid = ttf_parser::GlyphId(glyph_id);

    // Extract the outline as a tiny-skia path.
    let mut builder = GlyphPathBuilder::new();
    if face.outline_glyph(gid, &mut builder).is_none() {
        return;
    }
    let Some(path) = builder.finish() else {
        return;
    };

    // Transform: scale from design units to pixels, flip y-axis, position at (x, baseline_y).
    // Font coordinates: y-up from baseline.
    // Screen coordinates: y-down from top-left.
    // Transform: x' = x + du_x * scale, y' = baseline_y - du_y * scale
    let transform = Transform::from_row(
        scale as f32,
        0.0,
        0.0,
        -(scale as f32),
        x as f32,
        baseline_y as f32,
    );

    let mut paint = Paint::default();
    paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
    paint.anti_alias = true;

    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
}

/// Render a stretchy vertical delimiter/radical from its OpenType MATH glyph
/// assembly parts. Fills the vertical span `[box_top, box_top + fill_height*em]`
/// at horizontal position `x`. Returns `false` (drawing nothing) when the glyph
/// has no assembly recipe, so the caller can fall back to a synthetic path.
fn draw_vertical_assembly(
    pixmap: &mut Pixmap,
    fg: [u8; 3],
    ch: char,
    fill_height_em: f64,
    box_top: f64,
    x: f64,
    em: f64,
) -> bool {
    let Some(asm) = super::glyph_variants::vertical_assembly(ch, fill_height_em) else {
        return false;
    };
    let h_nat = asm.ink_top_em - asm.ink_bottom_em;
    if h_nat <= 0.001 {
        return false;
    }
    // Uniform fine-scale to land exactly on the requested fill height (the
    // assembly already gets within one extender of it, so `k` stays ≈ 1).
    let k = fill_height_em / h_nat;
    let fs = em * k;
    if fs < 0.5 {
        return false;
    }
    for p in &asm.placements {
        // Each part's baseline sits so its top ink aligns within the box.
        let baseline_y = box_top + (asm.ink_top_em - p.shift_em) * fs;
        draw_glyph_variant(pixmap, fg, p.glyph_id, fs, x, baseline_y);
    }
    true
}

/// OutlineBuilder that converts ttf-parser glyph outlines to tiny-skia Paths.
struct GlyphPathBuilder {
    pb: PathBuilder,
}

impl GlyphPathBuilder {
    fn new() -> Self {
        Self {
            pb: PathBuilder::new(),
        }
    }

    fn finish(self) -> Option<tiny_skia::Path> {
        self.pb.finish()
    }
}

impl ttf_parser::OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.pb.close();
    }
}

// ---------------------------------------------------------------------------
// SVG path parser (M/L/H/V/C/S/Q/T/A/Z, both absolute and relative)
// ---------------------------------------------------------------------------

/// Parse an SVG path string, detecting the `F:` fill prefix.
///
/// Returns `(path, is_fill)` where `is_fill` is `true` when the path data
/// started with `"F:"`, indicating the path should be filled rather than
/// stroked.
fn parse_svg_path_ex(d: &str) -> Option<(tiny_skia::Path, bool)> {
    let (path_str, is_fill) = match d.strip_prefix("F:") {
        Some(rest) => (rest, true),
        None => (d, false),
    };
    parse_svg_path(path_str).map(|p| (p, is_fill))
}

/// Parse an SVG arc flag (0 or 1) as a single character.
///
/// Arc flags are always a single digit and may appear without separators
/// between them or before the next coordinate value.
fn parse_flag(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<bool> {
    skip_separators(chars);
    match chars.peek().copied() {
        Some('0') => {
            chars.next();
            Some(false)
        },
        Some('1') => {
            chars.next();
            Some(true)
        },
        _ => None,
    }
}

/// Compute the angle (in radians) of vector (vx, vy) relative to (ux, uy).
fn arc_angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let n = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    if n < 1e-10 {
        return 0.0;
    }
    let c = (ux * vx + uy * vy) / n;
    let c = c.clamp(-1.0, 1.0);
    let angle = c.acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

/// Approximate an arc segment (from theta1 to theta2 on an ellipse) as a
/// single cubic bezier and append it to the path builder.
#[allow(clippy::too_many_arguments)]
fn arc_segment_to_cubic(
    pb: &mut PathBuilder,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    sin_phi: f32,
    cos_phi: f32,
    theta1: f32,
    theta2: f32,
) {
    let alpha = (theta2 - theta1).sin()
        * ((4.0 + 3.0 * ((theta2 - theta1) / 2.0).tan().powi(2)).sqrt() - 1.0)
        / 3.0;

    let cos1 = theta1.cos();
    let sin1 = theta1.sin();
    let cos2 = theta2.cos();
    let sin2 = theta2.sin();

    // Endpoint 1 (on the ellipse at theta1)
    let ex1 = rx * cos1;
    let ey1 = ry * sin1;
    // Endpoint 2 (on the ellipse at theta2)
    let ex2 = rx * cos2;
    let ey2 = ry * sin2;

    // Control point 1: P1 + alpha * P1'
    let dx1 = -rx * sin1;
    let dy1 = ry * cos1;
    let qx1 = ex1 + alpha * dx1;
    let qy1 = ey1 + alpha * dy1;

    // Control point 2: P2 - alpha * P2'
    let dx2 = -rx * sin2;
    let dy2 = ry * cos2;
    let qx2 = ex2 - alpha * dx2;
    let qy2 = ey2 - alpha * dy2;

    // Transform back from unit circle coordinates
    let cp1x = cos_phi * qx1 - sin_phi * qy1 + cx;
    let cp1y = sin_phi * qx1 + cos_phi * qy1 + cy;
    let cp2x = cos_phi * qx2 - sin_phi * qy2 + cx;
    let cp2y = sin_phi * qx2 + cos_phi * qy2 + cy;
    let ep_x = cos_phi * ex2 - sin_phi * ey2 + cx;
    let ep_y = sin_phi * ex2 + cos_phi * ey2 + cy;

    pb.cubic_to(cp1x, cp1y, cp2x, cp2y, ep_x, ep_y);
}

/// Convert an SVG arc (endpoint parameterization) to cubic bezier curves.
///
/// Implements the SVG spec F.6.5-F.6.6 conversion from endpoint to center
/// parameterization, then splits into segments of at most pi/2 radians and
/// approximates each with a cubic bezier.
#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    pb: &mut PathBuilder,
    cur_x: f32,
    cur_y: f32,
    mut rx: f32,
    mut ry: f32,
    x_rotation: f32,
    large_arc: bool,
    sweep: bool,
    end_x: f32,
    end_y: f32,
) {
    // Degenerate cases
    if (cur_x - end_x).abs() < 1e-7 && (cur_y - end_y).abs() < 1e-7 {
        return;
    }
    if rx.abs() < 1e-7 || ry.abs() < 1e-7 {
        pb.line_to(end_x, end_y);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();

    let phi = x_rotation.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    // Step 1: Compute (x1', y1') -- rotated midpoint
    let dx2 = (cur_x - end_x) / 2.0;
    let dy2 = (cur_y - end_y) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    // Step 2: Ensure radii are large enough
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let mut rx2 = rx * rx;
    let mut ry2 = ry * ry;
    let lambda = x1p2 / rx2 + y1p2 / ry2;
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
        rx2 = rx * rx;
        ry2 = ry * ry;
    }

    // Step 3: Compute center point (cx', cy')
    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let den = rx2 * y1p2 + ry2 * x1p2;
    let sq = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = sign * sq * (rx * y1p / ry);
    let cyp = sign * sq * -(ry * x1p / rx);

    // Step 4: Compute center (cx, cy) in original coordinates
    let cx = cos_phi * cxp - sin_phi * cyp + (cur_x + end_x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (cur_y + end_y) / 2.0;

    // Step 5: Compute start and sweep angles
    let theta1 = arc_angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = arc_angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );

    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }

    // Step 6: Split into segments and emit cubics
    let n_segs = (dtheta.abs() / (std::f32::consts::FRAC_PI_2 + 0.001)).ceil() as usize;
    let n_segs = n_segs.max(1);
    let seg_angle = dtheta / n_segs as f32;

    for i in 0..n_segs {
        let t1 = theta1 + seg_angle * i as f32;
        let t2 = theta1 + seg_angle * (i + 1) as f32;
        arc_segment_to_cubic(pb, cx, cy, rx, ry, sin_phi, cos_phi, t1, t2);
    }
}

fn parse_svg_path(d: &str) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let mut chars = d.chars().peekable();
    let mut has_content = false;

    // Current position tracking
    let mut cur_x: f32 = 0.0;
    let mut cur_y: f32 = 0.0;

    // Subpath start position (set by M/m, restored by Z/z per SVG spec)
    let mut subpath_x: f32 = 0.0;
    let mut subpath_y: f32 = 0.0;

    // Previous control point tracking for smooth curves
    let mut prev_cx: f32 = 0.0; // last cubic control point (2nd)
    let mut prev_cy: f32 = 0.0;
    let mut prev_qx: f32 = 0.0; // last quadratic control point
    let mut prev_qy: f32 = 0.0;

    // Last command for implicit repetition (SVG spec: subsequent coordinate
    // groups without a new command letter repeat the previous command).
    let mut last_cmd: Option<char> = None;

    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }

        // Determine the current command: either an explicit letter, or an
        // implicit repeat when the next char is a coordinate (digit, sign, dot).
        let cmd = match chars.peek().copied() {
            Some(c) if c.is_ascii_alphabetic() => {
                chars.next();
                // Per SVG spec: after M, implicit repeat becomes L; after m, becomes l.
                last_cmd = Some(match c {
                    'M' => 'L',
                    'm' => 'l',
                    other => other,
                });
                c
            },
            Some(c) if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() => {
                // Implicit repetition of last command
                match last_cmd {
                    Some(cmd) => cmd,
                    None => break, // no previous command — malformed
                }
            },
            None => break,
            _ => {
                chars.next(); // skip unknown character
                continue;
            },
        };

        match cmd {
            'M' => {
                if let Some((x, y)) = parse_coord_pair(&mut chars) {
                    pb.move_to(x, y);
                    has_content = true;
                    cur_x = x;
                    cur_y = y;
                    subpath_x = x;
                    subpath_y = y;
                    prev_cx = x;
                    prev_cy = y;
                    prev_qx = x;
                    prev_qy = y;
                }
            },
            'L' => {
                if let Some((x, y)) = parse_coord_pair(&mut chars) {
                    pb.line_to(x, y);
                    cur_x = x;
                    cur_y = y;
                    prev_cx = x;
                    prev_cy = y;
                    prev_qx = x;
                    prev_qy = y;
                }
            },
            'H' => {
                skip_separators(&mut chars);
                if let Some(x) = parse_number(&mut chars) {
                    pb.line_to(x, cur_y);
                    cur_x = x;
                    prev_cx = x;
                    prev_cy = cur_y;
                    prev_qx = x;
                    prev_qy = cur_y;
                }
            },
            'V' => {
                skip_separators(&mut chars);
                if let Some(y) = parse_number(&mut chars) {
                    pb.line_to(cur_x, y);
                    cur_y = y;
                    prev_cx = cur_x;
                    prev_cy = y;
                    prev_qx = cur_x;
                    prev_qy = y;
                }
            },
            'C' => {
                if let Some((x1, y1)) = parse_coord_pair(&mut chars)
                    && let Some((x2, y2)) = parse_coord_pair(&mut chars)
                    && let Some((x3, y3)) = parse_coord_pair(&mut chars)
                {
                    pb.cubic_to(x1, y1, x2, y2, x3, y3);
                    cur_x = x3;
                    cur_y = y3;
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x3;
                    prev_qy = y3;
                }
            },
            'S' => {
                if let Some((x2, y2)) = parse_coord_pair(&mut chars)
                    && let Some((x3, y3)) = parse_coord_pair(&mut chars)
                {
                    let rx = 2.0 * cur_x - prev_cx;
                    let ry = 2.0 * cur_y - prev_cy;
                    pb.cubic_to(rx, ry, x2, y2, x3, y3);
                    cur_x = x3;
                    cur_y = y3;
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x3;
                    prev_qy = y3;
                }
            },
            'Q' => {
                if let Some((x1, y1)) = parse_coord_pair(&mut chars)
                    && let Some((x2, y2)) = parse_coord_pair(&mut chars)
                {
                    pb.quad_to(x1, y1, x2, y2);
                    cur_x = x2;
                    cur_y = y2;
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x1;
                    prev_qy = y1;
                }
            },
            'T' => {
                if let Some((x2, y2)) = parse_coord_pair(&mut chars) {
                    let rx = 2.0 * cur_x - prev_qx;
                    let ry = 2.0 * cur_y - prev_qy;
                    pb.quad_to(rx, ry, x2, y2);
                    cur_x = x2;
                    cur_y = y2;
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = rx;
                    prev_qy = ry;
                }
            },
            // -------------------------------------------------------
            // Relative (lowercase) commands
            // -------------------------------------------------------
            'm' => {
                if let Some((dx, dy)) = parse_coord_pair(&mut chars) {
                    let x = cur_x + dx;
                    let y = cur_y + dy;
                    pb.move_to(x, y);
                    has_content = true;
                    cur_x = x;
                    cur_y = y;
                    subpath_x = x;
                    subpath_y = y;
                    prev_cx = x;
                    prev_cy = y;
                    prev_qx = x;
                    prev_qy = y;
                }
            },
            'l' => {
                if let Some((dx, dy)) = parse_coord_pair(&mut chars) {
                    let x = cur_x + dx;
                    let y = cur_y + dy;
                    pb.line_to(x, y);
                    cur_x = x;
                    cur_y = y;
                    prev_cx = x;
                    prev_cy = y;
                    prev_qx = x;
                    prev_qy = y;
                }
            },
            'h' => {
                skip_separators(&mut chars);
                if let Some(dx) = parse_number(&mut chars) {
                    let x = cur_x + dx;
                    pb.line_to(x, cur_y);
                    cur_x = x;
                    prev_cx = x;
                    prev_cy = cur_y;
                    prev_qx = x;
                    prev_qy = cur_y;
                }
            },
            'v' => {
                skip_separators(&mut chars);
                if let Some(dy) = parse_number(&mut chars) {
                    let y = cur_y + dy;
                    pb.line_to(cur_x, y);
                    cur_y = y;
                    prev_cx = cur_x;
                    prev_cy = y;
                    prev_qx = cur_x;
                    prev_qy = y;
                }
            },
            'c' => {
                if let Some((dx1, dy1)) = parse_coord_pair(&mut chars)
                    && let Some((dx2, dy2)) = parse_coord_pair(&mut chars)
                    && let Some((dx3, dy3)) = parse_coord_pair(&mut chars)
                {
                    let x1 = cur_x + dx1;
                    let y1 = cur_y + dy1;
                    let x2 = cur_x + dx2;
                    let y2 = cur_y + dy2;
                    let x3 = cur_x + dx3;
                    let y3 = cur_y + dy3;
                    pb.cubic_to(x1, y1, x2, y2, x3, y3);
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x3;
                    prev_qy = y3;
                    cur_x = x3;
                    cur_y = y3;
                }
            },
            's' => {
                if let Some((dx2, dy2)) = parse_coord_pair(&mut chars)
                    && let Some((dx3, dy3)) = parse_coord_pair(&mut chars)
                {
                    let rx = 2.0 * cur_x - prev_cx;
                    let ry = 2.0 * cur_y - prev_cy;
                    let x2 = cur_x + dx2;
                    let y2 = cur_y + dy2;
                    let x3 = cur_x + dx3;
                    let y3 = cur_y + dy3;
                    pb.cubic_to(rx, ry, x2, y2, x3, y3);
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x3;
                    prev_qy = y3;
                    cur_x = x3;
                    cur_y = y3;
                }
            },
            'q' => {
                if let Some((dx1, dy1)) = parse_coord_pair(&mut chars)
                    && let Some((dx2, dy2)) = parse_coord_pair(&mut chars)
                {
                    let x1 = cur_x + dx1;
                    let y1 = cur_y + dy1;
                    let x2 = cur_x + dx2;
                    let y2 = cur_y + dy2;
                    pb.quad_to(x1, y1, x2, y2);
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = x1;
                    prev_qy = y1;
                    cur_x = x2;
                    cur_y = y2;
                }
            },
            't' => {
                if let Some((dx, dy)) = parse_coord_pair(&mut chars) {
                    let x2 = cur_x + dx;
                    let y2 = cur_y + dy;
                    let rx = 2.0 * cur_x - prev_qx;
                    let ry = 2.0 * cur_y - prev_qy;
                    pb.quad_to(rx, ry, x2, y2);
                    prev_cx = x2;
                    prev_cy = y2;
                    prev_qx = rx;
                    prev_qy = ry;
                    cur_x = x2;
                    cur_y = y2;
                }
            },
            // -------------------------------------------------------
            // Arc commands
            // -------------------------------------------------------
            'A' => {
                skip_separators(&mut chars);
                if let Some(rx) = parse_number(&mut chars)
                    && let Some(ry) = {
                        skip_separators(&mut chars);
                        parse_number(&mut chars)
                    }
                    && let Some(x_rot) = {
                        skip_separators(&mut chars);
                        parse_number(&mut chars)
                    }
                    && let Some(large_arc) = parse_flag(&mut chars)
                    && let Some(sweep) = parse_flag(&mut chars)
                    && let Some((ex, ey)) = parse_coord_pair(&mut chars)
                {
                    arc_to_cubics(
                        &mut pb, cur_x, cur_y, rx, ry, x_rot, large_arc, sweep, ex, ey,
                    );
                    cur_x = ex;
                    cur_y = ey;
                    prev_cx = ex;
                    prev_cy = ey;
                    prev_qx = ex;
                    prev_qy = ey;
                }
            },
            'a' => {
                skip_separators(&mut chars);
                if let Some(rx) = parse_number(&mut chars)
                    && let Some(ry) = {
                        skip_separators(&mut chars);
                        parse_number(&mut chars)
                    }
                    && let Some(x_rot) = {
                        skip_separators(&mut chars);
                        parse_number(&mut chars)
                    }
                    && let Some(large_arc) = parse_flag(&mut chars)
                    && let Some(sweep) = parse_flag(&mut chars)
                    && let Some((dx, dy)) = parse_coord_pair(&mut chars)
                {
                    let ex = cur_x + dx;
                    let ey = cur_y + dy;
                    arc_to_cubics(
                        &mut pb, cur_x, cur_y, rx, ry, x_rot, large_arc, sweep, ex, ey,
                    );
                    cur_x = ex;
                    cur_y = ey;
                    prev_cx = ex;
                    prev_cy = ey;
                    prev_qx = ex;
                    prev_qy = ey;
                }
            },
            'Z' | 'z' => {
                pb.close();
                cur_x = subpath_x;
                cur_y = subpath_y;
                prev_cx = cur_x;
                prev_cy = cur_y;
                prev_qx = cur_x;
                prev_qy = cur_y;
                last_cmd = None; // Z resets implicit repeat
            },
            _ => {
                // Unknown command — skip
            },
        }
    }

    if has_content { pb.finish() } else { None }
}

fn parse_coord_pair(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<(f32, f32)> {
    skip_separators(chars);
    let x = parse_number(chars)?;
    skip_separators(chars);
    let y = parse_number(chars)?;
    Some((x, y))
}

fn skip_separators(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
        chars.next();
    }
}

fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f32> {
    // Stack-allocated buffer avoids heap allocation per number.
    let mut buf = [0u8; 32];
    let mut len = 0usize;
    // Optional sign
    if chars.peek().is_some_and(|c| *c == '-' || *c == '+') && len < buf.len() {
        buf[len] = chars.next().unwrap() as u8;
        len += 1;
    }
    // Digits and decimal point
    let mut has_digit = false;
    while chars
        .peek()
        .is_some_and(|c| c.is_ascii_digit() || *c == '.')
    {
        if len < buf.len() {
            buf[len] = chars.next().unwrap() as u8;
            len += 1;
        } else {
            chars.next(); // consume but skip on overflow
        }
        has_digit = true;
    }
    if has_digit {
        // SAFETY: buf contains only ASCII digits, '.', '+', '-'
        let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
        s.parse().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Color parser (CSS named colors + hex)
// ---------------------------------------------------------------------------

pub(crate) fn parse_color(s: &str) -> [u8; 3] {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return [0, 0, 0];
    }
    if let Some(rgb) = parse_rgb_function(s) {
        return rgb;
    }
    // Full CSS named colors (148 colors)
    if let Some(rgb) = crate::css_colors::lookup_css_color(s) {
        return rgb;
    }
    [0, 0, 0] // fallback to black
}

fn parse_rgb_function(s: &str) -> Option<[u8; 3]> {
    let (name, inner) = css_function(s)?;
    if !name.eq_ignore_ascii_case("rgb") {
        return None;
    }

    let mut parts = inner.split(',').map(str::trim);
    let r = parse_rgb_component(parts.next()?)?;
    let g = parse_rgb_component(parts.next()?)?;
    let b = parse_rgb_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some([r, g, b])
}

fn css_function(s: &str) -> Option<(&str, &str)> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open || !s[close + 1..].trim().is_empty() {
        return None;
    }
    Some((s[..open].trim(), &s[open + 1..close]))
}

fn parse_rgb_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(percent) = s.strip_suffix('%') {
        let pct = percent.trim().parse::<f64>().ok()?.clamp(0.0, 100.0);
        return Some((pct * 255.0 / 100.0).round() as u8);
    }
    Some(s.parse::<f64>().ok()?.clamp(0.0, 255.0).round() as u8)
}

fn parse_hex_color(hex: &str) -> [u8; 3] {
    let hex = hex.trim();
    // Reject non-ASCII input to prevent byte-boundary panics on multi-byte chars
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return [0, 0, 0];
    }
    match hex.len() {
        3 => {
            // #RGB → #RRGGBB (safe to byte-slice: all chars are ASCII hex digits)
            let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0);
            [r * 17, g * 17, b * 17]
        },
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b]
        },
        _ => [0, 0, 0],
    }
}

// ---------------------------------------------------------------------------
// Image encoding
// ---------------------------------------------------------------------------

fn encode_image(pixmap: &Pixmap, options: &RasterOptions) -> Result<Vec<u8>> {
    match options.format {
        ImageFormat::Png => pixmap
            .encode_png()
            .map_err(|e| crate::error::Error::Render {
                message: format!("PNG encoding failed: {e}"),
            }),
        ImageFormat::Jpeg => encode_jpeg(pixmap, options.quality),
    }
}

fn encode_jpeg(pixmap: &Pixmap, quality: u8) -> Result<Vec<u8>> {
    let quality = quality.clamp(1, 100);
    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    let data = pixmap.data(); // premultiplied RGBA

    // Convert premultiplied RGBA → straight RGB (with white fallback for transparent pixels)
    let mut rgb = Vec::with_capacity(w * h * 3);
    for pixel in data.chunks_exact(4) {
        let (pr, pg, pb, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);
        if a == 0 {
            rgb.extend_from_slice(&[255, 255, 255]); // transparent → white
        } else if a == 255 {
            rgb.extend_from_slice(&[pr, pg, pb]); // fully opaque
        } else {
            // Unpremultiply and blend with white background
            let af = a as f32 / 255.0;
            let r = (pr as f32 / af).min(255.0);
            let g = (pg as f32 / af).min(255.0);
            let b = (pb as f32 / af).min(255.0);
            let inv = 1.0 - af;
            rgb.push((r * af + 255.0 * inv) as u8);
            rgb.push((g * af + 255.0 * inv) as u8);
            rgb.push((b * af + 255.0 * inv) as u8);
        }
    }

    let w_u16 = u16::try_from(w).map_err(|_| crate::error::Error::Render {
        message: format!("image width {w} exceeds JPEG maximum of 65535"),
    })?;
    let h_u16 = u16::try_from(h).map_err(|_| crate::error::Error::Render {
        message: format!("image height {h} exceeds JPEG maximum of 65535"),
    })?;
    let mut buf = Vec::with_capacity(w * h); // rough estimate for JPEG output
    let encoder = jpeg_encoder::Encoder::new(&mut buf, quality);
    encoder
        .encode(&rgb, w_u16, h_u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| crate::error::Error::Render {
            message: format!("JPEG encoding failed: {e}"),
        })?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn render_png(nodes: Vec<MathNode>) -> Vec<u8> {
        let expr = MathExpr::inline(nodes);
        let options = RasterOptions::default();
        render_to_image(&expr, &options).unwrap()
    }

    fn render_jpeg(nodes: Vec<MathNode>) -> Vec<u8> {
        let expr = MathExpr::inline(nodes);
        let options = RasterOptions {
            format: ImageFormat::Jpeg,
            ..RasterOptions::default()
        };
        render_to_image(&expr, &options).unwrap()
    }

    #[test]
    fn png_magic_bytes() {
        let png = render_png(vec![MathNode::Number("1".into())]);
        assert!(png.len() > 8);
        assert_eq!(&png[..4], b"\x89PNG", "should have PNG magic bytes");
    }

    #[test]
    fn jpeg_magic_bytes() {
        let jpg = render_jpeg(vec![MathNode::Number("1".into())]);
        assert!(jpg.len() > 3);
        assert_eq!(jpg[0], 0xFF);
        assert_eq!(jpg[1], 0xD8);
    }

    #[test]
    fn scale_factor_larger_image() {
        let expr = MathExpr::inline(vec![MathNode::Number("42".into())]);
        let opts_1x = RasterOptions {
            scale: 1.0,
            ..RasterOptions::default()
        };
        let opts_2x = RasterOptions {
            scale: 2.0,
            ..RasterOptions::default()
        };
        let png_1x = render_to_image(&expr, &opts_1x).unwrap();
        let png_2x = render_to_image(&expr, &opts_2x).unwrap();
        // 2x should produce significantly more bytes (larger image)
        assert!(
            png_2x.len() > png_1x.len(),
            "2x scale should produce larger PNG"
        );
    }

    #[test]
    fn custom_background() {
        let expr = MathExpr::inline(vec![MathNode::Number("1".into())]);
        let options = RasterOptions {
            background: Some([255, 0, 0, 255]), // red background
            ..RasterOptions::default()
        };
        let png = render_to_image(&expr, &options).unwrap();
        assert!(png.len() > 8);
        assert_eq!(&png[..4], b"\x89PNG");
    }

    #[test]
    fn default_options() {
        let opts = RasterOptions::default();
        assert_eq!(opts.format, ImageFormat::Png);
        assert_eq!(opts.scale, 1.0);
        assert_eq!(opts.quality, 90);
        assert!(opts.background.is_none());
    }

    // --- Color parser ---

    #[test]
    fn color_parse_hex6() {
        assert_eq!(parse_color("#ff0000"), [255, 0, 0]);
        assert_eq!(parse_color("#00ff00"), [0, 255, 0]);
        assert_eq!(parse_color("#0000ff"), [0, 0, 255]);
    }

    #[test]
    fn color_parse_hex3() {
        assert_eq!(parse_color("#f00"), [255, 0, 0]);
        assert_eq!(parse_color("#0f0"), [0, 255, 0]);
    }

    #[test]
    fn color_parse_named() {
        assert_eq!(parse_color("red"), [255, 0, 0]);
        assert_eq!(parse_color("blue"), [0, 0, 255]);
        assert_eq!(parse_color("black"), [0, 0, 0]);
        assert_eq!(parse_color("white"), [255, 255, 255]);
    }

    #[test]
    fn color_parse_currentcolor() {
        assert_eq!(parse_color("currentColor"), [0, 0, 0]);
    }

    #[test]
    fn color_parse_rgb_comma_syntax() {
        assert_eq!(parse_color("rgb(255, 0, 0)"), [255, 0, 0]);
        assert_eq!(parse_color("rgb(100%, 50%, 0%)"), [255, 128, 0]);
    }

    #[test]
    fn color_parse_rgba_is_unsupported_without_alpha_pipeline() {
        assert_eq!(parse_rgb_function("rgba(255, 0, 0, 0.5)"), None);
        assert_eq!(parse_color("rgba(255, 0, 0, 0.5)"), [0, 0, 0]);
    }

    // --- SVG path parser ---

    #[test]
    fn path_parse_move_line() {
        let path = parse_svg_path("M0,0 L1,1").unwrap();
        let b = path.bounds();
        assert!(b.width() > 0.0 || b.height() > 0.0);
    }

    #[test]
    fn path_parse_empty() {
        assert!(parse_svg_path("").is_none());
    }

    // --- Rendering with various AST nodes ---

    #[test]
    fn fraction_renders() {
        let png = render_png(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(png.len() > 100, "fraction should produce non-trivial image");
    }

    #[test]
    fn superscript_renders() {
        let png = render_png(vec![MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(png.len() > 100);
    }

    #[test]
    fn sqrt_renders() {
        let png = render_png(vec![MathNode::Sqrt(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
        assert!(png.len() > 100);
    }

    #[test]
    fn empty_expr_renders() {
        let png = render_png(vec![]);
        assert!(!png.is_empty());
    }

    #[test]
    fn identifier_renders() {
        let png = render_png(vec![MathNode::Identifier("x".into())]);
        assert!(png.len() > 50);
    }

    #[test]
    fn parse_svg_path_handles_fill_prefix() {
        let (_, is_fill) = parse_svg_path_ex("F:M0,0 L1,0 L1,1 L0,1 Z").unwrap();
        assert!(is_fill, "F: prefix should indicate a filled path");
        let (_, is_fill) = parse_svg_path_ex("M0,0 L1,0 L1,1").unwrap();
        assert!(!is_fill, "no prefix should indicate a stroked path");
    }

    #[test]
    fn parse_svg_path_ex_returns_none_for_empty() {
        assert!(parse_svg_path_ex("").is_none());
        assert!(parse_svg_path_ex("F:").is_none());
    }

    #[test]
    fn parse_svg_path_horizontal_vertical() {
        let path = parse_svg_path("M0,0 H5 V3 Z");
        assert!(path.is_some(), "H and V commands should be supported");
    }

    #[test]
    fn parse_svg_path_smooth_cubic() {
        let path = parse_svg_path("M0,0 C1,1 2,0 3,0 S5,1 6,0");
        assert!(path.is_some(), "S command should be supported");
    }

    #[test]
    fn parse_svg_path_smooth_quad() {
        let path = parse_svg_path("M0,0 Q1,1 2,0 T4,0");
        assert!(path.is_some(), "T command should be supported");
    }

    // --- Relative (lowercase) command tests ---

    #[test]
    fn parse_svg_path_relative_line() {
        let path = parse_svg_path("M10,10 l5,5 l-5,0");
        assert!(path.is_some(), "relative l should work");
    }

    #[test]
    fn parse_svg_path_relative_h_v() {
        let path = parse_svg_path("M0,0 h10 v10 h-10 z");
        assert!(path.is_some(), "relative h/v should work");
    }

    #[test]
    fn parse_svg_path_relative_cubic() {
        // Mixed case commands as seen in widehat paths
        let path = parse_svg_path("M529 0h5l519 115c5 1 9 5 9 10 0 1-1 2-1 3l-4 22z");
        assert!(path.is_some(), "mixed case commands should work");
    }

    #[test]
    fn parse_svg_path_relative_move() {
        let path = parse_svg_path("M0,0 L5,5 m10,10 L20,20");
        assert!(path.is_some(), "relative m should work");
    }

    #[test]
    fn parse_svg_path_relative_smooth_cubic() {
        let path = parse_svg_path("M0,0 c1,1 2,0 3,0 s2,1 3,0");
        assert!(path.is_some(), "relative s (smooth cubic) should work");
    }

    #[test]
    fn parse_svg_path_relative_quad() {
        let path = parse_svg_path("M0,0 q1,1 2,0 t2,0");
        assert!(path.is_some(), "relative q and t should work");
    }

    // --- Arc command tests ---

    #[test]
    fn parse_svg_path_arc() {
        let path = parse_svg_path("M0.14,0.07 a0.07,0.07 0 1 1 -0.14,0 a0.07,0.07 0 1 1 0.14,0 Z");
        assert!(path.is_some(), "arc command should work");
    }

    #[test]
    fn parse_svg_path_arc_absolute() {
        let path = parse_svg_path("M10,80 A25,25 0 0 1 50,80");
        assert!(path.is_some(), "absolute arc command should work");
    }

    #[test]
    fn parse_svg_path_dot_circle_filled() {
        let result = parse_svg_path_ex(
            "F:M0.1400,0.0700 a0.0700,0.0700 0 1 1 -0.1400,0 a0.0700,0.0700 0 1 1 0.1400,0 Z",
        );
        assert!(result.is_some(), "dot circle should parse");
        let (_, is_fill) = result.unwrap();
        assert!(is_fill, "dot circle should be filled");
    }

    #[test]
    fn parse_svg_path_widehat() {
        let path = parse_svg_path(
            "M529 0h5l519 115c5 1 9 5 9 10 0 1-1 2-1 3l-4 22c-1 5-5 9-11 9h-2L532 67 19 159h-2c-5 0-9-4-11-9l-5-22c-1-6 2-12 8-13z",
        );
        assert!(path.is_some(), "widehat path should parse");
    }

    // --- Negative space / clipping tests ---

    #[test]
    fn negative_space_does_not_clip() {
        // Triple negative thin space (\!\!\!x) should produce a valid PNG,
        // not blank or clipped output.
        let nodes = vec![
            MathNode::Space(Rational::new(-3, 18)),
            MathNode::Space(Rational::new(-3, 18)),
            MathNode::Space(Rational::new(-3, 18)),
            MathNode::Identifier("x".into()),
        ];
        let png = render_png(nodes);
        assert!(
            png.len() > 100,
            "negative-space expression should produce a non-trivial image"
        );
        assert_eq!(&png[..4], b"\x89PNG");
    }

    #[test]
    fn compute_x_bounds_detects_negative_shift() {
        use crate::render::layout::PositionedBox;
        // Build a minimal HBox with a negative kern followed by a glyph
        let kern = PositionedBox::at_origin(LayoutBox {
            width: 0.0,
            height: 0.0,
            depth: 0.0,
            kind: BoxKind::Kern,
        });
        let glyph = PositionedBox::new(
            -0.5,
            0.0,
            LayoutBox {
                width: 0.6,
                height: 0.7,
                depth: 0.1,
                kind: BoxKind::Glyph {
                    ch: 'x',
                    size: 1.0,
                    italic: false,
                    font: FontId::Math,
                },
            },
        );
        let root = LayoutBox {
            width: 0.1, // artificially narrow
            height: 0.7,
            depth: 0.1,
            kind: BoxKind::HBox {
                children: vec![kern, glyph],
            },
        };
        let (min_x, max_x) = compute_x_bounds(&root, 0.0);
        assert!(min_x < 0.0, "min_x should be negative: got {min_x}");
        assert!(
            (max_x - 0.1).abs() < 1e-9,
            "max_x should be 0.1: got {max_x}"
        );
    }

    #[test]
    fn parse_svg_path_z_restores_subpath_start() {
        // After Z, the current position should be the subpath start (M coords).
        // Then a relative lineto should be relative to that start, not the last point before Z.
        let path = parse_svg_path("M10,10 L20,20 Z l5,0");
        assert!(
            path.is_some(),
            "path with Z followed by relative command should parse"
        );
    }

    #[test]
    fn parse_svg_path_multiple_subpaths() {
        // Two subpaths: first closes at M origin, second starts elsewhere
        let path = parse_svg_path("M0,0 L10,10 Z M20,20 L30,30 Z");
        assert!(path.is_some(), "multiple closed subpaths should parse");
    }

    #[test]
    fn parse_svg_path_filled_prefix() {
        // Test the F: prefix for filled paths
        let result = parse_svg_path_ex("F:M0,0 L10,0 L10,10 Z");
        assert!(result.is_some(), "filled path should parse");
        let (_, is_fill) = result.unwrap();
        assert!(is_fill, "F: prefix should set fill mode");
    }

    #[test]
    fn parse_svg_path_stroke_no_prefix() {
        let result = parse_svg_path_ex("M0,0 L10,0 L10,10 Z");
        assert!(result.is_some(), "stroke path should parse");
        let (_, is_fill) = result.unwrap();
        assert!(!is_fill, "no prefix should be stroke mode");
    }

    // --- Implicit command repetition tests ---

    #[test]
    fn parse_svg_path_implicit_lineto_after_moveto() {
        // After M, implicit coordinates are treated as L (lineto).
        // SVG spec: "If a moveto is followed by multiple pairs of coordinates,
        // the subsequent pairs are treated as implicit lineto commands."
        let path = parse_svg_path("M0,0 10,10 20,0 Z");
        assert!(path.is_some(), "implicit L after M should work");
    }

    #[test]
    fn parse_svg_path_implicit_cubic_repeat() {
        // After c, subsequent coordinate groups repeat the c command.
        // This is the pattern used in KaTeX widetilde paths.
        let path = parse_svg_path("M0,0 c1,2 3,4 5,6 7,8 9,10 11,12");
        assert!(path.is_some(), "implicit c repeat should work");
    }

    #[test]
    fn parse_svg_path_implicit_relative_lineto() {
        // After m, implicit coordinates become l (relative lineto).
        let path = parse_svg_path("m0,0 5,5 10,0 Z");
        assert!(path.is_some(), "implicit l after m should work");
    }

    #[test]
    fn parse_svg_path_widetilde_full() {
        // Full widetilde tier-0 path from KaTeX with implicit c repetitions.
        let path = parse_svg_path(
            "M200 55.538c-77 0-168 73.953-177 73.953-3 0-7-2.175-9-5.437L2 97c-1-2-2-4-2-6 0-4 2-7 5-9l20-12C116 12 171 0 207 0c86 0 114 68 191 68 78 0 168-68 177-68 4 0 7 2 9 5l12 19c1 2.175 2 4.35 2 6.525 0 4.35-2 7.613-5 9.788l-19 13.05c-92 63.077-116.937 75.308-183 76.128-68.267.847-113-73.952-191-73.952z",
        );
        assert!(
            path.is_some(),
            "full widetilde path with implicit repeats should parse"
        );
    }

    #[test]
    fn parse_svg_path_implicit_h_repeat() {
        // After H, implicit numbers repeat H.
        let path = parse_svg_path("M0,0 H10 20 30");
        assert!(path.is_some(), "implicit H repeat should work");
    }

    #[test]
    fn parse_svg_path_implicit_v_repeat() {
        // After V, implicit numbers repeat V.
        let path = parse_svg_path("M0,0 V10 20 30");
        assert!(path.is_some(), "implicit V repeat should work");
    }

    #[test]
    fn parse_svg_path_widehat_full() {
        // Full widehat path from KaTeX with implicit command repeats.
        let path = parse_svg_path(
            "M1181 0h2l1171 176c6 0 10 5 10 11l-2 23c-1 6-5 10-11 10h-1L1182 67 15 220h-1c-6 0-10-4-11-10l-2-23c-1-6 4-11 10-11z",
        );
        assert!(path.is_some(), "full widehat path should parse");
    }
}
