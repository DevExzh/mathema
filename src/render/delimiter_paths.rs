//! SVG path-based delimiter rendering.
//!
//! Generates SVG `<path>` data for common delimiters (parentheses, brackets,
//! braces, vertical bars, floor/ceiling, angle brackets) that scale vertically
//! without becoming disproportionately wide -- unlike font-size scaling which
//! scales uniformly in both dimensions.
//!
//! All paths are in a coordinate system where (0,0) is top-left and the path
//! extends to (width, height), measured in **em units** (not pixels).

use crate::render::path_writer::PathWriter;

/// Threshold: use SVG paths only when the delimiter is significantly taller
/// than normal text size. Below this ratio (target_height / size), we fall
/// back to text rendering which looks better at small sizes.
const PATH_THRESHOLD: f64 = 0.5;

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
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '|'
            | '‖'
            | '∥'
            | '⌊'
            | '⌋'
            | '⌈'
            | '⌉'
            | '⟨'
            | '⟩'
            | '〈'
            | '〉'
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
    Some(match ch {
        '(' => left_paren(h, sw),
        ')' => right_paren(h, sw),
        '[' => left_bracket(h, sw),
        ']' => right_bracket(h, sw),
        '{' => left_brace(h, sw),
        '}' => right_brace(h, sw),
        '|' => vertical_bar(h, sw),
        '‖' | '∥' => double_vertical_bar(h, sw),
        '⌊' => left_floor(h, sw),
        '⌋' => right_floor(h, sw),
        '⌈' => left_ceil(h, sw),
        '⌉' => right_ceil(h, sw),
        '⟨' | '〈' => left_angle(h, sw),
        '⟩' | '〉' => right_angle(h, sw),
        _ => return None,
    })
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
    (0.25 * h).clamp(0.30, 0.55)
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
    // Filled crescent shape: outer arc curves further left, inner arc curves
    // less, creating a shape that is thicker in the middle (~0.04em) and tapers
    // to ~0.015em at the tips -- mimicking KaTeX's font-traced glyphs.
    let tip = 0.015; // taper at endpoints
    let outer_cx = -w * 0.1; // outer control-point x (curves further left)
    let inner_cx = w * 0.15; // inner control-point x (curves less)

    // Outer curve: top-right → bottom-right, curving left
    let mut pw = PathWriter::with_prefix("F:");
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'C');
    pw.push_f64(outer_cx);
    pw.push_f64(h * 0.25);
    pw.push_f64(outer_cx);
    pw.push_f64(h * 0.75);
    pw.push_f64(w);
    pw.push_f64(h);
    // Line to inner bottom endpoint (slightly inset)
    pw.cmd(b'L');
    pw.push_f64(w - tip);
    pw.push_f64(h - tip);
    // Inner curve: bottom → top, curving left but less deeply (reverse direction)
    pw.cmd(b'C');
    pw.push_f64(inner_cx);
    pw.push_f64(h * 0.72);
    pw.push_f64(inner_cx);
    pw.push_f64(h * 0.28);
    pw.push_f64(w - tip);
    pw.push_f64(tip);
    pw.close();
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: 0.0,
    }
}

fn right_paren(h: f64, _sw: f64) -> DelimiterPath {
    let w = paren_width(h);
    // Filled crescent, mirror of left paren: arc from top-left to bottom-left,
    // curving right, with tapering tips.
    let tip = 0.015;
    let outer_cx = w * 1.1; // outer control-point x (curves further right)
    let inner_cx = w * 0.85; // inner control-point x (curves less)

    // Outer curve: top-left → bottom-left, curving right
    let mut pw = PathWriter::with_prefix("F:");
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'C');
    pw.push_f64(outer_cx);
    pw.push_f64(h * 0.25);
    pw.push_f64(outer_cx);
    pw.push_f64(h * 0.75);
    pw.push_f64(0.0);
    pw.push_f64(h);
    // Line to inner bottom endpoint (slightly inset)
    pw.cmd(b'L');
    pw.push_f64(tip);
    pw.push_f64(h - tip);
    // Inner curve: bottom → top, curving right but less deeply (reverse direction)
    pw.cmd(b'C');
    pw.push_f64(inner_cx);
    pw.push_f64(h * 0.72);
    pw.push_f64(inner_cx);
    pw.push_f64(h * 0.28);
    pw.push_f64(tip);
    pw.push_f64(tip);
    pw.close();
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: 0.0,
    }
}

