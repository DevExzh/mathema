//! All 148 CSS named colors (CSS Color Level 4).
//!
//! Uses a phf perfect-hash map for O(1) lookup by lowercase name.

use phf::phf_map;

/// Look up a CSS named color (case-insensitive). Returns RGB bytes.
pub fn lookup_css_color(name: &str) -> Option<[u8; 3]> {
    let mut lower = [0u8; 32];
    let name_bytes = name.as_bytes();
    if name_bytes.len() > 32 {
        return None;
    }
    for (i, &b) in name_bytes.iter().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    let key = std::str::from_utf8(&lower[..name_bytes.len()]).ok()?;
    CSS_COLORS.get(key).copied()
}

/// Format RGB bytes as a lowercase `#rrggbb` hex string.
pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Resolve a bare color reference (named CSS color, `#rgb`/`#rrggbb` hex, or
/// `rgb(r,g,b)` function) into RGB bytes. Returns `None` if it is not a
/// recognizable color reference.
pub fn resolve_named_or_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(rgb) = parse_rgb_func(s) {
        return Some(rgb);
    }
    lookup_css_color(s)
}

/// Resolve a color model + spec (as in `\definecolor`/`\color[model]{spec}`)
/// into RGB bytes. Supported models: `rgb` (0..1), `RGB` (0..255),
/// `HTML` (`RRGGBB` hex), `gray`/`Gray` (0..1), and `cmyk` (0..1 each).
/// Returns `None` on malformed input.
pub fn resolve_color_model(model: &str, spec: &str) -> Option<[u8; 3]> {
    let spec = spec.trim();
    match model {
        "rgb" => {
            let v = parse_floats::<3>(spec)?;
            Some([f01(v[0]), f01(v[1]), f01(v[2])])
        },
        "RGB" => {
            let v = parse_floats::<3>(spec)?;
            Some([clamp255(v[0]), clamp255(v[1]), clamp255(v[2])])
        },
        "HTML" => {
            let h = spec.strip_prefix('#').unwrap_or(spec);
            if h.len() != 6 {
                return None;
            }
            parse_hex(h)
        },
        "gray" | "Gray" => {
            let v = parse_floats::<1>(spec)?;
            let g = f01(v[0]);
            Some([g, g, g])
        },
        "cmyk" => {
            let v = parse_floats::<4>(spec)?;
            let k = v[3].clamp(0.0, 1.0);
            let conv = |x: f64| (255.0 * (1.0 - x.clamp(0.0, 1.0)) * (1.0 - k)).round() as u8;
            Some([conv(v[0]), conv(v[1]), conv(v[2])])
        },
        _ => None,
    }
}

/// Blend two colors: `pct_a`% of `a` mixed with `(100 - pct_a)`% of `b`
/// (xcolor `a!pct_a!b` semantics).
pub fn mix_rgb(a: [u8; 3], b: [u8; 3], pct_a: f64) -> [u8; 3] {
    let t = pct_a.clamp(0.0, 100.0) / 100.0;
    let blend = |x: u8, y: u8| (f64::from(x) * t + f64::from(y) * (1.0 - t)).round() as u8;
    [
        blend(a[0], b[0]),
        blend(a[1], b[1]),
        blend(a[2], b[2]),
    ]
}

/// Parse exactly `N` comma-separated floats from `spec`.
fn parse_floats<const N: usize>(spec: &str) -> Option<[f64; N]> {
    let mut out = [0.0f64; N];
    let mut parts = spec.split(',');
    for slot in out.iter_mut() {
        *slot = parts.next()?.trim().parse::<f64>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// Map a 0.0..=1.0 float to a 0..=255 byte.
fn f01(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Clamp a 0..=255 float to a byte.
fn clamp255(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// Parse a 3- or 6-digit hex color body (no leading `#`).
fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.trim();
    match hex.len() {
        6 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ]),
        3 => {
            let nib = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok();
            let r = nib(0)?;
            let g = nib(1)?;
            let b = nib(2)?;
            Some([r * 17, g * 17, b * 17])
        },
        _ => None,
    }
}

/// Parse `rgb(r,g,b)` (0..255 or percentages). Returns `None` if not such a
/// function form.
fn parse_rgb_func(s: &str) -> Option<[u8; 3]> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open || !s[..open].trim().eq_ignore_ascii_case("rgb") {
        return None;
    }
    let inner = &s[open + 1..close];
    let mut parts = inner.split(',');
    let mut comp = || -> Option<u8> {
        let p = parts.next()?.trim();
        if let Some(pc) = p.strip_suffix('%') {
            let pct = pc.trim().parse::<f64>().ok()?.clamp(0.0, 100.0);
            Some((pct * 255.0 / 100.0).round() as u8)
        } else {
            Some(clamp255(p.parse::<f64>().ok()?))
        }
    };
    let r = comp()?;
    let g = comp()?;
    let b = comp()?;
    if parts.next().is_some() {
        return None;
    }
    Some([r, g, b])
}

