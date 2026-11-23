//! Fallback drawing for characters the embedded fonts cannot draw.
//!
//! The renderer draws every glyph from Latin Modern Math (or a Latin Modern
//! text face). A code point absent from those fonts would otherwise rasterize
//! to nothing — a silent gap in PNG/JPEG output. Glyph boxes and text-run
//! characters are therefore checked against the fonts' coverage at layout time
//! (printable ASCII short-circuits; otherwise an O(1) bitset probe), and an
//! uncovered character is [resolved](resolve) in this order:
//!
//! 1. **substitution** by an equivalent in-font code point ([`SUBSTITUTES`]:
//!    `‾` → `¯`, presentation-form braces `︷` → `⏞`, …);
//! 2. a Latin Modern **text face** that has it (e.g. `ƒ`);
//! 3. a **recipe** from the data-driven [`RECIPES`] table — an in-font glyph
//!    transformed (`★` = scaled `⋆`, `Ⅎ` = `F` turned 180°) or composed from
//!    glyphs, single glyph contours and rules (`⩴` = `∷` + `=`, `⪵` = `≺`
//!    over the lower contour of `≨`). The result is a filled
//!    [`SvgPath`](super::layout::BoxKind::SvgPath) box, so every backend
//!    (embedded SVG, text SVG, PNG/JPEG) draws exactly the same shape;
//! 4. **notdef**: the glyph box is left as is. Both SVG emitters then write
//!    the character as `<text>` (so a browser can still show e.g. CJK from a
//!    system font), while the raster backend draws a hollow box
//!    ([`notdef_frame`]) — never a silent blank. This is the one intentional
//!    SVG/PNG difference.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use super::fonts_registry::{self, FontId};
use super::path_writer::PathWriter;

// ── Coverage ────────────────────────────────────────────────────────────────

/// Code points below this bound are answered from the [`MATH_COVERAGE`] bitset
/// (BMP + SMP, which holds the math alphanumerics); anything above falls back
/// to a cmap lookup.
const BITSET_LIMIT: u32 = 0x2_0000;

/// One bit per code point in `0..BITSET_LIMIT`: set when Latin Modern Math maps
/// it to a real (non-notdef) glyph. 16 KiB, built once from the cmap.
static MATH_COVERAGE: LazyLock<Box<[u64]>> = LazyLock::new(|| {
    let mut bits = vec![0_u64; (BITSET_LIMIT / 64) as usize].into_boxed_slice();
    let face = fonts_registry::face(FontId::Math);
    if let Some(cmap) = face.tables().cmap {
        for sub in cmap.subtables {
            if !sub.is_unicode() {
                continue;
            }
            sub.codepoints(|cp| {
                if cp < BITSET_LIMIT && sub.glyph_index(cp).is_some_and(|g| g.0 != 0) {
                    bits[(cp / 64) as usize] |= 1 << (cp % 64);
                }
            });
        }
    }
    bits
});

/// Does Latin Modern Math have a glyph for `ch`?
#[inline]
fn math_has_glyph(ch: char) -> bool {
    let cp = ch as u32;
    if cp < BITSET_LIMIT {
        MATH_COVERAGE[(cp / 64) as usize] & (1 << (cp % 64)) != 0
    } else {
        fonts_registry::face(FontId::Math)
            .glyph_index(ch)
            .is_some_and(|g| g.0 != 0)
    }
}

/// Characters that legitimately draw no ink (spaces, controls, format
/// characters). They never get a fallback — a notdef box would be wrong.
#[inline]
fn is_inkless(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
        )
}

/// True when a glyph box for `ch` drawn from `font` would render nothing
/// because neither the requested face nor the Math font (the emitters' own
/// fallback) has the glyph. Hot path: one bitset probe for covered glyphs.
#[inline]
pub(crate) fn needs_fallback(ch: char, font: FontId) -> bool {
    // Printable ASCII is always covered (asserted by a test); skip the
    // lazily-initialized bitset for the overwhelmingly common case.
    if ch.is_ascii_graphic() || math_has_glyph(ch) {
        return false;
    }
    needs_fallback_cold(ch, font)
}

#[cold]
fn needs_fallback_cold(ch: char, font: FontId) -> bool {
    if is_inkless(ch) {
        return false;
    }
    font.is_math() || fonts_registry::face(font).glyph_index(ch).is_none()
}

/// Compatibility / presentation-form characters whose canonical equivalent is
/// in Latin Modern Math (sorted by the first element).
static SUBSTITUTES: &[(char, char)] = &[
    ('\u{203E}', '\u{00AF}'), // ‾ OVERLINE (MathML &OverBar;) → ¯ MACRON
    ('\u{FE35}', '\u{23DC}'), // ︵ → ⏜ top parenthesis
    ('\u{FE36}', '\u{23DD}'), // ︶ → ⏝ bottom parenthesis
    ('\u{FE37}', '\u{23DE}'), // ︷ → ⏞ top curly bracket
    ('\u{FE38}', '\u{23DF}'), // ︸ → ⏟ bottom curly bracket
    ('\u{FE39}', '\u{23E0}'), // ︹ → ⏠ top tortoise shell bracket
    ('\u{FE3A}', '\u{23E1}'), // ︺ → ⏡ bottom tortoise shell bracket
];

