use std::collections::HashMap;

use super::packages;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MacroDef {
    param_count: u8,
    optional_default: Option<String>,
    body: String,
}

#[derive(Debug, Clone)]
struct EnvDef {
    begin: String,
    end: String,
}

#[derive(Debug, Default)]
struct Preprocessor {
    macros: HashMap<String, MacroDef>,
    environments: HashMap<String, EnvDef>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Lower bound on the macro-expansion output cap. Inputs smaller than
/// `MIN_EXPANSION_CAP / 64` bytes are still permitted to expand up to this
/// many bytes, so ordinary `\newcommand` use is never penalized.
const MIN_EXPANSION_CAP: usize = 1 << 20; // 1 MiB

/// Maximum number of bytes macro expansion may produce for a given input.
///
/// Scales with the input so large legitimate documents still expand, while a
/// tiny self-replicating macro is capped at the 1 MiB floor.
fn expansion_cap(input_len: usize) -> usize {
    input_len.saturating_mul(64).max(MIN_EXPANSION_CAP)
}

/// Main entry point: strip comments, extract definitions, expand macros.
///
/// **Not DoS-safe.** On macro-expansion overflow this returns the original
/// input unchanged, silently swallowing the [`preprocess_cow`] error. It is
/// `pub(crate)` (not part of the public API) and used only by this module's
/// tests; all crate parsing goes through the fallible [`preprocess_cow`], which
/// surfaces `LatexParseError::ExpansionLimit`. Callers that accept untrusted
/// input must use [`preprocess_cow`].
#[allow(dead_code)] // Infallible counterpart to `preprocess_cow`; exercised by this module's tests.
pub(crate) fn preprocess(input: &str) -> String {
    match preprocess_cow(input) {
        Ok(cow) => cow.into_owned(),
        Err(_) => input.to_owned(),
    }
}

/// Like the (crate-private) `preprocess` but borrows the input on the common
/// fast path (no macros or comments), avoiding an allocation + full-string copy
/// per parse.
///
/// Returns `Err(limit)` when macro expansion would exceed `limit` bytes,
/// protecting against exponential-blowup denial-of-service inputs.
pub fn preprocess_cow(input: &str) -> Result<std::borrow::Cow<'_, str>, usize> {
    if !needs_preprocessing(input) {
        return Ok(std::borrow::Cow::Borrowed(input));
    }
    let stripped = strip_comments(input);
    let mut pp = Preprocessor::default();
    // Seed builtin (package) macros first so that user `\newcommand`/`\def`
    // definitions extracted next override them (user definitions win).
    pp.seed_builtins();
    let after_defs = pp.extract_definitions(&stripped);
    Ok(std::borrow::Cow::Owned(pp.expand_all(&after_defs)?))
}

/// Fast-path check: does the input contain any syntax we need to process?
fn needs_preprocessing(input: &str) -> bool {
    input.contains('%')
        || input.contains("\\newcommand")
        || input.contains("\\renewcommand")
        || input.contains("\\def")
        || input.contains("\\newenvironment")
        || uses_builtin_macro(input)
}

/// Scan `input` for any `\command` that is a registered builtin (package)
/// macro. Returns on the first hit. Plain formulas — even those that use core
/// commands like `\frac` or `\alpha` — return `false` and stay zero-copy.
fn uses_builtin_macro(input: &str) -> bool {
    let mut s = input;
    while let Some(pos) = s.find('\\') {
        let after = &s[pos + 1..];
        let name_len: usize = after
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .map(|c| c.len_utf8())
            .sum();
        if name_len > 0 && packages::is_builtin_macro_name(&after[..name_len]) {
            return true;
        }
        // Advance past this backslash (and any command/escaped char) to avoid
        // re-finding the same `\`.
        let skip = if name_len > 0 {
            name_len
        } else {
            after.chars().next().map_or(0, char::len_utf8)
        };
        if skip == 0 {
            break;
        }
        s = &after[skip..];
    }
    false
}

