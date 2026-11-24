//! `\pu{...}`: a port of mhchem's `pu`, `pu-2` and `pu-9,9` state machines
//! (KaTeX 0.16 `contrib/mhchem`) and of the `texify` rules they output
//! through, so a body reads as mhchem reads it, failures included:
//! `\pu{m_2}` is `\mathrm{m_^{2}}` there too, which TeX rejects.
//!
//! * `pu` reads a number first: a power (`10^3`, `^2`), or a value with an
//!   uncertainty and an exponent (`1.2(3)`, `1.2e3`, `3 \times 10^8`); then
//!   operators (`+-`, `=`); then the unit text, split once at `/` or `//`.
//! * `pu-2` reads unit text: spaces and `*`/`.` separate units, a number
//!   after unit text is its exponent (`m2`, `s-1`, `m^22`), and anything
//!   else (`_`, `^`, control sequences) is copied into the `\mathrm{}`.
//! * `pu-9,9` groups the digits of a long number in threes.
//!
//! The one departure: a bare `#`, `%`, `&` or `$` in unit text is escaped,
//! where mhchem copies it and the TeX then fails.

/// How deeply a fraction or braced unit is re-read as a `\pu` body. mhchem
/// recurses without a bound; past this depth the text is written as a plain
/// unit, so adversarial input cannot exhaust the stack.
const MAX_DEPTH: usize = 16;

/// Translate a `\pu{...}` body into standard LaTeX, as mhchem writes it.
#[must_use]
pub fn pu_to_latex(body: &str) -> String {
    let mut input = String::with_capacity(body.len());
    for c in body.chars() {
        match c {
            '\n' => input.push(' '),
            '\u{2212}' | '\u{2013}' | '\u{2014}' | '\u{2010}' => input.push('-'),
            '\u{2026}' => input.push_str("..."),
            _ => input.push(c),
        }
    }
    let mut out = Out::default();
    pu(&mut out, &input, 0);
    if out.text.is_empty() {
        return out.text;
    }
    format!("{{{}}}", out.text)
}

/// Output text and the number of mhchem output items it holds (mhchem
/// writes a `/` between short parts and a spaced `\,/\,` otherwise).
#[derive(Default)]
struct Out {
    text: String,
    items: usize,
}

impl Out {
    fn item(&mut self, text: &str) {
        self.text.push_str(text);
        self.items += 1;
    }

