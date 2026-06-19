use super::RenderOptions;
use super::fonts_registry::{self, FontId};
use super::glyph_variants;
use super::math_table;
use super::metrics::{self, GlyphMetrics};
use super::params::TexParams;
use super::spacing::{self, AtomType};
use super::style::MathStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
/// Layout engine: converts MathExpr → LayoutBox tree.
/// Applies TeX typesetting rules for positioning.
use crate::ast::*;
use smallvec::SmallVec;

mod accent;
mod array;
mod fraction;
mod limits;
mod radical;
mod scripts;
mod stretchy;
mod styled;
#[cfg(test)]
mod tests;

use accent::*;
use array::*;
use fraction::*;
use limits::*;
use radical::*;
use scripts::*;
use stretchy::*;
use styled::*;

/// A positioned layout box in the rendering tree.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub width: f64,
    pub height: f64, // above baseline
    pub depth: f64,  // below baseline
    pub kind: BoxKind,
}

/// The content of a layout box.
#[derive(Debug, Clone)]
pub enum BoxKind {
    /// A single glyph character. `font` selects the embedded face it is drawn
    /// from (`FontId::Math` for ordinary math; a Latin Modern text face for
    /// `\mathrm`/`\mathbf`/`\mathsf`/`\mathtt` style glyphs).
    Glyph {
        ch: char,
        size: f64,
        italic: bool,
        font: FontId,
    },
    /// Horizontal box: children laid out left to right.
    HBox { children: Vec<PositionedBox> },
    /// Vertical box: children stacked top to bottom.
    VBox { children: Vec<PositionedBox> },
    /// A horizontal rule (fraction bar, vinculum).
    Rule { thickness: f64 },
    /// Invisible spacing.
    Kern,
    /// A stretchy delimiter.
    Delimiter { ch: char, target_height: f64 },
    /// SVG path data (radical signs, cancel lines).
    SvgPath(String),
    /// Color wrapper.
    Color {
        color: String,
        child: Box<PositionedBox>,
    },
    /// Invisible placeholder (takes space but doesn't render).
    Phantom,
    /// Box with border.
    Boxed {
        child: Box<PositionedBox>,
        padding: f64,
        bg_color: Option<String>,
        border_color: Option<String>,
    },
    /// A run of text characters rendered as a single `<text>` element.
    TextRun {
        text: String,
        size: f64,
        italic: bool,
        text_style: Option<TextStyle>,
    },
}

/// A box with an offset from its parent's reference point.
#[derive(Debug, Clone)]
pub struct PositionedBox {
    pub dx: f64,
    pub dy: f64, // positive = up from baseline
    pub layout: LayoutBox,
}

impl PositionedBox {
    #[inline]
    pub fn new(dx: f64, dy: f64, layout: LayoutBox) -> Self {
        Self { dx, dy, layout }
    }

    #[inline]
    pub fn at_origin(layout: LayoutBox) -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            layout,
        }
    }
}

impl LayoutBox {
    #[inline]
    pub fn kern(width: f64) -> Self {
        Self {
            width,
            height: 0.0,
            depth: 0.0,
            kind: BoxKind::Kern,
        }
    }

    #[inline]
    pub fn rule(width: f64, thickness: f64) -> Self {
        Self {
            width,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            kind: BoxKind::Rule { thickness },
        }
    }

    #[inline]
    fn glyph(ch: char, size: f64, italic: bool) -> Self {
        Self::glyph_font(ch, size, italic, FontId::Math)
    }

    /// Build a glyph box drawn from a specific embedded face.
    ///
    /// For `FontId::Math` this matches [`LayoutBox::glyph`]; for a Latin Modern
    /// text face the advance/bbox metrics come from that face so layout
    /// positions line up with the drawn outlines.
    #[inline]
    fn glyph_font(ch: char, size: f64, italic: bool, font: FontId) -> Self {
        if is_invisible_char(ch) {
            return Self {
                width: 0.0,
                height: 0.0,
                depth: 0.0,
                kind: BoxKind::Kern,
            };
        }
        let m = if font.is_math() {
            if italic {
                metrics::glyph_metrics(ch)
            } else {
                metrics::glyph_metrics_upright(ch)
            }
        } else {
            fonts_registry::text_metrics(ch, italic, font)
        };
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph {
                ch,
                size,
                italic,
                font,
            },
        }
    }

    #[inline]
    fn glyph_with_metrics(ch: char, m: GlyphMetrics, size: f64, italic: bool) -> Self {
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph {
                ch,
                size,
                italic,
                font: FontId::Math,
            },
        }
    }
}

/// Returns true for math "invisible" operators and zero-width formatting
/// characters. These must occupy no space and draw no glyph (otherwise they
/// render as missing-glyph "tofu" boxes). Covers ApplyFunction, InvisibleTimes,
/// InvisibleComma/Separator, InvisiblePlus, and the common zero-width controls.
#[inline]
fn is_invisible_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{2061}' | '\u{2062}' | '\u{2063}' | '\u{2064}' // function application / invisible ops
            | '\u{200B}' | '\u{200C}' | '\u{200D}'        // ZWSP, ZWNJ, ZWJ
            | '\u{2060}' | '\u{FEFF}' // word joiner, ZWNBSP
    )
}

/// Returns true for characters that should be vertically centered on the math axis
/// (e.g. \cdots, \ddots) rather than placed at the baseline.
#[inline]
fn is_axis_centered_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{22EF}' | '\u{22F1}' |  // ⋯ (cdots), ⋱ (ddots)
        '\u{22EE}' |                // ⋮ (vdots)
        '\u{22F0}' // ⋰ (iddots / reverse ddots)
    )
}

