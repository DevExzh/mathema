//! Glyph outline extraction from OpenType fonts via `ttf-parser`.
//!
//! Converts font glyph outlines into SVG `<path>` `d` attribute strings.
//! Coordinates are in font units with y-axis negated (font y-up → SVG y-down).

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{LazyLock, RwLock};

/// Extracted glyph outline ready for SVG embedding.
#[derive(Debug, Clone)]
pub struct GlyphOutline {
    /// SVG path `d` attribute string in font units (y-negated).
    pub path_data: String,
    /// Horizontal advance width in font units.
    pub advance_width: f64,
}

/// Font-level metrics needed for coordinate transforms.
#[derive(Debug, Clone, Copy)]
pub struct FontInfo {
    /// Font design units per em-square (typically 1000 or 2048).
    pub units_per_em: f64,
}

/// Cached parsed face for the embedded font, avoiding re-parsing per glyph.
static CACHED_FACE: LazyLock<ttf_parser::Face<'static>> = LazyLock::new(|| {
    ttf_parser::Face::parse(super::FONT_DATA, 0).expect("embedded font data must be valid")
});

/// Cache of extracted outlines for the embedded font, keyed by character.
///
/// Outline extraction (CFF charstring interpretation + path-string building)
/// dominates the embedded-SVG render path (~7 µs/glyph) and the embedded font
/// is static, so caching turns repeated renders into cheap clones.
static GLYPH_CACHE: LazyLock<RwLock<HashMap<char, Option<GlyphOutline>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Cache of extracted outlines for the embedded font, keyed by glyph id
/// (MATH-table size variants for delimiters / large operators).
static VARIANT_CACHE: LazyLock<RwLock<HashMap<u16, Option<GlyphOutline>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Cached font info for the embedded font.
static CACHED_FONT_INFO: LazyLock<FontInfo> = LazyLock::new(|| FontInfo {
    units_per_em: f64::from(CACHED_FACE.units_per_em()),
});

/// Parse font-level metrics from raw font data.
///
/// Returns `None` if the font data is malformed.
pub fn parse_font_info(font_data: &[u8]) -> Option<FontInfo> {
    // Fast path for the embedded font.
    if std::ptr::eq(font_data, super::FONT_DATA) {
        return Some(*CACHED_FONT_INFO);
    }
    let face = ttf_parser::Face::parse(font_data, 0).ok()?;
    Some(FontInfo {
        units_per_em: f64::from(face.units_per_em()),
    })
}

/// Extract a single glyph's outline from the font.
///
/// Returns `None` if the character has no glyph or no outline (e.g., space).
pub fn extract_glyph(font_data: &[u8], ch: char) -> Option<GlyphOutline> {
    // Fast path for the embedded font: use cached Face + outline cache.
    if std::ptr::eq(font_data, super::FONT_DATA) {
        if let Some(hit) = GLYPH_CACHE.read().unwrap().get(&ch) {
            return hit.clone();
        }
        let outline = extract_glyph_from_face(&CACHED_FACE, ch);
        GLYPH_CACHE.write().unwrap().insert(ch, outline.clone());
        return outline;
    }
    let face = ttf_parser::Face::parse(font_data, 0).ok()?;
    extract_glyph_from_face(&face, ch)
}

fn extract_glyph_from_face(face: &ttf_parser::Face<'_>, ch: char) -> Option<GlyphOutline> {
    let glyph_id = face.glyph_index(ch)?;
    let advance = face.glyph_hor_advance(glyph_id)?;

    let mut builder = SvgPathBuilder::new();
    face.outline_glyph(glyph_id, &mut builder)?;

    let path_data = builder.finish();
    if path_data.is_empty() {
        return None;
    }

    Some(GlyphOutline {
        path_data,
        advance_width: f64::from(advance),
    })
}

/// Extract a glyph outline directly by its font glyph index.
///
/// Used for OpenType MATH-table size variants (large operators, stretchy
/// delimiters) whose glyph IDs are not reachable through the cmap. The path
/// data uses the same y-negated font-unit coordinate space as [`extract_glyph`],
/// so it can be placed with the same positive-scale transform.
///
/// Returns `None` if the glyph has no outline.
pub fn extract_glyph_by_id(font_data: &[u8], glyph_id: u16) -> Option<GlyphOutline> {
    if std::ptr::eq(font_data, super::FONT_DATA) {
        if let Some(hit) = VARIANT_CACHE.read().unwrap().get(&glyph_id) {
            return hit.clone();
        }
        let outline = extract_glyph_by_id_from_face(&CACHED_FACE, glyph_id);
        VARIANT_CACHE.write().unwrap().insert(glyph_id, outline.clone());
        return outline;
    }
    let face = ttf_parser::Face::parse(font_data, 0).ok()?;
    extract_glyph_by_id_from_face(&face, glyph_id)
}

