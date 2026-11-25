#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;

#[path = "../support/latex_output.rs"]
mod latex_output;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();
        if let Ok(parsed) = MathmlReader::parse_compact(input, &config) {
            if let Ok(xml) = MathmlWriter::write_compact(&parsed.expr, &config) {
                let _ = MathmlReader::parse_compact(&xml, &config);
            }
            let _ = MathmlWriter::write(&parsed.expr.to_math_expr(), &config);
            latex_output::write(parsed.expr, &config);
        }
    }
});