    fn append(&mut self, other: Out) {
        self.text.push_str(&other.text);
        self.items += other.items;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PuState {
    /// Start, or after an operator.
    Start,
    /// After a number.
    Number,
    /// In unit text.
    Unit,
    /// After `/` or `//`.
    Slash,
    /// In the denominator.
    Denominator,
}

/// mhchem's `pu` machine.
fn pu(out: &mut Out, input: &str, depth: usize) {
    let mut state = PuState::Start;
    let mut unit = String::new();
    let mut denominator = String::new();
    let mut double_slash = false;
    let groups = Groups::new(input);
    let mut rest = input;
    loop {
        if rest.is_empty() {
            pu_output(out, &mut unit, &mut denominator, double_slash, depth);
            return;
        }
        // `space$`: one trailing space.
        if let Some(c) = rest.chars().next()
            && c.is_whitespace()
            && rest.len() == c.len_utf8()
        {
            pu_output(out, &mut unit, &mut denominator, double_slash, depth);
            out.item("~");
            return;
        }
        let in_number_part = matches!(state, PuState::Start | PuState::Number);
        // A run of spaces is skipped (`space`) or read as unit text whole;
        // its last space is kept when it ends the input (`space$`). Taking
        // the run at once keeps long runs linear.
        let spaces = rest.len() - rest.trim_start().len();
        if spaces > 0 {
            let run = if spaces == rest.len() {
                spaces - rest.chars().next_back().map_or(0, char::len_utf8)
            } else {
                spaces
            };
            let after = &rest[spaces..];
            let starts_marker = state == PuState::Start
                && ["*", "x", "\\times", "\u{00D7}"]
                    .iter()
                    .any(|op| after.starts_with(op));
            let starts_slash = state == PuState::Unit && after.starts_with('/');
            if starts_marker && let Some(len) = number_exponent(out, rest) {
                rest = &rest[len..];
                state = PuState::Number;
                continue;
            }
            if run > 0 && !starts_slash {
                match state {
                    PuState::Start | PuState::Number => {},
                    PuState::Unit => unit.push_str(&rest[..run]),
                    PuState::Slash | PuState::Denominator => {
                        denominator.push_str(&rest[..run]);
                        state = PuState::Denominator;
                    },
                }
                rest = &rest[run..];
                continue;
            }
        }
        if in_number_part
            && let Some(len) = ["\\{", "[", "(", ")", "]", "\\}"]
                .iter()
                .find(|open| rest.starts_with(**open))
                .map(|open| open.len())
        {
            out.item(&rest[..len]);
            rest = &rest[len..];
            continue;
        }
        if state == PuState::Start {
            if let Some(len) = number_power(out, rest) {
                rest = &rest[len..];
                state = PuState::Number;
                continue;
            }
            if let Some(len) = number_exponent(out, rest) {
                rest = &rest[len..];
                state = PuState::Number;
                continue;
            }
        }
        if in_number_part {
            if let Some(c) = rest.chars().next()
                && c.is_whitespace()
            {
                rest = &rest[c.len_utf8()..];
                continue;
            }
            if let Some(len) = pm_operator(rest) {
                out.item(" {}\\pm{} ");
                rest = &rest[len..];
                state = PuState::Start;
                continue;
            }
            if let Some(len) = operator_len(rest) {
                out.item(&rest[..len]);
                rest = &rest[len..];
                state = PuState::Start;
                continue;
            }
        }
        if state == PuState::Unit
            && let Some((len, double)) = slash(rest)
        {
            double_slash = double;
            rest = &rest[len..];
            state = PuState::Slash;
            continue;
        }
        // `{...}|else`
        let at = input.len() - rest.len();
        let len = groups
            .len_at(at)
            .unwrap_or_else(|| rest.chars().next().map_or(1, char::len_utf8));
        let taken = &rest[..len];
        rest = &rest[len..];
        match state {
            PuState::Start | PuState::Unit => {
                unit.push_str(taken);
                state = PuState::Unit;
            },
            PuState::Number => {
                out.item("~");
                unit.push_str(taken);
                state = PuState::Unit;
            },
            PuState::Slash | PuState::Denominator => {
                denominator.push_str(taken);
                state = PuState::Denominator;
            },
        }
    }
}

/// The `pu` machine's `output` action.
fn pu_output(
    out: &mut Out,
    unit: &mut String,
    denominator: &mut String,
    double_slash: bool,
    depth: usize,
) {
    let d = degrees(unwrap_group(unit));
    let q = degrees(unwrap_group(denominator));
    if q.is_empty() {
        pu2(out, &d, depth);
    } else if depth >= MAX_DEPTH {
        pu2(out, &d, depth);
        out.item("/");
        pu2(out, &q, depth);
    } else {
        let mut num = Out::default();
        pu(&mut num, &d, depth + 1);
        let mut den = Out::default();
        pu(&mut den, &q, depth + 1);
        if double_slash {
            let frac = format!("\\frac{{{}}}{{{}}}", num.text, den.text);
            // mhchem writes the fraction four times, once per style; a
            // fraction already holding such a choice is written once, so
            // nested `//` stays linear instead of growing fourfold a level.
            if frac.contains("\\mathchoice") {
                out.item(&frac);
            } else {
                out.item(&format!(
                    "\\mathchoice{{\\textstyle{frac}}}{{{frac}}}{{{frac}}}{{{frac}}}"
                ));
            }
        } else {
            let spaced = num.items > 1 || den.items > 1;
            out.append(num);
            out.item(if spaced { "\\,/\\," } else { "/" });
            out.append(den);
        }
    }
    unit.clear();
    denominator.clear();
}

/// mhchem's degree spellings: `°C`, `^oC` and `^{o}C` (and `F`).
fn degrees(text: &str) -> String {
    let mut s = text.to_owned();
    if s.contains(['\u{00B0}', '^']) {
        for unit in ["C", "F"] {
            for spelling in ["\u{00B0}", "^o", "^{o}"] {
                s = s.replace(
                    &format!("{spelling}{unit}"),
                    &format!("{{}}^{{\\circ}}{unit}"),
                );
            }
        }
    }
    s
}

/// mhchem's `pu-2` machine: unit text.
fn pu2(out: &mut Out, input: &str, depth: usize) {
    let mut rm = String::new();
    // mhchem's state `1`: after unit text, where a number is an exponent.
    let mut after_unit = false;
    let groups = Groups::new(input);
    let mut rest = input;
    while !rest.is_empty() {
        // `*`: `\s*[*.]\s*`
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix(['*', '.']) {
            pu2_output(out, &mut rm, depth);
            out.item("\\mkern1mu{\\cdot}\\mkern1mu ");
            after_unit = false;
            rest = after.trim_start();
            continue;
        }
        // `\x`: a control word (and the spaces after it), or `\_`, `\&`,
        // `\{`, `\}`, `\%`. Any other control symbol is copied the same way:
        // mhchem copies its two characters one by one.
        if let Some(after) = rest.strip_prefix('\\') {
            let len = match after.chars().next() {
                Some(c) if c.is_ascii_alphabetic() => {
                    let word = after.bytes().take_while(u8::is_ascii_alphabetic).count();
                    let after_word = &after[word..];
                    word + (after_word.len() - after_word.trim_start().len())
                },
                Some(c) => c.len_utf8(),
                None => 0,
            };
            rm.push_str(&rest[..=len]);
            rest = &after[len..];
            continue;
        }
        let spaces = rest.len() - trimmed.len();
        if spaces > 0 {
            pu2_output(out, &mut rm, depth);
            for _ in rest[..spaces].chars() {
                out.item("\\mkern3mu ");
            }
            after_unit = false;
            rest = trimmed;
            continue;
        }
        if after_unit && let Some(after) = rest.strip_prefix('^') {
            if let Some(len) = groups.len_at(input.len() - after.len()) {
                rm.push('^');
                rm.push_str(&after[..len]);
                rest = &after[len..];
                continue;
            }
            let sign = usize::from(after.starts_with('-'));
            let digits = after[sign..].bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                push_braced_script(&mut rm, &after[..sign + digits]);
                rest = &after[sign + digits..];
                continue;
            }
        }
        if let Some(len) = decimal_len(rest) {
            if after_unit {
                push_braced_script(&mut rm, &rest[..len]);
            } else {
                rm.push_str(&rest[..len]);
            }
            after_unit = false;
            rest = &rest[len..];
            continue;
        }
        if let Some(len) = groups.len_at(input.len() - rest.len()) {
            rm.push_str(&rest[..len]);
            rest = &rest[len..];
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            super::push_escaped_char(&mut rm, c);
            rest = &rest[c.len_utf8()..];
        }
        after_unit = true;
    }
    pu2_output(out, &mut rm, depth);
}

fn push_braced_script(rm: &mut String, exponent: &str) {
    rm.push_str("^{");
    rm.push_str(exponent);
    rm.push('}');
}

/// The `pu-2` machine's `output` action: a unit that is one braced group is
/// re-read as a `\pu` body.
fn pu2_output(out: &mut Out, rm: &mut String, depth: usize) {
    if rm.is_empty() {
        return;
    }
    let inner = unwrap_group(rm);
    if inner.len() != rm.len() && depth < MAX_DEPTH {
        let mut nested = Out::default();
        pu(&mut nested, inner, depth + 1);
        out.append(nested);
    } else {
        out.item(&format!("\\mathrm{{{rm}}}"));
    }
    rm.clear();
}

/// The content of `text` when all of it is one `{...}` group (mhchem's
/// `{(...)}` pattern with nothing left over), else `text`.
fn unwrap_group(text: &str) -> &str {
    match group_len(text) {
        Some(len) if len == text.len() => &text[1..len - 1],
        _ => text,
    }
}

/// The balanced `{...}` groups of a text, found in one pass, so that reading
/// many unbalanced `{` stays linear.
struct Groups {
    /// The byte offset of each `{` that is closed, and of its `}`.
    ends: std::collections::HashMap<usize, usize>,
}

impl Groups {
    fn new(text: &str) -> Self {
        let mut ends = std::collections::HashMap::new();
        let mut open = Vec::new();
        for (i, b) in text.bytes().enumerate() {
            match b {
                b'{' => open.push(i),
                b'}' => {
                    if let Some(start) = open.pop() {
                        ends.insert(start, i);
                    }
                },
                _ => {},
            }
        }
        Self { ends }
    }

