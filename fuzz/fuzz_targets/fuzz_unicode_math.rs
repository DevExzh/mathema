#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::unicode_math::UnicodeMathReader;
use mathema::writer::Writer;
use mathema::writer::unicode_math::UnicodeMathWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = UnicodeMathReader::parse(input, &config) {
            let _ = UnicodeMathWriter::write(&parsed.expr, &config);
        }
    }
});
