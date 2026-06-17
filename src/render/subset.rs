//! Font subsetting: collect the unique `(font, char)` pairs referenced by a
//! layout tree and extract only the glyph outlines needed for the SVG output.

use std::collections::{HashMap, HashSet};

use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
use super::font_embed::{FontInfo, GlyphOutline, extract_glyph, extract_glyph_by_id, font_info};
use super::fonts_registry::{self, FontId};
use super::layout::{BoxKind, LayoutBox};

/// Extracted glyph outlines for a single embedded face.
#[derive(Debug)]
pub struct FontGlyphs {
    /// Outlines keyed by character.
    pub glyphs: HashMap<char, GlyphOutline>,
    /// Font-level metrics (units per em) for this face.
    pub font_info: FontInfo,
}

/// Set of extracted glyph outlines, partitioned per embedded font, plus the
/// MATH-table size variants (which always come from the Math font).
#[derive(Debug)]
pub struct GlyphSet {
    /// Per-font extracted outlines.
    pub fonts: HashMap<FontId, FontGlyphs>,
    /// MATH-table size-variant outlines keyed by font glyph id (large operators,
    /// stretchy delimiters). Mirrors what the raster backend draws so the
    /// standalone SVG matches the PNG.
    pub variants: HashMap<u16, GlyphOutline>,
    /// Math-font metrics (used for variants and as the default).
    pub font_info: FontInfo,
}

impl GlyphSet {
    /// Look up an extracted outline for `(font, ch)`, falling back to the Math
    /// font when the chosen text face lacks the glyph.
    #[must_use]
    pub fn outline(&self, font: FontId, ch: char) -> Option<(&GlyphOutline, FontId, FontInfo)> {
        if let Some(fg) = self.fonts.get(&font)
            && let Some(o) = fg.glyphs.get(&ch)
        {
            return Some((o, font, fg.font_info));
        }
        if !font.is_math()
            && let Some(fg) = self.fonts.get(&FontId::Math)
            && let Some(o) = fg.glyphs.get(&ch)
        {
            return Some((o, FontId::Math, fg.font_info));
        }
        None
    }
}

// ── Collection (heap tree) ───────────────────────────────────────────

/// Collect all unique `(font, char)` pairs referenced by a heap layout tree.
pub fn collect_chars(root: &LayoutBox) -> HashSet<(FontId, char)> {
    let mut chars = HashSet::new();
    collect_chars_inner(root, &mut chars);
    chars
}

fn collect_chars_inner(node: &LayoutBox, chars: &mut HashSet<(FontId, char)>) {
    match &node.kind {
        BoxKind::Glyph {
            ch, italic, font, ..
        } => {
            collect_glyph(*ch, *italic, *font, chars);
        },
        BoxKind::Delimiter { ch, target_height } => {
            // Only collect the char if it won't be rendered as an SVG path
            // (i.e. we need the font glyph as a fallback)
            if super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0).is_none() {
                chars.insert((FontId::Math, *ch));
            }
        },
        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            for child in children {
                collect_chars_inner(&child.layout, chars);
            }
        },
        BoxKind::Color { child, .. } | BoxKind::Boxed { child, .. } => {
            collect_chars_inner(&child.layout, chars);
        },
        BoxKind::Rule { .. } | BoxKind::Kern | BoxKind::SvgPath(_) | BoxKind::Phantom => {},
        BoxKind::TextRun {
            text,
            italic,
            text_style,
            ..
        } => {
            let font = fonts_registry::font_for_text_run(*text_style);
            for ch in text.chars() {
                collect_glyph(ch, *italic, font, chars);
            }
        },
    }
}

/// Record the glyph(s) needed to draw `ch` from `font`.
///
/// For the Math font, italic letters also pull in their math-italic codepoint.
/// For a text face we additionally record the Math-font fallback, so the
/// emitters can fall back when the chosen face lacks the glyph.
fn collect_glyph(ch: char, italic: bool, font: FontId, chars: &mut HashSet<(FontId, char)>) {
    if font.is_math() {
        chars.insert((FontId::Math, ch));
        if italic && let Some(italic_ch) = super::layout::math_italic_codepoint(ch) {
            chars.insert((FontId::Math, italic_ch));
        }
    } else {
        chars.insert((font, ch));
        chars.insert((FontId::Math, ch));
    }
}

