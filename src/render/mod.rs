//! TeX-style layout and rendering: turns the AST into positioned boxes and emits
//! SVG (and, with the `raster` feature, PNG/JPEG) output.

#[doc(hidden)]
pub mod arena;
// `#[allow(dead_code)]`: these modules were previously `pub` and reachable as
// API; now `pub(crate)` some helpers are unused internally but kept for the
// render engine's completeness.
#[allow(dead_code)]
pub(crate) mod delimiter_paths;
#[doc(hidden)]
pub mod emit;
#[cfg(feature = "render")]
#[allow(dead_code)]
pub(crate) mod font_embed;
#[cfg(feature = "render")]
#[allow(dead_code)]
pub(crate) mod fonts_registry;
pub(crate) mod glyph_variants;
#[doc(hidden)]
pub mod layout;
pub(crate) mod math_table;
#[allow(dead_code)]
pub(crate) mod metrics;
#[allow(dead_code)]
pub(crate) mod params;
pub(crate) mod path_writer;
#[cfg(feature = "raster")]
pub mod raster;
pub mod spacing;
pub mod style;
#[cfg(feature = "render")]
pub(crate) mod subset;
#[cfg(feature = "render")]
pub(crate) mod svg_writer;

/// Embedded Latin Modern Math font data, shared between font-embed and raster features.
pub(crate) static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

use std::cell::RefCell;

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};
use crate::error::Result;

thread_local! {
    /// Per-thread reusable layout arena for the cold-by-default compact SVG
    /// entry points. Reusing it lets the *default* public API capture the
    /// warm-arena allocation-reuse benefit (the layout pass clears the arena on
    /// entry, so reuse is always correct and output is byte-identical — guarded
    /// by `render_to_svg_compact_arena_matches_heap`). A `try_borrow_mut`
    /// fallback keeps the API correct even under (currently impossible)
    /// re-entrant calls.
    static LAYOUT_ARENA_POOL: RefCell<arena::LayoutArena> =
        RefCell::new(arena::LayoutArena::new());
}

/// Run `f` with the pooled per-thread layout arena, falling back to a fresh
/// arena if the pool is already borrowed (re-entrancy guard). Generic over the
/// result type so both the SVG (`String`) and raster (`Vec<u8>`) entry points
/// can share one pool.
pub(crate) fn with_pooled_arena<T, F>(f: F) -> Result<T>
where
    F: FnOnce(&mut arena::LayoutArena) -> Result<T>,
{
    LAYOUT_ARENA_POOL.with(|cell| match cell.try_borrow_mut() {
        Ok(mut arena) => f(&mut arena),
        Err(_) => {
            let mut arena = arena::LayoutArena::new();
            f(&mut arena)
        },
    })
}

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

/// Render a compact math expression to an SVG string.
///
/// Uses the arena-based layout engine, reusing a per-thread pooled
/// [`arena::LayoutArena`] across calls so the default path captures the
/// allocation-reuse benefit (rather than allocating a fresh cold arena each
/// call). Output is identical to the heap path, guarded by
/// `render_to_svg_compact_arena_matches_heap`. Callers that want full control
/// over arena lifetime (e.g. cross-thread batching) can manage their own arena
/// via [`render_to_svg_compact_arena`].
pub fn render_to_svg_compact(expr: &CompactExpr, options: &RenderOptions) -> Result<String> {
    with_pooled_arena(|layout_arena| render_to_svg_compact_arena(expr, options, layout_arena))
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
#[cfg(feature = "render")]
pub fn render_to_svg_standalone(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    crate::ast::validate_ast_depth(expr)?;
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    let chars = subset::collect_chars(&root);
    let mut glyphs = subset::build_glyph_set(&chars);
    let variant_ids = subset::collect_variants(&root);
    subset::add_variants(&mut glyphs, &variant_ids);
    Ok(svg_writer::emit_svg_with_fonts(&root, options, &glyphs))
}

/// Render a compact expression to a standalone SVG with embedded font glyph paths.
///
/// Uses the arena-based layout engine (faster than the heap path, identical
/// output), reusing a per-thread pooled [`arena::LayoutArena`] across calls.
/// Callers that want explicit arena control can use
/// [`render_to_svg_standalone_compact_arena`].
#[cfg(feature = "render")]
pub fn render_to_svg_standalone_compact(
    expr: &CompactExpr,
    options: &RenderOptions,
) -> Result<String> {
    with_pooled_arena(|layout_arena| {
        render_to_svg_standalone_compact_arena(expr, options, layout_arena)
    })
}

/// Render a compact expression to standalone SVG using arena-based layout.
#[cfg(feature = "render")]
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
    let mut glyphs = subset::build_glyph_set(&chars);
    let variant_ids = subset::collect_variants_arena(layout_arena, root);
    subset::add_variants(&mut glyphs, &variant_ids);
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

#[cfg(all(test, feature = "render"))]
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
        assert!(
            !svg.contains("<text"),
            "standalone SVG should not have <text>"
        );
    }

    #[test]
    fn render_to_svg_standalone_compact_matches_heap() {
        let config = crate::error::Config::default();
        let result =
            crate::reader::latex::LatexReader::parse_compact("\\frac{a}{b} + \\sqrt{c}", &config)
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
