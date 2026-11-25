//! Font registry: maps logical font identities to embedded OpenType data,
//! parsed `ttf_parser::Face`s, and font-derived glyph metrics.
//!
//! The renderer uses the Latin Modern *Math* font for all mathematical
//! content (it carries the OpenType MATH table that drives stretchy
//! delimiters, large operators and math constants). For *text-style* glyphs
//! (`\mathrm`, `\mathbf`, `\mathsf`, `\mathtt`, …) we instead draw from the
//! matching Latin Modern *text* face, which yields better fidelity than
//! remapping to the Unicode Math-Alphanumeric block.
//!
//! Styles with no Latin Modern text-face equivalent (double-struck, script,
//! fraktur) keep using the Math font via the Unicode-remap path; for those
//! [`text_font_for_style`] returns `None`.

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock, RwLock};

use crate::ast::TextStyle;

use super::font_embed::FontInfo;
use super::metrics::{GlyphMetrics, glyph_metrics, glyph_metrics_upright};

/// Logical font identity used by glyph boxes and the emitters.
///
/// `Math` is the default for ordinary mathematics and is the only face that
/// carries an OpenType MATH table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Latin Modern Math (primary math font, carries the MATH table).
    Math,
    /// Latin Modern Roman, upright (`\mathrm`, `\text`, operator names, digits).
    RomanRegular,
    /// Latin Modern Roman, bold (`\mathbf`).
    RomanBold,
    /// Latin Modern Roman, italic.
    RomanItalic,
    /// Latin Modern Roman, bold italic (`\mathbfit`).
    RomanBoldItalic,
    /// Latin Modern Sans, regular (`\mathsf`).
    SansRegular,
    /// Latin Modern Sans, bold (`\mathsfbf`).
    SansBold,
    /// Latin Modern Sans, oblique (`\mathsfit`).
    SansItalic,
    /// Latin Modern Sans, bold oblique.
    SansBoldItalic,
    /// Latin Modern Mono, regular (`\mathtt`).
    MonoRegular,
    /// Latin Modern Mono, italic.
    MonoItalic,
    /// Latin Modern Mono (light), bold.
    MonoBold,
    /// Latin Modern Mono (light), bold oblique.
    MonoBoldItalic,
    /// The designs of Latin Modern Math for scripts (OpenType `ssty`).
    MathScript,
    /// Its designs for scripts of scripts.
    MathScriptScript,
    /// Its variants of the large operators for displays (MATH table).
    MathDisplay,
}

impl Default for FontId {
    #[inline]
    fn default() -> Self {
        FontId::Math
    }
}

/// All non-default font identities, used for eager registry population in tests.
pub const ALL_FONTS: [FontId; 16] = [
    FontId::Math,
    FontId::RomanRegular,
    FontId::RomanBold,
    FontId::RomanItalic,
    FontId::RomanBoldItalic,
    FontId::SansRegular,
    FontId::SansBold,
    FontId::SansItalic,
    FontId::SansBoldItalic,
    FontId::MonoRegular,
    FontId::MonoItalic,
    FontId::MonoBold,
    FontId::MonoBoldItalic,
    FontId::MathScript,
    FontId::MathScriptScript,
    FontId::MathDisplay,
];

impl FontId {
    /// Dense index into the lazily-parsed face / font-info tables. Also used to
    /// namespace embedded `<symbol>` ids per font in the standalone SVG writer.
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            FontId::Math => 0,
            FontId::RomanRegular => 1,
            FontId::RomanBold => 2,
            FontId::RomanItalic => 3,
            FontId::RomanBoldItalic => 4,
            FontId::SansRegular => 5,
            FontId::SansBold => 6,
            FontId::SansItalic => 7,
            FontId::SansBoldItalic => 8,
            FontId::MonoRegular => 9,
            FontId::MonoItalic => 10,
            FontId::MonoBold => 11,
            FontId::MonoBoldItalic => 12,
            FontId::MathScript => 13,
            FontId::MathScriptScript => 14,
            FontId::MathDisplay => 15,
        }
    }

    /// True for the primary math font (the only one with a MATH table).
    #[inline]
    pub const fn is_math(self) -> bool {
        matches!(self, FontId::Math)
    }
}

