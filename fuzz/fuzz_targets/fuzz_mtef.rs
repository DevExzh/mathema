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
        // Whatever was decoded must be writable as MTEF 5 and MTEF 3, and the
        // writer's output must read back in Strict mode.
        for version in [mathema::MtefVersion::V5, mathema::MtefVersion::V3] {
            let options = mathema::MtefWriteOptions::new(version);
            match result.to_mtef_bytes(&options) {
                Ok(bytes) => assert!(mathema::from_mtef_bytes(&bytes).is_ok()),
                // Output that would not read back for a reason other than the
                // caller's limits is a writer defect.
                Err(err @ mathema::Error::MtefWrite { .. }) => panic!("{err}"),
                // The caller's limits, unrepresentable content.
                Err(_) => {},
            }
        }
    }
});
