use smallvec::SmallVec;
use super::RenderOptions;
use super::metrics::{self, GlyphMetrics};
use super::params::TexParams;
use super::spacing::{self, AtomType};
use super::style::MathStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
/// Layout engine: converts MathExpr → LayoutBox tree.
/// Applies TeX typesetting rules for positioning.
use crate::ast::*;

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
    /// A single glyph character.
    Glyph { ch: char, size: f64, italic: bool },
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
    },
    /// A run of text characters rendered as a single `<text>` element.
    TextRun { text: String, size: f64, italic: bool },
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
        let m = if italic {
            metrics::glyph_metrics(ch)
        } else {
            metrics::glyph_metrics_upright(ch)
        };
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph { ch, size, italic },
        }
    }

    #[inline]
    fn glyph_with_metrics(ch: char, m: GlyphMetrics, size: f64, italic: bool) -> Self {
        Self {
            width: m.width * size,
            height: m.height * size,
            depth: m.depth * size,
            kind: BoxKind::Glyph { ch, size, italic },
        }
    }
}

/// Returns true for characters that should be vertically centered on the math axis
/// (e.g. \cdots, \ddots) rather than placed at the baseline.
#[inline]
fn is_axis_centered_char(ch: char) -> bool {
    matches!(ch, '\u{22EF}' | '\u{22F1}')  // ⋯ (cdots), ⋱ (ddots)
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

/// Wrap a glyph LayoutBox so it is centered on the math axis.
/// The shift moves the glyph's vertical center to `axis_height * size`.
fn center_glyph_on_axis(glyph: LayoutBox, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let axis = ctx.params.axis_height * size;
    let glyph_center = (glyph.height - glyph.depth) / 2.0;
    let shift = axis - glyph_center;
    let glyph_w = glyph.width;
    let new_height = (glyph.height + shift).max(glyph.height);
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

/// Integral-like operators that default to `\nolimits` (side placement) in TeX.
/// These use superscript/subscript even in display mode, unlike \sum and \prod.
#[inline]
fn is_nolimits_op(ch: char) -> bool {
    matches!(ch, '∫' | '∬' | '∭' | '∮' | '∯' | '∰')
}

/// Check whether a MathNode is a large (n-ary) operator symbol.
fn is_large_op(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(SymbolKind::LargeOp, _))
}

/// Check whether a CompactNode is a large (n-ary) operator symbol.
fn is_large_op_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(arena.node(id), CompactNode::Symbol(SymbolKind::LargeOp, _))
}

/// Extract the first character from a large-op symbol node.
fn large_op_char(node: &MathNode) -> Option<char> {
    match node {
        MathNode::Symbol(SymbolKind::LargeOp, s) => s.chars().next(),
        _ => None,
    }
}

/// Extract the first character from a large-op compact symbol node.
fn large_op_char_c(arena: &AstArena, id: NodeId) -> Option<char> {
    match arena.node(id) {
        CompactNode::Symbol(SymbolKind::LargeOp, sid) => arena.str_val(*sid).chars().next(),
        _ => None,
    }
}

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
const SIZE_SCALE_TABLE: [f64; 10] = [
    0.5, 0.7, 0.8, 0.9, 1.0, 1.2, 1.44, 1.728, 2.074, 2.488,
];


#[derive(Clone)]
struct LayoutCtx<'a> {
    style: MathStyle,
    params: TexParams,
    options: &'a RenderOptions,
    depth: u32,
    size_scale: f64,
}

impl<'a> LayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier() * self.size_scale
    }

    fn with_style(&self, style: MathStyle) -> LayoutCtx<'a> {
        LayoutCtx {
            style,
            params: TexParams::for_size_class(style.size_class()),
            options: self.options,
            depth: self.depth,
            size_scale: self.size_scale,
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
        }
    }
}

// ── Stretchy decoration SVG path helpers ──────────────────────────────

/// Returns `true` if the accent char should be drawn as a stretchy SVG path
/// that scales to the base width, rather than as a fixed glyph.
fn is_stretchy_accent(accent: char) -> bool {
    matches!(
        accent,
        '\u{0302}'  // hat / widehat (combining circumflex)
        | '\u{030C}' // check / widecheck (combining caron)
        | '\u{0303}' // tilde / widetilde (combining tilde)
        | '\u{0304}' // bar / overline (combining macron)
        | '\u{0307}' // dot above (combining dot above)
        | '\u{0308}' // ddot / diaeresis (combining diaeresis)
        | '\u{20D7}' // vec / overrightarrow (combining right arrow above)
        | '\u{2192}' // overrightarrow (rightwards arrow)
        | '\u{2190}' // overleftarrow (leftwards arrow)
        | '\u{2194}' // overleftrightarrow (left right arrow)
        | '\u{21C0}' // overrightharpoon (rightwards harpoon)
        | '\u{21BC}' // overleftharpoon (leftwards harpoon)
    )
}

/// Returns `true` if the over/under symbol char is a stretchy decoration
/// (brace, bracket, overline) that should scale to the base width.
fn is_stretchy_overunder(sym: &str) -> bool {
    matches!(
        sym,
        "\u{23DE}"  // overbrace ⏞
        | "\u{23DF}" // underbrace ⏟
        | "\u{23B4}" // overbracket ⎴
        | "\u{23B5}" // underbracket ⎵
        | "\u{203E}" // overline ‾
    )
}

/// Returns `true` if the symbol string is an overbrace character (U+23DE).
fn is_overbrace_sym(sym: &str) -> bool {
    sym == "\u{23DE}"
}

/// Returns `true` if the symbol string is an underbrace character (U+23DF).
fn is_underbrace_sym(sym: &str) -> bool {
    sym == "\u{23DF}"
}

/// Check if a MathNode is a Symbol containing overbrace ⏞.
fn node_is_overbrace(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if is_overbrace_sym(s))
}

/// Check if a MathNode is a Symbol containing underbrace ⏟.
fn node_is_underbrace(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if is_underbrace_sym(s))
}

/// Check if a CompactNode (via arena) is a Symbol containing overbrace ⏞.
fn node_is_overbrace_c(arena: &AstArena, id: NodeId) -> bool {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        is_overbrace_sym(arena.str_val(sid))
    } else {
        false
    }
}

/// Check if a CompactNode (via arena) is a Symbol containing underbrace ⏟.
fn node_is_underbrace_c(arena: &AstArena, id: NodeId) -> bool {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        is_underbrace_sym(arena.str_val(sid))
    } else {
        false
    }
}

/// Returns true if `ch` is an extensible arrow character used by \xrightarrow etc.
fn is_extensible_arrow(ch: char) -> bool {
    matches!(ch,
        '\u{2190}' // ← leftarrow
        | '\u{2192}' // → rightarrow
        | '\u{21D0}' // ⇐ Leftarrow
        | '\u{21D2}' // ⇒ Rightarrow
        | '\u{2194}' // ↔ leftrightarrow
        | '\u{21D4}' // ⇔ Leftrightarrow
        | '\u{21A9}' // ↩ hookleftarrow
        | '\u{21AA}' // ↪ hookrightarrow
        | '\u{21A6}' // ↦ mapsto
        | '\u{21CC}' // ⇌ rightleftharpoons
        | '\u{21CB}' // ⇋ leftrightharpoons
        | '\u{21A0}' // ↠ twoheadrightarrow
        | '\u{219E}' // ↞ twoheadleftarrow
        | '\u{21BC}' // ↼ leftharpoonup
        | '\u{21BD}' // ↽ leftharpoondown
        | '\u{21C0}' // ⇀ rightharpoonup
        | '\u{21C1}' // ⇁ rightharpoondown
    )
}

/// Check if a CompactNode (in AstArena) is an extensible arrow symbol and return the char.
fn get_extensible_arrow_c(arena: &AstArena, id: NodeId) -> Option<char> {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        let s = arena.str_val(sid);
        if let Some(ch) = s.chars().next()
            && is_extensible_arrow(ch)
        {
            return Some(ch);
        }
    }
    None
}

/// Check if a heap MathNode is an extensible arrow symbol and return the char.
fn get_extensible_arrow_heap(node: &MathNode) -> Option<char> {
    if let MathNode::Symbol(_, s) = node
        && let Some(ch) = s.chars().next()
        && is_extensible_arrow(ch)
    {
        return Some(ch);
    }
    None
}

