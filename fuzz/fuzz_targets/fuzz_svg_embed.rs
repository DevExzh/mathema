#![no_main]

use libfuzzer_sys::fuzz_target;
use mathema::render::font_embed::{extract_glyph, parse_font_info};

static FONT_DATA: &[u8] = include_bytes!("../../src/render/fonts/LatinModernMath-Regular.otf");

fuzz_target!(|data: &[u8]| {
    // Fuzz 1: random bytes as font data — should never panic
    let _ = parse_font_info(data);

    // Fuzz 2: random chars against the real font — should never panic
    // Interpret first 4 bytes of fuzz input as a Unicode codepoint
    if data.len() >= 4 {
        let cp = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if let Some(ch) = char::from_u32(cp) {
            let _ = extract_glyph(FONT_DATA, ch);
        }
    }

    // Fuzz 3: random bytes as font data with a valid char
    let _ = extract_glyph(data, 'A');
});
