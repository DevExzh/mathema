#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::validate_compact_depth_with;
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
        // `from_latex` and `convert` refuse a tree that nests too deep, before
        // they normalize: normalization goes no deeper, so what it leaves
        // below would be normalized in the second pass only.
        let limit = config.effective_max_depth();
        if validate_compact_depth_with(&r1.expr.arena, r1.expr.roots, limit).is_err() {
            return;
        }
        if normalize_compact_expr(&mut r1.expr).is_err() {
            return;
        }
        let once = format!("{:?}", r1.expr.to_math_expr().nodes);
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
        if normalize_compact_expr(&mut r2.expr).is_err() {
            return;
        }
        let Ok(out2) = LatexWriter::write_compact(&r2.expr, &config) else {
            panic!(
                "round-trip failed: second write panicked.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}"
            );
        };

        // U+2061 has an argument in the tree and none in LaTeX, where the
        // argument is a group only in the first output: what is read back
        // settles with the second.
        if once.contains("u{2061}") {
            let Ok(mut r3) = LatexReader::parse_compact(&out2, &config) else {
                panic!(
                    "round-trip failed: second write output could not be re-parsed.\n\
                     input:  {input:?}\n\
                     write2: {out2:?}"
                );
            };
            if normalize_compact_expr(&mut r3.expr).is_err() {
                return;
            }
            let out3 = LatexWriter::write_compact(&r3.expr, &config);
            assert_eq!(
                Some(&out2),
                out3.as_ref().ok(),
                "LaTeX round-trip does not settle.\ninput: {input:?}\nwrite1: {out1:?}"
            );
        } else {
            // Idempotency check
            assert_eq!(
                out1, out2,
                "LaTeX round-trip not idempotent.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}\n\
                 write2: {out2:?}"
            );
        }

        // The public conversion drops the colors LaTeX cannot hold before it
        // normalizes.
        if let Ok(out1) = mathema::convert(input, mathema::Format::LaTeX, mathema::Format::LaTeX) {
            let out2 = mathema::convert(&out1, mathema::Format::LaTeX, mathema::Format::LaTeX);
            assert_eq!(
                out2.as_ref().ok(),
                Some(&out1),
                "LaTeX conversion not idempotent.\ninput: {input:?}\nerror: {:?}",
                out2.as_ref().err()
            );
        }
    }
});