/// Generate a stretchy SVG path for an extensible arrow character at given width and em-size.
fn stretchy_arrow_svg(ch: char, w: f64, em: f64) -> String {
    let t = 0.02 * em;    // half-thickness of arrow shaft
    let tip = 0.24 * em;  // arrowhead length
    let tw = 0.12 * em;   // arrowhead half-width (perpendicular to shaft)
    let mid_y = 0.22 * em; // vertical center

    match ch {
        // → rightarrow: shaft + right arrowhead
        '\u{2192}' => {
            let ax = w - tip;
            format!("F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ← leftarrow: left arrowhead + shaft
        '\u{2190}' => {
            format!("F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ↔ leftrightarrow: arrowheads on both ends
        '\u{2194}' => {
            let ax = w - tip;
            format!("F:M0,{mid:.4} L{tip:.4},{t1:.4} L{tip:.4},{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L{tip:.4},{y1:.4} L{tip:.4},{t2:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ⇒ double rightarrow: two shafts + right double arrowhead
        '\u{21D2}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!("F:M0,{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{l1:.4} L0,{l1:.4} L0,{l0:.4} L{ax:.4},{l0:.4} L{ax:.4},{u1:.4} L0,{u1:.4} Z", u0 = upper - t, u1 = upper + t, l0 = lower - t, l1 = lower + t, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ⇐ double leftarrow
        '\u{21D0}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            format!("F:M{w:.4},{u0:.4} L{tip:.4},{u0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{tip:.4},{l1:.4} L{w:.4},{l1:.4} L{w:.4},{l0:.4} L{tip:.4},{l0:.4} L{tip:.4},{u1:.4} L{w:.4},{u1:.4} Z", u0 = upper - t, u1 = upper + t, l0 = lower - t, l1 = lower + t, tip = tip, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ⇔ double leftrightarrow
        '\u{21D4}' => {
            let gap = 0.06 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!("F:M0,{mid:.4} L{tip:.4},{t1:.4} L{tip:.4},{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{l1:.4} L{tip:.4},{l1:.4} L{tip:.4},{t2:.4} Z M{tip:.4},{u1:.4} L{ax:.4},{u1:.4} L{ax:.4},{l0:.4} L{tip:.4},{l0:.4} Z", u0 = upper - t, u1 = upper + t, l0 = lower - t, l1 = lower + t, tip = tip, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ↦ mapsto: vertical bar + shaft + right arrowhead
        '\u{21A6}' => {
            let bar_w = 0.04 * em;
            let ax = w - tip;
            format!("F:M0,{t1:.4} L0,{t2:.4} L{bw:.4},{t2:.4} L{bw:.4},{y1:.4} L{ax:.4},{y1:.4} L{ax:.4},{t2b:.4} L{w:.4},{mid:.4} L{ax:.4},{t1b:.4} L{ax:.4},{y0:.4} L{bw:.4},{y0:.4} L{bw:.4},{t1:.4} Z", bw = bar_w, y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw, t1b = mid_y - tw, t2b = mid_y + tw)
        }
        // ↪ hookrightarrow: shaft + right arrowhead (simplified)
        '\u{21AA}' => {
            let ax = w - tip;
            format!("F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ↩ hookleftarrow (simplified)
        '\u{21A9}' => {
            format!("F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{mid:.4} L{tip:.4},{t2:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
        // ⇌ rightleftharpoons: upper right harpoon + lower left harpoon
        '\u{21CC}' => {
            let gap = 0.08 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!("F:M0,{u0:.4} L{ax:.4},{u0:.4} L{ax:.4},{ut:.4} L{w:.4},{u1:.4} L{ax:.4},{u1:.4} L0,{u1:.4} Z M{w:.4},{l0:.4} L{tip:.4},{l0:.4} L{tip:.4},{lt:.4} L0,{l0:.4} L{tip:.4},{l1:.4} L{w:.4},{l1:.4} Z", u0 = upper - t, u1 = upper + t, ut = upper - tw, l0 = lower - t, l1 = lower + t, lt = lower + tw, tip = tip, ax = ax, w = w)
        }
        // ⇋ leftrightharpoons
        '\u{21CB}' => {
            let gap = 0.08 * em;
            let upper = mid_y - gap;
            let lower = mid_y + gap;
            let ax = w - tip;
            format!("F:M{w:.4},{u0:.4} L{tip:.4},{u0:.4} L{tip:.4},{ut:.4} L0,{u1:.4} L{tip:.4},{u1:.4} L{w:.4},{u1:.4} Z M0,{l0:.4} L{ax:.4},{l0:.4} L{ax:.4},{lt:.4} L{w:.4},{l0:.4} L{ax:.4},{l1:.4} L0,{l1:.4} Z", u0 = upper - t, u1 = upper + t, ut = upper - tw, l0 = lower - t, l1 = lower + t, lt = lower + tw, tip = tip, ax = ax, w = w)
        }
        // ↠ twoheadrightarrow
        '\u{21A0}' => {
            let ax1 = w - tip;
            let ax2 = w - tip * 1.6;
            format!("F:M0,{y0:.4} L{ax2:.4},{y0:.4} L{ax2:.4},{t1:.4} L{ax1:.4},{mid:.4} L{ax1:.4},{t1b:.4} L{w:.4},{mid:.4} L{ax1:.4},{t2b:.4} L{ax1:.4},{mid:.4} L{ax2:.4},{t2:.4} L{ax2:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax1 = ax1, ax2 = ax2, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw, t1b = mid_y - tw, t2b = mid_y + tw)
        }
        // ↞ twoheadleftarrow
        '\u{219E}' => {
            let t2x = tip * 1.6;
            format!("F:M{w:.4},{y0:.4} L{t2x:.4},{y0:.4} L{t2x:.4},{t1:.4} L{tip:.4},{mid:.4} L{tip:.4},{t1b:.4} L0,{mid:.4} L{tip:.4},{t2b:.4} L{tip:.4},{mid:.4} L{t2x:.4},{t2:.4} L{t2x:.4},{y1:.4} L{w:.4},{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, t2x = t2x, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw, t1b = mid_y - tw, t2b = mid_y + tw)
        }
        // ↼ leftharpoonup: shaft + upper-left half arrowhead
        '\u{21BC}' => {
            format!("F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L{tip:.4},{t1:.4} L0,{y1:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, w = w, t1 = mid_y - tw)
        }
        // ↽ leftharpoondown: shaft + lower-left half arrowhead
        '\u{21BD}' => {
            format!("F:M{w:.4},{y0:.4} L{tip:.4},{y0:.4} L0,{y0:.4} L{tip:.4},{t2:.4} L{tip:.4},{y1:.4} L{w:.4},{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, tip = tip, w = w, t2 = mid_y + tw)
        }
        // ⇀ rightharpoonup: shaft + upper-right half arrowhead
        '\u{21C0}' => {
            let ax = w - tip;
            format!("F:M0,{y0:.4} L{ax:.4},{y0:.4} L{w:.4},{y0:.4} L{ax:.4},{t1:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, t1 = mid_y - tw)
        }
        // ⇁ rightharpoondown: shaft + lower-right half arrowhead
        '\u{21C1}' => {
            let ax = w - tip;
            format!("F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t2:.4} L{w:.4},{y1:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, t2 = mid_y + tw)
        }
        // Fallback: simple right arrow
        _ => {
            let ax = w - tip;
            format!("F:M0,{y0:.4} L{ax:.4},{y0:.4} L{ax:.4},{t1:.4} L{w:.4},{mid:.4} L{ax:.4},{t2:.4} L{ax:.4},{y1:.4} L0,{y1:.4} Z", y0 = mid_y - t, y1 = mid_y + t, ax = ax, w = w, mid = mid_y, t1 = mid_y - tw, t2 = mid_y + tw)
        }
    }
}

// ---------------------------------------------------------------------------
// KaTeX-derived SVG path data for widehat, widetilde, and widecheck accents.
// Each accent has 4 tiers (progressively wider), selected by base width.
// Paths are from KaTeX's svgGeometry.ts (MIT license).
// ---------------------------------------------------------------------------

/// A pre-drawn accent path with its native viewBox dimensions (in KaTeX units).
struct KatexAccentPath {
    path: &'static str,
    viewbox_w: f64,
    viewbox_h: f64,
}

const WIDEHAT_PATHS: [KatexAccentPath; 4] = [
    // widehat1: viewBox 0 0 1062 239
    KatexAccentPath {
        path: "M529 0h5l519 115c5 1 9 5 9 10 0 1-1 2-1 3l-4 22\
               c-1 5-5 9-11 9h-2L532 67 19 159h-2c-5 0-9-4-11-9l-5-22c-1-6 2-12 8-13z",
        viewbox_w: 1062.0,
        viewbox_h: 239.0,
    },
    // widehat2: viewBox 0 0 2364 300
    KatexAccentPath {
        path: "M1181 0h2l1171 176c6 0 10 5 10 11l-2 23c-1 6-5 10\
               -11 10h-1L1182 67 15 220h-1c-6 0-10-4-11-10l-2-23c-1-6 4-11 10-11z",
        viewbox_w: 2364.0,
        viewbox_h: 300.0,
    },
    // widehat3: viewBox 0 0 2364 360
    KatexAccentPath {
        path: "M1181 0h2l1171 236c6 0 10 5 10 11l-2 23c-1 6-5 10\
               -11 10h-1L1182 67 15 280h-1c-6 0-10-4-11-10l-2-23c-1-6 4-11 10-11z",
        viewbox_w: 2364.0,
        viewbox_h: 360.0,
    },
    // widehat4: viewBox 0 0 2364 420
    KatexAccentPath {
        path: "M1181 0h2l1171 296c6 0 10 5 10 11l-2 23c-1 6-5 10\
               -11 10h-1L1182 67 15 340h-1c-6 0-10-4-11-10l-2-23c-1-6 4-11 10-11z",
        viewbox_w: 2364.0,
        viewbox_h: 420.0,
    },
];

const WIDETILDE_PATHS: [KatexAccentPath; 4] = [
    // tilde1: viewBox 0 0 600 260
    KatexAccentPath {
        path: "M200 55.538c-77 0-168 73.953-177 73.953-3 0-7-2.175-9-5.437L2 97c-1-2-2-4-2-6 0-4 2-7 5-9l20-12C116 12 171 0 207 0c86 0 114 68 191 68 78 0 168-68 177-68 4 0 7 2 9 5l12 19c1 2.175 2 4.35 2 6.525 0 4.35-2 7.613-5 9.788l-19 13.05c-92 63.077-116.937 75.308-183 76.128-68.267.847-113-73.952-191-73.952z",
        viewbox_w: 600.0,
        viewbox_h: 260.0,
    },
    // tilde2: viewBox 0 0 1033 286
    KatexAccentPath {
        path: "M344 55.266c-142 0-300.638 81.316-311.5 86.418-8.01 3.762-22.5 10.91-23.5 5.562L1 120c-1-2-1-3-1-4 0-5 3-9 8-10l18.4-9C160.9 31.9 283 0 358 0c148 0 188 122 331 122s314-97 326-97c4 0 8 2 10 7l7 21.114c1 2.14 1 3.21 1 4.28 0 5.347-3 9.626-7 10.696l-22.3 12.622C852.6 158.372 751 181.476 676 181.476c-149 0-189-126.21-332-126.21z",
        viewbox_w: 1033.0,
        viewbox_h: 286.0,
    },
    // tilde3: viewBox 0 0 2339 306
    KatexAccentPath {
        path: "M786 59C457 59 32 175.242 13 175.242c-6 0-10-3.457-11-10.37L.15 138c-1-7 3-12 10-13l19.2-6.4C378.4 40.7 634.3 0 804.3 0c337 0 411.8 157 746.8 157 328 0 754-112 773-112 5 0 10 3 11 9l1 14.075c1 8.066-.697 16.595-6.697 17.492l-21.052 7.31c-367.9 98.146-609.15 122.696-778.15 122.696-338 0-409-156.573-744-156.573z",
        viewbox_w: 2339.0,
        viewbox_h: 306.0,
    },
    // tilde4: viewBox 0 0 2340 312
    KatexAccentPath {
        path: "M786 58C457 58 32 177.487 13 177.487c-6 0-10-3.345-11-10.035L.15 143c-1-7 3-12 10-13l22-6.7C381.2 35 637.15 0 807.15 0c337 0 409 177 744 177 328 0 754-127 773-127 5 0 10 3 11 9l1 14.794c1 7.805-3 13.38-9 14.495l-20.7 5.574c-366.85 99.79-607.3 139.372-776.3 139.372-338 0-409-175.236-744-175.236z",
        viewbox_w: 2340.0,
        viewbox_h: 312.0,
    },
];

const WIDECHECK_PATHS: [KatexAccentPath; 4] = [
    // widecheck1: viewBox 0 0 1062 239
    KatexAccentPath {
        path: "M529,159h5l519,-115c5,-1,9,-5,9,-10c0,-1,-1,-2,-1,-3l-4,-22c-1,\
               -5,-5,-9,-11,-9h-2l-512,92l-513,-92h-2c-5,0,-9,4,-11,9l-5,22c-1,6,2,12,8,13z",
        viewbox_w: 1062.0,
        viewbox_h: 239.0,
    },
    // widecheck2: viewBox 0 0 2364 300
    KatexAccentPath {
        path: "M1181,220h2l1171,-176c6,0,10,-5,10,-11l-2,-23c-1,-6,-5,-10,\
               -11,-10h-1l-1168,153l-1167,-153h-1c-6,0,-10,4,-11,10l-2,23c-1,6,4,11,10,11z",
        viewbox_w: 2364.0,
        viewbox_h: 300.0,
    },
    // widecheck3: viewBox 0 0 2364 360
    KatexAccentPath {
        path: "M1181,280h2l1171,-236c6,0,10,-5,10,-11l-2,-23c-1,-6,-5,-10,\
               -11,-10h-1l-1168,213l-1167,-213h-1c-6,0,-10,4,-11,10l-2,23c-1,6,4,11,10,11z",
        viewbox_w: 2364.0,
        viewbox_h: 360.0,
    },
    // widecheck4: viewBox 0 0 2364 420
    KatexAccentPath {
        path: "M1181,340h2l1171,-296c6,0,10,-5,10,-11l-2,-23c-1,-6,-5,-10,\
               -11,-10h-1l-1168,273l-1167,-273h-1c-6,0,-10,4,-11,10l-2,23c-1,6,4,11,10,11z",
        viewbox_w: 2364.0,
        viewbox_h: 420.0,
    },
];

/// Select a KaTeX accent tier (0-3) based on base width in em.
fn katex_accent_tier(w: f64) -> usize {
    if w < 1.0 {
        0
    } else if w < 2.0 {
        1
    } else if w < 3.0 {
        2
    } else {
        3
    }
}

/// Scale an SVG path string by independent x and y factors.
///
/// Parses SVG path commands and multiplies each coordinate by the
/// appropriate scale factor (`sx` for x-coordinates, `sy` for y-coordinates).
/// Supports commands: M, L, H, V, C, S, Q, T, A, Z (both cases).
fn scale_svg_path(path: &str, sx: f64, sy: f64) -> String {
    let mut result = String::with_capacity(path.len() + 64);
    let bytes = path.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Current command determines how to interpret parameters.
    // 'x' means scale by sx, 'y' means scale by sy, 'n' means no scale.
    // For most commands, coords alternate x, y.
    let mut cmd = b'M';
    let mut param_index: usize = 0;

    while i < len {
        let b = bytes[i];
        if b.is_ascii_alphabetic() {
            cmd = b;
            param_index = 0;
            result.push(b as char);
            i += 1;
            continue;
        }
        // Skip whitespace and commas between tokens
        if b == b' ' || b == b',' || b == b'\n' || b == b'\r' || b == b'\t' {
            result.push(b as char);
            i += 1;
            continue;
        }
        // Try to parse a number
        if b == b'-' || b == b'+' || b == b'.' || b.is_ascii_digit() {
            let start = i;
            // Optional sign
            if bytes[i] == b'-' || bytes[i] == b'+' {
                i += 1;
            }
            // Integer part
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Decimal part
            if i < len && bytes[i] == b'.' {
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let num_str = &path[start..i];
            if let Ok(val) = num_str.parse::<f64>() {
                let cmd_upper = cmd.to_ascii_uppercase();
                let scaled = match cmd_upper {
                    b'H' => val * sx,
                    b'V' => val * sy,
                    b'Z' => val, // shouldn't happen
                    b'A' => {
                        // Arc: rx ry rotation large-arc sweep x y
                        // Params 0,1 = rx,ry (scale); 2 = rotation (no scale);
                        // 3,4 = flags (no scale); 5,6 = x,y (scale)
                        let pi = param_index % 7;
                        match pi {
                            0 => val * sx,
                            1 => val * sy,
                            2..=4 => val,
                            5 => val * sx,
                            6 => val * sy,
                            _ => val,
                        }
                    }
                    // M, L, C, S, Q, T: alternating x, y
                    _ => {
                        if param_index.is_multiple_of(2) {
                            val * sx
                        } else {
                            val * sy
                        }
                    }
                };
                // Ensure a separator before this number if the previous char
                // could be mistaken as part of the same token (digit or '.').
                if let Some(&last) = result.as_bytes().last()
                    && (last.is_ascii_digit() || last == b'.')
                {
                    result.push(' ');
                }
                // Format with up to 4 decimal places, trimming trailing zeros
                let formatted = format!("{:.4}", scaled);
                let formatted = formatted.trim_end_matches('0');
                let formatted = formatted.trim_end_matches('.');
                result.push_str(formatted);
            } else {
                // Fallback: push the raw string
                result.push_str(num_str);
            }
            param_index += 1;
            continue;
        }
        // Anything else, just copy
        result.push(b as char);
        i += 1;
    }
    result
}

/// Build a filled, scaled KaTeX accent path for the given base dimensions.
fn katex_accent_scaled(paths: &[KatexAccentPath; 4], w: f64, h: f64) -> String {
    let tier = katex_accent_tier(w);
    let data = &paths[tier];
    let sx = w / data.viewbox_w;
    let sy = h / data.viewbox_h;
    let scaled = scale_svg_path(data.path, sx, sy);
    format!("F:{scaled}")
}

/// Generate an SVG path string for a stretchy accent at the given width and height.
/// Coordinates are in em units with (0,0) at top-left.
/// Paths prefixed with "F:" are filled shapes (rendered without stroke);
/// others use stroke-based rendering.
/// Generate a filled circle SVG path for dot accents.
/// `r` is the radius in em-equivalent units. The path fits in a 2r × 2r box.
fn dot_circle_path(r: f64) -> String {
    let d = r * 2.0;
    format!("F:M{d:.4},{r:.4} a{r:.4},{r:.4} 0 1 1 -{d:.4},0 a{r:.4},{r:.4} 0 1 1 {d:.4},0 Z")
}

fn stretchy_accent_path(accent: char, w: f64, h: f64) -> String {
    let stroke = 0.04; // matches emit.rs stroke-width
    match accent {
        // Hat (U+0302): use KaTeX widehat paths
        '\u{0302}' => katex_accent_scaled(&WIDEHAT_PATHS, w, h),
        // Check/caron (U+030C): use KaTeX widecheck paths
        '\u{030C}' => katex_accent_scaled(&WIDECHECK_PATHS, w, h),
        // Tilde (U+0303): use KaTeX widetilde paths
        '\u{0303}' => katex_accent_scaled(&WIDETILDE_PATHS, w, h),
        // Right arrow (overrightarrow, vec)
        '\u{20D7}' | '\u{2192}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let tip = w;
            let shaft_end = w - head;
            format!(
                "M0,{mid:.4} L{shaft_end:.4},{mid:.4} M{tip:.4},{mid:.4} L{se:.4},{t:.4} M{tip:.4},{mid:.4} L{se:.4},{b:.4}",
                se = shaft_end,
                t = mid - head,
                b = mid + head,
            )
        },
        // Left arrow (overleftarrow)
        '\u{2190}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_start = head;
            format!(
                "M{shaft_start:.4},{mid:.4} L{w:.4},{mid:.4} M0,{mid:.4} L{shaft_start:.4},{t:.4} M0,{mid:.4} L{shaft_start:.4},{b:.4}",
                t = mid - head,
                b = mid + head,
            )
        },
        // Left-right arrow
        '\u{2194}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.12).max(stroke * 3.0);
            let shaft_start = head;
            let shaft_end = w - head;
            format!(
                "M{shaft_start:.4},{mid:.4} L{shaft_end:.4},{mid:.4} \
                 M0,{mid:.4} L{shaft_start:.4},{t:.4} M0,{mid:.4} L{shaft_start:.4},{b:.4} \
                 M{w:.4},{mid:.4} L{shaft_end:.4},{t:.4} M{w:.4},{mid:.4} L{shaft_end:.4},{b:.4}",
                t = mid - head,
                b = mid + head,
            )
        },
        // Right harpoon (upper barb)
        '\u{21C0}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_end = w - head;
            format!(
                "M0,{mid:.4} L{shaft_end:.4},{mid:.4} M{w:.4},{mid:.4} L{shaft_end:.4},{t:.4}",
                t = mid - head,
            )
        },
        // Left harpoon (upper barb)
        '\u{21BC}' => {
            let mid = h / 2.0;
            let head = h.min(w * 0.15).max(stroke * 3.0);
            let shaft_start = head;
            format!(
                "M{shaft_start:.4},{mid:.4} L{w:.4},{mid:.4} M0,{mid:.4} L{shaft_start:.4},{t:.4}",
                t = mid - head,
            )
        },
        // Fallback: horizontal line
        _ => {
            let mid = h / 2.0;
            format!("M0,{mid:.4} L{w:.4},{mid:.4}")
        },
    }
}

/// Generate an SVG path string for a stretchy over/under decoration.
/// Coordinates are in em units with (0,0) at top-left.
fn stretchy_overunder_path(sym: &str, w: f64, h: f64) -> String {
    // Thickness for filled brace shapes (proportion of height)
    let t = (h * 0.18).max(0.02);
    match sym {
        // Overbrace ⏞ — filled curly brace above, tip points up
        "\u{23DE}" => {
            let mid = w / 2.0;
            let q = w * 0.25;
            let tq = w * 0.75;
            // Outer curve: bottom-left to tip(top) to bottom-right
            // Inner curve: bottom-right (offset up by t) back to bottom-left (offset up by t)
            format!(
                "F:M0,{h:.4} Q{q:.4},{h:.4} {mid:.4},0 Q{tq:.4},{h:.4} {w:.4},{h:.4} \
                 L{w:.4},{ht:.4} Q{tq:.4},{ht:.4} {mid:.4},{t:.4} Q{q:.4},{ht:.4} 0,{ht:.4} Z",
                h = h, q = q, mid = mid, tq = tq, w = w, ht = h - t, t = t
            )
        },
        // Underbrace ⏟ — filled curly brace below, tip points down
        "\u{23DF}" => {
            let mid = w / 2.0;
            let q = w * 0.25;
            let tq = w * 0.75;
            // Outer curve: top-left to tip(bottom) to top-right
            // Inner curve: top-right (offset down by t) back to top-left (offset down by t)
            let hi = h - t;
            format!(
                "F:M0,0 Q{q:.4},0 {mid:.4},{h:.4} Q{tq:.4},0 {w:.4},0 \
                 L{w:.4},{t:.4} Q{tq:.4},{t:.4} {mid:.4},{hi:.4} Q{q:.4},{t:.4} 0,{t:.4} Z",
                h = h, q = q, mid = mid, tq = tq, w = w, t = t, hi = hi
            )
        },
        // Overbracket ⎴ — square bracket above (filled)
        "\u{23B4}" => {
            format!(
                "F:M0,{h:.4} L0,0 L{w:.4},0 L{w:.4},{h:.4} \
                 L{wi:.4},{h:.4} L{wi:.4},{t:.4} L{t:.4},{t:.4} L{t:.4},{h:.4} Z",
                h = h, w = w, wi = w - t, t = t
            )
        },
        // Underbracket ⎵ — square bracket below (filled)
        "\u{23B5}" => {
            format!(
                "F:M0,0 L0,{h:.4} L{w:.4},{h:.4} L{w:.4},0 \
                 L{wi:.4},0 L{wi:.4},{hi:.4} L{t:.4},{hi:.4} L{t:.4},0 Z",
                h = h, w = w, wi = w - t, hi = h - t, t = t
            )
        },
        // Overline ‾ — horizontal rule
        "\u{203E}" => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        _ => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
    }
}

/// Entry point: layout an entire math expression.
pub fn layout_expr(expr: &MathExpr, style: MathStyle, options: &RenderOptions) -> LayoutBox {
    let ctx = LayoutCtx {
        style,
        params: TexParams::for_size_class(style.size_class()),
        options,
        depth: 0,
        size_scale: 1.0,
    };
    layout_row(&expr.nodes, &ctx)
}

fn layout_row(nodes: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    if nodes.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
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
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.style);
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
        MathNode::Number(s) => layout_text_run(s, ctx, false),
        MathNode::Identifier(s) => {
            if s.chars().count() == 1 {
                // Single-char identifiers are italic; chars().next() is safe
                // because chars().count() == 1 guarantees at least one char
                let ch = s.chars().next().expect("non-empty checked");
                LayoutBox::glyph(ch, ctx.size(), is_math_italic_char(ch))
            } else {
                layout_text_run(s, ctx, false)
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
        MathNode::Text(_, s) => layout_text_run(s, ctx, false),
        MathNode::Space(r) => LayoutBox::kern(r.as_f64() * ctx.size()),
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
        } => layout_accent(base, *accent, *position, ctx),
        MathNode::Array { columns, rows, arraystretch, add_jot } => layout_array(columns, rows, *arraystretch, *add_jot, ctx),
        MathNode::Phantom(content) => layout_phantom(content, ctx),
        MathNode::Boxed(content) => layout_boxed(content, ctx),
        MathNode::Cancel { stroke, content } => layout_cancel(*stroke, content, ctx),
        MathNode::Scaled { factor, content } => layout_scaled(*factor, content, ctx),
        MathNode::Styled(style, children) => layout_styled(*style, children, ctx),
    }
}

// === Leaf node layouts ===

fn layout_text_run(text: &str, ctx: &LayoutCtx, italic: bool) -> LayoutBox {
    let size = ctx.size();
    let mut total_width = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        // Use upright metrics for non-italic text (operator names, numbers, \text{}).
        // Italic and upright fonts have significantly different character widths
        // (e.g. 'i' is 0.278em upright vs 0.345em italic — 24% wider in italic).
        let m = if italic {
            metrics::glyph_metrics(ch)
        } else {
            metrics::glyph_metrics_upright(ch)
        };
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
        kind: BoxKind::TextRun { text: text.to_string(), size, italic },
    }
}

fn layout_operator(op: &OperatorAttr, ctx: &LayoutCtx) -> LayoutBox {
    layout_text_run(&op.name, ctx, false)
}

// === Structure node layouts ===

/// Layout `\overbrace{content}^{annotation}` — stack annotation centered above the brace.
/// Produces: annotation (top) → brace → content (bottom), all centered.
fn layout_overbrace_annotated(
    inner_base: &MathNode,
    _over: &MathNode,
    annotation: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let p = &ctx.params;

    // 1. Layout the inner content
    let content_box = layout_node(inner_base, ctx);
    let content_w = content_box.width;

    // 2. Create brace decoration
    let sym = "\u{23DE}";
    let brace_box = make_stretchy_overunder_box(sym, content_w.max(0.5 * size), size);
    let brace_h = brace_box.height;

    // 3. Layout annotation in script style
    let ann_ctx = ctx.with_style(ctx.style.sup());
    let ann_box = layout_node(annotation, &ann_ctx);

    // 4. Compute vertical stacking
    let gap1 = p.big_op_spacing[2] * size; // gap between content and brace
    let gap2 = p.big_op_spacing[2] * size; // gap between brace and annotation

    let max_w = content_w.max(brace_box.width).max(ann_box.width);
    let content_dx = (max_w - content_box.width) / 2.0;
    let brace_dx = (max_w - brace_box.width) / 2.0;
    let ann_dx = (max_w - ann_box.width) / 2.0;

    // content is at baseline (dy=0). Brace goes above content, annotation above brace.
    let brace_dy = content_box.height + gap1 + brace_box.depth;
    let ann_dy = brace_dy + brace_h + gap2 + ann_box.depth;

    let total_height = ann_dy + ann_box.height;
    let total_depth = content_box.depth;

    let children = vec![
        PositionedBox::new(content_dx, 0.0, content_box),
        PositionedBox::new(brace_dx, brace_dy, brace_box),
        PositionedBox::new(ann_dx, ann_dy, ann_box),
    ];

    LayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

/// Layout `\underbrace{content}_{annotation}` — stack annotation centered below the brace.
/// Produces: content (top) → brace → annotation (bottom), all centered.
fn layout_underbrace_annotated(
    inner_base: &MathNode,
    _under: &MathNode,
    annotation: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let p = &ctx.params;

    // 1. Layout the inner content
    let content_box = layout_node(inner_base, ctx);
    let content_w = content_box.width;

    // 2. Create brace decoration
    let sym = "\u{23DF}";
    let brace_box = make_stretchy_overunder_box(sym, content_w.max(0.5 * size), size);
    let brace_h = brace_box.height;

    // 3. Layout annotation in script style
    let ann_ctx = ctx.with_style(ctx.style.sub());
    let ann_box = layout_node(annotation, &ann_ctx);

    // 4. Compute vertical stacking
    let gap1 = p.big_op_spacing[4] * size; // gap between content and brace
    let gap2 = p.big_op_spacing[4] * size; // gap between brace and annotation

    let max_w = content_w.max(brace_box.width).max(ann_box.width);
    let content_dx = (max_w - content_box.width) / 2.0;
    let brace_dx = (max_w - brace_box.width) / 2.0;
    let ann_dx = (max_w - ann_box.width) / 2.0;

    // content is at baseline (dy=0). Brace goes below content, annotation below brace.
    let brace_dy = content_box.depth + gap1 + brace_h;
    let ann_dy = brace_dy + brace_box.depth + gap2 + ann_box.height;

    let total_height = content_box.height;
    let total_depth = ann_dy + ann_box.depth;

    let children = vec![
        PositionedBox::new(content_dx, 0.0, content_box),
        PositionedBox::new(brace_dx, -brace_dy, brace_box),
        PositionedBox::new(ann_dx, -ann_dy, ann_box),
    ];

    LayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

/// TeXBook Rule 18a: superscript positioning.
/// When the base is a large operator in display mode, place the superscript above as a limit.
fn layout_superscript(base: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, None, Some(sup), ctx);
    }

    // Overbrace annotation: Superscript { base: Overscript { over: "⏞" }, sup: annotation }
    // Place annotation centered above the brace, not as a regular superscript.
    if let MathNode::Overscript { base: inner_base, over, .. } = base
        && node_is_overbrace(over)
    {
        return layout_overbrace_annotated(inner_base, over, sup, ctx);
    }

    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop, scaled by child/parent size ratio
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    // Superscript shift: max of sup1/sup2/sup3, base.height - sup_drop, x_height/4 + sup.depth
    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    // Italic correction: shift superscript right by the base glyph's italic correction
    let ic = italic_correction(&base_box);

    let total_width = base_box.width + ic + sup_box.width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    // Capture dimensions before moving base_box into children vec (avoids clone)
    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 18a: subscript positioning.
/// When the base is a large operator in display mode, place the subscript below as a limit.
fn layout_subscript(base: &MathNode, sub: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, Some(sub), None, ctx);
    }

    // Underbrace annotation: Subscript { base: Underscript { under: "⏟" }, sub: annotation }
    // Place annotation centered below the brace, not as a regular subscript.
    if let MathNode::Underscript { base: inner_base, under, .. } = base
        && node_is_underbrace(under)
    {
        return layout_underbrace_annotated(inner_base, under, sub, ctx);
    }

    let base_box = layout_node(base, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop, scaled by child/parent size ratio
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height / 5.0));

    // Italic correction: shift subscript left by the base glyph's italic correction
    let ic = italic_correction(&base_box);
    let sub_dx = (base_box.width - ic).max(0.0);

    let total_width = sub_dx + sub_box.width;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 18c: combined sub+superscript.
/// When the base is a large operator in display mode, place limits above/below.
fn layout_subsup(base: &MathNode, sub: &MathNode, sup: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place limits above/below
    if is_large_op(base) && ctx.style.is_display() {
        return layout_big_op_limits_heap(base, Some(sub), Some(sup), ctx);
    }
    let base_box = layout_node(base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    // Superscript shift
    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    // Subscript shift
    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    // KaTeX puts the entire gap adjustment into the subscript, then checks sup bottom vs xHeight
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        // Push subscript down to meet minimum gap (entire adjustment into sub)
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    // Then check if sup bottom is above 0.8 * xHeight
    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    // Italic correction: superscript shifts right, subscript shifts left by half
    let ic = italic_correction(&base_box);
    let sup_dx = base_box.width + ic;
    let sub_dx = (base_box.width - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Layout a big operator (LargeOp symbol) with limits above/below (heap path).
/// Used when SubSup/Subscript/Superscript has a large-op base in display mode.
fn layout_big_op_limits_heap(
    base: &MathNode,
    sub: Option<&MathNode>,
    sup: Option<&MathNode>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char(base).unwrap_or('\u{2211}');
    let op_size = size * metrics::display_op_scale(op_ch);
    let m = metrics::glyph_metrics_display(op_ch);
    let op_box = LayoutBox::glyph_with_metrics(op_ch, m, op_size, false);

    let mut children = Vec::new();
    let mut total_h = op_box.height;
    let mut total_d = op_box.depth;
    let mut max_w = op_box.width;

    let sup_box = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node(s, &sup_ctx)
    });
    let sub_box = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node(s, &sub_ctx)
    });

    // KaTeX big_op_spacing: [0]=sup gap, [1]=min sup clearance,
    // [2]=min sub clearance, [3]=sub gap, [4]=top/bottom padding
    let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
    let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
    let padding = p.big_op_spacing[4] * size;

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + sup_gap + padding;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + sub_gap + padding;
    }

    // Center op horizontally
    let op_dx = (max_w - op_box.width) / 2.0;

    if let Some(sb) = sup_box {
        let sup_dx = (max_w - sb.width) / 2.0;
        let sup_dy = op_box.height + sup_gap + sb.depth;
        children.push(PositionedBox::new(sup_dx, sup_dy, sb));
    }
    let op_depth = op_box.depth;
    children.push(PositionedBox::new(op_dx, 0.0, op_box));
    if let Some(sb) = sub_box {
        let sub_dx = (max_w - sb.width) / 2.0;
        let sub_dy = -(op_depth + sub_gap + sb.height);
        children.push(PositionedBox::new(sub_dx, sub_dy, sb));
    }

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

/// TeXBook Rule 15: fraction layout.
fn layout_fraction(
    kind: FractionKind,
    num: &MathNode,
    den: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Override context style for \dfrac (Display) and \tfrac (Inline)
    let override_ctx = match kind {
        FractionKind::Display => Some(ctx.with_style(MathStyle::Display)),
        FractionKind::Inline => Some(ctx.with_style(MathStyle::Text)),
        _ => None,
    };
    let ctx = override_ctx.as_ref().unwrap_or(ctx);

    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
    let num_box = layout_node(num, &num_ctx);
    let den_box = layout_node(den, &den_ctx);

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    // Shifts from axis (num3 used for no-bar fractions in non-display styles)
    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
    } else if bar_thickness == 0.0 {
        (p.num3 * size, p.denom2 * size)
    } else {
        (p.num2 * size, p.denom2 * size)
    };

    // Ensure minimum clearance
    let axis = p.axis_height * size;
    let mut num_up = num_shift;
    let mut den_down = den_shift;

    if bar_thickness > 0.0 {
        // Clearance from bar: at least 3*bar_thickness in display, 1*bar_thickness otherwise
        let min_clearance = if ctx.style.is_display() {
            3.0 * bar_thickness
        } else {
            bar_thickness
        };

        let num_clearance = (num_up - num_box.depth) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_box.height - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
        // No bar: ensure gap between num bottom and den top
        let min_gap = if ctx.style.is_display() {
            7.0 * p.rule_thickness * size
        } else {
            3.0 * p.rule_thickness * size
        };
        let gap = (num_up - num_box.depth) - (den_box.height - den_down);
        if gap < min_gap {
            let adjust = (min_gap - gap) / 2.0;
            num_up += adjust;
            den_down += adjust;
        }
    }

    let frac_width = num_box.width.max(den_box.width);
    // Add null-delimiter-space side padding to prevent content from touching adjacent glyphs.
    // Standard TeX adds ~0.12em on each side of a fraction.
    let side_padding = 0.12 * size;
    let padded_frac_width = frac_width + 2.0 * side_padding;
    let total_height = num_up + num_box.height;
    let total_depth = den_down + den_box.depth;

    // Center numerator and denominator within the padded width
    let num_dx = side_padding + (frac_width - num_box.width) / 2.0;
    let den_dx = side_padding + (frac_width - den_box.width) / 2.0;

    let mut children = vec![
        PositionedBox::new(num_dx, num_up, num_box),
        PositionedBox::new(den_dx, -den_down, den_box),
    ];

    // Add the fraction bar
    if bar_thickness > 0.0 {
        let bar = LayoutBox::rule(padded_frac_width, bar_thickness);
        children.push(PositionedBox::new(0.0, axis, bar));
    }

    LayoutBox {
        width: padded_frac_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

/// Generate a **filled** SVG path string for a radical sign (square root
/// symbol), matching the visual weight of KaTeX's font-traced radical.
///
/// The path is in local em coordinates where (0,0) is the top-left of the
/// bounding box.  `w` is the width of the radical symbol and `h` is the
/// total height it must cover.
///
/// Instead of a thin stroked line, this draws the full outline of a solid
/// radical sign:
///   - A small horizontal tail on the far left
///   - A diagonal downstroke to the bottom vertex
///   - A steep upstroke to the vinculum connection at the top
///   - A vinculum (horizontal bar) extending to the right edge
///   - An inner return path giving the shape proper thickness
///
/// The returned string is prefixed with "F:" so the emitter renders it as
/// a filled shape (`fill="currentColor"`) rather than a stroked path.
fn radical_path(w: f64, h: f64) -> String {
    // Thickness of the vinculum and the baked-in stroke width (in em).
    let t = 0.04;

    // --- Key x landmarks (as fractions of w) ---
    let tail_x = 0.08 * w; // right end of the small horizontal tail
    let bottom_x = 0.26 * w; // x of the bottom vertex of the V
    let top_x = 0.42 * w; // x where the upstroke meets the vinculum

    // --- Vertical position of the tail ---
    let tail_y = 0.55 * h;

    // === Outer edge (clockwise) ===
    // P0: tail start (left edge, top of tail stroke)
    let ox0 = 0.0;
    let oy0 = tail_y - t * 0.5;
    // P1: tail end, slight downward slope
    let ox1 = tail_x;
    let oy1 = tail_y + t;
    // P2: bottom-left of V (outer)
    let ox2 = bottom_x - t * 0.3;
    let oy2 = h;
    // P3: top of upstroke (outer left edge), meets vinculum
    let ox3 = top_x - t * 0.5;
    let oy3 = 0.0;
    // P4: vinculum right end, top edge
    let vx0 = w;
    let vy0 = 0.0;

    // === Inner edge (counter-clockwise, forming the return path) ===
    // P5: vinculum right end, bottom edge
    let vx1 = w;
    let vy1 = t;
    // P6: top of upstroke (inner right edge)
    let ix0 = top_x + t * 0.5;
    let iy0 = t;
    // P7: bottom-right of V (inner, slightly above and to the right)
    let ix1 = bottom_x + t * 0.5;
    let iy1 = h - t * 0.5;
    // P8: inner tail end
    let ix2 = tail_x + t * 0.3;
    let iy2 = tail_y + t * 0.5;
    // P9: inner tail start (left edge, bottom of tail stroke)
    let ix3 = 0.0;
    let iy3 = tail_y + t * 0.5;

    format!(
        "F:M{ox0:.4},{oy0:.4} L{ox1:.4},{oy1:.4} L{ox2:.4},{oy2:.4} \
         L{ox3:.4},{oy3:.4} L{vx0:.4},{vy0:.4} L{vx1:.4},{vy1:.4} \
         L{ix0:.4},{iy0:.4} L{ix1:.4},{iy1:.4} L{ix2:.4},{iy2:.4} \
         L{ix3:.4},{iy3:.4} Z",
    )
}

/// TeXBook Rule 11: square root.
fn layout_sqrt(radicand: &MathNode, index: Option<&MathNode>, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let cramped_ctx = ctx.with_style(ctx.style.cramped());
    let inner = layout_node(radicand, &cramped_ctx);

    let rule_thickness = p.rule_thickness * size;

    // KaTeX-compatible clearance: theta + phi/4
    // where phi = xHeight in display mode, theta otherwise
    let phi = if ctx.style.is_display() {
        p.x_height * size // ~0.431em — the x-height
    } else {
        rule_thickness // 0.04em — same as theta
    };
    let extra_clearance = rule_thickness + phi / 4.0;

    // Total height the radical must cover
    let inner_total = inner.height + inner.depth + extra_clearance + rule_thickness;

    // Minimum surd height of 1.0em (KaTeX's "small" surd minimum)
    let min_surd_height = 1.0 * size;
    let surd_height = inner_total.max(min_surd_height);

    // If surd is taller than needed, redistribute extra space evenly
    let extra_clearance = if surd_height > inner_total {
        extra_clearance + (surd_height - inner_total) / 2.0
    } else {
        extra_clearance
    };
    // Recompute with potentially larger clearance
    let _inner_total = surd_height;

    // Radical sign
    let radical_width = 0.833 * size; // approximate radical sign width
    let radical = LayoutBox {
        width: radical_width,
        height: surd_height,
        depth: 0.0,
        kind: BoxKind::SvgPath(radical_path(radical_width, surd_height)),
    };

    // Vinculum (horizontal rule on top)
    let vinculum = LayoutBox::rule(inner.width, rule_thickness);

    let shift_up = inner.height + extra_clearance + rule_thickness;

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_depth = inner.depth;
    let mut children = vec![
        PositionedBox::new(0.0, 0.0, radical),
        PositionedBox::new(radical_width, shift_up, vinculum),
        PositionedBox::new(radical_width, 0.0, inner),
    ];

    let mut total_width = radical_width + inner_width;
    let total_height = shift_up + rule_thickness;
    let total_depth = inner_depth;

    // Optional root index
    if let Some(idx) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box = layout_node(idx, &idx_ctx);
        let idx_shift = total_height * 0.6; // raise index
        let idx_width = idx_box.width;
        children.push(PositionedBox::new(0.0, idx_shift, idx_box));
        // Adjust if index overflows to the left
        let idx_overflow = idx_width.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
            // Shift everything right by extra
            for child in &mut children[..3] {
                child.dx += extra;
            }
            total_width += extra;
        }
    }

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

// === Remaining node layouts ===

fn layout_fenced(open: &str, close: &str, content: &[FencedItem], ctx: &LayoutCtx) -> LayoutBox {
    // Layout content
    let inner_nodes: Vec<&MathNode> = content
        .iter()
        .filter_map(|item| match item {
            FencedItem::Element(n) => Some(n),
            FencedItem::Middle(_) => None,
        })
        .collect();

    let mut inner_children = Vec::new();
    for n in &inner_nodes {
        inner_children.push(layout_node(n, ctx));
    }

    // Compute max height/depth of content
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for child in &inner_children {
        max_h = max_h.max(child.height);
        max_d = max_d.max(child.depth);
    }

    // Axis-centered target height (KaTeX-compatible): delimiters extend
    // symmetrically around the math axis, ensuring proper visual balance.
    let axis = ctx.params.axis_height * ctx.size();
    let max_dist_from_axis = (max_h - axis).max(max_d + axis);
    let target_h = (2.0 * max_dist_from_axis).max(max_h + max_d);
    // Axis-centered delimiter box dimensions
    let delim_h = (axis + max_dist_from_axis).max(max_h);
    let delim_d = (max_dist_from_axis - axis).max(max_d);

    // Compute delimiter width.
    // For path-based delimiters (tall content), use the path width which stays
    // thin. For text-based delimiters (normal size), use proportional scaling.
    use super::delimiter_paths;

    // Build positioned children
    let mut children = Vec::new();
    let mut cursor_x = 0.0;

    // Open delimiter
    if !open.is_empty()
        && open != "."
        && let Some(ch) = open.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim = LayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    // Content -- capture width before moving each child
    for child in inner_children {
        let w = child.width;
        children.push(PositionedBox::new(cursor_x, 0.0, child));
        cursor_x += w;
    }

    // Close delimiter
    if !close.is_empty()
        && close != "."
        && let Some(ch) = close.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim = LayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    LayoutBox {
        width: cursor_x,
        height: max_h,
        depth: max_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_nary(
    op: char,
    sub: &Option<Box<MathNode>>,
    sup: &Option<Box<MathNode>>,
    body: &MathNode,
    limits: LimitStyle,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Large op in display mode: use per-operator scale matching KaTeX Size2/Size1
    let (op_size, m) = if ctx.style.is_display() {
        (size * metrics::display_op_scale(op), metrics::glyph_metrics_display(op))
    } else {
        (size, metrics::glyph_metrics(op))
    };
    let op_box = LayoutBox::glyph_with_metrics(op, m, op_size, false);

    let body_box = layout_node(body, ctx);
    // Capture body dimensions before any move
    let body_w = body_box.width;
    let body_h = body_box.height;
    let body_d = body_box.depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        // Integrals default to side-limits (nolimits); sums/products to above/below in display.
        LimitStyle::Default => ctx.style.is_display() && !is_nolimits_op(op),
    };

    if use_limits {
        // Limits above/below
        let mut children = Vec::new();
        let mut total_h = op_box.height;
        let mut total_d = op_box.depth;
        let mut max_w = op_box.width;

        let sup_box = sup.as_ref().map(|s| {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            layout_node(s, &sup_ctx)
        });
        let sub_box = sub.as_ref().map(|s| {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            layout_node(s, &sub_ctx)
        });

        let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
        let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
        let padding = p.big_op_spacing[4] * size;

        if let Some(ref sb) = sup_box {
            max_w = max_w.max(sb.width);
            total_h += sb.height + sb.depth + sup_gap + padding;
        }
        if let Some(ref sb) = sub_box {
            max_w = max_w.max(sb.width);
            total_d += sb.height + sb.depth + sub_gap + padding;
        }

        // Center op
        let op_dx = (max_w - op_box.width) / 2.0;

        if let Some(sb) = sup_box {
            let sup_dx = (max_w - sb.width) / 2.0;
            let sup_dy = op_box.height + sup_gap + sb.depth;
            children.push(PositionedBox::new(sup_dx, sup_dy, sb));
        }
        let op_depth = op_box.depth;
        children.push(PositionedBox::new(op_dx, 0.0, op_box));
        if let Some(sb) = sub_box {
            let sub_dx = (max_w - sb.width) / 2.0;
            let sub_dy = -(op_depth + sub_gap + sb.height);
            children.push(PositionedBox::new(sub_dx, sub_dy, sb));
        }

        // Body follows
        let nary_width = max_w;
        let thin_space = 3.0 / 18.0 * size;
        children.push(PositionedBox::new(nary_width + thin_space, 0.0, body_box));
        LayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h,
            depth: total_d,
            kind: BoxKind::HBox { children },
        }
    } else {
        // As sub/superscripts — use the already-scaled op_box
        let thin_space = 3.0 / 18.0 * size;

        let build_hbox = |result: LayoutBox, body_box: LayoutBox| -> LayoutBox {
            let rw = result.width;
            let rh = result.height;
            let rd = result.depth;
            let bw = body_box.width;
            let bh = body_box.height;
            let bd = body_box.depth;
            let children = vec![
                PositionedBox::at_origin(result),
                PositionedBox::new(rw + thin_space, 0.0, body_box),
            ];
            LayoutBox {
                width: rw + thin_space + bw,
                height: rh.max(bh),
                depth: rd.max(bd),
                kind: BoxKind::HBox { children },
            }
        };

        if let (Some(sub_n), Some(sup_n)) = (sub, sup) {
            let result = layout_subsup_from_box_heap(op_box, sub_n, sup_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sub_n) = sub {
            let result = layout_subscript_from_box_heap(op_box, sub_n, ctx);
            return build_hbox(result, body_box);
        } else if let Some(sup_n) = sup {
            let result = layout_superscript_from_box_heap(op_box, sup_n, ctx);
            return build_hbox(result, body_box);
        }

        // Just op + body
        let op_w = op_box.width;
        let op_h = op_box.height;
        let op_d = op_box.depth;
        let children = vec![
            PositionedBox::at_origin(op_box),
            PositionedBox::new(op_w + thin_space, 0.0, body_box),
        ];
        LayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: BoxKind::HBox { children },
        }
    }
}

/// Helper: superscript layout where the base is an already-laid-out LayoutBox (heap path).
fn layout_superscript_from_box_heap(
    base_box: LayoutBox,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node(sup, &sup_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let ic = italic_correction(&base_box);

    let total_width = base_box.width + ic + sup_box.width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: subscript layout where the base is an already-laid-out LayoutBox (heap path).
fn layout_subscript_from_box_heap(
    base_box: LayoutBox,
    sub: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height / 5.0));

    let ic = italic_correction(&base_box);
    let sub_dx = (base_box.width - ic).max(0.0);

    let total_width = sub_dx + sub_box.width;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: combined sub+superscript layout where the base is an already-laid-out LayoutBox (heap path).
fn layout_subsup_from_box_heap(
    base_box: LayoutBox,
    sub: &MathNode,
    sup: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node(sup, &sup_ctx);
    let sub_box = layout_node(sub, &sub_ctx);

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    let ic = italic_correction(&base_box);
    let sup_dx = base_box.width + ic;
    let sub_dx = (base_box.width - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Extract the symbol string from a MathNode if it is a stretchy overunder decoration.
fn get_stretchy_overunder_sym(node: &MathNode) -> Option<&str> {
    if let MathNode::Symbol(_, s) = node
        && is_stretchy_overunder(s)
    {
        return Some(s);
    }
    None
}

/// Create a stretchy overunder decoration LayoutBox at the given width.
fn make_stretchy_overunder_box(sym: &str, width: f64, size: f64) -> LayoutBox {
    let deco_h = 0.40 * size;
    LayoutBox {
        width,
        height: deco_h,
        depth: 0.0,
        kind: BoxKind::SvgPath(stretchy_overunder_path(sym, width, deco_h)),
    }
}

fn layout_overscript(base: &MathNode, over: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node(over, &over_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = over_box.width.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap = p.big_op_spacing[2] * size;
        let max_w = arrow_box.width.max(over_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let over_dx = (max_w - over_box.width) / 2.0;
        let over_dy = arrow_box.height + gap + over_box.depth;
        let over_h = over_box.height;

        let children = vec![
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(over_dx, over_dy, over_box),
        ];

        return LayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: arrow_h / 2.0,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    // Check if the over decoration is a stretchy symbol that should scale to base width
    let over_box = if let Some(sym) = get_stretchy_overunder_sym(over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node(over, &over_ctx)
    };

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_box.width.max(over_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let over_dy = base_box.height + gap + over_box.depth;

    // Capture dimensions before moving into children
    let over_h = over_box.height;
    let base_d = base_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(over_dx, over_dy, over_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: base_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underscript(base: &MathNode, under: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node(under, &under_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = under_box.width.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap = p.big_op_spacing[4] * size;
        let max_w = arrow_box.width.max(under_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let under_dx = (max_w - under_box.width) / 2.0;
        let under_dy = arrow_box.depth + gap + under_box.height;
        let under_d = under_box.depth;

        let children = vec![
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(under_dx, -under_dy, under_box),
        ];

        return LayoutBox {
            width: max_w,
            height: arrow_h / 2.0,
            depth: under_dy + under_d,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    // Check if the under decoration is a stretchy symbol that should scale to base width
    let under_box = if let Some(sym) = get_stretchy_overunder_sym(under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node(under, &under_ctx)
    };

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(under_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;
    let under_dy = base_box.depth + gap + under_box.height;

    // Capture dimensions before moving into children
    let base_h = base_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: base_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underover(
    base: &MathNode,
    under: &MathNode,
    over: &MathNode,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_heap(base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node(over, &over_ctx);
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node(under, &under_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let content_w = over_box.width.max(under_box.width);
        let arrow_w = content_w.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap_above = p.big_op_spacing[2] * size;
        let gap_below = p.big_op_spacing[4] * size;
        let max_w = arrow_box.width.max(over_box.width).max(under_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let over_dx = (max_w - over_box.width) / 2.0;
        let under_dx = (max_w - under_box.width) / 2.0;
        let over_dy = arrow_box.height + gap_above + over_box.depth;
        let under_dy = arrow_box.depth + gap_below + under_box.height;
        let over_h = over_box.height;
        let under_d = under_box.depth;

        let children = vec![
            PositionedBox::new(over_dx, over_dy, over_box),
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(under_dx, -under_dy, under_box),
        ];

        return LayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: under_dy + under_d,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node(base, ctx);
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym(over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node(over, &over_ctx)
    };

    let under_box = if let Some(sym) = get_stretchy_overunder_sym(under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node(under, &under_ctx)
    };

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(over_box.width).max(under_box.width);

    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;

    let over_dy = base_box.height + gap_above + over_box.depth;
    let under_dy = base_box.depth + gap_below + under_box.height;

    // Capture dimensions before moving into children
    let over_h = over_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(over_dx, over_dy, over_box),
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_accent(
    base: &MathNode,
    accent: char,
    position: AccentPosition,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let base_width = base_box.width;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_box = if is_stretchy_accent(accent) && base_width > 0.0 {
        let size = ctx.size();
        match accent {
            // Bar: horizontal rule across base width
            '\u{0304}' => {
                let thickness = ctx.params.rule_thickness * size;
                LayoutBox::rule(base_width, thickness)
            },
            // Dot: small filled square centered above base
            '\u{0307}' => {
                let dot_r = 0.04 * size; // dot radius in em
                let dot_size = dot_r * 2.0;
                let dot = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                let dx = (base_width - dot_size) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, dot)],
                    },
                }
            },
            // Double dot: two small filled squares
            '\u{0308}' => {
                let dot_r = 0.04 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.12 * size; // gap between dots
                let total_dots_w = dot_size * 2.0 + gap;
                let dx_left = (base_width - total_dots_w) / 2.0;
                let dx_right = dx_left + dot_size + gap;
                let dot1 = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                let dot2 = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![
                            PositionedBox::new(dx_left, 0.0, dot1),
                            PositionedBox::new(dx_right, 0.0, dot2),
                        ],
                    },
                }
            },
            // Other stretchy accents: SVG path
            _ => {
                let accent_h = 0.12 * size;
                LayoutBox {
                    width: base_width,
                    height: accent_h,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(stretchy_accent_path(accent, base_width, accent_h)),
                }
            },
        }
    } else {
        LayoutBox::glyph(accent, ctx.size(), false)
    };

    let accent_dx = (base_width - accent_box.width) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_box.height + 0.02 * ctx.size(); // small gap
            (dy, dy + accent_box.height, base_box.depth)
        },
        AccentPosition::Below => {
            let dy = -(base_box.depth + accent_box.height + 0.02 * ctx.size());
            (
                dy,
                base_box.height,
                base_box.depth + accent_box.height + 0.02 * ctx.size(),
            )
        },
    };

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(accent_dx, accent_dy, accent_box),
    ];

    LayoutBox {
        width: base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_array(
    columns: &[ColumnAlign],
    rows: &[Vec<Vec<MathNode>>],
    arraystretch_opt: Option<f64>,
    add_jot: bool,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let col_gap = 1.0 * size; // column separation

    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_opt.unwrap_or(1.0);
    let baselineskip = 1.2 * size;
    let arrayskip = arraystretch * baselineskip;
    let arstrut_height = 0.7 * arrayskip;
    let arstrut_depth = 0.3 * arrayskip;
    let jot = 0.3 * size;

    // Layout all cells
    let mut cell_layouts: Vec<Vec<LayoutBox>> = Vec::new();
    for row in rows {
        let mut row_layouts = Vec::new();
        for cell in row {
            row_layouts.push(layout_row(cell, ctx));
        }
        cell_layouts.push(row_layouts);
    }

    let n_cols = columns
        .len()
        .max(cell_layouts.iter().map(|r| r.len()).max().unwrap_or(0));
    let n_rows = cell_layouts.len();

    // Compute column widths and row heights/depths with strut minimums
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for (i, row) in cell_layouts.iter().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            col_widths[j] = col_widths[j].max(cell.width);
            row_heights[i] = row_heights[i].max(cell.height);
            row_depths[i] = row_depths[i].max(cell.depth);
        }
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Position cells — consume cell_layouts by ownership to avoid cloning
    let mut children = Vec::new();
    let mut cursor_y = 0.0;

    // Start from top
    for (i, row) in cell_layouts.into_iter().enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut row_iter = row.into_iter();

        for (j, &col_w) in col_widths.iter().enumerate() {
            if let Some(cell) = row_iter.next() {
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            }
            cursor_x += col_w;
            if j < n_cols - 1 {
                cursor_x += col_gap;
            }
        }

        cursor_y += row_depths[i];
    }

    let total_width: f64 =
        col_widths.iter().sum::<f64>() + col_gap * (n_cols.saturating_sub(1) as f64);
    let total_extent = cursor_y;
    // Center vertically around the axis
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    LayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: BoxKind::VBox { children },
    }
}

fn layout_prescripts(
    base: &MathNode,
    pre_sub: &Option<Box<MathNode>>,
    pre_sup: &Option<Box<MathNode>>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let p = &ctx.params;

    let mut children = Vec::new();
    let mut pre_width: f64 = 0.0;
    let mut total_h = base_box.height;
    let mut total_d = base_box.depth;

    // Bug 1 fix: use child style params for prescripts too
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    if let Some(sup) = pre_sup {
        let sup_style = ctx.style.sup();
        let sup_params = TexParams::for_size_class(sup_style.size_class());
        let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
        let sup_ctx = ctx.with_style(sup_style);
        let sup_box = layout_node(sup, &sup_ctx);
        let mut shift_up = p.sup2;
        if !is_char_box {
            shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
        }
        pre_width = pre_width.max(sup_box.width);
        total_h = total_h.max(shift_up + sup_box.height);
        children.push(PositionedBox::new(0.0, shift_up, sup_box));
    }
    if let Some(sub) = pre_sub {
        let sub_style = ctx.style.sub();
        let sub_params = TexParams::for_size_class(sub_style.size_class());
        let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();
        let sub_ctx = ctx.with_style(sub_style);
        let sub_box = layout_node(sub, &sub_ctx);
        let mut shift_down = p.sub1;
        if !is_char_box {
            shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
        }
        pre_width = pre_width.max(sub_box.width);
        total_d = total_d.max(shift_down + sub_box.depth);
        children.push(PositionedBox::new(0.0, -shift_down, sub_box));
    }

    // Capture width before moving base_box into children vec
    let base_width = base_box.width;
    children.push(PositionedBox::new(pre_width, 0.0, base_box));

    LayoutBox {
        width: pre_width + base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_phantom(content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    LayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: BoxKind::Phantom,
    }
}

fn layout_boxed(content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner = layout_node(content, ctx);

    LayoutBox {
        width: inner.width + 2.0 * padding,
        height: inner.height + padding,
        depth: inner.depth + padding,
        kind: BoxKind::Boxed {
            child: Box::new(PositionedBox::new(padding, 0.0, inner)),
            padding,
        },
    }
}

fn layout_cancel(stroke: StrokeKind, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let w = inner.width;
    let h = inner.height + inner.depth;

    let path = match stroke {
        StrokeKind::Forward => format!("M0,{h} L{w},0"),
        StrokeKind::Backward => format!("M0,0 L{w},{h}"),
        StrokeKind::Cross => format!("M0,{h} L{w},0 M0,0 L{w},{h}"),
        StrokeKind::Horizontal => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        StrokeKind::Vertical => {
            let mid = w / 2.0;
            format!("M{mid},0 L{mid},{h}")
        },
    };

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_height = inner.height;
    let inner_depth = inner.depth;

    let children = vec![
        PositionedBox::at_origin(inner),
        PositionedBox::at_origin(LayoutBox {
            width: w,
            height: inner_height,
            depth: inner_depth,
            kind: BoxKind::SvgPath(path),
        }),
    ];

    LayoutBox {
        width: inner_width,
        height: inner_height,
        depth: inner_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_scaled(factor: Rational, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: inner.kind,
    }
}

fn layout_styled(style: TextStyle, children: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    let styled_children: Vec<MathNode> = children
        .iter()
        .map(|node| match node {
            MathNode::Identifier(s) => {
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                MathNode::Identifier(mapped)
            },
            MathNode::Number(s) => {
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                MathNode::Number(mapped)
            },
            other => other.clone(),
        })
        .collect();
    layout_row(&styled_children, ctx)
}

// ==========================================================================
// Compact arena-based layout functions (_c suffix)
// ==========================================================================

/// Entry point: layout an entire compact math expression.
pub fn layout_expr_c(expr: &CompactExpr, style: MathStyle, options: &RenderOptions) -> LayoutBox {
    let ctx = LayoutCtx {
        style,
        params: TexParams::for_size_class(style.size_class()),
        options,
        depth: 0,
        size_scale: 1.0,
    };
    layout_row_c(&expr.arena, expr.roots, &ctx)
}

fn layout_row_c(arena: &AstArena, span: NodeSpan, ctx: &LayoutCtx) -> LayoutBox {
    let ids = arena.children(span);
    if ids.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;
    let mut current_ctx = ctx.clone();

    for &id in ids {
        // Handle style switches — update context for subsequent children
        if let CompactNode::StyleSwitch(s) = *arena.node(id) {
            current_ctx = match s {
                0 => current_ctx.with_style(MathStyle::Display),
                1 => current_ctx.with_style(MathStyle::Text),
                2 => current_ctx.with_style(MathStyle::Script),
                _ => current_ctx.with_style(MathStyle::ScriptScript),
            };
            continue;
        }

        // Handle size scale switches — update scale for subsequent children
        if let CompactNode::SizeScale(idx) = *arena.node(id) {
            let scale = SIZE_SCALE_TABLE.get(idx as usize).copied().unwrap_or(1.0);
            current_ctx.size_scale = scale;
            continue;
        }

        // Inter-atom spacing with TeXBook Ch.26 Bin reclassification
        let raw_atom = spacing::classify_node_c(arena, id);
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.unwrap(); // safe: we passed Some
        if let Some(prev) = adj_prev {
            let space = spacing::inter_atom_space(prev, cur_atom, current_ctx.style);
            if space > 0.0 {
                let kern_width = space * current_ctx.size();
                cursor_x += kern_width;
            }
        }

        let layout = layout_node_c(arena, id, &current_ctx);
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);

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

fn layout_node_c(arena: &AstArena, id: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    if ctx.depth >= MAX_LAYOUT_DEPTH {
        return LayoutBox::kern(0.0);
    }
    let ctx = &ctx.deeper();
    match *arena.node(id) {
        CompactNode::Number(sid) => layout_text_run(arena.str_val(sid), ctx, false),
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            if s.chars().count() == 1 {
                let ch = s.chars().next().expect("non-empty checked");
                LayoutBox::glyph(ch, ctx.size(), is_math_italic_char(ch))
            } else {
                layout_text_run(s, ctx, false)
            }
        },
        CompactNode::Operator { name, .. } => layout_text_run(arena.str_val(name), ctx, false),
        CompactNode::Symbol(kind, sid) => {
            let s = arena.str_val(sid);
            if let Some(ch) = s.chars().next() {
                if kind == SymbolKind::LargeOp && ctx.style.is_display() {
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
        CompactNode::Text(_, sid) => layout_text_run(arena.str_val(sid), ctx, false),
        CompactNode::Space(r) => LayoutBox::kern(r.as_f64() * ctx.size()),
        CompactNode::Row(span) => layout_row_c(arena, span, ctx),
        CompactNode::Fenced {
            open,
            close,
            content,
        } => layout_fenced_c(arena, open, close, content, ctx),
        CompactNode::Subscript { base, sub } => layout_subscript_c(arena, base, sub, ctx),
        CompactNode::Superscript { base, sup } => layout_superscript_c(arena, base, sup, ctx),
        CompactNode::SubSup { base, sub, sup } => layout_subsup_c(arena, base, sub, sup, ctx),
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => layout_prescripts_c(arena, base, pre_sub, pre_sup, ctx),
        CompactNode::Overscript { base, over, .. } => layout_overscript_c(arena, base, over, ctx),
        CompactNode::Underscript { base, under, .. } => {
            layout_underscript_c(arena, base, under, ctx)
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => layout_underover_c(arena, base, under, over, ctx),
        CompactNode::Fraction { kind, num, den } => layout_fraction_c(arena, kind, num, den, ctx),
        CompactNode::Sqrt(inner) => layout_sqrt_c(arena, inner, None, ctx),
        CompactNode::Root { index, radicand } => layout_sqrt_c(arena, radicand, Some(index), ctx),
        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => layout_nary_c(arena, op, sub, sup, body, limits, ctx),
        CompactNode::Accent {
            base,
            accent,
            position,
        } => layout_accent_c(arena, base, accent, position, ctx),
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
        } => layout_array_c(arena, col_aligns, cells, num_cols, arraystretch_pct, add_jot, ctx),
        CompactNode::Phantom(inner) => layout_phantom_c(arena, inner, ctx),
        CompactNode::Boxed(inner) => layout_boxed_c(arena, inner, ctx),
        CompactNode::Cancel { stroke, content } => layout_cancel_c(arena, stroke, content, ctx),
        CompactNode::Scaled { factor, content } => layout_scaled_c(arena, factor, content, ctx),
        CompactNode::Styled(style, span) => layout_styled_c(arena, style, span, ctx),
        CompactNode::Color { color, content } => layout_color_c(arena, color, content, ctx),
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => LayoutBox::kern(0.0),
        CompactNode::StyleSwitch(_) => LayoutBox::kern(0.0),
        CompactNode::SizeScale(_) => LayoutBox::kern(0.0),
        CompactNode::Rule { width, height } => {
            let w = width.as_f64() * ctx.size();
            let h = height.as_f64() * ctx.size();
            LayoutBox {
                width: w,
                height: h,
                depth: 0.0,
                kind: BoxKind::Rule { thickness: h },
            }
        },
    }
}

/// Compact-path: layout `\overbrace{content}^{annotation}` with annotation above brace.
fn layout_overbrace_annotated_c(
    arena: &AstArena,
    inner_base: NodeId,
    annotation: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let p = &ctx.params;

    let content_box = layout_node_c(arena, inner_base, ctx);
    let content_w = content_box.width;

    let sym = "\u{23DE}";
    let brace_box = make_stretchy_overunder_box(sym, content_w.max(0.5 * size), size);
    let brace_h = brace_box.height;

    let ann_ctx = ctx.with_style(ctx.style.sup());
    let ann_box = layout_node_c(arena, annotation, &ann_ctx);

    let gap1 = p.big_op_spacing[2] * size;
    let gap2 = p.big_op_spacing[2] * size;

    let max_w = content_w.max(brace_box.width).max(ann_box.width);
    let content_dx = (max_w - content_box.width) / 2.0;
    let brace_dx = (max_w - brace_box.width) / 2.0;
    let ann_dx = (max_w - ann_box.width) / 2.0;

    let brace_dy = content_box.height + gap1 + brace_box.depth;
    let ann_dy = brace_dy + brace_h + gap2 + ann_box.depth;

    let total_height = ann_dy + ann_box.height;
    let total_depth = content_box.depth;

    let children = vec![
        PositionedBox::new(content_dx, 0.0, content_box),
        PositionedBox::new(brace_dx, brace_dy, brace_box),
        PositionedBox::new(ann_dx, ann_dy, ann_box),
    ];

    LayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

/// Compact-path: layout `\underbrace{content}_{annotation}` with annotation below brace.
fn layout_underbrace_annotated_c(
    arena: &AstArena,
    inner_base: NodeId,
    annotation: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let p = &ctx.params;

    let content_box = layout_node_c(arena, inner_base, ctx);
    let content_w = content_box.width;

    let sym = "\u{23DF}";
    let brace_box = make_stretchy_overunder_box(sym, content_w.max(0.5 * size), size);
    let brace_h = brace_box.height;

    let ann_ctx = ctx.with_style(ctx.style.sub());
    let ann_box = layout_node_c(arena, annotation, &ann_ctx);

    let gap1 = p.big_op_spacing[4] * size;
    let gap2 = p.big_op_spacing[4] * size;

    let max_w = content_w.max(brace_box.width).max(ann_box.width);
    let content_dx = (max_w - content_box.width) / 2.0;
    let brace_dx = (max_w - brace_box.width) / 2.0;
    let ann_dx = (max_w - ann_box.width) / 2.0;

    let brace_dy = content_box.depth + gap1 + brace_h;
    let ann_dy = brace_dy + brace_box.depth + gap2 + ann_box.height;

    let total_height = content_box.height;
    let total_depth = ann_dy + ann_box.depth;

    let children = vec![
        PositionedBox::new(content_dx, 0.0, content_box),
        PositionedBox::new(brace_dx, -brace_dy, brace_box),
        PositionedBox::new(ann_dx, -ann_dy, ann_box),
    ];

    LayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

fn layout_superscript_c(arena: &AstArena, base: NodeId, sup: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, None, Some(sup), ctx);
    }

    // Overbrace annotation: Superscript { base: Overscript { over: "⏞" }, sup: annotation }
    if let CompactNode::Overscript { base: inner_base, over, .. } = *arena.node(base)
        && node_is_overbrace_c(arena, over)
    {
        return layout_overbrace_annotated_c(arena, inner_base, sup, ctx);
    }

    let base_box = layout_node_c(arena, base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let ic = italic_correction(&base_box);

    let total_width = base_box.width + ic + sup_box.width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_subscript_c(arena: &AstArena, base: NodeId, sub: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, Some(sub), None, ctx);
    }

    // Underbrace annotation: Subscript { base: Underscript { under: "⏟" }, sub: annotation }
    if let CompactNode::Underscript { base: inner_base, under, .. } = *arena.node(base)
        && node_is_underbrace_c(arena, under)
    {
        return layout_underbrace_annotated_c(arena, inner_base, sub, ctx);
    }

    let base_box = layout_node_c(arena, base, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height / 5.0));

    let ic = italic_correction(&base_box);
    let sub_dx = (base_box.width - ic).max(0.0);

    let total_width = sub_dx + sub_box.width;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_subsup_c(
    arena: &AstArena,
    base: NodeId,
    sub: NodeId,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Large operator in display mode: place limits above/below
    if is_large_op_c(arena, base) && ctx.style.is_display() {
        return layout_big_op_limits_c(arena, base, Some(sub), Some(sup), ctx);
    }
    let base_box = layout_node_c(arena, base, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    let ic = italic_correction(&base_box);
    let sup_dx = base_box.width + ic;
    let sub_dx = (base_box.width - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Layout a big operator (LargeOp symbol) with limits above/below (compact path).
fn layout_big_op_limits_c(
    arena: &AstArena,
    base: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char_c(arena, base).unwrap_or('\u{2211}');
    let op_size = size * metrics::display_op_scale(op_ch);
    let m = metrics::glyph_metrics_display(op_ch);
    let op_box = LayoutBox::glyph_with_metrics(op_ch, m, op_size, false);

    let mut children = Vec::new();
    let mut total_h = op_box.height;
    let mut total_d = op_box.depth;
    let mut max_w = op_box.width;

    let sup_box = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, s, &sup_ctx)
    });
    let sub_box = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, s, &sub_ctx)
    });

    let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
    let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
    let padding = p.big_op_spacing[4] * size;

    if let Some(ref sb) = sup_box {
        max_w = max_w.max(sb.width);
        total_h += sb.height + sb.depth + sup_gap + padding;
    }
    if let Some(ref sb) = sub_box {
        max_w = max_w.max(sb.width);
        total_d += sb.height + sb.depth + sub_gap + padding;
    }

    let op_dx = (max_w - op_box.width) / 2.0;

    if let Some(sb) = sup_box {
        let sup_dx = (max_w - sb.width) / 2.0;
        let sup_dy = op_box.height + sup_gap + sb.depth;
        children.push(PositionedBox::new(sup_dx, sup_dy, sb));
    }
    let op_depth = op_box.depth;
    children.push(PositionedBox::new(op_dx, 0.0, op_box));
    if let Some(sb) = sub_box {
        let sub_dx = (max_w - sb.width) / 2.0;
        let sub_dy = -(op_depth + sub_gap + sb.height);
        children.push(PositionedBox::new(sub_dx, sub_dy, sb));
    }

    LayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_fraction_c(
    arena: &AstArena,
    kind: FractionKind,
    num: NodeId,
    den: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Override context style for \dfrac (Display) and \tfrac (Inline)
    let override_ctx = match kind {
        FractionKind::Display => Some(ctx.with_style(MathStyle::Display)),
        FractionKind::Inline => Some(ctx.with_style(MathStyle::Text)),
        _ => None,
    };
    let ctx = override_ctx.as_ref().unwrap_or(ctx);

    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
    let num_box = layout_node_c(arena, num, &num_ctx);
    let den_box = layout_node_c(arena, den, &den_ctx);

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
    } else if bar_thickness == 0.0 {
        (p.num3 * size, p.denom2 * size)
    } else {
        (p.num2 * size, p.denom2 * size)
    };

    let axis = p.axis_height * size;
    let mut num_up = num_shift;
    let mut den_down = den_shift;

    if bar_thickness > 0.0 {
        let min_clearance = if ctx.style.is_display() {
            3.0 * bar_thickness
        } else {
            bar_thickness
        };

        let num_clearance = (num_up - num_box.depth) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_box.height - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
        let min_gap = if ctx.style.is_display() {
            7.0 * p.rule_thickness * size
        } else {
            3.0 * p.rule_thickness * size
        };
        let gap = (num_up - num_box.depth) - (den_box.height - den_down);
        if gap < min_gap {
            let adjust = (min_gap - gap) / 2.0;
            num_up += adjust;
            den_down += adjust;
        }
    }

    let frac_width = num_box.width.max(den_box.width);
    // Add null-delimiter-space side padding to prevent content from touching adjacent glyphs
    let side_padding = 0.12 * size;
    let padded_frac_width = frac_width + 2.0 * side_padding;
    let total_height = num_up + num_box.height;
    let total_depth = den_down + den_box.depth;

    let num_dx = side_padding + (frac_width - num_box.width) / 2.0;
    let den_dx = side_padding + (frac_width - den_box.width) / 2.0;

    let mut children = vec![
        PositionedBox::new(num_dx, num_up, num_box),
        PositionedBox::new(den_dx, -den_down, den_box),
    ];

    if bar_thickness > 0.0 {
        let bar = LayoutBox::rule(padded_frac_width, bar_thickness);
        children.push(PositionedBox::new(0.0, axis, bar));
    }

    LayoutBox {
        width: padded_frac_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::VBox { children },
    }
}

fn layout_sqrt_c(
    arena: &AstArena,
    radicand: NodeId,
    index: Option<NodeId>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();
    let cramped_ctx = ctx.with_style(ctx.style.cramped());
    let inner = layout_node_c(arena, radicand, &cramped_ctx);

    let rule_thickness = p.rule_thickness * size;

    // KaTeX-compatible clearance: theta + phi/4
    // where phi = xHeight in display mode, theta otherwise
    let phi = if ctx.style.is_display() {
        p.x_height * size // ~0.431em — the x-height
    } else {
        rule_thickness // 0.04em — same as theta
    };
    let extra_clearance = rule_thickness + phi / 4.0;

    let inner_total = inner.height + inner.depth + extra_clearance + rule_thickness;

    // Minimum surd height of 1.0em (KaTeX's "small" surd minimum)
    let min_surd_height = 1.0 * size;
    let surd_height = inner_total.max(min_surd_height);

    // If surd is taller than needed, redistribute extra space evenly
    let extra_clearance = if surd_height > inner_total {
        extra_clearance + (surd_height - inner_total) / 2.0
    } else {
        extra_clearance
    };
    // Recompute with potentially larger clearance
    let _inner_total = surd_height;

    let radical_width = 0.833 * size;
    let radical = LayoutBox {
        width: radical_width,
        height: surd_height,
        depth: 0.0,
        kind: BoxKind::SvgPath(radical_path(radical_width, surd_height)),
    };

    let vinculum = LayoutBox::rule(inner.width, rule_thickness);

    let shift_up = inner.height + extra_clearance + rule_thickness;

    let inner_width = inner.width;
    let inner_depth = inner.depth;
    let mut children = vec![
        PositionedBox::new(0.0, 0.0, radical),
        PositionedBox::new(radical_width, shift_up, vinculum),
        PositionedBox::new(radical_width, 0.0, inner),
    ];

    let mut total_width = radical_width + inner_width;
    let total_height = shift_up + rule_thickness;
    let total_depth = inner_depth;

    if let Some(idx_id) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box = layout_node_c(arena, idx_id, &idx_ctx);
        let idx_shift = total_height * 0.6;
        let idx_width = idx_box.width;
        children.push(PositionedBox::new(0.0, idx_shift, idx_box));
        let idx_overflow = idx_width.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
            for child in &mut children[..3] {
                child.dx += extra;
            }
            total_width += extra;
        }
    }

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_fenced_c(
    arena: &AstArena,
    open: StrId,
    close: StrId,
    content: NodeSpan,
    ctx: &LayoutCtx,
) -> LayoutBox {
    // Layout content, skipping Middle separator nodes
    let mut inner_children = Vec::new();
    for &child_id in arena.children(content) {
        match *arena.node(child_id) {
            CompactNode::Middle(_) => {},
            _ => {
                inner_children.push(layout_node_c(arena, child_id, ctx));
            },
        }
    }

    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for child in &inner_children {
        max_h = max_h.max(child.height);
        max_d = max_d.max(child.depth);
    }

    // Axis-centered target height (KaTeX-compatible): delimiters extend
    // symmetrically around the math axis, ensuring proper visual balance.
    let axis = ctx.params.axis_height * ctx.size();
    let max_dist_from_axis = (max_h - axis).max(max_d + axis);
    let target_h = (2.0 * max_dist_from_axis).max(max_h + max_d);
    let delim_h = (axis + max_dist_from_axis).max(max_h);
    let delim_d = (max_dist_from_axis - axis).max(max_d);

    // Compute delimiter width using path-based sizing for tall content
    use super::delimiter_paths;

    let mut children = Vec::new();
    let mut cursor_x = 0.0;

    // Open delimiter
    let open_str = arena.str_val(open);
    if !open_str.is_empty()
        && open_str != "."
        && let Some(ch) = open_str.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim = LayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    for child in inner_children {
        let w = child.width;
        children.push(PositionedBox::new(cursor_x, 0.0, child));
        cursor_x += w;
    }

    // Close delimiter
    let close_str = arena.str_val(close);
    if !close_str.is_empty()
        && close_str != "."
        && let Some(ch) = close_str.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim = LayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: BoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        };
        let delim_w = delim.width;
        children.push(PositionedBox::new(cursor_x, 0.0, delim));
        cursor_x += delim_w;
    }

    LayoutBox {
        width: cursor_x,
        height: max_h,
        depth: max_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_nary_c(
    arena: &AstArena,
    op: char,
    sub: NodeId,
    sup: NodeId,
    body: NodeId,
    limits: LimitStyle,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    let (op_size, m) = if ctx.style.is_display() {
        (size * metrics::display_op_scale(op), metrics::glyph_metrics_display(op))
    } else {
        (size, metrics::glyph_metrics(op))
    };
    let op_box = LayoutBox::glyph_with_metrics(op, m, op_size, false);

    let body_box = layout_node_c(arena, body, ctx);
    let body_w = body_box.width;
    let body_h = body_box.height;
    let body_d = body_box.depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        // Integrals default to side-limits (nolimits); sums/products to above/below in display.
        LimitStyle::Default => ctx.style.is_display() && !is_nolimits_op(op),
    };

    if use_limits {
        let mut children = Vec::new();
        let mut total_h = op_box.height;
        let mut total_d = op_box.depth;
        let mut max_w = op_box.width;

        let sup_box = if sup.is_some() {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            Some(layout_node_c(arena, sup, &sup_ctx))
        } else {
            None
        };
        let sub_box = if sub.is_some() {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            Some(layout_node_c(arena, sub, &sub_ctx))
        } else {
            None
        };

        let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
        let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
        let padding = p.big_op_spacing[4] * size;

        if let Some(ref sb) = sup_box {
            max_w = max_w.max(sb.width);
            total_h += sb.height + sb.depth + sup_gap + padding;
        }
        if let Some(ref sb) = sub_box {
            max_w = max_w.max(sb.width);
            total_d += sb.height + sb.depth + sub_gap + padding;
        }

        let op_dx = (max_w - op_box.width) / 2.0;

        if let Some(sb) = sup_box {
            let sup_dx = (max_w - sb.width) / 2.0;
            let sup_dy = op_box.height + sup_gap + sb.depth;
            children.push(PositionedBox::new(sup_dx, sup_dy, sb));
        }
        let op_depth = op_box.depth;
        children.push(PositionedBox::new(op_dx, 0.0, op_box));
        if let Some(sb) = sub_box {
            let sub_dx = (max_w - sb.width) / 2.0;
            let sub_dy = -(op_depth + sub_gap + sb.height);
            children.push(PositionedBox::new(sub_dx, sub_dy, sb));
        }

        let nary_width = max_w;
        let thin_space = 3.0 / 18.0 * size;
        children.push(PositionedBox::new(nary_width + thin_space, 0.0, body_box));
        LayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h,
            depth: total_d,
            kind: BoxKind::HBox { children },
        }
    } else {
        // As sub/superscripts — use the op glyph directly
        let thin_space = 3.0 / 18.0 * size;

        let build_hbox = |result: LayoutBox, body_box: LayoutBox| -> LayoutBox {
            let rw = result.width;
            let rh = result.height;
            let rd = result.depth;
            let bw = body_box.width;
            let bh = body_box.height;
            let bd = body_box.depth;
            let children = vec![
                PositionedBox::at_origin(result),
                PositionedBox::new(rw + thin_space, 0.0, body_box),
            ];
            LayoutBox {
                width: rw + thin_space + bw,
                height: rh.max(bh),
                depth: rd.max(bd),
                kind: BoxKind::HBox { children },
            }
        };

        // Use the already-scaled op_box for the operator glyph
        let op_glyph_box = op_box;

        if sub.is_some() && sup.is_some() {
            let result = layout_subsup_from_box(op_glyph_box, arena, sub, sup, ctx);
            return build_hbox(result, body_box);
        } else if sub.is_some() {
            return build_hbox(
                layout_subscript_from_box(op_glyph_box, arena, sub, ctx),
                body_box,
            );
        } else if sup.is_some() {
            return build_hbox(
                layout_superscript_from_box(op_glyph_box, arena, sup, ctx),
                body_box,
            );
        }

        // Just op + body
        let op_w = op_glyph_box.width;
        let op_h = op_glyph_box.height;
        let op_d = op_glyph_box.depth;
        let children = vec![
            PositionedBox::at_origin(op_glyph_box),
            PositionedBox::new(op_w + thin_space, 0.0, body_box),
        ];
        LayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: BoxKind::HBox { children },
        }
    }
}

/// Helper: superscript layout where the base is an already-laid-out LayoutBox.
fn layout_superscript_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };

    // Bug 2 fix: only apply height-based sup_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let ic = italic_correction(&base_box);

    let total_width = base_box.width + ic + sup_box.width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = base_box.depth;

    let base_width = base_box.width;
    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(base_width + ic, shift_up, sup_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: subscript layout where the base is an already-laid-out LayoutBox.
fn layout_subscript_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sub: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply depth-based sub_drop term when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });
    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_box.height - (4.0 * p.x_height / 5.0));

    let ic = italic_correction(&base_box);
    let sub_dx = (base_box.width - ic).max(0.0);

    let total_width = sub_dx + sub_box.width;
    let total_height = base_box.height;
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Helper: subsup layout where the base is an already-laid-out LayoutBox.
fn layout_subsup_from_box(
    base_box: LayoutBox,
    arena: &AstArena,
    sub: NodeId,
    sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_box = layout_node_c(arena, sup, &sup_ctx);
    let sub_box = layout_node_c(arena, sub, &sub_ctx);

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    // Bug 2 fix: only apply height/depth-based drop terms when base is not a single character
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_box.depth);

    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_box.depth) - (sub_box.height - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_box.depth) + sub_box.height;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_box.depth);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    let ic = italic_correction(&base_box);
    let sup_dx = base_box.width + ic;
    let sub_dx = (base_box.width - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_box.width + sup_box.width)
        .max(sub_dx - base_box.width + sub_box.width)
        .max(0.0);
    let total_width = base_box.width + script_width;
    let total_height = (shift_up + sup_box.height).max(base_box.height);
    let total_depth = (shift_down + sub_box.depth).max(base_box.depth);

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(sup_dx, shift_up, sup_box),
        PositionedBox::new(sub_dx, -shift_down, sub_box),
    ];

    LayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: BoxKind::HBox { children },
    }
}

/// Check if a CompactNode (referenced by NodeId) is a stretchy overunder decoration symbol.
/// Returns the symbol string if so.
fn get_stretchy_overunder_sym_c(arena: &AstArena, id: NodeId) -> Option<&str> {
    if let CompactNode::Symbol(_, sid) = *arena.node(id) {
        let s = arena.str_val(sid);
        if is_stretchy_overunder(s) {
            return Some(s);
        }
    }
    None
}

fn layout_overscript_c(arena: &AstArena, base: NodeId, over: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(arena, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node_c(arena, over, &over_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = over_box.width.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap = p.big_op_spacing[2] * size;
        let max_w = arrow_box.width.max(over_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let over_dx = (max_w - over_box.width) / 2.0;
        let over_dy = arrow_box.height + gap + over_box.depth;
        let over_h = over_box.height;

        let children = vec![
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(over_dx, over_dy, over_box),
        ];

        return LayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: arrow_h / 2.0,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, over, &over_ctx)
    };

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_box.width.max(over_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let over_dy = base_box.height + gap + over_box.depth;

    let over_h = over_box.height;
    let base_d = base_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(over_dx, over_dy, over_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: base_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underscript_c(
    arena: &AstArena,
    base: NodeId,
    under: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(arena, base) {
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node_c(arena, under, &under_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = under_box.width.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap = p.big_op_spacing[4] * size;
        let max_w = arrow_box.width.max(under_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let under_dx = (max_w - under_box.width) / 2.0;
        let under_dy = arrow_box.depth + gap + under_box.height;
        let under_d = under_box.depth;

        let children = vec![
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(under_dx, -under_dy, under_box),
        ];

        return LayoutBox {
            width: max_w,
            height: arrow_h / 2.0,
            depth: under_dy + under_d,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let under_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, under, &under_ctx)
    };

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(under_box.width);
    let base_dx = (max_w - base_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;
    let under_dy = base_box.depth + gap + under_box.height;

    let base_h = base_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: base_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_underover_c(
    arena: &AstArena,
    base: NodeId,
    under: NodeId,
    over: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(arena, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_box = layout_node_c(arena, over, &over_ctx);
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_box = layout_node_c(arena, under, &under_ctx);
        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let content_w = over_box.width.max(under_box.width);
        let arrow_w = content_w.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_box = LayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: BoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        };

        let gap_above = p.big_op_spacing[2] * size;
        let gap_below = p.big_op_spacing[4] * size;
        let max_w = arrow_box.width.max(over_box.width).max(under_box.width);
        let base_dx = (max_w - arrow_box.width) / 2.0;
        let over_dx = (max_w - over_box.width) / 2.0;
        let under_dx = (max_w - under_box.width) / 2.0;
        let over_dy = arrow_box.height + gap_above + over_box.depth;
        let under_dy = arrow_box.depth + gap_below + under_box.height;
        let over_h = over_box.height;
        let under_d = under_box.depth;

        let children = vec![
            PositionedBox::new(over_dx, over_dy, over_box),
            PositionedBox::new(base_dx, 0.0, arrow_box),
            PositionedBox::new(under_dx, -under_dy, under_box),
        ];

        return LayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: under_dy + under_d,
            kind: BoxKind::VBox { children },
        };
    }

    let base_box = layout_node_c(arena, base, ctx);
    let base_w = base_box.width;

    let over_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, over) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_c(arena, over, &over_ctx)
    };

    let under_box = if let Some(sym) = get_stretchy_overunder_sym_c(arena, under) {
        make_stretchy_overunder_box(sym, base_w.max(0.5 * size), size)
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_c(arena, under, &under_ctx)
    };

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
    let max_w = base_box.width.max(over_box.width).max(under_box.width);

    let base_dx = (max_w - base_box.width) / 2.0;
    let over_dx = (max_w - over_box.width) / 2.0;
    let under_dx = (max_w - under_box.width) / 2.0;

    let over_dy = base_box.height + gap_above + over_box.depth;
    let under_dy = base_box.depth + gap_below + under_box.height;

    let over_h = over_box.height;
    let under_d = under_box.depth;
    let children = vec![
        PositionedBox::new(over_dx, over_dy, over_box),
        PositionedBox::new(base_dx, 0.0, base_box),
        PositionedBox::new(under_dx, -under_dy, under_box),
    ];

    LayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: BoxKind::VBox { children },
    }
}

fn layout_accent_c(
    arena: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node_c(arena, base, ctx);
    let base_width = base_box.width;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_box = if is_stretchy_accent(accent) && base_width > 0.0 {
        let size = ctx.size();
        match accent {
            '\u{0304}' => {
                let thickness = ctx.params.rule_thickness * size;
                LayoutBox::rule(base_width, thickness)
            },
            '\u{0307}' => {
                let dot_r = 0.04 * size;
                let dot_size = dot_r * 2.0;
                let dot = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                let dx = (base_width - dot_size) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, dot)],
                    },
                }
            },
            '\u{0308}' => {
                let dot_r = 0.04 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.12 * size;
                let total_dots_w = dot_size * 2.0 + gap;
                let dx_left = (base_width - total_dots_w) / 2.0;
                let dx_right = dx_left + dot_size + gap;
                let dot1 = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                let dot2 = LayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                };
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![
                            PositionedBox::new(dx_left, 0.0, dot1),
                            PositionedBox::new(dx_right, 0.0, dot2),
                        ],
                    },
                }
            },
            _ => {
                let accent_h = 0.12 * size;
                LayoutBox {
                    width: base_width,
                    height: accent_h,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(stretchy_accent_path(accent, base_width, accent_h)),
                }
            },
        }
    } else {
        LayoutBox::glyph(accent, ctx.size(), false)
    };

    let accent_dx = (base_width - accent_box.width) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_box.height + 0.02 * ctx.size();
            (dy, dy + accent_box.height, base_box.depth)
        },
        AccentPosition::Below => {
            let dy = -(base_box.depth + accent_box.height + 0.02 * ctx.size());
            (
                dy,
                base_box.height,
                base_box.depth + accent_box.height + 0.02 * ctx.size(),
            )
        },
    };

    let children = vec![
        PositionedBox::at_origin(base_box),
        PositionedBox::new(accent_dx, accent_dy, accent_box),
    ];

    LayoutBox {
        width: base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

#[allow(clippy::needless_range_loop)]
fn layout_array_c(
    arena: &AstArena,
    col_aligns: NodeSpan,
    cells: NodeSpan,
    num_cols: u16,
    arraystretch_pct: u8,
    add_jot: bool,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let col_gap = 1.0 * size;

    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_pct as f64 / 100.0;
    let baselineskip = 1.2 * size;
    let arrayskip = arraystretch * baselineskip;
    let arstrut_height = 0.7 * arrayskip;
    let arstrut_depth = 0.3 * arrayskip;
    let jot = 0.3 * size;

    // Extract column alignments
    let columns: Vec<ColumnAlign> = arena
        .children(col_aligns)
        .iter()
        .map(|&id| match *arena.node(id) {
            CompactNode::ColAlign(a) => a,
            _ => ColumnAlign::Center,
        })
        .collect();

    // Layout all cells (flat storage, row-major, stride = num_cols)
    let cell_ids = arena.children(cells);
    let nc = num_cols as usize;
    let n_rows = if nc > 0 { cell_ids.len() / nc } else { 0 };

    let mut cell_layouts: Vec<LayoutBox> = Vec::with_capacity(cell_ids.len());
    for &cell_id in cell_ids {
        // Each cell is a Row node
        match *arena.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layouts.push(layout_row_c(arena, span, ctx));
            },
            _ => {
                cell_layouts.push(layout_node_c(arena, cell_id, ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths with strut minimums
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for i in 0..n_rows {
        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layouts.len() {
                col_widths[j] = col_widths[j].max(cell_layouts[idx].width);
                row_heights[i] = row_heights[i].max(cell_layouts[idx].height);
                row_depths[i] = row_depths[i].max(cell_layouts[idx].depth);
            }
        }
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Position cells
    let mut children = Vec::new();
    let mut cursor_y = 0.0;

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layouts.len() {
                let col_w = col_widths[j];
                // Steal the cell layout to avoid clone
                let cell = std::mem::replace(&mut cell_layouts[idx], LayoutBox::kern(0.0));
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
                cursor_x += col_w;
            } else {
                cursor_x += col_widths.get(j).copied().unwrap_or(0.0);
            }
            if j < n_cols - 1 {
                cursor_x += col_gap;
            }
        }

        cursor_y += row_depths[i];
    }

    let total_width: f64 =
        col_widths.iter().sum::<f64>() + col_gap * (n_cols.saturating_sub(1) as f64);
    let total_extent = cursor_y;
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    LayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: BoxKind::VBox { children },
    }
}

fn layout_prescripts_c(
    arena: &AstArena,
    base: NodeId,
    pre_sub: NodeId,
    pre_sup: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node_c(arena, base, ctx);
    let p = &ctx.params;

    let mut children = Vec::new();
    let mut pre_width: f64 = 0.0;
    let mut total_h = base_box.height;
    let mut total_d = base_box.depth;

    // Bug 1 fix: use child style params for prescripts too
    let is_char_box = matches!(base_box.kind, BoxKind::Glyph { .. });

    if pre_sup.is_some() {
        let sup_style = ctx.style.sup();
        let sup_params = TexParams::for_size_class(sup_style.size_class());
        let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
        let sup_ctx = ctx.with_style(sup_style);
        let sup_box = layout_node_c(arena, pre_sup, &sup_ctx);
        let mut shift_up = p.sup2;
        if !is_char_box {
            shift_up = shift_up.max(base_box.height - sup_params.sup_drop * sup_size_ratio);
        }
        pre_width = pre_width.max(sup_box.width);
        total_h = total_h.max(shift_up + sup_box.height);
        children.push(PositionedBox::new(0.0, shift_up, sup_box));
    }
    if pre_sub.is_some() {
        let sub_style = ctx.style.sub();
        let sub_params = TexParams::for_size_class(sub_style.size_class());
        let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();
        let sub_ctx = ctx.with_style(sub_style);
        let sub_box = layout_node_c(arena, pre_sub, &sub_ctx);
        let mut shift_down = p.sub1;
        if !is_char_box {
            shift_down = shift_down.max(base_box.depth + sub_params.sub_drop * sub_size_ratio);
        }
        pre_width = pre_width.max(sub_box.width);
        total_d = total_d.max(shift_down + sub_box.depth);
        children.push(PositionedBox::new(0.0, -shift_down, sub_box));
    }

    let base_width = base_box.width;
    children.push(PositionedBox::new(pre_width, 0.0, base_box));

    LayoutBox {
        width: pre_width + base_width,
        height: total_h,
        depth: total_d,
        kind: BoxKind::HBox { children },
    }
}

fn layout_phantom_c(arena: &AstArena, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
    LayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: BoxKind::Phantom,
    }
}

fn layout_boxed_c(arena: &AstArena, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner = layout_node_c(arena, content, ctx);

    LayoutBox {
        width: inner.width + 2.0 * padding,
        height: inner.height + padding,
        depth: inner.depth + padding,
        kind: BoxKind::Boxed {
            child: Box::new(PositionedBox::new(padding, 0.0, inner)),
            padding,
        },
    }
}

fn layout_color_c(arena: &AstArena, color: StrId, content: NodeId, ctx: &LayoutCtx) -> LayoutBox {
    let color_str = arena.str_val(color).to_owned();
    let inner = layout_node_c(arena, content, ctx);
    let child_box = PositionedBox::new(0.0, 0.0, inner);
    LayoutBox {
        width: child_box.layout.width,
        height: child_box.layout.height,
        depth: child_box.layout.depth,
        kind: BoxKind::Color {
            color: color_str,
            child: Box::new(child_box),
        },
    }
}

fn layout_cancel_c(
    arena: &AstArena,
    stroke: StrokeKind,
    content: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
    let w = inner.width;
    let h = inner.height + inner.depth;

    let path = match stroke {
        StrokeKind::Forward => format!("M0,{h} L{w},0"),
        StrokeKind::Backward => format!("M0,0 L{w},{h}"),
        StrokeKind::Cross => format!("M0,{h} L{w},0 M0,0 L{w},{h}"),
        StrokeKind::Horizontal => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        StrokeKind::Vertical => {
            let mid = w / 2.0;
            format!("M{mid},0 L{mid},{h}")
        },
    };

    let inner_width = inner.width;
    let inner_height = inner.height;
    let inner_depth = inner.depth;

    let children = vec![
        PositionedBox::at_origin(inner),
        PositionedBox::at_origin(LayoutBox {
            width: w,
            height: inner_height,
            depth: inner_depth,
            kind: BoxKind::SvgPath(path),
        }),
    ];

    LayoutBox {
        width: inner_width,
        height: inner_height,
        depth: inner_depth,
        kind: BoxKind::HBox { children },
    }
}

fn layout_scaled_c(
    arena: &AstArena,
    factor: Rational,
    content: NodeId,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let inner = layout_node_c(arena, content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: inner.kind,
    }
}

fn layout_styled_c(arena: &AstArena, style: TextStyle, span: NodeSpan, ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    let ids = arena.children(span);
    if ids.is_empty() {
        return LayoutBox::kern(0.0);
    }

    let mut children = Vec::new();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        let layout = match *arena.node(id) {
            CompactNode::Identifier(sid) => {
                let s = arena.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                let styled_italic = matches!(style, TextStyle::Italic | TextStyle::BoldItalic | TextStyle::SansSerifItalic | TextStyle::SansSerifBoldItalic);
                if mapped.chars().count() == 1 {
                    let ch = mapped.chars().next().expect("non-empty checked");
                    LayoutBox::glyph(ch, ctx.size(), styled_italic)
                } else {
                    layout_text_run(&mapped, ctx, styled_italic)
                }
            },
            CompactNode::Number(sid) => {
                let s = arena.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                layout_text_run(&mapped, ctx, false)
            },
            _ => layout_node_c(arena, id, ctx),
        };
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);
        let layout_width = layout.width;
        children.push(PositionedBox::new(cursor_x, 0.0, layout));
        cursor_x += layout_width;
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children },
    }
}