fn left_bracket(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // ┌ shape: horizontal top, vertical left, horizontal bottom
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.push_f64(h);
    pw.push_f64(w);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn right_bracket(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // ┐ shape: horizontal top, vertical right, horizontal bottom
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.push_f64(w);
    pw.push_f64(h);
    pw.push_f64(0.0);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn left_brace(h: f64, _sw: f64) -> DelimiterPath {
    let w = brace_width(h);
    // Filled curly brace { — outer + inner contour approach.
    //
    // Shape anatomy (left brace):
    //   top hook: sharp serif-like turn from (w, 0) inward to the arm
    //   upper arm: nearly vertical descent to middle tip
    //   middle tip: pronounced V-shaped point extending to x ≈ 0
    //   lower arm: nearly vertical ascent from middle tip
    //   bottom hook: sharp serif-like turn outward to (w, h)
    let mid = h * 0.5;
    // Stroke thickness. Scale modestly with height so tall braces (e.g. the
    // `cases` brace) carry the heavier weight MathJax uses, while small inline
    // braces keep their original 0.065em weight.
    let thick = (h * 0.045).clamp(0.065, 0.12).min(w * 0.30); // stroke thickness

    // Where the vertical arm runs
    let arm = w * 0.40;
    // Hook height (how tall the top/bottom curves are) — taller for sharper hooks
    let hk = (h * 0.18).clamp(0.10, 0.35);
    // Middle tip x (extends to left edge)
    let tip = 0.0_f64;
    // Endpoint taper — visible thickness at hook endpoints
    let end_gap = (thick * 0.6).max(0.03);

    // V-tip parameters: control points offset from (tip, mid) to create angled
    // approach/departure, producing a sharp V-shape at center
    let tip_dx = w * 0.12;
    let tip_dy = h * 0.06;

    let mut pw = PathWriter::with_prefix("F:");
    // === Outer contour (clockwise) ===
    // Segment 1: top hook — (w, 0) → (arm, hk)
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'C');
    pw.push_f64(w * 0.45);
    pw.push_f64(0.0);
    pw.push_f64(arm);
    pw.push_f64(hk * 0.08);
    pw.push_f64(arm);
    pw.push_f64(hk);
    // Segment 2: upper arm → middle tip — (arm, hk) → (tip, mid)
    pw.cmd(b'C');
    pw.push_f64(arm);
    pw.push_f64(mid * 0.40);
    pw.push_f64(tip + tip_dx);
    pw.push_f64(mid - tip_dy);
    pw.push_f64(tip);
    pw.push_f64(mid);
    // Segment 3: middle tip → lower arm — (tip, mid) → (arm, h - hk)
    pw.cmd(b'C');
    pw.push_f64(tip + tip_dx);
    pw.push_f64(mid + tip_dy);
    pw.push_f64(arm);
    pw.push_f64(mid + (h - mid) * 0.60);
    pw.push_f64(arm);
    pw.push_f64(h - hk);
    // Segment 4: bottom hook — (arm, h - hk) → (w, h)
    pw.cmd(b'C');
    pw.push_f64(arm);
    pw.push_f64(h - hk * 0.08);
    pw.push_f64(w * 0.45);
    pw.push_f64(h);
    pw.push_f64(w);
    pw.push_f64(h);

    // === Inner contour (counter-clockwise) — offset inward by `thick` ===
    let iarm = arm + thick;
    let itip = tip + thick;
    let wi = w - end_gap;
    let eg = end_gap;
    let itip_dx = tip_dx.max(thick * 0.5); // ensure inner V control offset is meaningful

    // Segment 4i: bottom hook inner — (wi, h-eg) → (iarm, h-hk)
    pw.cmd(b'L');
    pw.push_f64(wi);
    pw.push_f64(h - eg);
    pw.cmd(b'C');
    pw.push_f64(w * 0.50);
    pw.push_f64(h - eg);
    pw.push_f64(iarm);
    pw.push_f64(h - hk * 0.12);
    pw.push_f64(iarm);
    pw.push_f64(h - hk);
    // Segment 3i: lower arm → tip inner — (iarm, h-hk) → (itip, mid)
    pw.cmd(b'C');
    pw.push_f64(iarm);
    pw.push_f64(mid + (h - mid) * 0.60);
    pw.push_f64(itip + itip_dx);
    pw.push_f64(mid + tip_dy);
    pw.push_f64(itip);
    pw.push_f64(mid);
    // Segment 2i: tip → upper arm inner — (itip, mid) → (iarm, hk)
    pw.cmd(b'C');
    pw.push_f64(itip + itip_dx);
    pw.push_f64(mid - tip_dy);
    pw.push_f64(iarm);
    pw.push_f64(mid * 0.40);
    pw.push_f64(iarm);
    pw.push_f64(hk);
    // Segment 1i: top hook inner — (iarm, hk) → (wi, eg)
    pw.cmd(b'C');
    pw.push_f64(iarm);
    pw.push_f64(hk * 0.12);
    pw.push_f64(w * 0.50);
    pw.push_f64(eg);
    pw.push_f64(wi);
    pw.push_f64(eg);
    pw.close();

    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: 0.0,
    }
}