/// Collect all unique `(font, char)` pairs from an arena layout tree.
pub fn collect_chars_arena(arena: &LayoutArena, root: LayoutBoxId) -> HashSet<(FontId, char)> {
    let mut chars = HashSet::new();
    collect_chars_arena_inner(arena, root, &mut chars);
    chars
}

fn collect_chars_arena_inner(
    arena: &LayoutArena,
    id: LayoutBoxId,
    chars: &mut HashSet<(FontId, char)>,
) {
    let node = arena.get(id);
    match &node.kind {
        ArenaBoxKind::Glyph {
            ch, italic, font, ..
        } => {
            collect_glyph(*ch, *italic, *font, chars);
        },
        ArenaBoxKind::Delimiter { ch, target_height } => {
            if super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0).is_none() {
                chars.insert((FontId::Math, *ch));
            }
        },
        ArenaBoxKind::HBox { children } | ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                collect_chars_arena_inner(arena, child.id, chars);
            }
        },
        ArenaBoxKind::Color { child, .. } => {
            collect_chars_arena_inner(arena, *child, chars);
        },
        ArenaBoxKind::Boxed { child, .. } => {
            collect_chars_arena_inner(arena, *child, chars);
        },
        ArenaBoxKind::Rule { .. }
        | ArenaBoxKind::Kern
        | ArenaBoxKind::SvgPath(_)
        | ArenaBoxKind::Phantom => {},
        ArenaBoxKind::TextRun {
            text,
            italic,
            text_style,
            ..
        } => {
            let font = fonts_registry::font_for_text_run(*text_style);
            for ch in text.chars() {
                collect_glyph(ch, *italic, font, chars);
            }
        },
    }
}

// ── Building ─────────────────────────────────────────────────────────

/// Build a `GlyphSet` by extracting outlines for each `(font, char)` in `chars`.
///
/// Glyphs not found in the chosen font are silently skipped (the emitters fall
/// back to the Math font or a `<text>` element).
pub fn build_glyph_set(chars: &HashSet<(FontId, char)>) -> GlyphSet {
    let mut fonts: HashMap<FontId, FontGlyphs> = HashMap::new();
    for &(font, ch) in chars {
        if let Some(outline) = extract_glyph(font, ch) {
            fonts
                .entry(font)
                .or_insert_with(|| FontGlyphs {
                    glyphs: HashMap::new(),
                    font_info: font_info(font),
                })
                .glyphs
                .insert(ch, outline);
        }
    }
    GlyphSet {
        fonts,
        variants: HashMap::new(),
        font_info: font_info(FontId::Math),
    }
}

/// Add MATH-table variant glyph outlines (by font glyph id) to an existing set.
///
/// The standalone SVG emitter uses these for stretchy delimiters and display
/// large operators so its output matches the raster backend, which draws the
/// same variant glyphs. Variants always come from the Math font.
pub fn add_variants(set: &mut GlyphSet, ids: &HashSet<u16>) {
    for &id in ids {
        if set.variants.contains_key(&id) {
            continue;
        }
        if let Some(outline) = extract_glyph_by_id(FontId::Math, id) {
            set.variants.insert(id, outline);
        }
    }
}

/// Collect MATH-table variant glyph ids needed by a heap layout tree, mirroring
/// the selection logic the raster backend uses (so both pick the same glyph).
pub fn collect_variants(root: &LayoutBox) -> HashSet<u16> {
    let mut ids = HashSet::new();
    collect_variants_inner(root, &mut ids);
    ids
}