// ==========================================================================
// Arena-based layout functions
// ==========================================================================

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
    pub fn layout(
        &self,
        arena: &mut LayoutArena,
        expr: &CompactExpr,
    ) -> LayoutBoxId {
        let ctx = ArenaLayoutCtx {
            style: self.style,
            params: TexParams::for_size_class(self.style.size_class()),
            options: self.options,
            depth: 0,
            size_scale: 1.0,
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
}

impl<'a> ArenaLayoutCtx<'a> {
    fn size(&self) -> f64 {
        self.style.size_multiplier() * self.size_scale
    }

    fn with_style(&self, style: MathStyle) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style,
            params: TexParams::for_size_class(style.size_class()),
            options: self.options,
            depth: self.depth,
            size_scale: self.size_scale,
        }
    }

    fn deeper(&self) -> ArenaLayoutCtx<'a> {
        ArenaLayoutCtx {
            style: self.style,
            params: self.params,
            options: self.options,
            depth: self.depth + 1,
            size_scale: self.size_scale,
        }
    }
}

// ── Leaf helpers ──────────────────────────────────────────────────────

fn alloc_glyph(arena: &mut LayoutArena, ch: char, size: f64, italic: bool) -> LayoutBoxId {
    let m = if italic {
        metrics::glyph_metrics(ch)
    } else {
        metrics::glyph_metrics_upright(ch)
    };
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph { ch, size, italic },
    })
}

