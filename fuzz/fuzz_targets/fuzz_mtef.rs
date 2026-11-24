#![no_main]
//! MTEF reader fuzz target.
//!
//! MTEF is a binary format, so the fuzzer feeds raw bytes (bare MTEF or an
//! `Equation Native` stream) to the byte entry point. For every input:
//!
//! - Strict and Lenient parsing must not panic.
//! - A decoded equation must convert to LaTeX, MathML and OMML without
//!   panicking.
//! - It must be writable as MTEF 5 and MTEF 3 (unless it holds content MTEF
//!   cannot express, or exceeds the configured limits), the output must read
//!   back in Strict mode, and from the second pass on the MTEF → MTEF round
//!   trip must be a fixed point: writing what was read back gives the same
//!   bytes again. The one exemption is a second write that reports the loss
//!   of a math atom class: the reader builds a bare operator atom the writer
//!   only keeps as its content.
//! - The un-normalized AST must be writable without panicking.
use libfuzzer_sys::fuzz_target;
use mathema::reader::mtef::MtefReader;
use mathema::writer::Writer;
use mathema::writer::mtef::MtefWriter;
use mathema::{
    Config, ConvertResult, Error, MtefVersion, MtefWriteOptions, Strictness, from_mtef_bytes_with,
};

/// Write `result` as MTEF, and whether no math atom class was lost;
/// `None` when the content or the limits rule it out.
fn write(result: &ConvertResult, options: &MtefWriteOptions) -> Option<(Vec<u8>, bool)> {
    match result.to_mtef_bytes_with_diagnostics(options) {
        Ok((bytes, diagnostics)) => {
            let keeps_atoms = !diagnostics
                .iter()
                .any(|d| d.message.contains("math atom classes"));
            Some((bytes, keeps_atoms))
        },
        // Output that would not read back for a reason other than the
        // caller's limits is a writer defect.
        Err(err @ Error::MtefWrite { .. }) => panic!("{err}"),
        // Unrepresentable content, the caller's limits.
        Err(_) => None,
    }
}

/// Read writer output back; it must always decode in Strict mode.
fn read_back(bytes: &[u8], strict: &Config, pass: u8) -> ConvertResult {
    from_mtef_bytes_with(bytes, strict)
        .unwrap_or_else(|e| panic!("pass {pass}: writer output does not read back: {e}"))
}

fn round_trip(result: &ConvertResult, version: MtefVersion, strict: &Config) {
    let options = MtefWriteOptions::new(version);
    let Some((first, _)) = write(result, &options) else {
        return;
    };
    let reread = read_back(&first, strict, 1);
    let Some((second, keeps_atoms)) = write(&reread, &options) else {
        panic!("{version:?}: writer output read back but cannot be written again");
    };
    let rereread = read_back(&second, strict, 2);
    if !keeps_atoms {
        return;
    }
    let Some((third, _)) = write(&rereread, &options) else {
        panic!("{version:?}: second-pass output cannot be written again");
    };
    assert_eq!(
        second, third,
        "{version:?}: MTEF round trip is not idempotent from the second pass"
    );
}

fn exercise(result: &ConvertResult, strict: &Config) {
    let _ = result.to_latex();
    let _ = result.to_mathml();
    let _ = result.to_omml();
    for version in [MtefVersion::V5, MtefVersion::V3] {
        round_trip(result, version, strict);
    }
}

fuzz_target!(|data: &[u8]| {
    let strict = Config::default().with_strictness(Strictness::Strict);
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    if let Ok(parsed) = MtefReader::parse_bytes(data, &strict) {
        let _ = MtefWriter::write(&parsed.expr, &strict);
    }
    let strict_result = from_mtef_bytes_with(data, &strict);
    if let Ok(result) = &strict_result {
        exercise(result, &strict);
    }
    if let Ok(result) = from_mtef_bytes_with(data, &lenient) {
        // A Strict success decodes the same equation in Lenient mode; only
        // exercise the Lenient result when it can differ.
        if strict_result.is_err() {
            exercise(&result, &strict);
        }
    }
});
