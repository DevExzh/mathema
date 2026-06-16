use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

fn main() {
    let config = Config::default();

    let cases = vec![
        ("hires_sqrt", r"\sqrt{x^2 + y^2}"),
        ("hires_nested_sqrt", r"\sqrt{\sqrt{x} + \sqrt[3]{y}}"),
        ("hires_quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        ("hires_sum", r"\sum_{i=0}^{n} x_i"),
        ("hires_integral", r"\int_{0}^{\infty} e^{-x^2} dx"),
        (
            "hires_matrix",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        ),
        (
            "hires_large_frac",
            r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)",
        ),
        (
            "hires_cases",
            r"\begin{cases} x & \text{if } x > 0 \\ -x & \text{otherwise} \end{cases}",
        ),
        ("hires_euler", r"e^{i\pi} + 1 = 0"),
        (
            "hires_gauss",
            r"\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}",
        ),
    ];

    std::fs::create_dir_all("test_matrix").ok();
    for (name, latex) in &cases {
        match LatexReader::parse_compact(latex, &config) {
            Ok(result) => {
                use mathema::render::RenderOptions;
                use mathema::render::raster::{
                    ImageFormat, RasterOptions, render_to_image_compact,
                };
                let opts = RasterOptions {
                    render: RenderOptions {
                        font_size_px: 48.0,
                        display_mode: Some(true),
                        fg_color: "#000000".into(),
                        font_family: "KaTeX_Main, Latin Modern Math, STIX Two Math, serif".into(),
                    },
                    format: ImageFormat::Png,
                    scale: 2.0,
                    background: Some([255, 255, 255, 255]),
                    quality: 90,
                };
                match render_to_image_compact(&result.expr, &opts) {
                    Ok(png) => {
                        std::fs::write(format!("test_matrix/{name}.png"), &png).unwrap();
                        println!("OK: {name}");
                    },
                    Err(e) => println!("FAIL: {name}: {e}"),
                }
            },
            Err(e) => println!("FAIL: {name}: {e}"),
        }
    }
}