fn alloc_glyph_with_metrics(
    arena: &mut LayoutArena,
    ch: char,
    m: metrics::GlyphMetrics,
    size: f64,
    italic: bool,
) -> LayoutBoxId {
    arena.alloc(ArenaLayoutBox {
        width: m.width * size,
        height: m.height * size,
        depth: m.depth * size,
        kind: ArenaBoxKind::Glyph { ch, size, italic },
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
    let new_height = (glyph_h + shift).max(glyph_h);
    let new_depth = (glyph_d - shift).max(0.0);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: shift, id: glyph_id });
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
) -> LayoutBoxId {
    let size = ctx.size();
    let mut total_width = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for ch in text.chars() {
        let m = if italic {
            metrics::glyph_metrics(ch)
        } else {
            metrics::glyph_metrics_upright(ch)
        };
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
        kind: ArenaBoxKind::TextRun { text: text.to_string(), size, italic },
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

    // Phase 1: layout all children and collect (box_id, dx) pairs.
    // We must NOT call begin_children/push_child while nested layout calls
    // are still in progress, because they also push into the same flat array.
    // SmallVec avoids heap allocation for rows with <=16 children (the common case).
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
            let space = spacing::inter_atom_space(prev, cur_atom, current_ctx.style);
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
        arena.push_child(ArenaPositionedBox { dx, dy: 0.0, id: box_id });
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
        CompactNode::Number(sid) => layout_text_run_arena(ast.str_val(sid), arena, ctx, false),
        CompactNode::Identifier(sid) => {
            let s = ast.str_val(sid);
            if s.chars().count() == 1 {
                let ch = s.chars().next().expect("non-empty checked");
                alloc_glyph(arena, ch, ctx.size(), is_math_italic_char(ch))
            } else {
                layout_text_run_arena(s, arena, ctx, false)
            }
        },
        CompactNode::Operator { name, .. } => layout_text_run_arena(ast.str_val(name), arena, ctx, false),
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
        CompactNode::Text(_, sid) => layout_text_run_arena(ast.str_val(sid), arena, ctx, false),
        CompactNode::Space(r) => arena.alloc(ArenaLayoutBox::kern(r.as_f64() * ctx.size())),
        CompactNode::Row(span) => layout_row_arena(ast, span, arena, ctx),
        CompactNode::Fenced { open, close, content } => layout_fenced_arena(ast, open, close, content, arena, ctx),
        CompactNode::Subscript { base, sub } => layout_subscript_arena(ast, base, sub, arena, ctx),
        CompactNode::Superscript { base, sup } => layout_superscript_arena(ast, base, sup, arena, ctx),
        CompactNode::SubSup { base, sub, sup } => layout_subsup_arena(ast, base, sub, sup, arena, ctx),
        CompactNode::PreScripts { base, pre_sub, pre_sup } => layout_prescripts_arena(ast, base, pre_sub, pre_sup, arena, ctx),
        CompactNode::Overscript { base, over, .. } => layout_overscript_arena(ast, base, over, arena, ctx),
        CompactNode::Underscript { base, under, .. } => layout_underscript_arena(ast, base, under, arena, ctx),
        CompactNode::UnderOver { base, under, over, .. } => layout_underover_arena(ast, base, under, over, arena, ctx),
        CompactNode::Fraction { kind, num, den } => layout_fraction_arena(ast, kind, num, den, arena, ctx),
        CompactNode::Sqrt(inner) => layout_sqrt_arena(ast, inner, None, arena, ctx),
        CompactNode::Root { index, radicand } => layout_sqrt_arena(ast, radicand, Some(index), arena, ctx),
        CompactNode::Nary { op, sub, sup, body, limits } => layout_nary_arena(ast, op, sub, sup, body, limits, arena, ctx),
        CompactNode::Accent { base, accent, position } => layout_accent_arena(ast, base, accent, position, arena, ctx),
        CompactNode::Array { col_aligns, cells, num_cols, arraystretch_pct, add_jot } => layout_array_arena(ast, col_aligns, cells, num_cols, arraystretch_pct, add_jot, arena, ctx),
        CompactNode::Phantom(inner) => layout_phantom_arena(ast, inner, arena, ctx),
        CompactNode::Boxed(inner) => layout_boxed_arena(ast, inner, arena, ctx),
        CompactNode::Cancel { stroke, content } => layout_cancel_arena(ast, stroke, content, arena, ctx),
        CompactNode::Scaled { factor, content } => layout_scaled_arena(ast, factor, content, arena, ctx),
        CompactNode::Styled(style, span) => layout_styled_arena(ast, style, span, arena, ctx),
        CompactNode::Color { color, content } => layout_color_arena(ast, color, content, arena, ctx),
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::StyleSwitch(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::SizeScale(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        CompactNode::Rule { width, height } => {
            let w = width.as_f64() * ctx.size();
            let h = height.as_f64() * ctx.size();
            arena.alloc(ArenaLayoutBox {
                width: w,
                height: h,
                depth: 0.0,
                kind: ArenaBoxKind::Rule { thickness: h },
            })
        },
    }
}

// ── Script and fraction arena layout ─────────────────────────────────

/// Arena-path: layout `\overbrace{content}^{annotation}` with annotation above brace.
fn layout_overbrace_annotated_arena(
    ast: &AstArena,
    inner_base: NodeId,
    annotation: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let p = &ctx.params;

    let content_id = layout_node_arena(ast, inner_base, arena, ctx);
    let content_w = arena.get(content_id).width;
    let content_h = arena.get(content_id).height;
    let content_d = arena.get(content_id).depth;

    let sym = "\u{23DE}";
    let deco_w = content_w.max(0.5 * size);
    let deco_h = 0.40 * size;
    let brace_id = arena.alloc(ArenaLayoutBox {
        width: deco_w,
        height: deco_h,
        depth: 0.0,
        kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
    });

    let ann_ctx = ctx.with_style(ctx.style.sup());
    let ann_id = layout_node_arena(ast, annotation, arena, &ann_ctx);
    let ann_w = arena.get(ann_id).width;
    let ann_h = arena.get(ann_id).height;
    let ann_d = arena.get(ann_id).depth;

    let brace_w = arena.get(brace_id).width;
    let brace_h = arena.get(brace_id).height;
    let brace_d = arena.get(brace_id).depth;

    let gap1 = p.big_op_spacing[2] * size;
    let gap2 = p.big_op_spacing[2] * size;

    let max_w = content_w.max(brace_w).max(ann_w);
    let content_dx = (max_w - content_w) / 2.0;
    let brace_dx = (max_w - brace_w) / 2.0;
    let ann_dx = (max_w - ann_w) / 2.0;

    let brace_dy = content_h + gap1 + brace_d;
    let ann_dy = brace_dy + brace_h + gap2 + ann_d;

    let total_height = ann_dy + ann_h;
    let total_depth = content_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: content_dx, dy: 0.0, id: content_id });
    arena.push_child(ArenaPositionedBox { dx: brace_dx, dy: brace_dy, id: brace_id });
    arena.push_child(ArenaPositionedBox { dx: ann_dx, dy: ann_dy, id: ann_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

/// Arena-path: layout `\underbrace{content}_{annotation}` with annotation below brace.
fn layout_underbrace_annotated_arena(
    ast: &AstArena,
    inner_base: NodeId,
    annotation: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let p = &ctx.params;

    let content_id = layout_node_arena(ast, inner_base, arena, ctx);
    let content_w = arena.get(content_id).width;
    let content_h = arena.get(content_id).height;
    let content_d = arena.get(content_id).depth;

    let sym = "\u{23DF}";
    let deco_w = content_w.max(0.5 * size);
    let deco_h = 0.40 * size;
    let brace_id = arena.alloc(ArenaLayoutBox {
        width: deco_w,
        height: deco_h,
        depth: 0.0,
        kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
    });

    let ann_ctx = ctx.with_style(ctx.style.sub());
    let ann_id = layout_node_arena(ast, annotation, arena, &ann_ctx);
    let ann_w = arena.get(ann_id).width;
    let ann_h = arena.get(ann_id).height;
    let ann_d = arena.get(ann_id).depth;

    let brace_w = arena.get(brace_id).width;
    let brace_h = arena.get(brace_id).height;

    let gap1 = p.big_op_spacing[4] * size;
    let gap2 = p.big_op_spacing[4] * size;

    let max_w = content_w.max(brace_w).max(ann_w);
    let content_dx = (max_w - content_w) / 2.0;
    let brace_dx = (max_w - brace_w) / 2.0;
    let ann_dx = (max_w - ann_w) / 2.0;

    let brace_dy = content_d + gap1 + brace_h;
    let ann_dy = brace_dy + 0.0 + gap2 + ann_h;

    let total_height = content_h;
    let total_depth = ann_dy + ann_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: content_dx, dy: 0.0, id: content_id });
    arena.push_child(ArenaPositionedBox { dx: brace_dx, dy: -brace_dy, id: brace_id });
    arena.push_child(ArenaPositionedBox { dx: ann_dx, dy: -ann_dy, id: ann_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_superscript_arena(
    ast: &AstArena,
    base: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place superscript above (limits)
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, None, Some(sup), arena, ctx);
    }

    // Overbrace annotation: Superscript { base: Overscript { over: "⏞" }, sup: annotation }
    if let CompactNode::Overscript { base: inner_base, over, .. } = *ast.node(base)
        && node_is_overbrace_c(ast, over)
    {
        return layout_overbrace_annotated_arena(ast, inner_base, sup, arena, ctx);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;

    let ic = italic_correction_arena(base_id, arena);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_d);

    let total_width = base_w + ic + sup_w;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w + ic, dy: shift_up, id: sup_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_subscript_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place subscript below (limits)
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, Some(sub), None, arena, ctx);
    }

    // Underbrace annotation: Subscript { base: Underscript { under: "⏟" }, sub: annotation }
    if let CompactNode::Underscript { base: inner_base, under, .. } = *ast.node(base)
        && node_is_underbrace_c(ast, under)
    {
        return layout_underbrace_annotated_arena(ast, inner_base, sub, arena, ctx);
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(base_id, arena);
    let sub_dx = (base_w - ic).max(0.0);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_h - (4.0 * p.x_height / 5.0));

    let total_width = sub_dx + sub_w;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_subsup_arena(
    ast: &AstArena,
    base: NodeId,
    sub: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Large operator in display mode: place limits above/below
    if is_large_op_c(ast, base) && ctx.style.is_display() {
        return layout_big_op_limits_arena(ast, base, Some(sub), Some(sup), arena, ctx);
    }
    let base_id = layout_node_arena(ast, base, arena, ctx);
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    // Read dimensions before any further arena mutation
    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(base_id, arena);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_d);

    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_d) + sub_h;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_d);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    let sup_dx = base_w + ic;
    let sub_dx = (base_w - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_w + sup_w)
        .max(sub_dx - base_w + sub_w)
        .max(0.0);
    let total_width = base_w + script_width;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: shift_up, id: sup_id });
    arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Layout a big operator (LargeOp symbol) with limits above/below (arena path).