/// Raw embedded OpenType bytes for a font identity.
///
/// `Math` aliases the shared `super::FONT_DATA` so the existing fast paths that
/// compare against it (and the MATH-table extractors) keep working unchanged.
#[inline]
pub fn font_bytes(id: FontId) -> &'static [u8] {
    match id {
        FontId::Math | FontId::MathScript | FontId::MathScriptScript | FontId::MathDisplay => {
            super::FONT_DATA
        },
        FontId::RomanRegular => include_bytes!("fonts/lmroman10-regular.otf"),
        FontId::RomanBold => include_bytes!("fonts/lmroman10-bold.otf"),
        FontId::RomanItalic => include_bytes!("fonts/lmroman10-italic.otf"),
        FontId::RomanBoldItalic => include_bytes!("fonts/lmroman10-bolditalic.otf"),
        FontId::SansRegular => include_bytes!("fonts/lmsans10-regular.otf"),
        FontId::SansBold => include_bytes!("fonts/lmsans10-bold.otf"),
        FontId::SansItalic => include_bytes!("fonts/lmsans10-oblique.otf"),
        FontId::SansBoldItalic => include_bytes!("fonts/lmsans10-boldoblique.otf"),
        FontId::MonoRegular => include_bytes!("fonts/lmmono10-regular.otf"),
        FontId::MonoItalic => include_bytes!("fonts/lmmono10-italic.otf"),
        FontId::MonoBold => include_bytes!("fonts/lmmonolt10-bold.otf"),
        FontId::MonoBoldItalic => include_bytes!("fonts/lmmonolt10-boldoblique.otf"),
    }
}

/// Per-id lazily-parsed faces. Parsing is deferred until a face is first used,
/// so a math-only render never pays to parse the 12 text faces.
static FACE_CELLS: [OnceLock<ttf_parser::Face<'static>>; 13] = [const { OnceLock::new() }; 13];

/// Parsed `ttf_parser::Face` for a font identity.
///
/// The embedded font data is static and validated, so a parse failure is a
/// build-time defect: we `expect` rather than thread `Result` through the
/// glyph-extraction hot paths.
#[inline]
pub fn face(id: FontId) -> &'static ttf_parser::Face<'static> {
    let id = match id {
        FontId::MathScript | FontId::MathScriptScript | FontId::MathDisplay => FontId::Math,
        id => id,
    };
    FACE_CELLS[id.index()].get_or_init(|| {
        ttf_parser::Face::parse(font_bytes(id), 0).expect("embedded font data must be valid")
    })
}

/// A character of the Math font with designs for scripts and for scripts of
/// scripts (OpenType `ssty`), and their metrics once read.
struct ScriptGlyph {
    glyphs: [u16; 2],
    metrics: [OnceLock<Option<GlyphMetrics>>; 2],
}

/// The alternates of the Math font's `ssty` feature.
static SCRIPT_ALTERNATES: LazyLock<Vec<ttf_parser::gsub::AlternateSubstitution<'static>>> =
    LazyLock::new(|| {
        let Some(gsub) = face(FontId::Math).tables().gsub else {
            return Vec::new();
        };
        gsub.features
            .into_iter()
            .filter(|feature| feature.tag == ttf_parser::Tag::from_bytes(b"ssty"))
            .flat_map(|feature| feature.lookup_indices)
            .filter_map(|index| gsub.lookups.get(index))
            .flat_map(|lookup| {
                lookup
                    .subtables
                    .into_iter::<ttf_parser::gsub::SubstitutionSubtable<'static>>()
            })
            .filter_map(|table| match table {
                ttf_parser::gsub::SubstitutionSubtable::Alternate(table) => Some(table),
                _ => None,
            })
            .collect()
    });

/// The designs of `ch` for scripts, read from the font.
fn read_script_glyph(ch: char) -> Option<ScriptGlyph> {
    let gid = face(FontId::Math).glyph_index(ch)?;
    let glyphs = SCRIPT_ALTERNATES.iter().find_map(|table| {
        let set = table.alternate_sets.get(table.coverage.get(gid)?)?;
        Some([set.alternates.get(0)?.0, set.alternates.get(1)?.0])
    })?;
    let metrics = [const { OnceLock::new() }; 2];
    Some(ScriptGlyph { glyphs, metrics })
}

/// The first code point of the Mathematical Alphanumeric Symbols.
const MATH_ALPHANUMERICS: u32 = 0x1D400;

/// The designs for scripts, read as characters are first drawn: ASCII and the
/// math alphanumerics in tables, others (only those of the font) in a map.
static ASCII_SCRIPT_GLYPHS: [OnceLock<Option<ScriptGlyph>>; 128] = [const { OnceLock::new() }; 128];
static ALPHANUMERIC_SCRIPT_GLYPHS: [OnceLock<Option<ScriptGlyph>>; 1024] =
    [const { OnceLock::new() }; 1024];
static OTHER_SCRIPT_GLYPHS: LazyLock<RwLock<HashMap<char, Option<&'static ScriptGlyph>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The designs of `ch` for scripts, if the Math font has them.
#[inline]
fn script_glyph(ch: char) -> Option<&'static ScriptGlyph> {
    let code = ch as u32;
    if let Some(cell) = ASCII_SCRIPT_GLYPHS.get(code as usize) {
        return cell.get_or_init(|| read_script_glyph(ch)).as_ref();
    }
    if let Some(cell) = code
        .checked_sub(MATH_ALPHANUMERICS)
        .and_then(|index| ALPHANUMERIC_SCRIPT_GLYPHS.get(index as usize))
    {
        return cell.get_or_init(|| read_script_glyph(ch)).as_ref();
    }
    if let Some(&hit) = OTHER_SCRIPT_GLYPHS.read().expect("script glyphs").get(&ch) {
        return hit;
    }
    // (Only characters of the font are kept, so the map stays small.)
    face(FontId::Math).glyph_index(ch)?;
    let glyph = read_script_glyph(ch).map(|glyph| &*Box::leak(Box::new(glyph)));
    *OTHER_SCRIPT_GLYPHS
        .write()
        .expect("script glyphs")
        .entry(ch)
        .or_insert(glyph)
}

