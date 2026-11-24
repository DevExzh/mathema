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
//! * Isotope left pre-scripts: `\ce{^{227}_{90}Th}` → mass/atomic numbers as
//!   true left super/subscripts (an `<mmultiscripts>`/`<mprescripts>`).
//!
//! Deferred (documented, not implemented): electron dots, complex bond
//! geometry / Lewis structures, advanced `\pu` unit algebra.

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
/// Inside a unit token the rules follow mhchem's `pu-2` grammar (see
/// [`push_unit_token`]), including its failures: `\pu{m_2}` translates to
/// `\mathrm{m_^{2}}` exactly as mhchem does, which neither TeX nor this
/// reader accepts, so the caller reports it instead of rendering nothing.
#[must_use]
pub fn pu_to_latex(body: &str) -> String {
    let mut out = String::with_capacity(body.len() * 3 + 4);
    push_pu(&mut out, body, 0);
    out
}

/// How deeply `\pu{{{…}}}` unit groups are unwrapped (mhchem re-reads a unit
/// that is one braced group as a `\pu` body); deeper groups are kept as they
/// are, so adversarial nesting cannot exhaust the stack.
const MAX_PU_GROUP_DEPTH: usize = 16;

fn push_pu(out: &mut String, body: &str, depth: usize) {
    for (idx, tok) in pu_tokens(body).enumerate() {
        if idx > 0 {
            out.push_str("\\,");
        }
        if is_number(tok) {
            out.push_str(tok);
        } else {
            push_unit_token(out, tok, depth);
        }
    }
}

/// Split a `\pu` body at the whitespace outside braces, so a braced unit
/// such as `{kg m}2` stays one token.
fn pu_tokens(body: &str) -> impl Iterator<Item = &str> {
    let mut rest = body;
    std::iter::from_fn(move || {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let mut depth = 0usize;
        let mut escaped = false;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                c if c.is_whitespace() && depth == 0 => {
                    end = i;
                    break;
                },
                _ => {},
            }
        }
        let (tok, after) = rest.split_at(end);
        rest = after;
        Some(tok)
    })
}

/// Push one `\pu` unit token, following mhchem's `pu-2` state machine:
///
/// * `*` and `.` multiply: `kg.m` is `\mathrm{kg}·\mathrm{m}`.
/// * A number after unit text is its exponent, written braced (`m2`, `s-1`,
///   `m^22`, `mol^-1` raise the whole number); `^{…}` there is copied
///   braced. After a number (or at the start) a digit run stays literal.
/// * Everything else, `_` and a `^` that starts no exponent included, is
///   copied into the `\mathrm{}` as TeX, so `a_b` is a subscript and
///   `m^2^3` is the double superscript mhchem writes too.
///
/// `°C`, `^oC` and `^{o}C` (and `F`) become `{}^{\circ}C`, as in mhchem.
fn push_unit_token(out: &mut String, tok: &str, depth: usize) {
    let degrees;
    let tok = if tok.contains(['°', '^']) {
        degrees = replace_degrees(tok);
        degrees.as_str()
    } else {
        tok
    };
    let mut run = String::with_capacity(tok.len() + 8);
    // `true` after unit text (mhchem's state `1`), where a number is an
    // exponent; `false` at the start and after a number (state `0`).
    let mut after_unit = false;
    let mut rest = tok;
    while let Some(c) = rest.chars().next() {
        if matches!(c, '*' | '.') {
            push_unit_run(out, &run, depth);
            run.clear();
            out.push_str("\\mkern1mu{\\cdot}\\mkern1mu ");
            after_unit = false;
            rest = &rest[1..];
            continue;
        }
        if c == '\\' {
            // A control word or symbol is copied whole and leaves the state
            // as it was (`m\alpha2` raises the 2, `\alpha2` does not).
            let after = &rest[1..];
            let len = match after.chars().next() {
                Some(n) if n.is_ascii_alphabetic() => {
                    after.bytes().take_while(u8::is_ascii_alphabetic).count()
                },
                Some(n) => n.len_utf8(),
                None => 0,
            };
            run.push_str(&rest[..=len]);
            rest = &after[len..];
            continue;
        }
        if after_unit && let Some(after) = rest.strip_prefix('^') {
            if after.starts_with('{')
                && let Some((tail, inner)) = capture_braced(after)
            {
                run.push_str("^{");
                run.push_str(inner);
                run.push('}');
                rest = tail;
                continue;
            }
            let sign = usize::from(after.starts_with('-'));
            let digits = after[sign..].bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                run.push_str("^{");
                run.push_str(&after[..sign + digits]);
                run.push('}');
                rest = &after[sign + digits..];
                continue;
            }
        }
        if let Some(len) = number_len(rest) {
            if after_unit {
                run.push_str("^{");
                run.push_str(&rest[..len]);
                run.push('}');
            } else {
                run.push_str(&rest[..len]);
            }
            after_unit = false;
            rest = &rest[len..];
            continue;
        }
        if c == '{'
            && let Some((tail, _)) = capture_braced(rest)
        {
            run.push_str(&rest[..rest.len() - tail.len()]);
            rest = tail;
        } else {
            push_escaped_char(&mut run, c);
            rest = &rest[c.len_utf8()..];
        }
        after_unit = true;
    }
    push_unit_run(out, &run, depth);
}

