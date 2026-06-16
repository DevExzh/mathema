use mathema::from_latex;
use std::fs;

fn main() {
    let formulas = vec![
        // Deeply nested
        (r"\frac{\frac{\frac{a}{b}}{c}}{d}", "deep_frac"),
        // Large expression
        (
            r"\int_{-\infty}^{\infty} \frac{1}{\sqrt{2\pi\sigma^2}} e^{-\frac{(x-\mu)^2}{2\sigma^2}} \, dx = 1",
            "gaussian",
        ),
        // Multiple scripts
        (r"T^{\mu\nu}_{\;\;\alpha\beta}", "tensor"),
        // Double sum
        (r"\sum_{i=1}^{m} \sum_{j=1}^{n} a_{ij} x_j", "double_sum"),
        // Matrix multiplication
        (
            r"\begin{pmatrix} a_{11} & a_{12} \\ a_{21} & a_{22} \end{pmatrix} \begin{pmatrix} x \\ y \end{pmatrix}",
            "matrix_mult",
        ),
        // Tall delimiters
        (
            r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)",
            "tall_delim",
        ),
        // Mixed operators
        (
            r"f'(x) = \lim_{h \to 0} \frac{f(x+h) - f(x)}{h}",
            "derivative",
        ),
        // Complex integral
        (
            r"\oint_{\partial\Sigma} \vec{E} \cdot d\vec{l} = -\frac{d}{dt}\iint_{\Sigma} \vec{B} \cdot d\vec{S}",
            "faraday",
        ),
        // Stokes
        (
            r"\nabla \times \vec{F} = \left(\frac{\partial F_z}{\partial y} - \frac{\partial F_y}{\partial z}\right)\hat{i}",
            "curl",
        ),
        // Big operators with limits
        (
            r"\bigcup_{i=1}^{n} A_i = A_1 \cup A_2 \cup \cdots \cup A_n",
            "bigcup",
        ),
        // Aligned system
        (
            r"\begin{aligned} \nabla \cdot \vec{E} &= \frac{\rho}{\epsilon_0} \\ \nabla \cdot \vec{B} &= 0 \end{aligned}",
            "maxwell",
        ),
        // Floor/ceil
        (
            r"\left\lfloor \frac{n}{2} \right\rfloor + \left\lceil \frac{n}{2} \right\rceil = n",
            "floor_ceil",
        ),
        // Overset/underset
        (r"\overset{n}{\underset{k=0}{\sum}} \binom{n}{k}", "overset"),
        // Spacing test
        (r"a \quad b \qquad c \, d \; e \: f", "spacing"),
        // Styled math
        (r"\mathbb{R}^n \to \mathbb{R}", "styled"),
        // Phantom
        (r"x^2 + \phantom{2x} + 1", "phantom"),
        // Boxed
        (r"\boxed{E = mc^2}", "boxed"),
        // Multiline fractions
        (r"\frac{1+\frac{1}{x}}{1-\frac{1}{x}}", "nested_frac2"),
        // Subscript/superscript stress
        (r"x^{y^{z^{w}}}", "power_tower"),
        // Wide accents
        (r"\widetilde{ABC} + \widehat{XYZ}", "wide_accents"),
    ];

    let dir = "test_gallery/stress";
    fs::create_dir_all(dir).unwrap();

    let mut pass = 0;
    let mut fail = 0;
    for (latex, name) in &formulas {
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
                    Err(e) => println!("  SVG FAIL: {}: {}", name, e),
                }
            },
            Err(e) => {
                println!("FAIL PARSE: {}: {}", name, e);
                fail += 1;
            },
        }
    }
    println!(
        "\n{} passed, {} failed out of {}",
        pass,
        fail,
        formulas.len()
    );
}