// ---------------------------------------------------------------------------
// Phase 1 — Comment stripping
// ---------------------------------------------------------------------------

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut prev_was_backslash = false;

    while i < input.len() {
        // Verbatim content must be copied unchanged, including `%` characters.
        if input.as_bytes().get(i) == Some(&b'\\')
            && let Some(end) = find_verb_end(input, i)
        {
            out.push_str(&input[i..end]);
            i = end;
            prev_was_backslash = false;
            continue;
        }

        let ch = input[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();
        if ch == '%' && !prev_was_backslash {
            // Skip until end of line.
            i += ch_len;
            while i < input.len() {
                let c = input[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '\n' {
                    out.push(c);
                    break;
                }
            }
            prev_was_backslash = false;
            continue;
        }
        out.push(ch);
        prev_was_backslash = ch == '\\' && !prev_was_backslash;
        i += ch_len;
    }
    out
}

// ---------------------------------------------------------------------------
// Scanning helpers
// ---------------------------------------------------------------------------

/// If `input[start..]` begins `\verb` or `\verb*` followed by a delimiter,
/// return the byte index just past the matching closing delimiter. Verbatim
/// segments must not be altered by comment stripping or macro expansion.
fn find_verb_end(input: &str, start: usize) -> Option<usize> {
    if !input[start..].starts_with("\\verb") {
        return None;
    }
    let mut i = start + "\\verb".len();
    if input[i..].starts_with('*') {
        i += 1;
    }
    let delim = input[i..].chars().next()?;
    let after_delim = i + delim.len_utf8();
    let end = input[after_delim..].find(delim)?;
    Some(after_delim + end + delim.len_utf8())
}

/// Trim leading whitespace.
fn skip_ws(s: &str) -> &str {
    s.trim_start()
}

/// Consume `{...}` with brace-depth matching. Allows leading whitespace
/// before `{`. Returns `(rest, inner_content)`.
fn scan_braced(s: &str) -> Option<(&str, &str)> {
    let s = skip_ws(s);
    let mut chars = s.chars();
    if chars.next()? != '{' {
        return None;
    }
    let after_open = &s[1..]; // safe: '{' is 1 byte
    let mut depth: u32 = 1;
    let mut end_byte = 0;
    for ch in after_open.chars() {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                let inner = &after_open[..end_byte];
                let rest = &after_open[end_byte + 1..]; // '}' is 1 byte
                return Some((rest, inner));
            }
        }
        end_byte += ch.len_utf8();
    }
    None
}

/// Consume `[...]` respecting nested `{...}`. Allows leading whitespace
/// before `[`. Returns `(rest, inner_content)`.
fn scan_bracket(s: &str) -> Option<(&str, &str)> {
    let s = skip_ws(s);
    let mut chars = s.chars();
    if chars.next()? != '[' {
        return None;
    }
    let after_open = &s[1..]; // safe: '[' is 1 byte
    let mut brace_depth: u32 = 0;
    let mut end_byte = 0;
    for ch in after_open.chars() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ']' if brace_depth == 0 => {
                let inner = &after_open[..end_byte];
                let rest = &after_open[end_byte + 1..]; // ']' is 1 byte
                return Some((rest, inner));
            },
            _ => {},
        }
        end_byte += ch.len_utf8();
    }
    None
}

/// Consume `\name` (backslash + alphabetic run). Returns `(rest, name_without_backslash)`.
fn scan_command(s: &str) -> Option<(&str, &str)> {
    let s = skip_ws(s);
    let mut chars = s.chars();
    if chars.next()? != '\\' {
        return None;
    }
    let after_bs = &s[1..]; // safe: '\' is 1 byte
    let name_len: usize = after_bs
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .map(|c| c.len_utf8())
        .sum();
    if name_len == 0 {
        return None;
    }
    let name = &after_bs[..name_len];
    let rest = &after_bs[name_len..];
    Some((rest, name))
}

// ---------------------------------------------------------------------------
// Phase 2 — Definition extraction
// ---------------------------------------------------------------------------

impl Preprocessor {
    /// Seed the static builtin (package) macro table into this preprocessor.
    ///
    /// Called before [`Self::extract_definitions`], so any user `\newcommand` /
    /// `\def` of the same name later overrides the builtin (user definitions
    /// win). Builtin bodies may reference other builtins or primitives; the
    /// existing 64-iteration cap in [`Self::expand_all`] guards against cycles.
    fn seed_builtins(&mut self) {
        for (name, param_count, body) in packages::builtin_macros() {
            self.macros.insert(
                name.to_owned(),
                MacroDef {
                    param_count,
                    optional_default: None,
                    body: body.to_owned(),
                },
            );
        }
    }

