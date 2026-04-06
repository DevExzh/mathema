//! SVG path-based delimiter rendering.
//!
//! Generates SVG `<path>` data for common delimiters (parentheses, brackets,
//! braces, vertical bars, floor/ceiling, angle brackets) that scale vertically
//! without becoming disproportionately wide -- unlike font-size scaling which
//! scales uniformly in both dimensions.
//!
//! All paths are in a coordinate system where (0,0) is top-left and the path
//! extends to (width, height), measured in **em units** (not pixels).

use std::fmt::Write;

/// Threshold: use SVG paths only when the delimiter is significantly taller
/// than normal text size. Below this ratio (target_height / size), we fall
/// back to text rendering which looks better at small sizes.
const PATH_THRESHOLD: f64 = 1.2;

/// Information about a generated delimiter path.
pub struct DelimiterPath {
    /// The SVG path data string (in em-unit coordinates).
    pub path_data: String,
    /// Width of the delimiter in em units.
    pub width: f64,
    /// Stroke width for rendering (in em units).
    pub stroke_width: f64,
}

/// Check whether a delimiter character should use SVG path rendering
/// at the given target height and base size.
///
/// Returns `true` if the delimiter is tall enough to benefit from path
/// rendering (i.e. target_height >= PATH_THRESHOLD * size).
#[inline]
pub fn should_use_path(ch: char, target_height: f64, size: f64) -> bool {
    has_path(ch) && target_height >= PATH_THRESHOLD * size
}

/// Check whether we have a custom SVG path for a given delimiter character.
#[inline]
fn has_path(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')' | '[' | ']' | '{' | '}' | '|' | '‖' | '∥'
            | '⌊' | '⌋' | '⌈' | '⌉'
            | '⟨' | '⟩' | '〈' | '〉'
    )
}

/// Generate SVG path data for a delimiter at the given height.
///
/// `height` is in em units (the total height of the delimiter).
/// Returns `None` if no custom path is available for the character.
pub fn delimiter_path(ch: char, height: f64, size: f64) -> Option<DelimiterPath> {
    if !should_use_path(ch, height, size) {
        return None;
    }
    let h = height;
    let sw = 0.04 * (h / 1.0).sqrt().min(1.5); // stroke scales gently with height
    match ch {
        '(' => Some(left_paren(h, sw)),
        ')' => Some(right_paren(h, sw)),
        '[' => Some(left_bracket(h, sw)),
        ']' => Some(right_bracket(h, sw)),
        '{' => Some(left_brace(h, sw)),
        '}' => Some(right_brace(h, sw)),
        '|' => Some(vertical_bar(h, sw)),
        '‖' | '∥' => Some(double_vertical_bar(h, sw)),
        '⌊' => Some(left_floor(h, sw)),
        '⌋' => Some(right_floor(h, sw)),
        '⌈' => Some(left_ceil(h, sw)),
        '⌉' => Some(right_ceil(h, sw)),
        '⟨' | '〈' => Some(left_angle(h, sw)),
        '⟩' | '〉' => Some(right_angle(h, sw)),
        _ => None,
    }
}

/// Compute the delimiter width for path-based rendering.
///
/// For path-based delimiters, width is controlled and does not scale
/// proportionally with height the way font-size scaling does.
pub fn delimiter_path_width(ch: char, height: f64, _size: f64) -> f64 {
    match ch {
        '(' | ')' => paren_width(height),
        '[' | ']' => bracket_width(height),
        '{' | '}' => brace_width(height),
        '|' => bar_width(height),
        '‖' | '∥' => double_bar_width(height),
        '⌊' | '⌋' | '⌈' | '⌉' => bracket_width(height),
        '⟨' | '⟩' | '〈' | '〉' => angle_width(height),
        _ => 0.389, // fallback: base delimiter width
    }
}

// ── Width helpers ───────────────────────────────────────────────────────

/// Parenthesis width: thin arc that caps out at a reasonable width.
#[inline]
fn paren_width(h: f64) -> f64 {
    // Proportional to height but capped so tall parens stay thin
    (0.178 * h).clamp(0.15, 0.40)
}

#[inline]
fn bracket_width(h: f64) -> f64 {
    (0.12 * h).clamp(0.12, 0.30)
}

#[inline]
fn brace_width(h: f64) -> f64 {
    (0.18 * h).clamp(0.18, 0.42)
}

#[inline]
fn bar_width(_h: f64) -> f64 {
    0.10
}