/// Determine whether a single character should be rendered italic in math mode.
/// Per TeX convention:
/// - Latin letters (a-z, A-Z) used as identifiers → italic
/// - Greek lowercase (α-ω and variants like ϵ, ϑ, ϰ, ϕ, ϱ, ϖ) → italic
/// - Greek uppercase (Γ, Δ, ..., Ω) → upright
/// - Digits, operators, symbols → upright
#[inline]
fn is_math_italic_char(ch: char) -> bool {
    match ch {
        'a'..='z' | 'A'..='Z' => true,
        // Greek lowercase: U+03B1 (α) to U+03C9 (ω)
        '\u{03B1}'..='\u{03C9}' => true,
        // Greek lowercase variants: ϵ ϑ ϰ ϕ ϱ ϖ
        '\u{03F5}' | '\u{03D1}' | '\u{03F0}' | '\u{03D5}' | '\u{03F1}' | '\u{03D6}' => true,
        _ => false,
    }
}

/// Map a character to its Mathematical Italic Unicode codepoint.
/// Returns `None` for non-mappable characters (digits, symbols, Greek uppercase).
#[inline]
pub(crate) fn math_italic_codepoint(ch: char) -> Option<char> {
    match ch {
        'A'..='Z' => char::from_u32(0x1D434 + (ch as u32 - b'A' as u32)),
        'h' => Some('\u{210E}'),
        'a'..='z' => char::from_u32(0x1D44E + (ch as u32 - b'a' as u32)),
        // Greek lowercase α-ρ → U+1D6FC-U+1D70C
        '\u{03B1}'..='\u{03C1}' => char::from_u32(0x1D6FC + (ch as u32 - 0x03B1)),
        // Greek ς (final sigma) → U+1D70D
        '\u{03C2}' => char::from_u32(0x1D70D),
        // Greek σ-ω → U+1D70E-U+1D714
        '\u{03C3}'..='\u{03C9}' => char::from_u32(0x1D70E + (ch as u32 - 0x03C3)),
        // Greek variants: ϵ ϑ ϰ ϕ ϱ ϖ
        '\u{03F5}' => char::from_u32(0x1D716), // ϵ → italic epsilon
        '\u{03D1}' => char::from_u32(0x1D717), // ϑ → italic theta
        '\u{03F0}' => char::from_u32(0x1D718), // ϰ → italic kappa
        '\u{03D5}' => char::from_u32(0x1D719), // ϕ → italic phi
        '\u{03F1}' => char::from_u32(0x1D71A), // ϱ → italic rho
        '\u{03D6}' => char::from_u32(0x1D71B), // ϖ → italic pi
        _ => None,
    }
}

/// Wrap a glyph LayoutBox so it is centered on the math axis.
/// The shift moves the glyph's vertical center to `axis_height * size`.
fn center_glyph_on_axis(glyph: LayoutBox, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let axis = ctx.params.axis_height * size;
    let glyph_center = (glyph.height - glyph.depth) / 2.0;
    let shift = axis - glyph_center;
    let glyph_w = glyph.width;
    let new_height = (glyph.height + shift).max(0.0);
    let new_depth = (glyph.depth - shift).max(0.0);
    LayoutBox {
        width: glyph_w,
        height: new_height,
        depth: new_depth,
        kind: BoxKind::HBox {
            children: vec![PositionedBox::new(0.0, shift, glyph)],
        },
    }
}

/// Maximum recursion depth for layout to prevent stack overflow on adversarial input.
const MAX_LAYOUT_DEPTH: u32 = 128;

// BIG_OP_DISPLAY_SCALE removed: display-mode operators now use per-operator
// scale factors from metrics::display_op_scale() matching KaTeX Size2/Size1 ratios.

/// Matrix/array inter-column gap as a fraction of the current font size (em).
///
/// Standard LaTeX uses `2 * \arraycolsep = 10pt = 1.0em` at 10pt, but KaTeX's
/// rendered output appears tighter due to CSS padding interactions and glyph
/// metrics. A value of 0.8em produces output that matches KaTeX more closely,
/// especially for compact matrix environments with single-character cells.
const MATRIX_COL_GAP_EM: f64 = 0.8;

/// LaTeX `\arraycolsep` (≈ 5pt at a 10pt size). Added on both sides of every
/// column in `array`-style tables, and used as half the inter-column gap.
const ARRAY_COLSEP_EM: f64 = 0.5;

/// Column gap within an rl-pair at alignment points in aligned environments.
/// KaTeX uses 0 for this gap (R→L boundary); we use a tiny value so content
/// doesn't collide.
const ALIGN_INTERNAL_GAP_EM: f64 = 0.0;

/// Column gap between rl-pairs in aligned environments (L→R boundary).
/// KaTeX uses 2 * arraycolsep = 10pt ≈ 1.0em; we use the same.
const ALIGN_PAIR_GAP_EM: f64 = 1.0;

/// TeX \scriptspace: 0.5pt at 10pt = 0.05em.
/// Added after super/subscripts per TeXBook Rule 18.
const SCRIPT_SPACE: f64 = 0.05;

/// Slant angle (degrees) applied to the Latin Modern Math `∂` (U+2202) glyph via
/// `skewX`. The bare LMMath outline is nearly upright, whereas MathJax renders
/// `∂` with the slight *forward* italic lean standard in math typography (top
/// leans right). A negative angle (same sign convention as the synthetic italic
/// shear) produces that forward lean. Centralized here so every raster/SVG
/// backend agrees (important for SVG↔PNG parity).
pub(crate) const PARTIAL_DESLANT_DEG: f32 = -8.0;

