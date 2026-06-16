//! `upgreek` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `upgreek` provides "upright" Greek letters: `\upalpha`..`\upomega` (lowercase)
//! and `\upGamma`, `\upDelta`, … (uppercase). We map each command to the plain
//! Greek Unicode codepoint (U+03B1.. for lowercase, U+0393.. for uppercase) with
//! the [`Alphabetic`](SymbolClass::Alphabetic) class, emitted through the parser's
//! unknown-command fallback.
//!
//! NOTE: a *true* upright-vs-italic distinction is a font/`mathvariant` matter,
//! not a codepoint matter — the same Greek codepoints render italic in the math
//! italic font and upright in a roman font. Mapping to the plain Greek
//! codepoints is the standard, portable approach (it matches how upgreek is
//! typically transcribed to Unicode/MathML), so that is what we do here.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// Lowercase `\up<letter>` -> U+03B1.., uppercase `\up<Letter>` -> U+0393..,
/// all classified as [`Alphabetic`](SymbolClass::Alphabetic).
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[
    // --- lowercase: \upalpha .. \upomega -------------------------------------
    ("upalpha", "\u{03B1}", SymbolClass::Alphabetic),   // α
    ("upbeta", "\u{03B2}", SymbolClass::Alphabetic),    // β
    ("upgamma", "\u{03B3}", SymbolClass::Alphabetic),   // γ
    ("updelta", "\u{03B4}", SymbolClass::Alphabetic),   // δ
    ("upepsilon", "\u{03B5}", SymbolClass::Alphabetic), // ε
    ("upzeta", "\u{03B6}", SymbolClass::Alphabetic),    // ζ
    ("upeta", "\u{03B7}", SymbolClass::Alphabetic),     // η
    ("uptheta", "\u{03B8}", SymbolClass::Alphabetic),   // θ
    ("upiota", "\u{03B9}", SymbolClass::Alphabetic),    // ι
    ("upkappa", "\u{03BA}", SymbolClass::Alphabetic),   // κ
    ("uplambda", "\u{03BB}", SymbolClass::Alphabetic),  // λ
    ("upmu", "\u{03BC}", SymbolClass::Alphabetic),      // μ
    ("upnu", "\u{03BD}", SymbolClass::Alphabetic),      // ν
    ("upxi", "\u{03BE}", SymbolClass::Alphabetic),      // ξ
    ("uppi", "\u{03C0}", SymbolClass::Alphabetic),      // π
    ("uprho", "\u{03C1}", SymbolClass::Alphabetic),     // ρ
    ("upsigma", "\u{03C3}", SymbolClass::Alphabetic),   // σ
    ("uptau", "\u{03C4}", SymbolClass::Alphabetic),     // τ
    ("upupsilon", "\u{03C5}", SymbolClass::Alphabetic), // υ
    ("upphi", "\u{03C6}", SymbolClass::Alphabetic),     // φ
    ("upchi", "\u{03C7}", SymbolClass::Alphabetic),     // χ
    ("uppsi", "\u{03C8}", SymbolClass::Alphabetic),     // ψ
    ("upomega", "\u{03C9}", SymbolClass::Alphabetic),   // ω
    // --- uppercase -----------------------------------------------------------
    ("upGamma", "\u{0393}", SymbolClass::Alphabetic),   // Γ
    ("upDelta", "\u{0394}", SymbolClass::Alphabetic),   // Δ
    ("upTheta", "\u{0398}", SymbolClass::Alphabetic),   // Θ
    ("upLambda", "\u{039B}", SymbolClass::Alphabetic),  // Λ
    ("upXi", "\u{039E}", SymbolClass::Alphabetic),      // Ξ
    ("upPi", "\u{03A0}", SymbolClass::Alphabetic),      // Π
    ("upSigma", "\u{03A3}", SymbolClass::Alphabetic),   // Σ
    ("upUpsilon", "\u{03A5}", SymbolClass::Alphabetic), // Υ
    ("upPhi", "\u{03A6}", SymbolClass::Alphabetic),     // Φ
    ("upPsi", "\u{03A8}", SymbolClass::Alphabetic),     // Ψ
    ("upOmega", "\u{03A9}", SymbolClass::Alphabetic),   // Ω
];

#[cfg(test)]
mod tests {
    /// `\upalpha` must convert to the plain Greek letter α (U+03B1).
    #[test]
    fn upalpha_is_alpha_mathml() {
        let out = crate::convert::from_latex(r"\upalpha")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(out.contains('\u{03B1}'), "expected α (U+03B1), got: {out}");
    }

    #[test]
    fn upalpha_is_alpha_unicode() {
        let out = crate::convert::from_latex(r"\upalpha")
            .unwrap()
            .to_unicode_math()
            .unwrap();
        assert!(out.contains('\u{03B1}'), "expected α (U+03B1), got: {out}");
    }

    #[test]
    fn lowercase_sample() {
        for (cmd, ch) in [
            (r"\upbeta", '\u{03B2}'),
            (r"\uppi", '\u{03C0}'),
            (r"\upomega", '\u{03C9}'),
        ] {
            let out = crate::convert::from_latex(cmd).unwrap().to_mathml().unwrap();
            assert!(out.contains(ch), "{cmd}: expected {ch}, got: {out}");
        }
    }

    #[test]
    fn uppercase_sample() {
        for (cmd, ch) in [
            (r"\upGamma", '\u{0393}'),
            (r"\upDelta", '\u{0394}'),
            (r"\upOmega", '\u{03A9}'),
            (r"\upPhi", '\u{03A6}'),
        ] {
            let out = crate::convert::from_latex(cmd).unwrap().to_mathml().unwrap();
            assert!(out.contains(ch), "{cmd}: expected {ch}, got: {out}");
        }
    }

    #[cfg(feature = "render")]
    #[test]
    fn upalpha_renders_svg() {
        let svg = crate::convert::from_latex(r"\upalpha")
            .unwrap()
            .to_svg()
            .unwrap();
        assert!(svg.starts_with("<svg"), "expected SVG output, got: {svg}");
    }
}
