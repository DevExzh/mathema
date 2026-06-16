//! `mhchem` package support: the `\ce{...}` chemical-equation mini-language and
//! `\pu{...}` physical units.
//!
//! Rather than introducing a dedicated [`crate::ast::compact::CompactNode`]
//! variant, this module **translates** the `\ce` / `\pu` body into an equivalent
//! standard-LaTeX math string. The parser then re-parses that string through its
//! normal path, so every existing layout / render / writer feature is reused for
//! free (subscripts, superscripts, extensible arrows, fractions, fences, ...).
//!
//! The translation is deliberately a *common subset* matching MathJax's mhchem
//! for the cases users hit most often. It is lenient: malformed input degrades
//! gracefully (best-effort literal emission) and never panics.
//!
//! # Grammar subset
//!
//! * Element / group counts become subscripts: `H2O` → `H₂O`, `Ca(OH)2`.
//! * Charges become superscripts: `SO4^2-`, `Na+`, `Cl-`, `Fe^3+`.
//! * Reaction arrows with optional `[above]` / `[above][below]` labels:
//!   `->`, `<-`, `<=>`, `<=>>`, `<<=>`, `<->`, `=>`.
//! * `+` between species is a spaced operator; leading numbers are
//!   stoichiometric coefficients (`2 H2O`, `1/2 O2`).
//! * States `(s) (l) (g) (aq)` render upright.
//! * Bonds `-` `=` `#` between atoms.
//! * Parentheses / brackets grouping with trailing subscripts.
//! * Embedded `$...$` math and nested `\ce{...}` (best-effort).
//! * Gas / precipitate markers: a standalone `^` → `↑`, `v` → `↓`.
//!
//! Deferred (documented, not implemented): isotope pre-scripts
//! `\ce{^{227}_{90}Th}`, electron dots, complex bond geometry / Lewis
//! structures, advanced `\pu` unit algebra.

/// Capture a balanced `{ ... }` group at the start of `input` (after optional
/// leading whitespace).
///
/// Returns `(rest_after_close_brace, body_without_braces)` or `None` when the
/// input does not begin with a brace group. Backslash escapes (`\{`, `\}`) are
/// honored so escaped braces do not unbalance the scan.
#[must_use]
pub fn capture_braced(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let rest = trimmed.strip_prefix('{')?;
    let mut depth = 1usize;
    let mut escaped = false;
    for (i, &b) in rest.as_bytes().iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' => escaped = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&rest[i + 1..], &rest[..i]));
                }
            },
            _ => {},
        }
    }
    None
}

/// Translate a `\ce{...}` body into an equivalent standard-LaTeX math string.
#[must_use]
pub fn ce_to_latex(body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut t = Translator {
        chars,
        i: 0,
        out: String::with_capacity(body.len() * 4 + 8),
        had_space: false,
        last: Kind::Start,
    };
    t.run();
    t.out
}

/// Translate a `\pu{...}` physical-units body into standard LaTeX.
///
/// Best-effort: a leading numeric token stays normal size; remaining
/// whitespace-separated tokens render upright with thin spaces between them.
#[must_use]
pub fn pu_to_latex(body: &str) -> String {
    let mut out = String::with_capacity(body.len() * 3 + 4);
    for (idx, tok) in body.split_whitespace().enumerate() {
        if idx > 0 {
            out.push_str("\\,");
        }
        if is_number(tok) {
            out.push_str(tok);
        } else {
            out.push_str("\\mathrm{");
            push_escaped_text(&mut out, tok);
            out.push('}');
        }
    }
    out
}

/// What the most recently emitted token was, for spacing / subscript decisions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Nothing emitted yet, or a position where a following number is a
    /// stoichiometric coefficient rather than a subscript.
    Start,
    /// A formula value (element run, count, charge, closing delimiter): a
    /// following digit run is a subscript.
    Atom,
    /// An operator / arrow / bond: manages its own spacing.
    Op,
}

struct Translator {
    chars: Vec<char>,
    i: usize,
    out: String,
    /// Whitespace was seen since the last emitted token.
    had_space: bool,
    last: Kind,
}

