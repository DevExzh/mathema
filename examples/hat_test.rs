use mathema::convert::from_latex;
use mathema::render::raster::RasterOptions;

fn main() {
    let cases = vec![
        ("hat_x", r"\hat{x}"),
        ("widehat_abc", r"\widehat{abc}"),
        ("hat_only", r"\hat{a}"),
    ];

    let opts = RasterOptions {
        scale: 10.0,
        ..RasterOptions::default()
    };

    for (name, latex) in &cases {
        match from_latex(latex) {
            Ok(result) => match result.to_png_with(&opts) {
                Ok(png) => {
                    let path = format!("test_matrix/hat_{name}.png");
                    std::fs::write(&path, &png).unwrap();
                    println!("OK: {name} ({} bytes)", png.len());
                },
                Err(e) => println!("FAIL: {name}: {e}"),
            },
            Err(e) => println!("PARSE FAIL: {name}: {e}"),
        }
    }
}
