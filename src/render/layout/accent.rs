use super::*;

/// Returns `true` if the accent char should be drawn as a stretchy SVG path
/// that scales to the base width, rather than as a fixed glyph.
pub(super) fn is_stretchy_accent(accent: char) -> bool {
    matches!(
        accent,
        '\u{0302}'  // hat / widehat (combining circumflex)
        | '\u{030C}' // check / widecheck (combining caron)
        | '\u{0303}' // tilde / widetilde (combining tilde)
        | '\u{0304}' // bar / overline (combining macron)
        | '\u{0307}' // dot above (combining dot above)
        | '\u{0308}' // ddot / diaeresis (combining diaeresis)
        | '\u{0300}' // grave accent
        | '\u{0301}' // acute accent
        | '\u{0306}' // breve
        | '\u{030A}' // ring above (mathring)
        | '\u{20DB}' // dddot (combining three dots above)
        | '\u{20DC}' // ddddot (combining four dots above)
        | '\u{0330}' // utilde (combining tilde below)
        | '\u{20D7}' // vec / overrightarrow (combining right arrow above)
        | '\u{2192}' // overrightarrow (rightwards arrow)
        | '\u{2190}' // overleftarrow (leftwards arrow)
        | '\u{2194}' // overleftrightarrow (left right arrow)
        | '\u{21C0}' // overrightharpoon (rightwards harpoon)
        | '\u{21BC}' // overleftharpoon (leftwards harpoon)
    )
}

/// A pre-drawn accent path with its native viewBox dimensions (in KaTeX units).
pub(super) struct KatexAccentPath {
    path: &'static str,
    viewbox_w: f64,
    viewbox_h: f64,
}