#[inline]
fn double_bar_width(_h: f64) -> f64 {
    0.22
}

#[inline]
fn angle_width(h: f64) -> f64 {
    (0.15 * h).clamp(0.12, 0.35)
}

// ── Path generators ────────────────────────────────────────────────────
//
// All paths use absolute coordinates in em units.
// (0,0) = top-left; (width, height) = bottom-right.

fn left_paren(h: f64, _sw: f64) -> DelimiterPath {
    let w = paren_width(h);
    let mut d = String::with_capacity(256);
    // Filled crescent shape: outer arc curves further left, inner arc curves
    // less, creating a shape that is thicker in the middle (~0.04em) and tapers
    // to ~0.015em at the tips -- mimicking KaTeX's font-traced glyphs.
    let tip = 0.015; // taper at endpoints
    let outer_cx = -w * 0.1; // outer control-point x (curves further left)
    let inner_cx = w * 0.15; // inner control-point x (curves less)

    // Outer curve: top-right → bottom-right, curving left
    let _ = write!(
        d,
        "F:M{w:.4},{start:.4} C{ocx:.4},{ocy1:.4} {ocx:.4},{ocy2:.4} {w:.4},{h:.4}",
        w = w,
        start = 0.0,
        ocx = outer_cx,
        ocy1 = h * 0.25,
        ocy2 = h * 0.75,
        h = h,
    );
    // Line to inner bottom endpoint (slightly inset)
    let _ = write!(
        d,
        " L{ix:.4},{iy:.4}",
        ix = w - tip,
        iy = h - tip,
    );
    // Inner curve: bottom → top, curving left but less deeply (reverse direction)
    let _ = write!(
        d,
        " C{icx:.4},{icy2:.4} {icx:.4},{icy1:.4} {ix:.4},{iy:.4} Z",
        icx = inner_cx,
        icy2 = h * 0.72,
        icy1 = h * 0.28,
        ix = w - tip,
        iy = tip,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: 0.0,
    }
}

fn right_paren(h: f64, _sw: f64) -> DelimiterPath {
    let w = paren_width(h);
    let mut d = String::with_capacity(256);
    // Filled crescent, mirror of left paren: arc from top-left to bottom-left,
    // curving right, with tapering tips.
    let tip = 0.015;
    let outer_cx = w * 1.1; // outer control-point x (curves further right)
    let inner_cx = w * 0.85; // inner control-point x (curves less)

    // Outer curve: top-left → bottom-left, curving right
    let _ = write!(
        d,
        "F:M{start:.4},{top:.4} C{ocx:.4},{ocy1:.4} {ocx:.4},{ocy2:.4} {start:.4},{bot:.4}",
        start = 0.0,
        top = 0.0,
        ocx = outer_cx,
        ocy1 = h * 0.25,
        ocy2 = h * 0.75,
        bot = h,
    );
    // Line to inner bottom endpoint (slightly inset)
    let _ = write!(
        d,
        " L{ix:.4},{iy:.4}",
        ix = tip,
        iy = h - tip,
    );
    // Inner curve: bottom → top, curving right but less deeply (reverse direction)
    let _ = write!(
        d,
        " C{icx:.4},{icy2:.4} {icx:.4},{icy1:.4} {ix:.4},{iy:.4} Z",
        icx = inner_cx,
        icy2 = h * 0.72,
        icy1 = h * 0.28,
        ix = tip,
        iy = tip,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: 0.0,
    }
}