/// The in-font equivalent of a compatibility character, if any.
#[inline]
pub(crate) fn substitute(ch: char) -> Option<char> {
    SUBSTITUTES
        .binary_search_by_key(&ch, |&(c, _)| c)
        .ok()
        .map(|i| SUBSTITUTES[i].1)
}

/// A Latin Modern text face that has `ch` (e.g. `ƒ`, which only the text
/// faces carry): roman italic first for italic glyphs, then roman regular.
#[cold]
pub(crate) fn text_face_for(ch: char, italic: bool) -> Option<FontId> {
    let candidates: &[FontId] = if italic {
        &[FontId::RomanItalic, FontId::RomanRegular]
    } else {
        &[FontId::RomanRegular]
    };
    candidates.iter().copied().find(|&font| {
        fonts_registry::face(font)
            .glyph_index(ch)
            .is_some_and(|g| g.0 != 0)
    })
}

/// How an uncovered character (one for which [`needs_fallback`] is true) is
/// drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// Draw this in-font code point instead.
    Substitute(char),
    /// Draw the character from this Latin Modern text face.
    TextFace(FontId),
    /// Draw the [`RECIPES`] outline via [`glyph_shape`].
    Recipe,
    /// Keep the glyph: SVG emits `<text>`, raster draws [`notdef_frame`].
    Notdef,
}

/// Resolve an uncovered character (see the module docs for the order).
#[cold]
pub(crate) fn resolve(ch: char, italic: bool) -> Resolution {
    if let Some(sub) = substitute(ch) {
        Resolution::Substitute(sub)
    } else if let Some(font) = text_face_for(ch, italic) {
        Resolution::TextFace(font)
    } else if has_recipe(ch) {
        Resolution::Recipe
    } else {
        Resolution::Notdef
    }
}

/// Must a text run be rewritten because `ch` (uncovered in the run's `font`)
/// has a substitute or a recipe? Notdef and text-face characters need no
/// rewrite: the run's per-character emitters already handle them.
#[inline]
pub(crate) fn text_run_needs_rewrite(ch: char, font: FontId) -> bool {
    needs_fallback(ch, font) && (substitute(ch).is_some() || has_recipe(ch))
}

/// Geometry of the hollow notdef box the raster backend draws for a glyph no
/// face can render, given the glyph's layout metrics (em): `(x0, x1, top,
/// bottom, stroke)` relative to the pen position and baseline, y up.
#[cfg_attr(not(feature = "raster"), allow(dead_code))]
pub(crate) fn notdef_frame(width: f64, height: f64, depth: f64) -> (f64, f64, f64, f64, f64) {
    let w = width.max(0.3);
    let top = height.max(0.5);
    (0.08 * w, 0.92 * w, top, -depth.max(0.0), 0.05)
}

// ── Recipe table ────────────────────────────────────────────────────────────

