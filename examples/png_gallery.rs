use mathema::from_latex;
use std::fs;

fn main() {
    let formulas = vec![
        (r"x^2 + y^2 = z^2", "01_pythagorean"),
        (r"\frac{a+b}{c-d}", "02_fraction"),
        (r"\sqrt{x^2 + y^2}", "03_sqrt"),
        (r"\sum_{i=1}^{n} x_i", "04_sum"),
        (r"\int_0^{\infty} e^{-x^2}\,dx", "05_integral"),
        (r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}", "06_quadratic"),
        (
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            "07_pmatrix",
        ),
        (
            r"|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}",
            "08_cases",
        ),
        (r"\hat{x} + \bar{y} + \dot{z} + \vec{v}", "09_accents"),
        (r"\underbrace{a+b+c}_{n}", "10_underbrace"),
        (r"e^{i\pi} + 1 = 0", "11_euler"),
        (r"\alpha\beta\gamma\delta\epsilon", "12_greek"),
        (r"\left(\frac{a}{b}\right)", "13_parens_frac"),
        (r"\left\{\sum_k a_k\right\}", "14_braces"),
        (r"\binom{n}{k}", "15_binom"),
        (
            r"\nabla \cdot \vec{E} = \frac{\rho}{\epsilon_0}",
            "16_gauss",
        ),
    ];

    let dir = "test_gallery";
    fs::create_dir_all(dir).unwrap();

    for (latex, name) in &formulas {
        let result = from_latex(latex);
        match result.unwrap().to_png() {
            Ok(png) => {
                let path = format!("{}/{}.png", dir, name);
                fs::write::<_, &[u8]>(&path, &png).unwrap();
                println!("OK: {} ({} bytes)", name, png.len());
            },
            Err(e) => println!("FAIL: {}: {}", name, e),
        }
    }
    println!("\nDone.");
}
