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
//! * **Variadic derivatives.** `\dv`, `\pdv`, and `\fdv` are *not* macros here
//!   (a fixed-arity macro body cannot express the optional `[order]`, the `*`
//!   slashed form, or the operator/derivative/mixed-partial arities). They are
//!   translated structurally by [`crate::latex::physics`] and dispatched in the
//!   parser, exactly like `siunitx`/`mhchem`.
//! * **Auto-sizing of `\eval`.** The real `\eval` grows a single right bar to
//!   the height of its (often very tall) argument and supports evaluation
//!   limits. We approximate it with `\left.…\right|`, which auto-sizes the bar
//!   but cannot attach the physics-style evaluation limits — **deferred** for
//!   the full feature.
//! * **Conflicts left to the existing handlers (not redefined here):**
//!   - `\div` — already a core symbol (`÷`, used in tests). The `physics`
//!     meaning `\nabla\cdot` would shadow it, so it is left alone; the explicit
//!     name `\divergence` is provided instead.
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
    // \abs / \norm are handled directly by the parser (`parse_paired_delim`),
    // which also supports their starred auto-size forms.
    // \eval{x} → .x|  (evaluation bar; auto-sizes the right bar)
    ("eval", 1, r"\left.#1\right|"),
    // --- Differential -----------------------------------------------------
    // \dd → d (upright differential). `\dd x` → upright d then x.
    ("dd", 0, r"\mathrm{d}"),
    // --- Dirac bra-ket notation -------------------------------------------
    // \bra{x} → ⟨x|   (auto-scaling; the effective \bra)
    ("bra", 1, r"\left\langle #1\right|"),
    // \ket{x} → |x⟩
    ("ket", 1, r"\left|#1\right\rangle"),
    // \dyad{a}{b} → |a⟩⟨b|  (outer product)
    ("dyad", 2, r"\left|#1\right\rangle\!\left\langle #2\right|"),
    // \op{a}{b} → |a⟩⟨b|  (outer product; alias of \dyad)
    ("op", 2, r"\left|#1\right\rangle\!\left\langle #2\right|"),
    // \ip{a}{b} → ⟨a|b⟩  (inner product)
    ("ip", 2, r"\left\langle #1\middle|#2\right\rangle"),
    // \mel{a}{B}{c} → ⟨a|B|c⟩  (matrix element)
    ("mel", 3, r"\left\langle #1\right|#2\left|#3\right\rangle"),
    // \expval{A} → ⟨A⟩  (expectation value)
    ("expval", 1, r"\left\langle #1\right\rangle"),
    // \ev{A} → ⟨A⟩  (alias of \expval)
    ("ev", 1, r"\left\langle #1\right\rangle"),
    // --- Commutators ------------------------------------------------------
    // \comm{A}{B} → [A,B]
    ("comm", 2, r"\left[#1,#2\right]"),
    // \acomm{A}{B} → {A,B}  (anticommutator)
    ("acomm", 2, r"\left\{#1,#2\right\}"),
    ("anticommutator", 2, r"\left\{#1,#2\right\}"),
    // \pb{A}{B} → {A,B}  (Poisson bracket)
    ("pb", 2, r"\left\{#1,#2\right\}"),
    ("poissonbracket", 2, r"\left\{#1,#2\right\}"),
    // --- Vector calculus operators ----------------------------------------
    // \grad → ∇
    ("grad", 0, r"\nabla"),
    // \curl → ∇×
    ("curl", 0, r"\nabla\times"),
    // \divergence → ∇·  (\div stays the core ÷)
    ("divergence", 0, r"\nabla\cdot"),
    // \laplacian → ∇²
    ("laplacian", 0, r"\nabla^{2}"),
    // \vb{a} → bold vector
    ("vb", 1, r"\mathbf{#1}"),
    // \va{a} → vector with arrow
    ("va", 1, r"\vec{#1}"),
    // \vu{a} → unit vector (hat)
    ("vu", 1, r"\hat{#1}"),
    // \vdot / \dotproduct → bold dot product
    ("vdot", 0, r"\boldsymbol{\cdot}"),
    ("dotproduct", 0, r"\boldsymbol{\cdot}"),
    // \cross / \crossproduct → bold cross product
    ("cross", 0, r"\boldsymbol{\times}"),
    ("crossproduct", 0, r"\boldsymbol{\times}"),
    // --- Matrices ---------------------------------------------------------
    // \mqty{..} → matrix (no delimiters); delimited variants below.
    ("mqty", 1, r"\begin{matrix}#1\end{matrix}"),
    ("smqty", 1, r"\begin{smallmatrix}#1\end{smallmatrix}"),
    ("pmqty", 1, r"\begin{pmatrix}#1\end{pmatrix}"),
    ("bmqty", 1, r"\begin{bmatrix}#1\end{bmatrix}"),
    ("vmqty", 1, r"\begin{vmatrix}#1\end{vmatrix}"),
    // --- Named operators / functions --------------------------------------
    // \order{x} → O(x)
    ("order", 1, r"O\!\left(#1\right)"),
    // \Tr → Tr (trace)
    ("Tr", 0, r"\operatorname{Tr}"),
    // \tr → tr (lowercase trace, also provided by physics)
    ("tr", 0, r"\operatorname{tr}"),
    // \rank → rank
    ("rank", 0, r"\operatorname{rank}"),
    // \erf → erf
    ("erf", 0, r"\operatorname{erf}"),
    // \Res → Res (residue)
    ("Res", 0, r"\operatorname{Res}"),
    // \pv / \PV → principal value
    ("pv", 0, r"\operatorname{PV}"),
    ("PV", 0, r"\operatorname{PV}"),
    // Inverse trigonometric operators (rendered as upright operators, not
    // italic identifiers).
    ("acos", 0, r"\operatorname{acos}"),
    ("asin", 0, r"\operatorname{asin}"),
    ("atan", 0, r"\operatorname{atan}"),
    ("acot", 0, r"\operatorname{acot}"),
    ("asec", 0, r"\operatorname{asec}"),
    ("acsc", 0, r"\operatorname{acsc}"),
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

    #[test]
    fn dv_single_arg_operator_form() {
        // \dv{x} → d/dx (operator form), now structurally dispatched.
        let m = to_mathml(r"\dv{x}");
        assert!(m.contains("<mfrac"), "expected fraction in {m}");
    }

    #[test]
    fn dv_order_form() {
        // \dv[2]{f}{x} → second derivative with ^2 in numerator.
        let m = to_mathml(r"\dv[2]{f}{x}");
        assert!(m.contains("<mfrac"), "expected fraction in {m}");
        assert!(m.contains("<msup"), "expected order superscript in {m}");
    }

    #[test]
    fn pdv_mixed_partial() {
        // \pdv{f}{x}{y} → ∂²f/∂x∂y.
        let m = to_mathml(r"\pdv{f}{x}{y}");
        assert!(m.contains("<mfrac"), "expected fraction in {m}");
        assert!(m.contains('\u{2202}'), "expected ∂ in {m}");
        assert!(m.contains("<msup"), "expected ∂² order in {m}");
    }

    #[test]
    fn inner_product_renders_angles() {
        let m = to_mathml(r"\ip{a}{b}");
        assert!(m.contains('\u{27E8}') && m.contains('\u{27E9}'), "got {m}");
    }

    #[test]
    fn expval_renders_angles() {
        let m = to_mathml(r"\expval{A}");
        assert!(m.contains('\u{27E8}') && m.contains('\u{27E9}'), "got {m}");
    }

    #[test]
    fn commutator_renders_brackets() {
        let m = to_mathml(r"\comm{A}{B}");
        assert!(
            m.contains('[') && m.contains(']') && m.contains(','),
            "got {m}"
        );
    }

    #[test]
    fn inverse_trig_is_operator_not_identifier() {
        // \acos must be an operator <mo>, not an italic identifier <mi>.
        let m = to_mathml(r"\acos");
        assert!(m.contains("<mo>acos</mo>"), "expected operator acos in {m}");
    }

    #[test]
    fn mqty_builds_matrix() {
        let m = to_mathml(r"\pmqty{a & b \\ c & d}");
        assert!(m.contains("<mtable"), "expected matrix table in {m}");
        assert!(m.contains('('), "expected parens in {m}");
    }
}
