//! sRGB paint values for the raster backends.

use tiny_skia::Color;

pub(super) fn parse_color(s: &str) -> Color {
    resolve_color(s, Color::BLACK)
}

pub(super) fn resolve_color(s: &str, current: Color) -> Color {
    if s.trim().eq_ignore_ascii_case("currentcolor") {
        current
    } else {
        parse_color_checked(s).unwrap_or(Color::BLACK)
    }
}

pub(super) fn parse_color_checked(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return hex_color(hex);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(Color::BLACK);
    }
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    if let Some((name, args)) = s.strip_suffix(')').and_then(|s| s.split_once('(')) {
        let (channels, alpha) = components(args)?;
        let name = name.trim();
        let [r, g, b] = if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") {
            [
                rgb_channel(channels[0])?,
                rgb_channel(channels[1])?,
                rgb_channel(channels[2])?,
            ]
        } else if name.eq_ignore_ascii_case("hsl") || name.eq_ignore_ascii_case("hsla") {
            hsl_channels(channels)?
        } else {
            return None;
        };
        let a = match alpha {
            Some(value) => (fraction(value)? * 255.0).round() as u8,
            None => 255,
        };
        return Some(Color::from_rgba8(r, g, b, a));
    }
    let [r, g, b] = crate::css_colors::lookup_css_color(s)?;
    Some(Color::from_rgba8(r, g, b, 255))
}

fn hex_color(s: &str) -> Option<Color> {
    let digits = match s.len() {
        3 | 4 => 1,
        6 | 8 => 2,
        _ => return None,
    };
    if !s.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut rgba = [255; 4];
    for (i, value) in rgba.iter_mut().take(s.len() / digits).enumerate() {
        let component = u8::from_str_radix(&s[i * digits..(i + 1) * digits], 16).ok()?;
        *value = if digits == 1 {
            component * 17
        } else {
            component
        };
    }
    Some(Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]))
}

fn components(args: &str) -> Option<([&str; 3], Option<&str>)> {
    if args.contains(',') {
        let mut parts = args.split(',').map(str::trim);
        let channels = [parts.next()?, parts.next()?, parts.next()?];
        let alpha = parts.next();
        return parts.next().is_none().then_some((channels, alpha));
    }
    let (args, alpha) = match args.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha.trim())),
        None => (args, None),
    };
    let mut parts = args.split_ascii_whitespace();
    let channels = [parts.next()?, parts.next()?, parts.next()?];
    parts.next().is_none().then_some((channels, alpha))
}

fn number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn fraction(s: &str) -> Option<f64> {
    let value = match s.strip_suffix('%') {
        Some(percent) => number(percent.trim())? / 100.0,
        None => number(s)?,
    };
    Some(value.clamp(0.0, 1.0))
}

fn rgb_channel(s: &str) -> Option<u8> {
    let value = if s.ends_with('%') {
        fraction(s)? * 255.0
    } else {
        number(s)?.clamp(0.0, 255.0)
    };
    Some(value.round() as u8)
}

fn hue_degrees(s: &str) -> Option<f64> {
    let suffix = s.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len();
    let value = number(&s[..suffix])?;
    let unit = &s[suffix..];
    let period = if unit.is_empty() || unit.eq_ignore_ascii_case("deg") {
        360.0
    } else if unit.eq_ignore_ascii_case("turn") {
        1.0
    } else if unit.eq_ignore_ascii_case("rad") {
        std::f64::consts::TAU
    } else if unit.eq_ignore_ascii_case("grad") {
        400.0
    } else {
        return None;
    };
    Some(value.rem_euclid(period) / period * 360.0)
}

fn hsl_channels([h, s, l]: [&str; 3]) -> Option<[u8; 3]> {
    let hue = hue_degrees(h)? / 30.0;
    let saturation = number(s.strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let lightness = number(l.strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let chroma = saturation * lightness.min(1.0 - lightness);
    // CSS Color 4, §7.2. The offsets select red, green and blue respectively.
    let channel = |offset: f64| {
        let k = (offset + hue) % 12.0_f64;
        let value = lightness - chroma * (k - 3.0_f64).min(9.0 - k).clamp(-1.0, 1.0);
        (value * 255.0).round() as u8
    };
    Some([channel(0.0), channel(8.0), channel(4.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_paints() {
        for (input, expected) in [
            ("red", [255, 0, 0, 255]),
            ("BLUE", [0, 0, 255, 255]),
            ("currentColor", [0, 0, 0, 255]),
            ("transparent", [0, 0, 0, 0]),
            ("#0f0", [0, 255, 0, 255]),
            ("#00ff00", [0, 255, 0, 255]),
            ("#0f08", [0, 255, 0, 136]),
            ("#10db327f", [16, 219, 50, 127]),
            ("rgb(100%, 50%, 0%)", [255, 128, 0, 255]),
            ("rgba(255,0,0,.25)", [255, 0, 0, 64]),
            ("rgb(30 80% 0 / 50%)", [30, 204, 0, 128]),
            ("hsla(-120, 100%, 50%, 1)", [0, 0, 255, 255]),
            ("hsl(.5turn 100% 50% / 0.25)", [0, 255, 255, 64]),
            ("hsl(200grad,0%,50%)", [128, 128, 128, 255]),
        ] {
            let [r, g, b, a] = expected;
            assert_eq!(
                parse_color_checked(input),
                Some(Color::from_rgba8(r, g, b, a)),
                "{input}"
            );
        }
        for input in [
            "#12345",
            "#+aaa",
            "#ß00",
            "var(--paint)",
            "rgba(0,0,0,nan)",
            "hsl(1s,10%,10%)",
            "rgb(1 2 3 / 1 / 2)",
        ] {
            assert_eq!(parse_color_checked(input), None, "{input}");
        }
    }
}
