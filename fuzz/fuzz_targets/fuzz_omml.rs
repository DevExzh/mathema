#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::omml::OmmlReader;
use mathema::writer::Writer;
use mathema::writer::omml::OmmlWriter;

#[path = "../support/latex_output.rs"]
mod latex_output;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = OmmlReader::parse(input, &config) {
            let _ = OmmlWriter::write(&parsed.expr, &config);
        }
        if let Ok(parsed) = OmmlReader::parse_compact(input, &config) {
            latex_output::write(parsed.expr, &config);
        }
    }
});
