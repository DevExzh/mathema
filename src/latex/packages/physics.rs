//! `physics` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! The `physics` package provides convenient notation for physics: auto-sized
//! delimiters, derivative operators, Dirac bra-ket notation, vector calculus
//! operators, and a few named operators. We implement these as preprocessor
//! **macros** that expand to constructs the parser already supports
//! (`\left`/`\right` auto-sizing delimiters, `\frac`, `\operatorname`,
//! `\mathrm`, `\langle`/`\rangle`, literal Unicode, ...).
//!
//! ## Notes on fidelity / deferred behavior
//!
//! * **Variable arity.** The real `physics` macros are variadic
//!   (`\dv{x}` vs `\dv{f}{x}` vs `\dv[n]{f}{x}`). Package macros in this
//!   registry have a *fixed* parameter count and cannot express an optional
//!   first argument, so the two-argument forms (`\dv{f}{x}`, `\pdv{f}{x}`)
//!   are provided here. The single-argument operator form `\dv{x}` and the
//!   `[n]`-order form are **deferred (needs structural support)**.
//! * **Auto-sizing of `\eval`.** The real `\eval` grows a single right bar to
//!   the height of its (often very tall) argument and supports evaluation
//!   limits. We approximate it with `\left.…\right|`, which auto-sizes the bar
//!   but cannot attach the physics-style evaluation limits — **deferred** for
//!   the full feature.
//! * **Conflicts left to the existing handlers (not redefined here):**
//!   - `\div` — already a core symbol (`÷`, used in tests). The `physics`
//!     meaning `\nabla\cdot` would shadow it, so it is left alone. Use
//!     `\nabla\cdot` directly for divergence.
//!   - `\Re` / `\Im` — already core symbols (`ℜ` / `ℑ`). The `physics`
//!     upright-operator redefinition would change established behavior, so the
//!     core symbols are kept.
//!   - `\braket` — already handled in `parse.rs` (`\braket{a|b}` → `⟨a|b⟩`),
//!     which covers the Dirac inner product, so no macro is added for it.
//!   - `\qty` — siunitx also defines `\qty`. It is handled in the parser's
//!     `dispatch_command` via [`crate::latex::siunitx`] with **arity-based
//!     coexistence**: `\qty{n}{unit}` (two groups) is a siunitx quantity, while
//!     the single-group `\qty{x}` keeps this package's auto-sized parentheses
//!     `\left(x\right)`. No macro is seeded here so the dispatch handler runs.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    // --- Auto-sized delimiters --------------------------------------------
    // \abs{x} → |x|   (auto-sizing absolute value)
    ("abs", 1, r"\left|#1\right|"),
    // \norm{x} → ‖x‖  (auto-sizing norm)
    ("norm", 1, r"\left\|#1\right\|"),
    // \eval{x} → .x|  (evaluation bar; auto-sizes the right bar)
    ("eval", 1, r"\left.#1\right|"),
    // --- Differential / derivative operators ------------------------------
    // \dd → d (upright differential)
    ("dd", 0, r"\mathrm{d}"),
    // \dv{f}{x} → df/dx (total derivative, two-argument form)
    ("dv", 2, r"\frac{\mathrm{d}#1}{\mathrm{d}#2}"),
    // \pdv{f}{x} → ∂f/∂x (partial derivative, two-argument form)
    ("pdv", 2, r"\frac{\partial #1}{\partial #2}"),
    // --- Dirac bra-ket notation -------------------------------------------
    // \bra{x} → ⟨x|
    ("bra", 1, r"\left\langle #1\right|"),
    // \ket{x} → |x⟩
    ("ket", 1, r"\left|#1\right\rangle"),
    // \dyad{a}{b} → |a⟩⟨b|
    ("dyad", 2, r"\left|#1\right\rangle\!\left\langle #2\right|"),
    // --- Vector calculus operators ----------------------------------------
    // \grad → ∇
    ("grad", 0, r"\nabla"),
    // \curl → ∇×
    ("curl", 0, r"\nabla\times"),
    // --- Named operators --------------------------------------------------
    // \Tr → Tr (trace)
    ("Tr", 0, r"\operatorname{Tr}"),
    // \tr → tr (lowercase trace, also provided by physics)
    ("tr", 0, r"\operatorname{tr}"),
    // \rank → rank
    ("rank", 0, r"\operatorname{rank}"),
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

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
    fn abs_uses_vertical_bars() {
        let m = to_mathml(r"\abs{x}");
        assert!(m.contains('|'), "expected | in {m}");
        assert!(m.contains('x'));
    }

    #[test]
    fn norm_uses_double_bar() {
        // ‖ (U+2016) is the norm delimiter; \| should map to it.
        let m = to_mathml(r"\norm{v}");
        assert!(
            m.contains('\u{2016}') || m.matches('|').count() >= 2,
            "expected double bar in {m}"
        );
    }

    #[test]
    fn qty_wraps_in_parens() {
        let m = to_mathml(r"\qty{a+b}");
        assert!(m.contains('('), "expected ( in {m}");
        assert!(m.contains(')'), "expected ) in {m}");
    }

    #[test]
    fn eval_has_right_bar() {
        let m = to_mathml(r"\eval{x}");
        assert!(m.contains('|'), "expected evaluation bar in {m}");
        assert!(m.contains('x'));
    }

    #[test]
    fn dd_is_upright_d() {
        let m = to_mathml(r"\dd");
        assert!(m.contains('d'), "expected d in {m}");
    }

    #[test]
    fn dv_two_arg_is_fraction() {
        let m = to_mathml(r"\dv{f}{x}");
        assert!(m.contains("<mfrac"), "expected fraction in {m}");
        assert!(m.contains('f') && m.contains('x'));
    }

    #[test]
    fn pdv_two_arg_uses_partial() {
        let m = to_mathml(r"\pdv{f}{x}");
        assert!(m.contains("<mfrac"), "expected fraction in {m}");
        assert!(m.contains('\u{2202}'), "expected ∂ in {m}");
    }

    #[test]
    fn bra_renders_langle() {
        let m = to_mathml(r"\bra{\psi}");
        assert!(m.contains('\u{27E8}'), "expected ⟨ in {m}");
    }

    #[test]
    fn ket_renders_rangle() {
        let m = to_mathml(r"\ket{\psi}");
        assert!(m.contains('\u{27E9}'), "expected ⟩ in {m}");
    }

    #[test]
    fn dyad_has_bracket_and_angles() {
        let m = to_mathml(r"\dyad{a}{b}");
        assert!(m.contains('\u{27E9}'), "expected ⟩ in {m}");
        assert!(m.contains('\u{27E8}'), "expected ⟨ in {m}");
        assert!(m.contains('|'), "expected | in {m}");
    }

    #[test]
    fn grad_is_nabla() {
        let m = to_mathml(r"\grad");
        assert!(m.contains('\u{2207}'), "expected ∇ in {m}");
    }

    #[test]
    fn curl_is_nabla_times() {
        let m = to_mathml(r"\curl");
        assert!(m.contains('\u{2207}'), "expected ∇ in {m}");
        assert!(m.contains('\u{00D7}'), "expected × in {m}");
    }

    #[test]
    fn tr_and_rank_are_operators() {
        let tr = to_mathml(r"\Tr");
        assert!(tr.contains("Tr"), "expected Tr in {tr}");
        let rank = to_mathml(r"\rank");
        assert!(rank.contains("rank"), "expected rank in {rank}");
    }

    #[test]
    fn unicode_math_path_works() {
        // Exercise a second output path (LaTeX -> UnicodeMath).
        let u = to_unicode(r"\ket{\phi}");
        assert!(u.contains('\u{27E9}'), "expected ⟩ in unicode output {u}");
    }

    #[test]
    fn user_macro_overrides_builtin() {
        // A user \newcommand must win over the physics builtin.
        let m = to_mathml(r"\newcommand{\grad}{G}\grad");
        assert!(m.contains('G'));
        assert!(!m.contains('\u{2207}'));
    }
}
