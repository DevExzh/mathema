use std::time::Instant;

fn main() {
    let formulas: Vec<(&str, &str)> = vec![
        (r"x^2", "simple"),
        (r"\frac{a}{b}", "fraction"),
        (r"\sum_{i=0}^{n} i^2", "sum"),
        (r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", "quadratic"),
        (r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", "matrix2x2"),
        (
            r"\left(\sum_{k=1}^n a_k b_k\right)^2 \leq \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
            "cauchy_schwarz",
        ),
        (
            r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}}",
            "deep_cfrac",
        ),
        (
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
            "matrix3x3",
        ),
    ];

    // Warmup
    let _ = mathema::from_latex(r"x").unwrap().to_svg();

    println!(
        "{:<20} {:>10} {:>10} {:>10}",
        "Formula", "Parse(µs)", "SVG(µs)", "Total(µs)"
    );
    println!("{}", "-".repeat(55));

    for (latex, name) in &formulas {
        let n = 100;

        // Measure parse
        let start = Instant::now();
        for _ in 0..n {
            let _ = mathema::from_latex(latex).unwrap();
        }
        let parse_us = start.elapsed().as_micros() / n as u128;

        // Measure SVG render
        let result = mathema::from_latex(latex).unwrap();
        let start = Instant::now();
        for _ in 0..n {
            let _ = result.to_svg().unwrap();
        }
        let svg_us = start.elapsed().as_micros() / n as u128;

        println!(
            "{:<20} {:>10} {:>10} {:>10}",
            name,
            parse_us,
            svg_us,
            parse_us + svg_us
        );
    }

    // Also measure PNG rendering (heavier)
    println!("\n{:<20} {:>10}", "Formula", "PNG(µs)");
    println!("{}", "-".repeat(35));

    for (latex, name) in &formulas {
        let n = 20;
        let result = mathema::from_latex(latex).unwrap();
        let start = Instant::now();
        for _ in 0..n {
            let _ = result.to_png().unwrap();
        }
        let png_us = start.elapsed().as_micros() / n as u128;
        println!("{:<20} {:>10}", name, png_us);
    }
}