/// Write one run of unit text between multiplication dots: upright, or, when
/// it is a single braced group, its content re-read as a `\pu` body (with no
/// group of its own), as mhchem does.
fn push_unit_run(out: &mut String, run: &str, depth: usize) {
    if run.is_empty() {
        return;
    }
    if depth < MAX_PU_GROUP_DEPTH
        && let Some(("", inner)) = capture_braced(run)
        && run.starts_with('{')
    {
        push_pu(out, inner, depth + 1);
        return;
    }
    out.push_str("\\mathrm{");
    out.push_str(run);
    out.push('}');
}

/// mhchem's degree spellings, rewritten before a unit is read.
fn replace_degrees(tok: &str) -> String {
    let mut s = tok.to_owned();
    for unit in ["C", "F"] {
        for spelling in ["\u{00B0}", "^o", "^{o}"] {
            s = s.replace(
                &format!("{spelling}{unit}"),
                &format!("{{}}^{{\\circ}}{unit}"),
            );
        }
    }
    s
}

/// The length of mhchem's `-9.,9` number at the start of `s`:
/// `[+-]?(?:[0-9]+(?:[,.][0-9]+)?|[0-9]*\.[0-9]+)`.
fn number_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(i);
    i += int;
    if matches!(b.get(i), Some(b',' | b'.')) && (b[i] == b'.' || int > 0) {
        let frac = digits(i + 1);
        if frac > 0 {
            return Some(i + 1 + frac);
        }
    }
    (int > 0).then_some(i)
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
            // Isotope left pre-scripts: a `^`/`_` at the *start* of a species
            // (before its element letters) is a mass/atomic-number pre-script,
            // not a gas marker or empty-base subscript.
            if matches!(c, '^' | '_') && self.try_prescript() {
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
        if c == '('
            && let Some(state) = self.match_state()
        {
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
            // A `-` glued to a preceding species is a *bond* only when it
            // directly joins it to another atom (`A-B`) or a bracket group
            // (`-[...]`). Otherwise the `-` *terminates* the species and is an
            // ionic charge superscript: this fires when the next char is
            // whitespace, end of input, `+`, a closing/opening delimiter such
            // as `(` (a state like `(aq)` or a following group), etc. In
            // particular `-(` is a charge so `\ce{Cl-(aq)}` → Cl⁻(aq), matching
            // mhchem, rather than the old inline `Cl−(aq)` bond reading.
            if matches!(next, Some(c) if c.is_ascii_alphabetic() || c == '[') {
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

    /// Try to parse a leading isotope pre-script `^{mass}_{atomic}Element`
    /// (mhchem order; either script optional, in either order) immediately
    /// followed by the element letters.
    ///
    /// Emits `\sideset{_{atomic}^{mass}}{}\mathrm{Element}` so the re-parser
    /// builds a *true* left pre-script (a `PreScripts` node → `<mmultiscripts>`
    /// with `<mprescripts/>`), instead of degrading the leading `^` into a `↑`
    /// gas marker and the `_` into a subscript on an empty base. The element
    /// letters themselves are emitted by the normal `letters()` path on the
    /// next loop iteration, becoming the pre-script base.
    ///
    /// Returns `true` when a pre-script was consumed and emitted.
    fn try_prescript(&mut self) -> bool {
        // Only meaningful where a `^`/`_` is NOT glued to a preceding atom,
        // i.e. at the very start of a species. A glued `^`/`_` is an ordinary
        // charge / subscript and must be left to `caret()` / `underscore()`.
        if self.last == Kind::Atom && !self.had_space {
            return false;
        }
        let mut j = self.i;
        let mut sup: Option<String> = None;
        let mut sub: Option<String> = None;
        // Collect the leading `^{...}` / `_{...}` script groups (each at most
        // once), in whatever order they appear.
        loop {
            match self.chars.get(j) {
                Some('^') if sup.is_none() => match self.scan_script_arg(j + 1) {
                    Some((next, body)) => {
                        sup = Some(body);
                        j = next;
                    },
                    None => return false,
                },
                Some('_') if sub.is_none() => match self.scan_script_arg(j + 1) {
                    Some((next, body)) => {
                        sub = Some(body);
                        j = next;
                    },
                    None => return false,
                },
                _ => break,
            }
        }
        // Require at least one script immediately followed by element letters;
        // otherwise this is not an isotope pre-script (e.g. a bare gas marker).
        if (sup.is_none() && sub.is_none())
            || !self.chars.get(j).is_some_and(|c| c.is_ascii_alphabetic())
        {
            return false;
        }
        self.value_space();
        self.out.push_str("\\sideset{");
        if let Some(s) = &sub {
            self.out.push_str("_{");
            self.out.push_str(s);
            self.out.push('}');
        }
        if let Some(s) = &sup {
            self.out.push_str("^{");
            self.out.push_str(s);
            self.out.push('}');
        }
        self.out.push_str("}{}");
        self.i = j; // advance past the consumed script groups to the element
        // `last = Start` so the following letters are emitted without a leading
        // thin space and are not mistaken for a subscript.
        self.last = Kind::Start;
        self.had_space = false;
        true
    }

    /// Scan a script argument starting at index `k` (just past `^`/`_`): either
    /// a braced group `{...}` (returns its inner text) or a bare run of digits.
    /// Returns the index just past the argument and the argument body, or
    /// `None` when nothing usable follows.
    fn scan_script_arg(&self, k: usize) -> Option<(usize, String)> {
        match self.chars.get(k) {
            Some('{') => {
                let mut depth = 1usize;
                let start = k + 1;
                let mut j = start;
                while j < self.chars.len() {
                    match self.chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some((j + 1, self.chars[start..j].iter().collect()));
                            }
                        },
                        _ => {},
                    }
                    j += 1;
                }
                None
            },
            Some(c) if c.is_ascii_digit() => {
                let mut j = k;
                while j < self.chars.len() && self.chars[j].is_ascii_digit() {
                    j += 1;
                }
                Some((j, self.chars[k..j].iter().collect()))
            },
            _ => None,
        }
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
            while self.i < self.chars.len() && matches!(self.chars[self.i], '0'..='9' | '+' | '-') {
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

/// Push a single char into a `\mathrm{}` body, escaping the LaTeX specials
/// that would otherwise break the translation (`#` is a macro parameter,
/// `%` a comment). `_` and `^` stay scripts, as mhchem copies them.
fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '%' | '&' | '#' | '$' => {
            out.push('\\');
            out.push(c);
        },
        _ => out.push(c),
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
        assert!(
            ml.contains("<msubsup>") || ml.contains("<msup>"),
            "expected superscript in {ml}"
        );
        // Charge sign renders as a Unicode minus (U+2212).
        assert!(
            ml.contains('-') || ml.contains('\u{2212}'),
            "expected charge sign in {ml}"
        );
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
        assert!(
            ml.contains('\u{0394}') || ml.contains("Delta"),
            "expected Δ in {ml}"
        );
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
        assert!(
            ml.contains("mathvariant=\"normal\""),
            "expected upright units in {ml}"
        );
        assert!(
            ml.contains(">k</mi>") && ml.contains(">g</mi>"),
            "expected unit kg in {ml}"
        );
    }

    #[test]
    fn malformed_does_not_panic() {
        // Unbalanced / odd input must recover, not panic.
        let _ = from_latex(r"\ce{H2O^{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ce{").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ce{((()))}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\pu{}").and_then(|c| c.to_mathml());
        // Leading scripts with no element / unbalanced braces must not panic.
        let _ = from_latex(r"\ce{^{227}").and_then(|c| c.to_mathml());
        let _ = from_latex(r"\ce{^{227}_{90}}").and_then(|c| c.to_mathml());
    }

    #[test]
    fn isotope_left_prescripts() {
        // `^{227}_{90}Th` must become a TRUE left pre-script (mmultiscripts with
        // mprescripts), not a stray gas-marker `↑` plus empty-base subscripts.
        let ml = mathml(r"\ce{^{227}_{90}Th}");
        assert!(
            ml.contains("<mmultiscripts>") && ml.contains("<mprescripts"),
            "expected mprescripts in {ml}"
        );
        assert!(
            ml.contains("227") && ml.contains("90"),
            "expected masses in {ml}"
        );
        // No degraded gas-marker up-arrow.
        assert!(!ml.contains('\u{2191}'), "unexpected ↑ in {ml}");
        // Element letters present as the base.
        assert!(
            ml.contains(">T</mi>") && ml.contains(">h</mi>"),
            "expected Th in {ml}"
        );
    }

    #[test]
    fn isotope_mass_only_prescript() {
        let ml = mathml(r"\ce{^{14}C}");
        assert!(
            ml.contains("<mmultiscripts>") && ml.contains("<mprescripts"),
            "expected mprescripts in {ml}"
        );
        assert!(ml.contains("14"), "expected mass 14 in {ml}");
    }

    #[test]
    fn chloride_charge_at_end() {
        // `Cl-` at end of input: the `-` is a charge, rendered as a superscript.
        let ml = mathml(r"\ce{Cl-}");
        assert!(ml.contains("<msup>"), "expected charge superscript in {ml}");
        assert!(
            ml.contains('-') || ml.contains('\u{2212}'),
            "expected minus charge in {ml}"
        );
    }

    #[test]
    fn chloride_charge_before_state() {
        // `Cl-(aq)`: the `-` terminates the species (next char `(`), so it is a
        // charge superscript, NOT an inline bond.
        let ml = mathml(r"\ce{Cl-(aq)}");
        assert!(ml.contains("<msup>"), "expected charge superscript in {ml}");
        // State `(aq)` still renders upright.
        assert!(ml.contains('(') && ml.contains(')'));
        // The minus must be the charge *inside* the superscript (closing
        // `</msup>` before the `(` of the state), not a sibling inline bond.
        let sup_end = ml.find("</msup>").expect("msup present");
        let paren = ml.find("form=\"prefix\"").expect("opening paren present");
        assert!(
            sup_end < paren,
            "charge superscript should close before the state in {ml}"
        );
    }

    #[test]
    fn chloride_charge_before_space_state() {
        let ml = mathml(r"\ce{Cl- (aq)}");
        assert!(ml.contains("<msup>"), "expected charge superscript in {ml}");
    }

    #[test]
    fn single_letter_bond_stays_bond() {
        // `A-B` joins two atoms: the `-` is a bond (inline operator), not a
        // charge. No charge superscript should appear.
        let ml = mathml(r"\ce{A-B}");
        assert!(
            !ml.contains("<msup>"),
            "A-B should not produce a charge in {ml}"
        );
        assert!(
            ml.contains('-') || ml.contains('\u{2212}'),
            "expected bond glyph in {ml}"
        );
    }
}
