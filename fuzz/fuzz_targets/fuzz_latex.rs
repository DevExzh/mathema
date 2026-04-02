#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = LatexReader::parse(input, &config) {
            // If parsing succeeds, writing must not panic
            let _ = LatexWriter::write(&parsed.expr, &config);
        }
    }
});