fn layout_big_op_limits_arena(
    ast: &AstArena,
    base: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Build the operator glyph at display size
    let op_ch = large_op_char_c(ast, base).unwrap_or('\u{2211}');
    let op_size = size * metrics::display_op_scale(op_ch);
    let m = metrics::glyph_metrics_display(op_ch);
    let op_id = alloc_glyph_with_metrics(arena, op_ch, m, op_size, false);

    let op_w = arena.get(op_id).width;
    let op_h = arena.get(op_id).height;
    let op_d = arena.get(op_id).depth;

    let mut total_h = op_h;
    let mut total_d = op_d;
    let mut max_w = op_w;

    let sup_id_opt = sup.map(|s| {
        let sup_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, s, arena, &sup_ctx)
    });
    let sub_id_opt = sub.map(|s| {
        let sub_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, s, arena, &sub_ctx)
    });

    let (sup_w_opt, sup_h_opt, sup_d_opt) = if let Some(sid) = sup_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };
    let (sub_w_opt, sub_h_opt, sub_d_opt) = if let Some(sid) = sub_id_opt {
        let b = arena.get(sid);
        (b.width, b.height, b.depth)
    } else {
        (0.0, 0.0, 0.0)
    };

    let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
    let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
    let padding = p.big_op_spacing[4] * size;

    if sup_id_opt.is_some() {
        max_w = max_w.max(sup_w_opt);
        total_h += sup_h_opt + sup_d_opt + sup_gap + padding;
    }
    if sub_id_opt.is_some() {
        max_w = max_w.max(sub_w_opt);
        total_d += sub_h_opt + sub_d_opt + sub_gap + padding;
    }

    let op_dx = (max_w - op_w) / 2.0;

    let children_start = arena.begin_children();

    if let Some(sid) = sup_id_opt {
        let sup_dx = (max_w - sup_w_opt) / 2.0;
        let sup_dy = op_h + sup_gap + sup_d_opt;
        arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: sup_dy, id: sid });
    }
    arena.push_child(ArenaPositionedBox { dx: op_dx, dy: 0.0, id: op_id });
    if let Some(sid) = sub_id_opt {
        let sub_dx = (max_w - sub_w_opt) / 2.0;
        let sub_dy = -(op_d + sub_gap + sub_h_opt);
        arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: sub_dy, id: sid });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_fraction_arena(
    ast: &AstArena,
    kind: FractionKind,
    num: NodeId,
    den: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Override context style for \dfrac (Display) and \tfrac (Inline)
    let override_ctx = match kind {
        FractionKind::Display => Some(ctx.with_style(MathStyle::Display)),
        FractionKind::Inline => Some(ctx.with_style(MathStyle::Text)),
        _ => None,
    };
    let ctx = override_ctx.as_ref().unwrap_or(ctx);

    let p = &ctx.params;
    let size = ctx.size();

    let num_ctx = ctx.with_style(ctx.style.fracnum());
    let den_ctx = ctx.with_style(ctx.style.fracden());
    let num_id = layout_node_arena(ast, num, arena, &num_ctx);
    let den_id = layout_node_arena(ast, den, arena, &den_ctx);

    // Read dimensions before any further arena mutation
    let num_w = arena.get(num_id).width;
    let num_h = arena.get(num_id).height;
    let num_d = arena.get(num_id).depth;
    let den_w = arena.get(den_id).width;
    let den_h = arena.get(den_id).height;
    let den_d = arena.get(den_id).depth;

    let bar_thickness = match kind {
        FractionKind::NoBar => 0.0,
        _ => p.rule_thickness * size,
    };

    let (num_shift, den_shift) = if ctx.style.is_display() {
        (p.num1 * size, p.denom1 * size)
    } else if bar_thickness == 0.0 {
        (p.num3 * size, p.denom2 * size)
    } else {
        (p.num2 * size, p.denom2 * size)
    };

    let axis = p.axis_height * size;
    let mut num_up = num_shift;
    let mut den_down = den_shift;

    if bar_thickness > 0.0 {
        let min_clearance = if ctx.style.is_display() {
            3.0 * bar_thickness
        } else {
            bar_thickness
        };

        let num_clearance = (num_up - num_d) - (axis + bar_thickness / 2.0);
        if num_clearance < min_clearance {
            num_up += min_clearance - num_clearance;
        }

        let den_clearance = (axis - bar_thickness / 2.0) - (den_h - den_down);
        if den_clearance < min_clearance {
            den_down += min_clearance - den_clearance;
        }
    } else {
        let min_gap = if ctx.style.is_display() {
            7.0 * p.rule_thickness * size
        } else {
            3.0 * p.rule_thickness * size
        };
        let gap = (num_up - num_d) - (den_h - den_down);
        if gap < min_gap {
            let adjust = (min_gap - gap) / 2.0;
            num_up += adjust;
            den_down += adjust;
        }
    }

    let frac_width = num_w.max(den_w);
    // Add null-delimiter-space side padding to prevent content from touching adjacent glyphs
    let side_padding = 0.12 * size;
    let padded_frac_width = frac_width + 2.0 * side_padding;
    let total_height = num_up + num_h;
    let total_depth = den_down + den_d;

    let num_dx = side_padding + (frac_width - num_w) / 2.0;
    let den_dx = side_padding + (frac_width - den_w) / 2.0;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: num_dx, dy: num_up, id: num_id });
    arena.push_child(ArenaPositionedBox { dx: den_dx, dy: -den_down, id: den_id });

    if bar_thickness > 0.0 {
        let bar_id = arena.alloc(ArenaLayoutBox::rule(padded_frac_width, bar_thickness));
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: axis, id: bar_id });
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: padded_frac_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

