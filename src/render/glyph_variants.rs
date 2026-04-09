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

// Functions and types in this module are infrastructure for upcoming glyph-variant
// integration in the layout engine; suppress dead-code warnings until wired in.
#![allow(dead_code)]

use std::sync::LazyLock;

/// A pre-designed glyph variant from the MATH table.
#[derive(Debug, Clone, Copy)]
pub struct VariantGlyph {
    /// The glyph ID in the font (for use with fontdue's `rasterize_indexed`).
    pub glyph_id: u16,
    /// The advance measurement in em units (height for vertical, width for horizontal).
    pub advance_em: f64,
}

/// Parsed vertical glyph variants for all covered glyphs.
struct VariantTable {
    /// Map from base character to its sorted list of vertical variants
    /// (sorted by ascending advance_em).
    vertical: Vec<(char, Vec<VariantGlyph>)>,
    /// The display_operator_min_height from the MATH constants, in em units.
    display_op_min_height: f64,
}

static VARIANT_TABLE: LazyLock<VariantTable> = LazyLock::new(|| {
    let font_bytes = super::FONT_DATA;
    let face = ttf_parser::Face::parse(font_bytes, 0).expect("embedded font data must be valid");
    let upem = face.units_per_em() as f64;

    let math = match face.tables().math.as_ref() {
        Some(m) => m,
        None => {
            return VariantTable {
                vertical: Vec::new(),
                display_op_min_height: 0.0,
            };
        }
    };

    let display_op_min_height = math
        .constants
        .as_ref()
        .map(|c| c.display_operator_min_height() as f64 / upem)
        .unwrap_or(0.0);

    let mut vertical = Vec::new();

    if let Some(ref variants) = math.variants {
        // Iterate all cmap entries to find characters with vertical variants.
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
                    if let Some(construction) =
                        variants.vertical_constructions.get(gid)
                    {
                        let mut glyph_variants = Vec::new();
                        for i in 0..construction.variants.len() {
                            let v = construction.variants.get(i).unwrap();
                            glyph_variants.push(VariantGlyph {
                                glyph_id: v.variant_glyph.0,
                                advance_em: v.advance_measurement as f64 / upem,
                            });
                        }
                        if !glyph_variants.is_empty() {
                            // Sort by advance (should already be sorted per spec, but be safe).
                            glyph_variants
                                .sort_by(|a, b| a.advance_em.partial_cmp(&b.advance_em).unwrap());
                            vertical.push((ch, glyph_variants));
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

    VariantTable {
        vertical,
        display_op_min_height,
    }
});

/// Select the best vertical glyph variant for a character at the given target height.
///
/// Returns the smallest variant whose advance is >= `target_height_em`, or the largest
/// available variant if none is large enough.
///
/// Returns `None` if no variants exist for this character.
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
pub fn display_operator_variant(ch: char) -> Option<VariantGlyph> {
    let table = &*VARIANT_TABLE;
    if table.display_op_min_height <= 0.0 {
        return None;
    }
    select_vertical_variant(ch, table.display_op_min_height)
}

/// Get the display_operator_min_height from the MATH table, in em units.
#[allow(dead_code)]
pub fn display_operator_min_height() -> f64 {
    VARIANT_TABLE.display_op_min_height
}

/// Get all vertical variants for a character (for debugging/testing).
#[cfg(test)]
pub fn vertical_variants(ch: char) -> Option<Vec<VariantGlyph>> {
    let table = &*VARIANT_TABLE;
    let idx = table.vertical.binary_search_by_key(&ch, |(c, _)| *c).ok()?;
    Some(table.vertical[idx].1.clone())
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
}