pub(super) const WIDEHAT_PATHS: [KatexAccentPath; 4] = [
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

pub(super) const WIDETILDE_PATHS: [KatexAccentPath; 4] = [
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

pub(super) const WIDECHECK_PATHS: [KatexAccentPath; 4] = [
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
pub(super) fn katex_accent_tier(w: f64) -> usize {
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
pub(super) fn scale_svg_path(path: &str, sx: f64, sy: f64) -> String {
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
            // Scientific notation (e.g. 1.5e-3)
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                i += 1;
                if i < len && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
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
                    },
                    // M, L, C, S, Q, T: alternating x, y
                    _ => {
                        if param_index.is_multiple_of(2) {
                            val * sx
                        } else {
                            val * sy
                        }
                    },
                };
                // Ensure a separator before this number if the previous char
                // could be mistaken as part of the same token (digit or '.').
                if let Some(&last) = result.as_bytes().last()
                    && (last.is_ascii_digit() || last == b'.')
                {
                    result.push(' ');
                }
                // Format with up to 4 decimal places, trimming trailing zeros
                fmt_fixed4_trim(&mut result, scaled);
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
pub(super) fn katex_accent_scaled(paths: &[KatexAccentPath; 4], w: f64, h: f64) -> String {
    let tier = katex_accent_tier(w);
    let data = &paths[tier];
    // KaTeX SVG accent paths have internal whitespace margins — the visible
    // shape occupies ~87% of the viewbox width. Scale wider to compensate
    // so the accent visually spans the full base width.
    let compensation = 1.15; // 1/0.87 ≈ 1.15
    let sx = (w * compensation) / data.viewbox_w;
    let sy = h / data.viewbox_h;
    let scaled = scale_svg_path(data.path, sx, sy);
    format!("F:{scaled}")
}

/// Compute the height of a KaTeX accent path for a given tier.
///
/// The viewbox_h value (e.g. 239 for widehat tier 0) represents the accent
/// height in 1/1000 em units in KaTeX's coordinate system. We convert this
/// directly to em, matching KaTeX's proportions. A minimum of 0.15 em
/// ensures accents remain visible at small rendering sizes.
pub(super) fn katex_accent_natural_height(paths: &[KatexAccentPath; 4], w: f64, size: f64) -> f64 {
    let tier = katex_accent_tier(w);
    let data = &paths[tier];
    let raw = data.viewbox_h / 1000.0 * size;
    raw.max(0.15 * size)
}

/// Compute the correct height for a stretchy accent at the given base width.
/// For KaTeX-path-based accents (hat, tilde, check), the height is fixed per
/// tier. For other accents (arrows, vec), uses 0.25 * size so the arrowhead
/// is clearly visible.
pub(super) fn stretchy_accent_height(accent: char, w: f64, size: f64) -> f64 {
    match accent {
        '\u{0302}' => katex_accent_natural_height(&WIDEHAT_PATHS, w, size),
        '\u{030C}' => katex_accent_natural_height(&WIDECHECK_PATHS, w, size),
        '\u{0303}' => katex_accent_natural_height(&WIDETILDE_PATHS, w, size),
        _ => 0.25 * size,
    }
}

/// Generate a stroke-based circle SVG path for ring accents (mathring).
/// `r` is the radius. The path fits in a 2r × 2r box.
pub(super) fn ring_circle_path(r: f64) -> String {
    let d = r * 2.0;
    format!("M{d:.4},{r:.4} a{r:.4},{r:.4} 0 1 1 -{d:.4},0 a{r:.4},{r:.4} 0 1 1 {d:.4},0")
}

/// Generate an SVG path string for a stretchy accent at the given width and height.
/// Coordinates are in em units with (0,0) at top-left.
/// Paths prefixed with "F:" are filled shapes (rendered without stroke);
/// others use stroke-based rendering.
/// Generate a filled circle SVG path for dot accents.
/// `r` is the radius in em-equivalent units. The path fits in a 2r × 2r box.
pub(super) fn dot_circle_path(r: f64) -> String {
    let d = r * 2.0;
    format!("F:M{d:.4},{r:.4} a{r:.4},{r:.4} 0 1 1 -{d:.4},0 a{r:.4},{r:.4} 0 1 1 {d:.4},0 Z")
}

pub(super) fn stretchy_accent_path(accent: char, w: f64, h: f64) -> String {
    match accent {
        // Hat (U+0302): use KaTeX widehat paths
        '\u{0302}' => katex_accent_scaled(&WIDEHAT_PATHS, w, h),
        // Check/caron (U+030C): use KaTeX widecheck paths
        '\u{030C}' => katex_accent_scaled(&WIDECHECK_PATHS, w, h),
        // Tilde (U+0303): use KaTeX widetilde paths
        '\u{0303}' => katex_accent_scaled(&WIDETILDE_PATHS, w, h),
        // Right arrow (overrightarrow, vec) — filled shape
        '\u{20D7}' | '\u{2192}' => {
            let mid = h / 2.0;
            let st = (h * 0.12).max(0.008); // shaft half-thickness
            let ah = (h * 0.46).max(st * 2.5); // arrowhead half-height
            let hl = (ah * 2.0).min(w * 0.18).max(h * 0.8); // head length
            let se = (w - hl).max(0.0); // shaft end
            format!(
                "F:M0,{mt:.4} L{se:.4},{mt:.4} L{se:.4},{at:.4} L{w:.4},{mid:.4} \
                 L{se:.4},{ab:.4} L{se:.4},{mb:.4} L0,{mb:.4} Z",
                mt = mid - st,
                mb = mid + st,
                at = mid - ah,
                ab = mid + ah,
            )
        },
        // Left arrow (overleftarrow) — filled shape
        '\u{2190}' => {
            let mid = h / 2.0;
            let st = (h * 0.12).max(0.008);
            let ah = (h * 0.46).max(st * 2.5);
            let hl = (ah * 2.0).min(w * 0.18).max(h * 0.8);
            let ss = hl.min(w); // shaft start
            format!(
                "F:M{w:.4},{mt:.4} L{ss:.4},{mt:.4} L{ss:.4},{at:.4} L0,{mid:.4} \
                 L{ss:.4},{ab:.4} L{ss:.4},{mb:.4} L{w:.4},{mb:.4} Z",
                mt = mid - st,
                mb = mid + st,
                at = mid - ah,
                ab = mid + ah,
            )
        },
        // Left-right arrow — filled shape with arrowheads at both ends
        '\u{2194}' => {
            let mid = h / 2.0;
            let st = (h * 0.12).max(0.008);
            let ah = (h * 0.44).max(st * 2.5);
            let hl = (ah * 2.0).min(w * 0.15).max(h * 0.7);
            let ss = hl.min(w * 0.5); // left shaft start
            let se = (w - hl).max(w * 0.5); // right shaft end
            format!(
                "F:M0,{mid:.4} L{ss:.4},{at:.4} L{ss:.4},{mt:.4} \
                 L{se:.4},{mt:.4} L{se:.4},{at:.4} L{w:.4},{mid:.4} \
                 L{se:.4},{ab:.4} L{se:.4},{mb:.4} \
                 L{ss:.4},{mb:.4} L{ss:.4},{ab:.4} Z",
                mt = mid - st,
                mb = mid + st,
                at = mid - ah,
                ab = mid + ah,
            )
        },
        // Right harpoon (upper barb only) — filled shape
        '\u{21C0}' => {
            let mid = h / 2.0;
            let st = (h * 0.12).max(0.008);
            let ah = (h * 0.46).max(st * 2.5);
            let hl = (ah * 2.0).min(w * 0.18).max(h * 0.8);
            let se = (w - hl).max(0.0);
            // Upper barb: shaft top → upper vertex → tip, then straight back at mid
            format!(
                "F:M0,{mt:.4} L{se:.4},{mt:.4} L{se:.4},{at:.4} L{w:.4},{mid:.4} \
                 L{se:.4},{mid:.4} L{se:.4},{mb:.4} L0,{mb:.4} Z",
                mt = mid - st,
                mb = mid + st,
                at = mid - ah,
            )
        },
        // Left harpoon (upper barb only) — filled shape
        '\u{21BC}' => {
            let mid = h / 2.0;
            let st = (h * 0.12).max(0.008);
            let ah = (h * 0.46).max(st * 2.5);
            let hl = (ah * 2.0).min(w * 0.18).max(h * 0.8);
            let ss = hl.min(w);
            // Upper barb: shaft top → upper vertex → tip, then straight back at mid
            format!(
                "F:M{w:.4},{mt:.4} L{ss:.4},{mt:.4} L{ss:.4},{at:.4} L0,{mid:.4} \
                 L{ss:.4},{mid:.4} L{ss:.4},{mb:.4} L{w:.4},{mb:.4} Z",
                mt = mid - st,
                mb = mid + st,
                at = mid - ah,
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
pub(super) fn stretchy_overunder_path(sym: &str, w: f64, h: f64) -> String {
    // Thickness for filled brace shapes (proportion of height)
    // KaTeX's brace glyphs have ~12-15% visible stroke weight relative to height
    let t = (h * 0.22).max(0.03);
    match sym {
        // Overbrace ⏞ — filled curly brace above, tip points up
        // Three-segment shape: left wing → sharp center cusp → right wing
        "\u{23DE}" => {
            let mid = w / 2.0;
            // Cusp depth: how far the tip extends above the wing baseline
            let cusp_dy = (h * 0.12).max(0.02);
            // Outer curve control points — wings from bottom up to sharp center tip
            let c1x = w * 0.08; // first control: near left edge
            let c1y = h * 0.12; // pulled high (near top)
            let c2x = mid * 0.50; // second control: approaching center
            let c2y = cusp_dy; // offset below tip for V-cusp angle
            // Right wing mirrors left
            let c3x = w - mid * 0.50;
            let c3y = cusp_dy;
            let c4x = w - w * 0.08;
            let c4y = h * 0.12;
            // Inner curve (offset inward by thickness t)
            let ih = h - t; // inner baseline
            let it = t; // inner tip y
            let inner_cusp = it + cusp_dy;
            let ic1y = h * 0.12 + t * 0.3;
            let ic4y = ic1y;
            format!(
                "F:M0,{h:.4} \
                 C{c1x:.4},{c1y:.4} {c2x:.4},{c2y:.4} {mid:.4},0 \
                 C{c3x:.4},{c3y:.4} {c4x:.4},{c4y:.4} {w:.4},{h:.4} \
                 L{w:.4},{ih:.4} \
                 C{c4x:.4},{ic4y:.4} {c3x:.4},{inner_cusp:.4} {mid:.4},{it:.4} \
                 C{c2x:.4},{inner_cusp:.4} {c1x:.4},{ic1y:.4} 0,{ih:.4} \
                 Z",
                h = h,
                mid = mid,
                w = w,
                c1x = c1x,
                c1y = c1y,
                c2x = c2x,
                c2y = c2y,
                c3x = c3x,
                c3y = c3y,
                c4x = c4x,
                c4y = c4y,
                ih = ih,
                it = it,
                inner_cusp = inner_cusp,
                ic1y = ic1y,
                ic4y = ic4y,
            )
        },
        // Underbrace ⏟ — filled curly brace below, tip points down
        "\u{23DF}" => {
            let mid = w / 2.0;
            let cusp_dy = (h * 0.12).max(0.02);
            let c1x = w * 0.08;
            let c1y = h - h * 0.12;
            let c2x = mid * 0.50;
            let c2y = h - cusp_dy;
            let c3x = w - mid * 0.50;
            let c3y = h - cusp_dy;
            let c4x = w - w * 0.08;
            let c4y = h - h * 0.12;
            let it = t;
            let ih = h - t;
            let inner_cusp = ih - cusp_dy;
            let ic1y = h - h * 0.12 - t * 0.3;
            let ic4y = ic1y;
            format!(
                "F:M0,0 \
                 C{c1x:.4},{c1y:.4} {c2x:.4},{c2y:.4} {mid:.4},{h:.4} \
                 C{c3x:.4},{c3y:.4} {c4x:.4},{c4y:.4} {w:.4},0 \
                 L{w:.4},{it:.4} \
                 C{c4x:.4},{ic4y:.4} {c3x:.4},{inner_cusp:.4} {mid:.4},{ih:.4} \
                 C{c2x:.4},{inner_cusp:.4} {c1x:.4},{ic1y:.4} 0,{it:.4} \
                 Z",
                h = h,
                mid = mid,
                w = w,
                c1x = c1x,
                c1y = c1y,
                c2x = c2x,
                c2y = c2y,
                c3x = c3x,
                c3y = c3y,
                c4x = c4x,
                c4y = c4y,
                it = it,
                ih = ih,
                inner_cusp = inner_cusp,
                ic1y = ic1y,
                ic4y = ic4y,
            )
        },
        // Overbracket ⎴ — square bracket above (filled)
        "\u{23B4}" => {
            format!(
                "F:M0,{h:.4} L0,0 L{w:.4},0 L{w:.4},{h:.4} \
                 L{wi:.4},{h:.4} L{wi:.4},{t:.4} L{t:.4},{t:.4} L{t:.4},{h:.4} Z",
                h = h,
                w = w,
                wi = w - t,
                t = t
            )
        },
        // Underbracket ⎵ — square bracket below (filled)
        "\u{23B5}" => {
            format!(
                "F:M0,0 L0,{h:.4} L{w:.4},{h:.4} L{w:.4},0 \
                 L{wi:.4},0 L{wi:.4},{hi:.4} L{t:.4},{hi:.4} L{t:.4},0 Z",
                h = h,
                w = w,
                wi = w - t,
                hi = h - t,
                t = t
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

/// Compute the italic skew correction for accent positioning.
/// When the base of an accent is a single italic character, TeX shifts the accent
/// right by half the italic correction to compensate for the italic slant.
pub(super) fn accent_skew(base: &MathNode, size: f64) -> f64 {
    if let MathNode::Identifier(s) = base {
        let mut chars = s.chars();
        if let Some(ch) = chars.next()
            && chars.next().is_none()
        {
            return metrics::glyph_metrics(ch).italic * size / 2.0;
        }
    }
    0.0
}

pub(super) fn layout_accent(
    base: &MathNode,
    accent: char,
    position: AccentPosition,
    wide: bool,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let base_box = layout_node(base, ctx);
    let base_width = base_box.width;

    // For stretchy accents, generate an SVG path scaled to the base width
    let accent_box = if is_stretchy_accent(accent) && base_width > 0.0 {
        let size = ctx.size();

        // For non-wide accents (\hat, \tilde, \check), use a fixed-size accent
        // instead of stretching to base width when the base is narrow (single char).
        let effective_width =
            if !wide && (accent == '\u{0302}' || accent == '\u{0303}' || accent == '\u{030C}') {
                let fixed = 0.45 * size;
                if base_width < fixed * 1.2 {
                    fixed
                } else {
                    base_width
                }
            } else {
                base_width
            };

        match accent {
            // Bar: horizontal rule across base width — thicker than fraction
            // rules so the accent is clearly visible.
            '\u{0304}' => {
                let thickness = math_table::math_constants()
                    .map(|c| (c.overbar_rule_thickness * size).max(0.06 * size))
                    .unwrap_or((ctx.params.rule_thickness * size * 1.8).max(0.06 * size));
                LayoutBox::rule(base_width, thickness)
            },
            // Dot: small filled square centered above base
            '\u{0307}' => {
                let dot_r = 0.07 * size;
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
                let dot_r = 0.07 * size;
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
            // Grave accent (U+0300): diagonal stroke upper-left to lower-right
            '\u{0300}' => {
                let ah = 0.22 * size;
                let aw = 0.30 * size;
                let accent = LayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(format!(
                        "M{:.4},0 L{:.4},{:.4}",
                        0.04 * size,
                        0.26 * size,
                        ah
                    )),
                };
                let dx = (base_width - aw) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, accent)],
                    },
                }
            },
            // Acute accent (U+0301): diagonal stroke lower-left to upper-right
            '\u{0301}' => {
                let ah = 0.22 * size;
                let aw = 0.30 * size;
                let accent = LayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(format!(
                        "M{:.4},{:.4} L{:.4},0",
                        0.04 * size,
                        ah,
                        0.26 * size
                    )),
                };
                let dx = (base_width - aw) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, accent)],
                    },
                }
            },
            // Breve (U+0306): cup/U curve
            '\u{0306}' => {
                let ah = 0.18 * size;
                let aw = 0.30 * size;
                let accent = LayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(format!(
                        "M0,0 Q{:.4},{:.4} {:.4},0",
                        aw / 2.0,
                        ah * 2.5,
                        aw
                    )),
                };
                let dx = (base_width - aw) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: ah,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, accent)],
                    },
                }
            },
            // Ring above / mathring (U+030A): small unfilled circle
            '\u{030A}' => {
                let ring_r = 0.08 * size;
                let ring_size = ring_r * 2.0;
                let ring = LayoutBox {
                    width: ring_size,
                    height: ring_size,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(ring_circle_path(ring_r)),
                };
                let dx = (base_width - ring_size) / 2.0;
                LayoutBox {
                    width: base_width,
                    height: ring_size,
                    depth: 0.0,
                    kind: BoxKind::HBox {
                        children: vec![PositionedBox::new(dx, 0.0, ring)],
                    },
                }
            },
            // Triple dot (U+20DB): three filled dots
            '\u{20DB}' => {
                let dot_r = 0.07 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.10 * size;
                let total_w = dot_size * 3.0 + gap * 2.0;
                let dx_start = (base_width - total_w) / 2.0;
                let children: Vec<PositionedBox> = (0..3)
                    .map(|i| {
                        let dx = dx_start + (i as f64) * (dot_size + gap);
                        PositionedBox::new(
                            dx,
                            0.0,
                            LayoutBox {
                                width: dot_size,
                                height: dot_size,
                                depth: 0.0,
                                kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                            },
                        )
                    })
                    .collect();
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox { children },
                }
            },
            // Quadruple dot (U+20DC): four filled dots
            '\u{20DC}' => {
                let dot_r = 0.07 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.10 * size;
                let total_w = dot_size * 4.0 + gap * 3.0;
                let dx_start = (base_width - total_w) / 2.0;
                let children: Vec<PositionedBox> = (0..4)
                    .map(|i| {
                        let dx = dx_start + (i as f64) * (dot_size + gap);
                        PositionedBox::new(
                            dx,
                            0.0,
                            LayoutBox {
                                width: dot_size,
                                height: dot_size,
                                depth: 0.0,
                                kind: BoxKind::SvgPath(dot_circle_path(dot_r)),
                            },
                        )
                    })
                    .collect();
                LayoutBox {
                    width: base_width,
                    height: dot_size,
                    depth: 0.0,
                    kind: BoxKind::HBox { children },
                }
            },
            // Tilde below / utilde (U+0330): always stretchy (no fixed-size variant),
            // so use base_width directly instead of effective_width.
            '\u{0330}' => {
                let accent_h = stretchy_accent_height('\u{0303}', base_width, size);
                LayoutBox {
                    width: base_width,
                    height: accent_h,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(stretchy_accent_path('\u{0303}', base_width, accent_h)),
                }
            },
            // Other stretchy accents: SVG path
            _ => {
                let accent_h = stretchy_accent_height(accent, effective_width, size);
                LayoutBox {
                    width: effective_width,
                    height: accent_h,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(stretchy_accent_path(accent, effective_width, accent_h)),
                }
            },
        }
    } else {
        LayoutBox::glyph(accent, ctx.size(), false)
    };

    // TeX Rule 12: italic skew correction only for above accents.
    let skew = match position {
        AccentPosition::Above => accent_skew(base, ctx.size()),
        AccentPosition::Below => 0.0,
    };
    let accent_dx = (base_width - accent_box.width) / 2.0 + skew;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            // OpenType MATH: use accent_base_height as minimum base height
            // so accents sit at a consistent level for short characters.
            let accent_base_h = math_table::math_constants()
                .map(|c| c.accent_base_height * ctx.size())
                .unwrap_or(0.0);
            let effective_h = base_box.height.max(accent_base_h);
            let dy = effective_h + 0.05 * ctx.size();
            (dy, dy + accent_box.height, base_box.depth)
        },
        AccentPosition::Below => {
            let dy = -(base_box.depth + accent_box.height + 0.05 * ctx.size());
            (
                dy,
                base_box.height,
                base_box.depth + accent_box.height + 0.05 * ctx.size(),
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

/// Compact-arena variant of `accent_skew`.
pub(super) fn accent_skew_c(arena: &AstArena, base: NodeId, size: f64) -> f64 {
    if let CompactNode::Identifier(sid) = arena.node(base) {
        let s = arena.str_val(*sid);
        let mut chars = s.chars();
        if let Some(ch) = chars.next()
            && chars.next().is_none()
        {
            return metrics::glyph_metrics(ch).italic * size / 2.0;
        }
    }
    0.0
}

// ==========================================================================
// Arena-based layout functions
// ==========================================================================

pub(super) fn layout_accent_arena(
    ast: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    wide: bool,
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

        // For non-wide accents (\hat, \tilde, \check), use a fixed-size accent
        // instead of stretching to base width when the base is narrow (single char).
        let effective_width =
            if !wide && (accent == '\u{0302}' || accent == '\u{0303}' || accent == '\u{030C}') {
                let fixed = 0.45 * size;
                if base_w < fixed * 1.2 { fixed } else { base_w }
            } else {
                base_w
            };

        match accent {
            '\u{0304}' => {
                let thickness = math_table::math_constants()
                    .map(|c| (c.overbar_rule_thickness * size).max(0.06 * size))
                    .unwrap_or((ctx.params.rule_thickness * size * 1.8).max(0.06 * size));
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: thickness / 2.0,
                    depth: thickness / 2.0,
                    kind: ArenaBoxKind::Rule { thickness },
                })
            },
            '\u{0307}' => {
                let dot_r = 0.07 * size;
                let dot_size = dot_r * 2.0;
                let dx = (base_w - dot_size) / 2.0;
                let dot_id = arena.alloc(ArenaLayoutBox {
                    width: dot_size,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                });
                let children_start = arena.begin_children();
                arena.push_child(ArenaPositionedBox {
                    dx,
                    dy: 0.0,
                    id: dot_id,
                });
                let children = arena.finish_children(children_start);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            '\u{0308}' => {
                let dot_r = 0.07 * size;
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
                arena.push_child(ArenaPositionedBox {
                    dx: dx_left,
                    dy: 0.0,
                    id: dot1,
                });
                arena.push_child(ArenaPositionedBox {
                    dx: dx_right,
                    dy: 0.0,
                    id: dot2,
                });
                let children = arena.finish_children(children_start);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Grave accent (U+0300)
            '\u{0300}' => {
                let ah = 0.22 * size;
                let aw = 0.30 * size;
                let acc_id = arena.alloc(ArenaLayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(format!(
                        "M{:.4},0 L{:.4},{:.4}",
                        0.04 * size,
                        0.26 * size,
                        ah
                    )),
                });
                let dx = (base_w - aw) / 2.0;
                let cs = arena.begin_children();
                arena.push_child(ArenaPositionedBox {
                    dx,
                    dy: 0.0,
                    id: acc_id,
                });
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Acute accent (U+0301)
            '\u{0301}' => {
                let ah = 0.22 * size;
                let aw = 0.30 * size;
                let acc_id = arena.alloc(ArenaLayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(format!(
                        "M{:.4},{:.4} L{:.4},0",
                        0.04 * size,
                        ah,
                        0.26 * size
                    )),
                });
                let dx = (base_w - aw) / 2.0;
                let cs = arena.begin_children();
                arena.push_child(ArenaPositionedBox {
                    dx,
                    dy: 0.0,
                    id: acc_id,
                });
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Breve (U+0306)
            '\u{0306}' => {
                let ah = 0.18 * size;
                let aw = 0.30 * size;
                let acc_id = arena.alloc(ArenaLayoutBox {
                    width: aw,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(format!(
                        "M0,0 Q{:.4},{:.4} {:.4},0",
                        aw / 2.0,
                        ah * 2.5,
                        aw
                    )),
                });
                let dx = (base_w - aw) / 2.0;
                let cs = arena.begin_children();
                arena.push_child(ArenaPositionedBox {
                    dx,
                    dy: 0.0,
                    id: acc_id,
                });
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: ah,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Ring above (U+030A)
            '\u{030A}' => {
                let ring_r = 0.08 * size;
                let ring_size = ring_r * 2.0;
                let ring_id = arena.alloc(ArenaLayoutBox {
                    width: ring_size,
                    height: ring_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(ring_circle_path(ring_r)),
                });
                let dx = (base_w - ring_size) / 2.0;
                let cs = arena.begin_children();
                arena.push_child(ArenaPositionedBox {
                    dx,
                    dy: 0.0,
                    id: ring_id,
                });
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: ring_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Triple dot (U+20DB)
            '\u{20DB}' => {
                let dot_r = 0.07 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.10 * size;
                let total_w = dot_size * 3.0 + gap * 2.0;
                let dx_start = (base_w - total_w) / 2.0;
                let cs = arena.begin_children();
                for i in 0..3 {
                    let d = arena.alloc(ArenaLayoutBox {
                        width: dot_size,
                        height: dot_size,
                        depth: 0.0,
                        kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                    });
                    arena.push_child(ArenaPositionedBox {
                        dx: dx_start + (i as f64) * (dot_size + gap),
                        dy: 0.0,
                        id: d,
                    });
                }
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Quadruple dot (U+20DC)
            '\u{20DC}' => {
                let dot_r = 0.07 * size;
                let dot_size = dot_r * 2.0;
                let gap = 0.10 * size;
                let total_w = dot_size * 4.0 + gap * 3.0;
                let dx_start = (base_w - total_w) / 2.0;
                let cs = arena.begin_children();
                for i in 0..4 {
                    let d = arena.alloc(ArenaLayoutBox {
                        width: dot_size,
                        height: dot_size,
                        depth: 0.0,
                        kind: ArenaBoxKind::SvgPath(dot_circle_path(dot_r)),
                    });
                    arena.push_child(ArenaPositionedBox {
                        dx: dx_start + (i as f64) * (dot_size + gap),
                        dy: 0.0,
                        id: d,
                    });
                }
                let children = arena.finish_children(cs);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: dot_size,
                    depth: 0.0,
                    kind: ArenaBoxKind::HBox { children },
                })
            },
            // Tilde below / utilde (U+0330): always stretchy (no fixed-size variant),
            // so use base_w directly instead of effective_width.
            '\u{0330}' => {
                let accent_h = stretchy_accent_height('\u{0303}', base_w, size);
                arena.alloc(ArenaLayoutBox {
                    width: base_w,
                    height: accent_h,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(stretchy_accent_path('\u{0303}', base_w, accent_h)),
                })
            },
            _ => {
                let accent_h = stretchy_accent_height(accent, effective_width, size);
                arena.alloc(ArenaLayoutBox {
                    width: effective_width,
                    height: accent_h,
                    depth: 0.0,
                    kind: ArenaBoxKind::SvgPath(stretchy_accent_path(
                        accent,
                        effective_width,
                        accent_h,
                    )),
                })
            },
        }
    } else {
        alloc_glyph(arena, accent, ctx.size(), false)
    };

    let accent_w = arena.get(accent_id).width;
    let accent_h = arena.get(accent_id).height;

    let skew = match position {
        AccentPosition::Above => accent_skew_c(ast, base, ctx.size()),
        AccentPosition::Below => 0.0,
    };
    let accent_dx = (base_w - accent_w) / 2.0 + skew;
    let (accent_dy, total_h, total_d) = match position {
        AccentPosition::Above => {
            let accent_base_h = math_table::math_constants()
                .map(|c| c.accent_base_height * ctx.size())
                .unwrap_or(0.0);
            let effective_h = base_h.max(accent_base_h);
            let dy = effective_h + 0.05 * ctx.size();
            (dy, dy + accent_h, base_d)
        },
        AccentPosition::Below => {
            let dy = -(base_d + accent_h + 0.05 * ctx.size());
            (dy, base_h, base_d + accent_h + 0.05 * ctx.size())
        },
    };

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: base_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: accent_dx,
        dy: accent_dy,
        id: accent_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: base_w,
        height: total_h,
        depth: total_d,
        kind: ArenaBoxKind::HBox { children },
    })
}