// ── Remaining arena layout functions ─────────────────────────────────

fn layout_sqrt_arena(
    ast: &AstArena,
    radicand: NodeId,
    index: Option<NodeId>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();
    let cramped_ctx = ctx.with_style(ctx.style.cramped());
    let inner_id = layout_node_arena(ast, radicand, arena, &cramped_ctx);

    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let rule_thickness = p.rule_thickness * size;

    // KaTeX-compatible clearance: theta + phi/4
    // where phi = xHeight in display mode, theta otherwise
    let phi = if ctx.style.is_display() {
        p.x_height * size // ~0.431em — the x-height
    } else {
        rule_thickness // 0.04em — same as theta
    };
    let extra_clearance = rule_thickness + phi / 4.0;

    let inner_total = inner_h + inner_d + extra_clearance + rule_thickness;

    // Minimum surd height of 1.0em (KaTeX's "small" surd minimum)
    let min_surd_height = 1.0 * size;
    let surd_height = inner_total.max(min_surd_height);

    // If surd is taller than needed, redistribute extra space evenly
    let extra_clearance = if surd_height > inner_total {
        extra_clearance + (surd_height - inner_total) / 2.0
    } else {
        extra_clearance
    };
    // Recompute with potentially larger clearance
    let _inner_total = surd_height;

    let radical_width = 0.833 * size;
    let radical_id = arena.alloc(ArenaLayoutBox {
        width: radical_width,
        height: surd_height,
        depth: 0.0,
        kind: ArenaBoxKind::SvgPath(radical_path(radical_width, surd_height)),
    });

    let vinculum_id = arena.alloc(ArenaLayoutBox::rule(inner_w, rule_thickness));

    let shift_up = inner_h + extra_clearance + rule_thickness;

    // Build children: radical, vinculum, inner
    // We may need to adjust dx values if there's an index, so we track the span start
    // and positions separately.
    let mut radical_dx = 0.0_f64;
    let mut vinculum_dx = radical_width;
    let mut inner_dx = radical_width;
    let mut total_width = radical_width + inner_w;
    let total_height = shift_up + rule_thickness;
    let total_depth = inner_d;

    // Optional root index
    if let Some(idx_id_node) = index {
        let idx_ctx = ctx.with_style(MathStyle::ScriptScript);
        let idx_box_id = layout_node_arena(ast, idx_id_node, arena, &idx_ctx);
        let idx_w = arena.get(idx_box_id).width;
        let idx_shift = total_height * 0.6;
        let idx_overflow = idx_w.max(radical_width * 0.5);
        if idx_overflow > radical_width * 0.5 {
            let extra = idx_overflow - radical_width * 0.5;
            radical_dx += extra;
            vinculum_dx += extra;
            inner_dx += extra;
            total_width += extra;
        }

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: radical_dx, dy: 0.0, id: radical_id });
        arena.push_child(ArenaPositionedBox { dx: vinculum_dx, dy: shift_up, id: vinculum_id });
        arena.push_child(ArenaPositionedBox { dx: inner_dx, dy: 0.0, id: inner_id });
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: idx_shift, id: idx_box_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: radical_dx, dy: 0.0, id: radical_id });
        arena.push_child(ArenaPositionedBox { dx: vinculum_dx, dy: shift_up, id: vinculum_id });
        arena.push_child(ArenaPositionedBox { dx: inner_dx, dy: 0.0, id: inner_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: total_width,
            height: total_height,
            depth: total_depth,
            kind: ArenaBoxKind::HBox { children },
        })
    }
}

fn layout_fenced_arena(
    ast: &AstArena,
    open: StrId,
    close: StrId,
    content: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    // Layout content, skipping Middle separator nodes
    // We need to collect IDs first, then measure
    let mut inner_ids: Vec<LayoutBoxId> = Vec::new();
    for &child_id in ast.children(content) {
        match *ast.node(child_id) {
            CompactNode::Middle(_) => {},
            _ => {
                let laid = layout_node_arena(ast, child_id, arena, ctx);
                inner_ids.push(laid);
            },
        }
    }

    // Compute max height/depth of content
    let mut max_h: f64 = 0.0;
    let mut max_d: f64 = 0.0;
    for &id in &inner_ids {
        let b = arena.get(id);
        max_h = max_h.max(b.height);
        max_d = max_d.max(b.depth);
    }

    // Axis-centered target height (KaTeX-compatible): delimiters extend
    // symmetrically around the math axis, ensuring proper visual balance.
    let axis = ctx.params.axis_height * ctx.size();
    let max_dist_from_axis = (max_h - axis).max(max_d + axis);
    let target_h = (2.0 * max_dist_from_axis).max(max_h + max_d);
    let delim_h = (axis + max_dist_from_axis).max(max_h);
    let delim_d = (max_dist_from_axis - axis).max(max_d);

    // Compute delimiter width using path-based sizing for tall content
    use super::delimiter_paths;

    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;

    // Open delimiter
    let open_str = ast.str_val(open);
    if !open_str.is_empty() && open_str != "."
        && let Some(ch) = open_str.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
        let delim_w = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id: delim_id });
        cursor_x += delim_w;
    }

    // Content
    for &id in &inner_ids {
        let w = arena.get(id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id });
        cursor_x += w;
    }

    // Close delimiter
    let close_str = ast.str_val(close);
    if !close_str.is_empty() && close_str != "."
        && let Some(ch) = close_str.chars().next()
    {
        let delim_width = if delimiter_paths::should_use_path(ch, target_h, ctx.size()) {
            delimiter_paths::delimiter_path_width(ch, target_h, ctx.size())
        } else {
            let base_delim_width = 0.389 * ctx.size();
            let standard_height = 0.9 * ctx.size();
            if target_h > standard_height {
                base_delim_width * (target_h / standard_height)
            } else {
                base_delim_width
            }
        };
        let delim_id = arena.alloc(ArenaLayoutBox {
            width: delim_width,
            height: delim_h,
            depth: delim_d,
            kind: ArenaBoxKind::Delimiter {
                ch,
                target_height: target_h,
            },
        });
        let delim_w = arena.get(delim_id).width;
        arena.push_child(ArenaPositionedBox { dx: cursor_x, dy: 0.0, id: delim_id });
        cursor_x += delim_w;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_h,
        depth: max_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[allow(clippy::too_many_arguments)]
fn layout_nary_arena(
    ast: &AstArena,
    op: char,
    sub: NodeId,
    sup: NodeId,
    body: NodeId,
    limits: LimitStyle,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Large op in display mode: use per-operator scale matching KaTeX Size2/Size1
    let (op_size, m) = if ctx.style.is_display() {
        (size * metrics::display_op_scale(op), metrics::glyph_metrics_display(op))
    } else {
        (size, metrics::glyph_metrics(op))
    };
    let op_id = alloc_glyph_with_metrics(arena, op, m, op_size, false);

    let body_id = layout_node_arena(ast, body, arena, ctx);
    let body_w = arena.get(body_id).width;
    let body_h = arena.get(body_id).height;
    let body_d = arena.get(body_id).depth;

    let use_limits = match limits {
        LimitStyle::AboveBelow => true,
        LimitStyle::SubSup => false,
        // Integrals default to side-limits (nolimits); sums/products to above/below in display.
        LimitStyle::Default => ctx.style.is_display() && !is_nolimits_op(op),
    };

    if use_limits {
        // Limits above/below
        let op_w = arena.get(op_id).width;
        let op_h = arena.get(op_id).height;
        let op_d = arena.get(op_id).depth;

        let mut total_h = op_h;
        let mut total_d = op_d;
        let mut max_w = op_w;

        let sup_id_opt = if sup.is_some() {
            let sup_ctx = ctx.with_style(ctx.style.sup());
            let sid = layout_node_arena(ast, sup, arena, &sup_ctx);
            Some(sid)
        } else {
            None
        };
        let sub_id_opt = if sub.is_some() {
            let sub_ctx = ctx.with_style(ctx.style.sub());
            let sid = layout_node_arena(ast, sub, arena, &sub_ctx);
            Some(sid)
        } else {
            None
        };

        // Read dimensions for sup/sub
        let (sup_w_opt, sup_h_opt, sup_d_opt) = if let Some(sid) = sup_id_opt {
            let b = arena.get(sid);
            (b.width, b.height, b.depth)
        } else {
            (0.0, 0.0, 0.0)
        };
        let (sub_w_opt, sub_h_opt, sub_d_opt) = if let Some(sid) = sub_id_opt {
            let b = arena.get(sid);
            (b.width, b.height, b.depth)
        } else {
            (0.0, 0.0, 0.0)
        };

        let sup_gap = (p.big_op_spacing[0] * size).max(p.big_op_spacing[1] * size);
        let sub_gap = (p.big_op_spacing[3] * size).max(p.big_op_spacing[2] * size);
        let padding = p.big_op_spacing[4] * size;

        if sup_id_opt.is_some() {
            max_w = max_w.max(sup_w_opt);
            total_h += sup_h_opt + sup_d_opt + sup_gap + padding;
        }
        if sub_id_opt.is_some() {
            max_w = max_w.max(sub_w_opt);
            total_d += sub_h_opt + sub_d_opt + sub_gap + padding;
        }

        let op_dx = (max_w - op_w) / 2.0;

        let children_start = arena.begin_children();

        if let Some(sid) = sup_id_opt {
            let sup_dx = (max_w - sup_w_opt) / 2.0;
            let sup_dy = op_h + sup_gap + sup_d_opt;
            arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: sup_dy, id: sid });
        }
        arena.push_child(ArenaPositionedBox { dx: op_dx, dy: 0.0, id: op_id });
        if let Some(sid) = sub_id_opt {
            let sub_dx = (max_w - sub_w_opt) / 2.0;
            let sub_dy = -(op_d + sub_gap + sub_h_opt);
            arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: sub_dy, id: sid });
        }

        // Body follows
        let nary_width = max_w;
        let thin_space = 3.0 / 18.0 * size;
        arena.push_child(ArenaPositionedBox { dx: nary_width + thin_space, dy: 0.0, id: body_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: nary_width + thin_space + body_w,
            height: total_h,
            depth: total_d,
            kind: ArenaBoxKind::HBox { children },
        })
    } else {
        // As sub/superscripts
        let thin_space = 3.0 / 18.0 * size;

        // Use the already-scaled op_id for the operator glyph
        let op_glyph_id = op_id;

        if sub.is_some() && sup.is_some() {
            let result_id = layout_subsup_from_box_arena(op_glyph_id, ast, sub, sup, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        } else if sub.is_some() {
            let result_id = layout_subscript_from_box_arena(op_glyph_id, ast, sub, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        } else if sup.is_some() {
            let result_id = layout_superscript_from_box_arena(op_glyph_id, ast, sup, arena, ctx);
            return build_hbox_arena(result_id, body_id, thin_space, arena);
        }

        // Just op + body
        let op_w = arena.get(op_glyph_id).width;
        let op_h = arena.get(op_glyph_id).height;
        let op_d = arena.get(op_glyph_id).depth;

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: op_glyph_id });
        arena.push_child(ArenaPositionedBox { dx: op_w + thin_space, dy: 0.0, id: body_id });
        let children = arena.finish_children(children_start);

        arena.alloc(ArenaLayoutBox {
            width: op_w + thin_space + body_w,
            height: op_h.max(body_h),
            depth: op_d.max(body_d),
            kind: ArenaBoxKind::HBox { children },
        })
    }
}

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
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: result_id });
    arena.push_child(ArenaPositionedBox { dx: rw + thin_space, dy: 0.0, id: body_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: rw + thin_space + bw,
        height: rh.max(bh),
        depth: rd.max(bd),
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: superscript layout where the base is an already-laid-out arena box.
fn layout_superscript_from_box_arena(
    base_id: LayoutBoxId,
    ast: &AstArena,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;

    let ic = italic_correction_arena(base_id, arena);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sup_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio);
    }
    let shift_up = shift_up.max(p.x_height / 4.0 + sup_d);

    let total_width = base_w + ic + sup_w;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = base_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: base_w + ic, dy: shift_up, id: sup_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subscript layout where the base is an already-laid-out arena box.
fn layout_subscript_from_box_arena(
    base_id: LayoutBoxId,
    ast: &AstArena,
    sub: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(base_id, arena);
    let sub_dx = (base_w - ic).max(0.0);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;
    // Bug 1 fix: use child (script) style params for sub_drop
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_down = p.sub1;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio);
    }
    let shift_down = shift_down.max(sub_h - (4.0 * p.x_height / 5.0));

    let total_width = sub_dx + sub_w;
    let total_height = base_h;
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