impl Translator {
    fn run(&mut self) {
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            if c.is_whitespace() {
                self.had_space = true;
                self.i += 1;
                continue;
            }
            if self.try_arrow() {
                continue;
            }
            match c {
                '\\' => self.read_command(),
                '$' => self.read_math(),
                '(' | '[' => self.open_delim(c),
                ')' | ']' => self.close_delim(c),
                '{' | '}' => self.i += 1, // bare braces: ignore grouping hints
                '+' => self.plus(),
                '-' => self.minus(),
                '=' => self.bond("="),
                '#' => self.bond("\\equiv "),
                '~' => self.bond("-"), // mhchem single bond variant
                '^' => self.caret(),
                '_' => self.underscore(),
                'v' if self.standalone_marker() => self.gas_marker("\\downarrow "),
                '*' | '.' => {
                    // adduct dot (e.g. hydrate `*`); render as a centered dot
                    self.out.push_str("{}\\cdot{}");
                    self.last = Kind::Op;
                    self.had_space = false;
                    self.i += 1;
                },
                c if c.is_ascii_digit() => self.number(),
                c if c.is_ascii_alphabetic() => self.letters(),
                other => {
                    self.value_space();
                    push_escaped_char(&mut self.out, other);
                    self.last = Kind::Atom;
                    self.had_space = false;
                    self.i += 1;
                },
            }
        }
    }

    #[inline]
    fn peek(&self, off: usize) -> Option<char> {
        self.chars.get(self.i + off).copied()
    }

    /// Emit a thin space when a whitespace gap separates two formula values.
    #[inline]
    fn value_space(&mut self) {
        if self.had_space && self.last == Kind::Atom {
            self.out.push_str("\\;");
        }
    }

    /// A `^` / `v` is a standalone gas/precipitate marker when it is not glued
    /// to a preceding atom (i.e. at start, after an operator, or after a space).
    #[inline]
    fn standalone_marker(&self) -> bool {
        self.had_space || self.last != Kind::Atom
    }

    fn gas_marker(&mut self, arrow: &str) {
        self.out.push_str("{}");
        self.out.push_str(arrow);
        self.last = Kind::Atom;
        self.had_space = false;
        self.i += 1;
    }

    fn letters(&mut self) {
        self.value_space();
        let start = self.i;
        while self.i < self.chars.len() && self.chars[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        self.out.push_str("\\mathrm{");
        for &c in &self.chars[start..self.i] {
            self.out.push(c);
        }
        self.out.push('}');
        self.last = Kind::Atom;
        self.had_space = false;
    }

    fn read_digits(&mut self) -> String {
        let start = self.i;
        while self.i < self.chars.len() && self.chars[self.i].is_ascii_digit() {
            self.i += 1;
        }
        self.chars[start..self.i].iter().collect()
    }

    fn number(&mut self) {
        if self.last == Kind::Atom && !self.had_space {
            // Count → subscript.
            let digits = self.read_digits();
            self.out.push_str("_{");
            self.out.push_str(&digits);
            self.out.push('}');
            self.last = Kind::Atom;
            return;
        }
        // Stoichiometric coefficient, possibly a fraction `a/b`.
        self.value_space();
        let num = self.read_digits();
        if self.peek(0) == Some('/') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1; // consume '/'
            let den = self.read_digits();
            self.out.push_str("\\frac{");
            self.out.push_str(&num);
            self.out.push_str("}{");
            self.out.push_str(&den);
            self.out.push('}');
        } else {
            self.out.push_str(&num);
        }
        self.last = Kind::Atom;
        self.had_space = false;
    }

    fn open_delim(&mut self, c: char) {
        if c == '(' && let Some(state) = self.match_state() {
            self.value_space();
            self.out.push_str("(\\mathrm{");
            self.out.push_str(state);
            self.out.push_str("})");
            self.last = Kind::Atom;
            self.had_space = false;
            return;
        }
        self.value_space();
        self.out.push(c);
        // A digit right after an opening delimiter is a coefficient, not a
        // subscript, so reset to a non-atom state.
        self.last = Kind::Start;
        self.had_space = false;
        self.i += 1;
    }

    fn close_delim(&mut self, c: char) {
        self.out.push(c);
        self.last = Kind::Atom;
        self.had_space = false;
        self.i += 1;
    }

    /// If positioned at `(s)`, `(l)`, `(g)`, `(aq)`, ... consume through the
    /// closing paren and return the inner state string.
    fn match_state(&mut self) -> Option<&'static str> {
        // self.chars[self.i] == '('
        let mut j = self.i + 1;
        let start = j;
        while j < self.chars.len() && self.chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j >= self.chars.len() || self.chars[j] != ')' {
            return None;
        }
        let word: String = self.chars[start..j].iter().collect();
        let state = match word.as_str() {
            "s" => "s",
            "l" => "l",
            "g" => "g",
            "aq" => "aq",
            "cr" => "cr",
            "sln" => "sln",
            _ => return None,
        };
        self.i = j + 1; // consume through ')'
        Some(state)
    }

    fn plus(&mut self) {
        if self.last == Kind::Atom && !self.had_space {
            self.out.push_str("^{+}"); // trailing charge
            self.last = Kind::Atom;
        } else {
            self.out.push_str("{}+{}"); // binary operator between species
            self.last = Kind::Op;
        }
        self.had_space = false;
        self.i += 1;
    }

    fn minus(&mut self) {
        let next = self.peek(1);
        if self.last == Kind::Atom && !self.had_space {
            if matches!(next, Some(c) if c.is_ascii_alphabetic() || c == '(' || c == '[') {
                self.out.push_str("{-}"); // bond
                self.last = Kind::Start;
            } else {
                self.out.push_str("^{-}"); // trailing charge
                self.last = Kind::Atom;
            }
        } else {
            self.out.push_str("{}-{}"); // binary minus
            self.last = Kind::Op;
        }
        self.had_space = false;
        self.i += 1;
    }

    /// Emit a bond glyph between atoms.
    fn bond(&mut self, glyph: &str) {
        self.out.push('{');
        self.out.push_str(glyph);
        self.out.push('}');
        self.last = Kind::Start;
        self.had_space = false;
        self.i += 1;
    }

    fn caret(&mut self) {
        if self.standalone_marker() {
            self.gas_marker("\\uparrow ");
            return;
        }
        self.i += 1; // consume '^'
        self.out.push_str("^{");
        if self.peek(0) == Some('{') {
            self.i += 1;
            self.copy_braced();
        } else {
            let start = self.i;
            while self.i < self.chars.len()
                && matches!(self.chars[self.i], '0'..='9' | '+' | '-')
            {
                self.i += 1;
            }
            if self.i == start && self.i < self.chars.len() {
                self.i += 1; // at least one char
            }
            for &c in &self.chars[start..self.i] {
                self.out.push(c);
            }
        }
        self.out.push('}');
        self.last = Kind::Atom;
        self.had_space = false;
    }

    fn underscore(&mut self) {
        self.i += 1; // consume '_'
        self.out.push_str("_{");
        if self.peek(0) == Some('{') {
            self.i += 1;
            self.copy_braced();
        } else {
            let digits = self.read_digits();
            self.out.push_str(&digits);
        }
        self.out.push('}');
        self.last = Kind::Atom;
        self.had_space = false;
    }

    /// Copy characters up to a matching `}` (the opening `{` already consumed),
    /// emitting them verbatim into `out`.
    fn copy_braced(&mut self) {
        let mut depth = 1usize;
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            self.i += 1;
            match c {
                '{' => {
                    depth += 1;
                    self.out.push(c);
                },
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    self.out.push(c);
                },
                _ => self.out.push(c),
            }
        }
    }

    fn read_math(&mut self) {
        self.i += 1; // consume opening '$'
        self.value_space();
        while self.i < self.chars.len() && self.chars[self.i] != '$' {
            self.out.push(self.chars[self.i]);
            self.i += 1;
        }
        if self.i < self.chars.len() {
            self.i += 1; // consume closing '$'
        }
        self.last = Kind::Atom;
        self.had_space = false;
    }

    fn read_command(&mut self) {
        // self.chars[self.i] == '\\'
        let start = self.i;
        self.i += 1;
        let name_start = self.i;
        while self.i < self.chars.len() && self.chars[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let name: String = self.chars[name_start..self.i].iter().collect();
        if name == "ce" {
            // Nested \ce{...}: translate its body recursively.
            let tail: String = self.chars[self.i..].iter().collect();
            if let Some((rest, body)) = capture_braced(&tail) {
                let consumed = tail.len() - rest.len();
                self.value_space();
                self.out.push_str(&ce_to_latex(body));
                // Advance index past what `capture_braced` consumed.
                self.i += tail[..consumed].chars().count();
                self.last = Kind::Atom;
                self.had_space = false;
                return;
            }
        }
        if name.is_empty() {
            // Escaped single char (e.g. `\,`, `\{`): emit it verbatim.
            if self.i < self.chars.len() {
                self.out.push('\\');
                self.out.push(self.chars[self.i]);
                self.i += 1;
            }
        } else {
            self.value_space();
            for &c in &self.chars[start..self.i] {
                self.out.push(c);
            }
            self.out.push(' ');
        }
        self.last = Kind::Atom;
        self.had_space = false;
    }

    /// Detect and emit a reaction arrow (with optional `[above]`/`[below]`
    /// labels). Returns `true` when an arrow was consumed.
    fn try_arrow(&mut self) -> bool {
        // Longest match first.
        let candidates: &[(&str, &str)] = &[
            ("<=>>", "\\xrightleftharpoons"),
            ("<<=>", "\\xrightleftharpoons"),
            ("<=>", "\\xrightleftharpoons"),
            ("<->", "\\xleftrightarrow"),
            ("->", "\\xrightarrow"),
            ("<-", "\\xleftarrow"),
            ("=>", "\\xRightarrow"),
        ];
        for (tok, cmd) in candidates {
            if self.matches_at(tok) {
                self.i += tok.chars().count();
                self.emit_arrow(cmd);
                return true;
            }
        }
        false
    }

    fn matches_at(&self, tok: &str) -> bool {
        for (k, tc) in (self.i..).zip(tok.chars()) {
            if self.chars.get(k) != Some(&tc) {
                return false;
            }
        }
        true
    }

    fn emit_arrow(&mut self, cmd: &str) {
        // Optional `[above]` then optional `[below]` labels.
        let above = self.read_optional_bracket();
        let below = self.read_optional_bracket();

        if !self.out.is_empty() {
            self.out.push_str("\\;");
        }
        self.out.push_str(cmd);
        match below {
            Some(b) => {
                self.out.push('[');
                self.out.push_str(&b);
                self.out.push(']');
                self.out.push('{');
                self.out.push_str(&above.unwrap_or_default());
                self.out.push('}');
            },
            None => {
                self.out.push('{');
                self.out.push_str(&above.unwrap_or_default());
                self.out.push('}');
            },
        }
        self.out.push_str("\\;");
        self.last = Kind::Op;
        self.had_space = false;
    }

    /// Read an optional `[ ... ]` label (raw, brace/bracket-balanced) right at
    /// the current position, skipping intervening whitespace.
    fn read_optional_bracket(&mut self) -> Option<String> {
        let mut j = self.i;
        while j < self.chars.len() && self.chars[j].is_whitespace() {
            j += 1;
        }
        if self.chars.get(j) != Some(&'[') {
            return None;
        }
        j += 1;
        let start = j;
        let mut depth = 1usize;
        while j < self.chars.len() {
            match self.chars[j] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                },
                _ => {},
            }
            j += 1;
        }
        if j >= self.chars.len() {
            return None; // unbalanced: leave input untouched
        }
        let label: String = self.chars[start..j].iter().collect();
        self.i = j + 1; // consume through ']'
        Some(label)
    }
}