/// Extract the italic correction (in absolute units) for a base LayoutBox.
/// Returns a non-zero value only when the base is a single glyph.
#[inline]
fn italic_correction(base: &LayoutBox) -> f64 {
    match &base.kind {
        BoxKind::Glyph { ch, size, .. } => metrics::glyph_metrics(*ch).italic * size,
        _ => 0.0,
    }
}

/// Font size scale factors for LaTeX sizing commands (\tiny..\Huge).
const SIZE_SCALE_TABLE: [f64; 10] = [0.5, 0.7, 0.8, 0.9, 1.0, 1.2, 1.44, 1.728, 2.074, 2.488];

#[derive(Clone)]
struct LayoutCtx<'a> {
    style: MathStyle,
    params: TexParams,
    options: &'a RenderOptions,
    depth: u32,
    size_scale: f64,
    /// When set, use this style for inter-atom spacing calculations instead of
    /// `self.style`.  This allows fraction numerators/denominators to preserve
    /// medium/thick spacing even when the layout style drops to Script or below.
    spacing_style: Option<MathStyle>,
}

impl<'a> LayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier() * self.size_scale
    }

    /// Style to use for inter-atom spacing calculations.
    fn effective_spacing_style(&self) -> MathStyle {
        self.spacing_style.unwrap_or(self.style)
    }

    fn with_style(&self, style: MathStyle) -> LayoutCtx<'a> {
        LayoutCtx {
            style,
            params: TexParams::for_size_class(style.size_class()),
            options: self.options,
            depth: self.depth,
            size_scale: self.size_scale,
            spacing_style: self.spacing_style,
        }
    }

    /// Return a new context with incremented depth for recursion tracking.
    fn deeper(&self) -> LayoutCtx<'a> {
        LayoutCtx {
            style: self.style,
            params: self.params,
            options: self.options,
            depth: self.depth + 1,
            size_scale: self.size_scale,
            spacing_style: self.spacing_style,
        }
    }
}

// ── Stretchy decoration SVG path helpers ──────────────────────────────

/// Entry point: layout an entire math expression.
pub fn layout_expr(expr: &MathExpr, style: MathStyle, options: &RenderOptions) -> LayoutBox {
    let ctx = LayoutCtx {
        style,
        params: TexParams::for_size_class(style.size_class()),
        options,
        depth: 0,
        size_scale: 1.0,
        spacing_style: None,
    };
    layout_row(&expr.nodes, &ctx)
}