fn left_bracket(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(80);
    // ┌ shape: horizontal top, vertical left, horizontal bottom
    let _ = write!(
        d,
        "M{w:.4},0 L0,0 L0,{h:.4} L{w:.4},{h:.4}",
        w = w,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn right_bracket(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(80);
    // ┐ shape: horizontal top, vertical right, horizontal bottom
    let _ = write!(
        d,
        "M0,0 L{w:.4},0 L{w:.4},{h:.4} L0,{h:.4}",
        w = w,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn left_brace(h: f64, _sw: f64) -> DelimiterPath {
    let w = brace_width(h);
    let mut d = String::with_capacity(512);
    // Filled curly brace { with a sharp middle bump pointing left.
    // Outer contour traces the visible edge; inner contour is slightly inset.
    let mid = h * 0.5;
    let tip = 0.015; // taper at top/bottom tips
    let thick = 0.04; // thickness at the straight portions
    let bump_out = w * 0.05; // how far the outer midpoint extends left of 0
    let bump_in = bump_out + thick; // inner midpoint (further right)

    // -- Outer contour (clockwise: top-right → mid-left → bottom-right) --
    let _ = write!(
        d,
        "F:M{tr:.4},0 C{c1x:.4},{c1y:.4} {c2x:.4},{c2y:.4} {mx:.4},{mid:.4}",
        tr = w,
        c1x = w * 0.35,
        c1y = 0.0,
        c2x = bump_out,
        c2y = mid * 0.45,
        mx = bump_out,
        mid = mid,
    );
    let _ = write!(
        d,
        " C{c3x:.4},{c3y:.4} {c4x:.4},{c4y:.4} {br:.4},{h:.4}",
        c3x = bump_out,
        c3y = mid + (h - mid) * 0.55,
        c4x = w * 0.35,
        c4y = h,
        br = w,
        h = h,
    );

    // -- Inner contour (counter-clockwise: bottom-right inset → mid-left inset → top-right inset) --
    let _ = write!(
        d,
        " L{br:.4},{hy:.4}",
        br = w - tip,
        hy = h - tip,
    );
    let _ = write!(
        d,
        " C{c5x:.4},{c5y:.4} {c6x:.4},{c6y:.4} {imx:.4},{mid:.4}",
        c5x = w * 0.40,
        c5y = h - tip,
        c6x = bump_in,
        c6y = mid + (h - mid) * 0.55,
        imx = bump_in,
        mid = mid,
    );
    let _ = write!(
        d,
        " C{c7x:.4},{c7y:.4} {c8x:.4},{c8y:.4} {it:.4},{ty:.4} Z",
        c7x = bump_in,
        c7y = mid * 0.45,
        c8x = w * 0.40,
        c8y = tip,
        it = w - tip,
        ty = tip,
    );

    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: 0.0,
    }
}

fn right_brace(h: f64, _sw: f64) -> DelimiterPath {
    let w = brace_width(h);
    let mut d = String::with_capacity(512);
    // Filled curly brace } — mirror of left brace, middle bump points right.
    let mid = h * 0.5;
    let tip = 0.015;
    let thick = 0.04;
    let bump_out = w * 0.95; // outer midpoint x (right of w)
    let bump_in = bump_out - thick; // inner midpoint x

    // -- Outer contour (clockwise: top-left → mid-right → bottom-left) --
    let _ = write!(
        d,
        "F:M0,0 C{c1x:.4},{c1y:.4} {c2x:.4},{c2y:.4} {mx:.4},{mid:.4}",
        c1x = w * 0.65,
        c1y = 0.0,
        c2x = bump_out,
        c2y = mid * 0.45,
        mx = bump_out,
        mid = mid,
    );
    let _ = write!(
        d,
        " C{c3x:.4},{c3y:.4} {c4x:.4},{c4y:.4} 0,{h:.4}",
        c3x = bump_out,
        c3y = mid + (h - mid) * 0.55,
        c4x = w * 0.65,
        c4y = h,
        h = h,
    );

    // -- Inner contour (counter-clockwise: bottom-left inset → mid-right inset → top-left inset) --
    let _ = write!(
        d,
        " L{bl:.4},{hy:.4}",
        bl = tip,
        hy = h - tip,
    );
    let _ = write!(
        d,
        " C{c5x:.4},{c5y:.4} {c6x:.4},{c6y:.4} {imx:.4},{mid:.4}",
        c5x = w * 0.60,
        c5y = h - tip,
        c6x = bump_in,
        c6y = mid + (h - mid) * 0.55,
        imx = bump_in,
        mid = mid,
    );
    let _ = write!(
        d,
        " C{c7x:.4},{c7y:.4} {c8x:.4},{c8y:.4} {it:.4},{ty:.4} Z",
        c7x = bump_in,
        c7y = mid * 0.45,
        c8x = w * 0.60,
        c8y = tip,
        it = tip,
        ty = tip,
    );

    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: 0.0,
    }
}

fn vertical_bar(h: f64, sw: f64) -> DelimiterPath {
    let w = bar_width(h);
    let mut d = String::with_capacity(40);
    let x = w * 0.5;
    let _ = write!(d, "M{x:.4},0 L{x:.4},{h:.4}", x = x, h = h);
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn double_vertical_bar(h: f64, sw: f64) -> DelimiterPath {
    let w = double_bar_width(h);
    let mut d = String::with_capacity(80);
    let x1 = w * 0.28;
    let x2 = w * 0.72;
    let _ = write!(
        d,
        "M{x1:.4},0 L{x1:.4},{h:.4} M{x2:.4},0 L{x2:.4},{h:.4}",
        x1 = x1,
        x2 = x2,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn left_floor(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(60);
    // └ shape: vertical left side + horizontal bottom
    let _ = write!(d, "M0,0 L0,{h:.4} L{w:.4},{h:.4}", h = h, w = w);
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn right_floor(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(60);
    // ┘ shape: vertical right side + horizontal bottom
    let _ = write!(
        d,
        "M{w:.4},0 L{w:.4},{h:.4} L0,{h:.4}",
        w = w,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn left_ceil(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(60);
    // ┌ shape: horizontal top + vertical left side
    let _ = write!(d, "M{w:.4},0 L0,0 L0,{h:.4}", w = w, h = h);
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn right_ceil(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    let mut d = String::with_capacity(60);
    // ┐ shape: horizontal top + vertical right side
    let _ = write!(d, "M0,0 L{w:.4},0 L{w:.4},{h:.4}", w = w, h = h);
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn left_angle(h: f64, sw: f64) -> DelimiterPath {
    let w = angle_width(h);
    let mut d = String::with_capacity(60);
    let mid = h * 0.5;
    // V-shape pointing left: top-right → middle-left → bottom-right
    let _ = write!(
        d,
        "M{w:.4},0 L0,{mid:.4} L{w:.4},{h:.4}",
        w = w,
        mid = mid,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

fn right_angle(h: f64, sw: f64) -> DelimiterPath {
    let w = angle_width(h);
    let mut d = String::with_capacity(60);
    let mid = h * 0.5;
    // V-shape pointing right: top-left → middle-right → bottom-left
    let _ = write!(
        d,
        "M0,0 L{w:.4},{mid:.4} L0,{h:.4}",
        w = w,
        mid = mid,
        h = h,
    );
    DelimiterPath {
        path_data: d,
        width: w,
        stroke_width: sw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_use_path_respects_threshold() {
        // At normal text size (size=1.0), a delimiter with target_height 0.9
        // is below the threshold and should use text.
        assert!(!should_use_path('(', 0.9, 1.0));
        // At 1.5x, it should use path rendering.
        assert!(should_use_path('(', 1.5, 1.0));
    }

    #[test]
    fn unknown_chars_return_none() {
        assert!(delimiter_path('?', 2.0, 1.0).is_none());
        assert!(delimiter_path('α', 2.0, 1.0).is_none());
    }

    #[test]
    fn known_chars_return_some() {
        for ch in ['(', ')', '[', ']', '{', '}', '|', '⟨', '⟩', '⌊', '⌋', '⌈', '⌉'] {
            let result = delimiter_path(ch, 2.0, 1.0);
            assert!(result.is_some(), "expected path for '{ch}'");
            let dp = result.unwrap();
            assert!(!dp.path_data.is_empty());
            assert!(dp.width > 0.0);
            // Filled paths (F: prefix) have stroke_width 0.0; stroked paths > 0.0
            if dp.path_data.starts_with("F:") {
                assert!(
                    dp.stroke_width == 0.0,
                    "filled path for '{ch}' should have stroke_width 0.0"
                );
            } else {
                assert!(dp.stroke_width > 0.0);
            }
        }
    }

    #[test]
    fn paren_width_is_capped() {
        // Very tall parens should not become extremely wide
        let w_small = paren_width(1.0);
        let w_large = paren_width(10.0);
        assert!(w_large <= 0.40, "paren width should be capped");
        assert!(w_large > w_small, "taller paren should be wider (up to cap)");
    }

    #[test]
    fn path_data_starts_with_move() {
        for ch in ['(', ')', '[', ']', '{', '}', '|'] {
            let dp = delimiter_path(ch, 2.0, 1.0).unwrap();
            // Filled paths start with "F:M", stroked paths start with "M"
            let starts_ok =
                dp.path_data.starts_with('M') || dp.path_data.starts_with("F:M");
            assert!(
                starts_ok,
                "path for '{ch}' should start with M or F:M, got: {}",
                &dp.path_data[..10.min(dp.path_data.len())]
            );
        }
    }

    #[test]
    fn double_bar_has_two_subpaths() {
        let dp = delimiter_path('‖', 2.0, 1.0).unwrap();
        let m_count = dp.path_data.matches('M').count();
        assert_eq!(m_count, 2, "double bar should have 2 M commands");
    }
}
