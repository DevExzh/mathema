//! OpenType MATH table glyph variant lookup.
//!
//! Extracts vertical and horizontal glyph variants from the embedded Latin Modern Math
//! font's MATH table. This allows the renderer to select properly designed larger glyph
//! variants for delimiters and large operators instead of simply scaling the base glyph.
//!
//! The MATH table provides two kinds of size variants:
//! 1. **Pre-designed variants**: Complete glyphs at specific larger sizes
//! 2. **Glyph assembly**: Recipes for constructing arbitrarily tall glyphs from parts
//!
//! This module currently implements (1) — pre-designed variant selection.

use std::sync::LazyLock;

/// A pre-designed glyph variant from the MATH table.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // fields used only when `raster` feature is enabled
pub struct VariantGlyph {
    /// The glyph ID in the font (for use with fontdue's `rasterize_indexed`).
    pub glyph_id: u16,
    /// The advance measurement in em units (height for vertical, width for horizontal).
    pub advance_em: f64,
    /// Glyph bounding-box top (y-max) above the baseline, in em units (nominal scale).
    pub y_max_em: f64,
    /// Glyph bounding-box bottom (y-min) relative to the baseline, in em units
    /// (negative when the glyph descends below the baseline).
    pub y_min_em: f64,
    /// Horizontal advance of the glyph, in em units (nominal scale).
    pub h_adv_em: f64,
}

/// Parsed vertical and horizontal glyph variants for all covered glyphs.
#[allow(dead_code)] // used only when `raster` feature is enabled
struct VariantTable {
    /// Map from base character to its sorted list of vertical variants
    /// (sorted by ascending advance_em).
    vertical: Vec<(char, Vec<VariantGlyph>)>,
    /// Map from base character to its sorted list of horizontal variants
    /// (sorted by ascending advance_em).
    horizontal: Vec<(char, Vec<VariantGlyph>)>,
    /// The display_operator_min_height from the MATH constants, in em units.
    display_op_min_height: f64,
}

#[allow(dead_code)] // used by pub functions that are feature-gated consumers
static VARIANT_TABLE: LazyLock<VariantTable> = LazyLock::new(|| {
    let font_bytes = super::FONT_DATA;
    let face = ttf_parser::Face::parse(font_bytes, 0).expect("embedded font data must be valid");
    let upem = face.units_per_em() as f64;

    let math = match face.tables().math.as_ref() {
        Some(m) => m,
        None => {
            return VariantTable {
                vertical: Vec::new(),
                horizontal: Vec::new(),
                display_op_min_height: 0.0,
            };
        },
    };

    let display_op_min_height = math
        .constants
        .as_ref()
        .map(|c| c.display_operator_min_height() as f64 / upem)
        .unwrap_or(0.0);

    let mut vertical = Vec::new();
    let mut horizontal = Vec::new();

    // Build a `VariantGlyph`, looking up the variant glyph's bounding box and
    // horizontal advance (needed so the layout can position e.g. a radical
    // surd precisely relative to its vinculum).
    let mk_variant = |gid: ttf_parser::GlyphId, advance_em: f64| -> VariantGlyph {
        let (y_max_em, y_min_em) = face
            .glyph_bounding_box(gid)
            .map(|b| (b.y_max as f64 / upem, b.y_min as f64 / upem))
            .unwrap_or((0.0, 0.0));
        let h_adv_em = face.glyph_hor_advance(gid).unwrap_or(0) as f64 / upem;
        VariantGlyph {
            glyph_id: gid.0,
            advance_em,
            y_max_em,
            y_min_em,
            h_adv_em,
        }
    };

    if let Some(ref variants) = math.variants {
        // Iterate all cmap entries to find characters with vertical/horizontal variants.
        if let Some(cmap) = face.tables().cmap.as_ref() {
            for subtable in cmap.subtables.into_iter() {
                if !subtable.is_unicode() {
                    continue;
                }
                subtable.codepoints(|cp| {
                    let Some(ch) = char::from_u32(cp) else {
                        return;
                    };
                    let Some(gid) = face.glyph_index(ch) else {
                        return;
                    };
                    // Check if this glyph has vertical variants.
                    if let Some(construction) = variants.vertical_constructions.get(gid) {
                        let mut glyph_variants = Vec::new();
                        for i in 0..construction.variants.len() {
                            let Some(v) = construction.variants.get(i) else {
                                continue;
                            };
                            glyph_variants.push(mk_variant(
                                v.variant_glyph,
                                v.advance_measurement as f64 / upem,
                            ));
                        }
                        if !glyph_variants.is_empty() {
                            // Sort by advance (should already be sorted per spec, but be safe).
                            glyph_variants.sort_by(|a, b| a.advance_em.total_cmp(&b.advance_em));
                            vertical.push((ch, glyph_variants));
                        }
                    }
                    // Check if this glyph has horizontal variants.
                    if let Some(construction) = variants.horizontal_constructions.get(gid) {
                        let mut glyph_variants = Vec::new();
                        for i in 0..construction.variants.len() {
                            let Some(v) = construction.variants.get(i) else {
                                continue;
                            };
                            glyph_variants.push(mk_variant(
                                v.variant_glyph,
                                v.advance_measurement as f64 / upem,
                            ));
                        }
                        if !glyph_variants.is_empty() {
                            glyph_variants.sort_by(|a, b| a.advance_em.total_cmp(&b.advance_em));
                            horizontal.push((ch, glyph_variants));
                        }
                    }
                });
            }
        }
    }

    // Sort by character for binary search.
    vertical.sort_by_key(|(ch, _)| *ch);
    // Deduplicate (multiple cmap subtables may list the same character).
    vertical.dedup_by_key(|(ch, _)| *ch);

    horizontal.sort_by_key(|(ch, _)| *ch);
    horizontal.dedup_by_key(|(ch, _)| *ch);

    VariantTable {
        vertical,
        horizontal,
        display_op_min_height,
    }
});

