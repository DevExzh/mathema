use mathema::Format;
use mathema::convert;
use mathema::from_latex;
use std::fs;

fn main() {
    let formulas = vec![
        // Core
        (r"x^2 + y^2 = z^2", "01_pythagorean"),
        (r"\frac{a+b}{c-d}", "02_fraction"),
        (r"\sqrt{x^2 + y^2}", "03_sqrt"),
        (r"\sum_{i=1}^{n} x_i", "04_sum"),
        (r"\int_0^{\infty} e^{-x^2} \, dx", "05_integral"),
        (r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}", "06_quadratic"),
        (
            r"\lim_{n \to \infty} \left(1+\frac{1}{n}\right)^n",
            "07_euler_limit",
        ),
        // Matrices
        (
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            "08_pmatrix",
        ),
        (
            r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}",
            "09_bmatrix",
        ),
        (
            r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
            "10_vmatrix",
        ),
        // Cases
        (
            r"|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}",
            "11_cases",
        ),
        // Accents
        (r"\hat{x} + \bar{y} + \dot{z} + \vec{v}", "12_accents"),
        (r"\widehat{ABC}", "13_wide_hat"),
        // Over/underbrace
        (r"\underbrace{a+b+\cdots+z}_{26}", "14_underbrace"),
        (r"\overbrace{1+2+3}^{6}", "15_overbrace"),
        // Large operators
        (r"\prod_{k=1}^n k!", "16_product"),
        (r"\coprod_{i \in I} X_i", "17_coprod"),
        // Delimiters
        (r"\left(\frac{a}{b}\right)", "18_parens_frac"),
        (r"\left\{\sum_k a_k\right\}", "19_braces"),
        (r"\left\lfloor \frac{n}{2} \right\rfloor", "20_floor"),
        // Nested fractions
        (r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}", "21_cfrac"),
        // Scripts
        (r"e^{i\pi} + 1 = 0", "22_euler_identity"),
        (r"a_{i_j}^{k^l}", "23_nested_scripts"),
        // Greek
        (
            r"\alpha\beta\gamma\delta\epsilon\zeta\eta\theta",
            "24_greek",
        ),
        (
            r"\Gamma\Delta\Theta\Lambda\Xi\Pi\Sigma\Phi\Psi\Omega",
            "25_greek_upper",
        ),
        // Arrows
        (r"A \xrightarrow{f} B \xrightarrow{g} C", "26_arrows"),
        // Maxwell
        (
            r"\nabla \cdot \vec{E} = \frac{\rho}{\epsilon_0}",
            "27_gauss",
        ),
        // Aligned environment
        (
            r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}",
            "28_aligned",
        ),
        // Binomial
        (r"\binom{n}{k} = \frac{n!}{k!(n-k)!}", "29_binomial"),
        // Color
        (r"\textcolor{red}{x} + \textcolor{blue}{y}", "30_color"),
    ];

    let dir = "test_gallery";
    fs::create_dir_all(dir).unwrap();

    let mut html = String::from(
        r#"<!DOCTYPE html><html><head><style>
body { font-family: sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; }
.row { display: flex; align-items: center; margin: 10px 0; border-bottom: 1px solid #eee; padding: 10px 0; }
.label { width: 200px; font-weight: bold; }
.svg-out { flex: 1; background: #fafafa; padding: 5px; text-align: center; }
.svg-out img { max-height: 80px; }
.latex { flex: 1; font-family: monospace; font-size: 12px; word-break: break-all; }
</style></head><body><h1>Mathema Rendering Gallery</h1>
"#,
    );

    for (latex, name) in &formulas {
        // SVG
        match convert(latex, Format::LaTeX, Format::Svg) {
            Ok(svg) => {
                let svg_path = format!("{}/{}.svg", dir, name);
                fs::write(&svg_path, &svg).unwrap();

                html.push_str(&format!(
                    r#"<div class="row"><div class="label">{}</div><div class="svg-out"><img src="{}.svg"></div><div class="latex"><code>{}</code></div></div>"#,
                    name, name, latex.replace('<', "&lt;").replace('>', "&gt;")
                ));
                println!("OK: {} ({} bytes SVG)", name, svg.len());
            },
            Err(e) => {
                println!("FAIL: {}: {}", name, e);
            },
        }

        // PNG — use the builder API which returns binary bytes
        match from_latex(latex) {
            Ok(result) => match result.to_png() {
                Ok(png) => {
                    let png_path = format!("{}/{}.png", dir, name);
                    fs::write(&png_path, &png).unwrap();
                    println!("  PNG: {} bytes", png.len());
                },
                Err(e) => println!("  PNG FAIL: {}", e),
            },
            Err(e) => println!("  PNG FAIL: {}", e),
        }
    }

    html.push_str("</body></html>");
    fs::write(format!("{}/index.html", dir), &html).unwrap();
    println!("\nGallery written to {}/index.html", dir);
}
