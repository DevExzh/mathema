use mathema::RasterOptions;

fn main() {
    let formulas: Vec<(&str, &str)> = vec![
        // === Basic expressions ===
        (r"x^2 + y^2 = z^2", "basic_pythagorean"),
        (r"e^{i\pi} + 1 = 0", "euler_identity"),
        (r"a_1, a_2, \ldots, a_n", "subscript_sequence"),
        // === Fractions ===
        (r"\frac{a}{b}", "frac_simple"),
        (r"\frac{1}{\frac{1}{x} + \frac{1}{y}}", "frac_nested"),
        (r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}", "cfrac_continued"),
        (r"\dfrac{a}{b} + \tfrac{c}{d}", "dfrac_tfrac"),
        (r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", "quadratic"),
        // === Radicals ===
        (r"\sqrt{x}", "sqrt_simple"),
        (r"\sqrt[3]{x}", "cbrt"),
        (r"\sqrt{\sqrt{\sqrt{x}}}", "sqrt_nested"),
        (r"\sqrt{x^2 + y^2}", "sqrt_expr"),
        // === Large operators ===
        (r"\sum_{i=0}^{n} i^2", "sum_limits"),
        (r"\prod_{k=1}^{n} k", "product_limits"),
        (r"\int_0^\infty e^{-x}\,dx", "integral"),
        (r"\iint_D f(x,y)\,dA", "double_integral"),
        (r"\iiint_V f\,dV", "triple_integral"),
        (r"\oint_C \vec{F} \cdot d\vec{r}", "contour_integral"),
        (r"\bigcup_{i=1}^{n} A_i", "big_union"),
        (r"\bigcap_{i=1}^{n} A_i", "big_intersection"),
        (r"\coprod_{\alpha} G_\alpha", "coproduct"),
        // === Delimiters ===
        (r"\left( \frac{a}{b} \right)", "paren_frac"),
        (r"\left[ \frac{a}{b} \right]", "bracket_frac"),
        (r"\left\{ \frac{a}{b} \right\}", "brace_frac"),
        (r"\left\lfloor \frac{n}{2} \right\rfloor", "floor"),
        (r"\left\lceil \frac{n}{2} \right\rceil", "ceil"),
        (r"\left| \frac{a}{b} \right|", "abs"),
        (r"\left\| \vec{v} \right\|", "norm"),
        (r"\left\langle \psi | \phi \right\rangle", "braket"),
        (
            r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)",
            "tall_delims",
        ),
        // === Matrices ===
        (r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", "pmatrix"),
        (r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}", "bmatrix"),
        (r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}", "vmatrix"),
        (
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
            "matrix3x3",
        ),
        // === Environments ===
        (
            r"\begin{cases} x & \text{if } x \geq 0 \\ -x & \text{if } x < 0 \end{cases}",
            "cases",
        ),
        (
            r"\begin{aligned} a &= b + c \\ d &= e + f \end{aligned}",
            "aligned",
        ),
        // === Accents ===
        (r"\hat{x} + \bar{y} + \dot{z} + \tilde{w}", "accents_basic"),
        (r"\vec{F} = m\vec{a}", "vec_accent"),
        (r"\widehat{ABC} + \widetilde{XYZ}", "wide_accents"),
        (r"\ddot{x} + \breve{a} + \check{v}", "accents_more"),
        (r"\overline{AB} + \underline{CD}", "over_underline"),
        // === Over/under ===
        (r"\underbrace{a+b+c}_{n\text{ terms}}", "underbrace"),
        (r"\overbrace{1+2+\cdots+n}^{S_n}", "overbrace"),
        (r"\overset{!}{=}", "overset"),
        (r"\underset{\sim}{A}", "underset"),
        (r"\xrightarrow{f}", "xrightarrow"),
        (
            r"A \xrightarrow{\alpha} B \xrightarrow{\beta} C",
            "xarrow_chain",
        ),
        // === Scripts ===
        (r"a_{i_j}^{k^l}", "nested_scripts"),
        (r"{}^{14}_6\text{C}", "isotope"),
        (r"f' + f'' + f'''", "primes"),
        (r"x^{x^{x^x}}", "tower"),
        // === Greek ===
        (
            r"\alpha\beta\gamma\delta\epsilon\zeta\eta\theta",
            "greek_lower",
        ),
        (
            r"\Gamma\Delta\Theta\Lambda\Xi\Pi\Sigma\Phi\Psi\Omega",
            "greek_upper",
        ),
        // === Fonts ===
        (
            r"\mathbb{R} \times \mathcal{H} \times \mathfrak{g}",
            "font_styles",
        ),
        (r"\mathrm{d}x + \mathbf{v} + \mathit{f}", "font_variants"),
        (r"\text{Hello World}", "text_mode"),
        // === Relations & operators ===
        (
            r"A \subseteq B \implies A \cap C \subseteq B \cap C",
            "set_theory",
        ),
        (r"\forall \epsilon > 0\, \exists \delta > 0", "logic"),
        (r"a \equiv b \pmod{n}", "modular"),
        (r"f: X \to Y", "function_map"),
        (r"a \leq b \leq c", "inequalities"),
        (r"\sim \approx \cong \simeq \propto", "relations"),
        // === Arrows ===
        (
            r"\rightarrow \Rightarrow \leftrightarrow \Leftrightarrow",
            "arrows",
        ),
        (
            r"\hookrightarrow \twoheadrightarrow \mapsto",
            "arrows_special",
        ),
        // === Spacing & phantom ===
        (r"\frac{1}{\smash[b]{q}}", "smash_bottom"),
        (r"a\,b\;c\quad d\qquad e", "spacing"),
        (r"\phantom{x}y", "phantom"),
        // === Physics/advanced ===
        (
            r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
            "maxwell",
        ),
        (
            r"f'(x) = \lim_{h \to 0} \frac{f(x+h) - f(x)}{h}",
            "derivative",
        ),
        (
            r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e",
            "euler_limit",
        ),
        (r"\binom{n}{k} = \frac{n!}{k!(n-k)!}", "binomial"),
        (
            r"\left(\sum_{k=1}^n a_k b_k\right)^2 \leq \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
            "cauchy_schwarz",
        ),
        // === Colors ===
        (r"{\color{red}x} + {\color{blue}y}", "colors"),
        // === Boxed ===
        (r"\boxed{E = mc^2}", "boxed"),
        // === Cancellation ===
        (r"\cancel{x} + \bcancel{y} + \xcancel{z}", "cancel"),
        // === Dots ===
        (r"a_1 + a_2 + \cdots + a_n", "cdots"),
        (r"a_1, a_2, \ldots, a_n", "ldots"),
        (r"\vdots \quad \ddots", "vdots_ddots"),
    ];

    std::fs::create_dir_all("test_matrix/audit").ok();

    let mut failures = Vec::new();
    let mut warnings = Vec::new();

    for (latex, name) in &formulas {
        let result = match mathema::from_latex(latex) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{name}: parse error: {e}"));
                println!("FAIL: {name}: {e}");
                continue;
            },
        };

        // Generate SVG
        match result.to_svg() {
            Ok(svg) => {
                std::fs::write(format!("test_matrix/audit/{name}.svg"), svg.as_bytes()).ok();
                if svg.contains("NaN") || svg.contains("Infinity") {
                    warnings.push(format!("{name}: SVG contains NaN/Infinity"));
                }
            },
            Err(e) => {
                failures.push(format!("{name}: SVG error: {e}"));
                println!("FAIL: {name}: {e}");
                continue;
            },
        }

        // Generate PNG at 4x scale
        let opts = RasterOptions {
            scale: 4.0,
            ..Default::default()
        };
        match result.to_png_with(&opts) {
            Ok(png) => {
                std::fs::write(format!("test_matrix/audit/{name}.png"), &png).ok();
            },
            Err(e) => {
                warnings.push(format!("{name}: PNG error: {e}"));
            },
        }

        println!("OK: {name}");
    }

    println!("\n=== Quality Audit: {} formulas ===", formulas.len());
    if !failures.is_empty() {
        println!("\nFAILURES ({}):", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
    }
    if !warnings.is_empty() {
        println!("\nWARNINGS ({}):", warnings.len());
        for w in &warnings {
            println!("  - {w}");
        }
    }
    if failures.is_empty() && warnings.is_empty() {
        println!("All formulas rendered successfully!");
    }
}
