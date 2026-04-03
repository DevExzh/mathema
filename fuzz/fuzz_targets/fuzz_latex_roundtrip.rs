#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::ast::normalize::normalize_compact_expr;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let config = Config::default();

        // First pass: parse → normalize → write
        let Ok(mut r1) = LatexReader::parse_compact(input, &config) else {
            return;
        };
        normalize_compact_expr(&mut r1.expr);
        let Ok(out1) = LatexWriter::write_compact(&r1.expr, &config) else {
            return;
        };

        // Second pass: parse → normalize → write
        let Ok(mut r2) = LatexReader::parse_compact(&out1, &config) else {
            panic!(
                "round-trip failed: first write output could not be re-parsed.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}"
            );
        };
        normalize_compact_expr(&mut r2.expr);
        let Ok(out2) = LatexWriter::write_compact(&r2.expr, &config) else {
            panic!(
                "round-trip failed: second write panicked.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}"
            );
        };

        // Idempotency check
        assert_eq!(
            out1, out2,
            "LaTeX round-trip not idempotent.\n\
             input:  {input:?}\n\
             write1: {out1:?}\n\
             write2: {out2:?}"
        );
    }
});
