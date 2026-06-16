use mathema::from_latex;
use std::fs;

fn main() {
    let formulas = vec![
        (r"\sqrt[3]{x+y}", "nth_root"),
        (
            r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}",
            "identity_matrix",
        ),
        (
            r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
            "determinant",
        ),
        (r"\binom{n}{k}", "binomial"),
        (r"\sin^2\theta + \cos^2\theta = 1", "trig_identity"),
        (r"\overrightarrow{AB}", "overrightarrow"),
        (r"f'(x) = \lim_{h\to 0} \frac{f(x+h)-f(x)}{h}", "derivative"),
        (r"\boxed{E = mc^2}", "boxed"),
        (r"\cancel{x}", "cancel"),
        (
            r"\textcolor{red}{x} + \textcolor{blue}{y} = \textcolor{green}{z}",
            "colors",
        ),
        (r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}", "continued_frac"),
        (
            r"\begin{aligned} f(x) &= x^2 + 2x + 1 \\ &= (x+1)^2 \end{aligned}",
            "aligned",
        ),
        (r"a \equiv b \pmod{n}", "modular"),
        (
            r"\forall \epsilon > 0\, \exists \delta > 0 : |x-a| < \delta \Rightarrow |f(x)-L| < \epsilon",
            "epsilon_delta",
        ),
        (
            r"\iiint_V \nabla \cdot \vec{F}\, dV = \oiint_S \vec{F} \cdot d\vec{S}",
            "divergence_thm",
        ),
        (
            r"\frac{\partial^2 u}{\partial x^2} + \frac{\partial^2 u}{\partial y^2} = 0",
            "laplace",
        ),
    ];

    let dir = "test_gallery";
    fs::create_dir_all(dir).unwrap();

    for (latex, name) in &formulas {
        match from_latex(latex) {
            Ok(result) => match result.to_png() {
                Ok(png) => {
                    let path = format!("{}/extra_{}.png", dir, name);
                    fs::write::<_, &[u8]>(&path, &png).unwrap();
                    println!("OK: {} ({} bytes)", name, png.len());
                },
                Err(e) => println!("FAIL (png): {}: {}", name, e),
            },
            Err(e) => println!("FAIL (parse): {}: {}", name, e),
        }
    }
}