fn layout_row(nodes: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    if nodes.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::with_capacity(nodes.len() * 2);
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for node in nodes {
        // Inter-atom spacing with TeXBook Ch.26 Bin reclassification
        let raw_atom = spacing::classify_node(node);
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.unwrap(); // safe: we passed Some
        if let Some(prev) = adj_prev {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.effective_spacing_style());
            if space > 0.0 {
                let kern_width = space * ctx.size();
                cursor_x += kern_width;
            }
        }

        let layout = layout_node(node, ctx);
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);

        // Capture width before moving layout into children vec
        let layout_width = layout.width;
        children.push(PositionedBox::new(cursor_x, 0.0, layout));
        cursor_x += layout_width;
        prev_atom = Some(cur_atom);
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_node(node: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Guard against stack overflow from deeply nested input (e.g. \frac{\frac{...}{...}}{...})
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return LayoutBox::kern(0.0);
    }
    let ctx = &ctx.deeper();
    match node {
        MathNode::Number(s) => layout_text_run(s, ctx, false, None),
        MathNode::Identifier(s) => {
            if s.chars().count() == 1 {
                // Single-char identifiers are italic; chars().next() is safe
                // because chars().count() == 1 guarantees at least one char
                let ch = s.chars().next().expect("non-empty checked");
                LayoutBox::glyph(ch, ctx.size(), is_math_italic_char(ch))
            } else {
                layout_text_run(s, ctx, false, None)
            }
        },
        MathNode::Operator(op) => layout_operator(op, ctx),
        MathNode::Symbol(kind, s) => {
            if let Some(ch) = s.chars().next() {
                if *kind == SymbolKind::LargeOp && ctx.style.is_display() {
                    let size = ctx.size() * metrics::display_op_scale(ch);
                    let m = metrics::glyph_metrics_display(ch);
                    LayoutBox::glyph_with_metrics(ch, m, size, false)
                } else {
                    let glyph = LayoutBox::glyph(ch, ctx.size(), false);
                    if is_axis_centered_char(ch) {
                        center_glyph_on_axis(glyph, ctx)
                    } else {
                        glyph
                    }
                }
            } else {
                LayoutBox::kern(0.0)
            }
        },
        MathNode::Text(style, s) => {
            let italic = matches!(
                style,
                TextStyle::Italic
                    | TextStyle::BoldItalic
                    | TextStyle::SansSerifItalic
                    | TextStyle::SansSerifBoldItalic
            );
            let ts = if matches!(style, TextStyle::Normal) {
                None
            } else {
                Some(*style)
            };
            layout_text_run(s, ctx, italic, ts)
        },
        MathNode::Space(r) => LayoutBox::kern(r.as_f64() * ctx.size()),
        MathNode::Rule {
            width,
            height,
            lift,
        } => layout_rule(*width, *height, *lift, ctx),
        MathNode::Row(children) => layout_row(children, ctx),
        MathNode::Fenced {
            open,
            close,
            content,
        } => layout_fenced(open, close, content, ctx),
        MathNode::Subscript { base, sub } => layout_subscript(base, sub, ctx),
        MathNode::Superscript { base, sup } => layout_superscript(base, sup, ctx),
        MathNode::SubSup { base, sub, sup } => layout_subsup(base, sub, sup, ctx),
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => layout_prescripts(base, pre_sub, pre_sup, ctx),
        MathNode::Overscript { base, over, .. } => layout_overscript(base, over, ctx),
        MathNode::Underscript { base, under, .. } => layout_underscript(base, under, ctx),
        MathNode::UnderOver {
            base, under, over, ..
        } => layout_underover(base, under, over, ctx),
        MathNode::Fraction { kind, num, den } => layout_fraction(*kind, num, den, ctx),
        MathNode::Sqrt(radicand) => layout_sqrt(radicand, None, ctx),
        MathNode::Root { index, radicand } => layout_sqrt(radicand, Some(index), ctx),
        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => layout_nary(*op, sub, sup, body, *limits, ctx),
        MathNode::Accent {
            base,
            accent,
            position,
            wide,
        } => layout_accent(base, *accent, *position, *wide, ctx),
        MathNode::Array {
            columns,
            rows,
            arraystretch,
            add_jot,
        } => layout_array(columns, rows, *arraystretch, *add_jot, ctx),
        MathNode::Phantom(content, kind) => layout_phantom(content, *kind, ctx),
        MathNode::Boxed {
            content,
            bg_color,
            border_color,
        } => layout_boxed(content, bg_color.as_deref(), border_color.as_deref(), ctx),
        MathNode::Cancel { stroke, content } => layout_cancel(*stroke, content, ctx),
        MathNode::Scaled { factor, content } => layout_scaled(*factor, content, ctx),
        MathNode::RaiseBox {
            shift,
            height,
            depth,
            content,
        } => {
            let child = layout_node(content, ctx);
            let shift = shift.as_f64() * ctx.size();
            let height = height
                .map(|h| h.as_f64() * ctx.size())
                .unwrap_or_else(|| (shift + child.height).max(0.0));
            let depth = depth
                .map(|d| d.as_f64() * ctx.size())
                .unwrap_or_else(|| (child.depth - shift).max(0.0));
            LayoutBox {
                width: child.width,
                height,
                depth,
                kind: BoxKind::HBox {
                    children: vec![PositionedBox::new(0.0, shift, child)],
                },
            }
        },
        MathNode::Styled(style, children) => layout_styled(*style, children, ctx),
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let chosen = match ctx.style {
                MathStyle::Display | MathStyle::DisplayCramped => display.as_ref(),
                MathStyle::Text | MathStyle::TextCramped => text.as_ref(),
                MathStyle::Script | MathStyle::ScriptCramped => script.as_ref(),
                MathStyle::ScriptScript | MathStyle::ScriptScriptCramped => script_script.as_ref(),
            };
            layout_node(chosen, ctx)
        },
        MathNode::MathAtom { content, .. } => layout_node(content, ctx),
    }
}

// === Leaf node layouts ===

fn layout_text_run(
    text: &str,
    ctx: &LayoutCtx,
    italic: bool,
    text_style: Option<TextStyle>,
) -> LayoutBox {
    let size = ctx.size();
    let font = fonts_registry::font_for_text_run(text_style);
    let mut total_width = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        // Use upright metrics for non-italic text (operator names, numbers, \text{}).
        // Italic and upright fonts have significantly different character widths
        // (e.g. 'i' is 0.278em upright vs 0.345em italic — 24% wider in italic).
        // For text faces the advance is read straight from the selected face so
        // layout positions line up with the drawn outlines.
        let m = fonts_registry::text_metrics(ch, italic, font);
        let h = m.height * size;
        let d = m.depth * size;
        max_height = max_height.max(h);
        max_depth = max_depth.max(d);
        total_width += m.width * size;
    }

    LayoutBox {
        width: total_width,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::TextRun {
            text: text.to_string(),
            size,
            italic,
            text_style,
        },
    }
}

fn layout_operator(op: &OperatorAttr, ctx: &LayoutCtx) -> LayoutBox {
    layout_text_run(&op.name, ctx, false, None)
}

// === Structure node layouts ===

/// Build the synthetic radical `<path>` for a `√` `Delimiter` box of the given
/// width and total height (em units). Used by the text-based SVG emitter, which
/// cannot embed font glyph outlines and would otherwise scale a `√` character.
pub(crate) fn radical_delim_path(width: f64, total_height: f64) -> String {
    let t = math_table::math_constants()
        .map(|c| c.radical_rule_thickness.max(0.06))
        .unwrap_or(0.06);
    radical_path(width, total_height, t)
}

