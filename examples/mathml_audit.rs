//! MathML corpus audit: parse + render every `*.mml` under a directory and
//! report any that fail to parse, fail to render, or produce empty output.
//!
//! Usage: `cargo run --example mathml_audit --features render -- <dir>`
//! (defaults to the `MATHEMA_TESTSUITE_DIR` env var, then `./testsuite`).

use std::path::{Path, PathBuf};

use mathema::ast::DisplayStyle;
use mathema::error::{Config, Strictness};
use mathema::from_mathml_with;

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "mml") {
            out.push(p);
        }
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("MATHEMA_TESTSUITE_DIR").ok())
        .unwrap_or_else(|| "testsuite".into());

    let mut files = Vec::new();
    collect(Path::new(&root), &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("no .mml files found under {root}");
        std::process::exit(1);
    }

    let cfg = Config {
        strictness: Strictness::Lenient,
        display: DisplayStyle::Block,
    };

    let (mut parse_fail, mut render_fail, mut empty, mut ok) = (0, 0, 0, 0);
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).display();
        let src = std::fs::read_to_string(f).unwrap_or_default();
        if src.trim().is_empty() {
            continue;
        }
        match from_mathml_with(&src, &cfg) {
            Err(e) => {
                parse_fail += 1;
                println!(
                    "PARSE  {rel}: {}",
                    format!("{e}").lines().next().unwrap_or("")
                );
            },
            Ok(r) => match r.to_svg() {
                Err(e) => {
                    render_fail += 1;
                    println!(
                        "RENDER {rel}: {}",
                        format!("{e}").lines().next().unwrap_or("")
                    );
                },
                Ok(svg) => {
                    if r.expr().nodes.is_empty() {
                        empty += 1;
                    } else if !(svg.contains("<path")
                        || svg.contains("<use")
                        || svg.contains("<text"))
                    {
                        empty += 1;
                        println!("EMPTY  {rel}");
                    } else {
                        ok += 1;
                    }
                },
            },
        }
    }

    println!(
        "\n{} files: ok={ok} empty={empty} parse_fail={parse_fail} render_fail={render_fail}",
        files.len()
    );
    if parse_fail > 0 || render_fail > 0 {
        std::process::exit(1);
    }
}
