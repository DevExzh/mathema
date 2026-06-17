//! `amssymb` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `amssymb` defines several hundred AMS math symbols. A large fraction of them
//! already live in the core PHF symbol table (`src/unicode/symbols.rs`), which
//! always wins over the package fallback, so this file only contributes the
//! entries that are **missing** from core. Each entry maps a command name to its
//! Unicode glyph and a [`SymbolClass`] (relation / binary-op / ordinary).
//!
//! Families covered here (only the missing members): extra binary relations,
//! negated relations, extra arrows, miscellaneous ordinary symbols, and extra
//! binary operators.

use super::SymbolClass;
use SymbolClass::{BinaryOp, Ordinary, Relation};

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// `amssymb` is purely a symbol-table package, so it contributes no macros.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// Only symbols **absent** from the core PHF table are listed here; core entries
/// win regardless, so duplicating them would be dead weight.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    // === Extra binary relations ====================================
    ("approxeq", "\u{224A}", Relation), // ≊ ALMOST EQUAL OR EQUAL TO
    ("backsim", "\u{223D}", Relation),  // ∽ REVERSED TILDE
    ("bumpeq", "\u{224F}", Relation),   // ≏ DIFFERENCE BETWEEN
    ("Bumpeq", "\u{224E}", Relation),   // ≎ GEOMETRICALLY EQUIVALENT TO
    ("between", "\u{226C}", Relation),  // ≬ BETWEEN
    ("eqslantless", "\u{2A95}", Relation), // ⪕ SLANTED EQUAL TO OR LESS-THAN
    ("thickapprox", "\u{2248}", Relation), // ≈ ALMOST EQUAL TO (thick variant)
    ("sqsubset", "\u{228F}", Relation), // ⊏ SQUARE IMAGE OF
    ("sqsupset", "\u{2290}", Relation), // ⊐ SQUARE ORIGINAL OF
    ("Subset", "\u{22D0}", Relation),   // ⋐ DOUBLE SUBSET
    ("Supset", "\u{22D1}", Relation),   // ⋑ DOUBLE SUPERSET
    ("vDash", "\u{22A8}", Relation),    // ⊨ TRUE
    ("Vdash", "\u{22A9}", Relation),    // ⊩ FORCES
    ("Vvdash", "\u{22AA}", Relation),   // ⊪ TRIPLE VERTICAL BAR RIGHT TURNSTILE
    ("blacktriangleleft", "\u{25C2}", Relation), // ◂ BLACK LEFT-POINTING SMALL TRIANGLE
    // === Negated relations =========================================
    ("nless", "\u{226E}", Relation),        // ≮ NOT LESS-THAN
    ("ngtr", "\u{226F}", Relation),         // ≯ NOT GREATER-THAN
    ("nsim", "\u{2241}", Relation),         // ≁ NOT TILDE
    ("ncong", "\u{2247}", Relation),        // ≇ NEITHER APPROXIMATELY NOR ACTUALLY EQUAL TO
    ("nvdash", "\u{22AC}", Relation),       // ⊬ DOES NOT PROVE
    ("nvDash", "\u{22AD}", Relation),       // ⊭ NOT TRUE
    ("varsubsetneq", "\u{228A}", Relation), // ⊊ SUBSET OF WITH NOT EQUAL TO (variant)
    // === Negated arrows ============================================
    ("nrightarrow", "\u{219B}", Relation), // ↛ RIGHTWARDS ARROW WITH STROKE
    ("nleftarrow", "\u{219A}", Relation),  // ↚ LEFTWARDS ARROW WITH STROKE
    ("nleftrightarrow", "\u{21AE}", Relation), // ↮ LEFT RIGHT ARROW WITH STROKE
    ("nRightarrow", "\u{21CF}", Relation), // ⇏ RIGHTWARDS DOUBLE ARROW WITH STROKE
    ("nLeftarrow", "\u{21CD}", Relation),  // ⇍ LEFTWARDS DOUBLE ARROW WITH STROKE
    ("nLeftrightarrow", "\u{21CE}", Relation), // ⇎ LEFT RIGHT DOUBLE ARROW WITH STROKE
    // === Extra arrows ==============================================
    ("dashrightarrow", "\u{21E2}", Relation), // ⇢ RIGHTWARDS DASHED ARROW
    ("dashleftarrow", "\u{21E0}", Relation),  // ⇠ LEFTWARDS DASHED ARROW
    ("rightleftarrows", "\u{21C4}", Relation), // ⇄ RIGHTWARDS ARROW OVER LEFTWARDS ARROW
    ("downdownarrows", "\u{21CA}", Relation), // ⇊ DOWNWARDS PAIRED ARROWS
    ("Lleftarrow", "\u{21DA}", Relation),     // ⇚ LEFTWARDS TRIPLE ARROW
    ("Rrightarrow", "\u{21DB}", Relation),    // ⇛ RIGHTWARDS TRIPLE ARROW
    ("rightarrowtail", "\u{21A3}", Relation), // ↣ RIGHTWARDS ARROW WITH TAIL
    ("leftarrowtail", "\u{21A2}", Relation),  // ↢ LEFTWARDS ARROW WITH TAIL
    ("looparrowright", "\u{21AC}", Relation), // ↬ RIGHTWARDS ARROW WITH LOOP
    ("looparrowleft", "\u{21AB}", Relation),  // ↫ LEFTWARDS ARROW WITH LOOP
    // === Miscellaneous ordinary symbols ============================
    ("bigstar", "\u{2605}", Ordinary),        // ★ BLACK STAR
    ("lozenge", "\u{25CA}", Ordinary),        // ◊ LOZENGE
    ("hslash", "\u{210F}", Ordinary),         // ℏ PLANCK CONSTANT OVER TWO PI
    ("Bbbk", "\u{1D55C}", Ordinary),          // 𝕜 MATHEMATICAL DOUBLE-STRUCK SMALL K
    ("circledR", "\u{00AE}", Ordinary),       // ® REGISTERED SIGN
    ("circledS", "\u{24C8}", Ordinary),       // Ⓢ CIRCLED LATIN CAPITAL LETTER S
    ("sphericalangle", "\u{2222}", Ordinary), // ∢ SPHERICAL ANGLE
    ("Finv", "\u{2132}", Ordinary),           // Ⅎ TURNED CAPITAL F
    ("Game", "\u{2141}", Ordinary),           // ⅁ TURNED SANS-SERIF CAPITAL G
    ("diagup", "\u{2571}", Ordinary), // ╱ BOX DRAWINGS LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT
    ("diagdown", "\u{2572}", Ordinary), // ╲ BOX DRAWINGS LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT
    ("mho", "\u{2127}", Ordinary),    // ℧ INVERTED OHM SIGN
    // === Extra binary operators ====================================
    ("Cap", "\u{22D2}", BinaryOp),      // ⋒ DOUBLE INTERSECTION
    ("Cup", "\u{22D3}", BinaryOp),      // ⋓ DOUBLE UNION
    ("barwedge", "\u{22BC}", BinaryOp), // ⊼ NAND
    ("veebar", "\u{22BB}", BinaryOp),   // ⊻ XOR
];