/// Scale numeric coordinate values in an SVG path data string by `f`.
///
/// Handles our "F:" fill-prefix and all standard SVG path commands.
/// For arc commands (`A`/`a`), parameters 3-5 (x-rotation, large-arc-flag,
/// sweep-flag) are preserved unscaled since they are angles/flags, not coordinates.
pub(crate) fn scale_svg_path_data(path: &str, f: f64) -> String {
    // Preserve the "F:" prefix if present
    let (prefix, data) = if let Some(rest) = path.strip_prefix("F:") {
        ("F:", rest)
    } else {
        ("", path)
    };

    let mut pw = crate::render::path_writer::PathWriter::with_prefix(prefix);

    let bytes = data.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Track current command and parameter index for arc handling
    let mut cmd = b'M'; // default command
    let mut param_idx: usize = 0;

    /// Number of parameters per SVG path command.
    fn param_count(c: u8) -> usize {
        match c.to_ascii_uppercase() {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            b'A' => 7,
            _ => 0, // Z and unknown
        }
    }

    /// Whether this parameter index should NOT be scaled for arc commands.
    /// Arc: rx(0) ry(1) x-rot(2) large-arc(3) sweep(4) x(5) y(6)
    /// Indices 2,3,4 are angle/flag values that must not be scaled.
    fn is_arc_noscale(c: u8, idx: usize) -> bool {
        c.eq_ignore_ascii_case(&b'A') && (2..=4).contains(&(idx % 7))
    }

    while i < len {
        let b = bytes[i];
        if b.is_ascii_alphabetic() && b != b'e' && b != b'E' {
            // SVG path command letter
            cmd = b;
            param_idx = 0;
            pw.cmd(b);
            i += 1;
        } else if b == b'-' || b == b'+' || b == b'.' || b.is_ascii_digit() {
            // Start of a number — collect it, parse, optionally scale, and re-emit
            let start = i;
            if b == b'-' || b == b'+' {
                i += 1;
            }
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // Handle scientific notation (e.g., 1.5e-3)
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                i += 1;
                if i < len && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let num_str = &data[start..i];
            if let Ok(val) = num_str.parse::<f64>() {
                if is_arc_noscale(cmd, param_idx) {
                    // Preserve arc rotation/flags unscaled
                    pw.push_f64(val);
                } else {
                    pw.push_f64(val * f);
                }
            } else {
                pw.number(num_str);
            }
            param_idx += 1;
            // Handle implicit command repeats (wrap around param count)
            let pc = param_count(cmd);
            if pc > 0 && param_idx >= pc {
                param_idx = 0;
                // After M, implicit repeats become L
                if cmd == b'M' {
                    cmd = b'L';
                } else if cmd == b'm' {
                    cmd = b'l';
                }
            }
        } else {
            // Whitespace, comma, or other separator — drop it (PathWriter inserts
            // minimal separators itself)
            i += 1;
        }
    }
    pw.finish()
}

/// Recursively scale all child positions (dx/dy) and dimensions in a BoxKind
/// so that `\scalebox` and friends produce correctly positioned content.
fn scale_box_kind(kind: BoxKind, f: f64) -> BoxKind {
    match kind {
        BoxKind::HBox { children } => BoxKind::HBox {
            children: children
                .into_iter()
                .map(|c| {
                    PositionedBox::new(
                        c.dx * f,
                        c.dy * f,
                        LayoutBox {
                            width: c.layout.width * f,
                            height: c.layout.height * f,
                            depth: c.layout.depth * f,
                            kind: scale_box_kind(c.layout.kind, f),
                        },
                    )
                })
                .collect(),
        },
        BoxKind::VBox { children } => BoxKind::VBox {
            children: children
                .into_iter()
                .map(|c| {
                    PositionedBox::new(
                        c.dx * f,
                        c.dy * f,
                        LayoutBox {
                            width: c.layout.width * f,
                            height: c.layout.height * f,
                            depth: c.layout.depth * f,
                            kind: scale_box_kind(c.layout.kind, f),
                        },
                    )
                })
                .collect(),
        },
        BoxKind::Color { color, child } => BoxKind::Color {
            color,
            child: Box::new(PositionedBox::new(
                child.dx * f,
                child.dy * f,
                LayoutBox {
                    width: child.layout.width * f,
                    height: child.layout.height * f,
                    depth: child.layout.depth * f,
                    kind: scale_box_kind(child.layout.kind, f),
                },
            )),
        },
        BoxKind::Boxed {
            child,
            padding,
            bg_color,
            border_color,
        } => BoxKind::Boxed {
            child: Box::new(PositionedBox::new(
                child.dx * f,
                child.dy * f,
                LayoutBox {
                    width: child.layout.width * f,
                    height: child.layout.height * f,
                    depth: child.layout.depth * f,
                    kind: scale_box_kind(child.layout.kind, f),
                },
            )),
            padding: padding * f,
            bg_color,
            border_color,
        },
        BoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        } => BoxKind::Glyph {
            ch,
            size: size * f,
            italic,
            font,
        },
        BoxKind::TextRun {
            text,
            size,
            italic,
            text_style,
        } => BoxKind::TextRun {
            text,
            size: size * f,
            italic,
            text_style,
        },
        BoxKind::Rule { thickness } => BoxKind::Rule {
            thickness: thickness * f,
        },
        BoxKind::Delimiter { ch, target_height } => BoxKind::Delimiter {
            ch,
            target_height: target_height * f,
        },
        // Kern, Phantom don't need child scaling.
        // SvgPath: scale all numeric coordinates in the path data.
        BoxKind::SvgPath(path) => BoxKind::SvgPath(scale_svg_path_data(&path, f)),
        other => other,
    }
}

use crate::render::arena::{
    ArenaBoxKind, ArenaLayoutBox, ArenaPositionedBox, LayoutArena, LayoutBoxId,
};

/// Extract the italic correction for a base box in the arena layout path.
#[inline]
fn italic_correction_arena(base_id: LayoutBoxId, arena: &LayoutArena) -> f64 {
    match &arena.get(base_id).kind {
        ArenaBoxKind::Glyph { ch, size, .. } => metrics::glyph_metrics(*ch).italic * size,
        _ => 0.0,
    }
}

/// Builder for arena-based layout.
pub struct LayoutBuilder<'a> {
    style: MathStyle,
    options: &'a RenderOptions,
}

