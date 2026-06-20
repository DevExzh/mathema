//! `mathtools` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! This file contributes the most common `mathtools` shorthands as macros and
//! symbols that expand to constructs the parser already supports:
//!
//! * **Relation colons** (`\coloneqq`, `\eqqcolon`, `\Coloneqq`, `\vcentcolon`,
//!   `\dblcolon`) are emitted as their Unicode relation glyphs via [`SYMBOLS`].
//!
//! Handled directly by the parser (not as fixed-arity macros, so the starred
//! auto-size forms work):
//!
//! * **Paired delimiters** (`\abs`, `\norm`, `\ceil`, `\floor`) and their
//!   starred forms (`\abs*`, `\ceil*`, …) wrap their argument in an
//!   auto-scaling `Fenced` (`parse_paired_delim`).
//! * `\DeclarePairedDelimiter\cmd{open}{close}` registers a new paired
//!   delimiter `\cmd` / `\cmd*` (`parse_declare_paired_delim`).
//! * the `multlined` environment is laid out like `multline`.
//!
//! Note: `\mathclap`, `\mathllap`, `\mathrlap` (and `\clap`/`\llap`/`\rlap`)
//! are already handled directly by the parser (they emit their content), so
//! this package intentionally does **not** define macros for them — doing so
//! would shadow the established parser behavior.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Empty: the paired-delimiter shorthands (`\abs`, `\norm`, `\ceil`, `\floor`)
/// are handled directly by the parser (`parse_paired_delim`), which also
/// supports the starred auto-size forms (`\ceil*`, `\floor*`, …) and
/// `\DeclarePairedDelimiter`. Defining them as fixed-arity macros here could not
/// consume the optional `*`.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// The colon-relation shorthands render as their dedicated Unicode glyphs and
/// are classified as relations so spacing matches `:=`-style operators.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    ("coloneqq", "\u{2254}", SymbolClass::Relation), // ≔  ( := )
    ("eqqcolon", "\u{2255}", SymbolClass::Relation), // ≕  ( =: )
    ("Coloneqq", "\u{2A74}", SymbolClass::Relation), // ⩴  ( ::= )
    ("vcentcolon", "\u{2236}", SymbolClass::Relation), // ∶  ( centered colon )
    ("dblcolon", "\u{2237}", SymbolClass::Relation), // ∷  ( double colon )
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

    #[test]
    fn starred_paired_delimiters_consume_star() {
        // `\ceil*{x}` must not leave a stray `*`; it auto-scales like `\ceil`.
        let out = mathml(r"\ceil*{x}");
        assert!(
            out.contains('\u{2308}') && out.contains('\u{2309}'),
            "got: {out}"
        );
        assert!(!out.contains('*'), "star must be consumed, got: {out}");
        let out2 = mathml(r"\abs*{y}");
        assert_eq!(out2.matches('|').count(), 2, "got: {out2}");
        assert!(!out2.contains('*'), "star must be consumed, got: {out2}");
    }

    #[test]
    fn declare_paired_delimiter_registers_command() {
        // \DeclarePairedDelimiter\bra{\langle}{\rangle} → \bra{x} = ⟨x⟩.
        let out = mathml(r"\DeclarePairedDelimiter\anglebr{\langle}{\rangle}\anglebr{x}");
        assert!(out.contains('\u{27E8}'), "expected ⟨ in: {out}");
        assert!(out.contains('\u{27E9}'), "expected ⟩ in: {out}");
        // Starred form works too.
        let out2 = mathml(r"\DeclarePairedDelimiter\anglebr{\langle}{\rangle}\anglebr*{x}");
        assert!(
            out2.contains('\u{27E8}') && out2.contains('\u{27E9}'),
            "got: {out2}"
        );
    }

    #[test]
    fn multlined_environment_lays_out_rows() {
        let out = mathml(r"\begin{multlined} a+b \\ +c \end{multlined}");
        assert!(out.contains("<mtable"), "expected mtable, got: {out}");
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
