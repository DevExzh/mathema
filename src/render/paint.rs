//! Small, dependency-free CSS paint classification shared by SVG and raster.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Srgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintClass {
    Concrete(Srgba),
    CurrentColor,
    HostDependent,
    Invalid,
}

pub(crate) fn classify(value: &str) -> PaintClass {
    let value = value.trim();
    if value.eq_ignore_ascii_case("currentcolor") {
        return PaintClass::CurrentColor;
    }
    if value.eq_ignore_ascii_case("transparent") || value.eq_ignore_ascii_case("none") {
        return PaintClass::Concrete(Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        });
    }
    if let Some(color) = parse_concrete(value) {
        return PaintClass::Concrete(color);
    }
    const VIEWER_FUNCTIONS: &[&str] = &[
        "var",
        "url",
        "color",
        "lab",
        "lch",
        "oklab",
        "oklch",
        "hwb",
        "color-mix",
        "device-cmyk",
        "light-dark",
        "contrast-color",
        "attr",
    ];
    const VIEWER_KEYWORDS: &[&str] = &[
        "inherit",
        "initial",
        "unset",
        "revert",
        "revert-layer",
        "context-fill",
        "context-stroke",
        "canvas",
        "canvastext",
        "linktext",
        "visitedtext",
        "buttonface",
        "buttontext",
        "field",
        "fieldtext",
        "highlight",
        "highlighttext",
        "mark",
        "marktext",
        "graytext",
        "accentcolor",
        "accentcolortext",
        "selecteditem",
        "selecteditemtext",
        "activetext",
        "inactivetext",
        "buttonborder",
        "buttonhighlight",
        "buttonshadow",
        "window",
        "windowtext",
        "windowframe",
        "menu",
        "menutext",
    ];
    let uses_viewer_function = value.split_once('(').is_some_and(|(name, _)| {
        VIEWER_FUNCTIONS
            .iter()
            .any(|f| name.eq_ignore_ascii_case(f))
    });
    if uses_viewer_function
        || VIEWER_KEYWORDS
            .iter()
            .any(|key| value.eq_ignore_ascii_case(key))
    {
        return PaintClass::HostDependent;
    }
    PaintClass::Invalid
}

fn parse_concrete(value: &str) -> Option<Srgba> {
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some((name, args)) = value.strip_suffix(')').and_then(|v| v.split_once('(')) {
        let (channels, alpha) = parse_components(args)?;
        let name = name.trim();
        let [red, green, blue] =
            if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") {
                [
                    parse_rgb_channel(channels[0])?,
                    parse_rgb_channel(channels[1])?,
                    parse_rgb_channel(channels[2])?,
                ]
            } else if name.eq_ignore_ascii_case("hsl") || name.eq_ignore_ascii_case("hsla") {
                parse_hsl(channels)?
            } else {
                return None;
            };
        let alpha = match alpha {
            Some(value) => parse_alpha(value)?,
            None => 1.0,
        };
        return Some(Srgba {
            red,
            green,
            blue,
            alpha: (alpha * 255.0).round() as u8,
        });
    }
    let [red, green, blue] = crate::css_colors::lookup_css_color(value)?;
    Some(Srgba {
        red,
        green,
        blue,
        alpha: 255,
    })
}

fn parse_hex(value: &str) -> Option<Srgba> {
    let digits = match value.len() {
        3 | 4 => 1,
        6 | 8 => 2,
        _ => return None,
    };
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut result = [255u8; 4];
    for (index, channel) in result.iter_mut().take(value.len() / digits).enumerate() {
        let number = u8::from_str_radix(&value[index * digits..(index + 1) * digits], 16).ok()?;
        *channel = if digits == 1 { number * 17 } else { number };
    }
    Some(Srgba {
        red: result[0],
        green: result[1],
        blue: result[2],
        alpha: result[3],
    })
}

fn parse_components(value: &str) -> Option<([&str; 3], Option<&str>)> {
    if value.contains(',') {
        let mut parts = value.split(',').map(str::trim);
        let channels = [parts.next()?, parts.next()?, parts.next()?];
        let alpha = parts.next();
        return parts.next().is_none().then_some((channels, alpha));
    }
    let (channels, alpha) = value
        .split_once('/')
        .map_or((value, None), |(a, b)| (a, Some(b.trim())));
    let mut parts = channels.split_ascii_whitespace();
    let result = [parts.next()?, parts.next()?, parts.next()?];
    parts.next().is_none().then_some((result, alpha))
}

fn parse_number(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn parse_alpha(value: &str) -> Option<f64> {
    let value = value.trim();
    let number = value.strip_suffix('%').map_or_else(
        || parse_number(value),
        |v| parse_number(v).map(|v| v / 100.0),
    )?;
    Some(number.clamp(0.0, 1.0))
}

fn parse_rgb_channel(value: &str) -> Option<u8> {
    let value = value.trim();
    let number = value.strip_suffix('%').map_or_else(
        || parse_number(value).map(|v| v.clamp(0.0, 255.0)),
        |v| parse_number(v).map(|v| v.clamp(0.0, 100.0) * 255.0 / 100.0),
    )?;
    Some(number.round() as u8)
}

fn parse_hsl([hue, saturation, lightness]: [&str; 3]) -> Option<[u8; 3]> {
    let hue = hue.trim();
    let split = hue
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .len();
    let number = parse_number(&hue[..split])?;
    let unit = &hue[split..];
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
    let h = number.rem_euclid(period) / period * 12.0;
    let s = parse_number(saturation.trim().strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let l = parse_number(lightness.trim().strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let c = s * l.min(1.0 - l);
    let channel = |offset: f64| {
        ((l - c
            * ((offset + h) % 12.0 - 3.0)
                .min(9.0 - (offset + h) % 12.0)
                .clamp(-1.0, 1.0))
            * 255.0)
            .round() as u8
    };
    Some([channel(0.0), channel(8.0), channel(4.0)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_concrete_and_browser_paints() {
        assert!(matches!(
            classify("rgba(255,0,0,.5)"),
            PaintClass::Concrete(Srgba { alpha: 128, .. })
        ));
        assert!(matches!(
            classify("#ff000080"),
            PaintClass::Concrete(Srgba { alpha: 128, .. })
        ));
        assert!(matches!(
            classify("transparent"),
            PaintClass::Concrete(Srgba { alpha: 0, .. })
        ));
        assert!(matches!(
            classify("none"),
            PaintClass::Concrete(Srgba { alpha: 0, .. })
        ));
        assert!(matches!(classify("currentColor"), PaintClass::CurrentColor));
        assert!(matches!(classify("var(--ink)"), PaintClass::HostDependent));
        assert!(matches!(classify("url(#paint)"), PaintClass::HostDependent));
        assert!(matches!(classify("rgba(nan,0,0,.5)"), PaintClass::Invalid));
        assert!(matches!(
            classify("rgba(0,0,0,not-alpha)"),
            PaintClass::Invalid
        ));
        assert!(matches!(
            classify("color(display-p3 1 0 0)"),
            PaintClass::HostDependent
        ));
    }
}
