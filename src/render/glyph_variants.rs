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

/// A single part in an OpenType MATH vertical glyph assembly (e.g. the bottom
/// hook, stretchy extender, or top of a radical/brace).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct AssemblyPart {
    glyph_id: u16,
    /// Full advance of the part along the assembly axis, in em.
    full_adv_em: f64,
    /// Connector overlap length on the start (lower) side, in em.
    start_conn_em: f64,
    /// Connector overlap length on the end (upper) side, in em.
    end_conn_em: f64,
    /// Whether this part may be repeated to stretch the assembly.
    is_extender: bool,
    /// Glyph bbox bottom relative to its own baseline, in em (≤ 0 when below).
    y_min_em: f64,
    /// Glyph bbox top relative to its own baseline, in em.
    y_max_em: f64,
}

/// A parsed vertical assembly recipe (parts ordered bottom → top).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AssemblyRecipe {
    parts: Vec<AssemblyPart>,
    /// Horizontal advance (width) of the assembly, in em.
    width_em: f64,
}

/// One positioned part instance in a stretched assembly, ready to render.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct AssemblyPlacement {
    /// Glyph ID to draw.
    pub glyph_id: u16,
    /// Baseline shift (em) of this part above the assembly's bottom origin.
    pub shift_em: f64,
}