/// The font for `ch`, a character of the Math font, in a style of the given size
/// class: the Math font has designs for scripts of most letters and digits.
#[inline]
pub fn script_font(ch: char, size_class: usize) -> Option<FontId> {
    let font = match size_class {
        0 => return None,
        1 => FontId::MathScript,
        _ => FontId::MathScriptScript,
    };
    script_glyph(ch).map(|_| font)
}

/// The level of a font of designs for scripts.
#[inline]
fn script_level(id: FontId) -> Option<usize> {
    match id {
        FontId::MathScript => Some(0),
        FontId::MathScriptScript => Some(1),
        _ => None,
    }
}

/// The glyph of `ch` in a font.
#[inline]
pub fn glyph_index(id: FontId, ch: char) -> Option<ttf_parser::GlyphId> {
    if id == FontId::MathDisplay {
        return super::glyph_variants::display_operator_variant(ch)
            .map(|v| ttf_parser::GlyphId(v.glyph_id));
    }
    let Some(level) = script_level(id) else {
        return face(id).glyph_index(ch);
    };
    script_glyph(ch).map(|glyph| ttf_parser::GlyphId(glyph.glyphs[level]))
}

/// The point of `ch` in a design for scripts that an accent is centred on, em.
pub fn script_top_accent(id: FontId, ch: char) -> Option<f64> {
    let info = face(id).tables().math?.glyph_info?;
    let at = info.top_accent_attachments?.get(glyph_index(id, ch)?)?;
    Some(f64::from(at.value) / font_info(id).units_per_em)
}

/// Per-id cached font-level info (units per em).
static FONT_INFO_CELLS: [OnceLock<FontInfo>; 16] = [const { OnceLock::new() }; 16];

/// Font-level metrics (units per em) for a font identity.
#[inline]
pub fn font_info(id: FontId) -> FontInfo {
    *FONT_INFO_CELLS[id.index()].get_or_init(|| FontInfo {
        units_per_em: f64::from(face(id).units_per_em()),
    })
}

/// Cache of font-derived metrics keyed by `(FontId, char)` for the text faces.
/// Metric extraction parses CFF/glyf tables, so caching avoids redoing the work
/// for repeated glyphs across the layout and the emitters.
type MetricsCache = RwLock<HashMap<(FontId, char), Option<GlyphMetrics>>>;

static TEXT_METRICS_CACHE: LazyLock<MetricsCache> = LazyLock::new(|| RwLock::new(HashMap::new()));

/// Map a styled-text [`TextStyle`] to the Latin Modern text face that best
/// renders it, or `None` when the style must keep the Math-font remap path.
///
/// Italic-letter styles (`\mathit`, `\mathnormal`) return `None` so they keep
/// using the Math-font math-italic glyphs (which match the existing tests and
/// look better than the upright Roman italics for math variables). Likewise the
/// double-struck / script / fraktur styles have no Latin Modern text equivalent.
#[inline]
pub fn text_font_for_style(style: TextStyle) -> Option<FontId> {
    Some(match style {
        TextStyle::Normal => FontId::RomanRegular,
        TextStyle::Bold => FontId::RomanBold,
        TextStyle::BoldItalic => FontId::RomanBoldItalic,
        TextStyle::SansSerif => FontId::SansRegular,
        TextStyle::SansSerifBold => FontId::SansBold,
        TextStyle::SansSerifItalic => FontId::SansItalic,
        TextStyle::SansSerifBoldItalic => FontId::SansBoldItalic,
        TextStyle::Monospace => FontId::MonoRegular,
        // Italic letters keep the Math-font math-italic path.
        TextStyle::Italic => return None,
        // No Latin Modern text equivalent → Math-font Unicode remap.
        TextStyle::DoubleStruck
        | TextStyle::Script
        | TextStyle::BoldScript
        | TextStyle::Fraktur
        | TextStyle::BoldFraktur => return None,
    })
}