/// Helper: subsup layout where the base is an already-laid-out arena box.
fn layout_subsup_from_box_arena(
    base_id: LayoutBoxId,
    ast: &AstArena,
    sub: NodeId,
    sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let sup_ctx = ctx.with_style(ctx.style.sup());
    let sub_ctx = ctx.with_style(ctx.style.sub());
    let sup_id = layout_node_arena(ast, sup, arena, &sup_ctx);
    let sub_id = layout_node_arena(ast, sub, arena, &sub_ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;
    let sup_h = arena.get(sup_id).height;
    let sup_d = arena.get(sup_id).depth;
    let sup_w = arena.get(sup_id).width;
    let sub_h = arena.get(sub_id).height;
    let sub_d = arena.get(sub_id).depth;
    let sub_w = arena.get(sub_id).width;

    let ic = italic_correction_arena(base_id, arena);

    // Bug 2 fix: check if base is a single character
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    let p = &ctx.params;

    // Bug 1 fix: use child style params for sup_drop/sub_drop
    let sup_style = ctx.style.sup();
    let sup_params = TexParams::for_size_class(sup_style.size_class());
    let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
    let sub_style = ctx.style.sub();
    let sub_params = TexParams::for_size_class(sub_style.size_class());
    let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();

    let mut shift_up = if ctx.style.is_display() {
        p.sup1
    } else if ctx.style.is_cramped() {
        p.sup3
    } else {
        p.sup2
    };
    if !is_char_box {
        shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio);
    }
    shift_up = shift_up.max(p.x_height / 4.0 + sup_d);

    let mut shift_down = p.sub2;
    if !is_char_box {
        shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio);
    }

    // Bug 3 fix: ensure gap between sub top and sup bottom (Rule 18d)
    let gap = (shift_up - sup_d) - (sub_h - shift_down);
    let min_gap = 4.0 * p.rule_thickness * ctx.size();
    if gap < min_gap {
        shift_down = min_gap - (shift_up - sup_d) + sub_h;
    }

    let psi = 0.8 * p.x_height * ctx.size() - (shift_up - sup_d);
    if psi > 0.0 {
        shift_up += psi;
        shift_down -= psi;
    }

    let sup_dx = base_w + ic;
    let sub_dx = (base_w - ic / 2.0).max(0.0);

    let script_width = (sup_dx - base_w + sup_w)
        .max(sub_dx - base_w + sub_w)
        .max(0.0);
    let total_width = base_w + script_width;
    let total_height = (shift_up + sup_h).max(base_h);
    let total_depth = (shift_down + sub_d).max(base_d);

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: sup_dx, dy: shift_up, id: sup_id });
    arena.push_child(ArenaPositionedBox { dx: sub_dx, dy: -shift_down, id: sub_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: total_height,
        depth: total_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_overscript_arena(
    ast: &AstArena,
    base: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_id = layout_node_arena(ast, over, arena, &over_ctx);
        let over_w = arena.get(over_id).width;
        let over_h = arena.get(over_id).height;
        let over_d = arena.get(over_id).depth;

        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = over_w.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_id = arena.alloc(ArenaLayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        });

        let gap = p.big_op_spacing[2] * size;
        let max_w = arrow_w.max(over_w);
        let base_dx = (max_w - arrow_w) / 2.0;
        let over_dx = (max_w - over_w) / 2.0;
        let over_dy = arrow_h / 2.0 + gap + over_d;

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: arrow_id });
        arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
        let children = arena.finish_children(children_start);

        return arena.alloc(ArenaLayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: arrow_h / 2.0,
            kind: ArenaBoxKind::VBox { children },
        });
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    // Check if the over node is a stretchy decoration symbol
    let over_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.40 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, over, arena, &over_ctx)
    };

    let over_w = arena.get(over_id).width;
    let over_h = arena.get(over_id).height;
    let over_d = arena.get(over_id).depth;

    let gap = p.big_op_spacing[2] * size;
    let max_w = base_w.max(over_w);
    let base_dx = (max_w - base_w) / 2.0;
    let over_dx = (max_w - over_w) / 2.0;
    let over_dy = base_h + gap + over_d;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: base_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_underscript_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_id = layout_node_arena(ast, under, arena, &under_ctx);
        let under_w = arena.get(under_id).width;
        let under_h = arena.get(under_id).height;
        let under_d = arena.get(under_id).depth;

        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let arrow_w = under_w.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_id = arena.alloc(ArenaLayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        });

        let gap = p.big_op_spacing[4] * size;
        let max_w = arrow_w.max(under_w);
        let base_dx = (max_w - arrow_w) / 2.0;
        let under_dx = (max_w - under_w) / 2.0;
        let under_dy = arrow_h / 2.0 + gap + under_h;

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: arrow_id });
        arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
        let children = arena.finish_children(children_start);

        return arena.alloc(ArenaLayoutBox {
            width: max_w,
            height: arrow_h / 2.0,
            depth: under_dy + under_d,
            kind: ArenaBoxKind::VBox { children },
        });
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let under_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.40 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, under, arena, &under_ctx)
    };

    let under_w = arena.get(under_id).width;
    let under_h = arena.get(under_id).height;
    let under_d = arena.get(under_id).depth;

    let gap = p.big_op_spacing[4] * size;
    let max_w = base_w.max(under_w);
    let base_dx = (max_w - base_w) / 2.0;
    let under_dx = (max_w - under_w) / 2.0;
    let under_dy = base_d + gap + under_h;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: base_h,
        depth: under_dy + under_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_underover_arena(
    ast: &AstArena,
    base: NodeId,
    under: NodeId,
    over: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let p = &ctx.params;
    let size = ctx.size();

    // Check if base is an extensible arrow
    if let Some(arrow_ch) = get_extensible_arrow_c(ast, base) {
        let over_ctx = ctx.with_style(ctx.style.sup());
        let over_id = layout_node_arena(ast, over, arena, &over_ctx);
        let under_ctx = ctx.with_style(ctx.style.sub());
        let under_id = layout_node_arena(ast, under, arena, &under_ctx);

        let over_w = arena.get(over_id).width;
        let over_h = arena.get(over_id).height;
        let over_d = arena.get(over_id).depth;
        let under_w = arena.get(under_id).width;
        let under_h = arena.get(under_id).height;
        let under_d = arena.get(under_id).depth;

        let padding = 0.6 * size;
        let min_width = 1.5 * size;
        let content_w = over_w.max(under_w);
        let arrow_w = content_w.max(min_width) + padding;
        let arrow_h = 0.44 * size;
        let arrow_id = arena.alloc(ArenaLayoutBox {
            width: arrow_w,
            height: arrow_h / 2.0,
            depth: arrow_h / 2.0,
            kind: ArenaBoxKind::SvgPath(stretchy_arrow_svg(arrow_ch, arrow_w, arrow_h)),
        });

        let gap_above = p.big_op_spacing[2] * size;
        let gap_below = p.big_op_spacing[4] * size;
        let max_w = arrow_w.max(over_w).max(under_w);
        let base_dx = (max_w - arrow_w) / 2.0;
        let over_dx = (max_w - over_w) / 2.0;
        let under_dx = (max_w - under_w) / 2.0;
        let over_dy = arrow_h / 2.0 + gap_above + over_d;
        let under_dy = arrow_h / 2.0 + gap_below + under_h;

        let children_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
        arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: arrow_id });
        arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
        let children = arena.finish_children(children_start);

        return arena.alloc(ArenaLayoutBox {
            width: max_w,
            height: over_dy + over_h,
            depth: under_dy + under_d,
            kind: ArenaBoxKind::VBox { children },
        });
    }

    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let over_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, over) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.40 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let over_ctx = ctx.with_style(ctx.style.sup());
        layout_node_arena(ast, over, arena, &over_ctx)
    };

    let under_id = if let Some(sym) = get_stretchy_overunder_sym_c(ast, under) {
        let deco_w = base_w.max(0.5 * size);
        let deco_h = 0.40 * size;
        arena.alloc(ArenaLayoutBox {
            width: deco_w,
            height: deco_h,
            depth: 0.0,
            kind: ArenaBoxKind::SvgPath(stretchy_overunder_path(sym, deco_w, deco_h)),
        })
    } else {
        let under_ctx = ctx.with_style(ctx.style.sub());
        layout_node_arena(ast, under, arena, &under_ctx)
    };

    let over_w = arena.get(over_id).width;
    let over_h = arena.get(over_id).height;
    let over_d = arena.get(over_id).depth;
    let under_w = arena.get(under_id).width;
    let under_h = arena.get(under_id).height;
    let under_d = arena.get(under_id).depth;

    let gap_above = p.big_op_spacing[2] * size;
    let gap_below = p.big_op_spacing[4] * size;
    let max_w = base_w.max(over_w).max(under_w);

    let base_dx = (max_w - base_w) / 2.0;
    let over_dx = (max_w - over_w) / 2.0;
    let under_dx = (max_w - under_w) / 2.0;

    let over_dy = base_h + gap_above + over_d;
    let under_dy = base_d + gap_below + under_h;

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: over_dx, dy: over_dy, id: over_id });
    arena.push_child(ArenaPositionedBox { dx: base_dx, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: under_dx, dy: -under_dy, id: under_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: max_w,
        height: over_dy + over_h,
        depth: under_dy + under_d,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_accent_arena(
    ast: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let base_id = layout_node_arena(ast, base, arena, ctx);

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_id = if is_stretchy_accent(accent) && base_w > 0.0 {
        let size = ctx.size();
        match accent {
            '\u{0304}' => {
                let thickness = ctx.params.rule_thickness * size;
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: thickness / 2.0,
                    depth: thickness / 2.0,
                    kind: ArenaBoxKind::Rule { thickness },
                })
            },
            '\u{0307}' => {
                let dot_r = 0.04 * size;
                let dot_size = dot_r * 2.0;
                let dx = (base_w - dot_size) / 2.0;
                let dot_id = arena.alloc(ArenaLayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                });
                let children_start = arena.begin_children();
                arena.push_child(ArenaPositionedBox { dx, dy: 0.0, id: dot_id });
                let children = arena.finish_children(children_start);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            '\u{0308}' => {
                let dot_r = 0.04 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.12 * size;
                let total_dots_w = dot_size * 2.0 + gap;
                let dx_left = (base_w - total_dots_w) / 2.0;
                let dx_right = dx_left + dot_size + gap;
                let dot1 = arena.alloc(ArenaLayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                });
                let dot2 = arena.alloc(ArenaLayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                });
                let children_start = arena.begin_children();
                arena.push_child(ArenaPositionedBox { dx: dx_left, dy: 0.0, id: dot1 });
                arena.push_child(ArenaPositionedBox { dx: dx_right, dy: 0.0, id: dot2 });
                let children = arena.finish_children(children_start);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            _ => {
                let accent_h = 0.12 * size;
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: accent_h,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(stretchy_accent_path(accent, base_w, accent_h)),
                })
            },
        }
    } else {
        alloc_glyph(arena, accent, ctx.size(), false)
    };

    let accent_w = arena.get(accent_id).width;
    let accent_h = arena.get(accent_id).height;

    let accent_dx = (base_w - accent_w) / 2.0;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let dy = base_h + 0.02 * ctx.size();
            (dy, dy + accent_h, base_d)
        },
        AccentPosition::Below => {
            let dy = -(base_d + accent_h + 0.02 * ctx.size());
            (
                dy,
                base_h,
                base_d + accent_h + 0.02 * ctx.size(),
            )
        },
    };

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: base_id });
    arena.push_child(ArenaPositionedBox { dx: accent_dx, dy: accent_dy, id: accent_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[allow(clippy::needless_range_loop)]
#[allow(clippy::too_many_arguments)]
fn layout_array_arena(
    ast: &AstArena,
    col_aligns: NodeSpan,
    cells: NodeSpan,
    num_cols: u16,
    arraystretch_pct: u8,
    add_jot: bool,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let col_gap = 1.0 * size;

    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_pct as f64 / 100.0;
    let baselineskip = 1.2 * size;
    let arrayskip = arraystretch * baselineskip;
    let arstrut_height = 0.7 * arrayskip;
    let arstrut_depth = 0.3 * arrayskip;
    let jot = 0.3 * size;

    // Extract column alignments
    let columns: Vec<ColumnAlign> = ast
        .children(col_aligns)
        .iter()
        .map(|&id| match *ast.node(id) {
            CompactNode::ColAlign(a) => a,
            _ => ColumnAlign::Center,
        })
        .collect();

    // Layout all cells (flat storage, row-major, stride = num_cols)
    let cell_ids_ast = ast.children(cells);
    let nc = num_cols as usize;
    let n_rows = if nc > 0 { cell_ids_ast.len() / nc } else { 0 };

    // Layout each cell and store the layout box IDs
    let mut cell_layout_ids: Vec<LayoutBoxId> = Vec::with_capacity(cell_ids_ast.len());
    for &cell_id in cell_ids_ast {
        match *ast.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layout_ids.push(layout_row_arena(ast, span, arena, ctx));
            },
            _ => {
                cell_layout_ids.push(layout_node_arena(ast, cell_id, arena, ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths with strut minimums
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for i in 0..n_rows {
        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let b = arena.get(cell_layout_ids[idx]);
                col_widths[j] = col_widths[j].max(b.width);
                row_heights[i] = row_heights[i].max(b.height);
                row_depths[i] = row_depths[i].max(b.depth);
            }
        }
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Pre-compute total extent and box_height so we can convert top-down
    // cursor_y to VBox dy (positive = up from baseline) when pushing children.
    let total_extent: f64 = row_heights.iter().sum::<f64>()
        + row_depths.iter().sum::<f64>();
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Position cells
    let children_start = arena.begin_children();
    let mut cursor_y = 0.0;

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let col_w = col_widths[j];
                let cell_w = arena.get(cell_layout_ids[idx]).width;
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell_w) / 2.0,
                    ColumnAlign::Right => col_w - cell_w,
                };
                arena.push_child(ArenaPositionedBox {
                    dx: cursor_x + dx,
                    dy: box_height - cursor_y,
                    id: cell_layout_ids[idx],
                });
                cursor_x += col_w;
            } else {
                cursor_x += col_widths.get(j).copied().unwrap_or(0.0);
            }
            if j < n_cols - 1 {
                cursor_x += col_gap;
            }
        }

        cursor_y += row_depths[i];
    }

    let children = arena.finish_children(children_start);
    let total_width: f64 =
        col_widths.iter().sum::<f64>() + col_gap * (n_cols.saturating_sub(1) as f64);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: ArenaBoxKind::VBox { children },
    })
}

fn layout_prescripts_arena(
    ast: &AstArena,
    base: NodeId,
    pre_sub: NodeId,
    pre_sup: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let base_id = layout_node_arena(ast, base, arena, ctx);
    let p = &ctx.params;

    let base_w = arena.get(base_id).width;
    let base_h = arena.get(base_id).height;
    let base_d = arena.get(base_id).depth;

    let mut pre_width: f64 = 0.0;
    let mut total_h = base_h;
    let mut total_d = base_d;

    // Bug 1 fix: use child style params for prescripts too
    let is_char_box = matches!(arena.get(base_id).kind, ArenaBoxKind::Glyph { .. });

    // Layout pre-superscript and pre-subscript
    let pre_sup_id = if pre_sup.is_some() {
        let sup_style = ctx.style.sup();
        let sup_params = TexParams::for_size_class(sup_style.size_class());
        let sup_size_ratio = sup_style.size_multiplier() / ctx.style.size_multiplier();
        let sup_ctx = ctx.with_style(sup_style);
        let sid = layout_node_arena(ast, pre_sup, arena, &sup_ctx);
        let b = arena.get(sid);
        let mut shift_up = p.sup2;
        if !is_char_box {
            shift_up = shift_up.max(base_h - sup_params.sup_drop * sup_size_ratio);
        }
        pre_width = pre_width.max(b.width);
        total_h = total_h.max(shift_up + b.height);
        Some((sid, shift_up))
    } else {
        None
    };

    let pre_sub_id = if pre_sub.is_some() {
        let sub_style = ctx.style.sub();
        let sub_params = TexParams::for_size_class(sub_style.size_class());
        let sub_size_ratio = sub_style.size_multiplier() / ctx.style.size_multiplier();
        let sub_ctx = ctx.with_style(sub_style);
        let sid = layout_node_arena(ast, pre_sub, arena, &sub_ctx);
        let b = arena.get(sid);
        let mut shift_down = p.sub1;
        if !is_char_box {
            shift_down = shift_down.max(base_d + sub_params.sub_drop * sub_size_ratio);
        }
        pre_width = pre_width.max(b.width);
        total_d = total_d.max(shift_down + b.depth);
        Some((sid, shift_down))
    } else {
        None
    };

    let children_start = arena.begin_children();
    if let Some((sid, shift_up)) = pre_sup_id {
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: shift_up, id: sid });
    }
    if let Some((sid, shift_down)) = pre_sub_id {
        arena.push_child(ArenaPositionedBox { dx: 0.0, dy: -shift_down, id: sid });
    }
    arena.push_child(ArenaPositionedBox { dx: pre_width, dy: 0.0, id: base_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: pre_width + base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_phantom_arena(
    ast: &AstArena,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: inner_w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::Phantom,
    })
}

fn layout_boxed_arena(
    ast: &AstArena,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: inner_w + 2.0 * padding,
        height: inner_h + padding,
        depth: inner_d + padding,
        kind: ArenaBoxKind::Boxed {
            child: inner_id,
            padding,
        },
    })
}

fn layout_color_arena(
    ast: &AstArena,
    color: StrId,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let color_str = ast.str_val(color).to_owned();
    let child_id = layout_node_arena(ast, content, arena, ctx);
    let w = arena.get(child_id).width;
    let h = arena.get(child_id).height;
    let d = arena.get(child_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: w,
        height: h,
        depth: d,
        kind: ArenaBoxKind::Color {
            color: color_str,
            child: child_id,
        },
    })
}

fn layout_cancel_arena(
    ast: &AstArena,
    stroke: StrokeKind,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let w = inner_w;
    let h = inner_h + inner_d;

    let path = match stroke {
        StrokeKind::Forward => format!("M0,{h} L{w},0"),
        StrokeKind::Backward => format!("M0,0 L{w},{h}"),
        StrokeKind::Cross => format!("M0,{h} L{w},0 M0,0 L{w},{h}"),
        StrokeKind::Horizontal => {
            let mid = h / 2.0;
            format!("M0,{mid} L{w},{mid}")
        },
        StrokeKind::Vertical => {
            let mid = w / 2.0;
            format!("M{mid},0 L{mid},{h}")
        },
    };

    let path_id = arena.alloc(ArenaLayoutBox {
        width: w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::SvgPath(path),
    });

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: inner_id });
    arena.push_child(ArenaPositionedBox { dx: 0.0, dy: 0.0, id: path_id });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: inner_w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

fn layout_scaled_arena(
    ast: &AstArena,
    factor: Rational,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;
    let inner_kind = arena.get(inner_id).kind.clone();
    let f = factor.as_f64();

    arena.alloc(ArenaLayoutBox {
        width: inner_w * f,
        height: inner_h * f,
        depth: inner_d * f,
        kind: inner_kind,
    })
}