impl<'a> LayoutBuilder<'a> {
    pub fn new(style: MathStyle, options: &'a RenderOptions) -> Self {
        Self { style, options }
    }

    /// Layout a compact expression into the arena. Returns the root box ID.
    pub fn layout(&self, arena: &mut LayoutArena, expr: &CompactExpr) -> LayoutBoxId {
        let ctx = ArenaLayoutCtx {
            style: self.style,
            params: TexParams::for_size_class(self.style.size_class()),
            options: self.options,
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        };
        layout_row_arena(&expr.arena, expr.roots, arena, &ctx)
    }
}

#[derive(Clone)]
struct ArenaLayoutCtx<'a> {
    style: MathStyle,
    params: TexParams,
    options: &'a RenderOptions,
    depth: u32,
    size_scale: f64,
    /// When set, use this style for inter-atom spacing calculations instead of
    /// `self.style`.  See `LayoutCtx::spacing_style` for rationale.
    spacing_style: Option<MathStyle>,
}

impl<'a> ArenaLayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier() * self.size_scale
    }

    /// Style to use for inter-atom spacing calculations.
    fn effective_spacing_style(&self) -> MathStyle {
        self.spacing_style.unwrap_or(self.style)
    }

    fn with_style(&self, style: MathStyle) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style,
            params: TexParams::for_size_class(style.size_class()),
            options: self.options,
            depth: self.depth,
            size_scale: self.size_scale,
            spacing_style: self.spacing_style,
        }
    }

    fn deeper(&self) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style: self.style,
            params: self.params,
            options: self.options,
            depth: self.depth + 1,
            size_scale: self.size_scale,
            spacing_style: self.spacing_style,
        }
    }
}

// ── Leaf helpers ──────────────────────────────────────────────────────

fn alloc_glyph(arena: &mut LayoutArena, ch: char, size: f64, italic: bool) -> LayoutBoxId {
    alloc_glyph_font(arena, ch, size, italic, FontId::Math)
}

/// Allocate a glyph box drawn from a specific embedded face (arena variant).
fn alloc_glyph_font(
    arena: &mut LayoutArena,
    ch: char,
    size: f64,
    italic: bool,
    font: FontId,
) -> LayoutBoxId {
    if is_invisible_char(ch) {
        return arena.alloc(ArenaLayoutBox {
            width: 0.0,
            height: 0.0,
            depth: 0.0,
            kind: ArenaBoxKind::Kern,
        });
    }
    let m = if font.is_math() {
        if italic {
            metrics::glyph_metrics(ch)
        } else {
            metrics::glyph_metrics_upright(ch)
        }
    } else {
        fonts_registry::text_metrics(ch, italic, font)
    };
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph {
            ch,
            size,
            italic,
            font,
        },
    })
}

fn alloc_glyph_with_metrics(
    arena: &mut LayoutArena,
    ch: char,
    m: metrics::GlyphMetrics,
    size: f64,
    italic: bool,
) -> LayoutBoxId {
    if is_invisible_char(ch) {
        return arena.alloc(ArenaLayoutBox {
            width: 0.0,
            height: 0.0,
            depth: 0.0,
            kind: ArenaBoxKind::Kern,
        });
    }
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph {
            ch,
            size,
            italic,
            font: FontId::Math,
        },
    })
}

/// Wrap a glyph so it is centered on the math axis (arena variant).
fn center_glyph_on_axis_arena(
    glyph_id: LayoutBoxId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let axis = ctx.params.axis_height * size;
    let b = arena.get(glyph_id);
    let glyph_h = b.height;
    let glyph_d = b.depth;
    let glyph_w = b.width;
    let glyph_center = (glyph_h - glyph_d) / 2.0;
    let shift = axis - glyph_center;
    let new_height = (glyph_h + shift).max(0.0);
    let new_depth = (glyph_d - shift).max(0.0);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: shift,
        id: glyph_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: glyph_w,
        height: new_height,
        depth: new_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_text_run_arena(
    text: &str,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
    italic: bool,
    text_style: Option<TextStyle>,
) -> LayoutBoxId {
    let size = ctx.size();
    let font = fonts_registry::font_for_text_run(text_style);
    let mut total_width = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        let m = fonts_registry::text_metrics(ch, italic, font);
        let h = m.height * size;
        let d = m.depth * size;
        max_height = max_height.max(h);
        max_depth = max_depth.max(d);
        total_width += m.width * size;
    }

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::TextRun {
            text: text.to_string(),
            size,
            italic,
            text_style,
        },
    })
}

