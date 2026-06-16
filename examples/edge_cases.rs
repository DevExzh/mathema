use mathema::from_latex;
use std::fs;

fn main() {
    let edge_cases = vec![
        // Empty/trivial
        (r"", "empty"),
        (r"x", "single_char"),
        // Nested scripts
        (r"x_{a_{b_{c}}}", "deep_subscript"),
        (r"2^{2^{2^{2}}}", "deep_superscript"),
        // Empty groups
        (r"\frac{}{b}", "empty_numerator"),
        (r"\sqrt{}", "empty_sqrt"),
        // Mixed operators
        (r"a + b - c \times d \div e", "mixed_ops"),
        // Parenthesis sizing edge
        (r"\left(\right)", "empty_parens"),
        (r"\left(\frac{a}{b}\right)^2", "frac_parens_script"),
        // Dots
        (r"a \cdot b \cdots c \ldots d \vdots e \ddots f", "dots"),
        // Multi-character operators
        (r"\sin(\theta) + \cos(\phi) + \tan(\psi)", "trig_ops"),
        (r"\log_2 n + \ln x + \exp(y)", "log_ops"),
        (r"\min(a,b) + \max(c,d) + \sup S + \inf T", "minmax_ops"),
        // Limits variations
        (r"\lim_{x \to 0^+} \frac{\sin x}{x}", "limit_positive"),
        (
            r"\limsup_{n\to\infty} a_n = \liminf_{n\to\infty} a_n",
            "limsup_inf",
        ),
        // Multiline
        (
            r"\begin{aligned} a &= b + c \\ d &= e - f \\ g &= h \cdot i \end{aligned}",
            "aligned_3line",
        ),
        // Matrices
        (
            r"\begin{pmatrix} 1 & 0 & 0 \\ 0 & 1 & 0 \\ 0 & 0 & 1 \end{pmatrix}",
            "identity_3x3",
        ),
        // Negative space
        (r"x \! y", "neg_space"),
        // Big operators with limits
        (r"\int_0^1 \int_0^1 f(x,y)\,dx\,dy", "double_integral"),
        // Modular arithmetic
        (r"a \equiv b \pmod{n}", "modular"),
        // Various arrows
        (
            r"A \Rightarrow B \Leftrightarrow C \implies D",
            "arrows_logic",
        ),
        (r"x \mapsto f(x)", "mapsto"),
        // Set theory
        (
            r"A \cap B \cup C \setminus D \subset E \subseteq F",
            "set_theory",
        ),
        // Infinity and empty set
        (r"\infty + \emptyset + \varnothing", "special_symbols"),
        // Stackrel
        (r"A \stackrel{f}{\longrightarrow} B", "stackrel"),
        // Hat on multiple chars
        (r"\widehat{xyz}", "widehat_multi"),
        // Prime
        (r"f'(x) + g''(x) + h'''(x)", "primes"),
        // Complex fraction
        (r"\frac{1}{\frac{1}{\frac{1}{x+1}+1}+1}", "complex_frac"),
        // Tensors
        (r"R^i{}_{jkl}", "riemann_tensor"),
        // Dirac notation
        (r"\langle \psi | \hat{H} | \phi \rangle", "dirac"),
    ];

    let dir = "test_gallery/edge";
    fs::create_dir_all(dir).unwrap();

    let mut pass = 0;
    let mut fail = 0;
    let mut svg_fail = 0;
    for (latex, name) in &edge_cases {
        // Skip empty input
        if latex.is_empty() {
            println!("SKIP: {} (empty input)", name);
            continue;
        }
        match from_latex(latex) {
            Ok(result) => {
                match result.to_png() {
                    Ok(png) => {
                        fs::write(format!("{}/{}.png", dir, name), &png).unwrap();
                        println!("OK: {} ({} bytes)", name, png.len());
                        pass += 1;
                    },
                    Err(e) => {
                        println!("FAIL PNG: {}: {}", name, e);
                        fail += 1;
                    },
                }
                match result.to_svg() {
                    Ok(svg) => {
                        fs::write(format!("{}/{}.svg", dir, name), &svg).unwrap();
                    },
                    Err(e) => {
                        println!("  SVG FAIL: {}: {}", name, e);
                        svg_fail += 1;
                    },
                }
            },
            Err(e) => {
                println!("FAIL PARSE: {}: {}", name, e);
                fail += 1;
            },
        }
    }
    println!(
        "\n{} passed, {} failed, {} SVG failures out of {}",
        pass,
        fail,
        svg_fail,
        edge_cases.len()
    );
}
