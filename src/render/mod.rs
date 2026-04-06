pub mod arena;
pub mod delimiter_paths;
pub mod emit;
pub mod layout;
pub mod metrics;
pub mod params;
#[cfg(feature = "raster")]
pub mod raster;
pub mod spacing;
pub mod style;
#[cfg(feature = "font-embed")]
pub mod font_embed;
#[cfg(feature = "font-embed")]
pub mod subset;
#[cfg(feature = "font-embed")]
pub(crate) mod svg_writer;

/// Embedded Latin Modern Math font data, shared between font-embed and raster features.
#[cfg(any(feature = "font-embed", feature = "raster"))]
pub(crate) static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};
use crate::error::Result;

/// Options controlling SVG rendering output.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Base font size in pixels (default: 16.0).
    pub font_size_px: f64,
    /// Whether to render in display mode. If None, uses MathExpr.display.
    pub display_mode: Option<bool>,
    /// Foreground color for text and rules (default: "currentColor").
    pub fg_color: String,
    /// CSS font-family for text elements (default: "serif").
    pub font_family: String,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            display_mode: None,
            fg_color: "currentColor".into(),
            font_family: "KaTeX_Main, Latin Modern Math, STIX Two Math, Cambria Math, serif".into(),
        }
    }
}

impl RenderOptions {
    /// Resolve whether display mode is active, considering expr metadata.
    #[must_use]
    pub fn is_display(&self, expr: &MathExpr) -> bool {
        self.display_mode
            .unwrap_or(expr.display == DisplayStyle::Block)
    }
}

/// Render a math expression to an SVG string.
pub fn render_to_svg(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    crate::ast::validate_ast_depth(expr)?;
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    Ok(emit::emit_svg(&root, options))
}

/// Render a compact math expression to an SVG string (arena-based, no MathExpr bridge).
pub fn render_to_svg_compact(expr: &CompactExpr, options: &RenderOptions) -> Result<String> {
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr_c(expr, style, options);
    Ok(emit::emit_svg(&root, options))
}

/// Render a compact expression to SVG using arena-based layout (zero heap alloc in layout pass).
pub fn render_to_svg_compact_arena(
    expr: &CompactExpr,
    options: &RenderOptions,
    layout_arena: &mut arena::LayoutArena,
) -> Result<String> {
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let builder = layout::LayoutBuilder::new(style, options);
    layout_arena.clear();
    let root = builder.layout(layout_arena, expr);
    Ok(emit::emit_svg_arena(layout_arena, root, options))
}

/// Render a math expression to a standalone SVG with embedded font glyph paths.
///
/// Unlike `render_to_svg`, the output does not require client-side fonts —
/// glyph outlines from Latin Modern Math are embedded as `<path>` elements.
#[cfg(feature = "font-embed")]
pub fn render_to_svg_standalone(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    crate::ast::validate_ast_depth(expr)?;
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    let chars = subset::collect_chars(&root);
    let glyphs = subset::build_glyph_set(FONT_DATA, &chars)
        .expect("embedded font data must be valid");
    Ok(svg_writer::emit_svg_with_fonts(&root, options, &glyphs))
}

/// Render a compact expression to a standalone SVG with embedded font glyph paths.
#[cfg(feature = "font-embed")]
pub fn render_to_svg_standalone_compact(
    expr: &CompactExpr,
    options: &RenderOptions,
) -> Result<String> {
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr_c(expr, style, options);
    let chars = subset::collect_chars(&root);
    let glyphs = subset::build_glyph_set(FONT_DATA, &chars)
        .expect("embedded font data must be valid");
    Ok(svg_writer::emit_svg_with_fonts(&root, options, &glyphs))
}

/// Render a compact expression to standalone SVG using arena-based layout.
#[cfg(feature = "font-embed")]
pub fn render_to_svg_standalone_compact_arena(
    expr: &CompactExpr,
    options: &RenderOptions,
    layout_arena: &mut arena::LayoutArena,
) -> Result<String> {
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let builder = layout::LayoutBuilder::new(style, options);
    layout_arena.clear();
    let root = builder.layout(layout_arena, expr);
    let chars = subset::collect_chars_arena(layout_arena, root);
    let glyphs = subset::build_glyph_set(FONT_DATA, &chars)
        .expect("embedded font data must be valid");
    Ok(svg_writer::emit_svg_with_fonts_arena(
        layout_arena,
        root,
        options,
        &glyphs,
    ))
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn render_to_svg_compact_arena_matches_heap() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            &config,
        )
        .unwrap();
        let options = RenderOptions::default();

        let heap_svg = render_to_svg_compact(&result.expr, &options).unwrap();

        let mut layout_arena = arena::LayoutArena::new();
        let arena_svg =
            render_to_svg_compact_arena(&result.expr, &options, &mut layout_arena).unwrap();

        assert_eq!(heap_svg, arena_svg);
    }
}

#[cfg(all(test, feature = "font-embed"))]
mod font_embed_integration {
    use super::*;
    use crate::reader::Reader;

    #[test]
    fn render_to_svg_standalone_produces_embedded_svg() {
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse("x^2 + 1", &config).unwrap();
        let options = RenderOptions::default();
        let svg = render_to_svg_standalone(&result.expr, &options).unwrap();
        assert!(svg.contains("<defs>"), "standalone SVG should have defs");
        assert!(svg.contains("<use"), "standalone SVG should use <use>");
        assert!(svg.contains("<path"), "standalone SVG should have paths");
        assert!(!svg.contains("<text"), "standalone SVG should not have <text>");
    }

    #[test]
    fn render_to_svg_standalone_compact_matches_heap() {
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(
            "\\frac{a}{b} + \\sqrt{c}",
            &config,
        )
        .unwrap();
        let options = RenderOptions::default();

        // Heap path via MathExpr
        let expr = result.expr.to_math_expr();
        let heap_svg = render_to_svg_standalone(&expr, &options).unwrap();

        // Compact path
        let compact_svg = render_to_svg_standalone_compact(&result.expr, &options).unwrap();

        assert_eq!(heap_svg, compact_svg);
    }
}
