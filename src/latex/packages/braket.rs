//! `braket` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! Provides Dirac bra-ket notation. The parser (`parse.rs`) already handles the
//! single-argument `\braket`/`\Braket` (→ `⟨…⟩`) and `\Set`/`\set` (→ `{…}`)
//! forms, so we deliberately do **not** define macros for those names (a builtin
//! macro would expand in preprocessing and shadow the existing handler). We only
//! contribute the forms the parser is currently missing: `\bra`, `\ket`,
//! `\ketbra`, and the auto-scaling `\Bra` / `\Ket`.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// All bodies expand to constructs the parser already supports
/// (`\langle`, `\rangle`, the `|` delimiter, `\left`/`\right`).
///
/// Note: this parser does not recognise `\vert`, so the literal `|` delimiter
/// (which is parser-supported and renders as the same glyph) is used instead.
pub const MACROS: &[(&str, u8, &str)] = &[
    // \bra{x} → ⟨x|
    ("bra", 1, r"\langle{#1}|"),
    // \ket{x} → |x⟩
    ("ket", 1, r"|{#1}\rangle"),
    // \ketbra{x}{y} → |x⟩⟨y|
    ("ketbra", 2, r"|{#1}\rangle\langle{#2}|"),
    // \Bra{x} → auto-scaling ⟨x| via \left\langle … \right|
    ("Bra", 1, r"\left\langle{#1}\right|"),
    // \Ket{x} → auto-scaling |x⟩ via \left| … \right\rangle
    ("Ket", 1, r"\left|{#1}\right\rangle"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::convert;

    fn mathml(latex: &str) -> String {
        convert::from_latex(latex).unwrap().to_mathml().unwrap()
    }

    const LANGLE: char = '\u{27E8}'; // ⟨
    const RANGLE: char = '\u{27E9}'; // ⟩

    #[test]
    fn bra_renders_langle_and_bar() {
        let out = mathml(r"\bra{x}");
        assert!(out.contains(LANGLE), "expected ⟨ in: {out}");
        assert!(out.contains('|'), "expected bar in: {out}");
        assert!(out.contains('x'));
        assert!(!out.contains(RANGLE), "bra should not contain ⟩: {out}");
    }

    #[test]
    fn ket_renders_bar_and_rangle() {
        let out = mathml(r"\ket{\psi}");
        assert!(out.contains(RANGLE), "expected ⟩ in: {out}");
        assert!(out.contains('|'), "expected bar in: {out}");
        assert!(!out.contains(LANGLE), "ket should not contain ⟨: {out}");
    }

    #[test]
    fn ketbra_renders_both_pairs() {
        let out = mathml(r"\ketbra{x}{y}");
        assert!(out.contains(LANGLE), "expected ⟨ in: {out}");
        assert!(out.contains(RANGLE), "expected ⟩ in: {out}");
        assert!(out.contains('x') && out.contains('y'));
    }

    #[test]
    fn big_bra_auto_scaling() {
        // \Bra expands via \left\langle … \right| → a fenced/stretchy delimiter.
        let out = mathml(r"\Bra{\frac{x}{y}}");
        assert!(out.contains(LANGLE), "expected ⟨ in: {out}");
        assert!(out.contains('|'), "expected bar in: {out}");
    }

    #[test]
    fn big_ket_auto_scaling() {
        let out = mathml(r"\Ket{\frac{x}{y}}");
        assert!(out.contains(RANGLE), "expected ⟩ in: {out}");
        assert!(out.contains('|'), "expected bar in: {out}");
    }

    #[test]
    fn bra_unicode_math_path() {
        let out = convert::from_latex(r"\bra{x}")
            .unwrap()
            .to_unicode_math()
            .unwrap();
        assert!(out.contains(LANGLE), "expected ⟨ in unicode: {out}");
    }

    #[cfg(feature = "render")]
    #[test]
    fn ket_svg_path() {
        let svg = convert::from_latex(r"\ket{x}").unwrap().to_svg().unwrap();
        assert!(svg.starts_with("<svg"), "expected svg output");
    }

    #[test]
    fn user_macro_overrides_builtin() {
        let out = mathml(r"\newcommand{\ket}[1]{Z#1}\ket{x}");
        // User redefinition must win: no ⟩ delimiter should be emitted.
        assert!(!out.contains(RANGLE), "user redefinition must win: {out}");
        assert!(out.contains('Z'), "expected user body: {out}");
    }

    // Pin: parser's single-arg \braket / \Set must NOT regress (we add no macro
    // for them, so the existing parse.rs handler stays in control).
    #[test]
    fn single_arg_braket_unregressed() {
        let out = mathml(r"\braket{x}");
        assert!(out.contains(LANGLE) && out.contains(RANGLE), "got: {out}");
    }

    #[test]
    fn set_unregressed() {
        let out = mathml(r"\Set{x}");
        assert!(out.contains('{') || out.contains("&#x"), "got: {out}");
    }
}