    /// Scan input, recognize definition commands, store them, and return the
    /// remaining (non-definition) text.
    fn extract_definitions(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut s = input;

        while !s.is_empty() {
            // Try each definition form
            if (s.starts_with("\\newcommand") || s.starts_with("\\renewcommand"))
                && let Some(rest) = self.parse_newcommand(s)
            {
                s = rest;
                continue;
            }
            if s.starts_with("\\def") {
                // Guard: \def must be followed by \ or whitespace (not \define, etc.)
                let after = &s[4..];
                if (after.starts_with('\\') || after.starts_with(|c: char| c.is_whitespace()))
                    && let Some(rest) = self.parse_def_cmd(s)
                {
                    s = rest;
                    continue;
                }
            }
            if s.starts_with("\\newenvironment")
                && let Some(rest) = self.parse_newenvironment(s)
            {
                s = rest;
                continue;
            }

            // Copy one char
            let ch = s.chars().next().unwrap();
            out.push(ch);
            s = &s[ch.len_utf8()..];
        }
        out
    }

    /// Parse `\newcommand{\name}[n][default]{body}` or `\renewcommand{...}`.
    /// Skip optional `*` after the command name.
    fn parse_newcommand<'a>(&mut self, s: &'a str) -> Option<&'a str> {
        // Consume \newcommand or \renewcommand
        let rest = s
            .strip_prefix("\\renewcommand")
            .or_else(|| s.strip_prefix("\\newcommand"))?;

        // Optional *
        let rest = skip_ws(rest);
        let rest = rest.strip_prefix('*').unwrap_or(rest);

        // The command name: either {\name} or \name
        let (rest, cmd_name) = if let Some((r, inner)) = scan_braced(rest) {
            // inner should be like \foo
            let inner = inner.trim();
            let name = inner.strip_prefix('\\')?;
            (r, name.to_owned())
        } else if let Some((r, name)) = scan_command(rest) {
            (r, name.to_owned())
        } else {
            return None;
        };

        // Optional [n] — param count
        let mut param_count: u8 = 0;
        let mut rest = rest;
        if let Some((r, n_str)) = scan_bracket(rest)
            && let Ok(n) = n_str.trim().parse::<u8>()
        {
            param_count = n;
            rest = r;
        }

        // Optional [default] for first parameter
        let mut optional_default: Option<String> = None;
        if param_count > 0
            && let Some((r, def)) = scan_bracket(rest)
        {
            optional_default = Some(def.to_owned());
            rest = r;
        }

        // Body {body}
        let (rest, body) = scan_braced(rest)?;

        self.macros.insert(
            cmd_name,
            MacroDef {
                param_count,
                optional_default,
                body: body.to_owned(),
            },
        );
        Some(rest)
    }

    /// Parse `\def\name#1#2{body}`. Count `#n` params between name and `{`.
    fn parse_def_cmd<'a>(&mut self, s: &'a str) -> Option<&'a str> {
        let rest = s.strip_prefix("\\def")?;
        let (rest, name) = scan_command(rest)?;
        let name = name.to_owned();

        // Count #n parameters between name and opening {
        let mut r = rest;
        let mut param_count: u8 = 0;
        loop {
            let trimmed = skip_ws(r);
            if let Some(after_hash) = trimmed.strip_prefix('#')
                && let Some(d) = after_hash.chars().next()
                && d.is_ascii_digit()
            {
                param_count += 1;
                r = &after_hash[d.len_utf8()..];
                continue;
            }
            break;
        }

        let (rest, body) = scan_braced(r)?;

        self.macros.insert(
            name,
            MacroDef {
                param_count,
                optional_default: None,
                body: body.to_owned(),
            },
        );
        Some(rest)
    }

    /// Parse `\newenvironment{name}[n]{begin}{end}`. Skip optional `*` and `[n]`.
    fn parse_newenvironment<'a>(&mut self, s: &'a str) -> Option<&'a str> {
        let rest = s.strip_prefix("\\newenvironment")?;

        // Optional *
        let rest = skip_ws(rest);
        let rest = rest.strip_prefix('*').unwrap_or(rest);

        // {name}
        let (rest, name) = scan_braced(rest)?;
        let name = name.trim().to_owned();

        // Optional [n] — skip it
        let mut rest = rest;
        if let Some((r, _)) = scan_bracket(rest) {
            rest = r;
        }

        // {begin}
        let (rest, begin_body) = scan_braced(rest)?;
        // {end}
        let (rest, end_body) = scan_braced(rest)?;

        self.environments.insert(
            name,
            EnvDef {
                begin: begin_body.to_owned(),
                end: end_body.to_owned(),
            },
        );
        Some(rest)
    }
}