#[cfg(test)]
mod tests {
    use crate::convert;

    fn mathml(latex: &str) -> String {
        convert::from_latex(latex).unwrap().to_mathml().unwrap()
    }

    fn unicode(latex: &str) -> String {
        convert::from_latex(latex)
            .unwrap()
            .to_unicode_math()
            .unwrap()
    }

    #[test]
    fn extra_relation_between() {
        assert!(mathml(r"a \between b").contains('\u{226C}'));
    }

    #[test]
    fn extra_relation_subset_supset() {
        assert!(mathml(r"A \Subset B").contains('\u{22D0}'));
        assert!(mathml(r"A \Supset B").contains('\u{22D1}'));
    }

    #[test]
    fn turnstiles_vdash_family() {
        assert!(mathml(r"\Gamma \vDash \phi").contains('\u{22A8}'));
        assert!(mathml(r"\Gamma \Vdash \phi").contains('\u{22A9}'));
        assert!(mathml(r"\Gamma \Vvdash \phi").contains('\u{22AA}'));
    }

    #[test]
    fn negated_relations() {
        assert!(mathml(r"a \nless b").contains('\u{226E}'));
        assert!(mathml(r"a \ngtr b").contains('\u{226F}'));
        assert!(mathml(r"a \ncong b").contains('\u{2247}'));
    }

    #[test]
    fn negated_arrows() {
        assert!(mathml(r"a \nrightarrow b").contains('\u{219B}'));
        assert!(mathml(r"a \nLeftrightarrow b").contains('\u{21CE}'));
    }

    #[test]
    fn extra_arrows() {
        assert!(mathml(r"a \dashrightarrow b").contains('\u{21E2}'));
        assert!(mathml(r"a \Rrightarrow b").contains('\u{21DB}'));
        assert!(mathml(r"a \rightarrowtail b").contains('\u{21A3}'));
    }

    #[test]
    fn misc_ordinary_symbols() {
        assert!(mathml(r"\bigstar").contains('\u{2605}'));
        assert!(mathml(r"\lozenge").contains('\u{25CA}'));
        assert!(mathml(r"\Game").contains('\u{2141}'));
        assert!(mathml(r"\Bbbk").contains('\u{1D55C}'));
    }

    #[test]
    fn extra_binary_operators() {
        assert!(mathml(r"A \Cap B").contains('\u{22D2}'));
        assert!(mathml(r"A \Cup B").contains('\u{22D3}'));
        assert!(mathml(r"a \barwedge b").contains('\u{22BC}'));
        assert!(mathml(r"a \veebar b").contains('\u{22BB}'));
    }

    #[test]
    fn approxeq_and_backsim() {
        assert!(mathml(r"a \approxeq b").contains('\u{224A}'));
        assert!(mathml(r"a \backsim b").contains('\u{223D}'));
    }

    #[test]
    fn unicode_output_path() {
        assert!(unicode(r"a \Subset b").contains('\u{22D0}'));
        assert!(unicode(r"\bigstar").contains('\u{2605}'));
    }
}
