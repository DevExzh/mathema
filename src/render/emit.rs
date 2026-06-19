/// SVG emission: converts a LayoutBox tree into an SVG string.
use super::RenderOptions;
use super::fonts_registry::FontId;
use super::layout::{BoxKind, LayoutBox};

/// Write an XML-escaped version of `s` directly into a `String` buffer.
/// Maps `<`, `>`, `&`, `"`, `'` to their entities to prevent attribute
/// injection, while avoiding the `std::fmt` machinery entirely.
#[inline]
pub(crate) fn push_xml_escaped(out: &mut String, s: &str) {
    // Fast path: if the string contains no special chars, push it directly
    if !s
        .bytes()
        .any(|b| matches!(b, b'<' | b'>' | b'&' | b'"' | b'\''))
    {
        out.push_str(s);
        return;
    }
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

/// Write a float with 3 decimal places directly into a String, avoiding
/// `std::fmt` formatting overhead. This is 3-5x faster than `write!(buf, "{:.3}", v)`.
#[inline]
pub(crate) fn push_f3(out: &mut String, v: f64) {
    // Guard against NaN/Infinity producing corrupt SVG attribute values
    if !v.is_finite() {
        out.push_str("0.000");
        return;
    }

    // Handle negative inline (no recursion)
    let v = if v < 0.0 {
        out.push('-');
        -v
    } else {
        v
    };

    // Multiply by 1000, round, then split into integer and fractional parts.
    // Clamp to prevent overflow when casting to u64 (v > ~1.8e16 would overflow).
    let scaled = (v * 1000.0).round().min(u64::MAX as f64) as u64;
    let integer_part = scaled / 1000;
    let frac_part = (scaled % 1000) as u32;

    // Write integer part using itoa-style manual conversion
    push_u64(out, integer_part);

    // Write decimal point and exactly 3 fractional digits
    out.push('.');
    if frac_part < 10 {
        out.push_str("00");
    } else if frac_part < 100 {
        out.push('0');
    }
    push_u32(out, frac_part);
}

/// Write a u64 in decimal to a String buffer (manual, no fmt overhead).
#[inline]
pub(crate) fn push_u64(out: &mut String, mut n: u64) {
    if n == 0 {
        out.push('0');
        return;
    }
    // Max u64 is 20 digits
    let mut buf = [0u8; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    // SAFETY: buf[i..20] contains only ASCII digits
    out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..20]) });
}

/// Write a u32 in decimal to a String buffer.
#[inline]
fn push_u32(out: &mut String, n: u32) {
    push_u64(out, n as u64);
}

/// Pre-escaped SVG attribute strings cached from RenderOptions.
/// Avoids re-escaping on every element emission.
struct EscapedAttrs<'a> {
    font_family: &'a str,
    fg_color: String,
}

#[inline]
fn svg_italic_char(ch: char, italic: bool) -> (char, bool) {
    if !italic {
        return (ch, false);
    }
    if let Some(mapped) = super::layout::math_italic_codepoint(ch) {
        (mapped, false)
    } else {
        (ch, true)
    }
}

/// CSS hints for drawing a glyph from a Latin Modern text face in the
/// client-font (`<text>`) SVG path: `(font-family override, bold, italic)`.
///
/// Returns `None` for the Math font, which keeps the Unicode-remap rendering.
#[inline]
fn font_css(font: FontId) -> Option<(Option<&'static str>, bool, bool)> {
    Some(match font {
        FontId::Math => return None,
        FontId::RomanRegular => (None, false, false),
        FontId::RomanBold => (None, true, false),
        FontId::RomanItalic => (None, false, true),
        FontId::RomanBoldItalic => (None, true, true),
        FontId::SansRegular => (Some("sans-serif"), false, false),
        FontId::SansBold => (Some("sans-serif"), true, false),
        FontId::SansItalic => (Some("sans-serif"), false, true),
        FontId::SansBoldItalic => (Some("sans-serif"), true, true),
        FontId::MonoRegular => (Some("monospace"), false, false),
        FontId::MonoItalic => (Some("monospace"), false, true),
        FontId::MonoBold => (Some("monospace"), true, false),
        FontId::MonoBoldItalic => (Some("monospace"), true, true),
    })
}

fn svg_text_run(text: &str, italic: bool) -> (String, bool) {
    if !italic {
        return (text.to_string(), false);
    }

    let mut mapped = String::with_capacity(text.len());
    for ch in text.chars() {
        let Some(mapped_ch) = super::layout::math_italic_codepoint(ch) else {
            return (text.to_string(), true);
        };
        mapped.push(mapped_ch);
    }
    (mapped, false)
}

/// Return `(view_box_left_px, content_width_px)` for visible horizontal SVG content.
pub(crate) fn svg_x_bounds_px(root: &LayoutBox, em: f64) -> (f64, f64) {
    let (min_x, max_x) = compute_x_bounds(root, 0.0);
    x_bounds_to_px(root.width, min_x, max_x, em)
}