/// Select the best vertical glyph variant for a character at the given target height.
///
/// Returns the smallest variant whose advance is >= `target_height_em`, or the largest
/// available variant if none is large enough.
///
/// Returns `None` if no variants exist for this character.
#[allow(dead_code)] // called from raster.rs (feature-gated)
pub fn select_vertical_variant(ch: char, target_height_em: f64) -> Option<VariantGlyph> {
    let table = &*VARIANT_TABLE;
    let idx = table.vertical.binary_search_by_key(&ch, |(c, _)| *c).ok()?;
    let variants = &table.vertical[idx].1;
    if variants.is_empty() {
        return None;
    }

    // Find the smallest variant that meets the target height.
    for v in variants {
        if v.advance_em >= target_height_em {
            return Some(*v);
        }
    }
    // If no variant is large enough, return the largest one.
    variants.last().copied()
}

/// Select the display-mode variant for a large operator character.
///
/// Uses the MATH table's `display_operator_min_height` to find a variant
/// that meets the minimum height for display-mode operators.
///
/// Returns `None` if no display variant exists.
#[allow(dead_code)] // called from raster.rs (feature-gated)
pub fn display_operator_variant(ch: char) -> Option<VariantGlyph> {
    let table = &*VARIANT_TABLE;
    if table.display_op_min_height <= 0.0 {
        return None;
    }
    select_vertical_variant(ch, table.display_op_min_height)
}

/// Select the best horizontal glyph variant for a character at the given target width.
///
/// Returns the smallest variant whose advance is >= `target_width_em`, or the largest
/// available variant if none is wide enough.
///
/// Returns `None` if no horizontal variants exist for this character.
#[allow(dead_code)]
pub fn select_horizontal_variant(ch: char, target_width_em: f64) -> Option<VariantGlyph> {
    let table = &*VARIANT_TABLE;
    let idx = table
        .horizontal
        .binary_search_by_key(&ch, |(c, _)| *c)
        .ok()?;
    let variants = &table.horizontal[idx].1;
    if variants.is_empty() {
        return None;
    }

    // Find the smallest variant that meets the target width.
    for v in variants {
        if v.advance_em >= target_width_em {
            return Some(*v);
        }
    }
    // If no variant is wide enough, return the widest one.
    variants.last().copied()
}

/// Get the display_operator_min_height from the MATH table, in em units.
#[allow(dead_code)]
pub fn display_operator_min_height() -> f64 {
    VARIANT_TABLE.display_op_min_height
}

/// Whether a character is a large (n-ary) operator that may have display-size
/// MATH-table variants (∑, ∫, ∏, …). Shared by the raster and SVG emitters so
/// both pick the same variant glyph for display-mode operators.
#[inline]
#[allow(dead_code)]
pub fn is_large_op_char(ch: char) -> bool {
    matches!(
        ch,
        '∑' | '∏'
            | '∐'
            | '∫'
            | '∬'
            | '∭'
            | '⨌'
            | '∮'
            | '∯'
            | '∰'
            | '⋂'
            | '⋃'
            | '⋁'
            | '⋀'
            | '⨀'
            | '⨁'
            | '⨂'
            | '⨄'
            | '⨆'
    )
}

/// Get all vertical variants for a character (for debugging/testing).
#[cfg(test)]
pub fn vertical_variants(ch: char) -> Option<Vec<VariantGlyph>> {
    let table = &*VARIANT_TABLE;
    let idx = table.vertical.binary_search_by_key(&ch, |(c, _)| *c).ok()?;
    Some(table.vertical[idx].1.clone())
}

