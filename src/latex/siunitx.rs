//! `siunitx` (and the small `units` package's `\unit`) support: typesetting of
//! numbers, units, angles, and quantities.
//!
//! Like [`super::mhchem`], this module does **not** introduce a dedicated
//! [`crate::ast::compact::CompactNode`] variant. Instead it **translates** each
//! siunitx command into an equivalent standard-LaTeX math string, which the
//! parser then re-parses through its normal path. Every existing layout /
//! render / writer feature (superscripts, `\mathrm`, thin spaces `\,`,
//! `\times`, …) is therefore reused for free.
//!
//! The translation is a *common subset* matching MathJax / siunitx for the
//! cases users hit most often. It is lenient: malformed input degrades
//! gracefully (best-effort emission) and never panics.
//!
//! # Commands
//!
//! * `\num{<number>}` — a formatted number. Supports a leading sign, a `.`/`,`
//!   decimal marker, scientific notation (`1.23e4` → `1.23×10^{4}`), digit-group
//!   thin-space separators for long integer parts (`12345` → `12\,345`), and
//!   uncertainties (`1.2(3)` and `1.2 +- 0.1` / `1.2\pm0.1`).
//! * `\si{<unit>}` / `\unit{<unit>}` (v3) — a formatted unit.
//! * `\SI{<number>}{<unit>}` / `\qty{<number>}{<unit>}` (v3) — number + thin
//!   space + unit.
//! * `\ang{<angle>}` — `\ang{30}` → `30°`, `\ang{1;2;3}` → `1°2′3″`.
//! * `\numrange{a}{b}` / `\SIrange{a}{b}{unit}` / `\qtyrange{a}{b}{unit}` —
//!   `a–b` (en dash), optionally followed by the unit.
//! * `\numlist{a;b;c}` / `\SIlist{…}{unit}` / `\qtylist{…}{unit}` — comma / "and"
//!   separated lists (best-effort).
//! * `\sisetup{…}` — accepted and ignored (global option handling is deferred).
//!
//! Units render **upright** (wrapped in `\mathrm`). Unit factors are separated
//! by thin spaces (`\kilo\gram\metre` → `kg\,m`); a `.` separator behaves the
//! same; `\per` (or `/`) emits a division slash; `\squared` / `\cubed` and an
//! explicit `^` become superscripts. Both literal units (`kg.m/s^2`, `m/s`) and
//! siunitx unit macros (`\kilo\gram`, `\metre\per\second\squared`, …) are
//! accepted.
//!
//! ## `\unit` / `\qty` conflict resolution
//!
//! The `units` package previously defined `\unit{x}` → `\,\mathrm{x}` and the
//! `physics` package defined `\qty{x}` → `\left(x\right)`. siunitx redefines
//! both. The conflict is resolved here:
//!
//! * `\unit` — siunitx semantics win: it is the unit-only formatter (no leading
//!   thin space). The old `units` macro is removed; simple cases such as
//!   `\unit{m}` / `\unit{kg}` still render upright.
//! * `\qty` — **arity-based coexistence**: `\qty{n}{unit}` (two groups) is a
//!   siunitx quantity, while the single-group `\qty{x}` keeps the `physics`
//!   auto-sized parentheses `\left(x\right)`.
//!
//! Both are wired through the parser's `dispatch_command`; user `\newcommand`
//! redefinitions still win because they expand in the preprocessor first.
//!
//! ## Deferred (documented, not implemented)
//!
//! Full option handling (`\num[...]{...}`, `\sisetup{...}`) — the optional
//! `[...]` is accepted and ignored. Locale decimal-marker output, fixed
//! exponents, per-mode reciprocals, unit `\tothe`/`\raisetothe`, complex
//! list/range phrasing, and tabular `S`-columns are out of scope.

use super::mhchem::capture_braced;

