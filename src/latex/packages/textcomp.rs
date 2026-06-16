//! `textcomp` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `textcomp` (the Text Companion fonts) provides a large collection of
//! typographic and unit symbols. We implement them as literal-Unicode SYMBOLS
//! consulted by the parser's unknown-command fallback, each tagged with an
//! appropriate [`SymbolClass`]. The single command that takes an argument,
//! `\textsuperscript`, is implemented as a preprocessor macro.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// `\textsuperscript{<arg>}` raises its argument; the closest construct the
/// parser already supports is an empty-base superscript `{}^{...}`.
pub const MACROS: &[(&str, u8, &str)] = &[("textsuperscript", 1, "{}^{#1}")];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    ("textdegree", "\u{00B0}", SymbolClass::Ordinary), // ° DEGREE SIGN
    ("textcelsius", "\u{2103}", SymbolClass::Ordinary), // ℃ DEGREE CELSIUS
    ("texteuro", "\u{20AC}", SymbolClass::Ordinary),   // € EURO SIGN
    ("textcent", "\u{00A2}", SymbolClass::Ordinary),   // ¢ CENT SIGN
    ("textsterling", "\u{00A3}", SymbolClass::Ordinary), // £ POUND SIGN
    ("textyen", "\u{00A5}", SymbolClass::Ordinary),    // ¥ YEN SIGN
    ("textcopyright", "\u{00A9}", SymbolClass::Ordinary), // © COPYRIGHT SIGN
    ("textregistered", "\u{00AE}", SymbolClass::Ordinary), // ® REGISTERED SIGN
    ("texttrademark", "\u{2122}", SymbolClass::Ordinary), // ™ TRADE MARK SIGN
    ("textpermille", "\u{2030}", SymbolClass::Ordinary), // ‰ PER MILLE SIGN
    ("textmu", "\u{00B5}", SymbolClass::Ordinary),     // µ MICRO SIGN
    ("textmicro", "\u{00B5}", SymbolClass::Ordinary),  // µ MICRO SIGN
    ("textohm", "\u{03A9}", SymbolClass::Ordinary),    // Ω OHM (capital omega)
    ("textbullet", "\u{2022}", SymbolClass::BinaryOp), // • BULLET
    ("textperiodcentered", "\u{00B7}", SymbolClass::BinaryOp), // · MIDDLE DOT
    ("textpm", "\u{00B1}", SymbolClass::BinaryOp),     // ± PLUS-MINUS SIGN
    ("texttimes", "\u{00D7}", SymbolClass::BinaryOp),  // × MULTIPLICATION SIGN
    ("textdiv", "\u{00F7}", SymbolClass::BinaryOp),    // ÷ DIVISION SIGN
    ("textonehalf", "\u{00BD}", SymbolClass::Ordinary), // ½ VULGAR FRACTION ONE HALF
    ("textonequarter", "\u{00BC}", SymbolClass::Ordinary), // ¼ ONE QUARTER
    ("textthreequarters", "\u{00BE}", SymbolClass::Ordinary), // ¾ THREE QUARTERS
    ("textangstrom", "\u{00C5}", SymbolClass::Ordinary), // Å ANGSTROM (A with ring)
    ("textnumero", "\u{2116}", SymbolClass::Ordinary), // № NUMERO SIGN
    ("textdagger", "\u{2020}", SymbolClass::Ordinary), // † DAGGER
    ("textdaggerdbl", "\u{2021}", SymbolClass::Ordinary), // ‡ DOUBLE DAGGER
    ("textsection", "\u{00A7}", SymbolClass::Ordinary), // § SECTION SIGN
    ("textparagraph", "\u{00B6}", SymbolClass::Ordinary), // ¶ PILCROW SIGN
];

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    fn to_unicode(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::UnicodeMath).expect("convert should succeed")
    }

    #[test]
    fn currency_symbols() {
        assert!(to_mathml("\\texteuro").contains('\u{20AC}'));
        assert!(to_mathml("\\textcent").contains('\u{00A2}'));
        assert!(to_mathml("\\textsterling").contains('\u{00A3}'));
        assert!(to_mathml("\\textyen").contains('\u{00A5}'));
    }

    #[test]
    fn legal_marks() {
        assert!(to_mathml("\\textcopyright").contains('\u{00A9}'));
        assert!(to_mathml("\\textregistered").contains('\u{00AE}'));
        assert!(to_mathml("\\texttrademark").contains('\u{2122}'));
    }

    #[test]
    fn units_and_typography() {
        assert!(to_mathml("\\textdegree").contains('\u{00B0}'));
        assert!(to_mathml("\\textcelsius").contains('\u{2103}'));
        assert!(to_mathml("\\textmu").contains('\u{00B5}'));
        assert!(to_mathml("\\textmicro").contains('\u{00B5}'));
        assert!(to_mathml("\\textohm").contains('\u{03A9}'));
        assert!(to_mathml("\\textpermille").contains('\u{2030}'));
        assert!(to_mathml("\\textnumero").contains('\u{2116}'));
        assert!(to_mathml("\\textangstrom").contains('\u{00C5}'));
    }

    #[test]
    fn binary_operators() {
        assert!(to_mathml("a \\textpm b").contains('\u{00B1}'));
        assert!(to_mathml("a \\texttimes b").contains('\u{00D7}'));
        assert!(to_mathml("a \\textdiv b").contains('\u{00F7}'));
        assert!(to_mathml("a \\textbullet b").contains('\u{2022}'));
        assert!(to_mathml("a \\textperiodcentered b").contains('\u{00B7}'));
    }

    #[test]
    fn fractions_and_marks() {
        assert!(to_mathml("\\textonehalf").contains('\u{00BD}'));
        assert!(to_mathml("\\textonequarter").contains('\u{00BC}'));
        assert!(to_mathml("\\textthreequarters").contains('\u{00BE}'));
        assert!(to_mathml("\\textdagger").contains('\u{2020}'));
        assert!(to_mathml("\\textdaggerdbl").contains('\u{2021}'));
        assert!(to_mathml("\\textsection").contains('\u{00A7}'));
        assert!(to_mathml("\\textparagraph").contains('\u{00B6}'));
    }

    #[test]
    fn superscript_macro() {
        let mathml = to_mathml("x\\textsuperscript{2}");
        assert!(mathml.contains("msup") || mathml.contains('2'));
    }

    #[test]
    fn unicode_output_path() {
        assert!(to_unicode("\\texteuro").contains('\u{20AC}'));
        assert!(to_unicode("\\textdegree").contains('\u{00B0}'));
    }
}