/// Resolve the font identity for a [`BoxKind::TextRun`](super::layout::BoxKind)
/// from its `text_style`.
///
/// `None` (plain `\text`, operator names, digits) resolves to upright Roman;
/// styles with no Latin Modern equivalent resolve to `Math` (and the emitters
/// fall back to the Math-font glyph there).
#[inline]
pub fn font_for_text_run(text_style: Option<TextStyle>) -> FontId {
    match text_style {
        Some(style) => text_font_for_style(style).unwrap_or(FontId::Math),
        None => FontId::RomanRegular,
    }
}

/// Font-derived glyph metrics (em units) for a glyph drawn from `font`.
///
/// For [`FontId::Math`] this defers to the hand-tuned tables in
/// [`super::metrics`] (italic vs upright). For text faces it reads the advance
/// width and bounding box straight from the face so layout positions line up
/// with the drawn outlines. Glyphs absent from the chosen text face fall back
/// to the Math-font metric tables (the emitters fall back to the Math glyph).
#[inline]
pub fn text_metrics(ch: char, italic: bool, font: FontId) -> GlyphMetrics {
    if font.is_math() {
        return if italic {
            glyph_metrics(ch)
        } else {
            glyph_metrics_upright(ch)
        };
    }
    if let Some(level) = script_level(font)
        && let Some(glyph) = script_glyph(ch)
    {
        return glyph.metrics[level]
            .get_or_init(|| face_glyph_metrics(font, ch))
            .unwrap_or_else(|| glyph_metrics_upright(ch));
    }
    if let Some(hit) = TEXT_METRICS_CACHE
        .read()
        .expect("metrics cache")
        .get(&(font, ch))
    {
        return hit.unwrap_or_else(|| glyph_metrics_upright(ch));
    }
    let computed = face_glyph_metrics(font, ch);
    TEXT_METRICS_CACHE
        .write()
        .expect("metrics cache")
        .insert((font, ch), computed);
    computed.unwrap_or_else(|| glyph_metrics_upright(ch))
}

/// Read advance/bbox metrics for `ch` from `font`'s face, or `None` if absent.
fn face_glyph_metrics(font: FontId, ch: char) -> Option<GlyphMetrics> {
    let f = face(font);
    let upem = f64::from(f.units_per_em());
    let gid = glyph_index(font, ch)?;
    let width = f64::from(f.glyph_hor_advance(gid).unwrap_or(0)) / upem;
    let (height, depth) = match f.glyph_bounding_box(gid) {
        Some(bbox) => (
            (f64::from(bbox.y_max) / upem).max(0.0),
            (-f64::from(bbox.y_min) / upem).max(0.0),
        ),
        None => (0.0, 0.0),
    };
    // Only the Math font has a MATH table.
    let italic = f
        .tables()
        .math
        .and_then(|math| math.glyph_info?.italic_corrections?.get(gid))
        .map_or(0.0, |value| f64::from(value.value) / upem);
    Some(GlyphMetrics::new(height, depth, width, italic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_parses() {
        for id in ALL_FONTS {
            let f = face(id);
            assert!(f.units_per_em() > 0, "{id:?} should parse");
            assert!(font_info(id).units_per_em > 0.0, "{id:?} info");
        }
    }

    #[test]
    fn math_aliases_shared_font_data() {
        assert!(std::ptr::eq(
            font_bytes(FontId::Math),
            super::super::FONT_DATA
        ));
    }

    #[test]
    fn roman_regular_has_basic_latin() {
        let m = text_metrics('A', false, FontId::RomanRegular);
        assert!(m.width > 0.0 && m.height > 0.0);
    }

    #[test]
    fn mono_is_monospaced() {
        let i = text_metrics('i', false, FontId::MonoRegular);
        let w = text_metrics('w', false, FontId::MonoRegular);
        assert!(
            (i.width - w.width).abs() < 1e-6,
            "mono advances should be equal: i={} w={}",
            i.width,
            w.width
        );
    }

    #[test]
    fn missing_glyph_falls_back() {
        // A codepoint absent from lmroman falls back to a positive-width metric.
        let m = text_metrics('\u{1D538}', false, FontId::RomanRegular);
        assert!(m.width > 0.0);
    }

    #[test]
    fn style_mapping_keeps_specials_on_math() {
        assert_eq!(
            text_font_for_style(TextStyle::Bold),
            Some(FontId::RomanBold)
        );
        assert_eq!(
            text_font_for_style(TextStyle::Monospace),
            Some(FontId::MonoRegular)
        );
        assert_eq!(text_font_for_style(TextStyle::Italic), None);
        assert_eq!(text_font_for_style(TextStyle::DoubleStruck), None);
        assert_eq!(text_font_for_style(TextStyle::Fraktur), None);
        assert_eq!(font_for_text_run(None), FontId::RomanRegular);
        assert_eq!(
            font_for_text_run(Some(TextStyle::DoubleStruck)),
            FontId::Math
        );
    }
}