/// A fully laid-out stretchy assembly at a requested target height.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StretchyAssembly {
    /// Part instances, bottom → top, each with its baseline shift in em.
    pub placements: Vec<AssemblyPlacement>,
    /// Assembly width (advance), in em.
    pub width_em: f64,
    /// Highest ink above the assembly bottom origin, in em.
    pub ink_top_em: f64,
    /// Lowest ink relative to the assembly bottom origin, in em (≤ 0).
    pub ink_bottom_em: f64,
}

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
    /// Map from base character to its vertical assembly recipe (parts).
    vertical_assemblies: Vec<(char, AssemblyRecipe)>,
    /// Minimum connector overlap between adjacent assembly parts, in em.
    min_connector_overlap: f64,
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
                vertical_assemblies: Vec::new(),
                min_connector_overlap: 0.0,
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
    let mut vertical_assemblies: Vec<(char, AssemblyRecipe)> = Vec::new();
    let min_connector_overlap = math
        .variants
        .as_ref()
        .map(|v| v.min_connector_overlap as f64 / upem)
        .unwrap_or(0.0);

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
                        // Extract the stretchy assembly recipe, if present.
                        if let Some(asm) = construction.assembly {
                            let mut parts = Vec::new();
                            let mut width_em = 0.0_f64;
                            for i in 0..asm.parts.len() {
                                let Some(p) = asm.parts.get(i) else { continue };
                                let (y_min_em, y_max_em) = face
                                    .glyph_bounding_box(p.glyph_id)
                                    .map(|b| (b.y_min as f64 / upem, b.y_max as f64 / upem))
                                    .unwrap_or((0.0, 0.0));
                                let h_adv =
                                    face.glyph_hor_advance(p.glyph_id).unwrap_or(0) as f64 / upem;
                                width_em = width_em.max(h_adv);
                                parts.push(AssemblyPart {
                                    glyph_id: p.glyph_id.0,
                                    full_adv_em: p.full_advance as f64 / upem,
                                    start_conn_em: p.start_connector_length as f64 / upem,
                                    end_conn_em: p.end_connector_length as f64 / upem,
                                    is_extender: p.part_flags.extender(),
                                    y_min_em,
                                    y_max_em,
                                });
                            }
                            if !parts.is_empty() {
                                vertical_assemblies.push((ch, AssemblyRecipe { parts, width_em }));
                            }
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

    vertical_assemblies.sort_by_key(|(ch, _)| *ch);
    vertical_assemblies.dedup_by_key(|(ch, _)| *ch);

    VariantTable {
        vertical,
        horizontal,
        vertical_assemblies,
        min_connector_overlap,
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

/// Select a vertical variant for a stretchy **delimiter** at `target_height_em`,
/// but only when the variant fits without significant stretching.
///
/// `select_vertical_variant` always returns *some* variant (the largest when
/// none is tall enough), which the emitters then scale to the exact target —
/// stretching the largest discrete glyph by 1.5–2× looks badly distorted. For
/// delimiters we instead return `None` once the target exceeds the largest
/// variant by more than ~5%, so the caller falls back to the synthetic
/// delimiter path, which scales cleanly to any height. Layout and every emitter
/// call this so the reserved width and the drawn glyph always agree.
#[allow(dead_code)]
pub fn delimiter_variant(ch: char, target_height_em: f64) -> Option<VariantGlyph> {
    let v = select_vertical_variant(ch, target_height_em)?;
    if v.advance_em > 0.01 && target_height_em <= v.advance_em * 1.05 {
        Some(v)
    } else {
        None
    }
}

/// Build a stretchy vertical assembly for `ch` tall enough to reach
/// `target_height_em`, using the font's OpenType MATH `GlyphAssembly` parts
/// (e.g. radical checkmark + stretchy extender + top, or the 5-part brace).
///
/// Returns `None` when the glyph has no assembly recipe. The returned
/// placements are ordered bottom → top; each `shift_em` is the baseline offset
/// of that part above the assembly's bottom origin. This is the same mechanism
/// MathJax/HarfBuzz use, so tall delimiters stay proportional instead of being
/// uniformly scaled or approximated by a synthetic path.
#[allow(dead_code)]
pub fn vertical_assembly(ch: char, target_height_em: f64) -> Option<StretchyAssembly> {
    let table = &*VARIANT_TABLE;
    let idx = table
        .vertical_assemblies
        .binary_search_by_key(&ch, |(c, _)| *c)
        .ok()?;
    let recipe = &table.vertical_assemblies[idx].1;
    let parts = &recipe.parts;
    if parts.is_empty() {
        return None;
    }

    let min_overlap = table.min_connector_overlap;
    // Maximum overlap any connection allows = min over adjacent connector pairs.
    let mut max_overlap = f64::INFINITY;
    for w in parts.windows(2) {
        max_overlap = max_overlap.min(w[0].end_conn_em.min(w[1].start_conn_em));
    }
    if !max_overlap.is_finite() {
        max_overlap = min_overlap;
    }
    let max_overlap = max_overlap.max(min_overlap);

    let fixed_count = parts.iter().filter(|p| !p.is_extender).count();
    let ext_count = parts.iter().filter(|p| p.is_extender).count();
    let fixed_sum: f64 = parts
        .iter()
        .filter(|p| !p.is_extender)
        .map(|p| p.full_adv_em)
        .sum();
    let ext_sum: f64 = parts
        .iter()
        .filter(|p| p.is_extender)
        .map(|p| p.full_adv_em)
        .sum();

    // Smallest repetition count whose maximum achievable size reaches the target.
    let mut rep = 1usize;
    if ext_count > 0 {
        loop {
            let num_parts = fixed_count + ext_count * rep;
            let conns = num_parts.saturating_sub(1) as f64;
            let full = fixed_sum + ext_sum * rep as f64;
            let max_size = full - conns * min_overlap;
            if max_size >= target_height_em || rep > 256 {
                break;
            }
            rep += 1;
        }
    }

    // Expand the part sequence with extenders repeated `rep` times.
    let mut seq: Vec<&AssemblyPart> = Vec::with_capacity(fixed_count + ext_count * rep);
    for p in parts {
        let n = if p.is_extender { rep } else { 1 };
        for _ in 0..n {
            seq.push(p);
        }
    }
    let num_parts = seq.len();
    let conns = num_parts.saturating_sub(1) as f64;
    let full: f64 = seq.iter().map(|p| p.full_adv_em).sum();

    // Choose a single overlap to hit the target as closely as possible.
    let overlap = if conns > 0.0 {
        ((full - target_height_em) / conns).clamp(min_overlap, max_overlap)
    } else {
        0.0
    };

    // Stack bottom → top, accumulating advance minus overlap.
    let mut placements = Vec::with_capacity(num_parts);
    let mut ink_top = f64::NEG_INFINITY;
    let mut ink_bottom = f64::INFINITY;
    let mut shift = 0.0_f64;
    for (i, p) in seq.iter().enumerate() {
        placements.push(AssemblyPlacement {
            glyph_id: p.glyph_id,
            shift_em: shift,
        });
        ink_top = ink_top.max(shift + p.y_max_em);
        ink_bottom = ink_bottom.min(shift + p.y_min_em);
        shift += p.full_adv_em;
        if i < num_parts - 1 {
            shift -= overlap;
        }
    }

    Some(StretchyAssembly {
        placements,
        width_em: recipe.width_em,
        ink_top_em: ink_top,
        ink_bottom_em: ink_bottom,
    })
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