/// Return `(view_box_left_px, content_width_px)` for visible arena SVG content.
pub(crate) fn svg_x_bounds_px_arena(arena: &LayoutArena, root: LayoutBoxId, em: f64) -> (f64, f64) {
    let root_box = arena.get(root);
    let (min_x, max_x) = compute_x_bounds_arena(arena, root, 0.0);
    x_bounds_to_px(root_box.width, min_x, max_x, em)
}

fn x_bounds_to_px(declared_width: f64, min_x: f64, max_x: f64, em: f64) -> (f64, f64) {
    let left = if min_x.is_finite() {
        min_x.min(0.0)
    } else {
        0.0
    };
    let right = if max_x.is_finite() {
        declared_width.max(max_x)
    } else {
        declared_width
    };
    let width = (right - left).max(0.0);
    (left * em, width * em)
}

fn compute_x_bounds(layout: &LayoutBox, x: f64) -> (f64, f64) {
    match &layout.kind {
        BoxKind::Glyph { .. }
        | BoxKind::TextRun { .. }
        | BoxKind::Rule { .. }
        | BoxKind::Delimiter { .. }
        | BoxKind::SvgPath(_) => (x, x + layout.width),

        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for child in children {
                let (cmin, cmax) = compute_x_bounds(&child.layout, x + child.dx);
                lo = lo.min(cmin);
                hi = hi.max(cmax);
            }
            if lo > hi {
                (x, x + layout.width)
            } else {
                (lo, hi)
            }
        },

        BoxKind::Color { child, .. } => compute_x_bounds(&child.layout, x + child.dx),

        BoxKind::Boxed { child, .. } => {
            let (cmin, cmax) = compute_x_bounds(&child.layout, x + child.dx);
            (cmin.min(x), cmax.max(x + layout.width))
        },

        BoxKind::Kern | BoxKind::Phantom => (f64::INFINITY, f64::NEG_INFINITY),
    }
}

fn compute_x_bounds_arena(arena: &LayoutArena, id: LayoutBoxId, x: f64) -> (f64, f64) {
    let layout = arena.get(id);
    match &layout.kind {
        ArenaBoxKind::Glyph { .. }
        | ArenaBoxKind::TextRun { .. }
        | ArenaBoxKind::Rule { .. }
        | ArenaBoxKind::Delimiter { .. }
        | ArenaBoxKind::SvgPath(_) => (x, x + layout.width),

        ArenaBoxKind::HBox { children } | ArenaBoxKind::VBox { children } => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for child in arena.children(*children) {
                let (cmin, cmax) = compute_x_bounds_arena(arena, child.id, x + child.dx);
                lo = lo.min(cmin);
                hi = hi.max(cmax);
            }
            if lo > hi {
                (x, x + layout.width)
            } else {
                (lo, hi)
            }
        },

        ArenaBoxKind::Color { child, .. } => compute_x_bounds_arena(arena, *child, x),

        ArenaBoxKind::Boxed { child, padding, .. } => {
            let (cmin, cmax) = compute_x_bounds_arena(arena, *child, x + padding);
            (cmin.min(x), cmax.max(x + layout.width))
        },

        ArenaBoxKind::Kern | ArenaBoxKind::Phantom => (f64::INFINITY, f64::NEG_INFINITY),
    }
}

