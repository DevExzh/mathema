use mathema::from_latex;
use mathema::render::raster::RasterOptions;

fn main() {
    let formulas = vec![
        (
            r"\displaystyle\int_0^\infty e^{-x^2} dx",
            "integral_display",
        ),
        (r"\prod_{k=1}^{n} \frac{k^2}{k^2-1}", "product_frac"),
        (
            r"\frac{1}{1+\frac{1}{2+\frac{1}{3+\frac{1}{4}}}}",
            "deep_nested",
        ),
        (
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
            "matrix3x3",
        ),
        (
            r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)",
            "nested_frac_delim",
        ),
        (r"\underbrace{a+b+\cdots+z}_{26}", "underbrace"),
        (r"\overbrace{1+2+3}^{6}", "overbrace"),
        (r"\boxed{E = mc^2}", "boxed"),
        (
            r"\hat{x} \quad \widehat{xy} \quad \widehat{xyz}",
            "hat_vs_widehat",
        ),
        (
            r"\sum_{\substack{0 \leq i \leq m \\ 0 < j < n}} P(i,j)",
            "substack",
        ),
    ];

    let options = RasterOptions {
        scale: 3.0,
        ..Default::default()
    };

    std::fs::create_dir_all("test_matrix").ok();

    for (latex, name) in &formulas {
        match from_latex(latex) {
            Ok(result) => match result.to_png_with(&options) {
                Ok(png) => {
                    let path = format!("test_matrix/{}.png", name);
                    std::fs::write(&path, &png).unwrap();
                    println!("OK: {} ({} bytes)", name, png.len());
                },
                Err(e) => println!("RENDER ERR: {} - {}", name, e),
            },
            Err(e) => println!("PARSE ERR: {} - {}", name, e),
        }
    }
}
