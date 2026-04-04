/// SVG emission: converts a LayoutBox tree into an SVG string.
use std::fmt::Write;

use super::RenderOptions;
use super::layout::{BoxKind, LayoutBox};

/// Wrapper that escapes a string for safe use in XML/SVG attributes.
/// Prevents attribute injection via `"`, `<`, `>`, `&`, `'` in user-controlled values.
pub(crate) struct XmlAttrEscape<'a>(pub(crate) &'a str);

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

/// Emit the final SVG string from a laid-out box tree.
pub fn emit_svg(root: &LayoutBox, options: &RenderOptions) -> String {
    let em = options.font_size_px;
    let width_px = root.width * em;
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;

    let mut svg = String::with_capacity(4096);

    // SVG header
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.3}" height="{height:.3}" viewBox="0 0 {width:.3} {height:.3}" style="vertical-align: -{depth:.3}px">"#,
        width = width_px,
        height = height_px,
        depth = depth_px,
    );

    // Render content
    // Origin in SVG: (0, root.height * em) corresponds to the baseline at x=0
    let baseline_y = root.height * em;
    emit_box(&mut svg, root, 0.0, baseline_y, em, options);

    svg.push_str("</svg>");
    svg
}

fn emit_box(
    svg: &mut String,
    layout: &LayoutBox,
    x: f64,
    y: f64,
    em: f64,
    options: &RenderOptions,
) {
    match &layout.kind {
        BoxKind::Glyph { ch, size } => {
            let font_size = em * size;
            // Write directly to buffer — avoids intermediate String from format!+escape_xml_char
            let _ = write!(
                svg,
                r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" fill="{color}">"#,
                x = x,
                y = y,
                fs = font_size,
                ff = XmlAttrEscape(&options.font_family),
                color = XmlAttrEscape(&options.fg_color),
            );
            write_escaped_char(svg, *ch);
            svg.push_str("</text>");
        },

        BoxKind::HBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                // dy is positive = up in TeX, but SVG y-down, so subtract
                let cy = y - child.dy * em;
                emit_box(svg, &child.layout, cx, cy, em, options);
            }
        },

        BoxKind::VBox { children } => {
            for child in children {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box(svg, &child.layout, cx, cy, em, options);
            }
        },

        BoxKind::Rule { thickness } => {
            let th_px = thickness * em;
            let w_px = layout.width * em;
            // Center the rule vertically at the given y
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
        },

        BoxKind::Kern => {
            // Invisible — no output
        },

        BoxKind::Delimiter { ch, target_height } => {
            let font_size = target_height * em;
            let _ = write!(
                svg,
                r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" fill="{color}">"#,
                x = x,
                y = y,
                fs = font_size,
                ff = XmlAttrEscape(&options.font_family),
                color = XmlAttrEscape(&options.fg_color),
            );
            write_escaped_char(svg, *ch);
            svg.push_str("</text>");
        },

        BoxKind::SvgPath(path) => {
            // Transform path coordinates from em to px
            // The path is in local coordinates where (0,0) is top-left
            // We need to translate to the actual position
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
        },

        BoxKind::Color { color, child } => {
            // Use the color as a group wrapper
            let _ = write!(
                svg,
                r#"<g fill="{color}" stroke="{color}">"#,
                color = XmlAttrEscape(color)
            );
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box(svg, &child.layout, cx, cy, em, options);
            svg.push_str("</g>");
        },

        BoxKind::Phantom => {
            // Invisible — takes space but doesn't render
        },

        BoxKind::Boxed { child, padding: _ } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;

            // Border rectangle
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{color}" fill="none" stroke-width="1"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                color = XmlAttrEscape(&options.fg_color),
            );

            // Content
            let cx = x + child.dx * em;
            let cy = y - child.dy * em;
            emit_box(svg, &child.layout, cx, cy, em, options);
        },
    }
}

// ── Arena-based SVG emission ─────────────────────────────────────────

use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};