fn layout_row_arena(
    ast: &AstArena,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let ids = ast.children(span);
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    // Fast path: no newlines => single-line layout (unchanged behaviour)
    let has_newlines = ids
        .iter()
        .any(|&id| matches!(*ast.node(id), CompactNode::Newline));
    if !has_newlines {
        return layout_row_segment_arena(ast, ids, arena, ctx);
    }

    // Multi-line: split at Newline boundaries and layout each line
    let size = ctx.size();
    let baselineskip = 1.2 * size;
    let mut line_ids: Vec<LayoutBoxId> = Vec::new();
    let mut line_start = 0;

    for i in 0..ids.len() {
        if matches!(*ast.node(ids[i]), CompactNode::Newline) {
            let line = layout_row_segment_arena(ast, &ids[line_start..i], arena, ctx);
            line_ids.push(line);
            line_start = i + 1;
        }
    }
    // Last segment after final newline
    let line = layout_row_segment_arena(ast, &ids[line_start..], arena, ctx);
    line_ids.push(line);

    if line_ids.len() == 1 {
        return line_ids[0];
    }

    // Compute row heights/depths with strut minimums, then position
    let mut row_heights = Vec::with_capacity(line_ids.len());
    let mut row_depths = Vec::with_capacity(line_ids.len());
    let mut total_width: f64 = 0.0;
    for &lid in &line_ids {
        let b = arena.get(lid);
        total_width = total_width.max(b.width);
        row_heights.push(b.height.max(0.7 * baselineskip));
        row_depths.push(b.depth.max(0.3 * baselineskip));
    }

    // Phase 1: compute positions top-down
    let mut positions: SmallVec<[(LayoutBoxId, f64); 8]> = SmallVec::new();
    let mut cursor_y: f64 = 0.0;
    for (i, &lid) in line_ids.iter().enumerate() {
        cursor_y += row_heights[i];
        positions.push((lid, cursor_y));
        cursor_y += row_depths[i];
    }

    let total_extent = cursor_y;
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Phase 2: push children with converted dy
    let children_start = arena.begin_children();
    for &(lid, cy) in &positions {
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: box_height - cy,
            id: lid,
        });
    }
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: ArenaBoxKind::VBox { children },
    })
}