fn collect_variants_inner(node: &LayoutBox, ids: &mut HashSet<u16>) {
    use super::glyph_variants::{
        display_operator_variant, is_large_op_char, select_vertical_variant,
    };
    match &node.kind {
        BoxKind::Glyph { ch, size, .. } => {
            if *size > 1.2
                && is_large_op_char(*ch)
                && let Some(v) = display_operator_variant(*ch)
            {
                ids.insert(v.glyph_id);
            }
        },
        BoxKind::Delimiter { ch, target_height } => {
            if super::glyph_variants::delimiter_variant(*ch, *target_height).is_some() {
                if let Some(v) = select_vertical_variant(*ch, *target_height) {
                    ids.insert(v.glyph_id);
                }
            } else if let Some(asm) =
                super::glyph_variants::vertical_assembly(*ch, *target_height)
            {
                for p in &asm.placements {
                    ids.insert(p.glyph_id);
                }
            } else if let Some(v) = select_vertical_variant(*ch, *target_height) {
                ids.insert(v.glyph_id);
            }
        },
        BoxKind::HBox { children } | BoxKind::VBox { children } => {
            for child in children {
                collect_variants_inner(&child.layout, ids);
            }
        },
        BoxKind::Color { child, .. } | BoxKind::Boxed { child, .. } => {
            collect_variants_inner(&child.layout, ids);
        },
        BoxKind::Rule { .. }
        | BoxKind::Kern
        | BoxKind::SvgPath(_)
        | BoxKind::Phantom
        | BoxKind::TextRun { .. } => {},
    }
}

/// Collect MATH-table variant glyph ids needed by an arena layout tree.
pub fn collect_variants_arena(arena: &LayoutArena, root: LayoutBoxId) -> HashSet<u16> {
    let mut ids = HashSet::new();
    collect_variants_arena_inner(arena, root, &mut ids);
    ids
}