// ---------------------------------------------------------------------------
// Phase 3 — Expansion
// ---------------------------------------------------------------------------

impl Preprocessor {
    /// Iterate `expand_once` up to 64 times until stable.
    ///
    /// Returns `Err(cap)` if expansion output exceeds `cap` bytes, which
    /// guards against exponential-blowup macros such as
    /// `\newcommand{\a}{\a\a}\a` whose output doubles every pass. The 64-pass
    /// iteration limit alone is *not* sufficient — a single self-doubling
    /// macro reaches ~`2^64` bytes well before 64 passes — so output size is
    /// bounded explicitly here and inside [`Self::expand_once`].
    fn expand_all(&self, input: &str) -> Result<String, usize> {
        if self.macros.is_empty() && self.environments.is_empty() {
            return Ok(input.to_owned());
        }
        let cap = expansion_cap(input.len());
        let mut text = input.to_owned();
        for _ in 0..64 {
            let (next, changed) = self.expand_once(&text, cap);
            if next.len() > cap {
                return Err(cap);
            }
            if !changed {
                return Ok(next);
            }
            text = next;
        }
        Ok(text)
    }

    /// Single-pass expansion. Returns `(output, changed)`.
    ///
    /// Stops appending once `out` grows past `cap`, bounding peak memory to
    /// roughly `cap` plus one macro body even when a single pass would
    /// otherwise multiply the input many-fold. The caller ([`Self::expand_all`])
    /// detects the over-cap output and reports an error.
    fn expand_once(&self, input: &str, cap: usize) -> (String, bool) {
        let mut out = String::with_capacity(input.len());
        let mut changed = false;
        let mut s = input;

        while !s.is_empty() {
            // Bail out early on runaway expansion; the caller turns the
            // over-cap result into an `ExpansionLimit` error.
            if out.len() > cap {
                break;
            }
            // Try \begin{name}
            if s.starts_with("\\begin{")
                && let Some((rest, replacement)) = self.try_expand_begin(s)
            {
                out.push_str(&replacement);
                s = rest;
                changed = true;
                continue;
            }
            // Try \end{name}
            if s.starts_with("\\end{")
                && let Some((rest, replacement)) = self.try_expand_end(s)
            {
                out.push_str(&replacement);
                s = rest;
                changed = true;
                continue;
            }
            // Skip \verb / \verb* content unchanged — macro expansion must not
            // rewrite text inside a verbatim block.
            if let Some(end) = find_verb_end(s, 0) {
                out.push_str(&s[..end]);
                s = &s[end..];
                continue;
            }
            // Try \name macro expansion
            if s.starts_with('\\')
                && let Some((cmd_rest, name)) = scan_command(s)
                && let Some(def) = self.macros.get(name)
            {
                let def = def.clone();
                if let Some((rest, args)) = consume_args(cmd_rest, &def) {
                    let replaced = substitute(&def.body, &args);
                    out.push_str(&replaced);
                    s = rest;
                    changed = true;
                    continue;
                }
            }
            // Copy one char
            let ch = s.chars().next().unwrap();
            out.push(ch);
            s = &s[ch.len_utf8()..];
        }
        (out, changed)
    }

    /// Expand `\begin{name}` if name is in the environment table.
    fn try_expand_begin<'a>(&self, s: &'a str) -> Option<(&'a str, String)> {
        let rest = s.strip_prefix("\\begin")?;
        let (rest, name) = scan_braced(rest)?;
        let env = self.environments.get(name)?;
        Some((rest, env.begin.clone()))
    }

    /// Expand `\end{name}` if name is in the environment table.
    fn try_expand_end<'a>(&self, s: &'a str) -> Option<(&'a str, String)> {
        let rest = s.strip_prefix("\\end")?;
        let (rest, name) = scan_braced(rest)?;
        let env = self.environments.get(name)?;
        Some((rest, env.end.clone()))
    }
}

