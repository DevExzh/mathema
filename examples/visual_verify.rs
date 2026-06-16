use mathema::convert::from_latex;
use mathema::render::raster::{ImageFormat, RasterOptions};

fn main() {
    let png_opts = RasterOptions {
        scale: 3.0,
        format: ImageFormat::Png,
        ..RasterOptions::default()
    };

    let samples = vec![
        ("italic_fx", r"f(x) = ax^2 + bx + c"),
        ("greek_italic", r"\alpha\beta\gamma\theta\phi"),
        (
            "mixed_styles",
            r"\mathbf{F} = m\mathbf{a}, \quad \mathbb{R}^n",
        ),
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        (
            "integral",
            r"\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}",
        ),
    ];

    for (name, latex) in &samples {
        let conv = from_latex(latex).unwrap_or_else(|e| panic!("parse failed for {}: {}", name, e));

        let result = conv
            .to_png_with(&png_opts)
            .unwrap_or_else(|e| panic!("PNG failed for {}: {}", name, e));
        let path = format!("/tmp/math_italic_{}.png", name);
        std::fs::write(&path, &result).unwrap();
        println!("Wrote {}", path);

        let svg = conv
            .to_svg()
            .unwrap_or_else(|e| panic!("SVG failed for {}: {}", name, e));
        let path = format!("/tmp/math_italic_{}.svg", name);
        std::fs::write(&path, &svg).unwrap();
        println!("Wrote {}", path);
    }
}
