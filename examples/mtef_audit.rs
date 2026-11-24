//! MTEF corpus audit: decode every `*.bin` file under a directory (bare MTEF
//! or an OLE `Equation Native` stream) and check what mathema makes of it.
//!
//! Usage:
//! `cargo run --release --example mtef_audit --features render -- <dir> <out.tsv>`
//!
//! For each file it records the MTEF version and generating product, the
//! Strict-mode result, the Lenient-mode diagnostics, three round trips
//! compared by their MathML (MTEF → MTEF, MTEF → LaTeX → MathML and
//! MTEF → OMML → MathML; `same` or `diff`, raw string equality, so a `diff`
//! may be benign), and the start of the LaTeX output. Rendering to SVG is
//! exercised for panics. A panic anywhere is recorded as `PANIC`.
//!
//! TSV columns: sha (file stem), size, version, product, strict (`ok` or
//! `ERR <bucket>`), lenient diagnostics (`|`-separated buckets), mtef→mtef,
//! latex→mathml, omml→mathml, latex. Messages are bucketed by replacing each
//! run of digits with `#`, so that they group by kind.
//!
//! The summary printed at the end counts versions, Strict results, Lenient
//! diagnostic buckets (once per file) and round-trip outcomes.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use mathema::{Config, ConvertResult, MtefWriteOptions, Strictness};

/// Collapse digit runs to `#` so that messages group by kind.
fn bucket(message: &str) -> String {
    let mut out = String::new();
    let mut previous_digit = false;
    for c in message.chars() {
        if c.is_ascii_digit() {
            if !previous_digit {
                out.push('#');
            }
            previous_digit = true;
        } else {
            previous_digit = false;
            out.push(c);
        }
    }
    out.replace("0x#", "#").chars().take(110).collect()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|x| x == "bin") {
            out.push(path);
        }
    }
}

fn mathml(result: &ConvertResult) -> String {
    result.to_mathml().unwrap_or_else(|e| format!("ERR {e}"))
}

/// Compare a round trip's MathML with the original's.
fn compare(original: &str, reread: mathema::Result<ConvertResult>) -> String {
    match reread {
        Err(e) => format!("RERR {}", bucket(&e.to_string())),
        Ok(r) if mathml(&r) == original => "same".into(),
        Ok(_) => "diff".into(),
    }
}

/// The audit columns after size for one file.
fn audit(bytes: &[u8], strict: &Config, lenient: &Config) -> Vec<String> {
    let mut cols = Vec::with_capacity(8);
    let decoded = mathema::from_mtef_bytes_with(bytes, lenient);
    let (version, product) = decoded
        .as_ref()
        .ok()
        .and_then(|r| r.mtef_info())
        .map(|i| {
            (
                format!("{:?}", i.version),
                format!(
                    "{:?}{}.{}",
                    i.product, i.product_version, i.product_subversion
                ),
            )
        })
        .unwrap_or_default();
    cols.push(version);
    cols.push(product);
    cols.push(match mathema::from_mtef_bytes_with(bytes, strict) {
        Ok(_) => "ok".into(),
        Err(e) => format!("ERR {}", bucket(&e.to_string())),
    });
    let r = match decoded {
        Ok(r) => r,
        Err(e) => {
            cols.push(format!("LERR {}", bucket(&e.to_string())));
            cols.extend(std::iter::repeat_n(String::new(), 4));
            return cols;
        },
    };
    let diagnostics: Vec<String> = r.diagnostics().iter().map(|d| bucket(&d.message)).collect();
    cols.push(diagnostics.join(" | "));
    let original = mathml(&r);
    cols.push(match r.to_mtef_bytes(&MtefWriteOptions::default()) {
        Err(e) => format!("WERR {}", bucket(&e.to_string())),
        Ok(b) => compare(&original, mathema::from_mtef_bytes_with(&b, strict)),
    });
    let latex = r.to_latex();
    cols.push(match &latex {
        Err(e) => format!("WERR {}", bucket(&e.to_string())),
        Ok(s) => compare(&original, mathema::from_latex(s)),
    });
    cols.push(match r.to_omml() {
        Err(e) => format!("WERR {}", bucket(&e.to_string())),
        Ok(s) => compare(&original, mathema::from_omml(&s)),
    });
    // Rendering must not panic.
    let _ = r.to_svg();
    cols.push(
        latex
            .unwrap_or_default()
            .replace(['\t', '\n'], " ")
            .chars()
            .take(200)
            .collect(),
    );
    cols
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, dir, tsv_path] = args.as_slice() else {
        eprintln!("usage: mtef_audit <dir of .bin files> <out.tsv>");
        std::process::exit(2);
    };
    let mut files = Vec::new();
    collect(Path::new(dir), &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("no .bin files found under {dir}");
        std::process::exit(1);
    }
    let mut tsv = BufWriter::new(fs::File::create(tsv_path).unwrap_or_else(|e| {
        eprintln!("cannot create {tsv_path}: {e}");
        std::process::exit(1);
    }));

    // Panics are counted, not printed one by one.
    panic::set_hook(Box::new(|_| {}));
    let strict = Config::default().with_strictness(Strictness::Strict);
    let lenient = Config::default().with_strictness(Strictness::Lenient);

    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut strict_results: BTreeMap<String, usize> = BTreeMap::new();
    let mut diagnostics: BTreeMap<String, usize> = BTreeMap::new();
    let mut round_trips: [BTreeMap<String, usize>; 3] = Default::default();
    let mut panics = 0;
    for path in &files {
        let Ok(bytes) = fs::read(path) else {
            eprintln!("cannot read {}", path.display());
            continue;
        };
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        let cols = panic::catch_unwind(AssertUnwindSafe(|| audit(&bytes, &strict, &lenient)))
            .unwrap_or_else(|_| {
                panics += 1;
                vec!["PANIC".into()]
            });
        writeln!(tsv, "{name}\t{}\t{}", bytes.len(), cols.join("\t")).expect("write TSV");
        if let [version, _, strict_result, diags, rest @ ..] = cols.as_slice() {
            *versions.entry(version.clone()).or_default() += 1;
            *strict_results.entry(strict_result.clone()).or_default() += 1;
            let mut seen: Vec<&str> = diags.split(" | ").filter(|d| !d.is_empty()).collect();
            seen.sort_unstable();
            seen.dedup();
            for d in seen {
                *diagnostics.entry(d.to_owned()).or_default() += 1;
            }
            for (counts, outcome) in round_trips.iter_mut().zip(rest) {
                *counts.entry(outcome.clone()).or_default() += 1;
            }
        }
    }
    tsv.flush().expect("write TSV");

    let print = |title: &str, counts: &BTreeMap<String, usize>| {
        println!("\n{title}:");
        let mut sorted: Vec<_> = counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (key, count) in sorted {
            let key = if key.is_empty() { "(none)" } else { key };
            println!("{count:>8}  {key}");
        }
    };
    println!("{} files, {panics} panics", files.len());
    print("Version", &versions);
    print("Strict", &strict_results);
    print("Lenient diagnostics (files)", &diagnostics);
    print("MTEF -> MTEF", &round_trips[0]);
    print("MTEF -> LaTeX -> MathML", &round_trips[1]);
    print("MTEF -> OMML -> MathML", &round_trips[2]);
    println!("\nwrote {tsv_path}");
    if panics > 0 {
        std::process::exit(1);
    }
}