    /// The length of the group that starts at byte `at`, if one does.
    fn len_at(&self, at: usize) -> Option<usize> {
        self.ends.get(&at).map(|end| end + 1 - at)
    }
}

/// The length of the balanced `{...}` group at the start of `s` (mhchem's
/// `{...}` pattern: braces are counted, backslashes are not special).
fn group_len(s: &str) -> Option<usize> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            },
            _ => {},
        }
    }
    None
}

fn digits_len(s: &str) -> usize {
    s.bytes().take_while(u8::is_ascii_digit).count()
}

/// mhchem's `-9.,9`: `[+-]?(?:[0-9]+(?:[,.][0-9]+)?|[0-9]*(?:\.[0-9]+))`.
fn decimal_len(s: &str) -> Option<usize> {
    let sign = usize::from(s.starts_with(['+', '-']));
    let len = unsigned_decimal_len(&s[sign..]);
    (len > 0).then_some(sign + len)
}

/// `[0-9]+(?:[,.][0-9]+)?|[0-9]*(?:\.[0-9]+)`; 0 when nothing matches
/// (which is also the empty match `(-)(9)^(-9)` allows).
fn unsigned_decimal_len(s: &str) -> usize {
    let b = s.as_bytes();
    let int = digits_len(s);
    if int > 0 {
        if matches!(b.get(int), Some(b',' | b'.')) {
            let frac = digits_len(&s[int + 1..]);
            if frac > 0 {
                return int + 1 + frac;
            }
        }
        return int;
    }
    if b.first() == Some(&b'.') {
        let frac = digits_len(&s[1..]);
        if frac > 0 {
            return 1 + frac;
        }
    }
    0
}

