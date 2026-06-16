use mathema::convert::from_latex;
use mathema::render::RenderOptions;
use mathema::render::raster::RasterOptions;

fn main() {
    let formulas = vec![
        // Basic operators at high res
        ("sum_only", r"\displaystyle\sum"),
        ("sum_display", r"\sum_{i=0}^{n} x_i"),
        ("integral_only", r"\displaystyle\int"),
        ("integral_display", r"\int_0^\infty e^{-x}\,dx"),
        // Delimiters
        ("large_parens", r"\left(\frac{a}{b}\right)"),
        ("large_brackets", r"\left[\frac{x}{y}\right]"),
        ("large_braces", r"\left\{\frac{p}{q}\right\}"),
        // Fractions and scripts
        ("nested_cfrac", r"\cfrac{1}{1+\cfrac{1}{2+\cfrac{1}{3}}}"),
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        // Matrices
        (
            "matrix_2x2",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        ),
        // Accents and text
        ("accents", r"\hat{x} + \vec{v} + \bar{z} + \tilde{w}"),
        ("binomial", r"\binom{n}{k} = \frac{n!}{k!(n-k)!}"),
        // Aligned
        (
            "aligned",
            r"\begin{aligned} a &= b + c \\ d &= e + f \end{aligned}",
        ),
        // Radicals
        ("radicals", r"\sqrt{\sqrt{x} + \sqrt[3]{y}}"),
    ];

    let svg_opts = RenderOptions::default();
    // High resolution for quality inspection
    let png_opts = RasterOptions {
        scale: 6.0,
        ..RasterOptions::default()
    };

    for (name, latex) in &formulas {
        match from_latex(latex) {
            Ok(result) => {
                match result.to_svg_with(&svg_opts) {
                    Ok(svg) => {
                        let path = format!("test_matrix/svg_{name}.svg");
                        std::fs::write(&path, &svg).unwrap();
                        println!("SVG OK: {name} ({} bytes)", svg.len());
                    },
                    Err(e) => println!("SVG FAIL: {name}: {e}"),
                }
                match result.to_png_with(&png_opts) {
                    Ok(png) => {
                        let path = format!("test_matrix/png_{name}.png");
                        std::fs::write(&path, &png).unwrap();
                        println!("PNG OK: {name} ({} bytes)", png.len());
                    },
                    Err(e) => println!("PNG FAIL: {name}: {e}"),
                }
            },
            Err(e) => println!("PARSE FAIL: {name}: {e}"),
        }
    }
}
