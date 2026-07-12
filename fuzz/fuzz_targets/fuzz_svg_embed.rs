#![no_main]

use libfuzzer_sys::fuzz_target;
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::{RenderOptions, render_to_svg_standalone_compact};

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(parsed) = LatexReader::parse_compact(input, &Config::default()) else {
        return;
    };
    let _ = render_to_svg_standalone_compact(&parsed.expr, &RenderOptions::default());
});
