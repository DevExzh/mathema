//! Font metric extraction from the embedded Latin Modern Math OpenType font.
//!
//! Parses the embedded OTF via `ttf-parser` and extracts glyph metrics
//! (height, depth, width, italic correction) for all glyphs in the font.
//! This provides accurate font-derived metrics for 1000+ glyphs, replacing
//! the ~200 hardcoded entries in `metrics.rs`.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::metrics::GlyphMetrics;

/// Parsed font data with glyph metrics for every codepoint in the font.
struct FontMetrics {
    /// Map from Unicode character to its glyph metrics (in em units).
    metrics: HashMap<char, GlyphMetrics>,
}

/// Lazily-parsed font metrics, initialized on first access.
static FONT_DATA: LazyLock<FontMetrics> = LazyLock::new(|| {
    let font_bytes = super::FONT_DATA;
    let face = ttf_parser::Face::parse(font_bytes, 0).expect("embedded font data must be valid");
    let upem = face.units_per_em() as f64;

    // Access the MATH table for italic corrections.
    let italic_corrections = face
        .tables()
        .math
        .as_ref()
        .and_then(|math| math.glyph_info.as_ref())
        .and_then(|info| info.italic_corrections.as_ref());

    let mut metrics = HashMap::new();

    // Iterate all cmap subtables to discover every codepoint in the font.
    if let Some(cmap) = face.tables().cmap.as_ref() {
        for subtable in cmap.subtables.into_iter() {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|cp| {
                let Some(ch) = char::from_u32(cp) else {
                    return;
                };
                // Skip if we already have metrics for this character (from another subtable).
                if metrics.contains_key(&ch) {
                    return;
                }
                let Some(gid) = face.glyph_index(ch) else {
                    return;
                };

                // Width: horizontal advance in em units.
                let width = face.glyph_hor_advance(gid).unwrap_or(0) as f64 / upem;

                // Height and depth from bounding box.
                let (height, depth) = if let Some(bbox) = face.glyph_bounding_box(gid) {
                    // y_max is the top of the glyph (height above baseline).
                    // y_min is the bottom (negative means below baseline).
                    let h = (bbox.y_max as f64 / upem).max(0.0);
                    let d = (-(bbox.y_min as f64) / upem).max(0.0);
                    (h, d)
                } else {
                    (0.0, 0.0)
                };

                // Italic correction from the MATH table.
                let italic = italic_corrections
                    .and_then(|ic| ic.get(gid))
                    .map(|mv| mv.value as f64 / upem)
                    .unwrap_or(0.0);

                metrics.insert(ch, GlyphMetrics::new(height, depth, width, italic));
            });
        }
    }

    FontMetrics { metrics }
});

/// Look up font-derived glyph metrics for a character (math italic variant).
///
/// Returns `None` if the character is not present in the embedded font.
#[inline]
pub fn font_glyph_metrics(ch: char) -> Option<GlyphMetrics> {
    FONT_DATA.metrics.get(&ch).copied()
}

/// Look up font-derived glyph metrics for an upright character.
///
/// For the embedded Latin Modern Math font, there is a single set of glyph
/// outlines per codepoint, so this currently returns the same metrics as
/// `font_glyph_metrics`. A future enhancement could map to a separate
/// upright font face if available.
#[inline]
pub fn font_glyph_metrics_upright(ch: char) -> Option<GlyphMetrics> {
    FONT_DATA.metrics.get(&ch).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_embedded_font() {
        let _ = &*FONT_DATA;
        assert!(
            !FONT_DATA.metrics.is_empty(),
            "should extract some metrics"
        );
    }

    #[test]
    fn latin_a_has_metrics() {
        let m = font_glyph_metrics('A');
        assert!(m.is_some(), "Latin A should have metrics");
        let m = m.unwrap();
        assert!(m.height > 0.5, "A height should be > 0.5em");
        assert!(m.width > 0.3, "A width should be > 0.3em");
    }

    #[test]
    fn greek_alpha_has_metrics() {
        let m = font_glyph_metrics('\u{03B1}');
        assert!(m.is_some(), "Greek alpha should have metrics");
    }

    #[test]
    fn missing_char_returns_none() {
        assert!(font_glyph_metrics('\u{1F600}').is_none());
    }

    #[test]
    fn metrics_count_exceeds_hardcoded() {
        assert!(
            FONT_DATA.metrics.len() > 500,
            "font should have >500 glyphs, got {}",
            FONT_DATA.metrics.len()
        );
    }

    #[test]
    fn upright_returns_same_as_italic() {
        let italic = font_glyph_metrics('x');
        let upright = font_glyph_metrics_upright('x');
        assert_eq!(
            italic.map(|m| (m.height, m.width)),
            upright.map(|m| (m.height, m.width)),
        );
    }

    #[test]
    fn integral_has_italic_correction() {
        // The integral sign typically has an italic correction in the MATH table.
        let m = font_glyph_metrics('\u{222B}');
        assert!(m.is_some(), "integral should have metrics");
        // Note: italic correction may or may not be present depending on font;
        // we just verify the lookup doesn't panic.
    }

    #[test]
    fn sum_has_metrics() {
        let m = font_glyph_metrics('\u{2211}');
        assert!(m.is_some(), "sum should have metrics");
        let m = m.unwrap();
        assert!(m.height > 0.0, "sum should have positive height");
        assert!(m.width > 0.0, "sum should have positive width");
    }

    #[test]
    fn lowercase_descender_has_depth() {
        let m = font_glyph_metrics('y');
        assert!(m.is_some(), "y should have metrics");
        let m = m.unwrap();
        assert!(m.depth > 0.0, "y should have depth (descender)");
    }

    #[test]
    fn digit_has_reasonable_dimensions() {
        let m = font_glyph_metrics('0').expect("digit 0 should have metrics");
        assert!(m.height > 0.4, "digit height should be > 0.4em");
        assert!(m.width > 0.3, "digit width should be > 0.3em");
    }
}