/// Emit the final SVG string from a laid-out box tree.
pub fn emit_svg(root: &LayoutBox, options: &RenderOptions) -> String {
    let em = options.font_size_px;
    let (view_x_px, width_px) = svg_x_bounds_px(root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;

    // Small padding around the viewBox so that paths (radical signs, accent
    // strokes, delimiter curves) are not clipped at the exact bounding box edge.
    let pad = 0.08 * em;

    let mut svg = String::with_capacity(4096);
    let mut font_family_escaped = String::with_capacity(options.font_family.len() + 4);
    push_xml_escaped(&mut font_family_escaped, &options.font_family);
    let mut fg_color_escaped = String::with_capacity(options.fg_color.len() + 4);
    push_xml_escaped(&mut fg_color_escaped, &options.fg_color);
    let attrs = EscapedAttrs {
        font_family: &font_family_escaped,
        fg_color: fg_color_escaped,
    };

    // SVG header — use fast float formatter
    svg.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width=""#);
    push_f3(&mut svg, width_px + 2.0 * pad);
    svg.push_str(r#"" height=""#);
    push_f3(&mut svg, height_px + 2.0 * pad);
    svg.push_str(r#"" viewBox=""#);
    push_f3(&mut svg, view_x_px - pad);
    svg.push(' ');
    push_f3(&mut svg, -pad);
    svg.push(' ');
    push_f3(&mut svg, width_px + 2.0 * pad);
    svg.push(' ');
    push_f3(&mut svg, height_px + 2.0 * pad);
    svg.push_str(r#"" style="vertical-align: "#);
    push_f3(&mut svg, -(depth_px + pad));
    svg.push_str(r#"px">"#);

    // Render content
    // Origin in SVG: (0, root.height * em) corresponds to the baseline at x=0
    let baseline_y = root.height * em;
    emit_box(&mut svg, root, 0.0, baseline_y, em, &attrs);

    svg.push_str("</svg>");
    svg
}

fn emit_box(
    svg: &mut String,
    layout: &LayoutBox,
    x: f64,
    y: f64,
    em: f64,
    attrs: &EscapedAttrs<'_>,
) {
    match &layout.kind {
        BoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => {
            let font_size = em * size;
            if let Some((family, bold, ital_style)) = font_css(*font) {
                // Text-face glyph: draw the base codepoint with CSS style hints.
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(family.unwrap_or(attrs.font_family));
                if ital_style {
                    svg.push_str(r#"" font-style="italic"#);
                }
                if bold {
                    svg.push_str(r#"" font-weight="bold"#);
                }
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, *ch);
                svg.push_str("</text>");
            } else {
                let (render_ch, emit_italic_style) = svg_italic_char(*ch, *italic);
                // Build <text> element with fast formatting and pre-escaped attributes
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(attrs.font_family);
                if emit_italic_style {
                    svg.push_str(r#"" font-style="italic"#);
                }
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, render_ch);
                svg.push_str("</text>");
            }
        },

        BoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => {
            let (render_text, emit_italic_style) = svg_text_run(text, *italic);
            let font_size = em * size;
            svg.push_str(r#"<text x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, y);
            svg.push_str(r#"" font-size=""#);
            push_f3(svg, font_size);
            svg.push_str(r#"" font-family=""#);
            match text_style {
                Some(
                    crate::ast::TextStyle::SansSerif
                    | crate::ast::TextStyle::SansSerifBold
                    | crate::ast::TextStyle::SansSerifItalic
                    | crate::ast::TextStyle::SansSerifBoldItalic,
                ) => {
                    svg.push_str("sans-serif");
                },
                Some(crate::ast::TextStyle::Monospace) => {
                    svg.push_str("monospace");
                },
                _ => {
                    svg.push_str(attrs.font_family);
                },
            }
            if emit_italic_style {
                svg.push_str(r#"" font-style="italic"#);
            }
            if matches!(
                text_style,
                Some(
                    crate::ast::TextStyle::Bold
                        | crate::ast::TextStyle::BoldItalic
                        | crate::ast::TextStyle::SansSerifBold
                        | crate::ast::TextStyle::SansSerifBoldItalic
                        | crate::ast::TextStyle::BoldScript
                        | crate::ast::TextStyle::BoldFraktur
                )
            ) {
                svg.push_str(r#"" font-weight="bold"#);
            }
            svg.push_str(r#"" fill=""#);
            svg.push_str(&attrs.fg_color);
            svg.push_str(r#"">"#);
            push_xml_escaped(svg, &render_text);
            svg.push_str("</text>");
        },

        BoxKind::HBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                // dy is positive = up in TeX, but SVG y-down, so subtract
                let cy = y - child.dy * em;
                emit_box(svg, &child.layout, cx, cy, em, attrs);
            }
        },

        BoxKind::VBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box(svg, &child.layout, cx, cy, em, attrs);
            }
        },

        BoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            // A zero-area rule (e.g. a zero-width `\\[Nem]` row strut) renders
            // nothing; skip it so output matches the arena engine, which adds
            // such gaps to positions directly rather than as a strut box.
            if w_px != 0.0 && th_px != 0.0 {
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
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#""/>"#);
            }
        },

        BoxKind::Kern => {
            // Invisible — no output
        },

        BoxKind::Delimiter { ch, target_height } => {
            // The radical (√) renders from the synthetic path here (the text-based
            // SVG cannot embed font outlines); standalone SVG / raster use the
            // real MATH-table variant glyph.
            if *ch == '\u{221A}' {
                let tx = x;
                let ty = y - layout.height * em;
                let path = super::layout::radical_delim_path(layout.width, *target_height);
                svg.push_str(r#"<g transform="translate("#);
                push_f3(svg, tx);
                svg.push(',');
                push_f3(svg, ty);
                svg.push_str(r#") scale("#);
                push_f3(svg, em);
                let fill_path = path.strip_prefix("F:").unwrap_or(&path);
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, fill_path);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke="none"/></g>"#);
            } else if let Some(dp) =
                super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0)
            {
                let tx = x;
                let ty = y - layout.height * em;
                svg.push_str(r#"<g transform="translate("#);
                push_f3(svg, tx);
                svg.push(',');
                push_f3(svg, ty);
                svg.push_str(r#") scale("#);
                push_f3(svg, em);
                // "F:" prefix indicates a filled path (parens, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    svg.push_str(r#")"><path d=""#);
                    push_xml_escaped(svg, fill_path);
                    svg.push_str(r#"" fill=""#);
                    svg.push_str(&attrs.fg_color);
                    svg.push_str(r#"" stroke="none"/></g>"#);
                } else {
                    svg.push_str(r#")"><path d=""#);
                    push_xml_escaped(svg, &dp.path_data);
                    svg.push_str(r#"" stroke=""#);
                    svg.push_str(&attrs.fg_color);
                    svg.push_str(r#"" stroke-width=""#);
                    push_f3(svg, dp.stroke_width);
                    svg.push_str(
                        r#"" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>"#,
                    );
                }
            } else {
                // Fallback: text element with font-size scaling
                let font_size = target_height * em;
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(attrs.font_family);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, *ch);
                svg.push_str("</text>");
            }
        },

        BoxKind::SvgPath(path) => {
            // Transform path coordinates from em to px
            // The path is in local coordinates where (0,0) is top-left
            // We need to translate to the actual position
            let tx = x;
            let ty = y - layout.height * em;
            svg.push_str(r#"<g transform="translate("#);
            push_f3(svg, tx);
            svg.push(',');
            push_f3(svg, ty);
            svg.push_str(r#") scale("#);
            push_f3(svg, em);
            // "F:" prefix indicates a filled path (KaTeX-style accents)
            if let Some(fill_path) = path.strip_prefix("F:") {
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, fill_path);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke="none"/></g>"#);
            } else {
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, path);
                svg.push_str(r#"" stroke=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke-width="0.04" fill="none"/></g>"#);
            }
        },

        BoxKind::Color { color, child } => {
            // Propagate color to child elements via both a group wrapper (for
            // any SVG elements that don't explicitly set fill/stroke) and via
            // modified attrs (so elements that emit explicit fill= use the
            // correct color instead of "currentColor").
            let color_attrs = EscapedAttrs {
                font_family: attrs.font_family,
                fg_color: {
                    let mut c = String::new();
                    push_xml_escaped(&mut c, color);
                    c
                },
            };
            svg.push_str(r#"<g fill=""#);
            push_xml_escaped(svg, color);
            svg.push_str(r#"" stroke=""#);
            push_xml_escaped(svg, color);
            svg.push_str(r#"">"#);
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box(svg, &child.layout, cx, cy, em, &color_attrs);
            svg.push_str("</g>");
        },

        BoxKind::Phantom => {
            // Invisible — takes space but doesn't render
        },

        BoxKind::Boxed {
            child,
            padding: _,
            bg_color,
            border_color,
        } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;

            // Border rectangle
            svg.push_str(r#"<rect x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, ry);
            svg.push_str(r#"" width=""#);
            push_f3(svg, w_px);
            svg.push_str(r#"" height=""#);
            push_f3(svg, h_px);
            svg.push_str(r#"" stroke=""#);
            if let Some(bc) = border_color {
                push_xml_escaped(svg, bc);
            } else {
                svg.push_str(&attrs.fg_color);
            }
            svg.push_str(r#"" fill=""#);
            if let Some(bg) = bg_color {
                push_xml_escaped(svg, bg);
            } else {
                svg.push_str("none");
            }
            svg.push_str(r#"" stroke-width=""#);
            // Proportional to font size (≈rule_thickness ≈ 0.04em)
            push_f3(svg, 0.04 * em);
            svg.push_str(r#""/>"#);

            // Content
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box(svg, &child.layout, cx, cy, em, attrs);
        },
    }
}

// ── Arena-based SVG emission ─────────────────────────────────────────

use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};

/// Emit the final SVG string from an arena-based layout.
/// Produces byte-identical output to `emit_svg` for the same expression.
pub fn emit_svg_arena(arena: &LayoutArena, root: LayoutBoxId, options: &RenderOptions) -> String {
    let root_box = arena.get(root);
    let em = options.font_size_px;
    let (view_x_px, width_px) = svg_x_bounds_px_arena(arena, root, em);
    let height_px = (root_box.height + root_box.depth) * em;
    let depth_px = root_box.depth * em;

    let mut svg = String::with_capacity(4096);
    let mut font_family_escaped = String::with_capacity(options.font_family.len() + 4);
    push_xml_escaped(&mut font_family_escaped, &options.font_family);
    let mut fg_color_escaped = String::with_capacity(options.fg_color.len() + 4);
    push_xml_escaped(&mut fg_color_escaped, &options.fg_color);
    let attrs = EscapedAttrs {
        font_family: &font_family_escaped,
        fg_color: fg_color_escaped,
    };

    // SVG header — identical format to emit_svg
    let pad = 0.08 * em;
    svg.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width=""#);
    push_f3(&mut svg, width_px + 2.0 * pad);
    svg.push_str(r#"" height=""#);
    push_f3(&mut svg, height_px + 2.0 * pad);
    svg.push_str(r#"" viewBox=""#);
    push_f3(&mut svg, view_x_px - pad);
    svg.push(' ');
    push_f3(&mut svg, -pad);
    svg.push(' ');
    push_f3(&mut svg, width_px + 2.0 * pad);
    svg.push(' ');
    push_f3(&mut svg, height_px + 2.0 * pad);
    svg.push_str(r#"" style="vertical-align: "#);
    push_f3(&mut svg, -(depth_px + pad));
    svg.push_str(r#"px">"#);

    // Render content
    let baseline_y = root_box.height * em;
    emit_box_arena(&mut svg, arena, root, 0.0, baseline_y, em, &attrs);

    svg.push_str("</svg>");
    svg
}

fn emit_box_arena(
    svg: &mut String,
    arena: &LayoutArena,
    id: LayoutBoxId,
    x: f64,
    y: f64,
    em: f64,
    attrs: &EscapedAttrs<'_>,
) {
    let layout = arena.get(id);
    match &layout.kind {
        ArenaBoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => {
            let font_size = em * size;
            if let Some((family, bold, ital_style)) = font_css(*font) {
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(family.unwrap_or(attrs.font_family));
                if ital_style {
                    svg.push_str(r#"" font-style="italic"#);
                }
                if bold {
                    svg.push_str(r#"" font-weight="bold"#);
                }
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, *ch);
                svg.push_str("</text>");
            } else {
                let (render_ch, emit_italic_style) = svg_italic_char(*ch, *italic);
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(attrs.font_family);
                if emit_italic_style {
                    svg.push_str(r#"" font-style="italic"#);
                }
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, render_ch);
                svg.push_str("</text>");
            }
        },

        ArenaBoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => {
            let (render_text, emit_italic_style) = svg_text_run(text, *italic);
            let font_size = em * size;
            svg.push_str(r#"<text x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, y);
            svg.push_str(r#"" font-size=""#);
            push_f3(svg, font_size);
            svg.push_str(r#"" font-family=""#);
            match text_style {
                Some(
                    crate::ast::TextStyle::SansSerif
                    | crate::ast::TextStyle::SansSerifBold
                    | crate::ast::TextStyle::SansSerifItalic
                    | crate::ast::TextStyle::SansSerifBoldItalic,
                ) => {
                    svg.push_str("sans-serif");
                },
                Some(crate::ast::TextStyle::Monospace) => {
                    svg.push_str("monospace");
                },
                _ => {
                    svg.push_str(attrs.font_family);
                },
            }
            if emit_italic_style {
                svg.push_str(r#"" font-style="italic"#);
            }
            if matches!(
                text_style,
                Some(
                    crate::ast::TextStyle::Bold
                        | crate::ast::TextStyle::BoldItalic
                        | crate::ast::TextStyle::SansSerifBold
                        | crate::ast::TextStyle::SansSerifBoldItalic
                        | crate::ast::TextStyle::BoldScript
                        | crate::ast::TextStyle::BoldFraktur
                )
            ) {
                svg.push_str(r#"" font-weight="bold"#);
            }
            svg.push_str(r#"" fill=""#);
            svg.push_str(&attrs.fg_color);
            svg.push_str(r#"">"#);
            push_xml_escaped(svg, &render_text);
            svg.push_str("</text>");
        },

        ArenaBoxKind::HBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena(svg, arena, child.id, cx, cy, em, attrs);
            }
        },

        ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena(svg, arena, child.id, cx, cy, em, attrs);
            }
        },

        ArenaBoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            // A zero-area rule renders nothing; skip it (keeps parity with the
            // legacy emitter and avoids emitting invisible struts).
            if w_px != 0.0 && th_px != 0.0 {
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
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#""/>"#);
            }
        },

        ArenaBoxKind::Kern => {
            // Invisible — no output
        },

        ArenaBoxKind::Delimiter { ch, target_height } => {
            // See the heap `BoxKind::Delimiter` arm: √ uses the synthetic path here.
            if *ch == '\u{221A}' {
                let tx = x;
                let ty = y - layout.height * em;
                let path = super::layout::radical_delim_path(layout.width, *target_height);
                svg.push_str(r#"<g transform="translate("#);
                push_f3(svg, tx);
                svg.push(',');
                push_f3(svg, ty);
                svg.push_str(r#") scale("#);
                push_f3(svg, em);
                let fill_path = path.strip_prefix("F:").unwrap_or(&path);
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, fill_path);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke="none"/></g>"#);
            } else if let Some(dp) =
                super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0)
            {
                let tx = x;
                let ty = y - layout.height * em;
                svg.push_str(r#"<g transform="translate("#);
                push_f3(svg, tx);
                svg.push(',');
                push_f3(svg, ty);
                svg.push_str(r#") scale("#);
                push_f3(svg, em);
                // "F:" prefix indicates a filled path (parens, braces)
                if let Some(fill_path) = dp.path_data.strip_prefix("F:") {
                    svg.push_str(r#")"><path d=""#);
                    push_xml_escaped(svg, fill_path);
                    svg.push_str(r#"" fill=""#);
                    svg.push_str(&attrs.fg_color);
                    svg.push_str(r#"" stroke="none"/></g>"#);
                } else {
                    svg.push_str(r#")"><path d=""#);
                    push_xml_escaped(svg, &dp.path_data);
                    svg.push_str(r#"" stroke=""#);
                    svg.push_str(&attrs.fg_color);
                    svg.push_str(r#"" stroke-width=""#);
                    push_f3(svg, dp.stroke_width);
                    svg.push_str(
                        r#"" fill="none" stroke-linecap="round" stroke-linejoin="round"/></g>"#,
                    );
                }
            } else {
                // Fallback: text element with font-size scaling
                let font_size = target_height * em;
                svg.push_str(r#"<text x=""#);
                push_f3(svg, x);
                svg.push_str(r#"" y=""#);
                push_f3(svg, y);
                svg.push_str(r#"" font-size=""#);
                push_f3(svg, font_size);
                svg.push_str(r#"" font-family=""#);
                svg.push_str(attrs.font_family);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"">"#);
                write_escaped_char(svg, *ch);
                svg.push_str("</text>");
            }
        },

        ArenaBoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            svg.push_str(r#"<g transform="translate("#);
            push_f3(svg, tx);
            svg.push(',');
            push_f3(svg, ty);
            svg.push_str(r#") scale("#);
            push_f3(svg, em);
            // "F:" prefix indicates a filled path (KaTeX-style accents)
            if let Some(fill_path) = path.strip_prefix("F:") {
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, fill_path);
                svg.push_str(r#"" fill=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke="none"/></g>"#);
            } else {
                svg.push_str(r#")"><path d=""#);
                push_xml_escaped(svg, path);
                svg.push_str(r#"" stroke=""#);
                svg.push_str(&attrs.fg_color);
                svg.push_str(r#"" stroke-width="0.04" fill="none"/></g>"#);
            }
        },

        ArenaBoxKind::Color { color, child } => {
            let color_attrs = EscapedAttrs {
                font_family: attrs.font_family,
                fg_color: {
                    let mut c = String::new();
                    push_xml_escaped(&mut c, color);
                    c
                },
            };
            svg.push_str(r#"<g fill=""#);
            push_xml_escaped(svg, color);
            svg.push_str(r#"" stroke=""#);
            push_xml_escaped(svg, color);
            svg.push_str(r#"">"#);
            // Color child is rendered at same position (no dx/dy offset)
            emit_box_arena(svg, arena, *child, x, y, em, &color_attrs);
            svg.push_str("</g>");
        },

        ArenaBoxKind::Phantom => {
            // Invisible — takes space but doesn't render
        },

        ArenaBoxKind::Boxed {
            child,
            padding,
            bg_color,
            border_color,
        } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;

            // Border rectangle
            svg.push_str(r#"<rect x=""#);
            push_f3(svg, x);
            svg.push_str(r#"" y=""#);
            push_f3(svg, ry);
            svg.push_str(r#"" width=""#);
            push_f3(svg, w_px);
            svg.push_str(r#"" height=""#);
            push_f3(svg, h_px);
            svg.push_str(r#"" stroke=""#);
            if let Some(bc) = border_color {
                push_xml_escaped(svg, bc);
            } else {
                svg.push_str(&attrs.fg_color);
            }
            svg.push_str(r#"" fill=""#);
            if let Some(bg) = bg_color {
                push_xml_escaped(svg, bg);
            } else {
                svg.push_str("none");
            }
            svg.push_str(r#"" stroke-width=""#);
            push_f3(svg, 0.04 * em);
            svg.push_str(r#""/>"#);

            // Content — arena version stores child without position offset,
            // so we must apply padding here (heap version stored dx=padding in PositionedBox)
            emit_box_arena(svg, arena, *child, x + padding * em, y, em, attrs);
        },
    }
}

/// Write an XML-escaped character directly to the buffer (zero allocation).
pub(crate) fn write_escaped_char(out: &mut String, ch: char) {
    match ch {
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '&' => out.push_str("&amp;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&apos;"),
        _ => out.push(ch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::render::layout::{PositionedBox, layout_expr};
    use crate::render::style::MathStyle;

    fn render(latex_nodes: Vec<MathNode>) -> String {
        let expr = MathExpr::inline(latex_nodes);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        emit_svg(&root, &options)
    }

    #[test]
    fn simple_number_svg() {
        let svg = render(vec![MathNode::Number("42".into())]);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("4"));
        assert!(svg.contains("2"));
    }

    #[test]
    fn fraction_has_rect() {
        let svg = render(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(svg.contains("<rect")); // fraction bar
    }

    #[test]
    fn xml_escaping() {
        let svg = render(vec![MathNode::Symbol(SymbolKind::Relation, "<".into())]);
        assert!(svg.contains("&lt;"));
    }

    #[test]
    fn svg_has_viewbox() {
        let svg = render(vec![MathNode::Number("1".into())]);
        assert!(svg.contains("viewBox"));
        assert!(svg.contains("xmlns"));
    }

    fn render_display(latex_nodes: Vec<MathNode>) -> String {
        let expr = MathExpr {
            nodes: latex_nodes,
            display: crate::ast::DisplayStyle::Block,
        };
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Display, &options);
        emit_svg(&root, &options)
    }

    #[test]
    fn superscript_svg() {
        let svg = render(vec![MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(svg.contains("<text"));
        assert!(svg.contains("x"));
        assert!(svg.contains("2"));
        // Superscript should have smaller font size
        assert!(
            svg.contains("11.200"),
            "superscript should use script-size font"
        );
    }

    #[test]
    fn subscript_svg() {
        let svg = render(vec![MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        }]);
        assert!(svg.contains("<text"));
        assert!(svg.contains("x"));
    }

    #[test]
    fn sqrt_svg_has_path_and_rule() {
        let svg = render(vec![MathNode::Sqrt(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
        assert!(
            svg.contains("<path") || svg.contains("<rect"),
            "sqrt should have path (radical) or rect (vinculum)"
        );
    }

    #[test]
    fn boxed_svg_has_border_rect() {
        let svg = render(vec![MathNode::Boxed {
            content: Box::new(MathNode::Identifier("x".into())),
            bg_color: None,
            border_color: None,
        }]);
        assert!(
            svg.contains("stroke="),
            "boxed should have a stroked border rect"
        );
        assert!(svg.contains("<rect"), "boxed should contain rect element");
        // Plain boxed should have fill="none"
        assert!(
            svg.contains(r#"fill="none""#),
            "plain boxed should have fill=none"
        );
    }

    #[test]
    fn colorbox_svg_has_bg_fill() {
        let svg = render(vec![MathNode::Boxed {
            content: Box::new(MathNode::Identifier("B".into())),
            bg_color: Some("teal".into()),
            border_color: None,
        }]);
        assert!(
            svg.contains("<rect"),
            "colorbox should contain rect element"
        );
        assert!(
            svg.contains(r#"fill="teal""#),
            "colorbox should have fill=teal, got: {svg}"
        );
    }

    #[test]
    fn fcolorbox_svg_has_bg_and_border() {
        let svg = render(vec![MathNode::Boxed {
            content: Box::new(MathNode::Identifier("C".into())),
            bg_color: Some("red".into()),
            border_color: Some("blue".into()),
        }]);
        assert!(
            svg.contains("<rect"),
            "fcolorbox should contain rect element"
        );
        assert!(
            svg.contains(r#"fill="red""#),
            "fcolorbox should have fill=red, got: {svg}"
        );
        assert!(
            svg.contains(r#"stroke="blue""#),
            "fcolorbox should have stroke=blue, got: {svg}"
        );
    }

    #[test]
    fn phantom_svg_no_visible_content() {
        let svg = render(vec![MathNode::Phantom(
            Box::new(MathNode::Identifier("x".into())),
            PhantomKind::Full,
        )]);
        // Phantom should NOT contain any text elements for its content
        assert!(
            !svg.contains("<text"),
            "phantom should not render visible text"
        );
    }

    #[test]
    fn cancel_svg_has_path() {
        let svg = render(vec![MathNode::Cancel {
            stroke: crate::ast::StrokeKind::Forward,
            content: Box::new(MathNode::Identifier("x".into())),
        }]);
        assert!(
            svg.contains("<path"),
            "cancel should produce an SVG path for the strike line"
        );
    }

    #[test]
    fn color_group_svg() {
        // Build a Color node directly via layout
        let options = RenderOptions::default();
        let child_layout = layout_expr(
            &MathExpr::inline(vec![MathNode::Identifier("x".into())]),
            MathStyle::Text,
            &options,
        );
        // Manually construct a color box
        let color_box = LayoutBox {
            width: child_layout.width,
            height: child_layout.height,
            depth: child_layout.depth,
            kind: BoxKind::Color {
                color: "red".into(),
                child: Box::new(PositionedBox::new(0.0, 0.0, child_layout)),
            },
        };
        let svg = emit_svg(&color_box, &options);
        assert!(svg.contains("fill=\"red\""), "color group should set fill");
        assert!(
            svg.contains("stroke=\"red\""),
            "color group should set stroke"
        );
    }

    #[test]
    fn kern_svg_no_output() {
        let options = RenderOptions::default();
        let kern_box = LayoutBox {
            width: 0.5,
            height: 0.0,
            depth: 0.0,
            kind: BoxKind::Kern,
        };
        let svg = emit_svg(&kern_box, &options);
        // Kern should only produce the svg wrapper, no content elements
        assert!(!svg.contains("<text"), "kern should not produce text");
        assert!(!svg.contains("<rect"), "kern should not produce rect");
    }

    #[test]
    fn delimiter_svg() {
        let svg = render(vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![crate::ast::FencedItem::Element(MathNode::Identifier(
                "x".into(),
            ))],
        }]);
        assert!(
            svg.contains("<text"),
            "fenced should produce text for delimiters"
        );
        assert!(
            svg.contains("(") || svg.contains("&#40;"),
            "should contain open paren"
        );
    }

    #[test]
    fn tall_delimiter_uses_path() {
        // A nested fraction creates tall content, forcing delimiters to use SVG paths
        let svg = render(vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![crate::ast::FencedItem::Element(MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(MathNode::Fraction {
                    kind: FractionKind::Normal,
                    num: Box::new(MathNode::Number("1".into())),
                    den: Box::new(MathNode::Number("2".into())),
                }),
                den: Box::new(MathNode::Number("3".into())),
            })],
        }]);
        // Tall delimiters should use <path> instead of <text> for the parens
        assert!(
            svg.contains("<path"),
            "tall delimiters should use SVG path rendering: {svg}"
        );
        // Filled paths use fill="currentColor", stroked paths use stroke-linecap
        assert!(
            svg.contains("fill=\"currentColor\"") || svg.contains("stroke-linecap=\"round\""),
            "delimiter paths should be filled or have round linecap: {svg}"
        );
    }

    #[test]
    fn display_mode_fraction_svg() {
        let svg = render_display(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(svg.contains("<rect"), "display fraction should have bar");
        assert!(svg.contains("1"));
        assert!(svg.contains("2"));
    }

    #[test]
    fn custom_options_color() {
        let expr = MathExpr::inline(vec![MathNode::Number("5".into())]);
        let options = RenderOptions {
            fg_color: "#ff0000".into(),
            ..RenderOptions::default()
        };
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let svg = emit_svg(&root, &options);
        assert!(svg.contains("#ff0000"), "custom color should appear in SVG");
    }

    #[test]
    fn custom_options_font_family() {
        let expr = MathExpr::inline(vec![MathNode::Number("5".into())]);
        let options = RenderOptions {
            font_family: "monospace".into(),
            ..RenderOptions::default()
        };
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let svg = emit_svg(&root, &options);
        assert!(
            svg.contains("monospace"),
            "custom font family should appear in SVG"
        );
    }

    #[test]
    fn multiple_xml_special_chars() {
        let svg = render(vec![MathNode::Symbol(SymbolKind::Relation, "&".into())]);
        assert!(svg.contains("&amp;"), "& should be escaped");
    }

    #[test]
    fn svg_vertical_align() {
        let svg = render(vec![MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Number("2".into())),
        }]);
        assert!(
            svg.contains("vertical-align"),
            "SVG should have vertical-align for inline"
        );
    }

    #[test]
    fn text_node_svg() {
        let svg = render(vec![MathNode::Text(
            crate::ast::TextStyle::Normal,
            "hello".into(),
        )]);
        assert!(svg.contains("h"));
        assert!(svg.contains("e"));
        assert!(svg.contains("l"));
        assert!(svg.contains("o"));
    }

    #[test]
    fn nary_sum_svg() {
        let svg = render_display(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: crate::ast::LimitStyle::Default,
        }]);
        assert!(svg.contains("<text"));
        // Should have multiple text elements (operator, limits, body)
        let text_count = svg.matches("<text").count();
        assert!(
            text_count >= 3,
            "nary should have at least 3 text elements, got {text_count}"
        );
    }

    #[test]
    fn arena_emit_matches_heap_simple() {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::layout::LayoutBuilder;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2 + 1", &config).unwrap();
        let options = RenderOptions::default();

        // Heap path
        let heap_root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &options);
        let heap_svg = emit_svg(&heap_root, &options);

        // Arena path
        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Text, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_svg = emit_svg_arena(&arena, arena_root, &options);

        assert_eq!(heap_svg, arena_svg, "arena emit must match heap emit");
    }

    #[test]
    fn arena_emit_matches_heap_quadratic() {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::layout::LayoutBuilder;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            &config,
        )
        .unwrap();
        let options = RenderOptions::default();

        let heap_root = layout_expr(&result.expr.to_math_expr(), MathStyle::Display, &options);
        let heap_svg = emit_svg(&heap_root, &options);

        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_svg = emit_svg_arena(&arena, arena_root, &options);

        assert_eq!(heap_svg, arena_svg);
    }

    #[test]
    fn push_f3_handles_nan() {
        let mut buf = String::new();
        push_f3(&mut buf, f64::NAN);
        assert_eq!(buf, "0.000");
    }

    #[test]
    fn push_f3_handles_infinity() {
        let mut buf = String::new();
        push_f3(&mut buf, f64::INFINITY);
        assert_eq!(buf, "0.000");
    }

    #[test]
    fn push_f3_handles_neg_infinity() {
        let mut buf = String::new();
        push_f3(&mut buf, f64::NEG_INFINITY);
        assert_eq!(buf, "0.000");
    }

    #[test]
    fn push_f3_normal_values() {
        let mut buf = String::new();
        push_f3(&mut buf, 1.23456);
        assert_eq!(buf, "1.235");

        buf.clear();
        push_f3(&mut buf, -2.5);
        assert_eq!(buf, "-2.500");

        buf.clear();
        push_f3(&mut buf, 0.0);
        assert_eq!(buf, "0.000");
    }

    #[test]
    fn push_f3_large_values_no_overflow() {
        // Values large enough to overflow u64 when multiplied by 1000
        let mut buf = String::new();
        push_f3(&mut buf, 1e16);
        // Should produce a valid number, not corrupt output
        let parsed: f64 = buf.parse().expect("should be a valid number");
        assert!(parsed > 0.0, "should be positive");

        buf.clear();
        push_f3(&mut buf, 1e18);
        let parsed: f64 = buf.parse().expect("should be a valid number");
        assert!(parsed > 0.0, "should be positive");
    }
}
