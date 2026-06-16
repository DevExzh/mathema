//! Font subsetting: collect unique characters from a layout tree and extract
//! only the glyph outlines needed for the SVG output.

use std::collections::{HashMap, HashSet};

use super::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
use super::font_embed::{FontInfo, GlyphOutline, extract_glyph, extract_glyph_by_id, parse_font_info};
use super::layout::{BoxKind, LayoutBox};

/// Set of extracted glyph outlines keyed by character, plus font metrics.
#[derive(Debug)]
pub struct GlyphSet {
    pub glyphs: HashMap<char, GlyphOutline>,
    /// MATH-table size-variant outlines keyed by font glyph id (large operators,
    /// stretchy delimiters). Mirrors what the raster backend draws so the
    /// standalone SVG matches the PNG.
    pub variants: HashMap<u16, GlyphOutline>,
    pub font_info: FontInfo,
}

/// Collect all unique characters referenced by `Glyph` and `Delimiter` nodes
/// in a heap-based layout tree.
pub fn collect_chars(root: &LayoutBox) -> HashSet<char> {
    let mut chars = HashSet::new();
    collect_chars_inner(root, &mut chars);
    chars
}

fn collect_chars_inner(node: &LayoutBox, chars: &mut HashSet<char>) {
    match &node.kind {
        BoxKind::Glyph { ch, italic, .. } => {
            collect_char(*ch, *italic, chars);
        },
        BoxKind::Delimiter { ch, target_height } => {
            // Only collect the char if it won't be rendered as an SVG path
            // (i.e. we need the font glyph as a fallback)
            if super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0).is_none() {
                chars.insert(*ch);
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
        BoxKind::TextRun { text, italic, .. } => {
            for ch in text.chars() {
                collect_char(ch, *italic, chars);
            }
        },
    }
}

fn collect_char(ch: char, italic: bool, chars: &mut HashSet<char>) {
    chars.insert(ch);
    if italic && let Some(italic_ch) = super::layout::math_italic_codepoint(ch) {
        chars.insert(italic_ch);
    }
}

/// Collect all unique characters from an arena-based layout tree.
pub fn collect_chars_arena(arena: &LayoutArena, root: LayoutBoxId) -> HashSet<char> {
    let mut chars = HashSet::new();
    collect_chars_arena_inner(arena, root, &mut chars);
    chars
}

fn collect_chars_arena_inner(arena: &LayoutArena, id: LayoutBoxId, chars: &mut HashSet<char>) {
    let node = arena.get(id);
    match &node.kind {
        ArenaBoxKind::Glyph { ch, italic, .. } => {
            collect_char(*ch, *italic, chars);
        },
        ArenaBoxKind::Delimiter { ch, target_height } => {
            // Only collect the char if it won't be rendered as an SVG path
            if super::delimiter_paths::delimiter_path(*ch, *target_height, 1.0).is_none() {
                chars.insert(*ch);
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
        ArenaBoxKind::TextRun { text, italic, .. } => {
            for ch in text.chars() {
                collect_char(ch, *italic, chars);
            }
        },
    }
}

/// Build a `GlyphSet` by extracting outlines for each character in `chars`.
///
/// Characters not found in the font are silently skipped (the SVG writer
/// will fall back to `<text>` for those).
pub fn build_glyph_set(font_data: &[u8], chars: &HashSet<char>) -> Option<GlyphSet> {
    let font_info = parse_font_info(font_data)?;
    let mut glyphs = HashMap::with_capacity(chars.len());
    for &ch in chars {
        if let Some(outline) = extract_glyph(font_data, ch) {
            glyphs.insert(ch, outline);
        }
    }
    Some(GlyphSet {
        glyphs,
        variants: HashMap::new(),
        font_info,
    })
}

/// Add MATH-table variant glyph outlines (by font glyph id) to an existing set.
///
/// The standalone SVG emitter uses these for stretchy delimiters and display
/// large operators so its output matches the raster backend, which draws the
/// same variant glyphs.
pub fn add_variants(set: &mut GlyphSet, font_data: &[u8], ids: &HashSet<u16>) {
    for &id in ids {
        if set.variants.contains_key(&id) {
            continue;
        }
        if let Some(outline) = extract_glyph_by_id(font_data, id) {
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
            if let Some(v) = select_vertical_variant(*ch, *target_height) {
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
            if let Some(v) = select_vertical_variant(*ch, *target_height) {
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

    static FONT_DATA: &[u8] = include_bytes!("fonts/LatinModernMath-Regular.otf");

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
        assert!(chars.contains(&'x'));
        assert!(chars.contains(&'+'));
        assert!(chars.contains(&'1'));
    }

    #[test]
    fn collect_chars_finds_delimiters() {
        // Build a layout tree manually so the test is independent of
        // whether the layout engine decides to use SVG-path delimiters.
        let glyph_x = LayoutBox {
            width: 0.5,
            height: 0.43,
            depth: 0.0,
            kind: BoxKind::Glyph {
                ch: 'x',
                size: 1.0,
                italic: false,
            },
        };
        // Use a small target_height so SVG path threshold is NOT met
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
        assert!(chars.contains(&'('));
        assert!(chars.contains(&')'));
        assert!(chars.contains(&'x'));
    }

    #[test]
    fn collect_chars_skips_svg_path_delimiters() {
        // When target_height is large enough for SVG path rendering,
        // collect_chars should NOT include the delimiter character
        // (the SVG writer uses the path, not a font glyph).
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
        // '(' has an SVG path at this height, so it should NOT be collected
        assert!(!chars.contains(&'('));
    }

    #[test]
    fn collect_chars_deduplicates() {
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
            MathNode::Identifier("x".into()),
        ]);
        let options = RenderOptions::default();
        let root = layout_expr(&expr, MathStyle::Text, &options);
        let chars = collect_chars(&root);
        assert_eq!(chars.len(), 3); // 'x', italic 'x' (U+1D465), and '+'
    }

    #[test]
    fn collect_chars_ignores_kern_and_rule() {
        let kern = LayoutBox::kern(0.5);
        let chars = collect_chars(&kern);
        assert!(chars.is_empty());

        let rule = LayoutBox::rule(1.0, 0.04);
        let chars = collect_chars(&rule);
        assert!(chars.is_empty());
    }

    #[test]
    fn build_glyph_set_extracts_outlines() {
        let mut chars = HashSet::new();
        chars.insert('x');
        chars.insert('+');
        chars.insert('2');

        let set = build_glyph_set(FONT_DATA, &chars).expect("valid font");
        assert_eq!(set.glyphs.len(), 3);
        assert!(set.glyphs.contains_key(&'x'));
        assert!(set.glyphs.contains_key(&'+'));
        assert!(set.glyphs.contains_key(&'2'));
        assert!((set.font_info.units_per_em - 1000.0).abs() < 1.0);
    }

    #[test]
    fn build_glyph_set_skips_missing_glyphs() {
        let mut chars = HashSet::new();
        chars.insert('x');
        chars.insert('\u{F8FF}');

        let set = build_glyph_set(FONT_DATA, &chars).expect("valid font");
        assert!(set.glyphs.contains_key(&'x'));
        assert!(!set.glyphs.contains_key(&'\u{F8FF}'));
    }

    #[test]
    fn build_glyph_set_rejects_bad_font() {
        let chars = HashSet::new();
        assert!(build_glyph_set(b"garbage", &chars).is_none());
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

        // Heap path
        let heap_root = layout_expr_c(&result.expr, MathStyle::Text, &options);
        let heap_chars = collect_chars(&heap_root);

        // Arena path
        let mut arena = LayoutArena::new();
        let builder = LayoutBuilder::new(MathStyle::Text, &options);
        let arena_root = builder.layout(&mut arena, &result.expr);
        let arena_chars = collect_chars_arena(&arena, arena_root);

        assert_eq!(heap_chars, arena_chars);
    }
}
