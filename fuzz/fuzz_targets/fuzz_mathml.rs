#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = MathmlReader::parse(input, &config) {
            let _ = MathmlWriter::write(&parsed.expr, &config);
        }
    }
});
