//! `gensymb` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `gensymb` provides a handful of unit/typographic symbols that are valid in
//! both text and math mode. We implement them as preprocessor macros expanding
//! to the literal Unicode glyph, which makes them mode-agnostic.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("degree", 0, "\u{00B0}"),      // ° DEGREE SIGN
    ("celsius", 0, "\u{2103}"),     // ℃ DEGREE CELSIUS
    ("micro", 0, "\u{00B5}"),       // µ MICRO SIGN
    ("ohm", 0, "\u{03A9}"),         // Ω GREEK CAPITAL LETTER OMEGA
    ("perthousand", 0, "\u{2030}"), // ‰ PER MILLE SIGN
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];
