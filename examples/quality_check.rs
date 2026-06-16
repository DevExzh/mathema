use mathema::Format;
use mathema::convert;

fn main() {
    let formulas = vec![
        // Basic
        (r"x^2 + y^2 = z^2", "pythagorean"),
        (r"\frac{a}{b}", "simple_frac"),
        (r"\sqrt{x^2 + 1}", "sqrt"),
        (r"\sum_{i=0}^{n} i^2", "sum"),
        (r"\int_0^\infty e^{-x} dx", "integral"),
        // Complex
        (r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", "quadratic"),
        (
            r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e",
            "euler",
        ),
        (r"\prod_{k=1}^{n} k = n!", "product"),
        (
            r"f'(x) = \lim_{h \to 0} \frac{f(x+h) - f(x)}{h}",
            "derivative",
        ),
        (
            r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
            "maxwell",
        ),
        // Matrices & environments
        (r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", "matrix"),
        (
            r"\begin{cases} x & \text{if } x \geq 0 \\ -x & \text{if } x < 0 \end{cases}",
            "cases",
        ),
        // Accents
        (r"\hat{x} + \bar{y} + \dot{z} + \tilde{w}", "accents"),
        (r"\vec{F} = m\vec{a}", "vectors"),
        (r"\widehat{ABC} + \widetilde{XYZ}", "wide_accents"),
        // Decorations
        (r"\underbrace{a+b+c}_{n \text{ terms}}", "underbrace"),
        (r"\overbrace{1+2+\cdots+n}^{S_n}", "overbrace"),
        (r"\overset{!}{=}", "overset"),
        (r"\xrightarrow{f}", "xrightarrow"),
        // Script combinations
        (r"a_{i_j}^{k^l}", "nested_scripts"),
        (r"\iiint_V f\,dV", "triple_int"),
        // Fonts/styles
        (r"\mathbb{R} \times \mathcal{H}", "font_styles"),
        (r"\alpha + \beta + \gamma = \pi", "greek"),
        // Delimiters
        (r"\left\lfloor \frac{n}{2} \right\rfloor", "floor"),
        (r"\left\| \vec{v} \right\|", "norm"),
        (r"\left\langle \psi | \phi \right\rangle", "braket"),
        // Relations
        (
            r"A \subseteq B \implies A \cap C \subseteq B \cap C",
            "set_theory",
        ),
        (r"\forall \epsilon > 0, \exists \delta > 0", "logic"),
        // Arrows
        (r"A \xrightarrow{f} B \xrightarrow{g} C", "arrows_chain"),
        (r"\hookrightarrow", "hook_arrow"),
    ];

    let mut issues = Vec::new();
    for (latex, name) in &formulas {
        let result = convert(latex, Format::LaTeX, Format::Svg);
        match result {
            Ok(svg) => {
                // Check for NaN/Infinity
                if svg.contains("NaN") || svg.contains("Infinity") {
                    issues.push(format!("{}: contains NaN/Infinity", name));
                }
                // Check for empty SVG
                if svg.len() < 50 {
                    issues.push(format!(
                        "{}: suspiciously small SVG ({} bytes)",
                        name,
                        svg.len()
                    ));
                }
                // Check for reasonable viewBox
                if !svg.contains("viewBox") {
                    issues.push(format!("{}: missing viewBox", name));
                }
                println!("OK: {} ({} bytes)", name, svg.len());
            },
            Err(e) => {
                issues.push(format!("{}: ERROR: {}", name, e));
                println!("FAIL: {}: {}", name, e);
            },
        }
    }

    println!("\n=== {} formulas tested ===", formulas.len());
    if issues.is_empty() {
        println!("All formulas rendered successfully!");
    } else {
        println!("{} issues found:", issues.len());
        for issue in &issues {
            println!("  - {}", issue);
        }
    }
}