fn right_brace(h: f64, _sw: f64) -> DelimiterPath {
    let w = brace_width(h);
    // Filled curly brace } — mirror of left brace, middle tip points right.
    //
    // Shape anatomy (right brace):
    //   top hook: sharp serif-like turn from (0, 0) inward to the arm
    //   upper arm: nearly vertical descent to middle tip
    //   middle tip: pronounced V-shaped point extending to x ≈ w
    //   lower arm: nearly vertical ascent from middle tip
    //   bottom hook: sharp serif-like turn outward to (0, h)
    let mid = h * 0.5;
    let thick = (h * 0.045).clamp(0.065, 0.12).min(w * 0.30);

    let arm = w * 0.60; // arm x position (mirrored from left brace's 0.40)
    let hk = (h * 0.18).clamp(0.10, 0.35);
    let tip = w; // tip extends to right edge
    // Endpoint taper — visible thickness at hook endpoints
    let end_gap = (thick * 0.6).max(0.03);

    // V-tip parameters (mirrored)
    let tip_dx = w * 0.12;
    let tip_dy = h * 0.06;

    let mut pw = PathWriter::with_prefix("F:");
    // === Outer contour (clockwise) ===
    // Segment 1: top hook — (0, 0) → (arm, hk)
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'C');
    pw.push_f64(w * 0.55);
    pw.push_f64(0.0);
    pw.push_f64(arm);
    pw.push_f64(hk * 0.08);
    pw.push_f64(arm);
    pw.push_f64(hk);
    // Segment 2: upper arm → middle tip — (arm, hk) → (tip, mid)
    pw.cmd(b'C');
    pw.push_f64(arm);
    pw.push_f64(mid * 0.40);
    pw.push_f64(tip - tip_dx);
    pw.push_f64(mid - tip_dy);
    pw.push_f64(tip);
    pw.push_f64(mid);
    // Segment 3: middle tip → lower arm — (tip, mid) → (arm, h - hk)
    pw.cmd(b'C');
    pw.push_f64(tip - tip_dx);
    pw.push_f64(mid + tip_dy);
    pw.push_f64(arm);
    pw.push_f64(mid + (h - mid) * 0.60);
    pw.push_f64(arm);
    pw.push_f64(h - hk);
    // Segment 4: bottom hook — (arm, h - hk) → (0, h)
    pw.cmd(b'C');
    pw.push_f64(arm);
    pw.push_f64(h - hk * 0.08);
    pw.push_f64(w * 0.55);
    pw.push_f64(h);
    pw.push_f64(0.0);
    pw.push_f64(h);

    // === Inner contour (counter-clockwise) ===
    let iarm = arm - thick;
    let itip = tip - thick;
    let eg = end_gap;
    let itip_dx = tip_dx.max(thick * 0.5);

    // Segment 4i: bottom hook inner — (eg, h-eg) → (iarm, h-hk)
    pw.cmd(b'L');
    pw.push_f64(eg);
    pw.push_f64(h - eg);
    pw.cmd(b'C');
    pw.push_f64(w * 0.50);
    pw.push_f64(h - eg);
    pw.push_f64(iarm);
    pw.push_f64(h - hk * 0.12);
    pw.push_f64(iarm);
    pw.push_f64(h - hk);
    // Segment 3i: lower arm → tip inner — (iarm, h-hk) → (itip, mid)
    pw.cmd(b'C');
    pw.push_f64(iarm);
    pw.push_f64(mid + (h - mid) * 0.60);
    pw.push_f64(itip - itip_dx);
    pw.push_f64(mid + tip_dy);
    pw.push_f64(itip);
    pw.push_f64(mid);
    // Segment 2i: tip → upper arm inner — (itip, mid) → (iarm, hk)
    pw.cmd(b'C');
    pw.push_f64(itip - itip_dx);
    pw.push_f64(mid - tip_dy);
    pw.push_f64(iarm);
    pw.push_f64(mid * 0.40);
    pw.push_f64(iarm);
    pw.push_f64(hk);
    // Segment 1i: top hook inner — (iarm, hk) → (eg, eg)
    pw.cmd(b'C');
    pw.push_f64(iarm);
    pw.push_f64(hk * 0.12);
    pw.push_f64(w * 0.50);
    pw.push_f64(eg);
    pw.push_f64(eg);
    pw.push_f64(eg);
    pw.close();

    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: 0.0,
    }
}

