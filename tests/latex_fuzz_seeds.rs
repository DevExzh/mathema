//! The committed fuzzing seeds and dictionary cover the reader. (Both are left
//! out of the published crate, where these tests have nothing to check.)

use std::path::Path;

fn fuzz(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fuzz")
        .join(path)
}

fn seeds(format: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(fuzz("seeds").join(format)) else {
        return Vec::new();
    };
    let mut seeds: Vec<_> = entries
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&path).unwrap())
        })
        .collect();
    seeds.sort();
    seeds
}

#[test]
fn seeds_are_read_and_stable() {
    use mathema::Format::LaTeX;
    for (name, source) in seeds("latex") {
        let first = mathema::convert(&source, LaTeX, LaTeX)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let second = mathema::convert(&first, LaTeX, LaTeX)
            .unwrap_or_else(|error| panic!("{name}: {error}: {first}"));
        assert_eq!(second, first, "{name}");
    }
}

/// The seeds of other formats are read, and written as LaTeX with no control
/// word that the writer did not write (it checks that itself in this build).
#[test]
fn seeds_of_other_formats_are_written_as_latex() {
    for (format, from) in [
        ("mathml", mathema::Format::MathML),
        ("omml", mathema::Format::Omml),
    ] {
        for (name, source) in seeds(format) {
            let read = match from {
                mathema::Format::MathML => mathema::from_mathml(&source),
                _ => mathema::from_omml(&source),
            };
            read.and_then(|read| read.to_latex())
                .unwrap_or_else(|error| panic!("{format} {name}: {error}"));
            mathema::convert(&source, from, mathema::Format::LaTeX)
                .unwrap_or_else(|error| panic!("{format} {name}: {error}"));
        }
    }
}

/// Every environment of the dictionary is in a seed.
#[test]
fn seeds_hold_every_environment() {
    let Ok(dictionary) = std::fs::read_to_string(fuzz("dict/latex.dict")) else {
        return;
    };
    let seeds: String = seeds("latex")
        .into_iter()
        .map(|(_, source)| source)
        .collect();
    let mut environments = 0;
    for line in dictionary.lines() {
        if let Some(name) = line
            .strip_prefix(r#""\\begin{"#)
            .and_then(|rest| rest.strip_suffix(r#"}""#))
        {
            environments += 1;
            assert!(seeds.contains(&format!(r"\begin{{{name}}}")), "{name}");
        }
    }
    assert!(environments > 40);
}