/// Translate a siunitx command (already matched by `name`, with `input`
/// positioned right after the command name) into standard LaTeX.
///
/// Returns `(remaining_input, latex)`. Always succeeds; malformed input yields
/// a best-effort (possibly empty) translation.
#[must_use]
pub fn translate<'a>(name: &str, input: &'a str) -> (&'a str, String) {
    let input = skip_optional(input);
    match name {
        "num" => map1(input, num_to_latex),
        "ang" => map1(input, ang_to_latex),
        "si" | "unit" => map1(input, unit_to_latex),
        "numlist" => map1(input, numlist_to_latex),
        "SI" | "qty" => si_qty(name, input),
        "numrange" => {
            let (r1, a) = grab(input);
            let (r2, b) = grab(r1);
            match (a, b) {
                (Some(a), Some(b)) => (r2, numrange_to_latex(a, b)),
                _ => (input, String::new()),
            }
        },
        "SIrange" | "qtyrange" => {
            let (r1, a) = grab(input);
            let (r2, b) = grab(r1);
            let (r3, u) = grab(r2);
            match (a, b) {
                (Some(a), Some(b)) => {
                    let mut out = numrange_to_latex(a, b);
                    // Skip the thin space + unit when the unit group is empty
                    // (e.g. `\SIrange{1}{10}{}`) so we don't emit a trailing `\,`.
                    if let Some(u) = u
                        && !u.trim().is_empty()
                    {
                        out.push_str("\\,");
                        out.push_str(&unit_to_latex(u));
                    }
                    (r3, out)
                },
                _ => (input, String::new()),
            }
        },
        "SIlist" | "qtylist" => {
            let (r1, list) = grab(input);
            let (r2, u) = grab(r1);
            match list {
                Some(list) => (r2, silist_to_latex(list, u.unwrap_or(""))),
                None => (input, String::new()),
            }
        },
        "sisetup" => {
            // Accept and ignore the mandatory option group.
            let (rest, _) = grab(input);
            (rest, String::new())
        },
        _ => (input, String::new()),
    }
}

/// Capture one `{ ... }` group, returning `(rest, Some(body))` or `(input,
/// None)` when no group is present.
fn grab(input: &str) -> (&str, Option<&str>) {
    match capture_braced(input) {
        Some((rest, body)) => (rest, Some(body)),
        None => (input, None),
    }
}

/// Apply a one-argument translator to the next `{...}` group.
fn map1(input: &str, f: impl Fn(&str) -> String) -> (&str, String) {
    match capture_braced(input) {
        Some((rest, body)) => (rest, f(body)),
        None => (input, String::new()),
    }
}
/// `\SI` / `\qty`: a number followed by a unit, with `\qty`'s single-argument
/// form falling back to the `physics` auto-sized parentheses.
fn si_qty<'a>(name: &str, input: &'a str) -> (&'a str, String) {
    let (r1, num) = grab(input);
    let (r2, unit) = grab(r1);
    match (num, unit) {
        (Some(n), Some(u)) => (r2, format!("{}\\,{}", num_to_latex(n), unit_to_latex(u))),
        (Some(x), None) if name == "qty" => (r1, format!("\\left({x}\\right)")),
        (Some(x), None) => (r1, num_to_latex(x)),
        (None, _) => (input, String::new()),
    }
}

/// Skip a leading optional `[...]` argument (accepted and ignored).
fn skip_optional(input: &str) -> &str {
    let trimmed = input.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return input;
    };
    let mut depth = 1usize;
    for (i, b) in rest.bytes().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[i + 1..];
                }
            },
            _ => {},
        }
    }
    input
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// Translate a `\num{...}` body.
#[must_use]
pub fn num_to_latex(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return String::new();
    }
    // Uncertainty: `1.2 +- 0.1` or `1.2\pm0.1`.
    if let Some((a, b)) = split_pm(body) {
        return format!(
            "{} \\pm {}",
            format_number(a.trim()),
            format_number(b.trim())
        );
    }
    // Concise uncertainty: `1.2(3)`.
    if let Some(open) = body.find('(')
        && body.ends_with(')')
    {
        let value = &body[..open];
        let unc = &body[open + 1..body.len() - 1];
        if !value.is_empty() && unc.chars().all(|c| c.is_ascii_digit()) {
            return format!("{}({unc})", format_number(value));
        }
    }
    format_number(body)
}

