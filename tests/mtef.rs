//! MTEF reader tests on real equations: goldens for the fixtures in
//! `tests/fixtures/mtef/`, hostile variants of them, and an optional corpus run.
//!
//! Corpus run: set `MATHEMA_MTEF_CORPUS` to a directory of `.bin` files, each
//! holding one equation (bare MTEF or an `Equation Native` stream), and run
//! `cargo test --test mtef corpus -- --nocapture`. Every file must decode in
//! Strict mode to a non-empty equation.

use std::path::{Path, PathBuf};

use mathema::{
    Config, DiagnosticKind, Error, MtefVersion, Strictness, from_mtef_bytes, from_mtef_bytes_with,
};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mtef")
}

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect()
}

fn goldens() -> Vec<(String, String)> {
    std::fs::read_to_string(fixture_dir().join("golden.txt"))
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, latex) = line
                .split_once(": ")
                .expect("golden lines are `name: latex`");
            (name.to_owned(), latex.to_owned())
        })
        .collect()
}

#[test]
fn fixtures_match_their_goldens() {
    let goldens = goldens();
    let fixtures = fixtures();
    assert_eq!(
        goldens.len(),
        fixtures.len(),
        "every fixture needs exactly one golden"
    );
    for ((name, bytes), (golden_name, golden)) in fixtures.iter().zip(&goldens) {
        assert_eq!(
            name, golden_name,
            "goldens must be listed in file-name order"
        );
        let result = from_mtef_bytes(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let warnings: Vec<_> = result
            .diagnostics()
            .iter()
            .filter(|d| d.kind == DiagnosticKind::Warning)
            .collect();
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
        assert_eq!(&result.to_latex().unwrap(), golden, "{name}");
    }
}

#[test]
fn fixtures_report_their_version_and_header() {
    for (name, bytes) in fixtures() {
        let result = from_mtef_bytes(&bytes).unwrap();
        let info = result.mtef_info().unwrap();
        let expected = if name.contains("v2-") {
            MtefVersion::V2
        } else if name.contains("v3-") {
            MtefVersion::V3
        } else if name.contains("v4-") {
            MtefVersion::V4
        } else {
            MtefVersion::V5
        };
        assert_eq!(info.version, expected, "{name}");
        // OLE streams start with cbHdr = 28.
        assert_eq!(info.ole_header.is_some(), bytes[0] == 28, "{name}");
    }
}

#[test]
fn hex_entry_point_agrees_with_the_binary_one() {
    for (name, bytes) in fixtures() {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let from_hex = mathema::from_mtef(&hex).unwrap().to_latex().unwrap();
        let from_bytes = from_mtef_bytes(&bytes).unwrap().to_latex().unwrap();
        assert_eq!(from_hex, from_bytes, "{name}");
    }
}

#[test]
fn truncated_fixtures_are_refused_without_panicking() {
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    for (name, bytes) in fixtures() {
        // Zero padding after the final END is allowed, so cut before it.
        let content_end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        for len in 1..content_end {
            let prefix = &bytes[..len];
            assert!(
                from_mtef_bytes(prefix).is_err(),
                "{name}: prefix of {len} bytes accepted"
            );
            let recovered = from_mtef_bytes_with(prefix, &lenient).unwrap();
            assert!(
                recovered
                    .diagnostics()
                    .iter()
                    .any(|d| d.kind == DiagnosticKind::Warning),
                "{name}: prefix of {len} bytes recovered silently"
            );
        }
    }
}

#[test]
fn corrupted_fixtures_never_panic() {
    // Overwrite each byte with values that are record tags in one version or
    // the other; decoding must end in a result or an error, never a panic.
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    for (_, bytes) in fixtures().into_iter().filter(|(_, b)| b.len() < 400) {
        for i in 0..bytes.len() {
            for value in [0x00, 0x03, 0x05, 0x0F, 0x13, 0x64, 0x80, 0xFF] {
                let mut corrupted = bytes.clone();
                corrupted[i] = value;
                let _ = from_mtef_bytes(&corrupted);
                let _ = from_mtef_bytes_with(&corrupted, &lenient);
            }
        }
    }
}

#[test]
fn unknown_record_types_in_real_equations_are_refused() {
    // Replace the final END of an MTEF 5 fixture's line with an unassigned
    // record type: Strict mode must name it, Lenient mode must warn.
    let bytes = std::fs::read(fixture_dir().join("v5-plus-minus.bin")).unwrap();
    let mut corrupted = bytes.clone();
    let end = corrupted.len() - 2;
    assert_eq!(&corrupted[end..], &[0, 0]);
    corrupted[end] = 42;
    match from_mtef_bytes(&corrupted) {
        Err(Error::MtefParse { message }) => {
            assert!(message.contains("record type 42"), "{message}")
        },
        other => panic!(
            "expected a parse error, got {:?}",
            other.map(|r| r.to_latex())
        ),
    }
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let recovered = from_mtef_bytes_with(&corrupted, &lenient).unwrap();
    assert!(
        recovered
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("record type 42"))
    );
}

