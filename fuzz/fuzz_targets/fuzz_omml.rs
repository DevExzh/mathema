#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::omml::OmmlReader;
use mathema::writer::Writer;
use mathema::writer::omml::OmmlWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = OmmlReader::parse(input, &config) {
            let _ = OmmlWriter::write(&parsed.expr, &config);
        }
    }
});
