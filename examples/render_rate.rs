// Measure LaTeX parse+render success rate over the KaTeX screenshotter corpus.
// Run: cargo run --example render_rate --features raster
use std::fs;

fn main() {
    let data = fs::read_to_string("tests/data/katex_corpus.json").unwrap();
    let formulas: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();
    let total = formulas.len();
    let mut parse_fail = Vec::new();
    let mut render_fail = Vec::new();
    let mut ok = 0;
    for f in &formulas {
        let name = f["name"].as_str().unwrap();
        let tex = f["tex"].as_str().unwrap_or("");
        match mathema::from_latex(tex) {
            Err(e) => parse_fail.push((name.to_string(), e.to_string())),
            Ok(r) => match r.to_svg() {
                Err(e) => render_fail.push((name.to_string(), e.to_string())),
                Ok(_) => ok += 1,
            },
        }
    }
    println!(
        "=== render rate: {ok}/{total} ({:.1}%) ===",
        100.0 * ok as f64 / total as f64
    );
    println!("\n--- parse failures ({}) ---", parse_fail.len());
    for (n, e) in &parse_fail {
        println!("  {n}: {}", e.lines().next().unwrap_or(""));
    }
    println!("\n--- render failures ({}) ---", render_fail.len());
    for (n, e) in &render_fail {
        println!("  {n}: {}", e.lines().next().unwrap_or(""));
    }
}
