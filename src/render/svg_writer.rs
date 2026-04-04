//! SVG emission with embedded font glyph paths.
//!
//! Produces standalone SVGs that render without client-side fonts.
//! Glyph outlines are defined once in `<defs>` and referenced via `<use>`.

use std::fmt::Write;

use super::RenderOptions;
use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
use super::layout::{BoxKind, LayoutBox};
use super::subset::GlyphSet;

/// Wrapper that escapes a string for safe use in XML/SVG attributes.
struct XmlAttrEscape<'a>(&'a str);

impl std::fmt::Display for XmlAttrEscape<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '&' => f.write_str("&amp;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&apos;")?,
                _ => f.write_char(ch)?,
            }
        }
        Ok(())
    }
}

/// Write an XML-escaped character directly to the buffer.
fn write_escaped_char(out: &mut String, ch: char) {
    match ch {
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '&' => out.push_str("&amp;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&apos;"),
        _ => out.push(ch),
    }
}

// ── Heap-based emission ──────────────────────────────────────────────

/// Emit SVG with embedded font paths from a heap-based layout tree.
pub fn emit_svg_with_fonts(
    root: &LayoutBox,
    options: &RenderOptions,
    glyphs: &GlyphSet,
) -> String {
    let em = options.font_size_px;
    let width_px = root.width * em;
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;

    let mut svg = String::with_capacity(4096);

    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.3}" height="{height:.3}" viewBox="0 0 {width:.3} {height:.3}" style="vertical-align: -{depth:.3}px">"#,
        width = width_px,
        height = height_px,
        depth = depth_px,
    );

    emit_defs(&mut svg, glyphs);

    let baseline_y = root.height * em;
    emit_box_embedded(&mut svg, root, 0.0, baseline_y, em, options, glyphs);

    svg.push_str("</svg>");
    svg
}