/// Split a number body on a `\pm` / `+-` uncertainty separator.
fn split_pm(body: &str) -> Option<(&str, &str)> {
    if let Some(i) = body.find("\\pm") {
        return Some((&body[..i], &body[i + 3..]));
    }
    if let Some(i) = body.find("+-") {
        return Some((&body[..i], &body[i + 2..]));
    }
    None
}

/// Format a single numeric token: optional sign, grouped integer part, decimal
/// fraction, and scientific exponent.
fn format_number(tok: &str) -> String {
    let mut tok = tok.trim();
    let mut out = String::with_capacity(tok.len() + 8);
    if let Some(rest) = tok.strip_prefix('-') {
        out.push('-');
        tok = rest;
    } else if let Some(rest) = tok.strip_prefix('+') {
        tok = rest;
    }

    let (mantissa, exponent) = match tok.find(['e', 'E']) {
        Some(i) => (&tok[..i], Some(&tok[i + 1..])),
        None => (tok, None),
    };

    let (int_part, frac_part) = match mantissa.find(['.', ',']) {
        Some(i) => (&mantissa[..i], Some(&mantissa[i + 1..])),
        None => (mantissa, None),
    };

    if int_part.is_empty() && frac_part.is_some() {
        // `\num{.5}` → `0.5`: supply the integer zero siunitx omits on input.
        out.push('0');
    } else {
        out.push_str(&group_int(int_part));
    }
    if let Some(frac) = frac_part {
        out.push('.');
        out.push_str(frac);
    }

    if let Some(exp) = exponent {
        let exp = clean_exp(exp.trim());
        if !exp.is_empty() {
            out.push_str("\\times10^{");
            out.push_str(&exp);
            out.push('}');
        }
    }
    out
}

/// Insert thin-space `\,` digit-group separators every three digits from the
/// right, but only for integer parts of five or more digits (siunitx's default
/// `group-minimum-digits=5`).
fn group_int(digits: &str) -> String {
    if digits.len() <= 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return digits.to_string();
    }
    let n = digits.len();
    let mut out = String::with_capacity(n + (n / 3) * 2);
    for (i, ch) in digits.char_indices() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push_str("\\,");
        }
        out.push(ch);
    }
    out
}

