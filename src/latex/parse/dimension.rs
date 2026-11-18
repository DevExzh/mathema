//! Numeric TeX dimensions at the converter's normal ten-point text size.
use crate::ast::Rational;
use nom::IResult;

pub(super) struct Dimension {
    pub(super) width: Rational,
    pub(super) unit: Option<&'static str>,
}

struct Unit {
    name: &'static str,
    numerator: i32,
    denominator: u32,
}

// Text em, KaTeX's default x-height, and TeX/KaTeX point conversion ratios.
fn finite_unit(input: &str) -> Option<(&str, Unit)> {
    let (letters, rest) = input.split_at_checked(2)?;
    let (name, numerator, denominator) = match [
        letters.as_bytes()[0].to_ascii_lowercase(),
        letters.as_bytes()[1].to_ascii_lowercase(),
    ] {
        [b'e', b'm'] => ("em", 1, 1),
        [b'e', b'x'] => ("ex", 431, 1000),
        [b'm', b'u'] => ("mu", 1, 18),
        [b'p', b't'] => ("pt", 1, 10),
        [b'p', b'c'] => ("pc", 6, 5),
        [b'b', b'p'] => ("bp", 803, 8000),
        [b'p', b'x'] => ("px", 803, 8000),
        [b'm', b'm'] => ("mm", 7227, 25400),
        [b'c', b'm'] => ("cm", 7227, 2540),
        [b'i', b'n'] => ("in", 7227, 1000),
        [b'd', b'd'] => ("dd", 1238, 11570),
        [b'c', b'c'] => ("cc", 14856, 11570),
        [b'n', b'd'] => ("nd", 685, 6420),
        [b'n', b'c'] => ("nc", 1370, 1070),
        [b's', b'p'] => ("sp", 1, 655360),
        _ => return None,
    };
    Some((
        rest,
        Unit {
            name,
            numerator,
            denominator,
        },
    ))
}

fn trim_space(s: &str) -> &str {
    &s[super::skip_ws_bytes(s.as_bytes())..]
}

/// Return the unsigned decimal, its sign, and the unconsumed input.
/// TeX permits whitespace and repeated signs before a coefficient.
fn coefficient(input: &str) -> Option<(&str, bool, &str)> {
    let mut number = trim_space(input);
    let mut negative = false;
    while let Some(&sign @ (b'-' | b'+')) = number.as_bytes().first() {
        negative ^= sign == b'-';
        number = trim_space(&number[1..]);
    }
    let integer_digits = number.bytes().take_while(u8::is_ascii_digit).count();
    let mut end = integer_digits;
    let mut fractional_digits = 0;
    if number.as_bytes().get(end) == Some(&b'.') {
        end += 1;
        fractional_digits = number[end..].bytes().take_while(u8::is_ascii_digit).count();
        end += fractional_digits;
    }
    if integer_digits + fractional_digits == 0 {
        return None;
    }
    Some((&number[..end], negative, &number[end..]))
}

pub(super) fn parse(input: &str) -> IResult<&str, Dimension> {
    let bad = || nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit));
    let (decimal, negative, tail) = coefficient(input).ok_or_else(bad)?;
    let rest = trim_space(tail);
    // Mathema has historically accepted a unitless coefficient as points.
    let (rest, name, n, d) = match finite_unit(rest) {
        Some((rest, unit)) => (rest, Some(unit.name), unit.numerator, unit.denominator),
        None => (rest, None, 1, 10),
    };
    let scale = Rational::new(if negative { -n } else { n }, d);
    let too_large =
        || nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Verify));
    let width = Rational::from_scaled_decimal(decimal, scale).ok_or_else(too_large)?;
    // TeX stores a dimension in scaled points (or scaled mu). Its maximum
    // magnitude is 2^30-1 scaled units; rounding here permits decimal
    // spellings of the same boundary value.
    let units_per_em = if name == Some("mu") { 18.0 } else { 10.0 };
    if (width.as_f64().abs() * units_per_em * 65536.0).round() > f64::from((1u32 << 30) - 1) {
        return Err(too_large());
    }
    Ok((rest, Dimension { width, unit: name }))
}

pub(super) fn width(input: &str) -> IResult<&str, Rational> {
    parse(input).map(|(rest, dim)| (rest, dim.width))
}

/// Stretch/shrink is discarded only after a complete, explicitly unit-bearing
/// coefficient. A trailing identifier still belongs to the enclosing formula.
pub(super) fn glue_component(input: &str) -> Option<(&str, &'static str)> {
    let (_, _, rest) = coefficient(input)?;
    let rest = trim_space(rest);
    for order in ["filll", "fill", "fil"] {
        if let Some((prefix, tail)) = rest.split_at_checked(order.len())
            && prefix.eq_ignore_ascii_case(order)
        {
            return Some((tail, order));
        }
    }
    finite_unit(rest).map(|(tail, unit)| (tail, unit.name))
}
