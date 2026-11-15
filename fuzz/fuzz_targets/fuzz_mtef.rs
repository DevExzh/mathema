#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::{Config, Strictness};
use mathema::reader::mtef::MtefReader;
use mathema::writer::Writer;
use mathema::writer::mtef::MtefWriter;

// MTEF is a binary format, so feed the fuzzer raw bytes directly through the
// `parse_bytes` entry point instead of requiring valid UTF-8 (which previously
// confined the fuzzer almost entirely to the hex-decode error path).
fuzz_target!(|data: &[u8]| {
    let config = Config::default();
    if let Ok(parsed) = MtefReader::parse_bytes(data, &config) {
        let _ = MtefWriter::write(&parsed.expr, &config);
    }
    // The public entry point adds normalization; Lenient mode reaches the
    // recovery paths.
    let lenient = config.with_strictness(Strictness::Lenient);
    if let Ok(result) = mathema::from_mtef_bytes_with(data, &lenient) {
        let _ = result.to_latex();
    }
});