/// Normalize an exponent: drop a leading `+`, strip leading zeros (keeping a
/// sign and at least one digit).
fn clean_exp(exp: &str) -> String {
    let (sign, digits) = if let Some(rest) = exp.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = exp.strip_prefix('+') {
        ("", rest)
    } else {
        ("", exp)
    };
    let trimmed = digits.trim_start_matches('0');
    let digits = if trimmed.is_empty() && !digits.is_empty() {
        "0"
    } else {
        trimmed
    };
    format!("{sign}{digits}")
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

/// A parsed unit-stream item.
enum Item {
    /// An upright unit factor (e.g. `kg`, `m`, `Ω`).
    Unit(String),
    /// A superscript exponent attached to the previous factor.
    Sup(String),
    /// A division slash.
    Per,
}

/// Translate a `\si{...}` / `\unit{...}` body into upright LaTeX.
#[must_use]
pub fn unit_to_latex(body: &str) -> String {
    let items = parse_units(body);
    let mut out = String::with_capacity(body.len() * 3 + 8);
    let mut prev_unit = false;
    for item in items {
        match item {
            Item::Unit(text) => {
                if prev_unit {
                    out.push_str("\\,");
                }
                out.push_str("\\mathrm{");
                push_escaped(&mut out, &text);
                out.push('}');
                prev_unit = true;
            },
            Item::Sup(exp) => {
                out.push_str("^{");
                out.push_str(&exp);
                out.push('}');
            },
            Item::Per => {
                out.push('/');
                prev_unit = false;
            },
        }
    }
    out
}

/// What a unit macro maps to.
enum UnitTok {
    /// An SI prefix glued to the following base unit (e.g. `\kilo` → `k`).
    Prefix(&'static str),
    /// A base / derived unit (e.g. `\metre` → `m`).
    Base(&'static str),
    /// `\per` — division.
    Per,
    /// `\squared` — exponent 2.
    Squared,
    /// `\cubed` — exponent 3.
    Cubed,
}

fn parse_units(body: &str) -> Vec<Item> {
    let chars: Vec<char> = body.chars().collect();
    let n = chars.len();
    let mut items: Vec<Item> = Vec::new();
    let mut atom = String::new();
    let mut has_base = false;
    let mut i = 0;

    // Emit the in-progress atom as a Unit item (does not touch `has_base`).
    macro_rules! emit {
        () => {
            if !atom.is_empty() {
                items.push(Item::Unit(std::mem::take(&mut atom)));
            }
        };
    }

    // Push a superscript exponent (call after `emit!`). When a `\per` (or `/`)
    // slash is directly followed by an exponent macro with no unit in between
    // (e.g. `\si{\per\squared}`), the slash would be left dangling and render as
    // a bare `/^2`. The least-surprising, non-panicking choice is to drop the
    // dangling slash and the orphan exponent rather than emit `/^2`.
    macro_rules! push_sup {
        ($exp:expr) => {
            if matches!(items.last(), Some(Item::Per)) {
                items.pop();
            } else {
                items.push(Item::Sup($exp));
            }
        };
    }

    while i < n {
        let c = chars[i];
        match c {
            c if c.is_whitespace() || c == '.' || c == '~' => {
                emit!();
                has_base = false;
                i += 1;
            },
            '/' => {
                emit!();
                has_base = false;
                items.push(Item::Per);
                i += 1;
            },
            '^' => {
                emit!();
                has_base = false;
                i += 1;
                let exp = read_exponent(&chars, &mut i);
                push_sup!(exp);
            },
            '\\' => {
                i += 1;
                let start = i;
                while i < n && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i == start {
                    // Escaped non-letter (e.g. `\%`): skip the backslash, treat
                    // the next char as literal text below on the next pass.
                    continue;
                }
                let name: String = chars[start..i].iter().collect();
                match unit_macro(&name) {
                    Some(UnitTok::Prefix(t)) => {
                        if has_base {
                            emit!();
                            has_base = false;
                        }
                        atom.push_str(t);
                    },
                    Some(UnitTok::Base(t)) => {
                        if has_base {
                            emit!();
                        }
                        atom.push_str(t);
                        has_base = true;
                    },
                    Some(UnitTok::Per) => {
                        emit!();
                        has_base = false;
                        items.push(Item::Per);
                    },
                    Some(UnitTok::Squared) => {
                        emit!();
                        has_base = false;
                        push_sup!("2".to_string());
                    },
                    Some(UnitTok::Cubed) => {
                        emit!();
                        has_base = false;
                        push_sup!("3".to_string());
                    },
                    None => {
                        // Unknown macro: emit its name as upright text.
                        if has_base {
                            emit!();
                        }
                        atom.push_str(&name);
                        has_base = true;
                    },
                }
            },
            // Micro sign / Greek mu behaves as a prefix (glues to next unit).
            '\u{00B5}' | '\u{03BC}' => {
                if has_base {
                    emit!();
                    has_base = false;
                }
                atom.push(c);
                i += 1;
            },
            c if c.is_ascii_digit() => {
                // A bare digit run after a unit is an exponent.
                emit!();
                has_base = false;
                let start = i;
                while i < n && chars[i].is_ascii_digit() {
                    i += 1;
                }
                push_sup!(chars[start..i].iter().collect());
            },
            c if c.is_alphabetic() => {
                if has_base {
                    emit!();
                }
                while i < n
                    && chars[i].is_alphabetic()
                    && chars[i] != '\u{00B5}'
                    && chars[i] != '\u{03BC}'
                {
                    atom.push(chars[i]);
                    i += 1;
                }
                has_base = true;
            },
            // Other glyphs (°, ℃, %, …): standalone upright factor.
            _ => {
                if has_base {
                    emit!();
                }
                atom.push(c);
                has_base = true;
                i += 1;
            },
        }
    }
    emit!();
    let _ = has_base;
    items
}

/// Read an exponent after a `^`: either a `{...}` group or a run of digits /
/// sign characters.
fn read_exponent(chars: &[char], i: &mut usize) -> String {
    let n = chars.len();
    if *i < n && chars[*i] == '{' {
        *i += 1;
        let start = *i;
        let mut depth = 1usize;
        while *i < n {
            match chars[*i] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let exp: String = chars[start..*i].iter().collect();
                        *i += 1;
                        return exp;
                    }
                },
                _ => {},
            }
            *i += 1;
        }
        return chars[start..*i].iter().collect();
    }
    let start = *i;
    while *i < n && matches!(chars[*i], '0'..='9' | '+' | '-') {
        *i += 1;
    }
    if *i == start && *i < n {
        *i += 1; // consume at least one character
    }
    chars[start..*i].iter().collect()
}