fn collect_variants_arena_inner(arena: &LayoutArena, id: LayoutBoxId, ids: &mut HashSet<u16>) {
    use super::glyph_variants::{
        display_operator_variant, is_large_op_char, select_vertical_variant,
    };
    let node = arena.get(id);
    match &node.kind {
        ArenaBoxKind::Glyph { ch, size, .. } => {
            if *size > 1.2
                && is_large_op_char(*ch)
                && let Some(v) = display_operator_variant(*ch)
            {
                ids.insert(v.glyph_id);
            }
        },
        ArenaBoxKind::Delimiter { ch, target_height } => {
            if super::glyph_variants::delimiter_variant(*ch, *target_height).is_some() {
                if let Some(v) = select_vertical_variant(*ch, *target_height) {
                    ids.insert(v.glyph_id);
                }
            } else if let Some(asm) =
                super::glyph_variants::vertical_assembly(*ch, *target_height)
            {
                for p in &asm.placements {
                    ids.insert(p.glyph_id);
                }
            } else if let Some(v) = select_vertical_variant(*ch, *target_height) {
                ids.insert(v.glyph_id);
            }
        },
        ArenaBoxKind::HBox { children } | ArenaBoxKind::VBox { children } => {
            for child in arena.children(*children) {
                collect_variants_arena_inner(arena, child.id, ids);
            }
        },
        ArenaBoxKind::Color { child, .. } | ArenaBoxKind::Boxed { child, .. } => {
            collect_variants_arena_inner(arena, *child, ids);
        },
        ArenaBoxKind::Rule { .. }
        | ArenaBoxKind::Kern
        | ArenaBoxKind::SvgPath(_)
        | ArenaBoxKind::Phantom
        | ArenaBoxKind::TextRun { .. } => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::render::RenderOptions;
    use crate::render::layout::{PositionedBox, layout_expr};
    use crate::render::style::MathStyle;

    fn math_chars(set: &HashSet<(FontId, char)>) -> HashSet<char> {
        set.iter()
            .filter(|(f, _)| f.is_math())
            .map(|(_, c)| *c)
            .collect()
    }

    #[test]
    fn collect_chars_finds_glyphs() {
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Number("1".into()),
        ]);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let chars = collect_chars(&root);
        let m = math_chars(&chars);
        assert!(m.contains(&'x'));
        assert!(m.contains(&'+'));
        // '1' is a number drawn from the Roman text face.
        assert!(chars.contains(&(FontId::RomanRegular, '1')));
    }

    #[test]
    fn collect_chars_finds_delimiters() {
        let glyph_x = LayoutBox {
            width: 0.5,
            height: 0.43,
            depth: 0.0,
            kind: BoxKind::Glyph {
                ch: 'x',
                size: 1.0,
                italic: false,
                font: FontId::Math,
            },
        };
        let delim_open = LayoutBox {
            width: 0.3,
            height: 0.43,
            depth: 0.0,
            kind: BoxKind::Delimiter {
                ch: '(',
                target_height: 0.3,
            },
        };
        let delim_close = LayoutBox {
            width: 0.3,
            height: 0.43,
            depth: 0.0,
            kind: BoxKind::Delimiter {
                ch: ')',
                target_height: 0.3,
            },
        };
        let root = LayoutBox {
            width: 1.1,
            height: 0.43,
            depth: 0.0,
            kind: BoxKind::HBox {
                children: vec![
                    PositionedBox::at_origin(delim_open),
                    PositionedBox::at_origin(glyph_x),
                    PositionedBox::at_origin(delim_close),
                ],
            },
        };
        let chars = collect_chars(&root);
        let m = math_chars(&chars);
        assert!(m.contains(&'('));
        assert!(m.contains(&')'));
        assert!(m.contains(&'x'));
    }

    #[test]
    fn collect_chars_skips_svg_path_delimiters() {
        let delim = LayoutBox {
            width: 0.3,
            height: 1.0,
            depth: 0.5,
            kind: BoxKind::Delimiter {
                ch: '(',
                target_height: 1.5,
            },
        };
        let root = LayoutBox {
            width: 0.3,
            height: 1.0,
            depth: 0.5,
            kind: BoxKind::HBox {
                children: vec![PositionedBox::at_origin(delim)],
            },
        };
        let chars = collect_chars(&root);
        assert!(!math_chars(&chars).contains(&'('));
    }

    #[test]
    fn build_glyph_set_extracts_outlines() {
        let mut chars = HashSet::new();
        chars.insert((FontId::Math, 'x'));
        chars.insert((FontId::Math, '+'));
        chars.insert((FontId::Math, '2'));

        let set = build_glyph_set(&chars);
        let math = set.fonts.get(&FontId::Math).expect("math glyphs");
        assert_eq!(math.glyphs.len(), 3);
        assert!(math.glyphs.contains_key(&'x'));
        assert!(math.glyphs.contains_key(&'+'));
        assert!(math.glyphs.contains_key(&'2'));
        assert!((set.font_info.units_per_em - 1000.0).abs() < 1.0);
    }

    #[test]
    fn build_glyph_set_skips_missing_glyphs() {
        let mut chars = HashSet::new();
        chars.insert((FontId::Math, 'x'));
        chars.insert((FontId::Math, '\u{F8FF}'));

        let set = build_glyph_set(&chars);
        let math = set.fonts.get(&FontId::Math).expect("math glyphs");
        assert!(math.glyphs.contains_key(&'x'));
        assert!(!math.glyphs.contains_key(&'\u{F8FF}'));
    }

    #[test]
    fn build_glyph_set_uses_text_face_for_roman() {
        let mut chars = HashSet::new();
        chars.insert((FontId::RomanBold, 'A'));
        let set = build_glyph_set(&chars);
        let bold = set
            .fonts
            .get(&FontId::RomanBold)
            .expect("roman bold glyphs");
        assert!(bold.glyphs.contains_key(&'A'));
    }

    #[test]
    fn collect_chars_arena_matches_heap() {
        use crate::reader::Reader;
        use crate::render::arena::LayoutArena;
        use crate::render::layout::{LayoutBuilder, layout_expr_c};

        let config = crate::error::Config::default();
        let result =
            crate::reader::latex::LatexReader::parse_compact("x^2 + y = \\frac{1}{2}", &config)
                .unwrap();
        let options = RenderOptions::default();

        let heap_root = layout_expr_c(&result.expr, MathStyle::Text, &options);
        let heap_chars = collect_chars(&heap_root);

        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Text, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_chars = collect_chars_arena(&arena, arena_root);

        assert_eq!(heap_chars, arena_chars);
    }
}
