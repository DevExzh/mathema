//! The examples of Unicode Technical Note 28, as written there: what each
//! reads as, and (in `GAPS`) what those not yet supported should read as.

use mathema::{Format, convert};

/// UnicodeMath and its LaTeX.
#[rustfmt::skip]
const EXAMPLES: &[(&str, &str)] = &[
    ("1/2", r"\frac{1}{2}"),
    ("abc/d", r"\frac{abc}{d}"),
    ("(a + c)/d", r"\frac{a+ c}{d}"),
    ("((a + c))/d", r"\frac{\left( a+c \right)}{d}"),
    ("a/(b+c/(d+f))", r"\frac{a}{b+ \frac{c}{d+ f}}"),
    ("(a + b)^n = ∑_(k=0)^n (n¦k) a^k b^(n-k)", r"{\left( a+b \right)}^n= \sum\limits_{k= 0}^n {\binom{n}{k}}a^k{b}^{n- k}"),
    ("a/=b", r"a\neq b"),
    ("1/3.1416", r"\frac{1}{3.1416}"),
    ("1+a/b/c/d", r"1+ \frac{\frac{\frac{a}{b}}{c}}{d}"),
    ("δ_μν", r"\delta_{\mu\nu}"),
    ("a^b", "a^b"),
    ("δ_(μ + ν)", r"\delta_{\mu+ \nu}"),
    ("a_b_c", "a_{b_c}"),
    ("a^b^c", "a^{b^c}"),
    ("a^(b_c)", "a^{b_c}"),
    ("a^b_c", "a_c^b"),
    ("a_c^b", "a_c^b"),
    ("sin^2 x", r"\sin^{2}x"),
    ("W^3β_δ1 ρ1 σ2", r"W_{\delta1}^{3\beta}\rho1\sigma2"),
    ("W^3β_δ_1 ρ_1 σ_2", r"W_{\delta_{1}}^{3\beta}\rho_{1}\sigma_{2}"),
    ("W_δ1ρ1σ2^3β=U_δ1ρ1^3β+1/8π^2 ∫_α1^α2 dα’2 [(U_δ1ρ1^2β-α’2 U_ρ1σ2^1β)/U_ρ1σ2^0β]", r"W_{\delta1\rho1\sigma2}^{3\beta}= U_{\delta1\rho1}^{3\beta}+ \frac{1}{8\pi^{2}}\int\nolimits_{\alpha1}^{\alpha2} {d\alpha’2}\left[ \frac{U_{\delta1\rho1}^{2\beta}- \alpha’2{U}_{\rho1\sigma2}^{1\beta}}{U_{\rho1\sigma2}^{0\beta}} \right]"),
    ("a_1 b_2", "a_{1}b_{2}"),
    ("|x|", "|x|"),
    ("|(|x|−|y|)|", r"|\left( |x|-|y| \right)|"),
    ("||x|−|y||", "||x|- |y||"),
    ("|a|b−c|d|", "|a|b- c|d|"),
    ("|(a|b−c|d)|", r"|\left( a|b-c|d \right)|"),
    ("ρ = ∑_ψ P_ψ |ψ⟩⟨ψ|", r"\rho= \sum\limits_\psi {P_\psi}|\psi\rangle\langle\psi|"),
    ("ρ=∑_ψ▒P_ψ  |ψ⟩⟨ψ |", r"\rho= \sum\limits_\psi {P_\psi}|\psi\rangle\langle\psi|"),
    ("f|_0^1", "f|_{0}^{1}"),
    ("‖x‖", r"\|x\|"),
    ("(a|b)", r"\left( a|b \right)"),
    ("(a|b|c)", r"\left( a|b|c \right)"),
    ("{x∣f(x)=0}", r"\left\{ x\mid f\left( x \right)=0 \right\}"),
    ("det A=|■(a&b@c&d)|", r"\det A= |\begin{matrix} a & b \\ c & d \end{matrix}|"),
    ("(_c^b)a", "{{}_{c}^{b}a}"),
    ("_c^b a", "{{}_{c}^{b}a}"),
    ("_1 F_1", "{{}_{1}F_{1}}"),
    ("(_1^)F_1", "{{}_{1}F_{1}}"),
    ("lim┬(n→∞) a_n", r"\underset{n\rightarrow \infty}{\lim}a_n"),
    ("lim_(n→∞) a_n", r"\lim_{n\rightarrow \infty}a_n"),
    ("∫_0^a xⅆx/(x^2+a^2)", r"\int\nolimits_{0}^a {\frac{xⅆx}{x^{2}+ a^{2}}}"),
    ("∫_0^a▒xⅆx/(x^2+a^2)", r"\int\nolimits_{0}^a {\frac{xⅆx}{x^{2}+ a^{2}}}"),
    ("∑_(k=0)^n▒a_k", r"\sum\limits_{k= 0}^n {a_k}"),
    ("sin x", r"\sin x"),
    ("√(a+b)", r"\sqrt{a+ b}"),
    ("∛(c+d)", r"\sqrt[3]{c+ d}"),
    ("∜(a+b)", r"\sqrt[4]{a+ b}"),
    ("√(n&a+b)", r"\sqrt[n]{a+ b}"),
    ("▭(E = mc^2)", r"\boxed{E= m{c}^{2}}"),
    ("(a+b)┴→", r"\overset{\rightarrow}{\left( a+b \right)}"),
    ("→┴a + b", r"\overset{a}{\rightarrow}+ b"),
    ("→┴(a+b)", r"\overset{a+ b}{\rightarrow}"),
    ("■(a&b@c&d)", r"\begin{matrix} a & b \\ c & d \end{matrix}"),
    ("(■(a&b@c&d))", r"\left( \begin{matrix} a & b \\ c & d \end{matrix} \right)"),
    ("a′", r"a^{\prime}"),
    ("a′^c", r"a^{\prime c}"),
    ("a^*2", "a^*2"),
    ("a^(*2)", "a^{* 2}"),
    ("a⃗", r"\vec{a}"),
    ("a/.3", r"\frac{a}{.3}"),
    ("f:x", "f: x"),
    ("x,y", "x,y"),
    ("1,234", "1,234"),
    ("⟡(x)", r"\phantom{x}"),
    ("α+β/γ", r"\alpha+ \frac{\beta}{\gamma}"),
    ("(α+β)/γ", r"\frac{\alpha+ \beta}{\gamma}"),
    ("x=(-b±√(b^2-4ac))/2a", r"x= \frac{- b\pm \sqrt{b^{2}- 4ac}}{2a}"),
    ("n!/(k!(n-k)!)", r"\frac{n!}{k!\left( n-k \right)!}"),
    ("a∘b/c", r"a\circ \frac{b}{c}"),
    ("ⅆx", "ⅆx"),
    ("a +₁ b =₂ c", "a+_{1}b=_{2}c"),
    ("W^3β_δ₁ρ₁σ₂", r"W_{\delta_{1}\rho_{1}\sigma_{2}}^{3\beta}"),
    ("α₂³/( β₂³ + γ₂³)", r"\frac{\alpha_{2}^{3}}{\beta_{2}^{3}+ \gamma_{2}^{3}}"),
    ("sin⁡(x) b", r"\sin \left( x \right)b"),
    ("√abc", r"\sqrt{abc}"),
    ("a'^c", r"a^{\prime c}"),
    ("sin θ = ½ e^iθ + \"c.c.\"", r"\sin \theta= ½e^{i\theta}+ \text{c.c.}"),
];