fn layout_styled_arena(
    ast: &AstArena,
    style: TextStyle,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    use crate::unicode::fonts::styled_char;

    let ids = ast.children(span);
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    let children_start = arena.begin_children();
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for &id in ids {
        let layout_id = match *ast.node(id) {
            CompactNode::Identifier(sid) => {
                let s = ast.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                let styled_italic = matches!(style, TextStyle::Italic | TextStyle::BoldItalic | TextStyle::SansSerifItalic | TextStyle::SansSerifBoldItalic);
                if mapped.chars().count() == 1 {
                    let ch = mapped.chars().next().expect("non-empty checked");
                    alloc_glyph(arena, ch, ctx.size(), styled_italic)
                } else {
                    layout_text_run_arena(&mapped, arena, ctx, styled_italic)
                }
            },
            CompactNode::Number(sid) => {
                let s = ast.str_val(sid);
                let mapped: String = s.chars().map(|c| styled_char(c, style).unwrap_or(c)).collect();
                layout_text_run_arena(&mapped, arena, ctx, false)
            },
            _ => layout_node_arena(ast, id, arena, ctx),
        };
        let b = arena.get(layout_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        let w = b.width;
        arena.push_child(ArenaPositionedBox {
            dx: cursor_x,
            dy: 0.0,
            id: layout_id,
        });
        cursor_x += w;
    }

    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> RenderOptions {
        RenderOptions::default()
    }

    #[test]
    fn layout_simple_number() {
        let expr = MathExpr::inline(vec![MathNode::Number("42".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_fraction_has_depth() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_superscript_raises() {
        let expr = MathExpr::inline(vec![MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Width should be base + sup
        assert!(root.width > 0.5);
    }

    #[test]
    fn empty_row_has_zero_width() {
        let expr = MathExpr::inline(vec![]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert_eq!(root.width, 0.0);
    }

    #[test]
    fn layout_subscript_lowers() {
        let expr = MathExpr::inline(vec![MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.5);
        assert!(root.depth > 0.0, "subscript should add depth");
    }

    #[test]
    fn layout_subsup_combined() {
        let expr = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.5);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0, "subsup should have both height and depth");
    }

    #[test]
    fn layout_fraction_display_wider_than_text() {
        let frac = vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }];
        let expr_d = MathExpr::inline(frac.clone());
        let expr_t = MathExpr::inline(frac);
        let opts = default_options();
        let display = layout_expr(&expr_d, MathStyle::Display, &opts);
        let text = layout_expr(&expr_t, MathStyle::Text, &opts);
        // Display fractions have larger numerator/denominator spacing
        assert!(
            display.height + display.depth >= text.height + text.depth,
            "display fraction should be at least as tall as text fraction"
        );
    }

    #[test]
    fn layout_sqrt_has_positive_dimensions() {
        let expr = MathExpr::inline(vec![MathNode::Sqrt(Box::new(MathNode::Identifier(
            "x".into(),
        )))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_sqrt_with_index() {
        let expr = MathExpr::inline(vec![MathNode::Root {
            index: Box::new(MathNode::Number("3".into())),
            radicand: Box::new(MathNode::Identifier("x".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_fenced_wider_than_content() {
        let content_nodes = vec![MathNode::Identifier("x".into())];
        let content_expr = MathExpr::inline(content_nodes.clone());
        let content_only = layout_expr(&content_expr, MathStyle::Text, &default_options());

        let fenced_nodes = vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        }];
        let fenced_expr = MathExpr::inline(fenced_nodes);
        let fenced = layout_expr(&fenced_expr, MathStyle::Text, &default_options());

        assert!(
            fenced.width > content_only.width,
            "fenced expression should be wider than content alone (delimiters add width)"
        );
    }

    #[test]
    fn layout_array_grid() {
        let expr = MathExpr::inline(vec![MathNode::Array {
            rows: vec![
                vec![
                    vec![MathNode::Identifier("a".into())],
                    vec![MathNode::Identifier("b".into())],
                ],
                vec![
                    vec![MathNode::Identifier("c".into())],
                    vec![MathNode::Identifier("d".into())],
                ],
            ],
            columns: vec![ColumnAlign::Center, ColumnAlign::Center],
            arraystretch: None,
            add_jot: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_nary_display_has_limits() {
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_accent_over_base() {
        let expr = MathExpr::inline(vec![MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '\u{0302}', // combining circumflex
            position: AccentPosition::Above,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_overscript() {
        let expr = MathExpr::inline(vec![MathNode::Overscript {
            base: Box::new(MathNode::Identifier("x".into())),
            over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "→".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_underscript() {
        let expr = MathExpr::inline(vec![MathNode::Underscript {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "_".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_underover() {
        let expr = MathExpr::inline(vec![MathNode::UnderOver {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(MathNode::Number("1".into())),
            over: Box::new(MathNode::Number("2".into())),
            movable_limits: false,
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_phantom_has_dimensions_but_invisible() {
        let content = MathNode::Identifier("x".into());
        let visible = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
                size_scale: 1.0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Phantom(Box::new(content.clone()))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(
            (root.width - visible.width).abs() < 0.01,
            "phantom should have same width as visible content"
        );
        if let BoxKind::HBox { children } = &root.kind {
            let phantom_kind = &children[0].layout.kind;
            assert!(matches!(phantom_kind, BoxKind::Phantom));
        }
    }

    #[test]
    fn layout_boxed_adds_padding() {
        let content = MathNode::Identifier("x".into());
        let inner = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
                size_scale: 1.0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Boxed(Box::new(content.clone()))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Boxed should be wider than inner due to padding
        assert!(
            root.width > inner.width,
            "boxed should add padding to width"
        );
    }

    #[test]
    fn layout_cancel_preserves_dimensions() {
        let expr = MathExpr::inline(vec![MathNode::Cancel {
            stroke: StrokeKind::Forward,
            content: Box::new(MathNode::Identifier("x".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_scaled_doubles_dimensions() {
        let content = MathNode::Identifier("x".into());
        let normal = layout_node(
            &content,
            &LayoutCtx {
                style: MathStyle::Text,
                params: TexParams::default(),
                options: &default_options(),
                depth: 0,
                size_scale: 1.0,
            },
        );
        let expr = MathExpr::inline(vec![MathNode::Scaled {
            factor: Rational::new(2, 1),
            content: Box::new(content.clone()),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Width should be approximately double (within the HBox wrapper)
        if let BoxKind::HBox { children } = &root.kind {
            let scaled = &children[0].layout;
            assert!(
                (scaled.width - normal.width * 2.0).abs() < 0.01,
                "scaled by 2 should double the width"
            );
        }
    }

    #[test]
    fn layout_text_upright() {
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, "abc".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_space_kern() {
        let expr = MathExpr::inline(vec![MathNode::Space(Rational::new(1, 2))]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0, "space should produce width");
    }

    #[test]
    fn layout_identifier_single_char() {
        let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
    }

    #[test]
    fn layout_operator_large_in_display() {
        let expr = MathExpr::inline(vec![MathNode::Operator(OperatorAttr {
            name: "∑".into(),
            limit_style: LimitStyle::Default,
        })]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        assert!(
            display.height >= text.height,
            "large op in display should be at least as tall as in text"
        );
    }

    #[test]
    fn layout_row_with_spacing() {
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("y".into()),
        ]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        // Row with binary op spacing should be wider than just x and y
        let x_expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
        let y_expr = MathExpr::inline(vec![MathNode::Identifier("y".into())]);
        let x_w = layout_expr(&x_expr, MathStyle::Text, &default_options()).width;
        let y_w = layout_expr(&y_expr, MathStyle::Text, &default_options()).width;
        assert!(
            root.width > x_w + y_w,
            "row with operator should include spacing"
        );
    }

    #[test]
    fn layout_prescripts_before_base() {
        let expr = MathExpr::inline(vec![MathNode::PreScripts {
            base: Box::new(MathNode::Identifier("C".into())),
            pre_sub: Some(Box::new(MathNode::Number("6".into()))),
            pre_sup: Some(Box::new(MathNode::Number("12".into()))),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_styled_children() {
        let expr = MathExpr::inline(vec![MathNode::Styled(
            TextStyle::Bold,
            vec![MathNode::Identifier("x".into())],
        )]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn layout_symbol_various_kinds() {
        for kind in [
            SymbolKind::Ordinary,
            SymbolKind::BinaryOp,
            SymbolKind::Relation,
            SymbolKind::Open,
            SymbolKind::Close,
            SymbolKind::Punctuation,
        ] {
            let expr = MathExpr::inline(vec![MathNode::Symbol(kind, "+".into())]);
            let root = layout_expr(&expr, MathStyle::Text, &default_options());
            assert!(root.width > 0.0, "symbol kind {:?} should have width", kind);
        }
    }

    #[test]
    fn layout_deeply_nested_fraction() {
        let inner_frac = MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        };
        let outer_frac = MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(inner_frac),
            den: Box::new(MathNode::Number("3".into())),
        };
        let expr = MathExpr::inline(vec![outer_frac]);
        let root = layout_expr(&expr, MathStyle::Display, &default_options());
        assert!(root.width > 0.0);
        assert!(root.height > 0.0);
        assert!(root.depth > 0.0);
    }

    #[test]
    fn layout_fraction_nobar() {
        let expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::NoBar,
            num: Box::new(MathNode::Number("n".into())),
            den: Box::new(MathNode::Number("k".into())),
        }]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0);
    }

    #[test]
    fn script_style_shrinks() {
        let base_expr = MathExpr::inline(vec![MathNode::Number("2".into())]);
        let opts = default_options();
        let text_box = layout_expr(&base_expr, MathStyle::Text, &opts);
        let script_box = layout_expr(&base_expr, MathStyle::Script, &opts);
        assert!(
            script_box.width < text_box.width,
            "script style should produce smaller glyphs"
        );
    }

    #[test]
    fn large_op_symbol_scales_in_display() {
        // Symbol(LargeOp, "∑") should be larger in display mode than text mode
        let expr = MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        assert!(
            display.height > text.height,
            "LargeOp symbol in display should be taller: display={}, text={}",
            display.height,
            text.height,
        );
        assert!(
            display.width > text.width,
            "LargeOp symbol in display should be wider: display={}, text={}",
            display.width,
            text.width,
        );
    }

    #[test]
    fn subsup_with_large_op_uses_limits_in_display() {
        // SubSup with LargeOp base in display mode should place scripts above/below
        let expr_display = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr_display, MathStyle::Display, &opts);
        // In limits mode, the total height should be larger because sup is above
        // and sub is below the operator, adding to overall vertical extent
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.height > just_op.height,
            "SubSup with limits should be taller than just the op: display={}, op={}",
            display.height,
            just_op.height,
        );
        assert!(
            display.depth > just_op.depth,
            "SubSup with limits should have more depth than just the op: display={}, op={}",
            display.depth,
            just_op.depth,
        );
    }

    #[test]
    fn subscript_with_large_op_uses_limits_in_display() {
        // Subscript with LargeOp base in display mode should place sub below
        let expr = MathExpr::inline(vec![MathNode::Subscript {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.depth > just_op.depth,
            "Subscript limit should add depth below op"
        );
    }

    #[test]
    fn superscript_with_large_op_uses_limits_in_display() {
        // Superscript with LargeOp base in display mode should place sup above
        let expr = MathExpr::inline(vec![MathNode::Superscript {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Display,
            &opts,
        );
        assert!(
            display.height > just_op.height,
            "Superscript limit should add height above op"
        );
    }

    #[test]
    fn subsup_with_large_op_no_limits_in_text() {
        // SubSup with LargeOp base in text mode should NOT use limits
        let expr = MathExpr::inline(vec![MathNode::SubSup {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
            sup: Box::new(MathNode::Identifier("n".into())),
        }]);
        let opts = default_options();
        let text = layout_expr(&expr, MathStyle::Text, &opts);
        let just_op = layout_expr(
            &MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::LargeOp,
                "\u{2211}".into(),
            )]),
            MathStyle::Text,
            &opts,
        );
        // In non-limits mode, the width should be wider (scripts to the side)
        assert!(
            text.width > just_op.width,
            "SubSup in text mode should be wider than just the op (scripts beside, not above/below)"
        );
    }

    #[test]
    fn nary_nolimits_still_scales_in_display() {
        // Nary with SubSup limit style in display mode should still scale the op glyph
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let opts = default_options();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let text_expr = MathExpr::inline(vec![MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let text = layout_expr(&text_expr, MathStyle::Text, &opts);
        assert!(
            display.height > text.height,
            "Nary with SubSup style in display should still be taller (scaled op): display={}, text={}",
            display.height,
            text.height,
        );
    }

    // === Arena layout tests ===

    #[test]
    fn arena_layout_simple_number() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("42", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_superscript() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5, "superscript should have positive width");
    }

    #[test]
    fn arena_layout_subscript() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x_i", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5);
        assert!(b.depth > 0.0, "subscript should add depth");
    }

    #[test]
    fn arena_layout_subsup() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x_i^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.5);
        assert!(b.height > 0.0);
        assert!(b.depth > 0.0);
    }

    #[test]
    fn arena_layout_fraction() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\frac{1}{2}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Display, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.height > 0.0);
        assert!(b.depth > 0.0);
    }

    #[test]
    fn arena_layout_sqrt() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\sqrt{x}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_fenced() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\left(x\\right)", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_nary_display() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\sum_{i=0}^{n} x", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Display, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_overunder() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\overset{a}{b}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_array() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            &config,
        ).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
    }

    #[test]
    fn arena_layout_accent() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("\\hat{x}", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        let root = builder.layout(&mut arena, &result.expr);
        let b = arena.get(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn arena_layout_depth_limit() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let mut latex = String::new();
        for _ in 0..150 {
            latex.push_str("\\frac{");
        }
        latex.push('x');
        for _ in 0..150 {
            latex.push_str("}{1}");
        }
        let config = crate::error::Config::default();
        if let Ok(result) = crate::reader::latex::LatexReader::parse_compact(&latex, &config) {
            let mut arena = LayoutArena::new();
            let opts = default_options();
            let builder = LayoutBuilder::new(MathStyle::Text, &opts);
            // Must not panic or overflow stack
            let _root = builder.layout(&mut arena, &result.expr);
        }
    }

    #[test]
    fn arena_clear_and_reuse() {
        use crate::render::arena::LayoutArena;
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);

        let root1 = builder.layout(&mut arena, &result.expr);
        let w1 = arena.get(root1).width;

        arena.clear();
        let root2 = builder.layout(&mut arena, &result.expr);
        let w2 = arena.get(root2).width;

        assert!((w1 - w2).abs() < f64::EPSILON, "reused arena should produce identical layout");
    }

    // === Arena-heap equivalence tests ===

    /// Helper: verify arena layout produces identical SVG to heap layout for a LaTeX string.
    fn assert_arena_heap_equivalent(latex: &str, style: MathStyle) {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::emit::{emit_svg, emit_svg_arena};

        let config = crate::error::Config::default();
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config)
            .unwrap_or_else(|e| panic!("parse failed for {latex:?}: {e}"));
        let options = default_options();

        let heap_root = layout_expr_c(&result.expr, style, &options);
        let heap_svg = emit_svg(&heap_root, &options);

        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(style, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_svg = emit_svg_arena(&arena, arena_root, &options);

        assert_eq!(heap_svg, arena_svg, "mismatch for {latex:?}");
    }

    #[test]
    fn equivalence_simple() {
        assert_arena_heap_equivalent("x^2 + 1", MathStyle::Text);
    }

    #[test]
    fn equivalence_quadratic() {
        assert_arena_heap_equivalent(
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            MathStyle::Display,
        );
    }

    #[test]
    fn equivalence_nested_frac() {
        assert_arena_heap_equivalent("\\frac{\\frac{a}{b}}{\\frac{c}{d}}", MathStyle::Display);
    }

    #[test]
    fn equivalence_sum() {
        assert_arena_heap_equivalent("\\sum_{i=0}^{n} \\frac{1}{i^2}", MathStyle::Display);
    }

    #[test]
    fn equivalence_sqrt_with_index() {
        assert_arena_heap_equivalent("\\sqrt[3]{x}", MathStyle::Text);
    }

    #[test]
    fn equivalence_fenced() {
        assert_arena_heap_equivalent("\\left(\\frac{a}{b}\\right)", MathStyle::Display);
    }

    #[test]
    fn equivalence_matrix() {
        assert_arena_heap_equivalent(
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            MathStyle::Text,
        );
    }

    #[test]
    fn equivalence_accent() {
        assert_arena_heap_equivalent("\\hat{x} + \\vec{v}", MathStyle::Text);
    }

    #[test]
    fn equivalence_text() {
        assert_arena_heap_equivalent("\\text{hello}", MathStyle::Text);
    }

    #[test]
    fn equivalence_navier_stokes() {
        assert_arena_heap_equivalent(
            "\\rho\\left(\\frac{\\partial \\mathbf{u}}{\\partial t} + \\mathbf{u} \\cdot \\nabla \\mathbf{u}\\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{u} + \\mathbf{f}",
            MathStyle::Display,
        );
    }

    // =====================================================================
    // Aligned / multi-column alignment tests
    // =====================================================================

    /// Simple 2-column aligned environment: columns should be laid out
    /// side by side rather than stacked vertically.
    #[test]
    fn aligned_two_column_has_grid_layout() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        assert!(
            root.width > 1.5,
            "aligned block should be wider than a single column, got {}",
            root.width
        );
    }

    /// Multi-pair aligned environment: 4 columns (rl rl).
    #[test]
    fn aligned_four_column_has_grid_layout() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();

        fn find_num_cols(arena: &AstArena, id: crate::ast::compact::NodeId) -> Option<u16> {
            match arena.node(id) {
                crate::ast::compact::CompactNode::Array { num_cols, .. } => Some(*num_cols),
                crate::ast::compact::CompactNode::Row(span) => {
                    for &child_id in arena.children(*span) {
                        if let Some(nc) = find_num_cols(arena, child_id) {
                            return Some(nc);
                        }
                    }
                    None
                },
                _ => None,
            }
        }
        let nc = {
            let arena = &result.expr.arena;
            let mut found = None;
            for &root_id in arena.children(result.expr.roots) {
                found = find_num_cols(arena, root_id);
                if found.is_some() {
                    break;
                }
            }
            found.expect("should find Array node")
        };
        assert_eq!(nc, 4, "aligned with 3 &'s per row should produce 4 columns");

        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        assert!(
            root.width > 3.0,
            "4-column aligned block should be wide, got {}",
            root.width
        );
    }

    #[test]
    fn equivalence_aligned() {
        assert_arena_heap_equivalent(
            r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}",
            MathStyle::Text,
        );
    }

    #[test]
    fn equivalence_aligned_multi_pair() {
        assert_arena_heap_equivalent(
            r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}",
            MathStyle::Text,
        );
    }

    #[test]
    fn alignedat_parses_and_renders() {
        use crate::reader::Reader;
        let config = crate::error::Config::default();
        let latex = r"\begin{alignedat}{2} x &= 1 & \quad y &= 2 \end{alignedat}";
        let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
        let opts = default_options();
        let root = layout_expr_c(&result.expr, MathStyle::Text, &opts);
        assert!(root.width > 1.0, "alignedat should render with meaningful width");
    }

    // ── Tests for P2 bug fixes ──────────────────────────────────────────

    #[test]
    fn delimiter_scales_with_content_height() {
        let small_fenced = MathExpr::inline(vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        }]);
        let small = layout_expr(&small_fenced, MathStyle::Text, &default_options());

        let tall_fenced = MathExpr::inline(vec![MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(MathNode::Superscript {
                    base: Box::new(MathNode::Identifier("x".into())),
                    sup: Box::new(MathNode::Number("2".into())),
                }),
                den: Box::new(MathNode::Identifier("y".into())),
            })],
        }]);
        let tall = layout_expr(&tall_fenced, MathStyle::Display, &default_options());

        assert!(
            tall.width > small.width,
            "fenced with fraction should be wider than fenced with single char: {} vs {}",
            tall.width, small.width
        );
    }

    #[test]
    fn fraction_bar_visible_at_scriptscript() {
        let frac = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        }]);
        let opts = default_options();
        let root = layout_expr(&frac, MathStyle::ScriptScript, &opts);

        fn find_rule_thickness(layout: &LayoutBox) -> Option<f64> {
            match &layout.kind {
                BoxKind::Rule { thickness } => Some(*thickness),
                BoxKind::HBox { children } | BoxKind::VBox { children } => {
                    for child in children {
                        if let Some(t) = find_rule_thickness(&child.layout) {
                            return Some(t);
                        }
                    }
                    None
                },
                _ => None,
            }
        }

        let thickness = find_rule_thickness(&root)
            .expect("fraction should contain a rule bar");
        // At ScriptScript (size=0.5), rule_thickness = 0.049 (cmex7), so
        // bar = 0.049 * 0.5 = 0.0245em.  Bars scale naturally with style.
        let expected = 0.049 * 0.5; // xi_rule[scriptscript] * size
        assert!(
            (thickness - expected).abs() < 0.001,
            "bar thickness at ScriptScript should be ~{}, got {}",
            expected, thickness
        );
    }

    #[test]
    fn cdots_raised_to_axis() {
        let cdots_expr = MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::Ordinary,
            "\u{22EF}".into(),
        )]);
        let opts = default_options();
        let root = layout_expr(&cdots_expr, MathStyle::Text, &opts);

        let ldots_expr = MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::Ordinary,
            ".".into(),
        )]);
        let ldots = layout_expr(&ldots_expr, MathStyle::Text, &opts);

        assert!(
            root.height > ldots.height,
            "cdots should be taller than baseline dots: {} vs {}",
            root.height, ldots.height
        );
    }

    #[test]
    fn fraction_has_side_padding() {
        let num_expr = MathExpr::inline(vec![MathNode::Identifier("a".into())]);
        let den_expr = MathExpr::inline(vec![MathNode::Identifier("b".into())]);
        let opts = default_options();
        let num_w = layout_expr(&num_expr, MathStyle::Text, &opts).width;
        let den_w = layout_expr(&den_expr, MathStyle::Text, &opts).width;
        let max_content_w = num_w.max(den_w);

        let frac_expr = MathExpr::inline(vec![MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Identifier("a".into())),
            den: Box::new(MathNode::Identifier("b".into())),
        }]);
        let frac = layout_expr(&frac_expr, MathStyle::Text, &opts);

        assert!(
            frac.width > max_content_w,
            "fraction should include side padding: {} vs {}",
            frac.width, max_content_w
        );
    }

    #[test]
    fn equivalence_cdots() {
        assert_arena_heap_equivalent("a \\cdots b", MathStyle::Text);
    }

    #[test]
    fn equivalence_fenced_fraction() {
        assert_arena_heap_equivalent(
            "\\left(\\frac{x^2}{y}\\right)",
            MathStyle::Display,
        );
    }
}
