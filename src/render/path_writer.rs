//! Minimal SVG path `d` data construction.
//!
//! Produces the shortest standards-compliant path encoding:
//! - trailing zeros and a redundant leading `0` before `.` are trimmed
//!   (`0.6200` -> `.62`, `1.0000` -> `1`);
//! - a numeric separator is inserted only when SVG number grammar would
//!   otherwise merge two adjacent numbers;
//! - consecutive identical *repeatable* drawing commands share one command
//!   letter (`L a b L c d` -> `L a b c d`); `M` and `Z` are never elided.
//!
//! All transformations are purely lexical — no coordinate is re-rounded — so
//! output renders identically to the verbose form.

use super::emit::push_u64;

/// Incrementally builds minimal SVG path `d` data.
pub(crate) struct PathWriter {
    buf: String,
    /// Last command letter emitted (ASCII), or 0 if none yet.
    last_cmd: u8,
    /// Whether the previously emitted token was a number.
    prev_was_number: bool,
    /// Whether the previous number token contained a `.`.
    prev_had_dot: bool,
}

impl PathWriter {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            buf: String::with_capacity(cap),
            last_cmd: 0,
            prev_was_number: false,
            prev_had_dot: false,
        }
    }

    /// Start with a verbatim prefix (e.g. `"F:"`) that is not path data.
    pub(crate) fn with_prefix(prefix: &str) -> Self {
        let mut w = Self::with_capacity(prefix.len() + 96);
        w.buf.push_str(prefix);
        w
    }

    /// Emit a command letter, eliding a redundant repeat of the same
    /// repeatable command. `M`/`Z` are never elided.
    pub(crate) fn cmd(&mut self, c: u8) {
        let repeatable = !matches!(c.to_ascii_uppercase(), b'M' | b'Z');
        if repeatable && self.last_cmd == c {
            return; // same command continues; omit redundant letter
        }
        self.buf.push(c as char);
        self.last_cmd = c;
        self.prev_was_number = false;
        self.prev_had_dot = false;
    }

    /// Emit the close-path command.
    pub(crate) fn close(&mut self) {
        self.cmd(b'Z');
    }

    /// Insert a separator before a number whose first emitted char is `first`,
    /// only when SVG grammar requires it.
    fn maybe_sep(&mut self, first: u8) {
        if self.prev_was_number {
            let self_delimits = match first {
                b'-' | b'+' => true,
                b'.' => self.prev_had_dot,
                _ => false, // digit-leading: always needs a separator
            };
            if !self_delimits {
                self.buf.push(' ');
            }
        }
    }

    /// Push a number given as a string token (may carry sign / leading zero /
    /// trailing zeros / scientific notation). Normalizes lexically.
    pub(crate) fn number(&mut self, tok: &str) {
        let mut s = tok;
        let mut neg = false;
        if let Some(r) = s.strip_prefix('-') {
            neg = true;
            s = r;
        } else if let Some(r) = s.strip_prefix('+') {
            s = r;
        }

        // Scientific notation is rare in our inputs; pass through without
        // trailing-zero trimming to stay correct.
        if s.contains(['e', 'E']) {
            let first = if neg {
                b'-'
            } else {
                s.as_bytes().first().copied().unwrap_or(b'0')
            };
            self.maybe_sep(first);
            if neg {
                self.buf.push('-');
            }
            self.buf.push_str(s);
            self.prev_was_number = true;
            self.prev_had_dot = s.contains('.');
            return;
        }

        let (int_part, frac_part) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s, ""),
        };
        let int_trimmed = int_part.trim_start_matches('0'); // "0"->"", "012"->"12"
        let frac_trimmed = frac_part.trim_end_matches('0');
        let has_frac = !frac_trimmed.is_empty();

        // Whole value is zero.
        if int_trimmed.is_empty() && !has_frac {
            self.maybe_sep(b'0');
            self.buf.push('0');
            self.prev_was_number = true;
            self.prev_had_dot = false;
            return;
        }

        let first: u8 = if neg {
            b'-'
        } else if int_trimmed.is_empty() {
            b'.'
        } else {
            int_trimmed.as_bytes()[0]
        };
        self.maybe_sep(first);
        if neg {
            self.buf.push('-');
        }
        self.buf.push_str(int_trimmed);
        if has_frac {
            self.buf.push('.');
            self.buf.push_str(frac_trimmed);
            self.prev_had_dot = true;
        } else {
            self.prev_had_dot = false;
        }
        self.prev_was_number = true;
    }

    /// Push an `f64` em-coordinate: round half-up to 4 decimals, trim, strip a
    /// leading zero, and emit with a minimal separator. Allocation-free.
    pub(crate) fn push_f64(&mut self, v: f64) {
        if !v.is_finite() {
            self.number("0");
            return;
        }
        let neg = v.is_sign_negative();
        let scaled = (v.abs() * 10_000.0 + 0.5) as u64;
        if scaled == 0 {
            self.maybe_sep(b'0');
            self.buf.push('0');
            self.prev_was_number = true;
            self.prev_had_dot = false;
            return;
        }
        let int = scaled / 10_000;
        let frac = (scaled % 10_000) as u32;
        let first: u8 = if neg {
            b'-'
        } else if int == 0 {
            b'.'
        } else {
            b'0' // any digit; maybe_sep only checks for -/+/.
        };
        self.maybe_sep(first);
        if neg {
            self.buf.push('-');
        }
        if int != 0 {
            push_u64(&mut self.buf, int);
        }
        if frac != 0 {
            let digits = [
                (frac / 1000) as u8,
                ((frac / 100) % 10) as u8,
                ((frac / 10) % 10) as u8,
                (frac % 10) as u8,
            ];
            let mut last = 3usize;
            while digits[last] == 0 {
                last -= 1;
            }
            self.buf.push('.');
            for &d in &digits[..=last] {
                self.buf.push((b'0' + d) as char);
            }
            self.prev_had_dot = true;
        } else {
            self.prev_had_dot = false;
        }
        self.prev_was_number = true;
    }

    pub(crate) fn finish(self) -> String {
        self.buf
    }
}

