/// Direct raster rendering: converts a LayoutBox tree into PNG or JPEG image bytes.
///
/// Pipeline: MathExpr → LayoutBox tree → tiny_skia::Pixmap → PNG/JPEG bytes.
/// Glyphs are rasterized with `fontdue` using an embedded Latin Modern Math font.
use std::collections::HashMap;
use std::sync::LazyLock;

use fontdue::{Font, FontSettings};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

use super::RenderOptions;
use super::layout::{BoxKind, LayoutBox};
use super::style::MathStyle;
use crate::ast::MathExpr;
use crate::ast::compact::CompactExpr;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Embedded font (Latin Modern Math, GUST Font License)
// ---------------------------------------------------------------------------

static MATH_FONT: LazyLock<Font> = LazyLock::new(|| {
    Font::from_bytes(super::FONT_DATA, FontSettings::default()).expect("embedded font must be valid")
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

fn rasterize_layout(root: &LayoutBox, options: &RasterOptions) -> Result<Vec<u8>> {
    let em = options.render.font_size_px * options.scale;
    let padding = 2.0 * options.scale; // 2px padding at 1x

    // Guard against excessive dimensions that could cause OOM
    const MAX_PIXMAP_DIMENSION: u32 = 8192;

    let width = (root.width * em + padding * 2.0).ceil().max(1.0) as u32;
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
    let font = &*MATH_FONT;

    let mut ctx = RasterCtx {
        font,
        em,
        fg,
        cache: HashMap::new(),
    };

    let baseline_y = root.height * em + padding;
    emit_raster_box(&mut pixmap, &mut ctx, root, padding, baseline_y);

    encode_image(&pixmap, options)
}

// ---------------------------------------------------------------------------
// Recursive tree walker
// ---------------------------------------------------------------------------

struct RasterCtx<'a> {
    font: &'a Font,
    em: f64,
    fg: [u8; 3],
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
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
        BoxKind::Glyph { ch, size, italic } => {
            let font_size_px = (em * size).max(1.0) as f32;
            draw_glyph(pixmap, ctx, *ch, font_size_px, x, y, *italic);
        },

        BoxKind::TextRun { text, size, italic } => {
            let font_size_px = (em * size).max(1.0) as f32;
            let mut cx = x;
            for ch in text.chars() {
                let m = super::metrics::glyph_metrics(ch);
                draw_glyph(pixmap, ctx, ch, font_size_px, cx, y, *italic);
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
            let ry = (y - thickness * em / 2.0) as f32;
            if let Some(rect) = Rect::from_xywh(x as f32, ry, w_px, th_px.max(1.0)) {
                let mut paint = Paint::default();
                paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                paint.anti_alias = true;
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
        },

        BoxKind::Kern | BoxKind::Phantom => {
            // Invisible — no output
        },

        BoxKind::Delimiter { ch, target_height } => {
            // Use SVG path for tall delimiters
            if let Some(dp) = super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0) {
                if let Some(path) = parse_svg_path(&dp.path_data) {
                    let tx = x as f32;
                    let ty = (y - layout.height * em) as f32;
                    let scale = em as f32;

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                    paint.anti_alias = true;

                    let stroke = Stroke {
                        width: dp.stroke_width as f32,
                        line_cap: tiny_skia::LineCap::Round,
                        line_join: tiny_skia::LineJoin::Round,
                        ..Stroke::default()
                    };

                    let transform = Transform::from_translate(tx, ty).pre_scale(scale, scale);
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            } else {
                // Fallback: render as text glyph
                let font_size_px = (target_height * em).max(1.0) as f32;
                draw_glyph(pixmap, ctx, *ch, font_size_px, x, y, false);
            }
        },

        BoxKind::SvgPath(path_data) => {
            if let Some(path) = parse_svg_path(path_data) {
                let tx = x as f32;
                let ty = (y - layout.height * em) as f32;
                let scale = em as f32;

                let mut paint = Paint::default();
                paint.set_color_rgba8(fg[0], fg[1], fg[2], 255);
                paint.anti_alias = true;

                let stroke = Stroke {
                    width: 0.04,
                    ..Stroke::default()
                };

                let transform = Transform::from_translate(tx, ty).pre_scale(scale, scale);
                pixmap.stroke_path(&path, &paint, &stroke, transform, None);
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

        BoxKind::Boxed { child, padding: _, bg_color, border_color } => {
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
                    width: 1.0,
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

fn draw_glyph(
    pixmap: &mut Pixmap,
    ctx: &mut RasterCtx,
    ch: char,
    font_size_px: f32,
    x: f64,
    baseline_y: f64,
    italic: bool,
) {
    if font_size_px < 0.5 {
        return;
    }
    let key = (ch, font_size_px.to_bits());
    let fg = ctx.fg;
    let (metrics, bitmap) = &*ctx
        .cache
        .entry(key)
        .or_insert_with(|| ctx.font.rasterize(ch, font_size_px));
    if metrics.width == 0 || metrics.height == 0 || bitmap.is_empty() {
        return;
    }

    // Position the glyph bitmap.
    // fontdue metrics: ymin is y-up from baseline to bottom of glyph bounding box.
    // Screen is y-down: bitmap top = baseline_y - ymin - height
    let gx = x as i32 + metrics.xmin;
    let gy = baseline_y as i32 - metrics.ymin - metrics.height as i32;

    // Create a small RGBA pixmap for the glyph
    let gw = metrics.width as u32;
    let gh = metrics.height as u32;
    if let Some(mut glyph_pm) = Pixmap::new(gw, gh) {
        let data = glyph_pm.data_mut();
        for (i, &coverage) in bitmap.iter().enumerate() {
            let idx = i * 4;
            if idx + 3 < data.len() {
                // Premultiplied RGBA: RGB values are pre-multiplied by alpha
                let a = coverage as u16;
                data[idx] = ((fg[0] as u16 * a) / 255) as u8;
                data[idx + 1] = ((fg[1] as u16 * a) / 255) as u8;
                data[idx + 2] = ((fg[2] as u16 * a) / 255) as u8;
                data[idx + 3] = coverage;
            }
        }
        // Apply synthetic italic shear (≈12° slant) when italic is true.
        // The shear matrix shifts x by y * tan(12°) ≈ y * 0.21.
        let transform = if italic {
            Transform::from_row(1.0, 0.0, -0.21, 1.0, gx as f32, gy as f32)
        } else {
            Transform::from_translate(gx as f32, gy as f32)
        };
        pixmap.draw_pixmap(
            0,
            0,
            glyph_pm.as_ref(),
            &PixmapPaint::default(),
            transform,
            None,
        );
    }
}

// ---------------------------------------------------------------------------
// SVG path parser (handles only M and L commands used in layout.rs)
// ---------------------------------------------------------------------------

fn parse_svg_path(d: &str) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let mut chars = d.chars().peekable();
    let mut has_content = false;

    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.peek().copied() {
            Some('M') => {
                chars.next();
                if let Some((x, y)) = parse_coord_pair(&mut chars) {
                    pb.move_to(x, y);
                    has_content = true;
                }
            },
            Some('L') => {
                chars.next();
                if let Some((x, y)) = parse_coord_pair(&mut chars) {
                    pb.line_to(x, y);
                }
            },
            Some('C') => {
                chars.next();
                // Cubic bezier: C x1,y1 x2,y2 x3,y3
                if let Some((x1, y1)) = parse_coord_pair(&mut chars)
                    && let Some((x2, y2)) = parse_coord_pair(&mut chars)
                    && let Some((x3, y3)) = parse_coord_pair(&mut chars)
                {
                    pb.cubic_to(x1, y1, x2, y2, x3, y3);
                }
            },
            Some('Q') => {
                chars.next();
                // Quadratic bezier: Q x1,y1 x2,y2
                if let Some((x1, y1)) = parse_coord_pair(&mut chars)
                    && let Some((x2, y2)) = parse_coord_pair(&mut chars)
                {
                    pb.quad_to(x1, y1, x2, y2);
                }
            },
            Some('Z') | Some('z') => {
                chars.next();
                pb.close();
            },
            None => break,
            _ => {
                chars.next(); // skip unknown
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
    let mut s = String::new();
    // Optional sign
    if chars.peek().is_some_and(|c| *c == '-' || *c == '+') {
        s.push(chars.next().unwrap());
    }
    // Digits and decimal point
    let mut has_digit = false;
    while chars
        .peek()
        .is_some_and(|c| c.is_ascii_digit() || *c == '.')
    {
        s.push(chars.next().unwrap());
        has_digit = true;
    }
    if has_digit { s.parse().ok() } else { None }
}

// ---------------------------------------------------------------------------
// Color parser (CSS named colors + hex)
// ---------------------------------------------------------------------------

pub(crate) fn parse_color(s: &str) -> [u8; 3] {
    let s = s.trim();
    // Hex colors
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    // Named colors
    match s.to_lowercase().as_str() {
        "black" | "currentcolor" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "gray" | "grey" => [128, 128, 128],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "brown" => [165, 42, 42],
        "navy" => [0, 0, 128],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "maroon" => [128, 0, 0],
        "lime" => [0, 255, 0],
        "silver" => [192, 192, 192],
        _ => [0, 0, 0], // fallback to black
    }
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
    let mut buf = Vec::new();
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
}