/// Emit the final SVG string from an arena-based layout.
/// Produces byte-identical output to `emit_svg` for the same expression.
pub fn emit_svg_arena(
    arena: &LayoutArena,
    root: LayoutBoxId,
    options: &RenderOptions,
) -> String {
    let root_box = arena.get(root);
    let em = options.font_size_px;
    let width_px = root_box.width * em;
    let height_px = (root_box.height + root_box.depth) * em;
    let depth_px = root_box.depth * em;

    let mut svg = String::with_capacity(4096);

    // SVG header — identical format to emit_svg
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.3}" height="{height:.3}" viewBox="0 0 {width:.3} {height:.3}" style="vertical-align: -{depth:.3}px">"#,
        width = width_px,
        height = height_px,
        depth = depth_px,
    );

    // Render content
    let baseline_y = root_box.height * em;
    emit_box_arena(&mut svg, arena, root, 0.0, baseline_y, em, options);

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
    options: &RenderOptions,
) {
    let layout = arena.get(id);
    match &layout.kind {
        ArenaBoxKind::Glyph { ch, size } => {
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
            write_escaped_char(svg, *ch);
            svg.push_str("</text>");
        },

        ArenaBoxKind::HBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena(svg, arena, child.id, cx, cy, em, options);
            }
        },

        ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                let cx = x + child.dx * em;
                let cy = y - child.dy * em;
                emit_box_arena(svg, arena, child.id, cx, cy, em, options);
            }
        },

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
        },

        ArenaBoxKind::Kern => {
            // Invisible — no output
        },

        ArenaBoxKind::Delimiter { ch, target_height } => {
            let font_size = target_height * em;
            let _ = write!(
                svg,
                r#"<text x="{x:.3}" y="{y:.3}" font-size="{fs:.3}" font-family="{ff}" fill="{color}">"#,
                x = x,
                y = y,
                fs = font_size,
                ff = XmlAttrEscape(&options.font_family),
                color = XmlAttrEscape(&options.fg_color),
            );
            write_escaped_char(svg, *ch);
            svg.push_str("</text>");
        },

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
        },

        ArenaBoxKind::Color { color, child } => {
            let _ = write!(
                svg,
                r#"<g fill="{color}" stroke="{color}">"#,
                color = XmlAttrEscape(color)
            );
            // Color child is rendered at same position (no dx/dy offset)
            emit_box_arena(svg, arena, *child, x, y, em, options);
            svg.push_str("</g>");
        },

        ArenaBoxKind::Phantom => {
            // Invisible — takes space but doesn't render
        },

        ArenaBoxKind::Boxed { child, padding } => {
            let w_px = layout.width * em;
            let h_px = (layout.height + layout.depth) * em;
            let ry = y - layout.height * em;

            // Border rectangle
            let _ = write!(
                svg,
                r#"<rect x="{x:.3}" y="{ry:.3}" width="{w:.3}" height="{h:.3}" stroke="{color}" fill="none" stroke-width="1"/>"#,
                x = x,
                ry = ry,
                w = w_px,
                h = h_px,
                color = XmlAttrEscape(&options.fg_color),
            );

            // Content — arena version stores child without position offset,
            // so we must apply padding here (heap version stored dx=padding in PositionedBox)
            emit_box_arena(svg, arena, *child, x + padding * em, y, em, options);
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
    use crate::render::layout::{PositionedBox, layout_expr, layout_expr_c};
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
        let svg = render(vec![MathNode::Boxed(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
        assert!(
            svg.contains("stroke="),
            "boxed should have a stroked border rect"
        );
        assert!(svg.contains("<rect"), "boxed should contain rect element");
    }

    #[test]
    fn phantom_svg_no_visible_content() {
        let svg = render(vec![MathNode::Phantom(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
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
        let heap_root = layout_expr_c(&result.expr, MathStyle::Text, &options);
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
        ).unwrap();
        let options = RenderOptions::default();

        let heap_root = layout_expr_c(&result.expr, MathStyle::Display, &options);
        let heap_svg = emit_svg(&heap_root, &options);

        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_svg = emit_svg_arena(&arena, arena_root, &options);

        assert_eq!(heap_svg, arena_svg);
    }
}