/// Layout a contiguous segment of nodes as a single horizontal line (arena path).
fn layout_row_segment_arena(
    ast: &AstArena,
    ids: &[NodeId],
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    // Phase 1: layout all children and collect (box_id, dx) pairs.
    let mut laid_out: SmallVec<[(LayoutBoxId, f64); 16]> = SmallVec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;
    let mut current_ctx = ctx.clone();

    for &id in ids {
        // Handle style switches — update context for subsequent children
        if let CompactNode::StyleSwitch(s) = *ast.node(id) {
            current_ctx = match s {
                0 => current_ctx.with_style(MathStyle::Display),
                1 => current_ctx.with_style(MathStyle::Text),
                2 => current_ctx.with_style(MathStyle::Script),
                _ => current_ctx.with_style(MathStyle::ScriptScript),
            };
            continue;
        }

        // Handle size scale switches — update scale for subsequent children
        if let CompactNode::SizeScale(idx) = *ast.node(id) {
            let scale = SIZE_SCALE_TABLE.get(idx as usize).copied().unwrap_or(1.0);
            current_ctx.size_scale = scale;
            continue;
        }

        // Inter-atom spacing with TeXBook Ch.26 Bin reclassification
        let raw_atom = spacing::classify_node_c(ast, id);
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.unwrap(); // safe: we passed Some
        if let Some(prev) = adj_prev {
            let space =
                spacing::inter_atom_space(prev, cur_atom, current_ctx.effective_spacing_style());
            if space > 0.0 {
                cursor_x += space * current_ctx.size();
            }
        }

        let box_id = layout_node_arena(ast, id, arena, &current_ctx);
        let b = arena.get(box_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        let w = b.width;

        laid_out.push((box_id, cursor_x));
        cursor_x += w;
        prev_atom = Some(cur_atom);
    }

    // Phase 2: now push all children into the arena's children array.
    let children_start = arena.begin_children();
    for &(box_id, dx) in &laid_out {
        arena.push_child(ArenaPositionedBox {
            dx,
            dy: 0.0,
            id: box_id,
        });
    }
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_node_arena(
    ast: &AstArena,
    id: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }
    let ctx = &ctx.deeper();
    match *ast.node(id) {
        CompactNode::Number(sid) => {
            layout_text_run_arena(ast.str_val(sid), arena, ctx, false, None)
        },
        CompactNode::Identifier(sid) => {
            let s = ast.str_val(sid);
            if s.chars().count() == 1 {
                let ch = s.chars().next().expect("non-empty checked");
                alloc_glyph(arena, ch, ctx.size(), is_math_italic_char(ch))
            } else {
                layout_text_run_arena(s, arena, ctx, false, None)
            }
        },
        CompactNode::Operator { name, .. } => {
            layout_text_run_arena(ast.str_val(name), arena, ctx, false, None)
        },
        CompactNode::Symbol(kind, sid) => {
            let s = ast.str_val(sid);
            if let Some(ch) = s.chars().next() {
                if kind == SymbolKind::LargeOp && ctx.style.is_display() {
                    let size = ctx.size() * metrics::display_op_scale(ch);
                    let m = metrics::glyph_metrics_display(ch);
                    alloc_glyph_with_metrics(arena, ch, m, size, false)
                } else {
                    let glyph_id = alloc_glyph(arena, ch, ctx.size(), false);
                    if is_axis_centered_char(ch) {
                        center_glyph_on_axis_arena(glyph_id, arena, ctx)
                    } else {
                        glyph_id
                    }
                }
            } else {
                arena.alloc(ArenaLayoutBox::kern(0.0))
            }
        },
        CompactNode::Text(style, sid) => {
            let italic = matches!(
                style,
                TextStyle::Italic
                    | TextStyle::BoldItalic
                    | TextStyle::SansSerifItalic
                    | TextStyle::SansSerifBoldItalic
            );
            let ts = if matches!(style, TextStyle::Normal) {
                None
            } else {
                Some(style)
            };
            layout_text_run_arena(ast.str_val(sid), arena, ctx, italic, ts)
        },
        CompactNode::Space(r) => arena.alloc(ArenaLayoutBox::kern(r.as_f64() * ctx.size())),
        CompactNode::Row(span) => layout_row_arena(ast, span, arena, ctx),
        CompactNode::Fenced {
            open,
            close,
            content,
        } => layout_fenced_arena(ast, open, close, content, arena, ctx),
        CompactNode::Subscript { base, sub } => layout_subscript_arena(ast, base, sub, arena, ctx),
        CompactNode::Superscript { base, sup } => {
            layout_superscript_arena(ast, base, sup, arena, ctx)
        },
        CompactNode::SubSup { base, sub, sup } => {
            layout_subsup_arena(ast, base, sub, sup, arena, ctx)
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => layout_prescripts_arena(ast, base, pre_sub, pre_sup, arena, ctx),
        CompactNode::Overscript { base, over, .. } => {
            layout_overscript_arena(ast, base, over, arena, ctx)
        },
        CompactNode::Underscript { base, under, .. } => {
            layout_underscript_arena(ast, base, under, arena, ctx)
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => layout_underover_arena(ast, base, under, over, arena, ctx),
        CompactNode::Fraction { kind, num, den } => {
            layout_fraction_arena(ast, kind, num, den, arena, ctx)
        },
        CompactNode::Sqrt(inner) => layout_sqrt_arena(ast, inner, None, arena, ctx),
        CompactNode::Root { index, radicand } => {
            layout_sqrt_arena(ast, radicand, Some(index), arena, ctx)
        },
        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => layout_nary_arena(ast, op, sub, sup, body, limits, arena, ctx),
        CompactNode::Accent {
            base,
            accent,
            position,
            wide,
        } => layout_accent_arena(ast, base, accent, position, wide, arena, ctx),
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            col_seps,
            row_seps,
            array_style,
        } => layout_array_arena(
            ast,
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            col_seps,
            row_seps,
            array_style,
            arena,
            ctx,
        ),
        CompactNode::Phantom(inner, kind) => layout_phantom_arena(ast, inner, kind, arena, ctx),
        CompactNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            let bg = {
                let s = ast.str_val(bg_color);
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_owned())
                }
            };
            let border = {
                let s = ast.str_val(border_color);
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_owned())
                }
            };
            layout_boxed_arena(ast, inner, bg, border, arena, ctx)
        },
        CompactNode::Cancel { stroke, content } => {
            layout_cancel_arena(ast, stroke, content, arena, ctx)
        },
        CompactNode::Scaled { factor, content } => {
            layout_scaled_arena(ast, factor, content, arena, ctx)
        },
        CompactNode::RaiseBox {
            shift,
            height,
            depth,
            extents,
            content,
        } => {
            let child_id = layout_node_arena(ast, content, arena, ctx);
            let child = arena.get(child_id);
            let shift = shift.as_f64() * ctx.size();
            let width = child.width;
            let height = if extents & 1 != 0 {
                height.as_f64() * ctx.size()
            } else {
                (shift + child.height).max(0.0)
            };
            let depth = if extents & 2 != 0 {
                depth.as_f64() * ctx.size()
            } else {
                (child.depth - shift).max(0.0)
            };
            let start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: 0.0,
                dy: shift,
                id: child_id,
            });
            let children = arena.finish_children(start);
            arena.alloc(ArenaLayoutBox {
                width,
                height,
                depth,
                kind: ArenaBoxKind::HBox { children },
            })
        },
        CompactNode::Styled(style, span) => layout_styled_arena(ast, style, span, arena, ctx),
        CompactNode::Color { color, content } => {
            layout_color_arena(ast, color, content, arena, ctx)
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let chosen = match ctx.style {
                MathStyle::Display | MathStyle::DisplayCramped => display,
                MathStyle::Text | MathStyle::TextCramped => text,
                MathStyle::Script | MathStyle::ScriptCramped => script,
                MathStyle::ScriptScript | MathStyle::ScriptScriptCramped => script_script,
            };
            layout_node_arena(ast, chosen, arena, ctx)
        },
        CompactNode::MathAtom { content, .. } => layout_node_arena(ast, content, arena, ctx),
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::StyleSwitch(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::SizeScale(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::Newline => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::Rule {
            width,
            height,
            lift,
        } => {
            let w = width.as_f64() * ctx.size();
            let h = height.as_f64() * ctx.size();
            let lift = lift.as_f64() * ctx.size();
            let rule_id = arena.alloc(ArenaLayoutBox {
                width: w,
                height: h,
                depth: 0.0,
                kind: if w > 1e-6 {
                    ArenaBoxKind::Rule { thickness: h }
                } else {
                    ArenaBoxKind::Kern
                },
            });
            let start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: 0.0,
                dy: lift,
                id: rule_id,
            });
            let children = arena.finish_children(start);
            arena.alloc(ArenaLayoutBox {
                width: w,
                height: (lift + h).max(0.0),
                depth: (-lift).max(0.0),
                kind: ArenaBoxKind::HBox { children },
            })
        },
    }
}

// ── Script and fraction arena layout ─────────────────────────────────

/// Helper: build an HBox of [result, thin_space, body] for nary non-limits mode.
fn build_hbox_arena(
    result_id: LayoutBoxId,
    body_id: LayoutBoxId,
    thin_space: f64,
    arena: &mut LayoutArena,
) -> LayoutBoxId {
    let rw = arena.get(result_id).width;
    let rh = arena.get(result_id).height;
    let rd = arena.get(result_id).depth;
    let bw = arena.get(body_id).width;
    let bh = arena.get(body_id).height;
    let bd = arena.get(body_id).depth;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: result_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: rw + thin_space,
        dy: 0.0,
        id: body_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: rw + thin_space + bw,
        height: rh.max(bh),
        depth: rd.max(bd),
        kind: ArenaBoxKind::HBox { children },
    })
}