#[test]
fn matrix_bomb_in_a_real_equation_stays_small() {
    // Give the matrix of a real equation 255 x 255 declared cells: Strict mode
    // refuses it, Lenient mode keeps only the cells that are present.
    let mut bytes = std::fs::read(fixture_dir().join("v3-brace-matrix.bin")).unwrap();
    // MTEF 3 MATRIX: tag 0x05, valign, h_just, v_just, rows, cols.
    let at = bytes.windows(4).position(|w| w == [0x05, 1, 1, 1]).unwrap();
    bytes[at + 4] = 255;
    bytes[at + 5] = 255;
    // Partition lists grow with the declared size: 64 bytes each.
    bytes.splice(at + 6..at + 8, std::iter::repeat_n(0u8, 128));
    assert!(from_mtef_bytes(&bytes).is_err());
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let result = from_mtef_bytes_with(&bytes, &lenient).unwrap();
    assert!(result.compact_expr().arena.node_count() < 200);
}

#[test]
fn equation_native_buffer_slack_is_noted_not_refused() {
    // Equation Editor and MathType declare the whole buffer the equation was
    // written into; the bytes after the final END are leftovers.
    for name in ["govdocs-v3-buffer-slack", "govdocs-v5-buffer-slack"] {
        let bytes = std::fs::read(fixture_dir().join(format!("{name}.bin"))).unwrap();
        let result = from_mtef_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind == DiagnosticKind::Info && d.message.contains("slack")),
            "{name}: {:?}",
            result.diagnostics()
        );
    }
    // The same bytes after bare MTEF data are still refused.
    let bytes = std::fs::read(fixture_dir().join("govdocs-v3-buffer-slack.bin")).unwrap();
    assert!(from_mtef_bytes(&bytes[28..]).is_err());
}

#[test]
fn mathtype_alphabets_and_spaces_decode_without_private_use_or_replacement_characters() {
    for name in [
        "govdocs-v5-bare-space-codes",
        "govdocs-v5-fraktur",
        "govdocs-v5-script",
        "govdocs-v5-symbol-arrow-extension",
    ] {
        let bytes = std::fs::read(fixture_dir().join(format!("{name}.bin"))).unwrap();
        let mathml = from_mtef_bytes(&bytes).unwrap().to_mathml().unwrap();
        assert!(
            !mathml
                .chars()
                .any(|c| c == '\u{FFFD}' || ('\u{E000}'..='\u{F8FF}').contains(&c)),
            "{name}: {mathml}"
        );
    }
}

/// Decode every `.bin` file of the directory named by `MATHEMA_MTEF_CORPUS`
/// in Strict mode; none may fail or come out empty.
#[test]
fn corpus() {
    let Some(dir) = std::env::var_os("MATHEMA_MTEF_CORPUS") else {
        eprintln!("MATHEMA_MTEF_CORPUS is not set; skipping the MTEF corpus run");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", Path::new(&dir).display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no .bin files in {}",
        Path::new(&dir).display()
    );
    let mut failures = Vec::new();
    let mut versions = [0usize; 6];
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        match from_mtef_bytes(&bytes) {
            Ok(result) if result.compact_expr().roots.is_empty() => {
                failures.push(format!("{}: empty result", path.display()));
            },
            Ok(result) => {
                let version = result.mtef_info().map_or(0, |i| i.version.number());
                versions[usize::from(version).min(5)] += 1;
            },
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    eprintln!(
        "MTEF corpus: {} files, {} MTEF 2, {} MTEF 3, {} MTEF 4, {} MTEF 5, {} failures",
        files.len(),
        versions[2],
        versions[3],
        versions[4],
        versions[5],
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
