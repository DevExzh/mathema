//! `upgreek` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `upgreek` provides **upright** Greek letters: `\upalpha`..`\upomega`
//! (lowercase, plus the variant forms `\upvarepsilon`, `\upvartheta`, …) and
//! `\upGamma`, `\upDelta`, … (uppercase).
//!
//! ## Upright vs. italic distinction
//!
//! In Unicode there is no separate "upright Greek" codepoint: the same Greek
//! codepoints (U+03xx) render *italic* in the math-italic font (how `\alpha` is
//! drawn) and *upright* in a roman font. The distinction is therefore a
//! `mathvariant` matter, not a codepoint matter.
//!
//! To actually preserve the package's defining feature (upright shape) — rather
//! than emitting a byte-identical glyph to the italic `\alpha` — each command is
//! a macro that expands to `\mathrm{<greek>}`. This makes `\upalpha`
//! (`<mi mathvariant="normal">α</mi>`, upright) genuinely distinct from `\alpha`
//! (`<mi>α</mi>`, italic) in MathML, and renders upright through the shared
//! `mathvariant="normal"` style path, while UnicodeMath/SVG still carry the
//! Greek glyph.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Each `\up<letter>` expands to `\mathrm{<greek glyph>}` so the upright
/// (roman / `mathvariant="normal"`) shape is preserved, distinct from the math
/// italic shape of the corresponding core command (`\alpha`, `\Gamma`, …).
pub const MACROS: &[(&str, u8, &str)] = &[
    // --- lowercase: \upalpha .. \upomega -------------------------------------
    ("upalpha", 0, r"\mathrm{α}"),
    ("upbeta", 0, r"\mathrm{β}"),
    ("upgamma", 0, r"\mathrm{γ}"),
    ("updelta", 0, r"\mathrm{δ}"),
    ("upepsilon", 0, r"\mathrm{ε}"),
    ("upzeta", 0, r"\mathrm{ζ}"),
    ("upeta", 0, r"\mathrm{η}"),
    ("uptheta", 0, r"\mathrm{θ}"),
    ("upiota", 0, r"\mathrm{ι}"),
    ("upkappa", 0, r"\mathrm{κ}"),
    ("uplambda", 0, r"\mathrm{λ}"),
    ("upmu", 0, r"\mathrm{μ}"),
    ("upnu", 0, r"\mathrm{ν}"),
    ("upxi", 0, r"\mathrm{ξ}"),
    ("upomicron", 0, r"\mathrm{ο}"), // U+03BF — completes the 24-letter lowercase set
    ("uppi", 0, r"\mathrm{π}"),
    ("uprho", 0, r"\mathrm{ρ}"),
    ("upsigma", 0, r"\mathrm{σ}"),
    ("uptau", 0, r"\mathrm{τ}"),
    ("upupsilon", 0, r"\mathrm{υ}"),
    ("upphi", 0, r"\mathrm{φ}"),
    ("upchi", 0, r"\mathrm{χ}"),
    ("uppsi", 0, r"\mathrm{ψ}"),
    ("upomega", 0, r"\mathrm{ω}"),
    // --- lowercase variant forms ---------------------------------------------
    ("upvarepsilon", 0, r"\mathrm{ϵ}"), // U+03F5
    ("upvartheta", 0, r"\mathrm{ϑ}"),   // U+03D1
    ("upvarkappa", 0, r"\mathrm{ϰ}"),   // U+03F0
    ("upvarpi", 0, r"\mathrm{ϖ}"),      // U+03D6
    ("upvarrho", 0, r"\mathrm{ϱ}"),     // U+03F1
    ("upvarsigma", 0, r"\mathrm{ς}"),   // U+03C2
    ("upvarphi", 0, r"\mathrm{ϕ}"),     // U+03D5
    // --- uppercase -----------------------------------------------------------
    ("upGamma", 0, r"\mathrm{Γ}"),
    ("upDelta", 0, r"\mathrm{Δ}"),
    ("upTheta", 0, r"\mathrm{Θ}"),
    ("upLambda", 0, r"\mathrm{Λ}"),
    ("upXi", 0, r"\mathrm{Ξ}"),
    ("upPi", 0, r"\mathrm{Π}"),
    ("upSigma", 0, r"\mathrm{Σ}"),
    ("upUpsilon", 0, r"\mathrm{Υ}"),
    ("upPhi", 0, r"\mathrm{Φ}"),
    ("upPsi", 0, r"\mathrm{Ψ}"),
    ("upOmega", 0, r"\mathrm{Ω}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
///
/// All `upgreek` commands are provided as upright macros above, so this table
/// is empty.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

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

    /// The defining feature: `\upalpha` is upright (mathvariant="normal") and
    /// therefore genuinely distinct from the italic `\alpha`.
    #[test]
    fn upalpha_is_upright_and_distinct_from_alpha() {
        let up = crate::convert::from_latex(r"\upalpha")
            .unwrap()
            .to_mathml()
            .unwrap();
        let it = crate::convert::from_latex(r"\alpha")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(
            up.contains("mathvariant=\"normal\""),
            "\\upalpha should be upright, got: {up}"
        );
        assert!(
            !it.contains("mathvariant=\"normal\""),
            "\\alpha should be italic, got: {it}"
        );
        assert_ne!(up, it, "upright and italic Greek must differ");
    }

    #[test]
    fn upomicron_present() {
        // Previously missing — completes the 24-letter lowercase set.
        let out = crate::convert::from_latex(r"\upomicron")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(out.contains('\u{03BF}'), "expected ο (U+03BF), got: {out}");
    }

    #[test]
    fn lowercase_sample() {
        for (cmd, ch) in [
            (r"\upbeta", '\u{03B2}'),
            (r"\uppi", '\u{03C0}'),
            (r"\upomega", '\u{03C9}'),
        ] {
            let out = crate::convert::from_latex(cmd)
                .unwrap()
                .to_mathml()
                .unwrap();
            assert!(out.contains(ch), "{cmd}: expected {ch}, got: {out}");
        }
    }

    #[test]
    fn variant_forms() {
        for (cmd, ch) in [(r"\upvarepsilon", '\u{03F5}'), (r"\upvarphi", '\u{03D5}')] {
            let out = crate::convert::from_latex(cmd)
                .unwrap()
                .to_mathml()
                .unwrap();
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
            let out = crate::convert::from_latex(cmd)
                .unwrap()
                .to_mathml()
                .unwrap();
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