/// Map a siunitx unit / prefix macro name to its replacement.
fn unit_macro(name: &str) -> Option<UnitTok> {
    use UnitTok::{Base, Cubed, Per, Prefix, Squared};
    Some(match name {
        // --- operators / exponents ---
        "per" => Per,
        "squared" => Squared,
        "cubed" => Cubed,
        // --- SI prefixes ---
        "yotta" => Prefix("Y"),
        "zetta" => Prefix("Z"),
        "exa" => Prefix("E"),
        "peta" => Prefix("P"),
        "tera" => Prefix("T"),
        "giga" => Prefix("G"),
        "mega" => Prefix("M"),
        "kilo" => Prefix("k"),
        "hecto" => Prefix("h"),
        "deca" | "deka" => Prefix("da"),
        "deci" => Prefix("d"),
        "centi" => Prefix("c"),
        "milli" => Prefix("m"),
        "micro" => Prefix("\u{00B5}"),
        "nano" => Prefix("n"),
        "pico" => Prefix("p"),
        "femto" => Prefix("f"),
        "atto" => Prefix("a"),
        "zepto" => Prefix("z"),
        "yocto" => Prefix("y"),
        // --- base units ---
        "gram" => Base("g"),
        "metre" | "meter" => Base("m"),
        "second" => Base("s"),
        "ampere" => Base("A"),
        "kelvin" => Base("K"),
        "mole" => Base("mol"),
        "candela" => Base("cd"),
        // --- derived / common units ---
        "hertz" => Base("Hz"),
        "newton" => Base("N"),
        "pascal" => Base("Pa"),
        "joule" => Base("J"),
        "watt" => Base("W"),
        "coulomb" => Base("C"),
        "volt" => Base("V"),
        "farad" => Base("F"),
        "ohm" => Base("\u{03A9}"),
        "siemens" => Base("S"),
        "weber" => Base("Wb"),
        "tesla" => Base("T"),
        "henry" => Base("H"),
        "celsius" => Base("\u{00B0}C"),
        // `\degreeCelsius` is the dedicated single-glyph unit ℃ (U+2103),
        // distinct from `\celsius` (which composes ° + C).
        "degreeCelsius" => Base("\u{2103}"),
        "lumen" => Base("lm"),
        "lux" => Base("lx"),
        "becquerel" => Base("Bq"),
        "gray" => Base("Gy"),
        "sievert" => Base("Sv"),
        "katal" => Base("kat"),
        "radian" => Base("rad"),
        "steradian" => Base("sr"),
        "litre" | "liter" => Base("L"),
        "degree" => Base("\u{00B0}"),
        "arcminute" => Base("\u{2032}"),
        "arcsecond" => Base("\u{2033}"),
        "percent" => Base("%"),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Angles
// ---------------------------------------------------------------------------

/// Translate an `\ang{...}` body: `30` → `30°`, `1;2;3` → `1°2′3″`.
#[must_use]
pub fn ang_to_latex(body: &str) -> String {
    let body = body.trim();
    if body.contains(';') {
        let mut out = String::new();
        for (idx, part) in body.split(';').enumerate() {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push_str(&format_number(part));
            out.push(match idx {
                0 => '\u{00B0}', // °
                1 => '\u{2032}', // ′
                _ => '\u{2033}', // ″
            });
        }
        out
    } else if body.is_empty() {
        String::new()
    } else {
        format!("{}\u{00B0}", format_number(body))
    }
}

// ---------------------------------------------------------------------------
// Ranges and lists
// ---------------------------------------------------------------------------

/// `\numrange{a}{b}` → `a–b` (en dash).
#[must_use]
pub fn numrange_to_latex(a: &str, b: &str) -> String {
    format!(
        "{}\\text{{\u{2013}}}{}",
        format_number(a.trim()),
        format_number(b.trim())
    )
}

/// `\numlist{a;b;c}` → `a, b and c` (best-effort).
#[must_use]
pub fn numlist_to_latex(body: &str) -> String {
    let items: Vec<String> = body
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(format_number)
        .collect();
    join_list(&items)
}

/// `\SIlist{a;b;c}{unit}` → `a u, b u and c u` (best-effort).
fn silist_to_latex(body: &str, unit: &str) -> String {
    let u = unit_to_latex(unit);
    let items: Vec<String> = body
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if u.is_empty() {
                format_number(s)
            } else {
                format!("{}\\,{u}", format_number(s))
            }
        })
        .collect();
    join_list(&items)
}

/// Join formatted items with commas and a trailing "and".
fn join_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => {
            let mut out = head.join("\\text{, }");
            out.push_str("\\text{ and }");
            out.push_str(last);
            out
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Push text into a `\mathrm{}` body, escaping LaTeX specials.
fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '%' | '&' | '_' | '#' | '$' => {
                out.push('\\');
                out.push(c);
            },
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::convert::from_latex;

    fn mathml(latex: &str) -> String {
        from_latex(latex)
            .expect("parse should succeed")
            .to_mathml()
            .expect("mathml render should succeed")
    }

    #[test]
    fn num_groups_long_integer() {
        let ml = mathml(r"\num{12345}");
        assert!(ml.contains("<mn>12</mn>"), "expected first group in {ml}");
        assert!(ml.contains("<mn>345</mn>"), "expected second group in {ml}");
        assert!(ml.contains("<mspace"), "expected thin space in {ml}");
    }

    #[test]
    fn num_short_integer_not_grouped() {
        let ml = mathml(r"\num{1234}");
        assert!(
            ml.contains("<mn>1234</mn>"),
            "expected ungrouped number in {ml}"
        );
        assert!(
            !ml.contains("<mspace"),
            "did not expect a thin space in {ml}"
        );
    }

    #[test]
    fn num_scientific_notation() {
        let ml = mathml(r"\num{1.23e4}");
        assert!(ml.contains("<mn>1.23</mn>"), "expected mantissa in {ml}");
        assert!(ml.contains('\u{00D7}'), "expected × in {ml}");
        assert!(
            ml.contains("<msup>"),
            "expected exponent superscript in {ml}"
        );
        assert!(ml.contains("<mn>10</mn>") && ml.contains("<mn>4</mn>"));
    }

    #[test]
    fn num_decimal_and_sign() {
        let ml = mathml(r"\num{-3.14}");
        assert!(ml.contains("<mn>3.14</mn>"), "expected decimal in {ml}");
        assert!(
            ml.contains('\u{2212}') || ml.contains('-'),
            "expected minus in {ml}"
        );
    }

    #[test]
    fn num_concise_uncertainty() {
        let ml = mathml(r"\num{1.2(3)}");
        assert!(ml.contains("<mn>1.2</mn>"));
        assert!(ml.contains('(') && ml.contains(')'));
        assert!(ml.contains("<mn>3</mn>"));
    }

    #[test]
    fn num_pm_uncertainty() {
        let ml = mathml(r"\num{1.2 +- 0.1}");
        assert!(ml.contains('\u{00B1}'), "expected ± in {ml}");
        let ml2 = mathml(r"\num{1.2\pm0.1}");
        assert!(ml2.contains('\u{00B1}'), "expected ± in {ml2}");
    }

    #[test]
    fn si_quantity_with_unit_macros() {
        let ml = mathml(r"\SI{9.81}{\metre\per\second\squared}");
        assert!(ml.contains("<mn>9.81</mn>"), "expected number in {ml}");
        assert!(
            ml.contains("mathvariant=\"normal\""),
            "expected upright units in {ml}"
        );
        assert!(
            ml.contains(">m</mi>") && ml.contains(">s</mi>"),
            "expected m and s in {ml}"
        );
        assert!(ml.contains('/'), "expected division slash in {ml}");
        assert!(ml.contains("<msup>"), "expected squared exponent in {ml}");
    }

    #[test]
    fn si_quantity_with_literal_unit() {
        let ml = mathml(r"\SI{5}{kg.m/s^2}");
        assert!(ml.contains("<mn>5</mn>"));
        assert!(
            ml.contains("mathvariant=\"normal\""),
            "expected upright units in {ml}"
        );
        assert!(ml.contains('/'), "expected slash in {ml}");
        assert!(ml.contains("<msup>"));
    }

    #[test]
    fn si_unit_thin_space_between_factors() {
        let ml = mathml(r"\si{kg m s^{-1}}");
        assert!(ml.contains("mathvariant=\"normal\""));
        assert!(
            ml.contains("<mspace"),
            "expected thin space between units in {ml}"
        );
    }

    #[test]
    fn si_prefixed_unit() {
        let ml = mathml(r"\si{\kilo\gram}");
        assert!(ml.contains("mathvariant=\"normal\""));
        assert!(
            ml.contains(">k</mi>") || ml.contains("kg"),
            "expected kg in {ml}"
        );
        assert!(ml.contains(">g</mi>") || ml.contains("kg"));
    }

    #[test]
    fn qty_v3_quantity() {
        let ml = mathml(r"\qty{3}{\nano\metre}");
        assert!(ml.contains("<mn>3</mn>"));
        assert!(ml.contains("mathvariant=\"normal\""));
        assert!(ml.contains(">n</mi>") || ml.contains("nm"));
    }

    #[test]
    fn qty_single_arg_keeps_physics_parens() {
        // Arity-based coexistence: `\qty{x}` keeps physics' auto-sized parens.
        let ml = mathml(r"\qty{a+b}");
        assert!(
            ml.contains('(') && ml.contains(')'),
            "expected parens in {ml}"
        );
    }

    #[test]
    fn unit_v3_spelling() {
        let ml = mathml(r"\unit{\metre\per\second}");
        assert!(ml.contains("mathvariant=\"normal\""));
        assert!(ml.contains('/'));
    }

    #[test]
    fn ang_degrees() {
        let ml = mathml(r"\ang{30}");
        assert!(ml.contains("<mn>30</mn>"));
        assert!(ml.contains('\u{00B0}'), "expected ° in {ml}");
    }

    #[test]
    fn ang_dms() {
        let ml = mathml(r"\ang{1;2;3}");
        assert!(ml.contains('\u{00B0}'), "expected ° in {ml}");
        assert!(ml.contains('\u{2032}'), "expected ′ in {ml}");
        assert!(ml.contains('\u{2033}'), "expected ″ in {ml}");
    }

    #[test]
    fn numrange_en_dash() {
        let ml = mathml(r"\numrange{1}{10}");
        assert!(ml.contains("<mn>1</mn>"));
        assert!(ml.contains("<mn>10</mn>"));
        assert!(ml.contains('\u{2013}'), "expected en dash in {ml}");
    }

    #[test]
    fn si_range_with_unit() {
        let ml = mathml(r"\SIrange{1}{10}{\metre}");
        assert!(ml.contains('\u{2013}'), "expected en dash in {ml}");
        assert!(ml.contains(">m</mi>"), "expected unit m in {ml}");
        assert!(ml.contains("mathvariant=\"normal\""));
    }

    #[test]
    fn numlist_best_effort() {
        let ml = mathml(r"\numlist{1;2;3}");
        assert!(
            ml.contains("<mn>1</mn>") && ml.contains("<mn>2</mn>") && ml.contains("<mn>3</mn>")
        );
        assert!(ml.contains("and"), "expected 'and' in list {ml}");
    }

    #[test]
    fn options_are_ignored() {
        let ml = mathml(r"\num[round-mode=places]{3.14159}");
        assert!(
            ml.contains("<mn>3.14159</mn>"),
            "expected number despite options in {ml}"
        );
    }

    #[test]
    fn sisetup_is_ignored() {
        let ml = mathml(r"\sisetup{round-mode=places}\num{5}");
        assert!(ml.contains("<mn>5</mn>"));
    }

    #[test]
    fn malformed_does_not_panic() {
        let _ = from_latex(r"\num{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\SI{1}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\si{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ang{}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\numrange{1}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\si{\per\squared}").and_then(|c| c.to_mathml());
    }

    #[test]
    fn degree_celsius_unit() {
        // `\degreeCelsius` renders the single upright glyph ℃ (U+2103).
        let ml = mathml(r"\SI{37}{\degreeCelsius}");
        assert!(ml.contains("<mn>37</mn>"), "expected number in {ml}");
        assert!(ml.contains('\u{2103}'), "expected ℃ in {ml}");
        assert!(
            ml.contains("mathvariant=\"normal\""),
            "expected upright unit in {ml}"
        );
        // Bare `\si{\degreeCelsius}` works too, and `\celsius` still renders.
        assert!(mathml(r"\si{\degreeCelsius}").contains('\u{2103}'));
        let cel = mathml(r"\si{\celsius}");
        assert!(
            cel.contains('\u{2103}') || cel.contains('\u{00B0}'),
            "expected a celsius glyph in {cel}"
        );
    }

    #[test]
    fn num_leading_zero_for_bare_decimal() {
        // `\num{.5}` → `0.5` (siunitx supplies the omitted leading zero).
        let ml = mathml(r"\num{.5}");
        assert!(ml.contains("<mn>0.5</mn>"), "expected 0.5 in {ml}");
        let neg = mathml(r"\num{-.5}");
        assert!(
            neg.contains("<mn>0.5</mn>"),
            "expected -0.5 mantissa in {neg}"
        );
    }

    #[test]
    fn si_range_empty_unit_no_trailing_thin_space() {
        // `\SIrange{1}{10}{}`: an empty unit must not leave a trailing `\,`.
        let ml = mathml(r"\SIrange{1}{10}{}");
        assert!(ml.contains("<mn>1</mn>") && ml.contains("<mn>10</mn>"));
        assert!(ml.contains('\u{2013}'), "expected en dash in {ml}");
        assert!(
            !ml.contains("<mspace"),
            "did not expect a trailing thin space in {ml}"
        );
    }

    #[test]
    fn per_squared_no_dangling_slash() {
        // `\si{\per\squared}` must not emit a bare `/^2`; the dangling slash and
        // orphan exponent are dropped.
        let ml = mathml(r"\si{\per\squared}");
        assert!(
            !ml.contains("<mo>/</mo>"),
            "did not expect a dangling slash in {ml}"
        );
    }
}
