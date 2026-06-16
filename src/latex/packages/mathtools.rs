//! `mathtools` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! This file contributes the most common `mathtools` shorthands as macros and
//! symbols that expand to constructs the parser already supports:
//!
//! * **Relation colons** (`\coloneqq`, `\eqqcolon`, `\Coloneqq`, `\vcentcolon`,
//!   `\dblcolon`) are emitted as their Unicode relation glyphs via [`SYMBOLS`].
//! * **Paired delimiters** (`\abs`, `\norm`, `\ceil`, `\floor`) expand to
//!   auto-scaling `\left ... \right` groups via [`MACROS`].
//!
//! Deferred (need structural / parser support, not expressible as a simple
//! macro or symbol):
//!
//! * `\DeclarePairedDelimiter` — declares *new* user macros; this is a
//!   meta-definition facility, not a fixed expansion.
//! * the `multlined` environment — a multi-line structural environment.
//! * the starred paired-delimiter forms (`\ceil*`, `\floor*`, ...) — the `*`
//!   is a separate token the preprocessor cannot fold into the macro name, so
//!   the unstarred (already auto-scaling) forms are provided instead.
//!
//! Note: `\mathclap`, `\mathllap`, `\mathrlap` (and `\clap`/`\llap`/`\rlap`)
//! are already handled directly by the parser (they emit their content), so
//! this package intentionally does **not** define macros for them — doing so
//! would shadow the established parser behavior.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// The paired-delimiter shorthands expand to auto-scaling `\left ... \right`
/// groups. `\abs`/`\norm` deliberately mirror the bodies the `physics` package
/// would use (registry is later-wins, so an identical body keeps behavior
/// stable regardless of which package "wins").
pub const MACROS: &[(&str, u8, &str)] = &[
    ("abs", 1, r"\left|#1\right|"),
    ("norm", 1, r"\left\|#1\right\|"),
    ("ceil", 1, r"\left\lceil #1\right\rceil"),
    ("floor", 1, r"\left\lfloor #1\right\rfloor"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// The colon-relation shorthands render as their dedicated Unicode glyphs and
/// are classified as relations so spacing matches `:=`-style operators.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    ("coloneqq", "\u{2254}", SymbolClass::Relation),   // ≔  ( := )
    ("eqqcolon", "\u{2255}", SymbolClass::Relation),   // ≕  ( =: )
    ("Coloneqq", "\u{2A74}", SymbolClass::Relation),   // ⩴  ( ::= )
    ("vcentcolon", "\u{2236}", SymbolClass::Relation), // ∶  ( centered colon )
    ("dblcolon", "\u{2237}", SymbolClass::Relation),   // ∷  ( double colon )
];

#[cfg(test)]
mod tests {
    use crate::convert;

    fn mathml(latex: &str) -> String {
        convert::from_latex(latex)
            .expect("parse should succeed")
            .to_mathml()
            .expect("mathml should succeed")
    }

    fn unicode(latex: &str) -> String {
        convert::from_latex(latex)
            .expect("parse should succeed")
            .to_unicode_math()
            .expect("unicode should succeed")
    }

    // --- relation colons ---------------------------------------------------

    #[test]
    fn coloneqq_glyph() {
        assert!(mathml(r"a \coloneqq b").contains('\u{2254}'));
        assert!(unicode(r"a \coloneqq b").contains('\u{2254}'));
    }

    #[test]
    fn eqqcolon_glyph() {
        assert!(mathml(r"a \eqqcolon b").contains('\u{2255}'));
    }

    #[test]
    fn capital_coloneqq_glyph() {
        assert!(mathml(r"a \Coloneqq b").contains('\u{2A74}'));
    }

    #[test]
    fn vcentcolon_glyph() {
        assert!(mathml(r"\vcentcolon").contains('\u{2236}'));
    }

    #[test]
    fn dblcolon_glyph() {
        assert!(mathml(r"\dblcolon").contains('\u{2237}'));
    }

    // --- paired delimiters -------------------------------------------------

    #[test]
    fn abs_wraps_in_bars() {
        let out = mathml(r"\abs{x}");
        // \left|x\right| -> two vertical-bar fences around x.
        assert_eq!(out.matches('|').count(), 2, "expected two bars in: {out}");
        assert!(out.contains("<mo"));
    }

    #[test]
    fn norm_wraps_in_double_bars() {
        let out = mathml(r"\norm{v}");
        // \| renders as the double vertical bar U+2016 (or two ASCII bars).
        assert!(
            out.contains('\u{2016}') || out.matches('|').count() >= 4,
            "expected double bars in: {out}"
        );
    }

    #[test]
    fn ceil_uses_ceiling_brackets() {
        let out = mathml(r"\ceil{x}");
        assert!(out.contains('\u{2308}'), "expected lceil in: {out}");
        assert!(out.contains('\u{2309}'), "expected rceil in: {out}");
    }

    #[test]
    fn floor_uses_floor_brackets() {
        let out = mathml(r"\floor{x}");
        assert!(out.contains('\u{230A}'), "expected lfloor in: {out}");
        assert!(out.contains('\u{230B}'), "expected rfloor in: {out}");
    }

    #[cfg(feature = "render")]
    #[test]
    fn floor_renders_to_svg() {
        let svg = convert::from_latex(r"\floor{x}")
            .expect("parse should succeed")
            .to_svg()
            .expect("svg should succeed");
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[cfg(feature = "render")]
    #[test]
    fn coloneqq_renders_to_svg() {
        let svg = convert::from_latex(r"a \coloneqq b")
            .expect("parse should succeed")
            .to_svg()
            .expect("svg should succeed");
        assert!(svg.starts_with("<svg"));
    }
}
