use std::fs;

fn main() {
    let data = fs::read_to_string("tests/data/formulas.json").unwrap();
    let formulas: serde_json::Value = serde_json::from_str(&data).unwrap();
    let formulas = formulas.as_array().unwrap();

    let out_dir = "tests/data/output/mathema_svgs";
    fs::create_dir_all(out_dir).unwrap();

    let mut success = 0;
    let mut fail = 0;
    let mut failures: Vec<(String, String, String)> = Vec::new();

    for f in formulas {
        let name = f["name"].as_str().unwrap_or("unknown");
        let tex = f["tex"].as_str().unwrap_or("");
        let category = f["category"].as_str().unwrap_or("unknown");

        let display = f["display"].as_bool().unwrap_or(false);
        let opts = mathema::render::RenderOptions {
            display_mode: Some(display),
            ..Default::default()
        };
        match mathema::from_latex(tex).and_then(|r| r.to_svg_with(&opts)) {
            Ok(svg) => {
                let path = format!("{}/{}.svg", out_dir, name);
                fs::write(&path, &svg).unwrap();
                success += 1;
            },
            Err(e) => {
                let short_tex = if tex.len() > 60 { &tex[..60] } else { tex };
                failures.push((
                    name.to_string(),
                    category.to_string(),
                    format!("{}: {}", short_tex, e),
                ));
                fail += 1;
            },
        }
    }

    println!(
        "Rendered {}/{} formulas ({} failures)",
        success,
        formulas.len(),
        fail
    );

    if !failures.is_empty() {
        println!("\nFailures:");
        for (name, cat, msg) in &failures {
            println!("  [{}] {} - {}", cat, name, msg);
        }
    }
}
