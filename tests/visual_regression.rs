#![cfg(feature = "raster")]

use std::fs;
use std::path::Path;

use mathema::render::RenderOptions;
use mathema::render::raster::{ImageFormat, RasterOptions};

struct Formula {
    name: String,
    tex: String,
    display: bool,
    category: String,
}

fn load_formulas() -> Vec<Formula> {
    let path = "tests/data/formulas.json";
    if !Path::new(path).exists() {
        println!("skipping {path}: corpus not present");
        return Vec::new();
    }
    let data = fs::read_to_string(path).expect("tests/data/formulas.json should exist");
    let values: Vec<serde_json::Value> =
        serde_json::from_str(&data).expect("formulas.json should be valid JSON");

    values
        .into_iter()
        .map(|value| Formula {
            name: value["name"].as_str().unwrap_or("unknown").to_string(),
            tex: value["tex"].as_str().unwrap_or("").to_string(),
            display: value["display"].as_bool().unwrap_or(false),
            category: value["category"]
                .as_str()
                .unwrap_or("uncategorized")
                .to_string(),
        })
        .collect()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[test]
#[ignore = "generates the full third-party visual fixture PNG set"]
fn render_third_party_visual_fixture_pngs() {
    let formulas = load_formulas();
    let output_dir = Path::new("tests/data/output");
    fs::create_dir_all(output_dir).expect("output directory should be creatable");

    let mut html = String::from(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>mathema visual regression output</title>
<style>
body { font-family: system-ui, sans-serif; margin: 24px; }
.formula { border-top: 1px solid #ddd; padding: 12px 0; }
.meta { color: #555; font-size: 12px; margin-bottom: 8px; }
.tex { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; white-space: pre-wrap; }
img { background: white; max-width: 100%; }
.error { color: #a40000; }
</style>
</head>
<body>
<h1>mathema visual regression output</h1>
"#,
    );

    let mut rendered = 0usize;
    let mut failures = Vec::new();

    for formula in &formulas {
        let options = RasterOptions {
            render: RenderOptions {
                display_mode: Some(formula.display),
                ..RenderOptions::default()
            },
            format: ImageFormat::Png,
            scale: 2.0,
            background: None,
            quality: 90,
        };

        match mathema::from_latex(&formula.tex).and_then(|result| result.to_png_with(&options)) {
            Ok(png) => {
                fs::write(output_dir.join(format!("{}.png", formula.name)), png)
                    .expect("PNG should be writable");
                rendered += 1;
                html.push_str(&format!(
                    r#"<div class="formula"><div class="meta"><strong>{}</strong> / {}</div><img src="{}.png" alt="{}"><div class="tex">{}</div></div>
"#,
                    escape_html(&formula.category),
                    escape_html(&formula.name),
                    escape_html(&formula.name),
                    escape_html(&formula.name),
                    escape_html(&formula.tex)
                ));
            },
            Err(err) => {
                failures.push((formula.name.clone(), err.to_string()));
                html.push_str(&format!(
                    r#"<div class="formula error"><div class="meta"><strong>{}</strong> / {}</div><div>{}</div><div class="tex">{}</div></div>
"#,
                    escape_html(&formula.category),
                    escape_html(&formula.name),
                    escape_html(&err.to_string()),
                    escape_html(&formula.tex)
                ));
            },
        }
    }

    html.push_str("</body></html>\n");
    fs::write(output_dir.join("mathema_visual_regression.html"), html)
        .expect("HTML report should be writable");

    println!(
        "Rendered {}/{} formulas to {}",
        rendered,
        formulas.len(),
        output_dir.display()
    );

    if !failures.is_empty() {
        println!("Failures:");
        for (name, err) in failures.iter().take(50) {
            println!("  {name}: {err}");
        }
    }

    assert_eq!(
        rendered,
        formulas.len(),
        "all visual fixtures should render"
    );
}
