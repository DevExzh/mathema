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
/// `\textsuperscript{<arg>}` raises its argument. Because macros are expanded by
/// the preprocessor *before* parsing, the body cannot inspect the preceding
/// atom directly — but it does not need to. Expanding to `^{#1}` simply emits a
/// raw `^` superscript token into the stream, which the parser then attaches to
/// whatever atom precedes the command, exactly as a hand-written `x^{2}` would.
/// This fixes the previous empty-base bug (`x\textsuperscript{2}` used to yield
/// `<mi>x</mi><msup><mrow></mrow><mn>2</mn></msup>`; it now yields the correct
/// `<msup><mi>x</mi><mn>2</mn></msup>`).
///
/// Limitation: when `\textsuperscript` has no preceding atom (e.g. at the start
/// of input), the `^` still falls back to an empty base — the same graceful
/// degradation a bare `^{2}` would produce. Real textcomp also renders its
/// argument in text mode; we keep `^{#1}` (rather than `^{\text{#1}}`) because
/// it produces the cleanest, least-wrong MathML for the common `x\textsuperscript{n}`
/// case while still attaching to the preceding atom.
pub const MACROS: &[(&str, u8, &str)] = &[("textsuperscript", 1, "^{#1}")];

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
    // --- ASCII punctuation aliases (textcomp-specific names) ----------------
    ("textdollar", "\u{0024}", SymbolClass::Ordinary), // $ DOLLAR SIGN
    ("textasciitilde", "\u{007E}", SymbolClass::Ordinary), // ~ TILDE
    ("textasciicircum", "\u{005E}", SymbolClass::Ordinary), // ^ CIRCUMFLEX ACCENT
    ("textbackslash", "\u{005C}", SymbolClass::Ordinary), // \ REVERSE SOLIDUS
    ("textbar", "\u{007C}", SymbolClass::Ordinary),    // | VERTICAL LINE
    ("textless", "\u{003C}", SymbolClass::Ordinary),   // < LESS-THAN SIGN
    ("textgreater", "\u{003E}", SymbolClass::Ordinary), // > GREATER-THAN SIGN
    ("textquotesingle", "\u{0027}", SymbolClass::Ordinary), // ' APOSTROPHE
    // --- ordinals & superscript digits -------------------------------------
    ("textordmasculine", "\u{00BA}", SymbolClass::Ordinary), // º MASCULINE ORDINAL INDICATOR
    ("textordfeminine", "\u{00AA}", SymbolClass::Ordinary),  // ª FEMININE ORDINAL INDICATOR
    ("textonesuperior", "\u{00B9}", SymbolClass::Ordinary),  // ¹ SUPERSCRIPT ONE
    ("texttwosuperior", "\u{00B2}", SymbolClass::Ordinary),  // ² SUPERSCRIPT TWO
    ("textthreesuperior", "\u{00B3}", SymbolClass::Ordinary), // ³ SUPERSCRIPT THREE
    // --- quotation marks & dashes ------------------------------------------
    ("textquotedblleft", "\u{201C}", SymbolClass::Ordinary), // “ LEFT DOUBLE QUOTATION MARK
    ("textquotedblright", "\u{201D}", SymbolClass::Ordinary), // ” RIGHT DOUBLE QUOTATION MARK
    ("textquoteleft", "\u{2018}", SymbolClass::Ordinary),    // ‘ LEFT SINGLE QUOTATION MARK
    ("textquoteright", "\u{2019}", SymbolClass::Ordinary),   // ’ RIGHT SINGLE QUOTATION MARK
    ("textendash", "\u{2013}", SymbolClass::Ordinary),       // – EN DASH
    ("textemdash", "\u{2014}", SymbolClass::Ordinary),       // — EM DASH
    // --- miscellaneous symbols ---------------------------------------------
    ("textbrokenbar", "\u{00A6}", SymbolClass::Ordinary), // ¦ BROKEN BAR
    ("textflorin", "\u{0192}", SymbolClass::Ordinary),    // ƒ LATIN SMALL LETTER F WITH HOOK
    ("textlnot", "\u{00AC}", SymbolClass::Ordinary),      // ¬ NOT SIGN
    ("textsurd", "\u{221A}", SymbolClass::Ordinary),      // √ SQUARE ROOT
    ("textcurrency", "\u{00A4}", SymbolClass::Ordinary),  // ¤ CURRENCY SIGN
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
        // `\textsuperscript` now anchors to the PRECEDING atom rather than an
        // empty base: `x\textsuperscript{2}` -> `<msup><mi>x</mi><mn>2</mn></msup>`.
        let mathml = to_mathml("x\\textsuperscript{2}");
        assert!(mathml.contains("msup"), "expected msup, got: {mathml}");
        assert!(
            !mathml.contains("<mrow></mrow>"),
            "superscript should anchor to `x`, not an empty base: {mathml}"
        );
    }

    #[test]
    fn ascii_punctuation_aliases() {
        assert!(to_mathml("\\textdollar").contains('\u{0024}'));
        assert!(to_mathml("\\textasciitilde").contains('\u{007E}'));
        assert!(to_mathml("\\textasciicircum").contains('\u{005E}'));
        assert!(to_mathml("\\textbackslash").contains('\u{005C}'));
        assert!(to_mathml("\\textbar").contains('\u{007C}'));
        // `'` is XML-escaped to `&apos;` in MathML output.
        assert!(to_mathml("\\textquotesingle").contains("&apos;"));
    }

    #[test]
    fn ordinals_and_superscript_digits() {
        assert!(to_mathml("\\textordmasculine").contains('\u{00BA}'));
        assert!(to_mathml("\\textordfeminine").contains('\u{00AA}'));
        assert!(to_mathml("\\textonesuperior").contains('\u{00B9}'));
        assert!(to_mathml("\\texttwosuperior").contains('\u{00B2}'));
        assert!(to_mathml("\\textthreesuperior").contains('\u{00B3}'));
    }

    #[test]
    fn quotes_dashes_and_misc() {
        assert!(to_mathml("\\textquotedblleft").contains('\u{201C}'));
        assert!(to_mathml("\\textquotedblright").contains('\u{201D}'));
        assert!(to_mathml("\\textquoteleft").contains('\u{2018}'));
        assert!(to_mathml("\\textquoteright").contains('\u{2019}'));
        assert!(to_mathml("\\textendash").contains('\u{2013}'));
        assert!(to_mathml("\\textemdash").contains('\u{2014}'));
        assert!(to_mathml("\\textbrokenbar").contains('\u{00A6}'));
        assert!(to_mathml("\\textflorin").contains('\u{0192}'));
        assert!(to_mathml("\\textlnot").contains('\u{00AC}'));
        assert!(to_mathml("\\textsurd").contains('\u{221A}'));
        assert!(to_mathml("\\textcurrency").contains('\u{00A4}'));
    }

    #[test]
    fn unicode_output_path() {
        assert!(to_unicode("\\texteuro").contains('\u{20AC}'));
        assert!(to_unicode("\\textdegree").contains('\u{00B0}'));
    }
}