fn extract_glyph_by_id_from_face(
    face: &ttf_parser::Face<'_>,
    glyph_id: u16,
) -> Option<GlyphOutline> {
    let gid = ttf_parser::GlyphId(glyph_id);
    let advance = face.glyph_hor_advance(gid).unwrap_or(0);

    let mut builder = SvgPathBuilder::new();
    face.outline_glyph(gid, &mut builder)?;

    let path_data = builder.finish();
    if path_data.is_empty() {
        return None;
    }

    Some(GlyphOutline {
        path_data,
        advance_width: f64::from(advance),
    })
}

/// Builds an SVG path `d` attribute string from `ttf_parser::OutlineBuilder` callbacks.
/// Y-coordinates are negated to convert from font coordinates (y-up) to SVG (y-down).
struct SvgPathBuilder {
    buf: String,
    first: bool,
}

impl SvgPathBuilder {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(256),
            first: true,
        }
    }

    fn finish(self) -> String {
        self.buf
    }
}

/// Format a font-unit coordinate compactly: integer when exact, 1 decimal otherwise.
fn fmt_coord(v: f32) -> CompactCoord {
    CompactCoord(v)
}

struct CompactCoord(f32);

impl std::fmt::Display for CompactCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = self.0;
        let rounded = (v * 10.0).round() / 10.0;
        if (rounded - rounded.round()).abs() < 0.01 {
            write!(f, "{}", rounded as i32)
        } else {
            write!(f, "{rounded:.1}")
        }
    }
}

impl ttf_parser::OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.first {
            self.buf.push(' ');
        }
        self.first = false;
        let _ = write!(self.buf, "M {} {}", fmt_coord(x), fmt_coord(-y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.buf, " L {} {}", fmt_coord(x), fmt_coord(-y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(
            self.buf,
            " Q {} {} {} {}",
            fmt_coord(x1),
            fmt_coord(-y1),
            fmt_coord(x),
            fmt_coord(-y),
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(
            self.buf,
            " C {} {} {} {} {} {}",
            fmt_coord(x1),
            fmt_coord(-y1),
            fmt_coord(x2),
            fmt_coord(-y2),
            fmt_coord(x),
            fmt_coord(-y),
        );
    }

    fn close(&mut self) {
        self.buf.push('Z');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

    #[test]
    fn parse_font_info_returns_valid_metrics() {
        let info = parse_font_info(FONT_DATA).expect("valid font");
        assert!((info.units_per_em - 1000.0).abs() < 1.0);
    }

    #[test]
    fn parse_font_info_rejects_garbage() {
        assert!(parse_font_info(b"not a font").is_none());
    }

    #[test]
    fn extract_glyph_for_letter_x() {
        let outline = extract_glyph(FONT_DATA, 'x').expect("x should have outline");
        assert!(
            !outline.path_data.is_empty(),
            "path data should not be empty"
        );
        assert!(
            outline.path_data.starts_with('M'),
            "path should start with M command"
        );
        assert!(outline.path_data.contains('Z'), "path should be closed");
        assert!(
            outline.advance_width > 0.0,
            "advance width should be positive"
        );
    }

    #[test]
    fn extract_glyph_for_digit_2() {
        let outline = extract_glyph(FONT_DATA, '2').expect("2 should have outline");
        assert!(outline.path_data.starts_with('M'));
        assert!(outline.advance_width > 0.0);
    }

    #[test]
    fn extract_glyph_for_plus() {
        let outline = extract_glyph(FONT_DATA, '+').expect("+ should have outline");
        assert!(outline.path_data.starts_with('M'));
    }

    #[test]
    fn extract_glyph_for_space_returns_none() {
        assert!(extract_glyph(FONT_DATA, ' ').is_none());
    }

    #[test]
    fn extract_glyph_for_nonexistent_char_returns_none() {
        assert!(extract_glyph(FONT_DATA, '\u{F8FF}').is_none());
    }

    #[test]
    fn path_data_has_negated_y() {
        let outline = extract_glyph(FONT_DATA, 'A').expect("A should have outline");
        let parts: Vec<&str> = outline.path_data.split_whitespace().collect();
        assert_eq!(parts[0], "M");
        // 'A' extends above baseline, so at least some y-coordinates must be negative
        // after negation (font y-up → SVG y-down). The first point may be at y=0
        // (baseline), so we check that negative y values exist anywhere in the path.
        let has_negative_y = parts
            .iter()
            .any(|s| s.parse::<f64>().map(|v| v < -1.0).unwrap_or(false));
        assert!(
            has_negative_y,
            "path should contain negated y values for glyph above baseline"
        );
    }

    #[test]
    fn path_data_coordinates_are_compact() {
        let outline = extract_glyph(FONT_DATA, 'x').expect("x should have outline");
        assert!(
            !outline.path_data.contains(".0 "),
            "integer coords should not have .0 suffix"
        );
        assert!(
            !outline.path_data.contains(".00"),
            "no excessive decimal places"
        );
    }

    #[test]
    fn extract_glyph_for_greek_alpha() {
        let outline = extract_glyph(FONT_DATA, 'α');
        assert!(outline.is_some(), "α should have outline in LM Math");
    }

    #[test]
    fn extract_glyph_for_integral() {
        let outline = extract_glyph(FONT_DATA, '∫');
        assert!(outline.is_some(), "∫ should have outline in LM Math");
    }
}
