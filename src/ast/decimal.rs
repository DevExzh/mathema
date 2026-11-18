//! Conversion of decimal lengths to the bounded public rational type.

use super::Rational;

impl Rational {
    /// Convert a signed decimal times a unit ratio. Prefer the exact reduced
    /// fraction. When that fraction exceeds our integer range, approximate
    /// the value within the numerator and denominator bounds.
    pub(crate) fn from_scaled_decimal(text: &str, unit: Self) -> Option<Self> {
        if unit.den == 0 {
            return None;
        }
        let (magnitude, negative) = match text.as_bytes().first()? {
            b'-' => (&text[1..], true),
            b'+' => (&text[1..], false),
            _ => (text, false),
        };
        let negative = negative != (unit.num < 0);
        if let Some((n, d)) = decimal_ratio(magnitude)
            && let Some(n) = n.checked_mul(u128::from(unit.num.unsigned_abs()))
            && let Some(d) = d.checked_mul(u128::from(unit.den))
            && let Some(ratio) = reduced_ratio(n, d, negative)
        {
            return Some(ratio);
        }

        if !magnitude.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return None;
        }
        let value = magnitude.parse::<f64>().ok()? * unit.as_f64().abs();
        if !value.is_finite() || value > f64::from(i32::MAX) + f64::from(u8::from(negative)) {
            return None;
        }
        Some(closest_ratio(value, negative))
    }
    /// Scale a width for an output font while retaining simple exact fractions.
    pub(crate) fn scaled_by(self, num: u32, den: u32) -> Self {
        if self.den == 0 || den == 0 {
            return self;
        }
        let n = u128::from(self.num.unsigned_abs()) * u128::from(num);
        let d = u128::from(self.den) * u128::from(den);
        reduced_ratio(n, d, self.num < 0).unwrap_or_else(|| {
            closest_ratio(
                self.as_f64().abs() * f64::from(num) / f64::from(den),
                self.num < 0,
            )
        })
    }
}

fn reduced_ratio(n: u128, d: u128, negative: bool) -> Option<Rational> {
    let (mut a, mut b) = (n, d);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let n = i64::try_from(n / a).ok()?;
    let d = u32::try_from(d / a).ok()?;
    Some(Rational {
        num: i32::try_from(if negative { -n } else { n }).ok()?,
        den: d,
    })
}

fn decimal_ratio(s: &str) -> Option<(u128, u128)> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    let den = 10u128.checked_pow(frac.len().try_into().ok()?)?;
    let mut n = 0u128;
    for b in whole.bytes().chain(frac.bytes()) {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(u128::from(b - b'0'))?;
    }
    Some((n, den))
}

/// Continued-fraction convergents. The final coefficient is bounded by
/// both public fields; choose the nearer of the two remaining candidates.
fn closest_ratio(value: f64, negative: bool) -> Rational {
    let max_num = i32::MAX as u64 + u64::from(negative);
    let max_den = u64::from(u32::MAX);
    let finish = |n: u64, d: u64| Rational {
        num: (if negative { -(n as i64) } else { n as i64 }) as i32,
        den: d as u32,
    };
    if value >= max_num as f64 {
        return finish(max_num, 1);
    }
    let mut x = value;
    let (mut n0, mut d0) = (0u64, 1u64);
    let (mut n1, mut d1) = (1u64, 0u64);
    loop {
        let a = x.floor() as u64;
        let bound = (max_num - n0)
            .checked_div(n1)
            .unwrap_or(u64::MAX)
            .min((max_den - d0).checked_div(d1).unwrap_or(u64::MAX));
        if a > bound {
            let (n, d) = (n0 + bound * n1, d0 + bound * d1);
            return if (n as f64 / d as f64 - value).abs() < (n1 as f64 / d1 as f64 - value).abs() {
                finish(n, d)
            } else {
                finish(n1, d1)
            };
        }
        let (n, d) = (n0 + a * n1, d0 + a * d1);
        let fraction = x - a as f64;
        if fraction == 0.0 || n as f64 / d as f64 == value {
            return finish(n, d);
        }
        (n0, d0, n1, d1) = (n1, d1, n, d);
        x = fraction.recip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_prefer_exact_widths_and_recover_recurring_em_values() {
        for (s, n, d) in [
            ("0.004", 1, 250),
            ("-.00000152587890625", -1, 655360),
            ("0.123456789", 123456789, 1000000000),
            ("0.16666666666666666", 1, 6),
            ("0.2222222222222222", 2, 9),
            ("-2147483648", i32::MIN, 1),
        ] {
            assert_eq!(
                Rational::from_scaled_decimal(s, Rational::one()),
                Some(Rational::new(n, d)),
                "{s}"
            );
        }
        assert_eq!(
            Rational::from_scaled_decimal(".5", Rational::new(1, 18)),
            Some(Rational::new(1, 36))
        );
        assert_eq!(Rational::from_scaled_decimal("NaN", Rational::one()), None);
        assert_eq!(Rational::from_scaled_decimal(".", Rational::one()), None);
    }
}
