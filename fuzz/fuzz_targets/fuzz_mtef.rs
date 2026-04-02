#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::mtef::MtefReader;
use mathema::writer::Writer;
use mathema::writer::mtef::MtefWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = MtefReader::parse(input, &config) {
            let _ = MtefWriter::write(&parsed.expr, &config);
        }
    }
});