fn vertical_bar(h: f64, sw: f64) -> DelimiterPath {
    let w = bar_width(h);
    let x = w * 0.5;
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(x);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(x);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn double_vertical_bar(h: f64, sw: f64) -> DelimiterPath {
    let w = double_bar_width(h);
    let x1 = w * 0.28;
    let x2 = w * 0.72;
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(x1);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(x1);
    pw.push_f64(h);
    pw.cmd(b'M');
    pw.push_f64(x2);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(x2);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn left_floor(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // └ shape: vertical left side + horizontal bottom
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(0.0);
    pw.push_f64(h);
    pw.push_f64(w);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn right_floor(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // ┘ shape: vertical right side + horizontal bottom
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(w);
    pw.push_f64(h);
    pw.push_f64(0.0);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn left_ceil(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // ┌ shape: horizontal top + vertical left side
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn right_ceil(h: f64, sw: f64) -> DelimiterPath {
    let w = bracket_width(h);
    // ┐ shape: horizontal top + vertical right side
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.push_f64(w);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn left_angle(h: f64, sw: f64) -> DelimiterPath {
    let w = angle_width(h);
    let mid = h * 0.5;
    // V-shape pointing left: top-right → middle-left → bottom-right
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(w);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(0.0);
    pw.push_f64(mid);
    pw.push_f64(w);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

fn right_angle(h: f64, sw: f64) -> DelimiterPath {
    let w = angle_width(h);
    let mid = h * 0.5;
    // V-shape pointing right: top-left → middle-right → bottom-left
    let mut pw = PathWriter::with_prefix("");
    pw.cmd(b'M');
    pw.push_f64(0.0);
    pw.push_f64(0.0);
    pw.cmd(b'L');
    pw.push_f64(w);
    pw.push_f64(mid);
    pw.push_f64(0.0);
    pw.push_f64(h);
    DelimiterPath {
        path_data: pw.finish(),
        width: w,
        stroke_width: sw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_use_path_respects_threshold() {
        // At normal text size (size=1.0), a delimiter with target_height 0.4
        // is below the threshold and should use text.
        assert!(!should_use_path('(', 0.4, 1.0));
        // At 0.8, it should now use path rendering (threshold lowered to 0.5).
        assert!(should_use_path('(', 0.8, 1.0));
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
        for ch in [
            '(', ')', '[', ']', '{', '}', '|', '⟨', '⟩', '⌊', '⌋', '⌈', '⌉',
        ] {
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
        assert!(
            w_large > w_small,
            "taller paren should be wider (up to cap)"
        );
    }

    #[test]
    fn path_data_starts_with_move() {
        for ch in ['(', ')', '[', ']', '{', '}', '|'] {
            let dp = delimiter_path(ch, 2.0, 1.0).unwrap();
            // Filled paths start with "F:M", stroked paths start with "M"
            let starts_ok = dp.path_data.starts_with('M') || dp.path_data.starts_with("F:M");
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
