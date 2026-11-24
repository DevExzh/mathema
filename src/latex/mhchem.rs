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
//! geometry / Lewis structures. `\pu` is a full port of mhchem's unit
//! grammar (see the `pu` submodule).

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
    translate(body, 0)
}

/// How deeply `\ce`, `\frac`, `\overset` and `\underset` arguments are
/// read as `\ce` bodies. mhchem recurses without a bound; past this depth
/// the command is copied as it is, so adversarial input cannot exhaust the
/// stack.
const MAX_DEPTH: usize = 16;

fn translate(body: &str, depth: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut t = Translator {
        depth,
        chars,
        i: 0,
        out: String::with_capacity(body.len() * 4 + 8),
        had_space: false,
        last: Kind::Start,
        scripts: Scripts::default(),
        scripted: false,
        base_start: 0,
        bare_word_end: None,
    };
    t.run();
    t.out
}

mod pu;

pub use pu::pu_to_latex;

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
    /// A control sequence copied as an opaque TeX token (`\text{heat}`,
    /// `\frac{1}{2}`, `\times`): mhchem's state `3`, where a following digit
    /// run is an amount, `+` an operator and `-` a bond, but `^`/`_` still
    /// script the token.
    Command,
}

struct Translator {
    /// How many `\ce` bodies enclose this one.
    depth: usize,
    chars: Vec<char>,
    i: usize,
    out: String,
    /// Whitespace was seen since the last emitted token.
    had_space: bool,
    last: Kind,
    /// The scripts already on the current base.
    scripts: Scripts,
    /// The step just taken emitted a script (so the base is unchanged).
    scripted: bool,
    /// Where the current base starts in `out`.
    base_start: usize,
    /// The end in `out` of a control word copied with no brace group and no
    /// space after it (`\frac)`): TeX may take the next token as its
    /// argument, so a script on that token goes on an empty group instead.
    bare_word_end: Option<usize>,
}

