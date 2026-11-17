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
    /// Base font size in pixels. Must be finite and positive (default: 16.0).
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
    /// Validate the numeric options used to turn layout units into pixels.
    ///
    /// Layout itself works in em units, so an invalid font size would only be
    /// discovered while emitting output. Rejecting it at the public rendering
    /// boundary keeps all SVG entry points from producing malformed dimensions
    /// (and gives raster rendering the same behavior).
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.font_size_px.is_finite() || self.font_size_px <= 0.0 {
            return Err(crate::error::Error::Render {
                message: "font_size_px must be finite and greater than zero".into(),
            });
        }
        Ok(())
    }

    /// Resolve whether display mode is active, considering expr metadata.
    #[must_use]
    pub fn is_display(&self, expr: &MathExpr) -> bool {
        self.display_mode
            .unwrap_or(expr.display == DisplayStyle::Block)
    }
}

/// Render a math expression to an SVG string.
pub fn render_to_svg(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    options.validate()?;
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
    render_compact_svg(expr, options, None, emit::emit_svg_arena)
}

/// Render a compact expression to SVG using arena-based layout (zero heap alloc in layout pass).
pub fn render_to_svg_compact_arena(
    expr: &CompactExpr,
    options: &RenderOptions,
    layout_arena: &mut arena::LayoutArena,
) -> Result<String> {
    render_compact_svg(expr, options, Some(layout_arena), emit::emit_svg_arena)
}

fn render_compact_svg(
    expr: &CompactExpr,
    options: &RenderOptions,
    arena: Option<&mut arena::LayoutArena>,
    emit: impl FnOnce(&arena::LayoutArena, arena::LayoutBoxId, &RenderOptions) -> String,
) -> Result<String> {
    options.validate()?;
    crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let render = |arena: &mut arena::LayoutArena| {
        arena.clear();
        let root = layout::LayoutBuilder::new(style, options).layout(arena, expr);
        Ok(emit(arena, root, options))
    };
    match arena {
        Some(arena) => render(arena),
        None => with_pooled_arena(render),
    }
}

/// Render a math expression to a standalone SVG with embedded font glyph paths.
///
/// Unlike `render_to_svg`, the output does not require client-side fonts —
/// glyph outlines from Latin Modern Math are embedded as `<path>` elements.
#[cfg(feature = "render")]
pub fn render_to_svg_standalone(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    options.validate()?;
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
    render_compact_svg(expr, options, None, emit_standalone_svg)
}

/// Render a compact expression to standalone SVG using arena-based layout.
#[cfg(feature = "render")]
pub fn render_to_svg_standalone_compact_arena(
    expr: &CompactExpr,
    options: &RenderOptions,
    layout_arena: &mut arena::LayoutArena,
) -> Result<String> {
    render_compact_svg(expr, options, Some(layout_arena), emit_standalone_svg)
}

#[cfg(feature = "render")]
fn emit_standalone_svg(
    layout_arena: &arena::LayoutArena,
    root: arena::LayoutBoxId,
    options: &RenderOptions,
) -> String {
    let chars = subset::collect_chars_arena(layout_arena, root);
    let mut glyphs = subset::build_glyph_set(&chars);
    let variant_ids = subset::collect_variants_arena(layout_arena, root);
    subset::add_variants(&mut glyphs, &variant_ids);
    svg_writer::emit_svg_with_fonts_arena(layout_arena, root, options, &glyphs)
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

    #[test]
    fn actuarial_angle_renders_top_and_right_rules_on_both_layout_paths() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result =
            crate::reader::latex::LatexReader::parse_compact(r"\angl{x}", &config).unwrap();
        let options = RenderOptions::default();
        let heap = render_to_svg_compact(&result.expr, &options).unwrap();
        let mut arena = arena::LayoutArena::new();
        let compact = render_to_svg_compact_arena(&result.expr, &options, &mut arena).unwrap();
        assert_eq!(heap, compact);
        assert!(
            heap.matches("<rect").count() >= 2,
            "missing connected top/right rules: {heap}"
        );
    }

    #[test]
    fn svg_entry_points_reject_invalid_font_sizes() {
        use crate::reader::Reader;

        let config = crate::error::Config::default();
        let compact = crate::reader::latex::LatexReader::parse_compact("x", &config).unwrap();
        let expr = compact.expr.to_math_expr();

        for font_size_px in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let options = RenderOptions {
                font_size_px,
                ..RenderOptions::default()
            };
            let outputs = [
                render_to_svg(&expr, &options),
                render_to_svg_compact(&compact.expr, &options),
                render_to_svg_standalone(&expr, &options),
                render_to_svg_standalone_compact(&compact.expr, &options),
            ];
            for output in outputs {
                match output {
                    Err(crate::error::Error::Render { message }) => {
                        assert!(
                            message.contains("font_size_px"),
                            "unexpected validation error: {message}"
                        );
                    },
                    other => panic!("invalid font size should be rejected: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn invalid_svg_options_preserve_the_callers_layout() {
        use crate::reader::Reader;
        let expr = crate::reader::latex::LatexReader::parse_compact("x", &Default::default())
            .unwrap()
            .expr;
        let mut arena = arena::LayoutArena::new();
        let previous = arena.alloc(arena::ArenaLayoutBox::kern(7.0));
        let options = RenderOptions {
            font_size_px: 0.0,
            ..Default::default()
        };

        assert!(render_to_svg_compact_arena(&expr, &options, &mut arena).is_err());
        assert!(render_to_svg_standalone_compact_arena(&expr, &options, &mut arena).is_err());
        assert_eq!(arena.get(previous).width, 7.0);
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
