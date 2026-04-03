pub mod emit;
pub mod layout;
pub mod metrics;
pub mod params;
#[cfg(feature = "raster")]
pub mod raster;
pub mod spacing;
pub mod style;

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
            font_family: "serif".into(),
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
