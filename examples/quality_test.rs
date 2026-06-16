use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::{RenderOptions, render_to_svg};

fn main() {
    let config = Config::default();
    let options = RenderOptions::default();

    let formulas = vec![
        // Test integral display size
        (
            r"\displaystyle\int_0^\infty e^{-x^2} dx",
            "integral_display",
        ),
        // Test double integral
        (r"\iint_D f(x,y) \, dA", "double_integral"),
        // Test product with limits
        (r"\prod_{k=1}^{n} \frac{k^2}{k^2-1}", "product_frac"),
        // Test deeply nested fractions
        (
            r"\frac{1}{1+\frac{1}{2+\frac{1}{3+\frac{1}{4}}}}",
            "deep_nested",
        ),
        // Test matrix 3x3
        (
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
            "matrix3x3",
        ),
        // Test large delimiter with fraction
        (
            r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)",
            "nested_frac_delim",
        ),
        // Test accents stacking
        (r"\hat{\hat{x}} + \vec{\dot{y}}", "stacked_accents"),
        // Test text/operatorname
        (
            r"\operatorname{lim}_{x \to 0} \frac{\sin x}{x} = 1",
            "operatorname",
        ),
        // Test spacing: thin/medium/thick
        (r"a\,b\;c\quad d\qquad e", "spacing"),
        // Test summation with complex limits
        (
            r"\sum_{\substack{0 \leq i \leq m \\ 0 < j < n}} P(i,j)",
            "substack",
        ),
        // Test phantom
        (r"\phantom{x} + y", "phantom"),
        // Test boxed
        (r"\boxed{E = mc^2}", "boxed"),
        // Test cancel
        (r"\cancel{x} + y", "cancel"),
        // Test large floor/ceil
        (
            r"\left\lfloor\frac{n}{2}\right\rfloor + \left\lceil\frac{n}{3}\right\rceil",
            "floor_ceil",
        ),
        // Test angle brackets
        (r"\langle x, y \rangle", "angle_brackets"),
        // Test hat vs widehat
        (
            r"\hat{x} \quad \widehat{xy} \quad \widehat{xyz}",
            "hat_vs_widehat",
        ),
    ];

    std::fs::create_dir_all("test_matrix").ok();

    for (latex, name) in &formulas {
        match LatexReader::parse(latex, &config) {
            Ok(result) => match render_to_svg(&result.expr, &options) {
                Ok(svg) => {
                    let path = format!("test_matrix/{}.svg", name);
                    std::fs::write(&path, &svg).unwrap();
                    println!("OK: {} ({} bytes)", name, svg.len());
                },
                Err(e) => println!("RENDER ERR: {} - {}", name, e),
            },
            Err(e) => println!("PARSE ERR: {} - {}", name, e),
        }
    }
}
