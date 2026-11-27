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
#[cfg(feature = "render")]
pub(crate) mod glyph_fallback;
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
pub use crate::ast::style;
#[cfg(feature = "render")]
pub(crate) mod subset;
#[cfg(feature = "render")]
pub(crate) mod svg_writer;

/// Embedded Latin Modern Math font data, shared between font-embed and raster features.
pub(crate) static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

use std::cell::RefCell;

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};
use crate::error::{Diagnostic, DiagnosticKind, Result};
use std::collections::HashSet;

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

/// Geometry of a prepared render in pixels.
///
/// The dimensions include the small padding used by SVG output. `baseline_px`
/// is measured from the top of that output, and `depth_px` is the distance
/// from the baseline to the bottom padding. `view_box_x_px` is the left edge of
/// the padded SVG view box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderGeometry {
    /// Output width in pixels at the requested font size.
    pub width_px: f64,
    /// Output height in pixels at the requested font size.
    pub height_px: f64,
    /// Baseline position measured from the top of the output.
    pub baseline_px: f64,
    /// Distance from the baseline to the bottom of the output.
    pub depth_px: f64,
    /// Left edge of the padded SVG view box.
    pub view_box_x_px: f64,
}

/// A laid-out expression that can be emitted in several formats.
///
/// Preparing an expression performs validation and layout once. The resulting
/// value can then produce text SVG, standalone SVG, and raster images without
/// traversing the AST and running the layout engine again. Rendering options
/// may change font size, colors, font family, raster scale, background, and
/// image format. If `display_mode` is set, it must agree with the mode used
/// when the layout was prepared.
pub struct PreparedRender {
    layout_arena: arena::LayoutArena,
    root: arena::LayoutBoxId,
    display_mode: bool,
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

/// Prepare a heap expression for repeated rendering.
pub fn prepare_render(expr: &MathExpr, options: &RenderOptions) -> Result<PreparedRender> {
    options.validate()?;
    let compact = CompactExpr::from_math_expr(expr)?;
    prepare_render_compact(&compact, options)
}

/// Prepare a compact expression for repeated rendering.
pub fn prepare_render_compact(
    expr: &CompactExpr,
    options: &RenderOptions,
) -> Result<PreparedRender> {
    options.validate()?;
    crate::ast::validate_compact_for_write(&expr.arena, expr.roots, crate::ast::MAX_AST_DEPTH)?;
    let display_mode = options
        .display_mode
        .unwrap_or(expr.display == DisplayStyle::Block);
    let style = if display_mode {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let mut layout_arena = arena::LayoutArena::new();
    let root = layout::LayoutBuilder::new(style, options).layout(&mut layout_arena, expr);
    Ok(PreparedRender {
        layout_arena,
        root,
        display_mode,
    })
}

impl PreparedRender {
    fn validate_options(&self, options: &RenderOptions) -> Result<()> {
        options.validate()?;
        if let Some(display_mode) = options.display_mode
            && display_mode != self.display_mode
        {
            return Err(crate::error::Error::Render {
                message: "display_mode does not match the prepared layout".into(),
            });
        }
        Ok(())
    }

    fn svg_geometry(&self, options: &RenderOptions) -> Result<RenderGeometry> {
        self.validate_options(options)?;
        let root = self.layout_arena.get(self.root);
        let em = options.font_size_px;
        let (view_x_px, width_px) = emit::svg_x_bounds_px_arena(&self.layout_arena, self.root, em);
        let height_px = (root.height + root.depth) * em;
        let depth_px = root.depth * em;
        let pad = 0.08 * em;
        emit::validate_svg_geometry(
            view_x_px - pad,
            width_px + 2.0 * pad,
            height_px + 2.0 * pad,
            root.height * em + pad,
            depth_px + pad,
        )?;
        Ok(RenderGeometry {
            width_px: width_px + 2.0 * pad,
            height_px: height_px + 2.0 * pad,
            baseline_px: root.height * em + pad,
            depth_px: depth_px + pad,
            view_box_x_px: view_x_px - pad,
        })
    }

    /// Return output geometry at the font size in `options`.
    pub fn geometry(&self, options: &RenderOptions) -> Result<RenderGeometry> {
        self.svg_geometry(options)
    }

    /// Emit a text-based SVG using client-installed fonts.
    pub fn to_svg_text(&self, options: &RenderOptions) -> Result<String> {
        self.svg_geometry(options)?;
        Ok(emit::emit_svg_arena(&self.layout_arena, self.root, options))
    }

    /// Emit a text-based SVG and report glyph fallbacks.
    pub fn to_svg_text_with_diagnostics(
        &self,
        options: &RenderOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        self.svg_geometry(options)?;
        let diagnostics = missing_glyph_diagnostics_arena(&self.layout_arena, self.root);
        Ok((
            emit::emit_svg_arena(&self.layout_arena, self.root, options),
            diagnostics,
        ))
    }

    /// Emit a standalone SVG with embedded glyph outlines.
    #[cfg(feature = "render")]
    pub fn to_svg(&self, options: &RenderOptions) -> Result<String> {
        self.svg_geometry(options)?;
        Ok(emit_standalone_svg(&self.layout_arena, self.root, options))
    }

    /// Emit a standalone SVG and report glyph fallbacks.
    #[cfg(feature = "render")]
    pub fn to_svg_with_diagnostics(
        &self,
        options: &RenderOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        self.svg_geometry(options)?;
        let diagnostics = missing_glyph_diagnostics_arena(&self.layout_arena, self.root);
        Ok((
            emit_standalone_svg(&self.layout_arena, self.root, options),
            diagnostics,
        ))
    }

    /// Emit a raster image using the format and raster settings in `options`.
    #[cfg(feature = "raster")]
    pub fn to_image(&self, options: &raster::RasterOptions) -> Result<Vec<u8>> {
        options.validate()?;
        self.validate_options(&options.render)?;
        raster::rasterize_layout_arena(&self.layout_arena, self.root, options)
    }

    /// Emit a raster image and report missing glyphs and unsupported colors.
    #[cfg(feature = "raster")]
    pub fn to_image_with_diagnostics(
        &self,
        options: &raster::RasterOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        options.validate()?;
        self.validate_options(&options.render)?;
        let diagnostics = raster::raster_diagnostics_arena(
            &self.layout_arena,
            self.root,
            &options.render.fg_color,
        );
        let image = raster::rasterize_layout_arena(&self.layout_arena, self.root, options)?;
        Ok((image, diagnostics))
    }

    /// Emit PNG bytes using the supplied raster settings.
    #[cfg(feature = "raster")]
    pub fn to_png(&self, options: &raster::RasterOptions) -> Result<Vec<u8>> {
        let options = raster::RasterOptions {
            format: raster::ImageFormat::Png,
            ..options.clone()
        };
        self.to_image(&options)
    }

    /// Emit PNG bytes and report missing glyphs and unsupported colors.
    #[cfg(feature = "raster")]
    pub fn to_png_with_diagnostics(
        &self,
        options: &raster::RasterOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        let options = raster::RasterOptions {
            format: raster::ImageFormat::Png,
            ..options.clone()
        };
        self.to_image_with_diagnostics(&options)
    }

    /// Emit JPEG bytes using the supplied raster settings.
    #[cfg(feature = "raster")]
    pub fn to_jpeg(&self, options: &raster::RasterOptions) -> Result<Vec<u8>> {
        let options = raster::RasterOptions {
            format: raster::ImageFormat::Jpeg,
            ..options.clone()
        };
        self.to_image(&options)
    }

    /// Emit JPEG bytes and report missing glyphs and unsupported colors.
    #[cfg(feature = "raster")]
    pub fn to_jpeg_with_diagnostics(
        &self,
        options: &raster::RasterOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        let options = raster::RasterOptions {
            format: raster::ImageFormat::Jpeg,
            ..options.clone()
        };
        self.to_image_with_diagnostics(&options)
    }
}

/// Render a math expression to an SVG string.
pub fn render_to_svg(expr: &MathExpr, options: &RenderOptions) -> Result<String> {
    options.validate()?;
    crate::ast::validate_ast_depth(expr)?;
    // The arena layout looks at nodes, not at what carries their attributes.
    if crate::ast::has_attributes(expr) {
        return render_to_svg_compact(&CompactExpr::from_math_expr(expr)?, options);
    }
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    let em = options.font_size_px;
    let (view_x_px, width_px) = emit::svg_x_bounds_px(&root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;
    let pad = 0.08 * em;
    emit::validate_svg_geometry(
        view_x_px - pad,
        width_px + 2.0 * pad,
        height_px + 2.0 * pad,
        root.height * em + pad,
        depth_px + pad,
    )?;
    Ok(emit::emit_svg(&root, options))
}

/// Render a math expression to an SVG string and report glyph fallbacks.
///
/// The SVG output is identical to [`render_to_svg`]. The diagnostics identify
/// characters that have no embedded glyph, text-face glyph, or vector recipe.
/// Such characters remain visible through a client-font `<text>` fallback in
/// SVG. Raster output represents the same characters with a missing-glyph box.
pub fn render_to_svg_with_diagnostics(
    expr: &MathExpr,
    options: &RenderOptions,
) -> Result<(String, Vec<Diagnostic>)> {
    options.validate()?;
    crate::ast::validate_ast_depth(expr)?;
    if crate::ast::has_attributes(expr) {
        return render_to_svg_compact_with_diagnostics(
            &CompactExpr::from_math_expr(expr)?,
            options,
        );
    }
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    validate_svg_layout(&root, options)?;
    let diagnostics = missing_glyph_diagnostics(&root);
    Ok((emit::emit_svg(&root, options), diagnostics))
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

/// Render a compact math expression to SVG and report glyph fallbacks.
///
/// The SVG output is identical to [`render_to_svg_compact`].
pub fn render_to_svg_compact_with_diagnostics(
    expr: &CompactExpr,
    options: &RenderOptions,
) -> Result<(String, Vec<Diagnostic>)> {
    render_compact_svg_with_diagnostics(expr, options, None, emit::emit_svg_arena)
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
    let (output, _) =
        render_compact_svg_with(expr, options, arena, emit, |_arena, _root| Vec::new())?;
    Ok(output)
}

fn render_compact_svg_with_diagnostics(
    expr: &CompactExpr,
    options: &RenderOptions,
    arena: Option<&mut arena::LayoutArena>,
    emit: impl FnOnce(&arena::LayoutArena, arena::LayoutBoxId, &RenderOptions) -> String,
) -> Result<(String, Vec<Diagnostic>)> {
    render_compact_svg_with(expr, options, arena, emit, missing_glyph_diagnostics_arena)
}

fn render_compact_svg_with<T>(
    expr: &CompactExpr,
    options: &RenderOptions,
    arena: Option<&mut arena::LayoutArena>,
    emit: impl FnOnce(&arena::LayoutArena, arena::LayoutBoxId, &RenderOptions) -> T,
    diagnostics: impl FnOnce(&arena::LayoutArena, arena::LayoutBoxId) -> Vec<Diagnostic>,
) -> Result<(T, Vec<Diagnostic>)> {
    options.validate()?;
    // Rendering follows auxiliary compact references (strings, column spans,
    // boundary metadata) in addition to the reachable node graph. Validate
    // the complete caller-owned arena before the layout traversal so malformed
    // public inputs return an error instead of panicking in an accessor.
    crate::ast::validate_compact_for_write(&expr.arena, expr.roots, crate::ast::MAX_AST_DEPTH)?;
    let style = if options
        .display_mode
        .unwrap_or(expr.display == crate::ast::DisplayStyle::Block)
    {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let render = |arena: &mut arena::LayoutArena| {
        arena.clear();
        let root = layout::LayoutBuilder::new(style, options).layout(arena, expr);
        let root_box = arena.get(root);
        validate_svg_layout_arena(arena, root, root_box, options)?;
        let diagnostics = diagnostics(arena, root);
        let output = emit(arena, root, options);
        Ok((output, diagnostics))
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
    if crate::ast::has_attributes(expr) {
        return render_to_svg_standalone_compact(&CompactExpr::from_math_expr(expr)?, options);
    }
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    let em = options.font_size_px;
    let (view_x_px, width_px) = emit::svg_x_bounds_px(&root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;
    let pad = 0.08 * em;
    emit::validate_svg_geometry(
        view_x_px - pad,
        width_px + 2.0 * pad,
        height_px + 2.0 * pad,
        root.height * em + pad,
        depth_px + pad,
    )?;
    let chars = subset::collect_chars(&root);
    let mut glyphs = subset::build_glyph_set(&chars);
    let variant_ids = subset::collect_variants(&root);
    subset::add_variants(&mut glyphs, &variant_ids);
    Ok(svg_writer::emit_svg_with_fonts(&root, options, &glyphs))
}

/// Render a math expression to a standalone SVG and report glyph fallbacks.
///
/// The SVG output is identical to [`render_to_svg_standalone`]. Characters
/// without embedded coverage remain `<text>` elements so a viewer may supply a
/// suitable client font. The diagnostic makes that dependency explicit.
#[cfg(feature = "render")]
pub fn render_to_svg_standalone_with_diagnostics(
    expr: &MathExpr,
    options: &RenderOptions,
) -> Result<(String, Vec<Diagnostic>)> {
    options.validate()?;
    crate::ast::validate_ast_depth(expr)?;
    if crate::ast::has_attributes(expr) {
        return render_to_svg_standalone_compact_with_diagnostics(
            &CompactExpr::from_math_expr(expr)?,
            options,
        );
    }
    let style = if options.is_display(expr) {
        style::MathStyle::Display
    } else {
        style::MathStyle::Text
    };
    let root = layout::layout_expr(expr, style, options);
    validate_svg_layout(&root, options)?;
    let diagnostics = missing_glyph_diagnostics(&root);
    let chars = subset::collect_chars(&root);
    let mut glyphs = subset::build_glyph_set(&chars);
    let variant_ids = subset::collect_variants(&root);
    subset::add_variants(&mut glyphs, &variant_ids);
    let output = svg_writer::emit_svg_with_fonts(&root, options, &glyphs);
    Ok((output, diagnostics))
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

/// Render a compact expression to a standalone SVG and report glyph fallbacks.
///
/// The SVG output is identical to [`render_to_svg_standalone_compact`].
#[cfg(feature = "render")]
pub fn render_to_svg_standalone_compact_with_diagnostics(
    expr: &CompactExpr,
    options: &RenderOptions,
) -> Result<(String, Vec<Diagnostic>)> {
    render_compact_svg_with_diagnostics(expr, options, None, emit_standalone_svg)
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

fn validate_svg_layout(root: &layout::LayoutBox, options: &RenderOptions) -> Result<()> {
    let em = options.font_size_px;
    let (view_x_px, width_px) = emit::svg_x_bounds_px(root, em);
    let height_px = (root.height + root.depth) * em;
    let depth_px = root.depth * em;
    let pad = 0.08 * em;
    emit::validate_svg_geometry(
        view_x_px - pad,
        width_px + 2.0 * pad,
        height_px + 2.0 * pad,
        root.height * em + pad,
        depth_px + pad,
    )
}

fn validate_svg_layout_arena(
    arena: &arena::LayoutArena,
    root: arena::LayoutBoxId,
    root_box: &arena::ArenaLayoutBox,
    options: &RenderOptions,
) -> Result<()> {
    let em = options.font_size_px;
    let (view_x_px, width_px) = emit::svg_x_bounds_px_arena(arena, root, em);
    let height_px = (root_box.height + root_box.depth) * em;
    let depth_px = root_box.depth * em;
    let pad = 0.08 * em;
    emit::validate_svg_geometry(
        view_x_px - pad,
        width_px + 2.0 * pad,
        height_px + 2.0 * pad,
        root_box.height * em + pad,
        depth_px + pad,
    )
}

fn missing_glyph_diagnostic(ch: char) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Warning,
        message: format!(
            "character U+{:04X} has no embedded glyph or vector fallback; SVG uses a client-font <text> fallback and raster output draws a missing-glyph box",
            ch as u32
        ),
        span: None,
    }
}

fn record_missing_glyph(
    ch: char,
    font: fonts_registry::FontId,
    italic: bool,
    seen: &mut HashSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if glyph_fallback::is_notdef(ch, font, italic) && seen.insert(ch) {
        diagnostics.push(missing_glyph_diagnostic(ch));
    }
}

fn missing_glyph_diagnostics(root: &layout::LayoutBox) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();
    collect_missing_glyphs(root, &mut seen, &mut diagnostics);
    diagnostics
}

fn collect_missing_glyphs(
    layout: &layout::LayoutBox,
    seen: &mut HashSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use layout::BoxKind;
    match &layout.kind {
        BoxKind::Glyph {
            ch, italic, font, ..
        } => record_missing_glyph(*ch, *font, *italic, seen, diagnostics),
        BoxKind::TextRun {
            text,
            italic,
            text_style,
            ..
        } => {
            let font = fonts_registry::font_for_text_run(*text_style);
            for ch in text.chars() {
                record_missing_glyph(ch, font, *italic, seen, diagnostics);
            }
        },
        BoxKind::Delimiter {
            ch,
            target_height,
            scale,
        } => {
            if glyph_fallback::delimiter_is_notdef(*ch, *target_height, *scale) {
                record_missing_glyph(*ch, fonts_registry::FontId::Math, false, seen, diagnostics);
            }
        },
        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            for child in children {
                collect_missing_glyphs(&child.layout, seen, diagnostics);
            }
        },
        BoxKind::Color { child, .. }
        | BoxKind::Boxed { child, .. }
        | BoxKind::Reflected { child } => {
            collect_missing_glyphs(&child.layout, seen, diagnostics);
        },
        BoxKind::Rule { .. } | BoxKind::Kern | BoxKind::SvgPath(_) | BoxKind::Phantom => {},
    }
}

fn missing_glyph_diagnostics_arena(
    arena: &arena::LayoutArena,
    root: arena::LayoutBoxId,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();
    collect_missing_glyphs_arena(arena, root, &mut seen, &mut diagnostics);
    diagnostics
}

fn collect_missing_glyphs_arena(
    arena: &arena::LayoutArena,
    id: arena::LayoutBoxId,
    seen: &mut HashSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use arena::ArenaBoxKind;
    match &arena.get(id).kind {
        ArenaBoxKind::Glyph {
            ch, italic, font, ..
        } => record_missing_glyph(*ch, *font, *italic, seen, diagnostics),
        ArenaBoxKind::TextRun {
            text,
            italic,
            text_style,
            ..
        } => {
            let font = fonts_registry::font_for_text_run(*text_style);
            for ch in text.chars() {
                record_missing_glyph(ch, font, *italic, seen, diagnostics);
            }
        },
        ArenaBoxKind::Delimiter {
            ch,
            target_height,
            scale,
        } => {
            if glyph_fallback::delimiter_is_notdef(*ch, *target_height, *scale) {
                record_missing_glyph(*ch, fonts_registry::FontId::Math, false, seen, diagnostics);
            }
        },
        ArenaBoxKind::HBox { children } | ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                collect_missing_glyphs_arena(arena, child.id, seen, diagnostics);
            }
        },
        ArenaBoxKind::Color { child, .. }
        | ArenaBoxKind::Boxed { child, .. }
        | ArenaBoxKind::Reflected { child } => {
            collect_missing_glyphs_arena(arena, *child, seen, diagnostics);
        },
        ArenaBoxKind::Rule { .. }
        | ArenaBoxKind::Kern
        | ArenaBoxKind::SvgPath(_)
        | ArenaBoxKind::Phantom => {},
    }
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
    fn prepared_render_reuses_layout_for_both_svg_modes() {
        use crate::reader::Reader;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let options = RenderOptions::default();
        let prepared = prepare_render_compact(&result.expr, &options).unwrap();

        assert_eq!(
            prepared.to_svg_text(&options).unwrap(),
            render_to_svg_compact(&result.expr, &options).unwrap()
        );
        assert_eq!(
            prepared.to_svg(&options).unwrap(),
            render_to_svg_standalone_compact(&result.expr, &options).unwrap()
        );
        let geometry = prepared.geometry(&options).unwrap();
        assert!(geometry.width_px > 0.0);
        assert!(geometry.height_px > 0.0);
        assert!(geometry.baseline_px > 0.0);
        assert!(geometry.baseline_px < geometry.height_px);
    }

    #[test]
    fn prepared_render_rejects_a_different_display_mode() {
        use crate::reader::Reader;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x", &config).unwrap();
        let options = RenderOptions {
            display_mode: Some(false),
            ..RenderOptions::default()
        };
        let prepared = prepare_render_compact(&result.expr, &options).unwrap();
        let display = RenderOptions {
            display_mode: Some(true),
            ..options
        };
        assert!(prepared.to_svg_text(&display).is_err());
    }

    #[test]
    fn prepared_render_diagnostics_match_direct_rendering() {
        let expr = MathExpr::inline(vec![crate::ast::MathNode::Text(
            crate::ast::TextStyle::Normal,
            "中".into(),
        )]);
        let options = RenderOptions::default();
        let prepared = prepare_render(&expr, &options).unwrap();
        let (prepared_svg, prepared_diagnostics) =
            prepared.to_svg_with_diagnostics(&options).unwrap();
        let (direct_svg, direct_diagnostics) =
            render_to_svg_standalone_with_diagnostics(&expr, &options).unwrap();
        assert_eq!(prepared_svg, direct_svg);
        assert_eq!(prepared_diagnostics.len(), direct_diagnostics.len());
        assert!(
            prepared_diagnostics
                .iter()
                .any(|d| d.message.contains("U+4E2D"))
        );
    }

    #[test]
    fn prepared_geometry_rejects_unrepresentable_font_sizes() {
        use crate::reader::Reader;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x", &config).unwrap();
        for font_size_px in [1e-100, 1e100] {
            let options = RenderOptions {
                font_size_px,
                ..RenderOptions::default()
            };
            let prepared = prepare_render_compact(&result.expr, &options).unwrap();
            assert!(prepared.geometry(&options).is_err());
            assert!(prepared.to_svg_text(&options).is_err());
            assert!(prepared.to_svg(&options).is_err());
            assert!(render_to_svg_compact(&result.expr, &options).is_err());
        }
    }

    #[cfg(feature = "raster")]
    #[test]
    fn prepared_render_reuses_layout_for_raster_output() {
        use crate::reader::Reader;

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let render = crate::render::raster::RasterOptions::default();
        let prepared = prepare_render_compact(&result.expr, &render.render).unwrap();

        assert_eq!(
            prepared.to_png(&render).unwrap(),
            crate::render::raster::render_to_image_compact(&result.expr, &render).unwrap()
        );

        let jpeg = crate::render::raster::RasterOptions {
            format: crate::render::raster::ImageFormat::Jpeg,
            ..render.clone()
        };
        assert_eq!(
            prepared.to_jpeg(&render).unwrap(),
            crate::render::raster::render_to_image_compact(&result.expr, &jpeg).unwrap()
        );

        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let invalid = crate::render::raster::RasterOptions {
                scale,
                ..render.clone()
            };
            assert!(prepared.to_image(&invalid).is_err());
        }
    }

    #[cfg(feature = "raster")]
    #[test]
    fn prepared_raster_diagnostics_use_the_retained_layout() {
        let expr = MathExpr::inline(vec![crate::ast::MathNode::Color {
            color: "var(--formula-ink)".into(),
            content: Box::new(crate::ast::MathNode::Text(
                crate::ast::TextStyle::Normal,
                "中".into(),
            )),
        }]);
        let options = crate::render::raster::RasterOptions::default();
        let prepared = prepare_render(&expr, &options.render).unwrap();
        let (prepared_png, prepared_diagnostics) =
            prepared.to_png_with_diagnostics(&options).unwrap();
        let (direct_png, direct_diagnostics) =
            crate::render::raster::render_to_image_with_diagnostics(&expr, &options).unwrap();
        assert_eq!(prepared_png, direct_png);
        assert_eq!(prepared_diagnostics.len(), direct_diagnostics.len());
        assert!(
            prepared_diagnostics
                .iter()
                .any(|d| d.message.contains("unsupported raster expression color"))
        );
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
    fn actuarial_subscript_geometry_matches_heap_and_arena_paths() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex =
            crate::reader::latex::LatexReader::parse_compact(r"\angl{$x_i$}", &config).unwrap();
        let mathml = crate::reader::mathml::MathmlReader::parse_compact(
            r#"<math><msub><mi>x</mi><menclose notation="actuarial"><mi>i</mi></menclose></msub></math>"#,
            &config,
        )
        .unwrap();
        for expression in [&latex.expr, &mathml.expr] {
            let options = RenderOptions::default();
            let heap = render_to_svg_compact(expression, &options).unwrap();
            let mut arena = arena::LayoutArena::new();
            let compact = render_to_svg_compact_arena(expression, &options, &mut arena).unwrap();
            assert_eq!(heap, compact);
            assert!(heap.contains("<rect"), "actuarial rules missing: {heap}");
        }
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

    #[test]
    fn compact_svg_entry_points_reject_foreign_references() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        fn malformed() -> CompactExpr {
            let mut foreign = crate::ast::arena::AstArena::new();
            let text = foreign.push_str("x");
            let mut arena = crate::ast::arena::AstArena::new();
            let root = arena.push_node(crate::ast::compact::CompactNode::Identifier(text));
            let roots = arena.push_children(&[root]).unwrap();
            CompactExpr::new(arena, roots, crate::ast::DisplayStyle::Inline)
        }

        fn malformed_array() -> CompactExpr {
            let mut foreign = crate::ast::arena::AstArena::new();
            let padding = foreign.push_node(crate::ast::compact::CompactNode::Row(
                crate::ast::compact::NodeSpan::EMPTY,
            ));
            let _ = foreign.push_children(&[padding]).unwrap();
            let column = foreign.push_node(crate::ast::compact::CompactNode::ColAlign(
                crate::ast::ColumnAlign::Center,
            ));
            let columns = foreign.push_children(&[column]).unwrap();
            let mut arena = crate::ast::arena::AstArena::new();
            let cells = arena.push_children(&[]).unwrap();
            let root = arena.push_node(crate::ast::compact::CompactNode::Array {
                col_aligns: columns,
                cells,
                num_cols: 0,
                arraystretch_pct: 100,
                add_jot: false,
                array_style: false,
                col_seps: 0,
                row_seps: 0,
            });
            let roots = arena.push_children(&[root]).unwrap();
            CompactExpr::new(arena, roots, crate::ast::DisplayStyle::Inline)
        }

        fn assert_rejected(call: impl FnOnce() -> crate::error::Result<String>) {
            let result = catch_unwind(AssertUnwindSafe(call));
            assert!(result.is_ok(), "rendering malformed compact input panicked");
            assert!(
                result.unwrap().is_err(),
                "malformed compact input was accepted"
            );
        }

        let options = RenderOptions::default();
        for compact in [malformed(), malformed_array()] {
            assert_rejected(|| render_to_svg_compact(&compact, &options));
            assert_rejected(|| render_to_svg_standalone_compact(&compact, &options));

            let mut arena = arena::LayoutArena::new();
            assert_rejected(|| render_to_svg_compact_arena(&compact, &options, &mut arena));
            let mut arena = arena::LayoutArena::new();
            assert_rejected(|| {
                render_to_svg_standalone_compact_arena(&compact, &options, &mut arena)
            });
        }
    }

    #[test]
    fn reflected_svg_uses_horizontal_transform_and_preserves_width() {
        use crate::reader::Reader;
        let parsed = crate::reader::latex::LatexReader::parse_compact(
            r"\mathreflectbox{xy^2}",
            &Default::default(),
        )
        .unwrap();
        let svg = render_to_svg_compact(&parsed.expr, &RenderOptions::default()).unwrap();
        assert!(svg.contains("scale(-1,1)"));
        assert!(svg.contains("translate("));
        let plain =
            crate::reader::latex::LatexReader::parse_compact("xy^2", &Default::default()).unwrap();
        let plain_svg = render_to_svg_compact(&plain.expr, &RenderOptions::default()).unwrap();
        let width = |s: &str| -> String {
            s.split("width=\"")
                .nth(1)
                .and_then(|v| v.split('"').next())
                .unwrap()
                .to_owned()
        };
        assert_eq!(width(&svg), width(&plain_svg));
    }

    #[test]
    fn nested_reflections_emit_nested_transforms() {
        use crate::reader::Reader;
        let parsed = crate::reader::latex::LatexReader::parse_compact(
            r"\mathreflectbox{\reflectbox{xy}}",
            &Default::default(),
        )
        .unwrap();
        let svg = render_to_svg_compact(&parsed.expr, &RenderOptions::default()).unwrap();
        assert_eq!(svg.matches("scale(-1,1)").count(), 2);
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

    #[test]
    fn render_diagnostics_report_uncovered_characters() {
        let expr = MathExpr::inline(vec![crate::ast::MathNode::Text(
            crate::ast::TextStyle::Normal,
            "中".into(),
        )]);
        let options = RenderOptions::default();
        let (diagnostic_svg, diagnostics) =
            render_to_svg_standalone_with_diagnostics(&expr, &options).unwrap();
        assert!(diagnostic_svg.contains("<text"));
        assert!(
            diagnostics.iter().any(|d| d.message.contains("U+4E2D")),
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostic_svg,
            render_to_svg_standalone(&expr, &options).unwrap()
        );
    }

    #[test]
    fn render_diagnostics_report_uncovered_stretchy_delimiters() {
        let parsed =
            crate::reader::latex::LatexReader::parse(r"\left中 x \right中", &Default::default())
                .unwrap();
        let options = RenderOptions::default();
        let (svg, diagnostics) =
            render_to_svg_standalone_with_diagnostics(&parsed.expr, &options).unwrap();
        assert!(svg.matches("<text").count() >= 2, "{svg}");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.message.contains("U+4E2D"))
                .count(),
            1,
            "{diagnostics:?}"
        );

        let compact = crate::reader::latex::LatexReader::parse_compact(
            r"\left中 x \right中",
            &Default::default(),
        )
        .unwrap();
        let (_, compact_diagnostics) =
            render_to_svg_standalone_compact_with_diagnostics(&compact.expr, &options).unwrap();
        assert_eq!(
            compact_diagnostics
                .iter()
                .filter(|d| d.message.contains("U+4E2D"))
                .count(),
            1,
            "{compact_diagnostics:?}"
        );

        let covered =
            crate::reader::latex::LatexReader::parse(r"\left( x \right)", &Default::default())
                .unwrap();
        let (_, covered_diagnostics) =
            render_to_svg_standalone_with_diagnostics(&covered.expr, &options).unwrap();
        assert!(covered_diagnostics.is_empty(), "{covered_diagnostics:?}");
    }

    #[test]
    fn heap_diagnostic_svg_routes_validate_geometry_like_plain_routes() {
        let expr = MathExpr::inline(vec![crate::ast::MathNode::Identifier("x".into())]);
        for font_size_px in [1e-100, 1e100] {
            let options = RenderOptions {
                font_size_px,
                ..RenderOptions::default()
            };
            assert!(render_to_svg(&expr, &options).is_err());
            assert!(render_to_svg_with_diagnostics(&expr, &options).is_err());
            assert!(render_to_svg_standalone(&expr, &options).is_err());
            assert!(render_to_svg_standalone_with_diagnostics(&expr, &options).is_err());
        }
    }
}
