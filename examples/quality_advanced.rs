use mathema::from_latex;
use mathema::render::raster::RasterOptions;

fn main() {
    let formulas = vec![
        // Complex real-world formulas
        (
            r"\displaystyle\sum_{n=0}^{\infty} \frac{(-1)^n}{(2n+1)!} x^{2n+1}",
            "taylor_sin",
        ),
        (
            r"\left\{\begin{array}{ll} x^2 + y^2 = 1 \\ y = mx + b \end{array}\right.",
            "system_eq",
        ),
        (
            r"\int_0^{2\pi} \frac{d\theta}{a + b\cos\theta} = \frac{2\pi}{\sqrt{a^2-b^2}}",
            "integral_cos",
        ),
        (
            r"\mathbb{E}[X] = \int_{-\infty}^{\infty} x f(x) \, dx",
            "expected_value",
        ),
        (
            r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
            "maxwell_curl",
        ),
        (r"\binom{n}{k} = \frac{n!}{k!(n-k)!}", "binomial_def"),
        (
            r"\prod_{p \text{ prime}} \frac{1}{1-p^{-s}} = \sum_{n=1}^{\infty} \frac{1}{n^s}",
            "euler_prod",
        ),
        (
            r"\left(\sum_{k=1}^n a_k b_k\right)^2 \leq \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
            "cauchy_schwarz",
        ),
        (r"e^{i\theta} = \cos\theta + i\sin\theta", "euler_formula"),
        (
            r"\frac{\partial^2 u}{\partial t^2} = c^2 \nabla^2 u",
            "wave_eq",
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