/// Get all horizontal variants for a character (for debugging/testing).
#[cfg(test)]
pub fn horizontal_variants(ch: char) -> Option<Vec<VariantGlyph>> {
    let table = &*VARIANT_TABLE;
    let idx = table
        .horizontal
        .binary_search_by_key(&ch, |(c, _)| *c)
        .ok()?;
    Some(table.horizontal[idx].1.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_table_is_non_empty() {
        let table = &*VARIANT_TABLE;
        assert!(
            !table.vertical.is_empty(),
            "MATH table should have vertical variants"
        );
    }

    #[test]
    fn display_op_min_height_is_reasonable() {
        let h = display_operator_min_height();
        assert!(
            h > 0.5 && h < 3.0,
            "display_operator_min_height should be between 0.5 and 3.0 em, got {h}"
        );
    }

    #[test]
    fn left_paren_has_variants() {
        let variants = vertical_variants('(');
        assert!(
            variants.is_some(),
            "left paren should have vertical variants"
        );
        let variants = variants.unwrap();
        assert!(
            variants.len() >= 2,
            "left paren should have at least 2 variants, got {}",
            variants.len()
        );
        // Variants should be sorted by ascending advance.
        for w in variants.windows(2) {
            assert!(
                w[0].advance_em <= w[1].advance_em,
                "variants should be sorted by advance"
            );
        }
    }

    #[test]
    fn right_paren_has_variants() {
        assert!(vertical_variants(')').is_some());
    }

    #[test]
    fn left_bracket_has_variants() {
        assert!(vertical_variants('[').is_some());
    }

    #[test]
    fn left_brace_has_variants() {
        assert!(vertical_variants('{').is_some());
    }

    #[test]
    fn sum_has_display_variant() {
        let v = display_operator_variant('\u{2211}');
        assert!(v.is_some(), "∑ should have a display variant");
        let v = v.unwrap();
        assert!(
            v.advance_em > 0.5,
            "display variant advance should be > 0.5 em"
        );
    }

    #[test]
    fn integral_has_display_variant() {
        let v = display_operator_variant('\u{222B}');
        assert!(v.is_some(), "∫ should have a display variant");
    }

    #[test]
    fn product_has_display_variant() {
        let v = display_operator_variant('\u{220F}');
        assert!(v.is_some(), "∏ should have a display variant");
    }

    #[test]
    fn select_variant_returns_smallest_fitting() {
        let variants = vertical_variants('(').unwrap();
        if variants.len() >= 2 {
            // Request a height between the first and second variant advances.
            let mid = (variants[0].advance_em + variants[1].advance_em) / 2.0;
            let selected = select_vertical_variant('(', mid).unwrap();
            assert_eq!(
                selected.glyph_id, variants[1].glyph_id,
                "should select the second (larger) variant"
            );
        }
    }

    #[test]
    fn select_variant_returns_largest_when_too_tall() {
        let variants = vertical_variants('(').unwrap();
        let huge_target = 100.0; // way larger than any variant
        let selected = select_vertical_variant('(', huge_target).unwrap();
        assert_eq!(
            selected.glyph_id,
            variants.last().unwrap().glyph_id,
            "should return the largest variant"
        );
    }

    #[test]
    fn no_variants_for_regular_letter() {
        assert!(
            vertical_variants('x').is_none(),
            "regular letters should not have vertical variants"
        );
    }

    #[test]
    fn horizontal_table_is_populated() {
        let table = &*VARIANT_TABLE;
        // Latin Modern Math should have horizontal variants for some glyphs
        // (e.g., overbrace, underbrace, horizontal arrows).
        eprintln!("horizontal variant entries: {}", table.horizontal.len());
        // Dump which characters have horizontal variants for debugging.
        for (ch, variants) in &table.horizontal {
            eprintln!(
                "  U+{:04X} '{}': {} variants",
                *ch as u32,
                ch,
                variants.len()
            );
            // Also test the public lookup function
            let _ = horizontal_variants(*ch);
        }
    }

    #[test]
    fn horizontal_variants_sorted() {
        let table = &*VARIANT_TABLE;
        for (ch, variants) in &table.horizontal {
            for w in variants.windows(2) {
                assert!(
                    w[0].advance_em <= w[1].advance_em,
                    "horizontal variants for U+{:04X} should be sorted by advance",
                    *ch as u32
                );
            }
        }
    }

    #[test]
    fn select_horizontal_variant_returns_none_for_missing() {
        assert!(
            select_horizontal_variant('x', 1.0).is_none(),
            "regular letter x should have no horizontal variants"
        );
    }
}