/// UnicodeMath that is not yet read as the note says, and how it should be.
#[rustfmt::skip]
const GAPS: &[(&str, &str)] = &[
    ("lim┬ (n→∞) a_n", r"\underset{n\rightarrow \infty}{\lim}a_n"),
    ("(a/(b+c))∕(d/e + f)", r"\frac{\frac{a}{b+ c}}{\frac{d}{e}+ f}"),
    ("n⒞k", r"\binom{n}{k}"),
    ("|x| = Ⓒ(&x\" if \"x ≥ 0@−&x\" if \"x < 0)", r"|x|= \begin{cases} x\text{ if }x\geq 0 \\ -x\text{ if }x< 0 \end{cases}"),
    ("├]a+b┤[", r"\left] a+b \right["),
    ("⒜(x+y)", "|x+ y|"),
    ("(a│b)", r"\left( a\middle|b \right)"),
    ("▁x", r"\underline{x}"),
    ("⏟(x+y+z)_(>0)", r"\underbrace{x+ y+ z}_{> 0}"),
    ("⒨(a&b@c&d)", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
    ("â", r"\hat{a}"),
    ("(a+b) ̂", r"\hat{a+b}"),
    ("\"rate\"=\"distance\"/\"time\"", r"\text{rate}= \frac{\text{distance}}{\text{time}}"),
    ("⬆x", r"\smash{x}"),
    ("E=mc^2#(30)", r"E= m{c}^{2}\tag{30}"),
];

fn check(rows: &[(&str, &str)]) {
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|&(input, wanted)| {
            let found = convert(input, Format::UnicodeMath, Format::LaTeX).unwrap();
            (found != wanted).then(|| format!("{input}: found {found}, wanted {wanted}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} rows failed:\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
}

#[test]
fn examples_read_as_the_note_says() {
    check(EXAMPLES);
}

#[test]
fn gaps_are_still_gaps() {
    for &(input, wanted) in GAPS {
        let found = convert(input, Format::UnicodeMath, Format::LaTeX).unwrap();
        assert_ne!(found, wanted, "{input}: move it to EXAMPLES");
    }
}