/// Re-emit any valid SVG path `d` string as minimal data. Preserves a leading
/// `"F:"` fill prefix. Single source of truth for "make this path minimal".
pub(crate) fn minify_path_d(d: &str) -> String {
    let (prefix, data) = match d.strip_prefix("F:") {
        Some(rest) => ("F:", rest),
        None => ("", d),
    };
    let mut pw = PathWriter::with_prefix(prefix);
    let bytes = data.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b.is_ascii_alphabetic() && b != b'e' && b != b'E' {
            pw.cmd(b);
            i += 1;
        } else if b == b'-' || b == b'+' || b == b'.' || b.is_ascii_digit() {
            let start = i;
            if bytes[i] == b'-' || bytes[i] == b'+' {
                i += 1;
            }
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && bytes[i] == b'.' {
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                i += 1;
                if i < len && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            pw.number(&data[start..i]);
        } else {
            // whitespace or comma separator — drop it
            i += 1;
        }
    }
    pw.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokenize a path into (commands, numbers) for value-equivalence checks.
    fn tokens(d: &str) -> (Vec<char>, Vec<f64>) {
        let data = d.strip_prefix("F:").unwrap_or(d);
        let mut cmds = Vec::new();
        let mut nums = Vec::new();
        let bytes = data.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b.is_ascii_alphabetic() && b != b'e' && b != b'E' {
                cmds.push(b as char);
                i += 1;
            } else if b == b'-' || b == b'+' || b == b'.' || b.is_ascii_digit() {
                let start = i;
                if bytes[i] == b'-' || bytes[i] == b'+' {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                nums.push(data[start..i].parse::<f64>().unwrap());
            } else {
                i += 1;
            }
        }
        (cmds, nums)
    }

    #[test]
    fn trims_trailing_and_leading_zeros() {
        let mut pw = PathWriter::with_prefix("F:");
        pw.cmd(b'M');
        pw.push_f64(0.0);
        pw.push_f64(0.62);
        pw.cmd(b'L');
        pw.push_f64(1.0);
        pw.push_f64(2.5);
        assert_eq!(pw.finish(), "F:M0 .62L1 2.5");
    }

    #[test]
    fn negative_self_delimits() {
        let mut pw = PathWriter::with_capacity(16);
        pw.cmd(b'L');
        pw.push_f64(0.5);
        pw.push_f64(-0.5);
        // ".5" then "-.5": minus self-delimits, no separator.
        assert_eq!(pw.finish(), "L.5-.5");
    }

    #[test]
    fn collapses_repeated_commands() {
        let mut pw = PathWriter::with_capacity(16);
        pw.cmd(b'L');
        pw.push_f64(1.0);
        pw.push_f64(2.0);
        pw.cmd(b'L');
        pw.push_f64(3.0);
        pw.push_f64(4.0);
        assert_eq!(pw.finish(), "L1 2 3 4");
    }

    #[test]
    fn never_collapses_moveto() {
        let mut pw = PathWriter::with_capacity(16);
        pw.cmd(b'M');
        pw.push_f64(1.0);
        pw.push_f64(2.0);
        pw.cmd(b'M');
        pw.push_f64(3.0);
        pw.push_f64(4.0);
        assert_eq!(pw.finish(), "M1 2M3 4");
    }

    #[test]
    fn dot_after_integer_needs_separator() {
        // "1" (no dot) then ".5": must keep a separator or it parses as 1.5.
        let mut pw = PathWriter::with_capacity(16);
        pw.cmd(b'L');
        pw.number("1");
        pw.number("0.5");
        assert_eq!(pw.finish(), "L1 .5");
    }

    #[test]
    fn dot_after_decimal_self_delimits() {
        let mut pw = PathWriter::with_capacity(16);
        pw.cmd(b'L');
        pw.number("1.5");
        pw.number("0.5");
        // "1.5" already has a dot, so ".5" cannot extend it -> no separator.
        assert_eq!(pw.finish(), "L1.5.5");
    }

    #[test]
    fn minify_preserves_fill_prefix_and_values() {
        let verbose =
            "F:M0.0000,0.6200 L0.1000,0.7000 C0.1300,0.8400 0.2000,1.0000 0.2000,1.0000 Z";
        let min = minify_path_d(verbose);
        assert!(min.starts_with("F:"));
        assert!(min.len() < verbose.len());
        let (_vc, vn) = tokens(verbose);
        let (mc, mn) = tokens(&min);
        assert_eq!(vn.len(), mn.len(), "number count must match");
        for (a, b) in vn.iter().zip(mn.iter()) {
            assert!((a - b).abs() < 1e-9, "value drift: {a} vs {b}");
        }
        assert!(mc.iter().all(|c| "MLCZ".contains(*c)));
    }

    #[test]
    fn minify_keeps_arc_flags_separate() {
        // a rx ry rot large-arc sweep x y — flags 0/1 must not merge with coords.
        let verbose = "M1,1 a0.5,0.5 0 1 1 -1,0";
        let min = minify_path_d(verbose);
        let (_, vn) = tokens(verbose);
        let (_, mn) = tokens(&min);
        assert_eq!(vn, mn, "arc parameter values must be preserved exactly");
    }

    #[test]
    fn minify_handles_separatorless_input_idempotent() {
        let once = minify_path_d("F:M0.5000,0.5000 L1.0000,0.0000Z");
        let twice = minify_path_d(&once);
        assert_eq!(once, twice, "minify must be idempotent");
    }
}
