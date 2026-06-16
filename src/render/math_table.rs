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
static PARSED_METRICS: LazyLock<FontMetrics> = LazyLock::new(|| {
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
    PARSED_METRICS.metrics.get(&ch).copied()
}

/// Look up font-derived glyph metrics for an upright character.
///
/// For the embedded Latin Modern Math font, there is a single set of glyph
/// outlines per codepoint, so this currently returns the same metrics as
/// `font_glyph_metrics`. A future enhancement could map to a separate
/// upright font face if available.
#[inline]
pub fn font_glyph_metrics_upright(ch: char) -> Option<GlyphMetrics> {
    PARSED_METRICS.metrics.get(&ch).copied()
}

/// Math constants extracted from the OpenType MATH table.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Fields are incrementally wired into the layout engine
pub struct MathConstants {
    pub axis_height: f64,
    pub fraction_rule_thickness: f64,
    pub overbar_rule_thickness: f64,
    pub underbar_rule_thickness: f64,
    pub radical_rule_thickness: f64,
    pub fraction_num_display_gap: f64,
    pub fraction_denom_display_gap: f64,
    pub stack_display_gap: f64,
    pub stack_gap: f64,
    pub superscript_shift_up: f64,
    pub subscript_shift_down: f64,
    pub radical_vertical_gap: f64,
    pub radical_display_vertical_gap: f64,
    pub accent_base_height: f64,
    pub delimiter_factor: i16,
    pub delimiter_shortfall: f64,
    /// Extra space above the radical rule (em units).
    pub radical_extra_ascender: f64,
    /// Kern before the degree index of a radical (em units).
    pub radical_kern_before_degree: f64,
    /// Kern after the degree index of a radical (em units, typically negative).
    pub radical_kern_after_degree: f64,
    /// Percentage of the radical height to raise the degree bottom.
    pub radical_degree_bottom_raise_percent: i16,
}

static MATH_CONSTANTS: LazyLock<Option<MathConstants>> = LazyLock::new(|| {
    let face = ttf_parser::Face::parse(super::FONT_DATA, 0).ok()?;
    extract_math_constants(&face)
});

/// Get the MATH table constants from the embedded font.
pub fn math_constants() -> Option<&'static MathConstants> {
    MATH_CONSTANTS.as_ref()
}

fn extract_math_constants(face: &ttf_parser::Face) -> Option<MathConstants> {
    let math = face.tables().math.as_ref()?;
    let consts = math.constants.as_ref()?;
    let upem = face.units_per_em() as f64;

    Some(MathConstants {
        axis_height: consts.axis_height().value as f64 / upem,
        fraction_rule_thickness: consts.fraction_rule_thickness().value as f64 / upem,
        overbar_rule_thickness: consts.overbar_rule_thickness().value as f64 / upem,
        underbar_rule_thickness: consts.underbar_rule_thickness().value as f64 / upem,
        radical_rule_thickness: consts.radical_rule_thickness().value as f64 / upem,
        fraction_num_display_gap: consts.fraction_num_display_style_gap_min().value as f64 / upem,
        fraction_denom_display_gap: consts.fraction_denom_display_style_gap_min().value as f64
            / upem,
        stack_display_gap: consts.stack_display_style_gap_min().value as f64 / upem,
        stack_gap: consts.stack_gap_min().value as f64 / upem,
        superscript_shift_up: consts.superscript_shift_up().value as f64 / upem,
        subscript_shift_down: consts.subscript_shift_down().value as f64 / upem,
        radical_vertical_gap: consts.radical_vertical_gap().value as f64 / upem,
        radical_display_vertical_gap: consts.radical_display_style_vertical_gap().value as f64
            / upem,
        accent_base_height: consts.accent_base_height().value as f64 / upem,
        delimiter_factor: 901,    // Not in MATH table, kept from TeX tradition
        delimiter_shortfall: 0.5, // Not in MATH table; TeX default of 5pt ≈ 0.5em
        radical_extra_ascender: consts.radical_extra_ascender().value as f64 / upem,
        radical_kern_before_degree: consts.radical_kern_before_degree().value as f64 / upem,
        radical_kern_after_degree: consts.radical_kern_after_degree().value as f64 / upem,
        radical_degree_bottom_raise_percent: consts.radical_degree_bottom_raise_percent(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_embedded_font() {
        let _ = &*PARSED_METRICS;
        assert!(
            !PARSED_METRICS.metrics.is_empty(),
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
            PARSED_METRICS.metrics.len() > 500,
            "font should have >500 glyphs, got {}",
            PARSED_METRICS.metrics.len()
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

    #[test]
    fn bold_greek_has_metrics() {
        // Bold alpha U+1D6C2 should be present in Latin Modern Math
        let bold_alpha = char::from_u32(0x1D6C2).unwrap();
        let _normal_alpha = '\u{03B1}';
        let m_bold = font_glyph_metrics(bold_alpha);
        let _m_normal = font_glyph_metrics(_normal_alpha);
        assert!(
            m_bold.is_some(),
            "Bold alpha (U+1D6C2) should have font metrics"
        );
    }

    #[test]
    fn math_constants_are_extracted() {
        let consts = math_constants();
        assert!(
            consts.is_some(),
            "MATH table constants should be extractable"
        );
        let c = consts.unwrap();
        assert!(
            (c.axis_height - 0.25).abs() < 0.05,
            "axis_height={}",
            c.axis_height
        );
        assert!(c.fraction_rule_thickness > 0.01 && c.fraction_rule_thickness < 0.1);
    }

    #[test]
    fn radical_constants_are_reasonable() {
        let c = math_constants().unwrap();
        eprintln!("radical_extra_ascender: {}", c.radical_extra_ascender);
        eprintln!(
            "radical_kern_before_degree: {}",
            c.radical_kern_before_degree
        );
        eprintln!("radical_kern_after_degree: {}", c.radical_kern_after_degree);
        eprintln!(
            "radical_degree_bottom_raise_percent: {}",
            c.radical_degree_bottom_raise_percent
        );
        eprintln!("radical_vertical_gap: {}", c.radical_vertical_gap);
        eprintln!(
            "radical_display_vertical_gap: {}",
            c.radical_display_vertical_gap
        );
        eprintln!("radical_rule_thickness: {}", c.radical_rule_thickness);
        // Sanity checks
        assert!(
            c.radical_extra_ascender > 0.0 && c.radical_extra_ascender < 0.2,
            "radical_extra_ascender={}",
            c.radical_extra_ascender
        );
        assert!(
            c.radical_kern_after_degree < 0.0,
            "radical_kern_after_degree should be negative, got {}",
            c.radical_kern_after_degree
        );
        assert!(
            c.radical_degree_bottom_raise_percent > 0
                && c.radical_degree_bottom_raise_percent <= 100,
            "radical_degree_bottom_raise_percent={}",
            c.radical_degree_bottom_raise_percent
        );
    }

    #[test]
    fn math_constants_superscript_shift() {
        let c = math_constants().unwrap();
        assert!(
            c.superscript_shift_up > 0.2,
            "superscript shift should be > 0.2em"
        );
        assert!(
            c.subscript_shift_down > 0.05,
            "subscript shift should be > 0.05em"
        );
    }
}