/// Consume arguments after a command name according to the macro definition.
fn consume_args<'a>(s: &'a str, def: &MacroDef) -> Option<(&'a str, Vec<String>)> {
    if def.param_count == 0 {
        return Some((s, vec![]));
    }
    let mut args: Vec<String> = Vec::new();
    let mut rest = s;
    let total = def.param_count as usize;

    // First arg may be optional [...]
    if let Some(default_val) = &def.optional_default {
        if let Some((r, val)) = scan_bracket(rest) {
            args.push(val.to_owned());
            rest = r;
        } else {
            // Use default
            args.push(default_val.clone());
        }
    }

    // Remaining required args: {..} or single token
    while args.len() < total {
        let trimmed = skip_ws(rest);
        if let Some((r, inner)) = scan_braced(trimmed) {
            args.push(inner.to_owned());
            rest = r;
        } else if let Some((r, tok)) = consume_single_token(trimmed) {
            args.push(tok);
            rest = r;
        } else {
            return None;
        }
    }
    Some((rest, args))
}

/// Consume `\commandname` or one non-whitespace char.
fn consume_single_token(s: &str) -> Option<(&str, String)> {
    let s = skip_ws(s);
    let mut chars = s.chars();
    let first = chars.next()?;
    if first == '\\' {
        // \commandname — alphabetic run
        let after_bs = &s[1..];
        let name_len: usize = after_bs
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .map(|c| c.len_utf8())
            .sum();
        if name_len == 0 {
            // single special char like \{
            let special = after_bs.chars().next()?;
            let tok_len = 1 + special.len_utf8();
            return Some((&s[tok_len..], s[..tok_len].to_owned()));
        }
        let tok_len = 1 + name_len;
        Some((&s[tok_len..], s[..tok_len].to_owned()))
    } else if first.is_whitespace() {
        None
    } else {
        let len = first.len_utf8();
        Some((&s[len..], first.to_string()))
    }
}