#[inline]
fn is_number(tok: &str) -> bool {
    let mut seen_digit = false;
    for c in tok.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            '.' | '+' | '-' | 'e' | 'E' => {},
            _ => return false,
        }
    }
    seen_digit
}

/// Push a single char into a `\mathrm{}` body, escaping LaTeX specials.
fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '%' | '&' | '_' | '#' | '$' => {
            out.push('\\');
            out.push(c);
        },
        _ => out.push(c),
    }
}

fn push_escaped_text(out: &mut String, s: &str) {
    for c in s.chars() {
        push_escaped_char(out, c);
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
    fn water_has_subscript() {
        let ml = mathml(r"\ce{H2O}");
        assert!(ml.contains("<msub>"), "expected subscript in {ml}");
        assert!(ml.contains('H') && ml.contains('O'));
        assert!(ml.contains("<mn>2</mn>"), "expected count 2 in {ml}");
    }

    #[test]
    fn co2_has_subscript() {
        let ml = mathml(r"\ce{CO2}");
        assert!(ml.contains("<msub>"));
    }

    #[test]
    fn group_subscript() {
        let ml = mathml(r"\ce{Ca(OH)2}");
        assert!(ml.contains("<msub>"));
        assert!(ml.contains('(') && ml.contains(')'));
    }

    #[test]
    fn sulfate_charge_superscript() {
        let ml = mathml(r"\ce{SO4^2-}");
        assert!(ml.contains("<msubsup>") || ml.contains("<msup>"), "expected superscript in {ml}");
        // Charge sign renders as a Unicode minus (U+2212).
        assert!(ml.contains('-') || ml.contains('\u{2212}'), "expected charge sign in {ml}");
    }

    #[test]
    fn reaction_with_coefficients_and_arrow() {
        let ml = mathml(r"\ce{2H2 + O2 -> 2H2O}");
        // Coefficient 2 stays a normal number.
        assert!(ml.contains("<mn>2</mn>"));
        // Plus operator present.
        assert!(ml.contains('+'));
        // Right arrow present (extensible arrow → over an empty label).
        assert!(ml.contains('\u{2192}'), "expected → in {ml}");
        assert!(ml.contains("<mover") || ml.contains("<munderover"));
    }

    #[test]
    fn arrow_with_label() {
        let ml = mathml(r"\ce{CaCO3 ->[\Delta] CaO + CO2 ^}");
        assert!(ml.contains('\u{2192}'), "expected → in {ml}");
        // Delta label above the arrow.
        assert!(ml.contains('\u{0394}') || ml.contains("Delta"), "expected Δ in {ml}");
        // Gas up-arrow marker.
        assert!(ml.contains('\u{2191}'), "expected ↑ in {ml}");
    }

    #[test]
    fn equilibrium_arrow() {
        let ml = mathml(r"\ce{A <=> B}");
        assert!(ml.contains('\u{21CC}'), "expected ⇌ in {ml}");
    }

    #[test]
    fn complex_ion_charge() {
        let ml = mathml(r"\ce{[Cu(NH3)4]^2+}");
        assert!(ml.contains('[') && ml.contains(']'));
        assert!(ml.contains("<msup>") || ml.contains("<msubsup>"));
        assert!(ml.contains('+'));
        assert!(ml.contains("<msub>"));
    }

    #[test]
    fn half_fraction_coefficient() {
        let ml = mathml(r"\ce{1/2 O2}");
        assert!(ml.contains("<mfrac>"), "expected fraction in {ml}");
    }

    #[test]
    fn bond_between_atoms() {
        let ml = mathml(r"\ce{C#C}");
        assert!(ml.contains('\u{2261}'), "expected ≡ triple bond in {ml}");
    }

    #[test]
    fn physical_units() {
        let ml = mathml(r"\pu{123 kg m/s}");
        assert!(ml.contains("<mn>123</mn>"), "expected number in {ml}");
        // Units render upright; `kg` becomes separate upright identifiers.
        assert!(ml.contains("mathvariant=\"normal\""), "expected upright units in {ml}");
        assert!(ml.contains(">k</mi>") && ml.contains(">g</mi>"), "expected unit kg in {ml}");
    }

    #[test]
    fn malformed_does_not_panic() {
        // Unbalanced / odd input must recover, not panic.
        let _ = from_latex(r"\ce{H2O^{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ce{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ce{((()))}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\pu{}").and_then(|c| c.to_mathml());
    }
}