/// mhchem's sign: `\+\-|\+\/\-|\+|\-|\\pm\s?`.
fn sign_len(s: &str) -> usize {
    if s.starts_with("+-") {
        2
    } else if s.starts_with("+/-") {
        3
    } else if s.starts_with(['+', '-']) {
        1
    } else if let Some(after) = s.strip_prefix("\\pm") {
        3 + after
            .chars()
            .next()
            .filter(|c| c.is_whitespace())
            .map_or(0, char::len_utf8)
    } else {
        0
    }
}

fn push_sign(out: &mut Out, sign: &str) {
    if sign == "+-" || sign == "+/-" {
        out.item("\\pm ");
    } else if !sign.is_empty() {
        out.item(sign);
    }
}

/// mhchem's exponent: `[+\-]?[0-9]+|\{[+\-]?[0-9]+\}`.
fn exponent_len(s: &str) -> usize {
    let (inner, braced) = match s.strip_prefix('{') {
        Some(inner) => (inner, true),
        None => (s, false),
    };
    let sign = usize::from(inner.starts_with(['+', '-']));
    let digits = digits_len(&inner[sign..]);
    if digits == 0 {
        return 0;
    }
    if !braced {
        return sign + digits;
    }
    if inner[sign + digits..].starts_with('}') {
        sign + digits + 2
    } else {
        0
    }
}