/// Replace `#1`..`#9` in body with the corresponding args.
fn substitute(body: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '#' {
            if let Some(d) = chars.next() {
                if let Some(idx) = d.to_digit(10) {
                    let idx = idx as usize;
                    if idx >= 1 && idx <= args.len() {
                        out.push_str(&args[idx - 1]);
                        continue;
                    }
                }
                // Not a valid param ref — output literally
                out.push('#');
                out.push(d);
            } else {
                out.push('#');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Comment stripping ---

    #[test]
    fn strip_simple_comment() {
        assert_eq!(strip_comments("a % comment\nb"), "a \nb");
    }

    #[test]
    fn strip_comment_at_start() {
        assert_eq!(strip_comments("% full line\nb"), "\nb");
    }

    #[test]
    fn strip_comment_at_end_no_newline() {
        assert_eq!(strip_comments("a % trailing"), "a ");
    }

    #[test]
    fn preserve_escaped_percent() {
        assert_eq!(strip_comments("a \\% b"), "a \\% b");
    }

    #[test]
    fn strip_multiple_comments() {
        let input = "a % one\nb % two\nc";
        assert_eq!(strip_comments(input), "a \nb \nc");
    }

    #[test]
    fn no_comments_unchanged() {
        assert_eq!(strip_comments("hello world"), "hello world");
    }

    // --- Scanning helpers ---

    #[test]
    fn scan_braced_simple() {
        let (rest, inner) = scan_braced("{abc}def").unwrap();
        assert_eq!(inner, "abc");
        assert_eq!(rest, "def");
    }

    #[test]
    fn scan_braced_nested() {
        let (rest, inner) = scan_braced("{a{b}c}d").unwrap();
        assert_eq!(inner, "a{b}c");
        assert_eq!(rest, "d");
    }

    #[test]
    fn scan_braced_with_leading_ws() {
        let (rest, inner) = scan_braced("  {x}y").unwrap();
        assert_eq!(inner, "x");
        assert_eq!(rest, "y");
    }

    #[test]
    fn scan_bracket_simple() {
        let (rest, inner) = scan_bracket("[42]rest").unwrap();
        assert_eq!(inner, "42");
        assert_eq!(rest, "rest");
    }

    #[test]
    fn scan_bracket_with_braces_inside() {
        let (rest, inner) = scan_bracket("[a{]}b]c").unwrap();
        assert_eq!(inner, "a{]}b");
        assert_eq!(rest, "c");
    }

    #[test]
    fn scan_command_basic() {
        let (rest, name) = scan_command("\\foo bar").unwrap();
        assert_eq!(name, "foo");
        assert_eq!(rest, " bar");
    }

    #[test]
    fn scan_command_eof() {
        let (rest, name) = scan_command("\\abc").unwrap();
        assert_eq!(name, "abc");
        assert_eq!(rest, "");
    }

    // --- Definition extraction ---

    #[test]
    fn extract_newcommand_no_args() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newcommand{\\foo}{bar}");
        let m = pp.macros.get("foo").unwrap();
        assert_eq!(m.param_count, 0);
        assert!(m.optional_default.is_none());
        assert_eq!(m.body, "bar");
    }

    #[test]
    fn extract_newcommand_with_args() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newcommand{\\my}[1]{(#1)}");
        let m = pp.macros.get("my").unwrap();
        assert_eq!(m.param_count, 1);
        assert!(m.optional_default.is_none());
        assert_eq!(m.body, "(#1)");
    }

    #[test]
    fn extract_newcommand_optional_default() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newcommand{\\xyz}[2][x]{#1 + #2}");
        let m = pp.macros.get("xyz").unwrap();
        assert_eq!(m.param_count, 2);
        assert_eq!(m.optional_default, Some("x".to_owned()));
        assert_eq!(m.body, "#1 + #2");
    }

    #[test]
    fn extract_renewcommand_overwrites() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newcommand{\\foo}{old}\\renewcommand{\\foo}{new}");
        let m = pp.macros.get("foo").unwrap();
        assert_eq!(m.body, "new");
    }

    #[test]
    fn extract_newcommand_star() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newcommand*{\\foo}{bar}");
        let m = pp.macros.get("foo").unwrap();
        assert_eq!(m.body, "bar");
    }

    #[test]
    fn extract_def_no_args() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\def\\hello{world}");
        let m = pp.macros.get("hello").unwrap();
        assert_eq!(m.param_count, 0);
        assert_eq!(m.body, "world");
    }

    #[test]
    fn extract_def_with_params() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\def\\bold#1{\\mathbf{#1}}");
        let m = pp.macros.get("bold").unwrap();
        assert_eq!(m.param_count, 1);
        assert_eq!(m.body, "\\mathbf{#1}");
    }

    #[test]
    fn extract_def_two_params() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\def\\pair#1#2{(#1,#2)}");
        let m = pp.macros.get("pair").unwrap();
        assert_eq!(m.param_count, 2);
        assert_eq!(m.body, "(#1,#2)");
    }

    #[test]
    fn extract_newenvironment() {
        let mut pp = Preprocessor::default();
        pp.extract_definitions("\\newenvironment{myenv}{BEGIN}{END}");
        let e = pp.environments.get("myenv").unwrap();
        assert_eq!(e.begin, "BEGIN");
        assert_eq!(e.end, "END");
    }

    #[test]
    fn extract_multiple_definitions() {
        let mut pp = Preprocessor::default();
        let rest = pp.extract_definitions(
            "\\newcommand{\\a}{A}\\def\\b{B}\\newenvironment{c}{C1}{C2} remaining",
        );
        assert!(pp.macros.contains_key("a"));
        assert!(pp.macros.contains_key("b"));
        assert!(pp.environments.contains_key("c"));
        assert_eq!(rest.trim(), "remaining");
    }

    // --- Expansion ---

    #[test]
    fn expand_no_arg_macro() {
        let result = preprocess("\\newcommand{\\foo}{bar}\\foo");
        assert_eq!(result, "bar");
    }

    #[test]
    fn expand_one_arg_braced() {
        let result = preprocess("\\newcommand{\\my}[1]{(#1)}\\my{x}");
        assert_eq!(result, "(x)");
    }

    #[test]
    fn expand_one_arg_single_token_command() {
        let result = preprocess("\\newcommand{\\my}[1]{(#1)}\\my\\phi");
        assert_eq!(result, "(\\phi)");
    }

    #[test]
    fn expand_one_arg_single_token_char() {
        let result = preprocess("\\newcommand{\\my}[1]{(#1)}\\my 3");
        assert_eq!(result, "(3)");
    }

    #[test]
    fn expand_optional_arg_with_default() {
        let result = preprocess("\\newcommand{\\f}[2][x]{#1+#2}\\f{3}");
        assert_eq!(result, "x+3");
    }

    #[test]
    fn expand_optional_arg_overridden() {
        let result = preprocess("\\newcommand{\\f}[2][x]{#1+#2}\\f[y]{3}");
        assert_eq!(result, "y+3");
    }

    #[test]
    fn expand_def_no_args() {
        let result = preprocess("\\def\\hi{hello}\\hi");
        assert_eq!(result, "hello");
    }

    #[test]
    fn expand_def_with_param() {
        let result = preprocess("\\def\\bold#1{\\mathbf{#1}}\\bold{x}");
        assert_eq!(result, "\\mathbf{x}");
    }

    #[test]
    fn expand_renewcommand() {
        let result = preprocess("\\newcommand{\\foo}{old}\\renewcommand{\\foo}{new}\\foo");
        assert_eq!(result, "new");
    }

    #[test]
    fn expand_environment() {
        let result = preprocess("\\newenvironment{ary}{BEGIN}{END}\\begin{ary}body\\end{ary}");
        assert_eq!(result, "BEGINbodyEND");
    }

    #[test]
    fn expand_recursive() {
        let result = preprocess("\\newcommand{\\inner}{I}\\newcommand{\\outer}{[\\inner]}\\outer");
        assert_eq!(result, "[I]");
    }

    #[test]
    fn expand_chain_three_deep() {
        let result =
            preprocess("\\newcommand{\\a}{X}\\newcommand{\\b}{\\a Y}\\newcommand{\\c}{\\b Z}\\c");
        assert_eq!(result, "X Y Z");
    }

    #[test]
    fn expand_self_referencing_terminates() {
        // \r expands to \r — should terminate after 64 iterations, not hang
        let result = preprocess("\\newcommand{\\r}{\\r}\\r");
        // After 64 iterations it will still be \r — just check it doesn't hang
        assert!(result.contains("\\r"));
    }

    #[test]
    fn expand_doubling_macro_is_capped() {
        // \newcommand{\a}{\a\a}\a doubles its output every pass. Without an
        // output-size cap this is an exponential-blowup DoS. It must terminate
        // quickly and surface an error instead of exhausting memory/CPU.
        let start = std::time::Instant::now();
        let result = preprocess_cow("\\newcommand{\\a}{\\a\\a}\\a");
        assert!(
            result.is_err(),
            "doubling macro should hit the expansion cap"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "expansion cap must terminate in well under a second"
        );
    }

    #[test]
    fn expand_tripling_macro_is_capped() {
        let result = preprocess_cow("\\newcommand{\\a}{\\a\\a\\a}\\a");
        assert!(
            result.is_err(),
            "tripling macro should hit the expansion cap"
        );
    }

    #[test]
    fn preprocess_falls_back_on_expansion_overflow() {
        // The infallible `preprocess` wrapper returns the input unchanged
        // rather than panicking when the cap is hit.
        let input = "\\newcommand{\\a}{\\a\\a}\\a";
        let result = preprocess(input);
        assert_eq!(result, input);
    }

    #[test]
    fn expand_preserves_non_macro_commands() {
        let result = preprocess("\\newcommand{\\foo}{bar}\\alpha + \\foo");
        assert_eq!(result, "\\alpha + bar");
    }

    #[test]
    fn no_macros_passthrough() {
        assert_eq!(preprocess("x+y"), "x+y");
    }

    #[test]
    fn verb_content_not_expanded() {
        // Builtin macros (here the physics package \ip) must not be expanded
        // inside a \verb / \verb* verbatim block.
        assert_eq!(preprocess("\\verb@\\ip!@"), "\\verb@\\ip!@");
        assert_eq!(preprocess("\\verb*!\\ip{!}!"), "\\verb*!\\ip{!}!");
    }

    #[test]
    fn comments_not_stripped_inside_verb() {
        // A percent sign inside \verb content is literal text, not a comment.
        assert_eq!(preprocess("\\verb@% still here@"), "\\verb@% still here@");
    }
}