static CSS_COLORS: phf::Map<&'static str, [u8; 3]> = phf_map! {
    "aliceblue" => [240, 248, 255],
    "antiquewhite" => [250, 235, 215],
    "aqua" => [0, 255, 255],
    "aquamarine" => [127, 255, 212],
    "azure" => [240, 255, 255],
    "beige" => [245, 245, 220],
    "bisque" => [255, 228, 196],
    "black" => [0, 0, 0],
    "blanchedalmond" => [255, 235, 205],
    "blue" => [0, 0, 255],
    "blueviolet" => [138, 43, 226],
    "brown" => [165, 42, 42],
    "burlywood" => [222, 184, 135],
    "cadetblue" => [95, 158, 160],
    "chartreuse" => [127, 255, 0],
    "chocolate" => [210, 105, 30],
    "coral" => [255, 127, 80],
    "cornflowerblue" => [100, 149, 237],
    "cornsilk" => [255, 248, 220],
    "crimson" => [220, 20, 60],
    "cyan" => [0, 255, 255],
    "darkblue" => [0, 0, 139],
    "darkcyan" => [0, 139, 139],
    "darkgoldenrod" => [184, 134, 11],
    "darkgray" => [169, 169, 169],
    "darkgreen" => [0, 100, 0],
    "darkgrey" => [169, 169, 169],
    "darkkhaki" => [189, 183, 107],
    "darkmagenta" => [139, 0, 139],
    "darkolivegreen" => [85, 107, 47],
    "darkorange" => [255, 140, 0],
    "darkorchid" => [153, 50, 204],
    "darkred" => [139, 0, 0],
    "darksalmon" => [233, 150, 122],
    "darkseagreen" => [143, 188, 143],
    "darkslateblue" => [72, 61, 139],
    "darkslategray" => [47, 79, 79],
    "darkslategrey" => [47, 79, 79],
    "darkturquoise" => [0, 206, 209],
    "darkviolet" => [148, 0, 211],
    "deeppink" => [255, 20, 147],
    "deepskyblue" => [0, 191, 255],
    "dimgray" => [105, 105, 105],
    "dimgrey" => [105, 105, 105],
    "dodgerblue" => [30, 144, 255],
    "firebrick" => [178, 34, 34],
    "floralwhite" => [255, 250, 240],
    "forestgreen" => [34, 139, 34],
    "fuchsia" => [255, 0, 255],
    "gainsboro" => [220, 220, 220],
    "ghostwhite" => [248, 248, 255],
    "gold" => [255, 215, 0],
    "goldenrod" => [218, 165, 32],
    "gray" => [128, 128, 128],
    "green" => [0, 128, 0],
    "greenyellow" => [173, 255, 47],
    "grey" => [128, 128, 128],
    "honeydew" => [240, 255, 240],
    "hotpink" => [255, 105, 180],
    "indianred" => [205, 92, 92],
    "indigo" => [75, 0, 130],
    "ivory" => [255, 255, 240],
    "khaki" => [240, 230, 140],
    "lavender" => [230, 230, 250],
    "lavenderblush" => [255, 240, 245],
    "lawngreen" => [124, 252, 0],
    "lemonchiffon" => [255, 250, 205],
    "lightblue" => [173, 216, 230],
    "lightcoral" => [240, 128, 128],
    "lightcyan" => [224, 255, 255],
    "lightgoldenrodyellow" => [250, 250, 210],
    "lightgray" => [211, 211, 211],
    "lightgreen" => [144, 238, 144],
    "lightgrey" => [211, 211, 211],
    "lightpink" => [255, 182, 193],
    "lightsalmon" => [255, 160, 122],
    "lightseagreen" => [32, 178, 170],
    "lightskyblue" => [135, 206, 250],
    "lightslategray" => [119, 136, 153],
    "lightslategrey" => [119, 136, 153],
    "lightsteelblue" => [176, 196, 222],
    "lightyellow" => [255, 255, 224],
    "lime" => [0, 255, 0],
    "limegreen" => [50, 205, 50],
    "linen" => [250, 240, 230],
    "magenta" => [255, 0, 255],
    "maroon" => [128, 0, 0],
    "mediumaquamarine" => [102, 205, 170],
    "mediumblue" => [0, 0, 205],
    "mediumorchid" => [186, 85, 211],
    "mediumpurple" => [147, 111, 219],
    "mediumseagreen" => [60, 179, 113],
    "mediumslateblue" => [123, 104, 238],
    "mediumspringgreen" => [0, 250, 154],
    "mediumturquoise" => [72, 209, 204],
    "mediumvioletred" => [199, 21, 133],
    "midnightblue" => [25, 25, 112],
    "mintcream" => [245, 255, 250],
    "mistyrose" => [255, 228, 225],
    "moccasin" => [255, 228, 181],
    "navajowhite" => [255, 222, 173],
    "navy" => [0, 0, 128],
    "oldlace" => [253, 245, 230],
    "olive" => [128, 128, 0],
    "olivedrab" => [107, 142, 35],
    "orange" => [255, 165, 0],
    "orangered" => [255, 69, 0],
    "orchid" => [218, 112, 214],
    "palegoldenrod" => [238, 232, 170],
    "palegreen" => [152, 251, 152],
    "paleturquoise" => [175, 238, 238],
    "palevioletred" => [219, 112, 147],
    "papayawhip" => [255, 239, 213],
    "peachpuff" => [255, 218, 185],
    "peru" => [205, 133, 63],
    "pink" => [255, 192, 203],
    "plum" => [221, 160, 221],
    "powderblue" => [176, 224, 230],
    "purple" => [128, 0, 128],
    "rebeccapurple" => [102, 51, 153],
    "red" => [255, 0, 0],
    "rosybrown" => [188, 143, 143],
    "royalblue" => [65, 105, 225],
    "saddlebrown" => [139, 69, 19],
    "salmon" => [250, 128, 114],
    "sandybrown" => [244, 164, 96],
    "seagreen" => [46, 139, 87],
    "seashell" => [255, 245, 238],
    "sienna" => [160, 82, 45],
    "silver" => [192, 192, 192],
    "skyblue" => [135, 206, 235],
    "slateblue" => [106, 90, 205],
    "slategray" => [112, 128, 144],
    "slategrey" => [112, 128, 144],
    "snow" => [255, 250, 250],
    "springgreen" => [0, 255, 127],
    "steelblue" => [70, 130, 180],
    "tan" => [210, 180, 140],
    "teal" => [0, 128, 128],
    "thistle" => [216, 191, 216],
    "tomato" => [255, 99, 71],
    "turquoise" => [64, 224, 208],
    "violet" => [238, 130, 238],
    "wheat" => [245, 222, 179],
    "white" => [255, 255, 255],
    "whitesmoke" => [245, 245, 245],
    "yellow" => [255, 255, 0],
    "yellowgreen" => [154, 205, 50],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_colors() {
        assert_eq!(lookup_css_color("red"), Some([255, 0, 0]));
        assert_eq!(lookup_css_color("blue"), Some([0, 0, 255]));
        assert_eq!(lookup_css_color("white"), Some([255, 255, 255]));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(lookup_css_color("DarkSlateBlue"), Some([72, 61, 139]));
        assert_eq!(lookup_css_color("DARKSLATEBLUE"), Some([72, 61, 139]));
        assert_eq!(lookup_css_color("darkslateblue"), Some([72, 61, 139]));
    }

    #[test]
    fn extended_colors() {
        assert_eq!(lookup_css_color("coral"), Some([255, 127, 80]));
        assert_eq!(lookup_css_color("tomato"), Some([255, 99, 71]));
        assert_eq!(lookup_css_color("rebeccapurple"), Some([102, 51, 153]));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(lookup_css_color("notacolor"), None);
        assert_eq!(lookup_css_color(""), None);
    }

    #[test]
    fn grey_variants() {
        assert_eq!(lookup_css_color("gray"), lookup_css_color("grey"));
        assert_eq!(lookup_css_color("darkgray"), lookup_css_color("darkgrey"));
    }

    #[test]
    fn hex_formatting() {
        assert_eq!(rgb_to_hex([255, 0, 0]), "#ff0000");
        assert_eq!(rgb_to_hex([0, 128, 255]), "#0080ff");
    }

    #[test]
    fn color_models() {
        assert_eq!(resolve_color_model("rgb", "1,0,0"), Some([255, 0, 0]));
        assert_eq!(resolve_color_model("RGB", "0,255,0"), Some([0, 255, 0]));
        assert_eq!(resolve_color_model("HTML", "0000FF"), Some([0, 0, 255]));
        assert_eq!(resolve_color_model("gray", "0.5"), Some([128, 128, 128]));
        assert_eq!(resolve_color_model("cmyk", "0,1,1,0"), Some([255, 0, 0]));
        // Malformed specs recover to None.
        assert_eq!(resolve_color_model("rgb", "1,0"), None);
        assert_eq!(resolve_color_model("HTML", "ZZZ"), None);
        assert_eq!(resolve_color_model("unknown", "1,0,0"), None);
    }

    #[test]
    fn named_or_hex_resolution() {
        assert_eq!(resolve_named_or_hex("red"), Some([255, 0, 0]));
        assert_eq!(resolve_named_or_hex("#00ff00"), Some([0, 255, 0]));
        assert_eq!(resolve_named_or_hex("#0f0"), Some([0, 255, 0]));
        assert_eq!(resolve_named_or_hex("rgb(255,0,0)"), Some([255, 0, 0]));
        assert_eq!(resolve_named_or_hex("nope"), None);
    }

    #[test]
    fn mixing() {
        // red!50!blue → halfway between red and blue.
        assert_eq!(mix_rgb([255, 0, 0], [0, 0, 255], 50.0), [128, 0, 128]);
        // red!100!blue → all red.
        assert_eq!(mix_rgb([255, 0, 0], [0, 0, 255], 100.0), [255, 0, 0]);
    }
}