/// Write `<defs>` block with glyph path definitions, sorted by codepoint for determinism.
fn emit_defs(svg: &mut String, glyphs: &GlyphSet) {
    if glyphs.glyphs.is_empty() {
        return;
    }

    svg.push_str("<defs>");

    let mut entries: Vec<_> = glyphs.glyphs.iter().collect();
    entries.sort_by_key(|(ch, _)| **ch as u32);

    for (ch, outline) in entries {
        let _ = write!(
            svg,
            r#"<path id="g{}" d="{}"/>"#,
            *ch as u32,
            outline.path_data,
        );
    }

    svg.push_str("</defs>");
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
        BoxKind::Glyph { ch, size } => {
            emit_glyph(svg, *ch, *size, x, y, em, options, glyphs);
        }

        BoxKind::HBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
            }
        }

        BoxKind::VBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
            }
        }

        BoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            let ry = y - th_px / 2.0;
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" fill="{color}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = th_px,
                color = XmlAttrEscape(&options.fg_color),
            );
        }

        BoxKind::Kern => {}

        BoxKind::Delimiter { ch, target_height } => {
            emit_glyph(svg, *ch, *target_height, x, y, em, options, glyphs);
        }

        BoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            let _ = write!(
                svg,
                r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="1" fill="none"/></g>"#,
                tx = tx,
                ty = ty,
                s = em,
                path = XmlAttrEscape(path),
                color = XmlAttrEscape(&options.fg_color),
            );
        }

        BoxKind::Color { color, child } => {
            let _ = write!(
                svg,
                r#"<g fill="{color}" stroke="{color}">"#,
                color = XmlAttrEscape(color)
            );
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
            svg.push_str("</g>");
        }

        BoxKind::Phantom => {}

        BoxKind::Boxed { child, padding: _ } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{color}" fill="none" stroke-width="1"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                color = XmlAttrEscape(&options.fg_color),
            );
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box_embedded(svg, &child.layout, cx, cy, em, options, glyphs);
        }
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
) {
    if glyphs.glyphs.contains_key(&ch) {
        let scale = size * em / glyphs.font_info.units_per_em;
        let _ = write!(
            svg,
            r##"<use href="#g{cp}" transform="translate({x:.3},{y:.3}) scale({s:.6})" fill="{color}"/>"##,
            cp = ch as u32,
            x = x,
            y = y,
            s = scale,
            color = XmlAttrEscape(&options.fg_color),
        );
    } else {
        // Fallback: <text> element (same as emit.rs)
        let font_size = em * size;
        let _ = write!(
            svg,
            r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" fill="{color}">"#,
            x = x,
            y = y,
            fs = font_size,
            ff = XmlAttrEscape(&options.font_family),
            color = XmlAttrEscape(&options.fg_color),
        );
        write_escaped_char(svg, ch);
        svg.push_str("</text>");
    }
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
    let width_px = root_box.width * em;
    let height_px = (root_box.height + root_box.depth) * em;
    let depth_px = root_box.depth * em;

    let mut svg = String::with_capacity(4096);

    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.3}" height="{height:.3}" viewBox="0 0 {width:.3} {height:.3}" style="vertical-align: -{depth:.3}px">"#,
        width = width_px,
        height = height_px,
        depth = depth_px,
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
        ArenaBoxKind::Glyph { ch, size } => {
            emit_glyph(svg, *ch, *size, x, y, em, options, glyphs);
        }

        ArenaBoxKind::HBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena_embedded(svg, arena, child.id, cx, cy, em, options, glyphs);
            }
        }

        ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena_embedded(svg, arena, child.id, cx, cy, em, options, glyphs);
            }
        }

        ArenaBoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            let ry = y - th_px / 2.0;
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" fill="{color}"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = th_px,
                color = XmlAttrEscape(&options.fg_color),
            );
        }

        ArenaBoxKind::Kern => {}

        ArenaBoxKind::Delimiter { ch, target_height } => {
            emit_glyph(svg, *ch, *target_height, x, y, em, options, glyphs);
        }

        ArenaBoxKind::SvgPath(path) => {
            let tx = x;
            let ty = y - layout.height * em;
            let _ = write!(
                svg,
                r#"<g transform="translate({tx:.3},{ty:.3}) scale({s:.3})"><path d="{path}" stroke="{color}" stroke-width="1" fill="none"/></g>"#,
                tx = tx,
                ty = ty,
                s = em,
                path = XmlAttrEscape(path),
                color = XmlAttrEscape(&options.fg_color),
            );
        }

        ArenaBoxKind::Color { color, child } => {
            let _ = write!(
                svg,
                r#"<g fill="{color}" stroke="{color}">"#,
                color = XmlAttrEscape(color)
            );
            emit_box_arena_embedded(svg, arena, *child, x, y, em, options, glyphs);
            svg.push_str("</g>");
        }

        ArenaBoxKind::Phantom => {}

        ArenaBoxKind::Boxed { child, padding } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{color}" fill="none" stroke-width="1"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                color = XmlAttrEscape(&options.fg_color),
            );
            emit_box_arena_embedded(svg, arena, *child, x + padding * em, y, em, options, glyphs);
        }
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

    static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

    fn render_embedded(latex_nodes: Vec<MathNode>) -> String {
        let expr = MathExpr::inline(latex_nodes);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let chars = collect_chars(&root);
        let glyphs = build_glyph_set(FONT_DATA, &chars).unwrap();
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
        assert!(
            svg.contains("<use"),
            "should use <use> to reference glyph"
        );
        assert!(
            svg.contains("href=\"#g120\""),
            "should reference glyph definition"
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
        assert!(svg.contains("<rect"), "fraction bar should still use <rect>");
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
        assert_eq!(
            defs_section.matches("g120").count(),
            1,
            "glyph 'x' should be defined exactly once in defs"
        );
        let body = &svg[defs_end..];
        assert_eq!(
            body.matches("#g120").count(),
            2,
            "'x' should be referenced twice"
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
        let glyphs = build_glyph_set(FONT_DATA, &chars).unwrap();
        let svg = emit_svg_with_fonts(&root, &options, &glyphs);
        assert!(svg.contains("fill=\"#ff0000\""));
    }

    #[test]
    fn svg_use_has_transform_with_scale() {
        let svg = render_embedded(vec![MathNode::Identifier("x".into())]);
        assert!(
            svg.contains("scale("),
            "<use> should have scale transform"
        );
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
        let glyphs = build_glyph_set(FONT_DATA, &empty_chars).unwrap();
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
        let result = crate::reader::latex::LatexReader::parse_compact(
            "x^2 + 1",
            &config,
        ).unwrap();
        let options = RenderOptions::default();

        // Heap path
        let heap_root = layout_expr_c(&result.expr, MathStyle::Text, &options);
        let heap_chars = collect_chars(&heap_root);
        let heap_glyphs = build_glyph_set(FONT_DATA, &heap_chars).unwrap();
        let heap_svg = emit_svg_with_fonts(&heap_root, &options, &heap_glyphs);

        // Arena path
        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Text, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_chars = collect_chars_arena(&arena, arena_root);
        let arena_glyphs = build_glyph_set(FONT_DATA, &arena_chars).unwrap();
        let arena_svg = emit_svg_with_fonts_arena(&arena, arena_root, &options, &arena_glyphs);

        assert_eq!(heap_svg, arena_svg, "arena must match heap");
    }
}
