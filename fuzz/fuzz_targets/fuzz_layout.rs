#![no_main]
use libfuzzer_sys::fuzz_target;
use mathema::reader::Reader;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Skip very long inputs to avoid timeouts
        if s.len() > 1024 {
            return;
        }
        let config = mathema::error::Config::default();
        if let Ok(result) = mathema::reader::latex::LatexReader::parse_compact(s, &config) {
            let options = mathema::render::RenderOptions::default();
            let mut arena = mathema::render::arena::LayoutArena::new();
            let builder = mathema::render::layout::LayoutBuilder::new(
                mathema::render::style::MathStyle::Display,
                &options,
            );
            // Must not panic or stack overflow
            let root = builder.layout(&mut arena, &result.expr);
            // Also verify emit doesn't panic
            let _svg = mathema::render::emit::emit_svg_arena(&arena, root, &options);
        }
    }
});