/// Geometry of one part of a fallback, in Latin Modern Math font units
/// (1000/em, y up).
#[derive(Clone, Copy)]
enum Shape {
    /// Every contour of an in-font glyph.
    Glyph(char),
    /// Contours `from..to` (in outline order) of an in-font glyph — e.g. just
    /// the "≠" lower half of `≨`, or one bar of `⊆`.
    Contours(char, u8, u8),
    /// Filled axis-aligned rectangle `(x0, y0, x1, y1)`.
    Rect(f32, f32, f32, f32),
    /// Filled polygon (counter-clockwise in y-up font units, matching the
    /// orientation of the font's outer contours so overlaps union under the
    /// nonzero fill rule).
    Poly(&'static [(f32, f32)]),
    /// Hand-written outline (`M`/`L`/`C`/`Z` in font units).
    Path(&'static [Seg]),
}

/// One segment of a [`Shape::Path`].
#[derive(Clone, Copy)]
enum Seg {
    M(f32, f32),
    L(f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// A [`Shape`] placed by `x' = sx·x + dx`, `y' = sy·y + dy`.
#[derive(Clone, Copy)]
struct Part {
    shape: Shape,
    sx: f32,
    sy: f32,
    dx: f32,
    dy: f32,
}

/// Untransformed part.
const fn at(shape: Shape) -> Part {
    Part {
        shape,
        sx: 1.0,
        sy: 1.0,
        dx: 0.0,
        dy: 0.0,
    }
}

/// Translated part.
const fn mv(shape: Shape, dx: f32, dy: f32) -> Part {
    Part {
        shape,
        sx: 1.0,
        sy: 1.0,
        dx,
        dy,
    }
}

/// Scaled-then-translated part.
const fn xf(shape: Shape, sx: f32, sy: f32, dx: f32, dy: f32) -> Part {
    Part {
        shape,
        sx,
        sy,
        dx,
        dy,
    }
}

/// How to draw one missing code point: its advance width (font units) and the
/// parts whose union forms the glyph.
struct Recipe {
    ch: char,
    advance: f32,
    parts: &'static [Part],
}

use Shape::{Contours, Glyph, Path, Poly, Rect};

/// Head of `→` (U+2192) cut from its shaft at x = 844, used by the dashed
/// arrows. Hand-copied from the Latin Modern Math 1.959 outline (as are the
/// other literal coordinates in this table); `recipe_components_exist_in_font`
/// guards the glyph references if the font is ever updated.
const ARROW_HEAD: &[Seg] = &[
    Seg::M(943.0, 250.0),
    Seg::C(943.0, 256.0, 939.0, 261.0, 934.0, 263.0),
    Seg::C(877.0, 282.0, 833.0, 322.0, 797.0, 367.0),
    Seg::C(769.0, 403.0, 748.0, 447.0, 739.0, 494.0),
    Seg::C(737.0, 503.0, 729.0, 510.0, 719.0, 510.0),
    Seg::C(708.0, 510.0, 699.0, 501.0, 699.0, 490.0),
    Seg::C(699.0, 489.0, 700.0, 487.0, 700.0, 486.0),
    Seg::C(710.0, 433.0, 733.0, 384.0, 766.0, 342.0),
    Seg::C(788.0, 314.0, 814.0, 290.0, 844.0, 270.0),
    Seg::L(844.0, 230.0),
    Seg::C(814.0, 210.0, 788.0, 186.0, 766.0, 158.0),
    Seg::C(733.0, 116.0, 710.0, 67.0, 700.0, 14.0),
    Seg::C(700.0, 13.0, 699.0, 11.0, 699.0, 10.0),
    Seg::C(699.0, -1.0, 708.0, -10.0, 719.0, -10.0),
    Seg::C(729.0, -10.0, 737.0, -3.0, 739.0, 6.0),
    Seg::C(748.0, 53.0, 769.0, 97.0, 797.0, 133.0),
    Seg::C(833.0, 178.0, 877.0, 218.0, 934.0, 237.0),
    Seg::C(939.0, 239.0, 943.0, 244.0, 943.0, 250.0),
    Seg::Z,
];

/// `⇢`: three dashes on the math axis (same weight as `−`) plus the `→` head.
const DASH_ARROW_RIGHT: &[Part] = &[
    at(Rect(58.0, 230.0, 198.0, 270.0)),
    at(Rect(288.0, 230.0, 428.0, 270.0)),
    at(Rect(518.0, 230.0, 846.0, 270.0)),
    at(Path(ARROW_HEAD)),
];

/// `⇠`: [`DASH_ARROW_RIGHT`] mirrored about the 1000-unit advance.
const DASH_ARROW_LEFT: &[Part] = &[
    xf(Rect(58.0, 230.0, 198.0, 270.0), -1.0, 1.0, 1001.0, 0.0),
    xf(Rect(288.0, 230.0, 428.0, 270.0), -1.0, 1.0, 1001.0, 0.0),
    xf(Rect(518.0, 230.0, 846.0, 270.0), -1.0, 1.0, 1001.0, 0.0),
    xf(Path(ARROW_HEAD), -1.0, 1.0, 1001.0, 0.0),
];

/// `╱`: a thin diagonal rule spanning cap height to below the baseline.
const DIAG_UP: &[(f32, f32)] = &[
    (86.0, -192.0),
    (724.0, 668.0),
    (692.0, 692.0),
    (54.0, -168.0),
];

/// The table, sorted by code point (binary-searched; checked by a test).
///
/// Coordinates come from the Latin Modern Math outlines (inspected with
/// fontTools); components are positioned the way the font builds the analogous
/// in-font glyph (e.g. `⪵` mirrors `≨`, `⫅` mirrors `≦`).
static RECIPES: &[Recipe] = &[
    // ϝ \digamma: an upright F shrunk and dropped below the baseline.
    Recipe {
        ch: '\u{03DD}',
        advance: 520.0,
        parts: &[xf(Glyph('F'), 0.75, 0.75, 5.0, -80.0)],
    },
    // Ⅎ \Finv: F turned 180° within its advance and cap height.
    Recipe {
        ch: '\u{2132}',
        advance: 653.0,
        parts: &[xf(Glyph('F'), -1.0, -1.0, 653.0, 683.0)],
    },
    // ⅁ \Game: sans-serif G (𝖦) turned 180°.
    Recipe {
        ch: '\u{2141}',
        advance: 667.0,
        parts: &[xf(Glyph('\u{1D5A6}'), -1.0, -1.0, 664.0, 695.0)],
    },
    // ⇠ \dashleftarrow
    Recipe {
        ch: '\u{21E0}',
        advance: 1000.0,
        parts: DASH_ARROW_LEFT,
    },
    // ⇢ \dashrightarrow
    Recipe {
        ch: '\u{21E2}',
        advance: 1000.0,
        parts: DASH_ARROW_RIGHT,
    },
    // ⋔ \pitchfork: ∩ with a vertical tine through its middle.
    Recipe {
        ch: '\u{22D4}',
        advance: 667.0,
        parts: &[at(Glyph('∩')), at(Rect(314.0, -20.0, 354.0, 692.0))],
    },
    // ⎰ \lmoustache: upper-left brace hook ⎧ over lower-right hook ⎭ (their
    // vertical strokes share x = 400..502), squeezed to a 1em delimiter.
    Recipe {
        ch: '\u{23B0}',
        advance: 600.0,
        parts: &[
            xf(Glyph('\u{23A7}'), 0.8, 0.667, -62.0, 250.0),
            xf(Glyph('\u{23AD}'), 0.8, 0.667, -62.0, -250.0),
        ],
    },
    // ⎱ \rmoustache: upper-right hook ⎫ over lower-left hook ⎩.
    Recipe {
        ch: '\u{23B1}',
        advance: 600.0,
        parts: &[
            xf(Glyph('\u{23AB}'), 0.8, 0.667, -62.0, 250.0),
            xf(Glyph('\u{23A9}'), 0.8, 0.667, -62.0, -250.0),
        ],
    },
    // Ⓢ \circledS: S inside ○.
    Recipe {
        ch: '\u{24C8}',
        advance: 796.0,
        parts: &[at(Glyph('○')), xf(Glyph('S'), 0.55, 0.55, 245.4, 62.2)],
    },
    // ╱ \diagup
    Recipe {
        ch: '\u{2571}',
        advance: 778.0,
        parts: &[at(Poly(DIAG_UP))],
    },
    // ╲ \diagdown: ╱ mirrored.
    Recipe {
        ch: '\u{2572}',
        advance: 778.0,
        parts: &[xf(Poly(DIAG_UP), -1.0, 1.0, 778.0, 0.0)],
    },
    // ▴ ▵ ▸ ▹ ▾ ▿ ◂ ◃ small triangles: the full-size ▲ △ ▶ ▷ ▼ ▽ ◀ ◁
    // shrunk (up/down ones resting on the baseline, left/right ones on the
    // math axis).
    Recipe {
        ch: '\u{25B4}',
        advance: 754.0,
        parts: &[xf(Glyph('▲'), 0.75, 0.75, 14.0, 4.0)],
    },
    Recipe {
        ch: '\u{25B5}',
        advance: 754.0,
        parts: &[xf(Glyph('△'), 0.75, 0.75, 14.0, 4.0)],
    },
    Recipe {
        ch: '\u{25B8}',
        advance: 634.0,
        parts: &[xf(Glyph('▶'), 0.7, 0.7, 17.0, 75.0)],
    },
    Recipe {
        ch: '\u{25B9}',
        advance: 634.0,
        parts: &[xf(Glyph('▷'), 0.7, 0.7, 17.0, 75.0)],
    },
    Recipe {
        ch: '\u{25BE}',
        advance: 754.0,
        parts: &[xf(Glyph('▼'), 0.75, 0.75, 14.0, 181.0)],
    },
    Recipe {
        ch: '\u{25BF}',
        advance: 754.0,
        parts: &[xf(Glyph('▽'), 0.75, 0.75, 14.0, 181.0)],
    },
    Recipe {
        ch: '\u{25C2}',
        advance: 634.0,
        parts: &[xf(Glyph('◀'), 0.7, 0.7, 17.0, 75.0)],
    },
    Recipe {
        ch: '\u{25C3}',
        advance: 634.0,
        parts: &[xf(Glyph('◁'), 0.7, 0.7, 17.0, 75.0)],
    },
    // ★ \bigstar: ⋆ enlarged 1.5× about the math axis.
    Recipe {
        ch: '\u{2605}',
        advance: 860.0,
        parts: &[xf(Glyph('⋆'), 1.5, 1.5, 55.0, -125.0)],
    },
    // ⦃ \lBrace: KaTeX's `{\mkern-3.2mu[`.
    Recipe {
        ch: '\u{2983}',
        advance: 600.0,
        parts: &[at(Glyph('{')), mv(Glyph('['), 322.0, 0.0)],
    },
    // ⦄ \rBrace: KaTeX's `]\mkern-3.2mu}`.
    Recipe {
        ch: '\u{2984}',
        advance: 600.0,
        parts: &[at(Glyph(']')), mv(Glyph('}'), 100.0, 0.0)],
    },
    // ⦵ circle with horizontal bar: ○ crossed by a rule wider than the circle.
    Recipe {
        ch: '\u{29B5}',
        advance: 896.0,
        parts: &[
            mv(Glyph('○'), 50.0, 0.0),
            at(Rect(56.0, 230.0, 840.0, 270.0)),
        ],
    },
    // ⧫ \blacklozenge: the ◊ outline, filled.
    Recipe {
        ch: '\u{29EB}',
        advance: 572.0,
        parts: &[at(Poly(&[
            (286.0, -110.0),
            (516.0, 250.0),
            (286.0, 610.0),
            (56.0, 250.0),
        ]))],
    },
    // ⩞ \doublebarwedge: the ⊼ wedge, lowered, under two copies of its bar.
    Recipe {
        ch: '\u{2A5E}',
        advance: 642.0,
        parts: &[
            xf(Contours('⊼', 0, 1), 1.0, 0.85, 0.0, -5.0),
            mv(Contours('⊼', 1, 2), 0.0, -100.0),
            mv(Contours('⊼', 1, 2), 0.0, 20.0),
        ],
    },
    // ⩴ \Coloneqq: ∷ followed by =, spaced like the font's ≔.
    Recipe {
        ch: '\u{2A74}',
        advance: 1144.0,
        parts: &[mv(Glyph('∷'), -30.0, 0.0), mv(Glyph('='), 366.0, 0.0)],
    },
    // ⪵ \precneqq: ≺ raised like the < of ≨, over ≨'s slashed double bar.
    Recipe {
        ch: '\u{2AB5}',
        advance: 778.0,
        parts: &[mv(Glyph('≺'), 0.0, 168.0), at(Contours('≨', 1, 2))],
    },
    // ⪶ \succneqq
    Recipe {
        ch: '\u{2AB6}',
        advance: 778.0,
        parts: &[mv(Glyph('≻'), 0.0, 168.0), at(Contours('≩', 1, 2))],
    },
    // ⪷ \precapprox: ≺ sized like the < of ⪅, over ⪅'s ≈.
    Recipe {
        ch: '\u{2AB7}',
        advance: 776.0,
        parts: &[
            xf(Glyph('≺'), 1.0, 0.815, 0.0, 278.3),
            at(Contours('⪅', 1, 3)),
        ],
    },
    // ⪸ \succapprox
    Recipe {
        ch: '\u{2AB8}',
        advance: 776.0,
        parts: &[
            xf(Glyph('≻'), 1.0, 0.815, 0.0, 278.3),
            at(Contours('⪆', 1, 3)),
        ],
    },
    // ⪹ \precnapprox: ≺ placed like the < of ⪉, over ⪉'s slashed ≈. The
    // ⪹/⪺ offsets differ (310.3 vs 278.3) because they copy the font's own
    // asymmetric ⪉ (< at 272..756) and ⪊ (> at 240..724).
    Recipe {
        ch: '\u{2AB9}',
        advance: 776.0,
        parts: &[
            xf(Glyph('≺'), 1.0, 0.815, 0.0, 310.3),
            at(Contours('⪉', 1, 2)),
        ],
    },
    // ⪺ \succnapprox
    Recipe {
        ch: '\u{2ABA}',
        advance: 776.0,
        parts: &[
            xf(Glyph('≻'), 1.0, 0.815, 0.0, 278.3),
            at(Contours('⪊', 1, 2)),
        ],
    },
    // ⫅ \subseteqq: ⊂ raised like the < of ≦, over two copies of ⊆'s bar.
    Recipe {
        ch: '\u{2AC5}',
        advance: 778.0,
        parts: &[
            mv(Glyph('⊂'), 0.0, 168.0),
            mv(Contours('⊆', 1, 2), 0.0, -76.0),
            mv(Contours('⊆', 1, 2), 0.0, 92.0),
        ],
    },
    // ⫆ \supseteqq
    Recipe {
        ch: '\u{2AC6}',
        advance: 778.0,
        parts: &[
            mv(Glyph('⊃'), 0.0, 168.0),
            mv(Contours('⊇', 1, 2), 0.0, -76.0),
            mv(Contours('⊇', 1, 2), 0.0, 92.0),
        ],
    },
];

fn recipe(ch: char) -> Option<&'static Recipe> {
    RECIPES
        .binary_search_by_key(&ch, |r| r.ch)
        .ok()
        .map(|i| &RECIPES[i])
}

#[inline]
pub(crate) fn has_recipe(ch: char) -> bool {
    recipe(ch).is_some()
}

// ── Outline assembly ────────────────────────────────────────────────────────

/// Path commands in transformed font units (y up), with their bounds.
struct Outline {
    cmds: Vec<(u8, [f32; 6])>,
    min_y: f32,
    max_y: f32,
}

impl Outline {
    fn new() -> Self {
        Self {
            cmds: Vec::with_capacity(64),
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    fn push(&mut self, cmd: u8, pts: &[f32]) {
        let mut buf = [0.0; 6];
        buf[..pts.len()].copy_from_slice(pts);
        for y in pts.iter().skip(1).step_by(2) {
            self.min_y = self.min_y.min(*y);
            self.max_y = self.max_y.max(*y);
        }
        self.cmds.push((cmd, buf));
    }

    fn add_part(&mut self, p: &Part) {
        let mut sink = Sink {
            out: self,
            part: *p,
            contour: 0,
            from: 0,
            to: usize::MAX,
        };
        match p.shape {
            Glyph(ch) => sink.glyph(ch),
            Contours(ch, from, to) => {
                sink.from = usize::from(from);
                sink.to = usize::from(to);
                sink.glyph(ch);
            },
            Rect(x0, y0, x1, y1) => sink.poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]),
            Poly(pts) => sink.poly(pts),
            Path(segs) => {
                use ttf_parser::OutlineBuilder;
                for s in segs {
                    match *s {
                        Seg::M(x, y) => sink.move_to(x, y),
                        Seg::L(x, y) => sink.line_to(x, y),
                        Seg::C(a, b, c, d, x, y) => sink.curve_to(a, b, c, d, x, y),
                        Seg::Z => sink.close(),
                    }
                }
            },
        }
    }
}

/// Applies a part's transform (and contour filter) while collecting outline
/// commands.
struct Sink<'a> {
    out: &'a mut Outline,
    part: Part,
    contour: usize,
    from: usize,
    to: usize,
}

impl Sink<'_> {
    #[inline]
    fn active(&self) -> bool {
        // `contour` is 1-based once the first `move_to` has been seen.
        (self.from + 1..=self.to).contains(&self.contour)
    }

    #[inline]
    fn tx(&self, x: f32) -> f32 {
        self.part.sx * x + self.part.dx
    }

    #[inline]
    fn ty(&self, y: f32) -> f32 {
        self.part.sy * y + self.part.dy
    }

    fn glyph(&mut self, ch: char) {
        let face = fonts_registry::face(FontId::Math);
        if let Some(gid) = face.glyph_index(ch) {
            let _ = face.outline_glyph(gid, self);
        }
    }

    fn poly(&mut self, pts: &[(f32, f32)]) {
        use ttf_parser::OutlineBuilder;
        let Some(&(x, y)) = pts.first() else { return };
        self.move_to(x, y);
        for &(x, y) in &pts[1..] {
            self.line_to(x, y);
        }
        self.close();
    }
}

impl ttf_parser::OutlineBuilder for Sink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contour += 1;
        if self.active() {
            let p = [self.tx(x), self.ty(y)];
            self.out.push(b'M', &p);
        }
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if self.active() {
            let p = [self.tx(x), self.ty(y)];
            self.out.push(b'L', &p);
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if self.active() {
            let p = [self.tx(x1), self.ty(y1), self.tx(x), self.ty(y)];
            self.out.push(b'Q', &p);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if self.active() {
            let p = [
                self.tx(x1),
                self.ty(y1),
                self.tx(x2),
                self.ty(y2),
                self.tx(x),
                self.ty(y),
            ];
            self.out.push(b'C', &p);
        }
    }

    fn close(&mut self) {
        if self.active() {
            self.out.push(b'Z', &[]);
        }
    }
}

/// A fallback glyph ready to become an `SvgPath` layout box: `path` is filled
/// (`F:`-prefixed) path data in em units with its origin at the box's top-left
/// corner, y down — the convention every emitter uses for `SvgPath`.
pub(crate) struct FallbackShape {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub path: String,
}

/// An assembled recipe: advance width plus outline (font units).
struct Assembled {
    advance: f32,
    outline: Outline,
}

/// Assembled outlines keyed by code point. Bounded by the recipe table size,
/// so repeated renders skip re-reading the component glyph outlines.
static ASSEMBLED: LazyLock<RwLock<HashMap<char, Arc<Assembled>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The assembled outline for `ch`'s recipe; a recipe whose components vanished
/// from the font (guarded by tests) yields an empty-but-valid outline.
fn assemble(ch: char) -> Arc<Assembled> {
    if let Some(hit) = ASSEMBLED.read().ok().and_then(|m| m.get(&ch).cloned()) {
        return hit;
    }
    let mut outline = Outline::new();
    let advance = match recipe(ch) {
        Some(r) => {
            for p in r.parts {
                outline.add_part(p);
            }
            r.advance
        },
        None => 0.0,
    };
    if outline.cmds.is_empty() {
        outline.min_y = 0.0;
        outline.max_y = 0.0;
    }
    let built = Arc::new(Assembled { advance, outline });
    if recipe(ch).is_some()
        && let Ok(mut m) = ASSEMBLED.write()
    {
        m.insert(ch, Arc::clone(&built));
    }
    built
}

/// Serialize `outline`, mapping font-unit `(x, y)` to em `(x·sx, (top − y)·sy)`.
fn write_path(outline: &Outline, sx: f64, sy: f64, top: f64) -> String {
    let mut w = PathWriter::with_prefix("F:");
    for (cmd, pts) in &outline.cmds {
        w.cmd(*cmd);
        let n = match cmd {
            b'M' | b'L' => 2,
            b'Q' => 4,
            b'C' => 6,
            _ => 0,
        };
        for &[x, y] in pts[..n].as_chunks::<2>().0 {
            w.push_f64(f64::from(x) * sx);
            w.push_f64((top - f64::from(y)) * sy);
        }
    }
    w.finish()
}

/// Fallback drawing of `ch` at its natural size, for a glyph box of font size
/// `size` (em).
pub(crate) fn glyph_shape(ch: char, size: f64) -> FallbackShape {
    let a = assemble(ch);
    let (advance, outline) = (a.advance, &a.outline);
    let s = size / fonts_registry::font_info(FontId::Math).units_per_em;
    let top = f64::from(outline.max_y.max(0.0));
    let bottom = f64::from(outline.min_y.min(0.0));
    FallbackShape {
        width: f64::from(advance) * s,
        height: top * s,
        depth: -bottom * s,
        path: write_path(outline, s, s, top),
    }
}

/// True when a stretchy delimiter for `ch` (at `target_height` em) has no font
/// glyph or built-in path but does have a recipe. (Without a recipe the
/// delimiter box is kept: SVG emits `<text>`, raster draws notdef.)
#[inline]
pub(crate) fn delimiter_needs_fallback(ch: char, target_height: f64) -> bool {
    needs_fallback(ch, FontId::Math)
        && has_recipe(ch)
        && super::delimiter_paths::delimiter_path(ch, target_height, 1.0).is_none()
}

/// Fallback drawing of a stretchy delimiter: the glyph's ink is stretched
/// vertically to span `height` above and `depth` below the baseline, keeping
/// its natural width at font size `size`.
pub(crate) fn delimiter_shape(ch: char, size: f64, height: f64, depth: f64) -> FallbackShape {
    let a = assemble(ch);
    let (advance, outline) = (a.advance, &a.outline);
    let s = size / fonts_registry::font_info(FontId::Math).units_per_em;
    let ink = f64::from(outline.max_y - outline.min_y).max(1.0);
    let sy = (height + depth).max(0.0) / ink;
    FallbackShape {
        width: f64::from(advance) * s,
        height,
        depth,
        path: write_path(outline, s, sy, f64::from(outline.max_y)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_sorted_unique_and_actually_missing() {
        for w in RECIPES.windows(2) {
            assert!(w[0].ch < w[1].ch, "RECIPES must be sorted: {:?}", w[1].ch);
        }
        for r in RECIPES {
            assert!(
                needs_fallback(r.ch, FontId::Math),
                "U+{:04X} is in the font; its recipe is dead",
                r.ch as u32
            );
        }
    }

    #[test]
    fn recipe_components_exist_in_font() {
        for r in RECIPES {
            for p in r.parts {
                if let Glyph(c) | Contours(c, _, _) = p.shape {
                    assert!(
                        math_has_glyph(c),
                        "U+{:04X} recipe uses missing U+{:04X}",
                        r.ch as u32,
                        c as u32
                    );
                }
            }
            assert!(
                assemble(r.ch).outline.cmds.len() > 4,
                "U+{:04X} recipe drew nothing",
                r.ch as u32
            );
        }
    }

    #[test]
    fn coverage_matches_cmap() {
        let face = fonts_registry::face(FontId::Math);
        for cp in 0..0x3_0000_u32 {
            if let Some(ch) = char::from_u32(cp) {
                assert_eq!(
                    math_has_glyph(ch),
                    face.glyph_index(ch).is_some_and(|g| g.0 != 0),
                    "U+{cp:04X}"
                );
            }
        }
        for ch in (0x21_u8..0x7F).map(char::from) {
            assert!(
                math_has_glyph(ch),
                "printable ASCII {ch:?} must be in the font"
            );
        }
        assert!(!needs_fallback('a', FontId::Math));
        assert!(!needs_fallback('∑', FontId::Math));
        assert!(!needs_fallback(' ', FontId::Math));
        assert!(!needs_fallback('\u{2003}', FontId::Math));
        assert!(needs_fallback('\u{2A74}', FontId::Math));
        assert!(needs_fallback('\u{2A74}', FontId::RomanRegular));
    }

    #[test]
    fn resolution_order() {
        for w in SUBSTITUTES.windows(2) {
            assert!(w[0].0 < w[1].0, "SUBSTITUTES must be sorted");
        }
        for &(from, to) in SUBSTITUTES {
            assert!(needs_fallback(from, FontId::Math), "U+{:04X}", from as u32);
            assert!(math_has_glyph(to), "U+{:04X}", to as u32);
        }
        assert_eq!(resolve('\u{FE37}', false), Resolution::Substitute('⏞'));
        assert_eq!(resolve('\u{203E}', false), Resolution::Substitute('¯'));
        assert_eq!(
            resolve('\u{0192}', false),
            Resolution::TextFace(FontId::RomanRegular)
        );
        assert_eq!(resolve('\u{2A74}', false), Resolution::Recipe);
        assert_eq!(resolve('\u{4E2D}', false), Resolution::Notdef);
        assert!(text_run_needs_rewrite('★', FontId::RomanRegular));
        assert!(text_run_needs_rewrite('\u{FE37}', FontId::RomanRegular));
        assert!(!text_run_needs_rewrite('a', FontId::RomanRegular));
        assert!(!text_run_needs_rewrite('\u{4E2D}', FontId::RomanRegular));
    }

    #[test]
    fn recipe_shape_is_cached_and_valid() {
        let a = glyph_shape('\u{2605}', 1.0);
        let b = glyph_shape('\u{2605}', 1.0);
        assert_eq!(a.path, b.path);
        assert!(a.width > 0.0 && a.height > 0.0);
        assert!(a.path.starts_with("F:M"));
    }

    #[test]
    fn shape_scales_with_size() {
        let a = glyph_shape('\u{2A74}', 1.0);
        let b = glyph_shape('\u{2A74}', 2.0);
        assert!((b.width - 2.0 * a.width).abs() < 1e-9);
        assert!((b.height - 2.0 * a.height).abs() < 1e-6);
    }

    /// Every code point produced by the LaTeX symbol tables must put ink on
    /// the PNG — guards against glyphs silently missing from the fonts.
    #[cfg(feature = "raster")]
    #[test]
    fn every_latex_symbol_renders_ink() {
        use crate::latex::packages as pk;

        let mut cmds: Vec<(String, String)> = Vec::new();
        // Core table: scan the `LATEX_TO_UNICODE` source for its keys and
        // resolve each through the public lookup (the map itself is private).
        let src = include_str!("../unicode/symbols.rs");
        let start = src
            .find("static LATEX_TO_UNICODE")
            .expect("LATEX_TO_UNICODE table");
        let body = &src[start..];
        let body = &body[..body.find("\n};").expect("table end")];
        for line in body.lines() {
            let line = line.trim_start();
            let Some(rest) = line.strip_prefix('"') else {
                continue;
            };
            let Some(end) = rest.find('"') else { continue };
            let name = &rest[..end];
            if let Some((ch, _)) = crate::unicode::symbols::lookup_command(name) {
                cmds.push((name.to_owned(), ch.to_string()));
            }
        }
        assert!(cmds.len() > 300, "scanned only {} core symbols", cmds.len());
        let tables: &[&[(&str, &str, pk::SymbolClass)]] = &[
            pk::amsbsy::SYMBOLS,
            pk::amsfonts::SYMBOLS,
            pk::amsmath::SYMBOLS,
            pk::amssymb::SYMBOLS,
            pk::bbm::SYMBOLS,
            pk::bboldx::SYMBOLS,
            pk::braket::SYMBOLS,
            pk::centernot::SYMBOLS,
            pk::dsfont::SYMBOLS,
            pk::extpfeil::SYMBOLS,
            pk::gensymb::SYMBOLS,
            pk::mathtools::SYMBOLS,
            pk::physics::SYMBOLS,
            pk::textcomp::SYMBOLS,
            pk::units::SYMBOLS,
            pk::upgreek::SYMBOLS,
        ];
        for table in tables {
            for &(name, text, _) in *table {
                cmds.push((name.to_owned(), text.to_owned()));
            }
        }

        let mut blank = Vec::new();
        let mut notdef = Vec::new();
        let mut unparsed = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (name, text) in &cmds {
            // Notdef has ink in the PNG, so check resolution separately: every
            // table character must come from a font, text face or recipe.
            for c in text.chars() {
                if needs_fallback(c, FontId::Math) && resolve(c, false) == Resolution::Notdef {
                    notdef.push(format!("\\{name} (U+{:04X})", c as u32));
                }
            }
            if text.chars().all(is_inkless) || !seen.insert(text.clone()) {
                continue;
            }
            // Accents etc. need an argument; try bare first, then with `{}`.
            let parsed = crate::from_latex(&format!("\\{name}"))
                .or_else(|_| crate::from_latex(&format!("\\{name}{{}}")));
            let Ok(parsed) = parsed else {
                unparsed.push(format!("\\{name}"));
                continue;
            };
            let png = parsed.to_png().expect("png render");
            if ink_pixels(&png) == 0 {
                let cps: Vec<String> = text
                    .chars()
                    .map(|c| format!("U+{:04X}", c as u32))
                    .collect();
                blank.push(format!("\\{name} ({text} {})", cps.join(" ")));
            }
        }
        assert!(
            blank.is_empty(),
            "{} symbol(s) render no ink:\n  {}",
            blank.len(),
            blank.join("\n  ")
        );
        assert!(
            notdef.is_empty(),
            "{} symbol(s) only resolve to a notdef box:\n  {}",
            notdef.len(),
            notdef.join("\n  ")
        );
        assert!(
            unparsed.is_empty(),
            "{} symbol command(s) fail to parse:\n  {}",
            unparsed.len(),
            unparsed.join("\n  ")
        );
    }

    #[cfg(feature = "raster")]
    pub(super) fn ink_pixels(png: &[u8]) -> usize {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .expect("png header");
        let mut buf = vec![0; reader.output_buffer_size().expect("png size")];
        let info = reader.next_frame(&mut buf).expect("png frame");
        let px = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            _ => 1,
        };
        buf[..info.buffer_size()]
            .chunks_exact(px)
            .filter(|p| match px {
                4 | 2 => p[px - 1] > 32,
                _ => p[0] < 224,
            })
            .count()
    }
}

#[cfg(all(test, feature = "raster"))]
mod render_tests {
    /// Notdef characters stay `<text>` in both SVG outputs (browser fonts can
    /// still show them) while recipe characters become identical paths.
    #[test]
    fn notdef_stays_text_in_svg_and_recipes_become_paths() {
        for tex in [
            r"x_{最大}",
            r"\mathrm{中}",
            r"\text{中}",
            r"\operatorname{中}",
        ] {
            let r = crate::from_latex(tex).unwrap();
            let cjk = tex.chars().find(|c| !c.is_ascii()).unwrap();
            assert!(r.to_svg().unwrap().contains(cjk), "{tex}: to_svg");
            assert!(r.to_svg_text().unwrap().contains(cjk), "{tex}: to_svg_text");
        }
        let r = crate::from_latex(r"\text{a★b}").unwrap();
        for svg in [r.to_svg().unwrap(), r.to_svg_text().unwrap()] {
            assert!(!svg.contains('★'), "recipe char must be drawn as a path");
            assert!(svg.contains("<path"), "recipe path missing");
        }
    }

    fn ink(tex: &str) -> usize {
        let png = crate::from_latex(tex).unwrap().to_png().unwrap();
        super::tests::ink_pixels(&png)
    }

    /// Raster never leaves a gap: recipe and notdef characters in math and in
    /// text runs all put ink on the PNG.
    #[test]
    fn raster_draws_text_runs_and_notdef() {
        for tex in [
            r"\text{★}",
            r"\text{⩴}",
            r"\operatorname{中}",
            r"x_{最大}",
            r"\mathrm{中}",
            "\u{FE37}",
            r"\text{︷}",
        ] {
            assert!(ink(tex) > 0, "{tex} renders no ink");
        }
    }

    /// MathML presentation forms render exactly like their canonical
    /// characters (stretchy braces, overline accent), not as notdef.
    #[test]
    fn mathml_overbar_and_presentation_braces() {
        let svg = |mml: String| crate::from_mathml(&mml).unwrap().to_svg().unwrap();
        let over = |base: &str, op: &str, tag: &str, attr: &str| {
            format!("<math><{tag}{attr}>{base}<mo>{op}</mo></{tag}></math>")
        };
        let ab = "<mrow><mi>a</mi><mi>b</mi></mrow>";
        for (form, canon, tag) in [
            ("&#xFE37;", "&#x23DE;", "mover"),
            ("&#xFE38;", "&#x23DF;", "munder"),
            ("&#xFE35;", "&#x23DC;", "mover"),
            ("&#xFE36;", "&#x23DD;", "munder"),
        ] {
            assert_eq!(
                svg(over(ab, form, tag, "")),
                svg(over(ab, canon, tag, "")),
                "{form}"
            );
        }
        let y = "<mi>y</mi>";
        let acc = r#" accent="true""#;
        assert_eq!(
            svg(over(y, "&OverBar;", "mover", acc)),
            svg(over(y, "&#x0304;", "mover", acc))
        );
    }
}