/// mhchem's `(-)(9)^(-9)` and `number^` action: a number (possibly empty)
/// raised to a power, `10^3`, `-2^{10}`, `^2`.
fn number_power(out: &mut Out, s: &str) -> Option<usize> {
    let sign = sign_len(s);
    let number = unsigned_decimal_len(&s[sign..]);
    let caret = sign + number;
    let after = s[caret..].strip_prefix('^')?;
    let exponent = exponent_len(after);
    if exponent == 0 {
        return None;
    }
    push_sign(out, &s[..sign]);
    pu_digits(out, &s[sign..caret]);
    out.item(&format!("^{{{}}}", &after[..exponent]));
    Some(caret + 1 + exponent)
}

/// mhchem's `(-)(9.,9)(e)(99)` and `enumber` action: a value with an optional
/// uncertainty and exponent, `1.2(3)`, `1.2e3`, `3 \times 10^8`, `-`.
fn number_exponent(out: &mut Out, s: &str) -> Option<usize> {
    let sign = sign_len(s);
    let mut at = sign;
    let number = unsigned_decimal_len(&s[at..]);
    at += number;
    let mut uncertainty = 0;
    if let Some(inner) = s[at..].strip_prefix('(') {
        let len = unsigned_decimal_len(inner);
        if len > 0 && inner[len..].starts_with(')') {
            uncertainty = len + 2;
        }
    }
    let uncertainty_at = at;
    at += uncertainty;
    let mut marker: Option<&str> = None;
    let mut exponent: Option<&str> = None;
    if let Some((marker_len, op)) = exponent_marker(&s[at..]) {
        let len = exponent_len(&s[at + marker_len..]);
        if len > 0 {
            marker = Some(op);
            exponent = Some(&s[at + marker_len..at + marker_len + len]);
            at += marker_len + len;
        }
    }
    if at == 0 {
        return None;
    }
    push_sign(out, &s[..sign]);
    if number > 0 {
        pu_digits(out, &s[sign..sign + number]);
        if uncertainty > 0 {
            let text = &s[uncertainty_at..uncertainty_at + uncertainty];
            if text.contains([',', '.']) {
                pu_digits(out, text);
            } else {
                out.item(text);
            }
        }
        if let Some(op) = marker {
            if op == "e" || op.starts_with('*') {
                out.item("\\cdot ");
            } else {
                out.item("\\times ");
            }
        }
    }
    if let Some(exponent) = exponent {
        out.item(&format!("10^{{{exponent}}}"));
    }
    Some(at)
}

/// `[eE]|\s*(\*|x|\\times|×)\s*10\^`: the length matched and the marker
/// (`e`, `E`, or the trimmed operator).
fn exponent_marker(s: &str) -> Option<(usize, &str)> {
    if s.starts_with(['e', 'E']) {
        return Some((1, &s[..1]));
    }
    let trimmed = s.trim_start();
    let op = ["*", "x", "\\times", "\u{00D7}"]
        .into_iter()
        .find(|op| trimmed.starts_with(op))?;
    let after_op = trimmed[op.len()..].trim_start();
    let after = after_op.strip_prefix("10^")?;
    Some((s.len() - after.len(), op))
}

/// mhchem's `pm-operator`: `\pm`, `$\pm$`, `+-`, `+/-`.
fn pm_operator(s: &str) -> Option<usize> {
    ["\\pm", "$\\pm$", "+-", "+/-"]
        .into_iter()
        .find(|op| s.starts_with(op))
        .map(str::len)
}

