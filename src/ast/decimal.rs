//! Decimal lengths and conversion to the bounded public rational type.
use super::Rational;
use std::fmt::Write;

impl Rational {
    /// Convert a signed decimal times an exact unit ratio. If it cannot be
    /// represented exactly, choose a nearby fraction within both field bounds.
    pub(crate) fn from_scaled_decimal(text: &str, unit: Self) -> Option<Self> {
        if unit.den == 0 {
            return None;
        }
        let (s, negative) = match text.as_bytes().first()? {
            b'-' => (&text[1..], true),
            b'+' => (&text[1..], false),
            _ => (text, false),
        };
        let negative = negative != (unit.num < 0);
        if let Some((n, d)) = decimal_ratio(s)
            && let Some(n) = n.checked_mul(u128::from(unit.num.unsigned_abs()))
            && let Some(d) = d.checked_mul(u128::from(unit.den))
        {
            return bounded_ratio(n, d, negative);
        }
        // Scientific notation and decimals beyond u128 precision. Reduce
        // the finite floating-point value to a bounded decimal first.
        if !s.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return None;
        }
        let value = s.parse::<f64>().ok()? * unit.as_f64().abs();
        if !value.is_finite() || value > f64::from(i32::MAX) + f64::from(u8::from(negative)) {
            return None;
        }
        if value < 0.5 / f64::from(u32::MAX) {
            return Some(Self::zero());
        }
        let (n, d) = decimal_ratio(&value.to_string())?;
        bounded_ratio(n, d, negative)
    }

    pub(crate) fn scaled_by(self, num: u32, den: u32) -> Self {
        if self.den == 0 || den == 0 {
            return self;
        }
        bounded_ratio(
            u128::from(self.num.unsigned_abs()) * u128::from(num),
            u128::from(self.den) * u128::from(den),
            self.num < 0,
        )
        .unwrap_or_else(|| Self::new(if self.num < 0 { i32::MIN } else { i32::MAX }, 1))
    }

    /// Use the short conventional spelling when it recovers the same
    /// fraction. Extra decimal digits distinguish nearby large denominators.
    pub(crate) fn write_scaled_decimal(self, buf: &mut String, multiplier: u32) {
        let n = i64::from(self.num) * i64::from(multiplier);
        let d = i64::from(self.den);
        if d != 0 && n % d == 0 {
            let _ = write!(buf, "{}", n / d);
            return;
        }
        let start = buf.len();
        let _ = write!(buf, "{}", n as f64 / d as f64);
        if d == 0
            || Self::from_scaled_decimal(&buf[start..], Self::new(1, multiplier)) == Some(self)
        {
            return;
        }
        buf.truncate(start);
        if n < 0 {
            buf.push('-');
        }
        let (n, d) = (n.unsigned_abs(), d as u64);
        let _ = write!(buf, "{}", n / d);
        let mut rem = n % d;
        if rem == 0 {
            return;
        }
        buf.push('.');
        // Two distinct reduced fractions with u32 denominators differ by at
        // least 1/(u32::MAX)^2. 22 decimal places suffice for all these unit
        // multipliers, even when the exact decimal is recurring.
        for _ in 0..22 {
            rem *= 10;
            buf.push(char::from(b'0' + (rem / d) as u8));
            rem %= d;
            if rem == 0 {
                break;
            }
        }
        while buf.ends_with('0') {
            buf.pop();
        }
    }
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

/// Continued fractions with bounded numerator and denominator. After the
/// first out-of-range convergent, compare the last convergent and the
/// largest admissible intermediate fraction using integer remainders.
fn bounded_ratio(mut n: u128, mut d: u128, negative: bool) -> Option<Rational> {
    let max_n = i32::MAX as u64 + u64::from(negative);
    let max_d = u64::from(u32::MAX);
    let finish = |n: u64, d: u64| {
        Rational::new(
            (if negative { -(n as i64) } else { n as i64 }) as i32,
            d as u32,
        )
    };
    if n <= u128::from(max_n) && d <= u128::from(max_d) {
        return Some(finish(n as u64, d as u64));
    }
    if n / d > u128::from(max_n) || (n / d == u128::from(max_n) && !n.is_multiple_of(d)) {
        return None;
    }
    let (mut p0, mut q0, mut p1, mut q1) = (0u64, 1u64, 1u64, 0u64);
    loop {
        let (a, rem) = (n / d, n % d);
        let k = (max_n - p0)
            .checked_div(p1)
            .unwrap_or(u64::MAX)
            .min((max_d - q0).checked_div(q1).unwrap_or(u64::MAX));
        if a > u128::from(k) {
            let middle = 2 * u128::from(k);
            // n/d < 2k + q0/q1 selects the intermediate fraction. Both
            // remainder products are bounded by the input denominator.
            let near = a < middle || (a == middle && rem * u128::from(q1) < d * u128::from(q0));
            return Some(if near {
                finish(p0 + k * p1, q0 + k * q1)
            } else {
                finish(p1, q1)
            });
        }
        let (p, q) = (p0 + a as u64 * p1, q0 + a as u64 * q1);
        if rem == 0 {
            return Some(finish(p, q));
        }
        (p0, q0, p1, q1) = (p1, q1, p, q);
        (n, d) = (d, rem);
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
