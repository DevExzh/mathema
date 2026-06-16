use mathema::from_latex;
use std::fs;

fn main() {
    let formulas = vec![
        (
            r"\begin{array}{|c|c|c|} a & b & c \\ d & e & f \end{array}",
            "array_full_rules",
        ),
        (
            r"\begin{array}{c|c} x & y \\ 1 & 2 \end{array}",
            "array_single_rule",
        ),
        (
            r"\begin{array}{|l|r|} \text{left} & \text{right} \\ a & b \end{array}",
            "array_lr_rules",
        ),
    ];

    let dir = "test_gallery";
    fs::create_dir_all(dir).unwrap();

    for (latex, name) in &formulas {
        // SVG
        match from_latex(latex) {
            Ok(result) => {
                match result.to_svg() {
                    Ok(svg) => {
                        let path = format!("{}/sep_{}.svg", dir, name);
                        fs::write(&path, &svg).unwrap();
                        println!("SVG OK: {} ({} bytes)", name, svg.len());
                    },
                    Err(e) => println!("SVG FAIL: {}: {}", name, e),
                }
                match result.to_png() {
                    Ok(png) => {
                        let path = format!("{}/sep_{}.png", dir, name);
                        fs::write::<_, &[u8]>(&path, &png).unwrap();
                        println!("PNG OK: {} ({} bytes)", name, png.len());
                    },
                    Err(e) => println!("PNG FAIL: {}: {}", name, e),
                }
            },
            Err(e) => println!("PARSE FAIL: {}: {}", name, e),
        }
    }
}