/// mhchem's `operator`: `+`, or `-`, `=`, `<`, `>`, `<<`, `>>`, `\approx`
/// before a space, the end or a number. The `pu` machine copies it as is.
fn operator_len(s: &str) -> Option<usize> {
    if s.starts_with('+') {
        return Some(1);
    }
    let followed = |len: usize| {
        let after = &s[len..];
        let unsigned = after.strip_prefix('-').unwrap_or(after);
        after.is_empty()
            || after.starts_with(char::is_whitespace)
            || unsigned.starts_with(|c: char| c.is_ascii_digit())
    };
    ["-", "=", "<", ">", "<<", ">>", "\\approx", "$\\approx$"]
        .into_iter()
        .find(|op| s.starts_with(op) && followed(op.len()))
        .map(str::len)
}

/// mhchem's `/` and `//` (with the spaces around them).
fn slash(s: &str) -> Option<(usize, bool)> {
    let trimmed = s.trim_start();
    let (after, double) = if let Some(after) = trimmed.strip_prefix("//") {
        (after, true)
    } else {
        (trimmed.strip_prefix('/')?, false)
    };
    let after = after.trim_start();
    Some((s.len() - after.len(), double))
}

/// mhchem's `pu-9,9` machine: a number's digits, grouped in threes with
/// `\mkern2mu` when a part has more than four, and a decimal comma `{,}`.
fn pu_digits(out: &mut Out, text: &str) {
    if text.is_empty() {
        return;
    }
    match text.find([',', '.']) {
        Some(at) => {
            group_integer(out, &text[..at]);
            out.item(if text[at..].starts_with(',') {
                "{,}"
            } else {
                "."
            });
            group_fraction(out, &text[at + 1..]);
        },
        None => group_integer(out, text),
    }
}

const THOUSANDS: &str = "\\mkern2mu ";

fn group_integer(out: &mut Out, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 4 {
        out.item(text);
        return;
    }
    let first = match chars.len() % 3 {
        0 => 3,
        n => n,
    };
    out.item(&chars[..first].iter().collect::<String>());
    for group in chars[first..].chunks(3) {
        out.item(THOUSANDS);
        out.item(&group.iter().collect::<String>());
    }
}

fn group_fraction(out: &mut Out, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 4 {
        out.item(text);
        return;
    }
    let full = chars.len() - 3;
    let mut i = 0;
    while i < full {
        out.item(&chars[i..i + 3].iter().collect::<String>());
        out.item(THOUSANDS);
        i += 3;
    }
    out.item(&chars[i..].iter().collect::<String>());
}

#[cfg(test)]
mod tests {
    use super::pu_to_latex;
    use crate::error::{Config, Strictness};