/// The scripts on the current base. TeX takes one subscript and one
/// superscript per base, so a further one goes on a new empty base, as
/// mhchem writes it (`H2_{(aq)}` is `H_{2}{\vphantom{X}}_{(aq)}`).
#[derive(Clone, Copy, Default)]
struct Scripts {
    sub: bool,
    sup: bool,
    /// The output ends with a superscript, into which a following bare one
    /// merges, as mhchem's charge does: `A^2^+` is `A^{2+}`, `Y^{99}+` is
    /// `Y^{99+}` (a braced one starts a new group: `A^2^{+}`).
    open_sup: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Sub,
    /// A superscript written in braces: a new group, never merged.
    BracedSup,
    /// A bare superscript or charge: merged into a preceding superscript.
    Sup,
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
            self.scripted = false;
            let start = self.out.len();
            self.step(c);
            if !self.scripted {
                self.scripts = Scripts::default();
                self.base_start = start;
            }
        }
    }

    /// Start a script of `kind` on the current base, or on a new empty base
    /// when the current one has that script already; a bare superscript
    /// reopens the open one instead. [`Self::end_script`] closes it.
    fn begin_script(&mut self, kind: Script) {
        self.scripted = true;
        if kind == Script::Sup && self.scripts.open_sup && self.out.ends_with('}') {
            self.out.pop();
            return;
        }
        if self.bare_word_end.take() == Some(self.base_start) && self.out.len() > self.base_start {
            // `\frac)_9` would read `_` as `\frac`'s argument: mhchem
            // scripts an empty base (`{\vphantom{X}}`), so does this.
            self.out.push_str("{}");
            self.scripts = Scripts::default();
        }
        let taken = if kind == Script::Sub {
            self.scripts.sub
        } else {
            self.scripts.sup
        };
        if taken {
            self.out.push_str("{\\vphantom{X}}");
            self.scripts = Scripts::default();
        }
        if kind == Script::Sub {
            self.scripts.sub = true;
            self.out.push_str("_{");
        } else {
            self.scripts.sup = true;
            self.out.push_str("^{");
        }
    }

    /// Close the script [`Self::begin_script`] opened (or merged into).
    fn end_script(&mut self, kind: Script) {
        self.out.push('}');
        self.scripts.open_sup = kind != Script::Sub;
    }

    /// Write a whole script `^{body}`/`_{body}`.
    fn script(&mut self, kind: Script, body: &str) {
        self.begin_script(kind);
        self.out.push_str(body);
        self.end_script(kind);
    }

    fn step(&mut self, c: char) {
        if self.try_arrow() {
            return;
        }
        // Isotope left pre-scripts: a `^`/`_` at the *start* of a species
        // (before its element letters) is a mass/atomic-number pre-script,
        // not a gas marker or empty-base subscript.
        if matches!(c, '^' | '_') && self.try_prescript() {
            return;
        }
        match c {
            '\\' => self.read_command(),
            '$' => self.read_math(),
            '(' | '[' => self.open_delim(c),
            ')' | ']' => self.close_delim(c),
            // `{}` ends a species (mhchem's `{}`): a number after it is an
            // amount, not a count (`x1{}0`).
            '{' if self.peek(1) == Some('}') => {
                self.i += 2;
                if self.last != Kind::Op {
                    self.last = Kind::Start;
                }
            },
            // Other bare braces are grouping hints: ignored, keeping the
            // current base and its scripts.
            '{' | '}' => {
                self.i += 1;
                self.scripted = true;
            },
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
                // A Greek letter is element text (mhchem's `letters`); other
                // characters, like `°` or `/`, are mhchem's `else2`/`others`,
                // after which a number is an amount (state `3`).
                let element =
                    (other.is_ascii() && !matches!(other, '/' | '|')) || is_greek_char(other);
                self.last = if element { Kind::Atom } else { Kind::Command };
                self.had_space = false;
                self.i += 1;
            },
        }
    }

    #[inline]
    fn peek(&self, off: usize) -> Option<char> {
        self.chars.get(self.i + off).copied()
    }

    /// Emit a thin space when a whitespace gap separates two formula values.
    #[inline]
    fn value_space(&mut self) {
        if self.had_space && matches!(self.last, Kind::Atom | Kind::Command) {
            self.out.push_str("\\;");
        }
    }

    /// A `^` / `v` is a standalone gas/precipitate marker when it is not glued
    /// to a preceding atom (i.e. at start, after an operator, or after a space).
    #[inline]
    fn standalone_marker(&self) -> bool {
        self.had_space || !matches!(self.last, Kind::Atom | Kind::Command)
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
            self.script(Script::Sub, &digits);
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
            self.script(Script::Sup, "+"); // trailing charge
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
        if self.last == Kind::Command && !self.had_space {
            self.out.push_str("{-}"); // bond after a token, as mhchem's state `3`
            self.last = Kind::Start;
        } else if self.last == Kind::Atom && !self.had_space {
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
                self.script(Script::Sup, "-"); // trailing charge
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
        if matches!(self.last, Kind::Atom | Kind::Command) && !self.had_space {
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
        // mhchem's `arrowUpDown`: a `^` not glued to a species, before a
        // space, the end, or `,;)]}`, is the gas marker.
        let ends = self
            .peek(1)
            .is_none_or(|c| c.is_whitespace() || matches!(c, ',' | ';' | ')' | ']' | '}'));
        if self.standalone_marker() && ends {
            self.gas_marker("\\uparrow ");
            return;
        }
        self.i += 1; // consume '^'
        if !self.has_script_arg('_') {
            return;
        }
        let kind = if self.peek(0) == Some('{') {
            Script::BracedSup
        } else {
            Script::Sup
        };
        self.begin_script(kind);
        if kind == Script::BracedSup {
            self.i += 1;
            self.copy_braced();
        } else if self.peek(0) == Some('\\') {
            self.script_command();
        } else {
            let start = self.i;
            while self.i < self.chars.len() && matches!(self.chars[self.i], '0'..='9' | '+' | '-') {
                self.i += 1;
            }
            if self.i == start {
                // At least one char (`has_script_arg` saw one).
                push_escaped_char(&mut self.out, self.chars[self.i]);
                self.i += 1;
            } else {
                self.out.extend(&self.chars[start..self.i]);
            }
        }
        self.end_script(kind);
        self.last = Kind::Atom;
        self.had_space = false;
    }

    /// Whether the `^`/`_` just consumed has an argument. mhchem's `^_`
    /// pattern drops a `^` before `_` (`_` before `^`) or at the end, and a
    /// `}` or `^` is no argument here; a dropped script leaves the current
    /// base and its scripts as they were.
    fn has_script_arg(&mut self, other: char) -> bool {
        let has = match self.peek(0) {
            None | Some('}' | '^') => false,
            Some(c) => c != other,
        };
        if !has {
            self.scripted = true;
        }
        has
    }

    fn underscore(&mut self) {
        self.i += 1; // consume '_'
        if !self.has_script_arg('^') {
            return;
        }
        self.begin_script(Script::Sub);
        match self.peek(0) {
            Some('{') => {
                self.i += 1;
                self.copy_braced();
            },
            Some('\\') => self.script_command(),
            _ => {
                // mhchem's `_9`: a signed digit run, or one character.
                let sign = usize::from(matches!(self.peek(0), Some('+' | '-')));
                let mut end = self.i + sign;
                while self.chars.get(end).is_some_and(char::is_ascii_digit) {
                    end += 1;
                }
                if end > self.i + sign {
                    self.out.extend(&self.chars[self.i..end]);
                    self.i = end;
                } else {
                    push_escaped_char(&mut self.out, self.chars[self.i]);
                    self.i += 1;
                }
            },
        }
        self.end_script(Script::Sub);
        self.last = Kind::Atom;
        self.had_space = false;
    }

    /// A script argument that starts with `\`: a control word with its brace
    /// groups, or a control symbol TeX reads (`\_`), is copied whole, as is
    /// `\0`, which fails as in KaTeX; any other backslash is dropped and the
    /// character after it is the argument.
    fn script_command(&mut self) {
        match self.peek(1) {
            Some(c) if c.is_ascii_alphanumeric() || is_control_symbol(c) => {
                self.copy_command_token();
            },
            Some('^') | None => self.i += 1,
            Some(c) => {
                push_escaped_char(&mut self.out, c);
                self.i += 2;
            },
        }
    }

    /// Copy characters up to a matching `}` (the opening `{` already consumed),
    /// emitting them verbatim into `out`. Escaped braces (`\{`) do not count.
    fn copy_braced(&mut self) {
        let mut depth = 1usize;
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            self.i += 1;
            match c {
                '\\' => {
                    self.out.push(c);
                    if let Some(&next) = self.chars.get(self.i) {
                        self.out.push(next);
                        self.i += 1;
                    }
                },
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

    /// Scan the control sequence at `k` (a `\`) as mhchem's `\x{}{}`,
    /// `\x{}` and `\x` patterns read it: a control word with up to two brace
    /// groups directly after it, or, with no group, the word and the spaces
    /// after it; else a control symbol. Returns the end of the control
    /// word's name (just past the `\\` for a control symbol) and the end of
    /// the token.
    fn command_token(&self, k: usize) -> (usize, usize) {
        let mut name_end = k + 1;
        while self
            .chars
            .get(name_end)
            .is_some_and(char::is_ascii_alphabetic)
        {
            name_end += 1;
        }
        if name_end == k + 1 {
            // A control symbol (`\,`, `\_`), or a lone `\` at the end.
            let end = if k + 1 < self.chars.len() {
                k + 2
            } else {
                k + 1
            };
            return (k + 1, end);
        }
        let mut end = name_end;
        for _ in 0..2 {
            match self.group_end(end) {
                Some(next) => end = next,
                None => break,
            }
        }
        if end == name_end {
            while self.chars.get(end).is_some_and(|c| c.is_whitespace()) {
                end += 1;
            }
        }
        (name_end, end)
    }

    /// The index just past the balanced `{...}` group starting at `k`
    /// (escaped braces do not count), or `None` when there is none.
    fn group_end(&self, k: usize) -> Option<usize> {
        if self.chars.get(k) != Some(&'{') {
            return None;
        }
        let mut depth = 0usize;
        let mut j = k;
        while j < self.chars.len() {
            match self.chars[j] {
                '\\' => j += 1,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j + 1);
                    }
                },
                _ => {},
            }
            j += 1;
        }
        None
    }

    /// Copy the control sequence at the cursor (with its brace groups) into
    /// the output unchanged, as one token.
    fn copy_command_token(&mut self) {
        let (name_end, end) = self.command_token(self.i);
        if end == self.i + 1 {
            // A lone `\` at the end of the body: nothing to copy.
            self.i = end;
            return;
        }
        self.out.extend(&self.chars[self.i..end]);
        if end == name_end && self.chars[name_end - 1].is_ascii_alphabetic() {
            // Keep a following letter from extending the control word.
            self.out.push(' ');
            self.bare_word_end = Some(self.out.len());
        }
        self.i = end;
    }

    /// The text of the brace group at `k` without its braces, and the index
    /// past it.
    fn group_body(&self, k: usize) -> Option<(String, usize)> {
        let end = self.group_end(k)?;
        Some((self.chars[k + 1..end - 1].iter().collect(), end))
    }

    /// A control sequence is one opaque token, as mhchem reads it: its brace
    /// groups go with it, and it is never split into letters. `\ce{...}`,
    /// `\frac`, `\overset` and `\underset` read their arguments as `\ce`
    /// bodies; Greek letters are element letters; `\pm` is an operator.
    fn read_command(&mut self) {
        // self.chars[self.i] == '\\'
        let (name_end, end) = self.command_token(self.i);
        let name: String = self.chars[self.i + 1..name_end].iter().collect();
        if name.is_empty() {
            match self.peek(1) {
                // A control symbol TeX reads (`\,`, `\{`, `\_`): copied
                // verbatim. A spacing one is a space between species
                // (mhchem's `\,`, state `1`); the others are tokens.
                Some(c) if is_control_symbol(c) => {
                    self.value_space();
                    self.copy_command_token();
                    self.last = if matches!(c, ',' | ';' | ':' | ' ' | '!' | '>') {
                        Kind::Start
                    } else {
                        Kind::Command
                    };
                    self.had_space = false;
                },
                // A text accent KaTeX reads after mhchem's copy (`\~{n}`,
                // `\"C`, `A\.2`; a `.` before anything else is an adduct
                // dot): copied, so it accents the next token. An empty group
                // stands in for a token when none follows.
                Some(c)
                    if matches!(c, '"' | '`' | '~')
                        || (c == '.' && self.peek(2).is_some_and(|d| d.is_ascii_digit())) =>
                {
                    self.value_space();
                    self.out.push('\\');
                    self.out.push(c);
                    self.i += 2;
                    if self
                        .peek(0)
                        .is_some_and(|d| d.is_whitespace() || matches!(d, '^' | '_' | '}'))
                    {
                        self.out.push_str("{}");
                    }
                    self.last = Kind::Command;
                    self.had_space = false;
                },
                // `\0`: mhchem copies it, and TeX has no such command, so
                // it fails to read back, as in KaTeX.
                Some(c) if c.is_ascii_digit() => {
                    self.copy_command_token();
                    self.last = Kind::Command;
                    self.had_space = false;
                },
                // Any other backslash (`\+`, `\*`, a trailing `\`) is no
                // command TeX reads: it is dropped, but like mhchem, which
                // copies it as a token of its own (state `3`), it ends the
                // species, so `\+` is an operator.
                _ => {
                    self.i += 1;
                    if self.last != Kind::Op {
                        self.last = Kind::Command;
                    }
                },
            }
            return;
        }
        // mhchem's `pm-operator` matches `\pm` as a prefix (`\pmE` is `\pm E`).
        if name.starts_with("pm") {
            self.out.push_str("{}\\pm{}");
            self.i += 3;
            self.last = Kind::Op;
            self.had_space = false;
            return;
        }
        let nested = match name.as_str() {
            _ if self.depth >= MAX_DEPTH => None,
            "ce" => self
                .group_body(name_end)
                .map(|(body, next)| (translate(&body, self.depth + 1), next)),
            "frac" | "overset" | "underset" => self.group_body(name_end).and_then(|(a, mid)| {
                let (b, next) = self.group_body(mid)?;
                let (a, b) = (translate(&a, self.depth + 1), translate(&b, self.depth + 1));
                let tex = format!("\\{name}{{{a}}}{{{b}}}");
                Some((tex, next))
            }),
            _ => None,
        };
        self.value_space();
        if let Some((tex, next)) = nested {
            self.out.push_str(&tex);
            self.i = next;
            self.last = if name == "ce" {
                Kind::Atom
            } else {
                Kind::Command
            };
        } else if is_greek(&name) {
            // A Greek letter is element text: a following count is its
            // subscript (`\alpha2`).
            self.out.push('\\');
            self.out.push_str(&name);
            self.out.push(' ');
            self.i = name_end;
            while self.peek(0).is_some_and(char::is_whitespace) {
                self.i += 1;
            }
            self.last = Kind::Atom;
        } else {
            self.copy_command_token();
            debug_assert_eq!(self.i, end);
            self.last = Kind::Command;
        }
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

/// The characters after `\\` that make a control symbol TeX reads in math:
/// spaces (`\,`), escaped specials (`\_`, `\{`), and `\\`.
fn is_control_symbol(c: char) -> bool {
    matches!(
        c,
        ',' | ';' | ':' | ' ' | '!' | '>' | '_' | '&' | '{' | '}' | '%' | '#' | '$' | '|' | '\\'
    )
}

/// The Greek letters (`α`–`ω`, `Α`–`Ω`) mhchem reads as element letters.
fn is_greek_char(c: char) -> bool {
    matches!(c, '\u{3B1}'..='\u{3C9}' | '\u{391}'..='\u{3A9}')
}

/// The Greek letter names mhchem reads as element letters.
fn is_greek(name: &str) -> bool {
    matches!(
        name,
        "alpha"
            | "beta"
            | "gamma"
            | "delta"
            | "epsilon"
            | "zeta"
            | "eta"
            | "theta"
            | "iota"
            | "kappa"
            | "lambda"
            | "mu"
            | "nu"
            | "xi"
            | "omicron"
            | "pi"
            | "rho"
            | "sigma"
            | "tau"
            | "upsilon"
            | "phi"
            | "chi"
            | "psi"
            | "omega"
            | "Gamma"
            | "Delta"
            | "Theta"
            | "Lambda"
            | "Xi"
            | "Pi"
            | "Sigma"
            | "Upsilon"
            | "Phi"
            | "Psi"
            | "Omega"
    )
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

    #[test]
    fn deep_nesting_stays_bounded() {
        // Nested `\ce`/`\frac` bodies are re-read only to a fixed depth, so
        // deep input cannot overflow the stack.
        for open in [r"\ce{", r"\frac{", r"\overset{"] {
            let body = format!("{}x{}", open.repeat(20_000), "}{1}".repeat(20_000));
            let out = super::ce_to_latex(&body);
            assert!(out.len() < body.len() * 4, "{open}");
        }
    }
}