    /// `(body, what KaTeX 0.16.47's mhchem writes for \pu{body}, whether
    /// KaTeX then reads it)`.
    const KATEX: &[(&str, &str, bool)] = &[
        (
            r"3 \times 10^8 m s-1",
            r"{3\times 10^{8}~\mathrm{m}\mkern3mu \mathrm{s^{-1}}}",
            true,
        ),
        (
            r"6.02 \times 10^23 mol-1",
            r"{6.02\times 10^{23}~\mathrm{mol^{-1}}}",
            true,
        ),
        (r"10^3 m", r"{10^{3}~\mathrm{m}}", true),
        (r"1.2*10^3 m", r"{1.2\cdot 10^{3}~\mathrm{m}}", true),
        (r"^2 H", r"{^{2}~\mathrm{H}}", true),
        (r"10^{3}5", r"{10^{{3}}~\mathrm{5}}", true),
        (r"2^{10} B", r"{2^{{10}}~\mathrm{B}}", true),
        (r"1.2(3) m", r"{1.2(3)~\mathrm{m}}", true),
        (r"1.2(3)e-4 m", r"{1.2(3)\cdot 10^{-4}~\mathrm{m}}", true),
        (r"1.2e3 kg", r"{1.2\cdot 10^{3}~\mathrm{kg}}", true),
        (r"1E5 Pa", r"{1\times 10^{5}~\mathrm{Pa}}", true),
        (
            r"12345.67891 J",
            r"{12\mkern2mu 345.678\mkern2mu 91~\mathrm{J}}",
            true,
        ),
        (r"1,5 m", r"{1{,}5~\mathrm{m}}", true),
        (r"+-5 K", r"{\pm 5~\mathrm{K}}", true),
        (r"1 - 2 m", r"{1-2~\mathrm{m}}", true),
        (r"kJ/mol", r"{\mathrm{kJ}/\mathrm{mol}}", true),
        (
            r"kJ//mol",
            r"{\mathchoice{\textstyle\frac{\mathrm{kJ}}{\mathrm{mol}}}{\frac{\mathrm{kJ}}{\mathrm{mol}}}{\frac{\mathrm{kJ}}{\mathrm{mol}}}{\frac{\mathrm{kJ}}{\mathrm{mol}}}}",
            true,
        ),
        (
            r"J/(mol K)",
            r"{\mathrm{J}\,/\,(\mathrm{mol}\mkern3mu \mathrm{K)}}",
            true,
        ),
        (
            r"J K^-1 mol^-1",
            r"{\mathrm{J}\mkern3mu \mathrm{K^{-1}}\mkern3mu \mathrm{mol^{-1}}}",
            true,
        ),
        (
            r"kg m2 s-2",
            r"{\mathrm{kg}\mkern3mu \mathrm{m^{2}}\mkern3mu \mathrm{s^{-2}}}",
            true,
        ),
        (
            r"kg.m",
            r"{\mathrm{kg}\mkern1mu{\cdot}\mkern1mu \mathrm{m}}",
            true,
        ),
        (r"25 °C", r"{25~\mathrm{{}^{\circ}C}}", true),
        (r"a_b", r"{\mathrm{a_b}}", true),
        (r"x_a2", r"{\mathrm{x_a^{2}}}", true),
        (r"m\_2", r"{\mathrm{m\_^{2}}}", true),
        (r"{m}2", r"{\mathrm{{m}^{2}}}", true),
        (r"m^{2}", r"{\mathrm{m^{2}}}", true),
        (r"m_2", r"{\mathrm{m_^{2}}}", false),
        (r"m^2^3", r"{\mathrm{m^{2}^{3}}}", false),
        (r"m^-1^2", r"{\mathrm{m^{-1}^{2}}}", false),
        (r"m_", r"{\mathrm{m_}}", false),
    ];

    #[test]
    fn translation_matches_katex_mhchem() {
        for &(body, expected, _) in KATEX {
            assert_eq!(pu_to_latex(body), expected, "\\pu{{{body}}}");
        }
    }

    #[test]
    fn reads_where_katex_reads() {
        for &(body, _, katex_reads) in KATEX {
            let input = format!("\\pu{{{body}}}");
            let strict = crate::convert::from_latex_with(&input, &Config::default());
            assert_eq!(strict.is_ok(), katex_reads, "{input}");
            let lenient = crate::convert::from_latex_with(
                &input,
                &Config::default().with_strictness(Strictness::Lenient),
            )
            .unwrap();
            assert!(!lenient.to_latex().unwrap().is_empty(), "{input}");
        }
    }

    #[test]
    fn deep_groups_and_long_runs_stay_bounded() {
        let deep = format!("{}m{}", "{".repeat(5000), "}".repeat(5000));
        assert!(!pu_to_latex(&deep).is_empty());
        let slashes = "m/".repeat(5000);
        assert!(!pu_to_latex(&slashes).is_empty());
        // Found by fuzzing: each `//` level wrote its fraction four times.
        let fractions = "m//".repeat(40);
        assert!(pu_to_latex(&fractions).len() < 20_000);
        let spaces = format!("1{}x m", " ".repeat(100_000));
        assert!(!pu_to_latex(&spaces).is_empty());
        let open = format!("m{}", "{".repeat(100_000));
        assert!(!pu_to_latex(&open).is_empty());
    }
}
