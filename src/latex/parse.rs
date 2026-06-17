use nom::IResult;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::complete::char;
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Span,
    Strictness,
};
use crate::unicode::fonts::latex_font_to_style;
use crate::unicode::symbols::{
    is_operator_command, latex_to_unicode, lookup_command, operator_has_limits,
};

use super::error::LatexParseError;
use super::preprocess::preprocess_cow;

pub(crate) const MAX_PARSE_DEPTH: u32 = 128;

// ---------------------------------------------------------------------------
// Byte scanning helpers
// ---------------------------------------------------------------------------

/// Skip past leading ASCII whitespace bytes, returning the count of
/// whitespace bytes consumed.
///
/// All whitespace bytes (0x20 space, 0x09 tab, 0x0A LF, 0x0D CR) are
/// single-byte ASCII, so the returned offset is a valid UTF-8 boundary.
#[inline]
fn skip_ws_bytes(input: &[u8]) -> usize {
    input
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        .unwrap_or(input.len())
}

#[inline]
fn skip_horizontal_ws_bytes(input: &[u8]) -> usize {
    input
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(input.len())
}

/// Byte table of ASCII characters that are recognized as symbol tokens.
const SYMBOL_BYTES: &[u8] = b"+-*/=<>!,;:.?'|@()[]";

// ---------------------------------------------------------------------------
// Standalone helpers (stateless — no arena access needed)
// ---------------------------------------------------------------------------

#[inline]
fn ws(input: &str) -> IResult<&str, ()> {
    let skip = skip_ws_bytes(input.as_bytes());
    Ok((&input[skip..], ()))
}

#[inline]
fn consume_source_newline(input: &str) -> Option<&str> {
    input
        .strip_prefix("\r\n")
        .or_else(|| input.strip_prefix('\n'))
        .or_else(|| input.strip_prefix('\r'))
}

fn source_newline_is_hard_break(full_input: &str, newline_at: &str) -> bool {
    let consumed_len = full_input.len().saturating_sub(newline_at.len());
    full_input[..consumed_len]
        .chars()
        .rev()
        .find(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r'))
        .is_some_and(|ch| matches!(ch, '=' | '<' | '>' | ':' | ';'))
}

#[inline]
fn is_table_boundary(input: &str) -> bool {
    input.is_empty()
        || input.starts_with('&')
        || input.starts_with("\\\\")
        || input.starts_with("\\end")
}

fn classify_char(c: char) -> SymbolKind {
    match c {
        '+' | '-' | '*' => SymbolKind::BinaryOp,
        '/' => SymbolKind::BinaryOp,
        '=' | '<' | '>' => SymbolKind::Relation,
        '!' => SymbolKind::Ordinary,
        ',' | ';' => SymbolKind::Punctuation,
        '.' | ':' | '?' | '\'' | '@' => SymbolKind::Ordinary,
        '|' => SymbolKind::Fence,
        '(' | '[' => SymbolKind::Open,
        ')' | ']' => SymbolKind::Close,
        // All callers guard with SYMBOL_BYTES, so this arm is unreachable.
        _ => unreachable!("classify_char called with non-symbol byte: {c:?}"),
    }
}

fn negate_char(ch: char) -> Option<char> {
    match ch {
        '=' => Some('\u{2260}'),
        '<' => Some('\u{226E}'),
        '>' => Some('\u{226F}'),
        '\u{2208}' => Some('\u{2209}'),
        '\u{2282}' => Some('\u{2284}'),
        '\u{2283}' => Some('\u{2285}'),
        '\u{2286}' => Some('\u{2288}'),
        '\u{2287}' => Some('\u{2289}'),
        '\u{2261}' => Some('\u{2262}'),
        '\u{223C}' => Some('\u{2241}'),
        '\u{2264}' => Some('\u{2270}'),
        '\u{2265}' => Some('\u{2271}'),
        '\u{227A}' => Some('\u{2280}'),
        '\u{227B}' => Some('\u{2281}'),
        '\u{2203}' => Some('\u{2204}'),
        _ => None,
    }
}

/// Process raw LaTeX text-mode content: strip braces, handle `~`, `\ `, dashes, etc.
fn clean_text_content(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'{' | b'}' => {
                i += 1;
            }, // strip grouping braces
            b'~' => {
                out.push('\u{00A0}');
                i += 1;
            }, // non-breaking space
            b'\\' if i + 1 < len => {
                match bytes[i + 1] {
                    b' ' => {
                        out.push(' ');
                        i += 2;
                    },
                    b'&' => {
                        out.push('&');
                        i += 2;
                    },
                    b'#' => {
                        out.push('#');
                        i += 2;
                    },
                    b'$' => {
                        out.push('$');
                        i += 2;
                    },
                    b'%' => {
                        out.push('%');
                        i += 2;
                    },
                    b'_' => {
                        out.push('_');
                        i += 2;
                    },
                    b'\\' => {
                        out.push(' ');
                        i += 2;
                    }, // \\ = line break → space
                    b',' => {
                        out.push('\u{2009}');
                        i += 2;
                    }, // thin space
                    b';' => {
                        out.push('\u{2005}');
                        i += 2;
                    }, // medium space
                    _ => {
                        // Skip unknown commands: consume \word
                        i += 1; // skip backslash
                        let start = i;
                        while i < len && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        if i == start && i < len {
                            // Single non-alpha char after \, pass through
                            let ch = raw[i..].chars().next().unwrap();
                            out.push(ch);
                            i += ch.len_utf8();
                        }
                        // Otherwise silently drop the command name
                    },
                }
            },
            b'-' if i + 2 < len && bytes[i + 1] == b'-' && bytes[i + 2] == b'-' => {
                out.push('\u{2014}'); // em-dash
                i += 3;
            },
            b'-' if i + 1 < len && bytes[i + 1] == b'-' => {
                out.push('\u{2013}'); // en-dash
                i += 2;
            },
            b'`' if i + 1 < len && bytes[i + 1] == b'`' => {
                out.push('\u{201C}'); // left double quote
                i += 2;
            },
            b'\'' if i + 1 < len && bytes[i + 1] == b'\'' => {
                out.push('\u{201D}'); // right double quote
                i += 2;
            },
            _ => {
                // Use raw str slice to correctly handle multi-byte UTF-8 characters.
                let ch = raw[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            },
        }
    }
    out
}

/// Parse empheq option list (`box=…,left=…,right=…`), returning the raw value
/// strings for the `box`, `left`, and `right` keys. Splitting respects brace
/// depth so values like `\colorbox{a,b}` are not mis-split. Unknown keys are
/// ignored.
fn parse_empheq_options(opts: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (mut box_spec, mut left_spec, mut right_spec) = (None, None, None);
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut bytes_end = opts.len();
    let mut parts: SmallVec<[&str; 4]> = SmallVec::new();
    for (i, c) in opts.char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' if depth > 0 => depth -= 1,
            ',' if depth <= 0 => {
                parts.push(&opts[start..i]);
                start = i + 1;
            },
            _ => {},
        }
        bytes_end = i + c.len_utf8();
    }
    if start <= bytes_end {
        parts.push(&opts[start..]);
    }
    for part in parts {
        let part = part.trim();
        if let Some((key, val)) = part.split_once('=') {
            let val = val.trim().to_string();
            match key.trim() {
                "box" => box_spec = Some(val),
                "left" => left_spec = Some(val),
                "right" => right_spec = Some(val),
                _ => {},
            }
        }
    }
    (box_spec, left_spec, right_spec)
}

/// Extract the `n`-th `{…}` group from a slice that begins after a command
/// name (e.g. the `{yellow}` of `\colorbox{yellow}`). Returns `""` when the
/// requested group is absent.
fn empheq_brace_arg(mut s: &str, n: usize) -> &str {
    let mut idx = 0;
    loop {
        let open = match s.find('{') {
            Some(p) => p,
            None => return "",
        };
        let after = &s[open + 1..];
        let mut depth = 1i32;
        let mut close = None;
        for (i, c) in after.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                },
                _ => {},
            }
        }
        let close = match close {
            Some(p) => p,
            None => return "",
        };
        if idx == n {
            return &after[..close];
        }
        idx += 1;
        s = &after[close + 1..];
    }
}

/// Per-cell colortbl background directives gathered while scanning the leading
/// tokens of a table cell. Used to wrap cell content in a [`CompactNode::Boxed`]
/// with a background color.
#[derive(Default)]
struct ColortblCell {
    /// Background from `\cellcolor` (or an inline `\columncolor`); takes
    /// precedence over a row background for this cell.
    cell_bg: Option<String>,
    /// Background from `\rowcolor`; applies to every cell of the row.
    row_bg: Option<String>,
}

/// If `input` begins with the control sequence `\<name>` (and the following
/// byte is not alphabetic, so `\rowcolors` is not mistaken for `\rowcolor`),
/// return the slice immediately after the command name.
fn strip_control_word<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let after = input.strip_prefix('\\')?.strip_prefix(name)?;
    if after
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
    {
        None
    } else {
        Some(after)
    }
}

fn parse_column_spec(spec: &str) -> (SmallVec<[ColumnAlign; 4]>, u32) {
    let mut result = SmallVec::new();
    let mut seps: u32 = 0;
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'l' => result.push(ColumnAlign::Left),
            'c' => result.push(ColumnAlign::Center),
            'r' => result.push(ColumnAlign::Right),
            '|' => {
                // Set separator bit at current column boundary.
                // Position = number of alignment columns seen so far.
                let pos = result.len();
                if pos < 32 {
                    seps |= 1 << pos;
                }
            },
            '*'
                // *{n}{pattern} — repeat pattern n times
                if chars.peek() == Some(&'{') => {
                    chars.next(); // consume '{'
                    let mut n_str = String::new();
                    for ch in chars.by_ref() {
                        if ch == '}' {
                            break;
                        }
                        n_str.push(ch);
                    }
                    let n: usize = n_str.trim().parse().unwrap_or(1);
                    if chars.peek() == Some(&'{') {
                        chars.next(); // consume '{'
                        let mut pattern = String::new();
                        let mut depth = 1u32;
                        for ch in chars.by_ref() {
                            if ch == '{' {
                                depth += 1;
                            }
                            if ch == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            pattern.push(ch);
                        }
                        let (sub, _sub_seps) = parse_column_spec(&pattern);
                        for _ in 0..n {
                            result.extend_from_slice(&sub);
                        }
                    }
                },
            _ => {}, // skip :, @{}, and other decorations
        }
    }
    (result, seps)
}

fn parse_row_separator(input: &str) -> IResult<&str, (Option<&str>, bool)> {
    let (input, _) = ws(input)?;
    // Accept \\ or \cr as row separator
    let input = if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\cr")(input) {
        // \cr must not be followed by alphabetic (to avoid matching \cross etc.)
        if rest
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        rest
    } else {
        let (rest, _) = tag("\\\\")(input)?;
        rest
    };
    let (input, _) = ws(input)?;
    // Optionally consume [dimension] like [1ex], [3pt]; capture it so the caller
    // can apply the extra inter-row space.
    let (input, extra) = if input.starts_with('[') {
        if let Some(end) = input.find(']') {
            (&input[end + 1..], Some(&input[1..end]))
        } else {
            (input, None)
        }
    } else {
        (input, None)
    };
    let (input, _) = ws(input)?;
    // Consume any \hline or \hdashline directives that follow; report whether a
    // solid \hline was present so the caller can record a horizontal rule.
    let (input, hline) = take_table_directives(input);
    Ok((input, (extra, hline)))
}

// ---------------------------------------------------------------------------
// amscd `CD` environment helpers (commutative diagrams)
// ---------------------------------------------------------------------------

/// One lexical token inside a `CD` row.
struct CdToken<'a> {
    /// Whether this token belongs in an *object* column (objects and vertical
    /// connectors `@V`/`@A`/`@|`) as opposed to a *connector* column
    /// (horizontal connectors `@>`/`@<`/`@=`).
    is_object: bool,
    /// Raw text of the token (object text, or the full `@...` command).
    text: &'a str,
}

/// Length (in bytes) of the `CD` body, i.e. the offset of the matching
/// top-level `\end`. Brace-nested `\end` (inside labels) is ignored.
fn cd_body_len(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'\\' if depth == 0 && input[i..].starts_with("\\end") => return i,
            _ => {},
        }
        i += 1;
    }
    input.len()
}

/// Split a `CD` body into rows on top-level `\\` (or `\cr`).
fn cd_split_rows(body: &str) -> SmallVec<[&str; 4]> {
    let bytes = body.as_bytes();
    let mut rows: SmallVec<[&str; 4]> = SmallVec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'\\' if depth == 0 && bytes.get(i + 1) == Some(&b'\\') => {
                rows.push(&body[start..i]);
                i += 2;
                start = i;
                continue;
            },
            _ => {},
        }
        i += 1;
    }
    rows.push(&body[start..]);
    rows
}

/// Tokenize a single `CD` row into objects and `@`-connectors. Whitespace and
/// `&` act as separators; an object run extends until the next `@`/`&` at brace
/// depth 0. Empty object runs are dropped.
fn cd_tokenize_row(row: &str) -> SmallVec<[CdToken<'_>; 8]> {
    let bytes = row.as_bytes();
    let mut tokens: SmallVec<[CdToken<'_>; 8]> = SmallVec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == b'&' {
            i += 1;
            continue;
        }
        if b == b'@' {
            let end = cd_connector_end(row, i);
            let text = &row[i..end];
            // Classify by the char following '@'.
            let kind = text.as_bytes().get(1).copied().unwrap_or(b'.');
            let is_object = matches!(kind, b'V' | b'A' | b'|' | b'\\');
            tokens.push(CdToken { is_object, text });
            i = end;
        } else {
            // Object run until next '@'/'&' at depth 0.
            let start = i;
            let mut depth = 0i32;
            while i < bytes.len() {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' if depth > 0 => depth -= 1,
                    b'@' | b'&' if depth == 0 => break,
                    _ => {},
                }
                i += 1;
            }
            let text = row[start..i].trim();
            if !text.is_empty() {
                tokens.push(CdToken {
                    is_object: true,
                    text,
                });
            }
        }
    }
    tokens
}

/// Given the byte offset of an `@` in `row`, return the end offset of the
/// connector command. Delimiter forms (`@>..>..>`) consume three delimiter
/// characters at brace depth 0; `@.`/`@=`/`@|` consume two bytes; `@\vert`
/// consumes the backslash command.
fn cd_connector_end(row: &str, at: usize) -> usize {
    let bytes = row.as_bytes();
    let Some(&kind) = bytes.get(at + 1) else {
        return row.len();
    };
    match kind {
        b'>' | b'<' | b'V' | b'A' => {
            let mut depth = 0i32;
            let mut count = 0;
            let mut i = at + 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' if depth > 0 => depth -= 1,
                    _ if depth == 0 && bytes[i] == kind => {
                        count += 1;
                        if count == 3 {
                            return i + 1;
                        }
                    },
                    _ => {},
                }
                i += 1;
            }
            row.len()
        },
        b'\\' => {
            let mut i = at + 2;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            i
        },
        // `@.`, `@=`, `@|`
        _ => at + 2,
    }
}

/// Split `s` on `delim` (an ASCII char) at brace depth 0.
fn cd_split_delim(s: &str, delim: u8) -> SmallVec<[&str; 4]> {
    let bytes = s.as_bytes();
    let mut parts: SmallVec<[&str; 4]> = SmallVec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            _ if depth == 0 && bytes[i] == delim => {
                parts.push(&s[start..i]);
                start = i + 1;
            },
            _ => {},
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

// ── bussproofs / xypic helpers ─────────────────────────────────────────

/// Build a `Rational` (in em) from a floating-point value with hundredths
/// precision, clamped to a sane positive range.
fn rat_em(em: f64) -> Rational {
    let h = (em * 100.0).round().clamp(1.0, 1_000_000.0) as i32;
    Rational::new(h, 100)
}

/// If the next non-whitespace char is `{`, consume a brace-balanced group and
/// return `(rest_after, Some(inner))`; otherwise `(input, None)`.
fn bussproofs_take_arg(input: &str) -> (&str, Option<&str>) {
    match super::mhchem::capture_braced(input) {
        Some((rest, inner)) => (rest, Some(inner)),
        None => (input, None),
    }
}

/// Return the byte offset just past the first top-level `\DisplayProof` (or
/// `\DP`) command in `input`, or `input.len()` when none is present.
fn bussproofs_inline_end(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'\\' if depth == 0 => {
                let rest = &input[i + 1..];
                for cmd in ["DisplayProof", "DP"] {
                    if let Some(after) = rest.strip_prefix(cmd)
                        && !after.starts_with(|c: char| c.is_ascii_alphabetic())
                    {
                        return i + 1 + cmd.len();
                    }
                }
            },
            _ => {},
        }
        i += 1;
    }
    input.len()
}

/// Map an xypic direction string (`r`/`l`/`u`/`d` combinations) to an arrow
/// glyph approximating that direction.
fn xy_arrow_glyph(dir: &str) -> char {
    let u = dir.contains('u');
    let d = dir.contains('d');
    let l = dir.contains('l');
    let r = dir.contains('r');
    match (u, d, l, r) {
        (true, _, true, _) => '\u{2196}', // ↖
        (true, _, _, true) => '\u{2197}', // ↗
        (_, true, true, _) => '\u{2199}', // ↙
        (_, true, _, true) => '\u{2198}', // ↘
        (true, _, _, _) => '\u{2191}',    // ↑
        (_, true, _, _) => '\u{2193}',    // ↓
        (_, _, true, _) => '\u{2190}',    // ←
        _ => '\u{2192}',                  // → (default / `r`)
    }
}

/// Split an xypic row into cells on top-level `&` (ignoring `{}` and `[]`).
fn xy_split_cells(row: &str) -> SmallVec<[&str; 8]> {
    let bytes = row.as_bytes();
    let mut out: SmallVec<[&str; 8]> = SmallVec::new();
    let mut bdepth = 0i32;
    let mut sdepth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => bdepth += 1,
            b'}' if bdepth > 0 => bdepth -= 1,
            b'[' => sdepth += 1,
            b']' if sdepth > 0 => sdepth -= 1,
            b'&' if bdepth == 0 && sdepth == 0 => {
                out.push(&row[start..i]);
                start = i + 1;
            },
            _ => {},
        }
        i += 1;
    }
    out.push(&row[start..]);
    out
}

/// Find the byte offset of the next top-level `\ar` command in `s`, or
/// `s.len()` when none remains.
fn xy_find_ar(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'\\'
                if depth == 0
                    && s[i..].starts_with("\\ar")
                    && !bytes.get(i + 3).is_some_and(|b| b.is_ascii_alphabetic()) =>
            {
                return i;
            },
            _ => {},
        }
        i += 1;
    }
    s.len()
}

/// Consume a `[dir]` bracket group, returning `(rest_after, dir)`. Any
/// modifiers before the bracket (e.g. `@/^/`) are skipped.
fn xy_take_bracket(input: &str) -> (&str, &str) {
    if let Some(open) = input.find('[')
        && let Some(close_rel) = input[open + 1..].find(']')
    {
        let dir = &input[open + 1..open + 1 + close_rel];
        let rest = &input[open + 1 + close_rel + 1..];
        return (rest, dir);
    }
    (input, "")
}

/// Consume a script argument following `^`/`_`: a braced group or a single
/// token (one command or one character). Returns `(rest_after, content)`.
fn xy_take_script(input: &str) -> (&str, &str) {
    let t = input.trim_start();
    if let Some((rest, inner)) = super::mhchem::capture_braced(t) {
        return (rest, inner);
    }
    if let Some(after_bs) = t.strip_prefix('\\') {
        let nlen = after_bs
            .as_bytes()
            .iter()
            .take_while(|b| b.is_ascii_alphabetic())
            .count()
            .max(1);
        let end = 1 + nlen;
        let end = end.min(t.len());
        return (&t[end..], &t[..end]);
    }
    let ch_len = t.chars().next().map_or(0, char::len_utf8);
    (&t[ch_len..], &t[..ch_len])
}

/// Consume leading `\hline`/`\hdashline` directives, reporting whether at least
/// one solid `\hline` (not merely `\hdashline`) was seen. Used to record
/// horizontal-rule positions for array rendering.
fn take_table_directives(input: &str) -> (&str, bool) {
    let mut had_hline = false;
    let mut cur = input;
    loop {
        let trimmed = cur.trim_start();
        if let Some(rest) = trimmed.strip_prefix("\\hline") {
            // Ensure it's not a prefix of a longer command
            if rest
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic())
            {
                break;
            }
            had_hline = true;
            cur = rest;
        } else if let Some(rest) = trimmed.strip_prefix("\\hdashline") {
            if rest
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic())
            {
                break;
            }
            cur = rest;
        } else {
            break;
        }
    }
    (cur, had_hline)
}

fn parse_cell_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    let (input, _) = char('&')(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

fn nom_err_to_latex_error(
    e: &nom::Err<nom::error::Error<&str>>,
    original_input: &str,
) -> LatexParseError {
    match e {
        nom::Err::Error(inner) | nom::Err::Failure(inner) => {
            let offset = original_input.len().saturating_sub(inner.input.len());
            let end = (offset + inner.input.chars().take(20).count()).min(original_input.len());
            let span = Some(Span::new(offset, end));
            match inner.code {
                nom::error::ErrorKind::TooLarge => LatexParseError::DepthExceeded {
                    depth: MAX_PARSE_DEPTH,
                    span,
                },
                nom::error::ErrorKind::Tag => LatexParseError::UnmatchedDelimiter {
                    delimiter: inner.input.chars().take(10).collect(),
                    span,
                },
                _ => LatexParseError::UnexpectedToken {
                    token: inner.input.chars().take(20).collect(),
                    span,
                },
            }
        },
        nom::Err::Incomplete(_) => LatexParseError::UnexpectedToken {
            token: String::new(),
            span: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a LaTeX string into a compact arena representation.
///
/// This is the primary entry point for LaTeX parsing. The parser operates
/// directly on `&str` slices and builds into an arena, avoiding per-node
/// heap allocations on the hot path.
pub fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(CompactParseResult::new(CompactExpr::new(
            AstArena::new(),
            NodeSpan::EMPTY,
            config.display,
        )));
    }
    let processed = preprocess_cow(trimmed)
        .map_err(|limit| Error::from(LatexParseError::ExpansionLimit { limit }))?;
    let mut parser = LatexCompactParser::with_capacity(processed.len());
    match parser.parse_node_list(&processed) {
        Ok(("", ids)) => {
            let roots = parser.arena.push_children(&ids);
            let display = if parser.display_override {
                DisplayStyle::Block
            } else {
                config.display
            };
            Ok(CompactParseResult::new(CompactExpr::new(
                parser.arena,
                roots,
                display,
            )))
        },
        Ok((rest, ids)) => {
            if config.strictness == Strictness::Strict {
                Err(Error::from(LatexParseError::UnexpectedToken {
                    token: rest.chars().take(20).collect(),
                    span: Some(Span::new(input.len() - rest.len(), input.len())),
                }))
            } else {
                let mut diagnostics = vec![Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message: format!("ignored trailing input: {rest:?}"),
                    span: Some(Span::new(input.len() - rest.len(), input.len())),
                }];
                let mut result_ids = ids;
                let recovered = parser.parse_lenient_remainder(rest, &mut diagnostics);
                result_ids.extend(recovered);
                let roots = parser.arena.push_children(&result_ids);
                let display = if parser.display_override {
                    DisplayStyle::Block
                } else {
                    config.display
                };
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, roots, display),
                    diagnostics,
                ))
            }
        },
        Err(ref e) => {
            if config.strictness == Strictness::Lenient {
                let mut diagnostics = Vec::new();
                let ids = parser.parse_lenient_remainder(&processed, &mut diagnostics);
                diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message: format!("parse error (recovered): {e}"),
                    span: None,
                });
                let roots = parser.arena.push_children(&ids);
                let display = if parser.display_override {
                    DisplayStyle::Block
                } else {
                    config.display
                };
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, roots, display),
                    diagnostics,
                ))
            } else {
                Err(Error::from(nom_err_to_latex_error(e, input)))
            }
        },
    }
}

/// Parse a LaTeX string into the heap-allocated `MathExpr` AST.
///
/// Delegates to [`parse_compact`] and materializes the result.
pub fn parse(input: &str, config: &Config) -> Result<ParseResult> {
    parse_compact(input, config).map(|r| r.to_parse_result())
}

// ---------------------------------------------------------------------------
// Compact parser
// ---------------------------------------------------------------------------

pub(crate) struct LatexCompactParser {
    pub(crate) arena: AstArena,
    depth: u32,
    /// Set to `true` when `\displaystyle` is encountered at the top level.
    pub(crate) display_override: bool,
    /// User-defined colors registered via `\definecolor`/`\colorlet`,
    /// keyed by exact name, resolved to RGB bytes.
    colors: HashMap<String, [u8; 3]>,
}

impl LatexCompactParser {
    /// Create a parser with pre-allocated arena capacity based on input length.
    ///
    /// Heuristic: a typical LaTeX formula produces roughly 1 node per 3 input bytes,
    /// with similar proportions for children and strings.
    fn with_capacity(input_len: usize) -> Self {
        let est_nodes = input_len / 3 + 4;
        let est_children = est_nodes;
        let est_strings = input_len;
        Self {
            arena: AstArena::with_capacity(est_nodes, est_children, est_strings),
            depth: 0,
            display_override: false,
            colors: HashMap::new(),
        }
    }

    /// Intern a single char as a string, avoiding heap allocation.
    #[inline]
    fn push_char_str(&mut self, c: char) -> StrId {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.arena.push_str(s)
    }

    /// If exactly one element, return it; otherwise wrap in a Row.
    fn flatten_single(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    /// Check whether a parsed node list contains an infix fraction marker
    /// (`\over`, `\atop`, `\choose`, `\brace`, `\brack`). If found, split
    /// into numerator/denominator and produce the appropriate node.
    fn check_infix_fraction(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        // Scan for the first infix marker
        let mut marker_pos = None;
        let mut marker_kind = "";
        for (i, &id) in ids.iter().enumerate() {
            if let CompactNode::Identifier(sid) = *self.arena.node(id) {
                let s = self.arena.str_val(sid);
                if s.starts_with("__infix_") && s.ends_with("__") {
                    marker_kind = match s {
                        "__infix_over__" => "over",
                        "__infix_atop__" => "atop",
                        "__infix_choose__" => "choose",
                        "__infix_brace__" => "brace",
                        "__infix_brack__" => "brack",
                        _ => continue,
                    };
                    marker_pos = Some(i);
                    break;
                }
            }
        }
        let pos = marker_pos?;

        let num_ids: SmallVec<[NodeId; 8]> = ids[..pos].to_vec().into();
        let den_ids: SmallVec<[NodeId; 8]> = ids[pos + 1..].to_vec().into();

        let num_id = self.flatten_single(&num_ids);
        let den_id = self.flatten_single(&den_ids);

        match marker_kind {
            "over" => {
                // \over → normal fraction with bar
                Some(self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::Normal,
                    num: num_id,
                    den: den_id,
                }))
            },
            "atop" => {
                // \atop → fraction with no bar
                Some(self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num: num_id,
                    den: den_id,
                }))
            },
            "choose" => {
                // \choose → binom (no bar, parenthesized)
                let frac_id = self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num: num_id,
                    den: den_id,
                });
                let open = self.arena.push_str("(");
                let close = self.arena.push_str(")");
                let content = self.arena.push_children(&[frac_id]);
                Some(self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }))
            },
            "brace" => {
                // \brace → curly braces
                let frac_id = self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num: num_id,
                    den: den_id,
                });
                let open = self.arena.push_str("{");
                let close = self.arena.push_str("}");
                let content = self.arena.push_children(&[frac_id]);
                Some(self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }))
            },
            "brack" => {
                // \brack → square brackets
                let frac_id = self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num: num_id,
                    den: den_id,
                });
                let open = self.arena.push_str("[");
                let close = self.arena.push_str("]");
                let content = self.arena.push_children(&[frac_id]);
                Some(self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }))
            },
            _ => None,
        }
    }

    /// Check whether a node is a large-operator symbol (n-ary).
    fn is_nary_id(&self, id: NodeId) -> bool {
        matches!(
            *self.arena.node(id),
            CompactNode::Symbol(SymbolKind::LargeOp, _)
        )
    }

    /// Build an Array node from column alignments and a row-major grid of cells.
    fn build_array(
        &mut self,
        columns: &[ColumnAlign],
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
    ) -> NodeId {
        self.build_array_with_spacing(columns, rows, 100, false, 0, 0)
    }

    fn build_array_with_seps_rows(
        &mut self,
        columns: &[ColumnAlign],
        col_seps: u32,
        row_seps: u32,
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
    ) -> NodeId {
        self.build_array_with_spacing(columns, rows, 100, false, col_seps, row_seps)
    }

    /// Build an Array node with explicit spacing parameters.
    fn build_array_with_spacing(
        &mut self,
        columns: &[ColumnAlign],
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
        arraystretch_pct: u8,
        add_jot: bool,
        col_seps: u32,
        row_seps: u32,
    ) -> NodeId {
        // Compute the actual number of columns as the maximum of the declared
        // column spec and the widest row.  This ensures the flat cell array
        // stride is correct even when rows contain more `&` separators than
        // the column spec declares (e.g. multi-pair `aligned` environments).
        let max_row_len = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let num_cols = columns.len().max(max_row_len).max(1) as u16;

        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for &col in columns {
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(col)));
        }
        let col_aligns = self.arena.push_children(&align_ids);

        let mut cell_ids: SmallVec<[NodeId; 16]> = SmallVec::new();
        for row in &rows {
            for cell in row {
                let span = self.arena.push_children(cell);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
            // Pad short rows to num_cols
            for _ in row.len()..(num_cols as usize) {
                cell_ids.push(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)));
            }
        }
        let cells = self.arena.push_children(&cell_ids);
        self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            col_seps,
            row_seps,
        })
    }

    /// Parse a delimiter and intern the result directly into the arena.
    ///
    /// Zero-allocation: returns `StrId` instead of intermediate `String`.
    /// For command delimiters (`\langle`, `\rfloor`, etc.), the Unicode char
    /// is encoded on the stack via `char::encode_utf8` and interned.
    fn parse_delimiter_into<'a>(&mut self, input: &'a str) -> IResult<&'a str, StrId> {
        let c = input.chars().next().ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let rest = &input[c.len_utf8()..];
        match c {
            '(' => Ok((rest, self.arena.push_str("("))),
            ')' => Ok((rest, self.arena.push_str(")"))),
            '[' => Ok((rest, self.arena.push_str("["))),
            ']' => Ok((rest, self.arena.push_str("]"))),
            '|' => Ok((rest, self.arena.push_str("|"))),
            '/' => Ok((rest, self.arena.push_str("/"))),
            '<' => Ok((rest, self.arena.push_str("\u{27E8}"))), // ⟨
            '>' => Ok((rest, self.arena.push_str("\u{27E9}"))), // ⟩
            '.' => Ok((rest, self.arena.push_str(""))),
            '\\' => {
                let c2 = rest.chars().next().ok_or_else(|| {
                    nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
                })?;
                let rest2 = &rest[c2.len_utf8()..];
                match c2 {
                    '{' => Ok((rest2, self.arena.push_str("{"))),
                    '}' => Ok((rest2, self.arena.push_str("}"))),
                    '|' => Ok((rest2, self.arena.push_str("\u{2016}"))),
                    _ if c2.is_ascii_alphabetic() => {
                        let end = rest
                            .find(|ch: char| !ch.is_ascii_alphabetic())
                            .unwrap_or(rest.len());
                        let name = &rest[..end];
                        let after = &rest[end..];
                        let sid = match latex_to_unicode(name) {
                            Some(ch) => self.push_char_str(ch),
                            None => self.arena.push_str(name),
                        };
                        Ok((after, sid))
                    },
                    _ => Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Char,
                    ))),
                }
            },
            _ => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Top-level combinators
    // -----------------------------------------------------------------------

    pub(crate) fn parse_node_list<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let result = self.parse_node_list_inner(input);
        self.depth -= 1;
        result
    }

    fn parse_node_list_inner<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            if self.depth == 1 {
                let skip = skip_horizontal_ws_bytes(rest.as_bytes());
                rest = &rest[skip..];
                if let Some(after_newline) = consume_source_newline(rest) {
                    if source_newline_is_hard_break(input, rest) {
                        ids.push(self.arena.push_node(CompactNode::Newline));
                    }
                    rest = after_newline;
                    continue;
                }
            } else {
                let (r, _) = ws(rest)?;
                rest = r;
            }
            match self.parse_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        Ok((rest, ids))
    }

    /// Parse one element: an atom optionally followed by scripts.
    #[inline]
    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Handle leading ^/_ /' with implicit empty base (e.g., {^3+1})
        if input.starts_with('^') || input.starts_with('_') || input.starts_with('\'') {
            let base = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            return self.parse_scripts(input, base);
        }
        let (input, base) = self.parse_atom(input)?;

        // N-ary operators consume scripts + body specially
        if self.is_nary_id(base) {
            return self.parse_nary_rest(input, base);
        }

        // Check for \limits/\nolimits after Operator nodes
        if let CompactNode::Operator { name, .. } = *self.arena.node(base) {
            let (trimmed, _) = ws(input)?;
            if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\limits")(trimmed)
                && rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
            {
                let new_id = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: LimitStyle::AboveBelow,
                });
                return self.parse_scripts(rest, new_id);
            }
            if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(trimmed)
                && rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
            {
                let new_id = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: LimitStyle::SubSup,
                });
                return self.parse_scripts(rest, new_id);
            }
        }

        self.parse_scripts(input, base)
    }

    /// Attach optional sub/super scripts to a base node.
    ///
    /// Handles `'` (prime) as LaTeX shorthand for `^{\prime}`.  Multiple
    /// consecutive primes are gathered into a single superscript containing a
    /// row of U+2032 PRIME symbols.  A trailing `_` after primes produces a
    /// `SubSup` node.
    #[inline]
    fn parse_scripts<'a>(&mut self, input: &'a str, base: NodeId) -> IResult<&'a str, NodeId> {
        // Skip whitespace, then check for ^, _, or ' via byte inspection
        let skip = self.skip_script_ws(input.as_bytes());
        let input = &input[skip..];

        // --- Prime (') handling -------------------------------------------
        if input.as_bytes().first() == Some(&b'\'') {
            // Count consecutive prime characters
            let prime_count = input.bytes().take_while(|&b| b == b'\'').count();
            let input = &input[prime_count..];

            // Build prime symbol nodes (one U+2032 per ')
            let mut prime_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
            for _ in 0..prime_count {
                let sid = self.arena.push_str("\u{2032}");
                prime_ids.push(
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
                );
            }

            // If an explicit `^` follows the primes, merge its argument into
            // the superscript group so that e.g. `f'^2` becomes `f^{\prime 2}`
            // instead of two separate superscript nodes.
            let skip_caret = self.skip_script_ws(input.as_bytes());
            let input = if input.as_bytes().get(skip_caret) == Some(&b'^') {
                let after_caret = &input[skip_caret + 1..];
                let (rest, extra_sup) = self.parse_script_arg(after_caret)?;
                prime_ids.push(extra_sup);
                rest
            } else {
                input
            };

            let sup_node = self.flatten_single(&prime_ids);

            // Check for a following subscript (_)
            let skip2 = self.skip_script_ws(input.as_bytes());
            let input2 = &input[skip2..];
            if input2.as_bytes().first() == Some(&b'_') {
                let input2 = &input2[1..];
                let (input2, sub_arg) = self.parse_script_arg(input2)?;
                return Ok((
                    input2,
                    self.arena.push_node(CompactNode::SubSup {
                        base,
                        sub: sub_arg,
                        sup: sup_node,
                    }),
                ));
            }

            return Ok((
                input,
                self.arena.push_node(CompactNode::Superscript {
                    base,
                    sup: sup_node,
                }),
            ));
        }

        // --- Normal ^/_ handling ------------------------------------------
        let first = match input.as_bytes().first() {
            Some(b'^') => '^',
            Some(b'_') => '_',
            _ => return Ok((input, base)),
        };
        let input = &input[1..];

        let (input, first_arg) = self.parse_script_arg(input)?;

        // Check for the complementary script
        let skip2 = self.skip_script_ws(input.as_bytes());
        let input2 = &input[skip2..];
        let second_ch = if first == '^' { b'_' } else { b'^' };
        let has_second = input2.as_bytes().first() == Some(&second_ch);

        if !has_second {
            // When the first script was `_` and there is no `^`, also check
            // for `'` as a complementary superscript so that `f_0'` produces
            // a single SubSup node instead of separate Subscript + Superscript.
            if first == '_' && input2.as_bytes().first() == Some(&b'\'') {
                let prime_count = input2.bytes().take_while(|&b| b == b'\'').count();
                let input2 = &input2[prime_count..];
                let mut prime_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
                for _ in 0..prime_count {
                    let sid = self.arena.push_str("\u{2032}");
                    prime_ids.push(
                        self.arena
                            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
                    );
                }
                let sup_node = self.flatten_single(&prime_ids);
                return Ok((
                    input2,
                    self.arena.push_node(CompactNode::SubSup {
                        base,
                        sub: first_arg,
                        sup: sup_node,
                    }),
                ));
            }
            return if first == '^' {
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Superscript {
                        base,
                        sup: first_arg,
                    }),
                ))
            } else {
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Subscript {
                        base,
                        sub: first_arg,
                    }),
                ))
            };
        }

        let input = &input2[1..]; // consume second script char
        let (input, second_arg) = self.parse_script_arg(input)?;

        if first == '^' {
            Ok((
                input,
                self.arena.push_node(CompactNode::SubSup {
                    base,
                    sub: second_arg,
                    sup: first_arg,
                }),
            ))
        } else {
            Ok((
                input,
                self.arena.push_node(CompactNode::SubSup {
                    base,
                    sub: first_arg,
                    sup: second_arg,
                }),
            ))
        }
    }

    #[inline]
    fn skip_script_ws(&self, input: &[u8]) -> usize {
        if self.depth == 1 {
            skip_horizontal_ws_bytes(input)
        } else {
            skip_ws_bytes(input)
        }
    }

    /// Parse an n-ary operator with optional limits and a body.
    fn parse_nary_rest<'a>(&mut self, input: &'a str, op_id: NodeId) -> IResult<&'a str, NodeId> {
        let CompactNode::Symbol(_, sid) = *self.arena.node(op_id) else {
            return Ok((input, op_id));
        };
        let op_char = self.arena.str_val(sid).chars().next().unwrap_or('\u{2211}');

        let (input, _) = ws(input)?;

        // Check for \limits or \nolimits
        let (input, limit_style) =
            if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\limits")(input) {
                if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
                    (rest, LimitStyle::AboveBelow)
                } else {
                    (input, LimitStyle::Default)
                }
            } else if let Ok((rest, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(input)
            {
                if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
                    (rest, LimitStyle::SubSup)
                } else {
                    (input, LimitStyle::Default)
                }
            } else {
                (input, LimitStyle::Default)
            };

        let (input, _) = ws(input)?;
        let (input, (sub_id, sup_id)) = self.parse_optional_limits(input)?;
        let (input, _) = ws(input)?;

        // The body is the next element (or empty row if nothing follows).
        // Use `parse_element` (not bare `parse_atom`) so a nested n-ary operator
        // body — e.g. the 2nd integral in `\int\limits.. \int\limits.. f` — is
        // itself routed through `parse_nary_rest` and keeps its \limits/limits.
        let (input, body_id) = if is_table_boundary(input) {
            let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            (input, row_id)
        } else {
            match self.parse_element(input) {
                Ok((rest, id)) => (rest, id),
                Err(_) => {
                    let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    (input, row_id)
                },
            }
        };

        Ok((
            input,
            self.arena.push_node(CompactNode::Nary {
                op: op_char,
                sub: sub_id,
                sup: sup_id,
                body: body_id,
                limits: limit_style,
            }),
        ))
    }

    /// Parse optional _{...} and ^{...} in any order, returning (sub, sup).
    /// Uses `NodeId::NONE` for absent limits.
    fn parse_optional_limits<'a>(&mut self, input: &'a str) -> IResult<&'a str, (NodeId, NodeId)> {
        let first_ch = match input.as_bytes().first() {
            Some(b'_') => '_',
            Some(b'^') => '^',
            _ => return Ok((input, (NodeId::NONE, NodeId::NONE))),
        };
        let input = &input[1..];
        let (input, first_arg) = self.parse_script_arg(input)?;
        let skip = self.skip_script_ws(input.as_bytes());
        let input_ws = &input[skip..];
        let second_byte = if first_ch == '_' { b'^' } else { b'_' };
        if input_ws.as_bytes().first() == Some(&second_byte) {
            let input = &input_ws[1..];
            let (input, second_arg) = self.parse_script_arg(input)?;
            if first_ch == '_' {
                Ok((input, (first_arg, second_arg)))
            } else {
                Ok((input, (second_arg, first_arg)))
            }
        } else if first_ch == '_' {
            Ok((input, (first_arg, NodeId::NONE)))
        } else {
            Ok((input, (NodeId::NONE, first_arg)))
        }
    }

    /// Parse a single script argument: either a single token or a braced group.
    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        self.parse_atom(input)
    }

    // -----------------------------------------------------------------------
    // Atoms — byte-dispatch for hot-path performance
    // -----------------------------------------------------------------------

    /// Parse a single atom using byte dispatch.
    ///
    /// Instead of trying each sub-parser sequentially (try_alt!), we
    /// inspect the first byte to dispatch directly to the correct parser.
    /// This avoids the overhead of failed nom combinator attempts.
    #[inline]
    fn parse_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        match input.as_bytes().first() {
            Some(b'{') => self.parse_braced_group(input),
            Some(b'\\') => self.parse_command(input),
            Some(b'0'..=b'9') => self.parse_number(input),
            Some(&b) if SYMBOL_BYTES.contains(&b) => self.parse_symbol_char(input),
            Some(&b) if b.is_ascii_alphabetic() => self.parse_letter(input),
            // ~ is a non-breaking space in LaTeX
            Some(b'~') => Ok((
                &input[1..],
                self.arena
                    .push_node(CompactNode::Space(Rational::new(1, 4))),
            )),
            // $ in math mode is a no-op (used inside \hbox{$...$} etc.)
            Some(b'$') => Ok((
                &input[1..],
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            )),
            // Backtick ` → ordinary symbol (used in ``quotes'')
            Some(b'`') => {
                let sid = self.arena.push_str("`");
                Ok((
                    &input[1..],
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
                ))
            },
            // Non-ASCII: Unicode letters → identifiers, other → symbols
            Some(&b) if b > 127 => {
                let ch = input.chars().next().unwrap();
                let len = ch.len_utf8();
                let sid = self.arena.push_str(&input[..len]);
                if ch.is_alphabetic() {
                    Ok((
                        &input[len..],
                        self.arena.push_node(CompactNode::Identifier(sid)),
                    ))
                } else {
                    Ok((
                        &input[len..],
                        self.arena
                            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
                    ))
                }
            },
            _ => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alt,
            ))),
        }
    }

    /// Parse a braced group `{ ... }` into a Row (or single node).
    /// Also handles TeX infix fraction operators: `{a \over b}`, `{a \atop b}`,
    /// `{n \choose k}`, `{a \brace b}`, `{a \brack b}`.
    #[inline]
    fn parse_braced_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = char('{')(input)?;
        let (input, ids) = self.parse_node_list(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;

        // Check for infix fraction markers within the parsed nodes
        if let Some(result) = self.check_infix_fraction(&ids) {
            return Ok((input, result));
        }

        Ok((input, self.flatten_single(&ids)))
    }

    /// Parse a bracket group `[ ... ]` for optional arguments.
    ///
    /// Scans forward to the matching `]` (with brace-depth tracking) before
    /// parsing content, so that `]` atoms inside `SYMBOL_BYTES` don't get
    /// consumed as content by `parse_node_list`.
    fn parse_bracket_group<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        if !input.starts_with('[') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let inner = &input[1..]; // skip '['
        let mut depth = 0u32;
        let mut end = None;
        for (i, c) in inner.char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => depth -= 1,
                ']' if depth == 0 => {
                    end = Some(i);
                    break;
                },
                _ => {},
            }
        }
        let end = end.ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let content = &inner[..end];
        let rest = &inner[end + 1..];
        let (leftover, ids) = self.parse_node_list(content)?;
        // leftover should be empty; if not, ignore trailing content
        let _ = leftover;
        Ok((rest, ids))
    }

    /// Parse a number: sequence of digits with optional decimal point.
    ///
    /// Hand-rolled byte scan replaces nom `recognize(tuple(...))` to avoid
    /// combinator overhead on the hot path.
    #[inline]
    fn parse_number<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let bytes = input.as_bytes();
        let mut i = 0;
        // Leading digits (we know bytes[0] is a digit from byte dispatch)
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // Optional decimal point + trailing digits
        if i < bytes.len() && bytes[i] == b'.' {
            let dot = i;
            i += 1;
            if i < bytes.len() && bytes[i].is_ascii_digit() {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            } else {
                i = dot; // no digits after dot, rewind
            }
        }
        let num = &input[..i];
        let sid = self.arena.push_str(num);
        Ok((&input[i..], self.arena.push_node(CompactNode::Number(sid))))
    }

    /// Parse a single letter as an identifier.
    ///
    /// Called only when byte dispatch confirmed the first byte is ASCII alphabetic,
    /// so we consume it directly.
    #[inline]
    fn parse_letter<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let c = input.as_bytes()[0] as char;
        let rest = &input[1..]; // ASCII alphabetic chars are single-byte
        let sid = self.push_char_str(c);
        Ok((rest, self.arena.push_node(CompactNode::Identifier(sid))))
    }

    /// Parse a symbol character: +, -, =, <, >, !, comma, semicolon, etc.
    ///
    /// Called only when byte dispatch already confirmed the first byte is in
    /// `SYMBOL_BYTES`, so we consume the char directly instead of re-checking.
    #[inline]
    fn parse_symbol_char<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let c = input.as_bytes()[0] as char;
        let rest = &input[1..]; // all symbol chars are single-byte ASCII
        let kind = classify_char(c);
        // Remap ASCII hyphen-minus to proper math minus sign
        let ch = if c == '-' { '\u{2212}' } else { c };
        let sid = self.push_char_str(ch);
        Ok((rest, self.arena.push_node(CompactNode::Symbol(kind, sid))))
    }

    // -----------------------------------------------------------------------
    // Commands (backslash-prefixed)
    // -----------------------------------------------------------------------

    fn parse_command<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Skip the backslash (caller guarantees input starts with '\')
        let after_backslash = &input[1..];
        // Special single-character commands
        if let Ok((rest, id)) = self.parse_special_command(after_backslash) {
            return Ok((rest, id));
        }
        // Extract alphabetic command name via byte scan (avoids nom overhead)
        let bytes = after_backslash.as_bytes();
        let name_len = bytes
            .iter()
            .position(|&b| !b.is_ascii_alphabetic())
            .unwrap_or(bytes.len());
        if name_len == 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alpha,
            )));
        }
        let name = &after_backslash[..name_len];
        let rest = &after_backslash[name_len..];
        self.dispatch_command(rest, name)
    }

    fn parse_special_command<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let c = input.chars().next().ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let rest = &input[c.len_utf8()..];
        let node = match c {
            '{' => {
                let sid = self.arena.push_str("{");
                CompactNode::Symbol(SymbolKind::Open, sid)
            },
            '}' => {
                let sid = self.arena.push_str("}");
                CompactNode::Symbol(SymbolKind::Close, sid)
            },
            '|' => {
                let sid = self.arena.push_str("\u{2016}");
                CompactNode::Symbol(SymbolKind::Fence, sid)
            },
            ',' => CompactNode::Space(Rational::new(3, 18)),
            ';' => CompactNode::Space(Rational::new(5, 18)),
            ':' => CompactNode::Space(Rational::new(4, 18)),
            '!' => CompactNode::Space(Rational::new(-3, 18)),
            '\\' => {
                // Consume optional [dimension] after \\ (e.g. \\[1ex])
                let rest = if rest.starts_with('[') {
                    if let Some(end) = rest.find(']') {
                        &rest[end + 1..]
                    } else {
                        rest
                    }
                } else {
                    rest
                };
                return Ok((rest, self.arena.push_node(CompactNode::Newline)));
            },
            ' ' => CompactNode::Space(Rational::new(1, 4)),
            '$' => {
                let sid = self.arena.push_str("$");
                CompactNode::Symbol(SymbolKind::Ordinary, sid)
            },
            '#' => {
                let sid = self.arena.push_str("#");
                CompactNode::Symbol(SymbolKind::Ordinary, sid)
            },
            '%' => {
                let sid = self.arena.push_str("%");
                CompactNode::Symbol(SymbolKind::Ordinary, sid)
            },
            '&' => {
                let sid = self.arena.push_str("&");
                CompactNode::Symbol(SymbolKind::Ordinary, sid)
            },
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            },
        };
        Ok((rest, self.arena.push_node(node)))
    }

    fn dispatch_command<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, NodeId> {
        // Leaf commands that produce a node without consuming additional input.
        if let Some(id) = self.try_leaf_command(name) {
            return Ok((input, id));
        }
        match name {
            // --- Fractions ---
            "frac" => self.parse_frac(input, FractionKind::Normal),
            "dfrac" | "cfrac" => self.parse_frac(input, FractionKind::Display),
            "tfrac" => self.parse_frac(input, FractionKind::Inline),
            "binom" => self.parse_binom(input),

            // --- Roots ---
            "sqrt" => self.parse_sqrt(input),

            // --- Accents (above) ---
            "hat" => self.parse_accent(input, '\u{0302}', AccentPosition::Above),
            "widehat" => self.parse_accent_wide(input, '\u{0302}', AccentPosition::Above, true),
            "bar" => self.parse_accent(input, '\u{0304}', AccentPosition::Above),
            "overline" => self.parse_overunder(input, true),
            "underline" | "underbar" => self.parse_overunder(input, false),
            "vec" => self.parse_accent(input, '\u{20D7}', AccentPosition::Above),
            "dot" => self.parse_accent(input, '\u{0307}', AccentPosition::Above),
            "ddot" => self.parse_accent(input, '\u{0308}', AccentPosition::Above),
            "dddot" => self.parse_accent(input, '\u{20DB}', AccentPosition::Above),
            "ddddot" => self.parse_accent(input, '\u{20DC}', AccentPosition::Above),
            "tilde" => self.parse_accent(input, '\u{0303}', AccentPosition::Above),
            "widetilde" => self.parse_accent_wide(input, '\u{0303}', AccentPosition::Above, true),
            "breve" => self.parse_accent(input, '\u{0306}', AccentPosition::Above),
            "check" => self.parse_accent(input, '\u{030C}', AccentPosition::Above),
            "widecheck" => self.parse_accent_wide(input, '\u{030C}', AccentPosition::Above, true),
            "acute" => self.parse_accent(input, '\u{0301}', AccentPosition::Above),
            "grave" => self.parse_accent(input, '\u{0300}', AccentPosition::Above),
            "mathring" => self.parse_accent(input, '\u{030A}', AccentPosition::Above),

            // --- Stretchy accents (arrows above) ---
            "overrightarrow" => self.parse_accent(input, '\u{2192}', AccentPosition::Above),
            "overleftarrow" => self.parse_accent(input, '\u{2190}', AccentPosition::Above),
            "overleftrightarrow" => self.parse_accent(input, '\u{2194}', AccentPosition::Above),
            "overrightharpoon" | "overset{\\rightharpoonup}" => {
                self.parse_accent(input, '\u{21C0}', AccentPosition::Above)
            },
            "overleftharpoon" => self.parse_accent(input, '\u{21BC}', AccentPosition::Above),
            "Overrightarrow" => self.parse_accent(input, '\u{21D2}', AccentPosition::Above),

            // --- Under accents (arrows below) ---
            "underrightarrow" => self.parse_accent(input, '\u{2192}', AccentPosition::Below),
            "underleftarrow" => self.parse_accent(input, '\u{2190}', AccentPosition::Below),
            "underleftrightarrow" => self.parse_accent(input, '\u{2194}', AccentPosition::Below),
            "utilde" => self.parse_accent(input, '\u{0330}', AccentPosition::Below),

            // --- Over/under group arcs and line segments ---
            "overgroup" => self.parse_accent(input, '\u{23DC}', AccentPosition::Above),
            "undergroup" => self.parse_accent(input, '\u{23DD}', AccentPosition::Below),
            "overlinesegment" => self.parse_accent(input, '\u{23B4}', AccentPosition::Above),
            "underlinesegment" => self.parse_accent(input, '\u{23B5}', AccentPosition::Below),

            // --- Overbrace / underbrace / overbracket / underbracket ---
            "overbrace" => self.parse_overbrace(input),
            "underbrace" => self.parse_underbrace(input),
            "overbracket" => self.parse_overbracket(input),
            "underbracket" => self.parse_underbracket(input),

            // --- Delimiters ---
            "left" => self.parse_left_right(input),

            // --- Text and font ---
            "text" | "textrm" | "textit" | "textbf" | "texttt" | "textsf" | "textnormal"
            | "textup" | "mbox" | "hbox" => self.parse_text(input, name),
            "mathrm" | "mathbf" | "mathit" | "mathtt" | "mathsf" | "mathbb" | "mathcal"
            | "mathscr" | "mathfrak" | "boldsymbol" | "bm" | "mathnormal" | "mathup"
            | "mathsfup" | "mathsfit" | "mathbfup" | "mathbfit" | "mathbfcal" | "mathbffrak"
            | "mathbfscr" | "mathbfsfit" | "mathbfsfup" => self.parse_math_font(input, name),

            // --- Decorators ---
            "phantom" | "vphantom" | "hphantom" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                let kind = match name {
                    "vphantom" => PhantomKind::Vertical,
                    "hphantom" => PhantomKind::Horizontal,
                    _ => PhantomKind::Full,
                };
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Phantom(arg_id, kind)),
                ))
            },
            "mathstrut" => {
                // \mathstrut — zero-width strut; emit as phantom of "("
                let sid = self.arena.push_str("(");
                let inner = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Open, sid));
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Phantom(inner, PhantomKind::Vertical)),
                ))
            },
            // \ensuremath{content} — transparent wrapper; content already in math mode
            "ensuremath" => self.parse_required_group(input),

            // --- mhchem: \ce{...} chemical equations, \pu{...} physical units ---
            "ce" | "pu" => {
                let Some((after, body)) = super::mhchem::capture_braced(input) else {
                    return Ok((
                        input,
                        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    ));
                };
                let latex = if name == "ce" {
                    super::mhchem::ce_to_latex(body)
                } else {
                    super::mhchem::pu_to_latex(body)
                };
                let processed = match preprocess_cow(&latex) {
                    Ok(p) => p,
                    Err(_) => {
                        return Ok((
                            after,
                            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                        ));
                    },
                };
                let id = match self.parse_node_list(&processed) {
                    Ok((_, ids)) => self.flatten_single(&ids),
                    Err(_) => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                };
                Ok((after, id))
            },

            // --- siunitx (+ units `\unit`): numbers, units, quantities ---
            "num" | "si" | "SI" | "qty" | "unit" | "ang" | "numrange" | "SIrange" | "qtyrange"
            | "numlist" | "SIlist" | "qtylist" | "sisetup" => {
                let (after, latex) = super::siunitx::translate(name, input);
                let processed = match preprocess_cow(&latex) {
                    Ok(p) => p,
                    Err(_) => {
                        return Ok((
                            after,
                            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                        ));
                    },
                };
                let id = match self.parse_node_list(&processed) {
                    Ok((_, ids)) => self.flatten_single(&ids),
                    Err(_) => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                };
                Ok((after, id))
            },
            "smash" => {
                // \smash[b]{...} or \smash[t]{...} or \smash{...}
                let (input, _) = ws(input)?;
                let (input, smash_kind) = if let Some(rest) = input.strip_prefix('[') {
                    if let Some(end) = rest.find(']') {
                        let arg = &rest[..end];
                        let kind = match arg {
                            "b" => PhantomKind::SmashBottom,
                            "t" => PhantomKind::SmashTop,
                            _ => PhantomKind::SmashBoth,
                        };
                        (&rest[end + 1..], kind)
                    } else {
                        (input, PhantomKind::SmashBoth)
                    }
                } else {
                    (input, PhantomKind::SmashBoth)
                };
                let (input, arg_id) = self.parse_required_group(input)?;
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Phantom(arg_id, smash_kind)),
                ))
            },
            "tag" => {
                // \tag{...} — scan-consume argument (may contain $...$)
                let (input, _) = ws(input)?;
                let input = input.strip_prefix('*').unwrap_or(input);
                let (input, _) = ws(input)?;
                let input = if let Some(inner) = input.strip_prefix('{') {
                    let mut depth = 1u32;
                    let mut end = 0;
                    for (i, c) in inner.char_indices() {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = i + 1;
                                    break;
                                }
                            },
                            _ => {},
                        }
                    }
                    &inner[end..]
                } else {
                    input
                };
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },
            "boxed" | "fbox" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                let empty_sid = self.arena.push_str("");
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Boxed {
                        content: arg_id,
                        bg_color: empty_sid,
                        border_color: empty_sid,
                    }),
                ))
            },
            "cancel" => self.parse_cancel(input, StrokeKind::Forward),
            "bcancel" => self.parse_cancel(input, StrokeKind::Backward),
            "xcancel" => self.parse_cancel(input, StrokeKind::Cross),

            // --- Environments ---
            "begin" => self.parse_environment(input),

            // --- xypic: \xymatrix{...} diagram (arrow-glyph approximation) ---
            "xymatrix" => self.parse_xymatrix_cmd(input),

            // --- bussproofs: bare proof ended by \DisplayProof (best-effort) ---
            "AxiomC" | "AXC" => self.parse_bussproofs_inline(input, name),

            // --- Overset / Underset ---
            "overset" | "stackrel" => self.parse_overset(input),
            "underset" => self.parse_underset(input),
            "sideset" => self.parse_sideset(input),

            // --- Operatorname ---
            "operatorname" => self.parse_operatorname(input),

            // --- Additional binomials ---
            "dbinom" => self.parse_styled_binom(input, FractionKind::Display),
            "tbinom" => self.parse_styled_binom(input, FractionKind::Inline),

            // --- Math atom type wrappers ---
            "mathop" | "mathrel" | "mathbin" | "mathord" | "mathpunct" | "mathopen"
            | "mathclose" | "mathinner" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                let atom_type = match name {
                    "mathop" => MathAtomType::Op,
                    "mathrel" => MathAtomType::Rel,
                    "mathbin" => MathAtomType::Bin,
                    "mathpunct" => MathAtomType::Punct,
                    "mathopen" => MathAtomType::Open,
                    "mathclose" => MathAtomType::Close,
                    "mathinner" => MathAtomType::Inner,
                    _ => MathAtomType::Ord,
                };
                Ok((
                    input,
                    self.arena.push_node(CompactNode::MathAtom {
                        atom_type,
                        content: arg_id,
                    }),
                ))
            },

            // --- Negation ---
            "not" => self.parse_not(input),

            // --- Infix fraction operators (produce markers; resolved by parse_braced_group) ---
            "over" | "atop" | "choose" | "brace" | "brack" => {
                let sid = self.arena.push_str(match name {
                    "over" => "__infix_over__",
                    "atop" => "__infix_atop__",
                    "choose" => "__infix_choose__",
                    "brace" => "__infix_brace__",
                    "brack" => "__infix_brack__",
                    _ => unreachable!(),
                });
                Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
            },

            // --- Mod operators ---
            "pmod" | "pod" => self.parse_pmod(input),

            // --- Kern/space dimension commands ---
            "kern" => self.parse_kern(input, false),
            "mkern" => self.parse_kern(input, true),
            "hspace" => self.parse_hspace(input),
            "mspace" => self.parse_hspace(input),

            // --- Rule (consume arguments, emit empty) ---
            "rule" => self.parse_rule(input),

            // --- Extensible arrows ---
            "xleftarrow" => self.parse_extensible_arrow(input, '\u{2190}'),
            "xrightarrow" => self.parse_extensible_arrow(input, '\u{2192}'),
            "xLeftarrow" => self.parse_extensible_arrow(input, '\u{21D0}'),
            "xRightarrow" => self.parse_extensible_arrow(input, '\u{21D2}'),
            "xleftrightarrow" => self.parse_extensible_arrow(input, '\u{2194}'),
            "xLeftrightarrow" => self.parse_extensible_arrow(input, '\u{21D4}'),
            "xhookleftarrow" => self.parse_extensible_arrow(input, '\u{21A9}'),
            "xhookrightarrow" => self.parse_extensible_arrow(input, '\u{21AA}'),
            "xmapsto" => self.parse_extensible_arrow(input, '\u{21A6}'),
            "xrightleftharpoons" => self.parse_extensible_arrow(input, '\u{21CC}'),
            "xleftrightharpoons" => self.parse_extensible_arrow(input, '\u{21CB}'),
            "xtwoheadrightarrow" => self.parse_extensible_arrow(input, '\u{21A0}'),
            "xtwoheadleftarrow" => self.parse_extensible_arrow(input, '\u{219E}'),
            "xtofrom" => self.parse_extensible_arrow(input, '\u{21C4}'),
            "xleftharpoondown" => self.parse_extensible_arrow(input, '\u{21BD}'),
            "xleftharpoonup" => self.parse_extensible_arrow(input, '\u{21BC}'),
            "xrightharpoondown" => self.parse_extensible_arrow(input, '\u{21C1}'),
            "xrightharpoonup" => self.parse_extensible_arrow(input, '\u{21C0}'),
            "xlongequal" => self.parse_extensible_arrow(input, '='),

            // --- Substack ---
            "substack" => self.parse_substack(input),

            // --- Sized delimiters ---
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
                self.parse_big_delim(input, name)
            },

            // --- \middle delimiter (outside \left..\right context) ---
            "middle" => {
                let (input, _) = ws(input)?;
                let (input, sid) = self.parse_delimiter_into(input)?;
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Fence, sid)),
                ))
            },

            // --- Color (consume color arg, return content) ---
            "color" => self.parse_color_discard(input),
            "textcolor" => self.parse_textcolor(input),
            "definecolor" | "providecolor" => self.parse_definecolor(input),
            "colorlet" => self.parse_colorlet(input),

            // --- Colored boxes (consume color, return content) ---
            "colorbox" => self.parse_colorbox(input),
            "fcolorbox" => self.parse_fcolorbox(input),

            // --- Color shorthand commands ---
            "blue" | "red" | "green" | "orange" | "purple" | "cyan" | "magenta" | "yellow"
            | "black" | "white" | "gray" | "brown" | "lime" | "olive" | "pink" | "teal"
            | "violet" => self.parse_color_shorthand(input, name),

            // --- Generalized fraction ---
            "genfrac" => self.parse_genfrac(input),

            // --- Strikethrough / sout ---
            "sout" => self.parse_cancel(input, StrokeKind::Horizontal),

            // --- Poor man's bold (render as bold font) ---
            "pmb" => self.parse_math_font(input, "mathbf"),

            // --- Raisebox (consume dimension, return content) ---
            "raisebox" => self.parse_raisebox(input),

            // --- Centernot (negation overlay) ---
            "centernot" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                // Overlay a combining solidus on the argument
                let slash_sid = self.arena.push_str("\u{0338}");
                let slash_id = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, slash_sid));
                let children = self.arena.push_children(&[arg_id, slash_id]);
                Ok((input, self.arena.push_node(CompactNode::Row(children))))
            },

            // --- Hyperlinks and URLs (render visible text only) ---
            "href" => {
                // \href{url}{text} — consume url, return text
                let (input, _url_id) = self.parse_required_group(input)?;
                self.parse_required_group(input)
            },
            "url" => {
                // \url{text} — render as text
                let (input, _) = ws(input)?;
                let (input, _) = char('{')(input)?;
                let mut depth = 1u32;
                let mut end = 0;
                for (i, c) in input.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i;
                                break;
                            }
                        },
                        _ => {},
                    }
                }
                let text = &input[..end];
                let rest = &input[end + 1..];
                let sid = self.arena.push_str(text);
                Ok((
                    rest,
                    self.arena
                        .push_node(CompactNode::Text(TextStyle::Monospace, sid)),
                ))
            },

            // --- HTML attributes (transparent wrappers) ---
            "htmlId" | "htmlClass" | "htmlData" | "htmlStyle" => {
                // \htmlId{id}{content} — consume attribute arg, return content
                let (input, _attr_id) = self.parse_required_group(input)?;
                self.parse_required_group(input)
            },

            // --- DeclareMathOperator (consume and discard) ---
            "DeclareMathOperator"
            | "newcommand"
            | "renewcommand"
            | "providecommand"
            | "newenvironment"
            | "renewenvironment" => self.parse_declaration(input),

            // --- Includegraphics (consume and skip) ---
            "includegraphics" => {
                let (input, _) = ws(input)?;
                // Optional [options]
                let input = if input.starts_with('[') {
                    if let Some(end) = input.find(']') {
                        &input[end + 1..]
                    } else {
                        input
                    }
                } else {
                    input
                };
                let (input, _) = self.parse_required_group(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },

            // --- Logo text commands ---
            "TeX" => {
                let sid = self.arena.push_str("TeX");
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                ))
            },
            "LaTeX" => {
                let sid = self.arena.push_str("LaTeX");
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                ))
            },
            "KaTeX" => {
                let sid = self.arena.push_str("KaTeX");
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                ))
            },

            // --- Idotsint (integral with dots) ---
            "idotsint" => {
                let int_sid = self.push_char_str('\u{222B}');
                let dots_sid = self.push_char_str('\u{22EF}');
                let int1 = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::LargeOp, int_sid));
                let dots = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, dots_sid));
                let int_sid2 = self.push_char_str('\u{222B}');
                let int2 = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::LargeOp, int_sid2));
                let children = self.arena.push_children(&[int1, dots, int2]);
                Ok((input, self.arena.push_node(CompactNode::Row(children))))
            },

            // --- Verb (verbatim text) ---
            "verb" | "verb*" => self.parse_verb(input),

            // --- Overlap commands (emit content, ignore overlap positioning) ---
            "mathllap" | "mathrlap" | "mathclap" | "llap" | "rlap" | "clap" => {
                self.parse_required_group(input)
            },

            // --- Vcenter (vertical centering — just emit content) ---
            "vcenter" => self.parse_required_group(input),

            // --- Old-style font switches (TeX two-letter commands) ---
            "rm" | "it" | "bf" | "sf" | "tt" => self.parse_old_font_switch(input, name),

            // --- Above (infix with dimension) ---
            "above" => {
                // \above <dimen> — like \over but with specified bar thickness
                // Consume optional dimension, produce infix marker (treated like \over)
                let (input, _) = ws(input)?;
                // Try to consume a dimension
                let input = match self.parse_dimension(input) {
                    Ok((rest, _)) => rest,
                    Err(_) => input,
                };
                let sid = self.arena.push_str("__infix_over__");
                Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
            },

            // --- Table directives (when encountered outside table context) ---
            "hline" | "hdashline" => Ok((
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            )),

            // --- Label (consume and discard) ---
            "label" => {
                let (input, _) = self.parse_required_group(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },

            // --- Mathchoice: store all 4 branches for style-dependent selection ---
            "mathchoice" => {
                // \mathchoice{D}{T}{S}{SS} — store all four branches
                let (input, display_id) = self.parse_required_group(input)?;
                let (input, text_id) = self.parse_required_group(input)?;
                let (input, script_id) = self.parse_required_group(input)?;
                let (input, sscript_id) = self.parse_required_group(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::MathChoice {
                        display: display_id,
                        text: text_id,
                        script: script_id,
                        script_script: sscript_id,
                    }),
                ))
            },

            // --- Intop (integral with limits style) ---
            "intop" => {
                let sid = self.push_char_str('\u{222B}');
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::LargeOp, sid)),
                ))
            },

            // --- Reaction arrows (extensible) ---
            "xrightleftarrows" => self.parse_extensible_arrow(input, '\u{21C4}'),
            "xrightequilibrium" => self.parse_extensible_arrow(input, '\u{21CC}'),
            "xleftequilibrium" => self.parse_extensible_arrow(input, '\u{21CB}'),

            // --- Actuarial angle notation ---
            "angl" | "angln" => {
                // \angl{n} → n with angle bracket — render as content with right angle bar
                let (input, _) = ws(input)?;
                if input.starts_with('{') {
                    let (input, content_id) = self.parse_braced_group(input)?;
                    // Emit content with an overline to approximate the actuarial angle
                    let line_sid = self.arena.push_str("\u{203E}");
                    let line_id = self
                        .arena
                        .push_node(CompactNode::Symbol(SymbolKind::Ordinary, line_sid));
                    Ok((
                        input,
                        self.arena.push_node(CompactNode::Overscript {
                            base: content_id,
                            over: line_id,
                            movable_limits: false,
                        }),
                    ))
                } else {
                    // No argument — just emit empty
                    Ok((
                        input,
                        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    ))
                }
            },

            // --- Phase angle ---
            "phase" => {
                // \phase{angle} — render as underline with angle content
                let (input, content_id) = self.parse_required_group(input)?;
                let angle_sid = self.arena.push_str("\u{2220}");
                let angle_id = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, angle_sid));
                let children = self.arena.push_children(&[angle_id, content_id]);
                Ok((input, self.arena.push_node(CompactNode::Row(children))))
            },

            // --- Braket/Set (physics notation) ---
            "Braket" | "braket" => {
                // \Braket{...} → ⟨...⟩
                let (input, content_id) = self.parse_required_group(input)?;
                let open = self.arena.push_str("\u{27E8}");
                let close = self.arena.push_str("\u{27E9}");
                let content = self.arena.push_children(&[content_id]);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Fenced {
                        open,
                        close,
                        content,
                    }),
                ))
            },
            "Set" | "set" => {
                // \Set{...} → {...}
                let (input, content_id) = self.parse_required_group(input)?;
                let open = self.arena.push_str("{");
                let close = self.arena.push_str("}");
                let content = self.arena.push_children(&[content_id]);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Fenced {
                        open,
                        close,
                        content,
                    }),
                ))
            },

            // --- \cr (row separator used outside table context) ---
            "cr" => Ok((
                input,
                self.arena.push_node(CompactNode::Space(Rational::zero())),
            )),

            // --- Catch-all: symbols, n-ary, operators, Greek letters ---
            //
            // Single PHF lookup via `lookup_command` covers n-ary operators
            // (which appear as LargeOp kind) and all other symbols/Greek letters.
            // Named operators (sin, cos, etc.) are not in that map, so we fall
            // through to `is_operator_command` only when the first lookup misses.
            _ => {
                if let Some((unicode, kind)) = lookup_command(name) {
                    if kind == SymbolKind::LargeOp {
                        // N-ary operator (sum, prod, int, etc.)
                        let sid = self.push_char_str(unicode);
                        Ok((
                            input,
                            self.arena
                                .push_node(CompactNode::Symbol(SymbolKind::LargeOp, sid)),
                        ))
                    } else if kind == SymbolKind::Alphabetic {
                        let sid = self.push_char_str(unicode);
                        Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
                    } else {
                        let sid = self.push_char_str(unicode);
                        Ok((input, self.arena.push_node(CompactNode::Symbol(kind, sid))))
                    }
                } else if is_operator_command(name) {
                    // Named operators (sin, cos, lim, ...)
                    let limit_style = if operator_has_limits(name) {
                        LimitStyle::Default
                    } else {
                        LimitStyle::SubSup
                    };
                    let sid = self.arena.push_str(name);
                    Ok((
                        input,
                        self.arena.push_node(CompactNode::Operator {
                            name: sid,
                            limits: limit_style,
                        }),
                    ))
                } else if let Some((unicode, kind)) =
                    crate::latex::packages::lookup_package_symbol(name)
                {
                    // Package-contributed symbol (consulted only after the core
                    // PHF tables miss, so it cannot shadow a core symbol).
                    let sid = self.arena.push_str(unicode);
                    if kind == SymbolKind::Alphabetic {
                        Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
                    } else {
                        Ok((input, self.arena.push_node(CompactNode::Symbol(kind, sid))))
                    }
                } else {
                    // Unknown command: emit as identifier
                    let sid = self.arena.push_str(name);
                    Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
                }
            },
        }
    }

    /// Handle commands that produce a single node without consuming further input.
    fn try_leaf_command(&mut self, name: &str) -> Option<NodeId> {
        match name {
            // Spacing
            "quad" => Some(
                self.arena
                    .push_node(CompactNode::Space(Rational::new(1, 1))),
            ),
            "qquad" => Some(
                self.arena
                    .push_node(CompactNode::Space(Rational::new(2, 1))),
            ),
            "enspace" => Some(
                self.arena
                    .push_node(CompactNode::Space(Rational::new(1, 2))),
            ),
            // Style commands — emit StyleSwitch nodes that the layout engine uses
            // to change the math style for subsequent siblings in the same row.
            "displaystyle" => {
                self.display_override = true;
                Some(self.arena.push_node(CompactNode::StyleSwitch(0)))
            },
            "textstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(1))),
            "scriptstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(2))),
            "scriptscriptstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(3))),
            // Explicit line break.
            "newline" => Some(self.arena.push_node(CompactNode::Newline)),
            // Line-break hints and equation tags are layout no-ops in this renderer.
            "nonumber" | "notag" | "nobreak" | "allowbreak" | "goodbreak" | "limits"
            | "nolimits" => Some(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))),
            // Font sizing commands — emit SizeScale nodes for the layout engine
            "Huge" => Some(self.arena.push_node(CompactNode::SizeScale(9))),
            "huge" => Some(self.arena.push_node(CompactNode::SizeScale(8))),
            "LARGE" => Some(self.arena.push_node(CompactNode::SizeScale(7))),
            "Large" => Some(self.arena.push_node(CompactNode::SizeScale(6))),
            "large" => Some(self.arena.push_node(CompactNode::SizeScale(5))),
            "normalsize" => Some(self.arena.push_node(CompactNode::SizeScale(4))),
            "small" => Some(self.arena.push_node(CompactNode::SizeScale(3))),
            "footnotesize" => Some(self.arena.push_node(CompactNode::SizeScale(2))),
            "scriptsize" => Some(self.arena.push_node(CompactNode::SizeScale(1))),
            "tiny" => Some(self.arena.push_node(CompactNode::SizeScale(0))),
            // Mod operator
            "bmod" => {
                let sid = self.arena.push_str("mod");
                Some(self.arena.push_node(CompactNode::Operator {
                    name: sid,
                    limits: LimitStyle::Default,
                }))
            },
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Command sub-parsers
    // -----------------------------------------------------------------------

    fn parse_required_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        if input.starts_with('{') {
            self.parse_braced_group(input)
        } else {
            // LaTeX allows single unbraced tokens as arguments:
            // \frac12 → \frac{1}{2}, \vec x → \vec{x}
            self.parse_single_token(input)
        }
    }

    /// Parse a single unbraced TeX token: one letter, one digit, one symbol, or a command.
    fn parse_single_token<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        match input.as_bytes().first() {
            Some(b'\\') => self.parse_command(input),
            Some(&b) if b.is_ascii_alphabetic() => {
                let sid = self.arena.push_str(&input[..1]);
                Ok((
                    &input[1..],
                    self.arena.push_node(CompactNode::Identifier(sid)),
                ))
            },
            Some(&b) if b.is_ascii_digit() => {
                let sid = self.arena.push_str(&input[..1]);
                Ok((&input[1..], self.arena.push_node(CompactNode::Number(sid))))
            },
            Some(&b) if SYMBOL_BYTES.contains(&b) => self.parse_symbol_char(input),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alt,
            ))),
        }
    }

    fn parse_cancel<'a>(&mut self, input: &'a str, stroke: StrokeKind) -> IResult<&'a str, NodeId> {
        let (input, arg_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Cancel {
                stroke,
                content: arg_id,
            }),
        ))
    }

    fn parse_frac<'a>(&mut self, input: &'a str, kind: FractionKind) -> IResult<&'a str, NodeId> {
        let (input, num_id) = self.parse_required_group(input)?;
        let (input, den_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Fraction {
                kind,
                num: num_id,
                den: den_id,
            }),
        ))
    }

    /// `\genfrac{left}{right}{thickness}{style}{num}{den}`
    fn parse_genfrac<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Parse all 6 arguments (each can be braced or single token)
        let (input, left_id) = self.parse_required_group(input)?;
        let (input, right_id) = self.parse_required_group(input)?;
        let (input, _thickness) = self.parse_required_group(input)?; // discard
        let (input, _style) = self.parse_required_group(input)?; // discard
        let (input, num_id) = self.parse_required_group(input)?;
        let (input, den_id) = self.parse_required_group(input)?;

        let frac_id = self.arena.push_node(CompactNode::Fraction {
            kind: FractionKind::Normal,
            num: num_id,
            den: den_id,
        });

        // Extract delimiter strings from the parsed nodes
        let left_s = self.extract_delimiter_str(left_id);
        let right_s = self.extract_delimiter_str(right_id);

        if left_s.is_empty() && right_s.is_empty() {
            Ok((input, frac_id))
        } else {
            let open = self.arena.push_str(&left_s);
            let close = self.arena.push_str(&right_s);
            let content = self.arena.push_children(&[frac_id]);
            Ok((
                input,
                self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }),
            ))
        }
    }

    /// Extract a delimiter string from a parsed node (for \genfrac).
    fn extract_delimiter_str(&self, id: NodeId) -> String {
        match self.arena.node(id) {
            CompactNode::Symbol(_, sid) => self.arena.str_val(*sid).to_string(),
            CompactNode::Row(span) if span.is_empty() => String::new(),
            _ => String::new(),
        }
    }

    fn parse_binom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, top_id) = self.parse_required_group(input)?;
        let (input, bottom_id) = self.parse_required_group(input)?;
        let frac_id = self.arena.push_node(CompactNode::Fraction {
            kind: FractionKind::NoBar,
            num: top_id,
            den: bottom_id,
        });
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[frac_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    fn parse_sqrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional index: \sqrt[n]{...}
        let (input, index) = if input.starts_with('[') {
            let (rest, idx_ids) = self.parse_bracket_group(input)?;
            (rest, Some(idx_ids))
        } else {
            (input, None)
        };
        let (input, content_id) = self.parse_required_group(input)?;
        match index {
            Some(idx_ids) => {
                let index_id = self.flatten_single(&idx_ids);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Root {
                        index: index_id,
                        radicand: content_id,
                    }),
                ))
            },
            None => Ok((input, self.arena.push_node(CompactNode::Sqrt(content_id)))),
        }
    }

    fn parse_accent<'a>(
        &mut self,
        input: &'a str,
        accent_char: char,
        position: AccentPosition,
    ) -> IResult<&'a str, NodeId> {
        self.parse_accent_wide(input, accent_char, position, false)
    }

    fn parse_accent_wide<'a>(
        &mut self,
        input: &'a str,
        accent_char: char,
        position: AccentPosition,
        wide: bool,
    ) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Accent {
                base: base_id,
                accent: accent_char,
                position,
                wide,
            }),
        ))
    }

    fn parse_overunder<'a>(&mut self, input: &'a str, is_over: bool) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let line_sid = self.arena.push_str("\u{203E}");
        let line_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, line_sid));
        if is_over {
            Ok((
                input,
                self.arena.push_node(CompactNode::Overscript {
                    base: base_id,
                    over: line_id,
                    movable_limits: false,
                }),
            ))
        } else {
            Ok((
                input,
                self.arena.push_node(CompactNode::Underscript {
                    base: base_id,
                    under: line_id,
                    movable_limits: false,
                }),
            ))
        }
    }

    fn parse_overbrace<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23DE}");
        let over_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_underbrace<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23DF}");
        let under_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_overbracket<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23B4}");
        let over_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_underbracket<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23B5}");
        let under_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    /// Parse \left delim ... \right delim — zero-allocation delimiter interning.
    fn parse_left_right<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let result = self.parse_left_right_inner(input);
        self.depth -= 1;
        result
    }

    fn parse_left_right_inner<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, open) = self.parse_delimiter_into(input)?;
        let (input, content_ids) = self.parse_until_right(input)?;
        let (input, close) = self.parse_delimiter_into(input)?;

        let content = self.arena.push_children(&content_ids);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    fn parse_until_right<'a>(&mut self, input: &'a str) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            // Check for \right
            if let Ok((after_right, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\right")(rest)
            {
                return Ok((after_right, ids));
            }
            // Check for \middle
            if let Ok((after_mid, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\middle")(rest)
            {
                let (after_mid, _) = ws(after_mid)?;
                let (after_delim, sid) = self.parse_delimiter_into(after_mid)?;
                ids.push(self.arena.push_node(CompactNode::Middle(sid)));
                rest = after_delim;
                continue;
            }
            if rest.is_empty() {
                return Err(nom::Err::Error(nom::error::Error::new(
                    rest,
                    nom::error::ErrorKind::Tag,
                )));
            }
            let (r, id) = self.parse_element(rest)?;
            ids.push(id);
            rest = r;
        }
    }

    fn parse_text<'a>(&mut self, input: &'a str, cmd_name: &str) -> IResult<&'a str, NodeId> {
        let style = match cmd_name {
            "textbf" => TextStyle::Bold,
            "textit" => TextStyle::Italic,
            "texttt" => TextStyle::Monospace,
            "textsf" => TextStyle::SansSerif,
            _ => TextStyle::Normal,
        };
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        // Scan for matching '}' with brace counting (handles nested {})
        let mut depth = 1u32;
        let mut end = 0;
        for (i, c) in input.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                },
                _ => {},
            }
        }
        if depth != 0 {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let raw = &input[..end];
        let rest = &input[end + 1..]; // skip the closing '}'
        let text = clean_text_content(raw);
        let sid = self.arena.push_str(&text);
        Ok((rest, self.arena.push_node(CompactNode::Text(style, sid))))
    }

    fn parse_math_font<'a>(&mut self, input: &'a str, cmd_name: &str) -> IResult<&'a str, NodeId> {
        let style = latex_font_to_style(cmd_name).unwrap_or(TextStyle::Normal);
        let (input, content_id) = self.parse_required_group(input)?;
        // If content is a Row, use its children span; otherwise wrap in single-element span
        let span = match *self.arena.node(content_id) {
            CompactNode::Row(s) => s,
            _ => self.arena.push_children(&[content_id]),
        };
        Ok((
            input,
            self.arena.push_node(CompactNode::Styled(style, span)),
        ))
    }

    /// `\sideset{<left scripts>}{<right scripts>}<op>` — corner scripts on a
    /// large operator. The left/right groups use bare script syntax (`_a^b`).
    /// Renders the operator flanked by the left scripts (as prescripts) and the
    /// right scripts (as postscripts); the operator keeps its own \limits.
    fn parse_sideset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Two brace groups whose contents are `_x^y`-style scripts.
        let parse_side = |this: &mut Self, inp: &'a str| -> IResult<&'a str, (NodeId, NodeId)> {
            let (inp, _) = ws(inp)?;
            let (inp, _) = char('{')(inp)?;
            let (inp, _) = ws(inp)?;
            let (inp, (sub, sup)) = this.parse_optional_limits(inp)?;
            let (inp, _) = ws(inp)?;
            let (inp, _) = char('}')(inp)?;
            Ok((inp, (sub, sup)))
        };
        let (input, (l_sub, l_sup)) = parse_side(self, input)?;
        let (input, (r_sub, r_sup)) = parse_side(self, input)?;
        let (input, _) = ws(input)?;

        // The operator (parsed as a full element so it keeps its \limits/body).
        let (input, op_id) = match self.parse_element(input) {
            Ok(v) => v,
            Err(_) => (
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            ),
        };

        // Decompose the parsed operator so the four corner scripts hug the bare
        // operator glyph (like amsmath / MathJax) rather than scattering around
        // a pre-stacked limit box. For the common n-ary case (`\sum`, `\int`, …)
        // we lift the glyph out, attach the left group as prescripts and the
        // right group as corner postscripts, then re-apply the operator's own
        // limits above/below. The operator body (e.g. `a_n`) follows after.
        let (op_glyph, op_sub, op_sup, op_body, op_limits) = match *self.arena.node(op_id) {
            CompactNode::Nary {
                op,
                sub,
                sup,
                body,
                limits,
            } => {
                let mut buf = [0u8; 4];
                let sid = self.arena.push_str(op.encode_utf8(&mut buf));
                let glyph = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::LargeOp, sid));
                (glyph, sub, sup, body, limits)
            },
            // Non-n-ary operator: hug it directly, no above/below limits to lift.
            _ => (op_id, NodeId::NONE, NodeId::NONE, NodeId::NONE, LimitStyle::Default),
        };

        // Left group → prescripts on the glyph.
        let with_pre = if l_sub != NodeId::NONE || l_sup != NodeId::NONE {
            self.arena.push_node(CompactNode::PreScripts {
                base: op_glyph,
                pre_sub: l_sub,
                pre_sup: l_sup,
            })
        } else {
            op_glyph
        };

        // Right group → corner postscripts. Because `with_pre` is not itself an
        // operator node, these stay as side (corner) scripts.
        let with_post = match (r_sub != NodeId::NONE, r_sup != NodeId::NONE) {
            (true, true) => self.arena.push_node(CompactNode::SubSup {
                base: with_pre,
                sub: r_sub,
                sup: r_sup,
            }),
            (true, false) => self
                .arena
                .push_node(CompactNode::Subscript { base: with_pre, sub: r_sub }),
            (false, true) => self
                .arena
                .push_node(CompactNode::Superscript { base: with_pre, sup: r_sup }),
            (false, false) => with_pre,
        };

        // Re-apply the operator's own limits above/below the corner-scripted op.
        // `_ = op_limits;` keeps the field available for future inline tuning;
        // over/under placement matches the display-mode amsmath rendering.
        let _ = op_limits;
        let stacked = match (op_sub != NodeId::NONE, op_sup != NodeId::NONE) {
            (true, true) => self.arena.push_node(CompactNode::UnderOver {
                base: with_post,
                under: op_sub,
                over: op_sup,
                movable_limits: true,
            }),
            (true, false) => self.arena.push_node(CompactNode::Underscript {
                base: with_post,
                under: op_sub,
                movable_limits: true,
            }),
            (false, true) => self.arena.push_node(CompactNode::Overscript {
                base: with_post,
                over: op_sup,
                movable_limits: true,
            }),
            (false, false) => with_post,
        };

        let mut items: SmallVec<[NodeId; 4]> = SmallVec::new();
        items.push(stacked);
        if op_body != NodeId::NONE {
            items.push(op_body);
        }

        let span = self.arena.push_children(&items);
        Ok((input, self.arena.push_node(CompactNode::Row(span))))
    }

    fn parse_overset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, over_id) = self.parse_required_group(input)?;
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_underset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, under_id) = self.parse_required_group(input)?;
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_operatorname<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Handle \operatorname* (limits placement)
        let (input, limits) = if let Some(rest) = input.strip_prefix('*') {
            (rest, LimitStyle::AboveBelow)
        } else {
            (input, LimitStyle::Default)
        };
        let (input, _) = ws(input)?;
        // Accept both braced {name} and unbraced single letter
        let (input, name) = if input.starts_with('{') {
            let (input, _) = char('{')(input)?;
            let (input, name) = take_while(|c: char| c != '}')(input)?;
            let (input, _) = char('}')(input)?;
            (input, name)
        } else if input
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            (&input[1..], &input[..1])
        } else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alpha,
            )));
        };
        // Resolve LaTeX commands within the name (e.g., \Gamma → Γ)
        let sid = if name.contains('\\') {
            let mut resolved = String::with_capacity(name.len());
            let mut chars = name.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    let mut cmd = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_alphabetic() {
                            cmd.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(unicode) = latex_to_unicode(&cmd) {
                        resolved.push(unicode);
                    } else if !cmd.is_empty() {
                        resolved.push_str(&cmd);
                    }
                } else {
                    resolved.push(ch);
                }
            }
            self.arena.push_str(&resolved)
        } else {
            self.arena.push_str(name)
        };
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Operator { name: sid, limits }),
        ))
    }

    fn parse_styled_binom<'a>(
        &mut self,
        input: &'a str,
        kind: FractionKind,
    ) -> IResult<&'a str, NodeId> {
        let (input, top_id) = self.parse_required_group(input)?;
        let (input, bottom_id) = self.parse_required_group(input)?;
        let frac_id = self.arena.push_node(CompactNode::Fraction {
            kind,
            num: top_id,
            den: bottom_id,
        });
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[frac_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    fn parse_not<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Try direct character negation: \not= \not< \not>
        if let Some(first) = input.chars().next()
            && let Some(negated) = negate_char(first)
        {
            let rest = &input[first.len_utf8()..];
            let sid = self.push_char_str(negated);
            return Ok((
                rest,
                self.arena
                    .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
            ));
        }
        // Try command negation: \not\in, \not\subset, etc.
        if let Some(after_backslash) = input.strip_prefix('\\') {
            let cmd_end = after_backslash
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(after_backslash.len());
            if cmd_end > 0 {
                let cmd = &after_backslash[..cmd_end];
                if let Some(unicode) = latex_to_unicode(cmd)
                    && let Some(negated) = negate_char(unicode)
                {
                    let rest = &after_backslash[cmd_end..];
                    let sid = self.push_char_str(negated);
                    return Ok((
                        rest,
                        self.arena
                            .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
                    ));
                }
            }
        }
        // Fallback: combining solidus overlay
        let sid = self.arena.push_str("\u{0338}");
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
        ))
    }

    fn parse_pmod<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, content_id) = self.parse_required_group(input)?;
        let mod_name = self.arena.push_str("mod");
        let mod_id = self.arena.push_node(CompactNode::Operator {
            name: mod_name,
            limits: LimitStyle::Default,
        });
        let space_id = self
            .arena
            .push_node(CompactNode::Space(Rational::new(5, 18)));
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[mod_id, space_id, content_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    /// Parse a TeX dimension: an optional sign, a number (integer or decimal),
    /// followed by a unit. Returns a value in em.
    fn parse_dimension<'a>(&mut self, input: &'a str) -> IResult<&'a str, Rational> {
        let (input, _) = ws(input)?;
        let bytes = input.as_bytes();
        let mut i = 0;
        // Optional sign
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        // Digits (integer part)
        let digit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // Optional decimal point + fractional digits
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == digit_start
            && !(digit_start > 0
                && (bytes[digit_start - 1] == b'+' || bytes[digit_start - 1] == b'-'))
        {
            // No number found at all
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Digit,
            )));
        }
        let num_str = &input[..i];
        let rest = &input[i..];
        let value: f64 = num_str.parse().unwrap_or(0.0);

        // Skip optional whitespace before unit
        let skip = skip_ws_bytes(rest.as_bytes());
        let rest = &rest[skip..];

        // Parse unit
        let (rest, em_value) = if let Some(r) = rest.strip_prefix("em") {
            (r, value)
        } else if let Some(r) = rest.strip_prefix("ex") {
            (r, value * 0.43) // 1ex ~ 0.43em
        } else if let Some(r) = rest.strip_prefix("pt") {
            (r, value * 0.1) // 1pt ~ 0.1em at 10pt base
        } else if let Some(r) = rest.strip_prefix("mu") {
            (r, value / 18.0) // 1mu = 1/18 em
        } else if let Some(r) = rest.strip_prefix("mm") {
            (r, value * 0.284) // approximate
        } else if let Some(r) = rest.strip_prefix("cm") {
            (r, value * 2.84) // approximate
        } else if let Some(r) = rest.strip_prefix("in") {
            (r, value * 7.23) // approximate
        } else if let Some(r) = rest.strip_prefix("pc") {
            (r, value * 1.2) // 1pc = 12pt
        } else if let Some(r) = rest.strip_prefix("bp") {
            (r, value * 0.1) // 1bp ~ 1pt
        } else if let Some(r) = rest.strip_prefix("sp") {
            (r, value * 0.0000015) // 1sp = 1/65536 pt, negligible
        } else {
            // No unit — assume pt (common in TeX)
            (rest, value * 0.1)
        };

        // Ensure the unit isn't followed by an alphabetic char (e.g. "emph" isn't "em"+"ph")
        // But since we already stripped, this is just a safety check handled by the strip_prefix.

        // Convert f64 to rational: multiply by 100, round, use den=100
        let num_i = (em_value * 100.0).round() as i32;
        Ok((rest, Rational::new(num_i, 100)))
    }

    /// `\kern<dimension>` or `\mkern<dimension>` — explicit spacing.
    /// `is_mu` indicates math units (1mu = 1/18 em).
    fn parse_kern<'a>(&mut self, input: &'a str, is_mu: bool) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // If the dimension is braced, handle {dim}
        if input.starts_with('{') {
            let (rest, _) = char('{')(input)?;
            let end = rest.find('}').ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
            })?;
            let dim_str = &rest[..end];
            let after = &rest[end + 1..];
            // Parse the dimension within the braces
            let mut sub_parser = LatexCompactParser::with_capacity(dim_str.len());
            if let Ok((_, r)) = sub_parser.parse_dimension(dim_str) {
                return Ok((after, self.arena.push_node(CompactNode::Space(r))));
            }
            // Failed to parse dimension; emit thin space
            return Ok((
                after,
                self.arena.push_node(CompactNode::Space(Rational::zero())),
            ));
        }
        match self.parse_dimension(input) {
            Ok((rest, r)) => {
                let r = if is_mu {
                    // parse_dimension with "mu" already handled; for bare numbers assume mu
                    r
                } else {
                    r
                };
                Ok((rest, self.arena.push_node(CompactNode::Space(r))))
            },
            Err(_) => {
                // Couldn't parse dimension; just emit zero space
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Space(Rational::zero())),
                ))
            },
        }
    }

    /// `\hspace{dimension}` or `\hspace*{dimension}` — horizontal space.
    fn parse_hspace<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional * after command name
        let input = input.strip_prefix('*').unwrap_or(input);
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let dim_str = &input[..end];
        let rest = &input[end + 1..];
        // Parse dimension
        let mut sub_parser = LatexCompactParser::with_capacity(dim_str.len());
        if let Ok((_, r)) = sub_parser.parse_dimension(dim_str) {
            return Ok((rest, self.arena.push_node(CompactNode::Space(r))));
        }
        Ok((
            rest,
            self.arena.push_node(CompactNode::Space(Rational::zero())),
        ))
    }

    /// `\rule[lift]{width}{height}` — parse dimensions and emit a filled rectangle.
    fn parse_rule<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, lift) = if input.starts_with('[') {
            let end = input.find(']').ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
            })?;
            let lift_str = &input[1..end];
            let rest = &input[end + 1..];
            let mut lp = LatexCompactParser::with_capacity(lift_str.len());
            let lift = lp
                .parse_dimension(lift_str)
                .map(|(_, r)| r)
                .unwrap_or(Rational::zero());
            (rest, lift)
        } else {
            (input, Rational::zero())
        };
        // Required {width}
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let width_end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let width_str = &input[..width_end];
        let input = &input[width_end + 1..];
        let mut wp = LatexCompactParser::with_capacity(width_str.len());
        let width = wp
            .parse_dimension(width_str)
            .map(|(_, r)| r)
            .unwrap_or(Rational::zero());

        // Required {height}
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let height_end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let height_str = &input[..height_end];
        let input = &input[height_end + 1..];
        let mut hp = LatexCompactParser::with_capacity(height_str.len());
        let height = hp
            .parse_dimension(height_str)
            .map(|(_, r)| r)
            .unwrap_or(Rational::zero());

        Ok((
            input,
            self.arena.push_node(CompactNode::Rule {
                width,
                height,
                lift,
            }),
        ))
    }

    fn parse_extensible_arrow<'a>(
        &mut self,
        input: &'a str,
        arrow: char,
    ) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional below: \xrightarrow[below]{above}
        let (input, below) = if input.starts_with('[') {
            let (rest, ids) = self.parse_bracket_group(input)?;
            (rest, Some(ids))
        } else {
            (input, None)
        };
        let (input, above_id) = self.parse_required_group(input)?;

        let arrow_sid = self.push_char_str(arrow);
        let base_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Relation, arrow_sid));

        match below {
            Some(below_ids) => {
                let under_id = self.flatten_single(&below_ids);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::UnderOver {
                        base: base_id,
                        under: under_id,
                        over: above_id,
                        movable_limits: false,
                    }),
                ))
            },
            None => Ok((
                input,
                self.arena.push_node(CompactNode::Overscript {
                    base: base_id,
                    over: above_id,
                    movable_limits: false,
                }),
            )),
        }
    }

    fn parse_substack<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;

        let mut cells: Vec<SmallVec<[NodeId; 8]>> = Vec::new();
        let mut rest = input;
        let (r, first) = self.parse_table_cell(rest)?;
        cells.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, _)) => {
                    let (r, cell) = self.parse_table_cell(after_sep)?;
                    cells.push(cell);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }

        let (input, _) = ws(rest)?;
        let (input, _) = char('}')(input)?;

        let table_rows: Vec<Vec<SmallVec<[NodeId; 8]>>> =
            cells.into_iter().map(|cell| vec![cell]).collect();
        Ok((
            input,
            self.build_array_with_spacing(&[ColumnAlign::Center], table_rows, 50, false, 0, 0),
        ))
    }

    fn parse_big_delim<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, NodeId> {
        let factor = match name {
            "big" | "bigl" | "bigr" | "bigm" => Rational::new(6, 5),
            "Big" | "Bigl" | "Bigr" | "Bigm" => Rational::new(9, 5),
            "bigg" | "biggl" | "biggr" | "biggm" => Rational::new(12, 5),
            "Bigg" | "Biggl" | "Biggr" | "Biggm" => Rational::new(3, 1),
            _ => Rational::new(6, 5),
        };
        let (input, _) = ws(input)?;
        let (input, delim_id) = self.parse_big_delim_arg(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Scaled {
                factor,
                content: delim_id,
            }),
        ))
    }

    fn parse_big_delim_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if let Some(c) = input.chars().next() {
            let rest = &input[c.len_utf8()..];
            let (kind, s) = match c {
                '(' => (SymbolKind::Open, "("),
                ')' => (SymbolKind::Close, ")"),
                '[' => (SymbolKind::Open, "["),
                ']' => (SymbolKind::Close, "]"),
                '|' => (SymbolKind::Fence, "|"),
                '/' => (SymbolKind::Ordinary, "/"),
                _ => return self.parse_command(input),
            };
            let sid = self.arena.push_str(s);
            return Ok((rest, self.arena.push_node(CompactNode::Symbol(kind, sid))));
        }
        self.parse_command(input)
    }

    fn parse_color_discard<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, color_str) = self.parse_color_arg(input)?;
        let color_sid = self.arena.push_str(&color_str);
        // \color{X}{content} — if followed by braced group, parse just that group.
        // Otherwise, parse remaining elements but stop at cell/row/env boundaries
        // to avoid consuming past table structure (e.g., \\, &, \end).
        let (input, _) = ws(input)?;
        if input.starts_with('{') {
            let (input, content_id) = self.parse_braced_group(input)?;
            Ok((
                input,
                self.arena.push_node(CompactNode::Color {
                    color: color_sid,
                    content: content_id,
                }),
            ))
        } else {
            let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
            let mut rest = input;
            loop {
                let (r, _) = ws(rest)?;
                rest = r;
                if rest.is_empty()
                    || rest.starts_with('&')
                    || rest.starts_with("\\\\")
                    || rest.starts_with("\\end")
                    || rest.starts_with("\\right")
                    || rest.starts_with("\\middle")
                    || rest.starts_with('}')
                {
                    break;
                }
                match self.parse_element(rest) {
                    Ok((r, id)) => {
                        ids.push(id);
                        rest = r;
                    },
                    Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                    Err(_) => break,
                }
            }
            let content_id = self.flatten_single(&ids);
            Ok((
                rest,
                self.arena.push_node(CompactNode::Color {
                    color: color_sid,
                    content: content_id,
                }),
            ))
        }
    }

    fn parse_textcolor<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, color_str) = self.parse_color_arg(input)?;
        let color_sid = self.arena.push_str(&color_str);
        let (input, content_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Color {
                color: color_sid,
                content: content_id,
            }),
        ))
    }

    /// Parse a color argument for `\color`/`\textcolor`: either an optional
    /// model `[model]{spec}` or a plain `{name}` (which may be an xcolor
    /// mixing expression or user-defined color). Returns the resolved color
    /// string ready for the renderer (named CSS colors are kept verbatim;
    /// anything computed becomes `#rrggbb`).
    fn parse_color_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, String> {
        let (input, _) = ws(input)?;
        if input.starts_with('[') {
            let (input, _) = char('[')(input)?;
            let (input, model) = take_while(|c: char| c != ']')(input)?;
            let (input, _) = char(']')(input)?;
            let (input, _) = ws(input)?;
            let (input, _) = char('{')(input)?;
            let (input, spec) = take_while(|c: char| c != '}')(input)?;
            let (input, _) = char('}')(input)?;
            let resolved = crate::css_colors::resolve_color_model(model.trim(), spec.trim())
                .map(crate::css_colors::rgb_to_hex)
                .unwrap_or_else(|| spec.trim().to_string());
            Ok((input, resolved))
        } else {
            let (input, _) = char('{')(input)?;
            let (input, name) = take_while(|c: char| c != '}')(input)?;
            let (input, _) = char('}')(input)?;
            Ok((input, self.resolve_color(name)))
        }
    }

    /// Parse a colortbl color argument: an optional `[model]{spec}` or plain
    /// `{name}`, followed by up to two optional `[overhang]` arguments (used by
    /// `\rowcolor`/`\cellcolor`/`\columncolor`), which are parsed and ignored.
    /// Returns the resolved, renderer-ready color string.
    fn parse_colortbl_color<'a>(&mut self, input: &'a str) -> IResult<&'a str, String> {
        let (mut rest, color) = self.parse_color_arg(input)?;
        // Discard up to two trailing optional overhang args, e.g. `[2pt][3pt]`.
        for _ in 0..2 {
            let (r, _) = ws(rest)?;
            match r
                .strip_prefix('[')
                .and_then(|s| s.find(']').map(|e| &s[e + 1..]))
            {
                Some(after) => rest = after,
                None => break,
            }
        }
        Ok((rest, color))
    }

    /// `\definecolor{name}{model}{spec}` / `\providecolor{...}` — register a
    /// user-defined named color. Produces no visible output.
    fn parse_definecolor<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, name) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, model) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, spec) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        if let Some(rgb) = crate::css_colors::resolve_color_model(model.trim(), spec.trim()) {
            self.colors.insert(name.trim().to_string(), rgb);
        }
        Ok((
            input,
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        ))
    }

    /// `\colorlet{name}{colorexpr}` — define `name` as the (possibly mixed)
    /// color expression. Produces no visible output.
    fn parse_colorlet<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, name) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, expr) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        if let Some(rgb) = self.resolve_color_rgb(expr.trim()) {
            self.colors.insert(name.trim().to_string(), rgb);
        }
        Ok((
            input,
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        ))
    }

    /// Resolve a color token into a renderer-ready string. Plain named CSS
    /// colors are kept verbatim (for round-trip stability); user-defined
    /// colors and xcolor mixing expressions resolve to `#rrggbb`.
    fn resolve_color(&self, token: &str) -> String {
        let token = token.trim();
        if token.contains('!') {
            return self
                .resolve_color_rgb(token)
                .map(crate::css_colors::rgb_to_hex)
                .unwrap_or_else(|| token.to_string());
        }
        if let Some(rgb) = self.colors.get(token) {
            return crate::css_colors::rgb_to_hex(*rgb);
        }
        token.to_string()
    }

    /// Resolve a color token (xcolor expression, user color, named CSS color,
    /// hex, or `rgb(...)`) into RGB bytes.
    fn resolve_color_rgb(&self, token: &str) -> Option<[u8; 3]> {
        let token = token.trim();
        if token.contains('!') {
            return self.resolve_color_expr(token);
        }
        self.resolve_color_base(token)
    }

    /// Resolve a single (non-mixed) color reference to RGB bytes.
    fn resolve_color_base(&self, token: &str) -> Option<[u8; 3]> {
        let token = token.trim();
        if let Some(rgb) = self.colors.get(token) {
            return Some(*rgb);
        }
        crate::css_colors::resolve_named_or_hex(token)
    }

    /// Resolve an xcolor mixing expression: `a!p!b!q!c...` (left-associative).
    /// `a!p` is shorthand for `a!p!white`.
    fn resolve_color_expr(&self, expr: &str) -> Option<[u8; 3]> {
        let mut parts = expr.split('!');
        let mut cur = self.resolve_color_base(parts.next()?.trim())?;
        while let Some(pct_s) = parts.next() {
            let pct: f64 = pct_s.trim().parse().ok()?;
            let other = match parts.next() {
                Some(name) => self.resolve_color_base(name.trim())?,
                None => [255, 255, 255], // mix toward white
            };
            cur = crate::css_colors::mix_rgb(cur, other, pct);
        }
        Some(cur)
    }

    /// Color shorthand: `\blue{...}` or `\blue x` wraps following content in a Color node.
    fn parse_color_shorthand<'a>(
        &mut self,
        input: &'a str,
        color_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let color_sid = self.arena.push_str(color_name);
        let (input, _) = ws(input)?;
        if input.starts_with('{') {
            let (input, content_id) = self.parse_braced_group(input)?;
            Ok((
                input,
                self.arena.push_node(CompactNode::Color {
                    color: color_sid,
                    content: content_id,
                }),
            ))
        } else {
            // Consume the next single token
            match self.parse_single_token(input) {
                Ok((rest, content_id)) => Ok((
                    rest,
                    self.arena.push_node(CompactNode::Color {
                        color: color_sid,
                        content: content_id,
                    }),
                )),
                Err(_) => {
                    // No content follows; just emit an empty node
                    let empty = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    Ok((
                        input,
                        self.arena.push_node(CompactNode::Color {
                            color: color_sid,
                            content: empty,
                        }),
                    ))
                },
            }
        }
    }

    /// `\colorbox{color}{content}` — consume color arg, emit content in a colored box.
    fn parse_colorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        let bg_color = self.resolve_color(color);
        let bg_sid = self.arena.push_str(&bg_color);
        let border_sid = self.arena.push_str("");
        Ok((
            input,
            self.arena.push_node(CompactNode::Boxed {
                content: content_id,
                bg_color: bg_sid,
                border_color: border_sid,
            }),
        ))
    }

    /// `\fcolorbox{framecolor}{bgcolor}{content}` — consume two color args, emit content in a colored box.
    fn parse_fcolorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, frame_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, bg_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        let bg = self.resolve_color(bg_color);
        let frame = self.resolve_color(frame_color);
        let bg_sid = self.arena.push_str(&bg);
        let border_sid = self.arena.push_str(&frame);
        Ok((
            input,
            self.arena.push_node(CompactNode::Boxed {
                content: content_id,
                bg_color: bg_sid,
                border_color: border_sid,
            }),
        ))
    }

    /// `\raisebox{dimension}[height][depth]{content}` — shift content vertically.
    fn parse_raisebox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let dim_str = &input[..end];
        let mut dp = LatexCompactParser::with_capacity(dim_str.len());
        let shift = dp
            .parse_dimension(dim_str)
            .map(|(_, r)| r)
            .unwrap_or_else(|_| Rational::zero());
        let rest = &input[end + 1..];

        let (rest, _) = ws(rest)?;
        let (rest, height) = if rest.starts_with('[') {
            self.parse_optional_dimension_bracket(rest)?
        } else {
            (rest, None)
        };
        let (rest, _) = ws(rest)?;
        let (rest, depth) = if rest.starts_with('[') {
            self.parse_optional_dimension_bracket(rest)?
        } else {
            (rest, None)
        };
        let (rest, content) = self.parse_required_group(rest)?;
        Ok((
            rest,
            self.arena.push_node(CompactNode::RaiseBox {
                shift,
                height: height.unwrap_or_else(Rational::zero),
                depth: depth.unwrap_or_else(Rational::zero),
                extents: u8::from(height.is_some()) | (u8::from(depth.is_some()) << 1),
                content,
            }),
        ))
    }

    fn parse_optional_dimension_bracket<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Option<Rational>> {
        let (input, _) = char('[')(input)?;
        let end = input.find(']').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let dim_str = &input[..end];
        let rest = &input[end + 1..];
        let mut dp = LatexCompactParser::with_capacity(dim_str.len());
        let dim = dp.parse_dimension(dim_str).map(|(_, r)| r).ok();
        Ok((rest, dim))
    }

    /// `\verb|...|` or `\verb*|...|` — verbatim text between delimiters.
    fn parse_verb<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // The delimiter is the first non-space character after \verb or \verb*
        let input = input.strip_prefix('*').unwrap_or(input);
        let mut chars = input.chars();
        let delim = chars.next().ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let after_delim = &input[delim.len_utf8()..];
        // Find matching closing delimiter
        if let Some(end) = after_delim.find(delim) {
            let text = &after_delim[..end];
            let rest = &after_delim[end + delim.len_utf8()..];
            let sid = self.arena.push_str(text);
            Ok((
                rest,
                self.arena
                    .push_node(CompactNode::Text(TextStyle::Monospace, sid)),
            ))
        } else {
            // No closing delimiter found; consume to end
            let sid = self.arena.push_str(after_delim);
            Ok((
                "",
                self.arena
                    .push_node(CompactNode::Text(TextStyle::Monospace, sid)),
            ))
        }
    }

    /// Old-style font switches: `\rm`, `\it`, `\bf`, `\sf`, `\tt`.
    /// These apply to the rest of the current group.
    fn parse_old_font_switch<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        // In math mode, these are typically no-ops or apply a style to following content.
        // We'll parse the next token and apply the style.
        let style = match name {
            "rm" => TextStyle::Normal,
            "it" => TextStyle::Italic,
            "bf" => TextStyle::Bold,
            "sf" => TextStyle::SansSerif,
            "tt" => TextStyle::Monospace,
            _ => TextStyle::Normal,
        };
        let (input, _) = ws(input)?;
        // If there's content to style, consume the next token
        if input.is_empty()
            || input.starts_with('}')
            || input.starts_with('&')
            || input.starts_with("\\\\")
            || input.starts_with("\\end")
        {
            return Ok((
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            ));
        }
        // Parse remaining content in current group
        let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            if rest.is_empty()
                || rest.starts_with('}')
                || rest.starts_with('&')
                || rest.starts_with("\\\\")
                || rest.starts_with("\\end")
            {
                break;
            }
            match self.parse_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        if ids.is_empty() {
            return Ok((
                rest,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            ));
        }
        let content_span = self.arena.push_children(&ids);
        Ok((
            rest,
            self.arena
                .push_node(CompactNode::Styled(style, content_span)),
        ))
    }

    /// Parse \DeclareMathOperator and similar declaration commands — consume all arguments.
    fn parse_declaration<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional *
        let input = input.strip_prefix('*').unwrap_or(input);
        // Consume braced arguments until we stop finding them
        let mut rest = input;
        for _ in 0..4 {
            let (r, _) = ws(rest)?;
            if r.starts_with('{') {
                if let Ok((after, _)) = self.parse_braced_group(r) {
                    rest = after;
                } else {
                    break;
                }
            } else if r.starts_with('[') {
                if let Some(end) = r.find(']') {
                    rest = &r[end + 1..];
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok((
            rest,
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        ))
    }

    // -----------------------------------------------------------------------
    // Environments
    // -----------------------------------------------------------------------

    fn parse_environment<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, env_name) =
            take_while1(|c: char| c.is_ascii_alphanumeric() || c == '*')(input)?;
        let (input, _) = char('}')(input)?;

        match env_name {
            "array" => self.parse_array_env(input, env_name),
            "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Bmatrix" | "Vmatrix" | "matrix*"
            | "pmatrix*" | "bmatrix*" | "vmatrix*" | "Bmatrix*" | "Vmatrix*" => {
                self.parse_matrix_env(input, env_name)
            },
            "smallmatrix" => self.parse_smallmatrix_env(input, env_name),
            "CD" => self.parse_cd_env(input, env_name),
            "prooftree" => self.parse_prooftree_env(input, env_name),
            "xy" => self.parse_xy_env(input, env_name),
            "cases" | "dcases" => self.parse_cases_env(input, env_name),
            "rcases" | "drcases" => self.parse_rcases_env(input, env_name),
            "numcases" | "subnumcases" => self.parse_numcases_env(input, env_name),
            "empheq" => self.parse_empheq_env(input),
            "aligned" | "align" | "align*" => self.parse_align_env(input, env_name),
            "alignedat" => self.parse_alignedat_env(input, env_name),
            "gathered" => self.parse_gather_env(input, env_name),
            "gather" | "gather*" => self.parse_gather_env(input, env_name),
            "equation" | "equation*" => self.parse_equation_env(input, env_name),
            "multline" | "multline*" => self.parse_multline_env(input, env_name),
            "split" => self.parse_align_env(input, env_name),
            _ => self.parse_array_env(input, env_name),
        }
    }

    fn parse_array_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, columns) = 'col: {
            if let Some(inner) = input.strip_prefix('{') {
                // Scan with brace-depth to handle nested {} in column specs
                // e.g., {l*{2}{rc}} has nested braces
                let mut depth = 0u32;
                let mut end = None;
                for (i, c) in inner.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' if depth > 0 => depth -= 1,
                        '}' => {
                            end = Some(i);
                            break;
                        },
                        _ => {},
                    }
                }
                if let Some(end) = end {
                    let spec = &inner[..end];
                    let rest = &inner[end + 1..];
                    if !spec.is_empty() {
                        break 'col (rest, parse_column_spec(spec));
                    }
                }
            }
            (input, (SmallVec::new(), 0u32))
        };

        let (input, (rows, row_seps)) = self.parse_table_body_with_seps(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((
            input,
            self.build_array_with_seps_rows(&columns.0, columns.1, row_seps, rows),
        ))
    }

    fn parse_matrix_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        // Starred variants (matrix*, pmatrix*, etc.) accept optional [l], [r], [c]
        let (input, col_align) = if env_name.ends_with('*') {
            let (input, _) = ws(input)?;
            if input.starts_with('[') {
                let a = match input.as_bytes().get(1) {
                    Some(b'l') => ColumnAlign::Left,
                    Some(b'r') => ColumnAlign::Right,
                    _ => ColumnAlign::Center,
                };
                let end = input.find(']').unwrap_or(2);
                (&input[end + 1..], Some(a))
            } else {
                (input, None)
            }
        } else {
            (input, None)
        };

        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let base = env_name.strip_suffix('*').unwrap_or(env_name);
        let (open_s, close_s) = match base {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "vmatrix" => ("|", "|"),
            "Bmatrix" => ("{", "}"),
            "Vmatrix" => ("\u{2016}", "\u{2016}"),
            _ => ("", ""),
        };

        let align = col_align.unwrap_or(ColumnAlign::Center);
        let columns: SmallVec<[ColumnAlign; 4]> = if let Some(first_row) = rows.first() {
            smallvec::smallvec![align; first_row.len()]
        } else {
            SmallVec::new()
        };

        let array_id = self.build_array(&columns, rows);

        if open_s.is_empty() {
            Ok((input, array_id))
        } else {
            let open = self.arena.push_str(open_s);
            let close = self.arena.push_str(close_s);
            let content = self.arena.push_children(&[array_id]);
            Ok((
                input,
                self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }),
            ))
        }
    }

    /// Parse the amscd `\begin{CD} ... \end{CD}` environment for simple
    /// commutative diagrams (horizontal and vertical arrows; no diagonals).
    ///
    /// The body is laid out as a centered [`CompactNode::Array`] grid in which
    /// object columns (even, 0-indexed) alternate with connector columns (odd).
    /// Objects and vertical connectors occupy object columns; horizontal arrows
    /// occupy connector columns. Malformed bodies recover leniently.
    fn parse_cd_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let body_len = cd_body_len(input);
        let body = &input[..body_len];
        let rest = &input[body_len..];
        let (rest, _) = ws(rest)?;
        let (rest, _) = tag("\\end")(rest)?;
        let (rest, _) = ws(rest)?;
        let (rest, _) = char('{')(rest)?;
        let (rest, _) = tag(env_name)(rest)?;
        let (rest, _) = char('}')(rest)?;

        let mut rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::new();
        let mut num_cols = 0usize;
        for row_str in cd_split_rows(body) {
            let tokens = cd_tokenize_row(row_str);
            if tokens.is_empty() {
                continue;
            }
            // Place tokens into alternating object/connector columns.
            let mut placed: Vec<(usize, SmallVec<[NodeId; 8]>)> = Vec::with_capacity(tokens.len());
            let mut col = 0usize;
            for tok in &tokens {
                if tok.is_object {
                    if !col.is_multiple_of(2) {
                        col += 1; // skip an empty connector slot
                    }
                } else if col.is_multiple_of(2) {
                    col += 1; // skip an empty object slot
                }
                let cell = self.parse_cd_cell(tok);
                placed.push((col, cell));
                col += 1;
            }
            let width = placed.iter().map(|(c, _)| c + 1).max().unwrap_or(0);
            let mut row: Vec<SmallVec<[NodeId; 8]>> = (0..width).map(|_| SmallVec::new()).collect();
            for (c, cell) in placed {
                row[c] = cell;
            }
            num_cols = num_cols.max(width);
            rows.push(row);
        }

        let columns: SmallVec<[ColumnAlign; 4]> =
            smallvec::smallvec![ColumnAlign::Center; num_cols];
        let array = self.build_array_with_spacing(&columns, rows, 150, false, 0, 0);
        Ok((rest, array))
    }

    /// Parse one `CD` token (object text or `@`-connector) into a cell.
    fn parse_cd_cell(&mut self, tok: &CdToken<'_>) -> SmallVec<[NodeId; 8]> {
        if let Some(conn) = tok.text.strip_prefix('@') {
            self.parse_cd_connector(conn)
        } else {
            match self.parse_node_list(tok.text) {
                Ok((_, ids)) => ids,
                Err(_) => SmallVec::new(),
            }
        }
    }

    /// Parse the part of a connector following `@` into cell contents.
    fn parse_cd_connector(&mut self, conn: &str) -> SmallVec<[NodeId; 8]> {
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        match conn.as_bytes().first() {
            // `@.` placeholder / empty cell (also empty `@`).
            None | Some(b'.') => {},
            // `@=` long equals (approximated by a plain `=`).
            Some(b'=') => {
                let sid = self.arena.push_str("=");
                out.push(
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
                );
            },
            // `@|` / `@\vert` / `@\Vert` vertical double line.
            Some(b'|') | Some(b'\\') => {
                let sid = self.push_char_str('\u{2016}'); // ‖
                out.push(
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
                );
            },
            // Horizontal arrows `@>top>bot>` / `@<top<bot<`.
            Some(d @ (b'>' | b'<')) => {
                let arrow = if *d == b'>' { '\u{2192}' } else { '\u{2190}' };
                let parts = cd_split_delim(conn, *d);
                let above = self.parse_cd_label(parts.get(1).copied().unwrap_or(""));
                let below = self.parse_cd_label(parts.get(2).copied().unwrap_or(""));
                out.push(self.cd_horizontal_arrow(arrow, above, below));
            },
            // Vertical arrows `@VleftVrightV` / `@AleftArightA`.
            Some(d @ (b'V' | b'A')) => {
                let arrow = if *d == b'V' { '\u{2193}' } else { '\u{2191}' };
                let parts = cd_split_delim(conn, *d);
                let left = self.parse_cd_label(parts.get(1).copied().unwrap_or(""));
                let right = self.parse_cd_label(parts.get(2).copied().unwrap_or(""));
                let sid = self.push_char_str(arrow);
                let glyph = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Relation, sid));
                if let Some(l) = left {
                    out.push(l);
                }
                out.push(glyph);
                if let Some(r) = right {
                    out.push(r);
                }
            },
            // Unknown connector: recover as an empty cell.
            _ => {},
        }
        out
    }

    /// Parse a horizontal-arrow label segment into a single node, or `None`
    /// when empty.
    fn parse_cd_label(&mut self, seg: &str) -> Option<NodeId> {
        let seg = seg.trim();
        if seg.is_empty() {
            return None;
        }
        match self.parse_node_list(seg) {
            Ok((_, ids)) if !ids.is_empty() => Some(self.flatten_single(&ids)),
            _ => None,
        }
    }

    /// Build a stretchy horizontal arrow with optional over/under labels,
    /// reusing the extensible-arrow mechanism.
    fn cd_horizontal_arrow(
        &mut self,
        arrow: char,
        above: Option<NodeId>,
        below: Option<NodeId>,
    ) -> NodeId {
        let sid = self.push_char_str(arrow);
        let base = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Relation, sid));
        match (above, below) {
            (Some(over), Some(under)) => self.arena.push_node(CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits: false,
            }),
            (Some(over), None) => self.arena.push_node(CompactNode::Overscript {
                base,
                over,
                movable_limits: false,
            }),
            (None, Some(under)) => self.arena.push_node(CompactNode::Underscript {
                base,
                under,
                movable_limits: false,
            }),
            (None, None) => base,
        }
    }

    // ── bussproofs (proof trees) ───────────────────────────────────────

    /// Parse a `\begin{prooftree} ... \end{prooftree}` environment into a
    /// vertically-stacked proof tree built from [`CompactNode::Array`]
    /// (centered single column) and [`CompactNode::Rule`] inference lines.
    /// Malformed bodies recover leniently (see [`Self::build_bussproofs`]).
    fn parse_prooftree_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let body_len = cd_body_len(input);
        let body = &input[..body_len];
        let rest = &input[body_len..];
        let (rest, _) = ws(rest)?;
        let (rest, _) = tag("\\end")(rest)?;
        let (rest, _) = ws(rest)?;
        let (rest, _) = char('{')(rest)?;
        let (rest, _) = tag(env_name)(rest)?;
        let (rest, _) = char('}')(rest)?;
        let node = self.build_bussproofs(body);
        Ok((rest, node))
    }

    /// Best-effort bare bussproofs form: `\AxiomC{...} ... \DisplayProof`.
    /// `input` is positioned just after the leading command name `name`.
    fn parse_bussproofs_inline<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let end = bussproofs_inline_end(input);
        let mut body = String::with_capacity(end + name.len() + 1);
        body.push('\\');
        body.push_str(name);
        body.push_str(&input[..end]);
        let node = self.build_bussproofs(&body);
        Ok((&input[end..], node))
    }

    /// Tokenize a bussproofs body into commands and run the proof-construction
    /// stack. Stack underflow and leftover entries never panic: missing
    /// premises are padded with empty rows and leftover stack entries are
    /// wrapped in a `Row`.
    fn build_bussproofs(&mut self, body: &str) -> NodeId {
        let mut stack: Vec<NodeId> = Vec::new();
        let mut right_label: Option<NodeId> = None;
        let mut left_label: Option<NodeId> = None;
        let mut rest = body;

        while let Some(bs) = rest.find('\\') {
            rest = &rest[bs + 1..];
            let nlen = rest
                .as_bytes()
                .iter()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            let name = &rest[..nlen];
            rest = &rest[nlen..];

            match name {
                "AxiomC" | "AXC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    let leaf = self.buss_content(arg.unwrap_or(""));
                    stack.push(leaf);
                    rest = after;
                },
                "UnaryInfC" | "UIC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    self.buss_infer(
                        &mut stack,
                        1,
                        arg.unwrap_or(""),
                        &mut left_label,
                        &mut right_label,
                    );
                    rest = after;
                },
                "BinaryInfC" | "BIC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    self.buss_infer(
                        &mut stack,
                        2,
                        arg.unwrap_or(""),
                        &mut left_label,
                        &mut right_label,
                    );
                    rest = after;
                },
                "TrinaryInfC" | "TIC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    self.buss_infer(
                        &mut stack,
                        3,
                        arg.unwrap_or(""),
                        &mut left_label,
                        &mut right_label,
                    );
                    rest = after;
                },
                "QuaternaryInfC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    self.buss_infer(
                        &mut stack,
                        4,
                        arg.unwrap_or(""),
                        &mut left_label,
                        &mut right_label,
                    );
                    rest = after;
                },
                "QuinaryInfC" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    self.buss_infer(
                        &mut stack,
                        5,
                        arg.unwrap_or(""),
                        &mut left_label,
                        &mut right_label,
                    );
                    rest = after;
                },
                "RightLabel" | "RL" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    right_label = Some(self.buss_content(arg.unwrap_or("")));
                    rest = after;
                },
                "LeftLabel" | "LL" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    left_label = Some(self.buss_content(arg.unwrap_or("")));
                    rest = after;
                },
                // \DisplayProof / \DP take no argument; everything else is
                // skipped along with any braced argument it might carry.
                "DisplayProof" | "DP" => {},
                _ => {
                    let (after, _) = bussproofs_take_arg(rest);
                    rest = after;
                },
            }
        }

        match stack.len() {
            1 => stack[0],
            0 => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            _ => {
                let span = self.arena.push_children(&stack);
                self.arena.push_node(CompactNode::Row(span))
            },
        }
    }

    /// Pop `arity` premises (lenient), build an inference node with the given
    /// conclusion source and pending labels, and push it back on the stack.
    fn buss_infer(
        &mut self,
        stack: &mut Vec<NodeId>,
        arity: usize,
        concl_src: &str,
        left: &mut Option<NodeId>,
        right: &mut Option<NodeId>,
    ) {
        let avail = stack.len().min(arity);
        let split = stack.len() - avail;
        let mut premises: SmallVec<[NodeId; 4]> = stack.drain(split..).collect();
        while premises.len() < arity {
            let empty = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            premises.insert(0, empty);
        }
        let concl = self.buss_content(concl_src);
        let node = self.buss_make_node(&premises, concl, left.take(), right.take());
        stack.push(node);
    }

    /// Assemble a single proof node: premises stacked horizontally above a
    /// [`CompactNode::Rule`] inference line (optionally flanked by labels),
    /// above the centered conclusion.
    fn buss_make_node(
        &mut self,
        premises: &[NodeId],
        concl: NodeId,
        left: Option<NodeId>,
        right: Option<NodeId>,
    ) -> NodeId {
        let prem_row = self.buss_hstack(premises);
        let width = self
            .buss_width(prem_row)
            .max(self.buss_width(concl))
            .max(0.6)
            + 0.3;
        let rule = self.arena.push_node(CompactNode::Rule {
            width: rat_em(width),
            height: Rational::new(4, 100),
            lift: Rational::new(6, 100),
        });
        let rule_row = if left.is_some() || right.is_some() {
            let mut items: SmallVec<[NodeId; 4]> = SmallVec::new();
            if let Some(l) = left {
                items.push(l);
                items.push(self.buss_space());
            }
            items.push(rule);
            if let Some(r) = right {
                items.push(self.buss_space());
                items.push(r);
            }
            self.flatten_single(&items)
        } else {
            rule
        };

        let columns = [ColumnAlign::Center];
        let rows = vec![
            vec![smallvec::smallvec![prem_row]],
            vec![smallvec::smallvec![rule_row]],
            vec![smallvec::smallvec![concl]],
        ];
        self.build_array(&columns, rows)
    }

    /// Lay premises out horizontally, separated by gaps.
    fn buss_hstack(&mut self, premises: &[NodeId]) -> NodeId {
        if premises.len() == 1 {
            return premises[0];
        }
        let mut items: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (i, &p) in premises.iter().enumerate() {
            if i > 0 {
                items.push(self.buss_space());
            }
            items.push(p);
        }
        self.flatten_single(&items)
    }

    /// A horizontal gap used between premises and around inference lines.
    fn buss_space(&mut self) -> NodeId {
        self.arena
            .push_node(CompactNode::Space(Rational::new(3, 2)))
    }

    /// Parse the content of an axiom/conclusion/label, stripping surrounding
    /// inline-math delimiters (`$...$` or `\(...\)`).
    fn buss_content(&mut self, src: &str) -> NodeId {
        let mut s = src.trim();
        if let Some(inner) = s.strip_prefix('$').and_then(|x| x.strip_suffix('$')) {
            s = inner.trim();
        } else if let Some(inner) = s.strip_prefix("\\(").and_then(|x| x.strip_suffix("\\)")) {
            s = inner.trim();
        }
        if s.is_empty() {
            return self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
        }
        match self.parse_node_list(s) {
            Ok((_, ids)) if !ids.is_empty() => self.flatten_single(&ids),
            _ => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        }
    }

    /// Estimate the rendered em-width of a node (used to size inference lines).
    fn buss_width(&self, id: NodeId) -> f64 {
        match *self.arena.node(id) {
            CompactNode::Number(s)
            | CompactNode::Identifier(s)
            | CompactNode::Text(_, s)
            | CompactNode::Symbol(_, s)
            | CompactNode::Middle(s) => self.arena.str_val(s).chars().count() as f64 * 0.55,
            CompactNode::Operator { name, .. } => {
                self.arena.str_val(name).chars().count() as f64 * 0.6
            },
            CompactNode::Space(r) => r.as_f64(),
            CompactNode::Rule { width, .. } => width.as_f64(),
            CompactNode::Row(span) | CompactNode::Styled(_, span) => self
                .arena
                .children(span)
                .iter()
                .map(|&c| self.buss_width(c))
                .sum(),
            CompactNode::Array {
                cells, num_cols, ..
            } => {
                let nc = (num_cols as usize).max(1);
                let ids = self.arena.children(cells);
                let mut max_w = 0.0_f64;
                for chunk in ids.chunks(nc) {
                    let row_w: f64 = chunk.iter().map(|&c| self.buss_width(c)).sum();
                    max_w = max_w.max(row_w);
                }
                max_w
            },
            _ => 0.8,
        }
    }

    // ── xypic (xymatrix) ───────────────────────────────────────────────

    /// Parse the `\xymatrix{...}` command into an arrow-glyph approximation
    /// grid. Inline options (e.g. `@C=1pc`) before the body are skipped.
    fn parse_xymatrix_cmd<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let start = input.find('{').unwrap_or(input.len());
        let Some((rest, body)) = super::mhchem::capture_braced(&input[start..]) else {
            return Ok((
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            ));
        };
        let node = self.build_xymatrix(body);
        Ok((rest, node))
    }

    /// Parse `\begin{xy} ... \end{xy}`. Only an embedded `\xymatrix{...}` is
    /// rendered (arrow-glyph approximation); the full XY coordinate/path
    /// language is deferred and recovered as an empty node.
    fn parse_xy_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let body_len = cd_body_len(input);
        let body = &input[..body_len];
        let rest = &input[body_len..];
        let (rest, _) = ws(rest)?;
        let (rest, _) = tag("\\end")(rest)?;
        let (rest, _) = ws(rest)?;
        let (rest, _) = char('{')(rest)?;
        let (rest, _) = tag(env_name)(rest)?;
        let (rest, _) = char('}')(rest)?;

        let node = if let Some(pos) = body.find("\\xymatrix") {
            let after = &body[pos + "\\xymatrix".len()..];
            let start = after.find('{').unwrap_or(after.len());
            match super::mhchem::capture_braced(&after[start..]) {
                Some((_, inner)) => self.build_xymatrix(inner),
                None => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            }
        } else {
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
        };
        Ok((rest, node))
    }

    /// Build a centered [`CompactNode::Array`] grid from an xymatrix body,
    /// rendering `\ar[dir]` commands as directional arrow glyphs.
    fn build_xymatrix(&mut self, body: &str) -> NodeId {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::new();
        let mut num_cols = 0usize;
        for row_src in cd_split_rows(body) {
            let cells_src = xy_split_cells(row_src);
            let mut row: Vec<SmallVec<[NodeId; 8]>> = Vec::with_capacity(cells_src.len());
            for cell_src in &cells_src {
                row.push(self.parse_xy_cell(cell_src));
            }
            num_cols = num_cols.max(row.len());
            rows.push(row);
        }
        // Drop a trailing all-empty row produced by a dangling `\\`.
        if rows.len() > 1
            && rows
                .last()
                .is_some_and(|r| r.iter().all(SmallVec::is_empty))
        {
            rows.pop();
        }
        let columns: SmallVec<[ColumnAlign; 4]> =
            smallvec::smallvec![ColumnAlign::Center; num_cols];
        self.build_array_with_spacing(&columns, rows, 130, false, 0, 0)
    }

    /// Parse one xymatrix cell: leading object content followed by any
    /// `\ar[dir]^{..}_{..}` arrows rendered as glyphs.
    fn parse_xy_cell(&mut self, cell: &str) -> SmallVec<[NodeId; 8]> {
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        let head_end = xy_find_ar(cell);
        let head = cell[..head_end].trim();
        if !head.is_empty()
            && let Ok((_, ids)) = self.parse_node_list(head)
        {
            out.extend(ids);
        }
        let mut rest = &cell[head_end..];
        while rest.starts_with("\\ar") {
            rest = &rest[3..];
            let (after_dir, dir) = xy_take_bracket(rest);
            rest = after_dir;
            let mut over = None;
            let mut under = None;
            loop {
                let t = rest.trim_start();
                if let Some(r) = t.strip_prefix('^') {
                    let (after, lab) = xy_take_script(r);
                    over = self.xy_label(lab);
                    rest = after;
                } else if let Some(r) = t.strip_prefix('_') {
                    let (after, lab) = xy_take_script(r);
                    under = self.xy_label(lab);
                    rest = after;
                } else {
                    rest = t;
                    break;
                }
            }
            out.push(self.xy_arrow(dir, over, under));
            let next = xy_find_ar(rest);
            let between = rest[..next].trim();
            if !between.is_empty()
                && let Ok((_, ids)) = self.parse_node_list(between)
            {
                out.extend(ids);
            }
            rest = &rest[next..];
        }
        out
    }

    /// Parse an arrow label into a single node, or `None` when empty.
    fn xy_label(&mut self, src: &str) -> Option<NodeId> {
        let s = src.trim();
        if s.is_empty() {
            return None;
        }
        match self.parse_node_list(s) {
            Ok((_, ids)) if !ids.is_empty() => Some(self.flatten_single(&ids)),
            _ => None,
        }
    }

    /// Build an arrow glyph for `dir` with optional over/under labels.
    fn xy_arrow(&mut self, dir: &str, over: Option<NodeId>, under: Option<NodeId>) -> NodeId {
        let sid = self.push_char_str(xy_arrow_glyph(dir));
        let base = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Relation, sid));
        match (over, under) {
            (Some(over), Some(under)) => self.arena.push_node(CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits: false,
            }),
            (Some(over), None) => self.arena.push_node(CompactNode::Overscript {
                base,
                over,
                movable_limits: false,
            }),
            (None, Some(under)) => self.arena.push_node(CompactNode::Underscript {
                base,
                under,
                movable_limits: false,
            }),
            (None, None) => base,
        }
    }

    fn parse_cases_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let array_id = self.build_array_with_spacing(
            &[ColumnAlign::Left, ColumnAlign::Left],
            rows,
            120,
            false,
            0,
            0,
        );
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children(&[array_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    fn parse_rcases_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let array_id = self.build_array_with_spacing(
            &[ColumnAlign::Left, ColumnAlign::Left],
            rows,
            120,
            false,
            0,
            0,
        );
        let open = self.arena.push_str("");
        let close = self.arena.push_str("}");
        let content = self.arena.push_children(&[array_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    /// `cases` package: `numcases`/`subnumcases`.
    ///
    /// Syntax: `\begin{numcases}{<left-expr>} <val> & <cond> \\ ... \end{...}`.
    /// Builds `<left-expr> = { <cases-rows> }` and appends a right-aligned
    /// equation-number column whose cells are `(1a)`, `(1b)`, … Since this is a
    /// single-formula converter with no document-wide counter, the master
    /// number is fixed at `1`; the sub-letters `a, b, c, …` increment per row.
    /// `subnumcases` is handled identically (its first argument is treated as
    /// the shared left expression, best-effort).
    fn parse_numcases_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        // Mandatory first argument: the shared left-hand expression.
        let (input, left_id) = self.parse_required_group(input)?;
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        // Append an upright parenthesized equation-number cell to each row.
        let mut numbered_rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::with_capacity(rows.len());
        for (i, mut row) in rows.into_iter().enumerate() {
            let letter = char::from(b'a' + (i % 26) as u8);
            let label = format!("(1{letter})");
            let sid = self.arena.push_str(&label);
            let num_id = self
                .arena
                .push_node(CompactNode::Text(TextStyle::Normal, sid));
            let mut cell: SmallVec<[NodeId; 8]> = SmallVec::new();
            cell.push(num_id);
            row.push(cell);
            numbered_rows.push(row);
        }

        // value | condition | (number) — number right-aligned far from cases.
        let array_id = self.build_array_with_spacing(
            &[ColumnAlign::Left, ColumnAlign::Left, ColumnAlign::Right],
            numbered_rows,
            120,
            false,
            0,
            0,
        );
        // Big left brace, no right delimiter (cases style).
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children(&[array_id]);
        let fenced = self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        });

        // <left-expr> = { cases }
        let eq_sid = self.arena.push_str("=");
        let eq_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Relation, eq_sid));
        let row_span = self.arena.push_children(&[left_id, eq_id, fenced]);
        Ok((input, self.arena.push_node(CompactNode::Row(row_span))))
    }

    /// `empheq` package: boxed/delimited equation displays.
    ///
    /// Syntax: `\begin{empheq}[<options>]{<inner-env>} <body> \end{empheq}`.
    /// The body is parsed *as* the inner environment (`align`, `gather`, …) by
    /// dispatching to the matching parser with `\end{empheq}` as terminator.
    /// Recognized options (best-effort): `box=…` wraps in `Boxed`; `left=…` and
    /// `right=…` wrap in a `Fenced` with the given delimiters. Unknown options
    /// are ignored gracefully.
    fn parse_empheq_env<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Optional `[options]`.
        let (input, _) = ws(input)?;
        let (input, opts) = if let Some(after) = input.strip_prefix('[') {
            // Scan to the top-level closing `]` (brace-depth aware).
            let mut depth = 0i32;
            let mut end = None;
            for (i, c) in after.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' if depth > 0 => depth -= 1,
                    ']' if depth == 0 => {
                        end = Some(i);
                        break;
                    },
                    _ => {},
                }
            }
            match end {
                Some(e) => (&after[e + 1..], &after[..e]),
                None => (after, after),
            }
        } else {
            (input, "")
        };

        // Mandatory `{inner-env-name}`.
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, inner) = take_while1(|c: char| c.is_ascii_alphanumeric() || c == '*')(input)?;
        let (input, _) = char('}')(input)?;

        // Parse the body as the inner environment, terminated by `\end{empheq}`.
        let (input, inner_id) = match inner {
            "gather" | "gather*" | "gathered" => self.parse_gather_env(input, "empheq")?,
            "equation" | "equation*" => self.parse_equation_env(input, "empheq")?,
            "multline" | "multline*" => self.parse_multline_env(input, "empheq")?,
            "alignat" | "alignat*" | "alignedat" => self.parse_alignedat_env(input, "empheq")?,
            // align / align* / aligned / split / flalign / unknown → aligned grid.
            _ => self.parse_align_env(input, "empheq")?,
        };

        // Apply recognized options.
        let (box_spec, left_spec, right_spec) = parse_empheq_options(opts);
        let mut node = inner_id;
        if let Some(spec) = box_spec {
            node = self.apply_empheq_box(node, &spec);
        }
        if left_spec.is_some() || right_spec.is_some() {
            let open = self.resolve_empheq_delim(left_spec.as_deref());
            let close = self.resolve_empheq_delim(right_spec.as_deref());
            let content = self.arena.push_children(&[node]);
            node = self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            });
        }
        Ok((input, node))
    }

    /// Wrap a node in `Boxed` according to an empheq `box=` spec.
    /// `\fbox`/`\boxed` → plain border; `\colorbox{c}` → background; and
    /// `\fcolorbox{frame}{bg}` → both. Unknown specs fall back to a plain box.
    fn apply_empheq_box(&mut self, content: NodeId, spec: &str) -> NodeId {
        let spec = spec.trim();
        let (bg, border) = if let Some(rest) = spec.strip_prefix("\\colorbox") {
            (self.resolve_color(empheq_brace_arg(rest, 0)), String::new())
        } else if let Some(rest) = spec.strip_prefix("\\fcolorbox") {
            (
                self.resolve_color(empheq_brace_arg(rest, 1)),
                self.resolve_color(empheq_brace_arg(rest, 0)),
            )
        } else {
            // \fbox, \boxed, \widefbox, or unknown → plain box.
            (String::new(), String::new())
        };
        let bg_color = self.arena.push_str(&bg);
        let border_color = self.arena.push_str(&border);
        self.arena.push_node(CompactNode::Boxed {
            content,
            bg_color,
            border_color,
        })
    }

    /// Resolve an empheq delimiter spec (e.g. `\Rightarrow`, `[`, `.`) to a
    /// `StrId`. `None`/`.`/empty yield an empty (invisible) delimiter.
    fn resolve_empheq_delim(&mut self, spec: Option<&str>) -> StrId {
        let tok = spec.unwrap_or("").trim();
        if tok.is_empty() || tok == "." {
            return self.arena.push_str("");
        }
        if let Some(name) = tok.strip_prefix('\\') {
            let name = name.trim();
            if let Some(ch) = latex_to_unicode(name) {
                return self.push_char_str(ch);
            }
            return self.arena.push_str(name);
        }
        self.arena.push_str(tok)
    }

    fn parse_smallmatrix_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let columns: SmallVec<[ColumnAlign; 4]> = if let Some(first_row) = rows.first() {
            smallvec::smallvec![ColumnAlign::Center; first_row.len()]
        } else {
            SmallVec::new()
        };

        Ok((
            input,
            self.build_array_with_spacing(&columns, rows, 50, false, 0, 0),
        ))
    }

    fn parse_align_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        // Aligned environments use repeating rl alignment pairs.
        // Compute the actual column count and repeat the pattern.
        let max_row_len = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let base_pattern = [ColumnAlign::Right, ColumnAlign::Left];
        let columns: SmallVec<[ColumnAlign; 8]> = base_pattern
            .iter()
            .copied()
            .cycle()
            .take(max_row_len.max(base_pattern.len()))
            .collect();

        Ok((
            input,
            self.build_array_with_spacing(&columns, rows, 100, true, 0, 0),
        ))
    }

    fn parse_alignedat_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        // \begin{alignedat}{n} — n is the number of rl pairs.
        // We consume the mandatory argument but derive the actual column
        // count from the rows, same as parse_align_env.
        let (input, _) = ws(input)?;
        let input = if let Ok((r, _)) = char::<_, nom::error::Error<&str>>('{')(input) {
            let (r, _) = take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(r)
                .unwrap_or((r, ""));
            let (r, _) = char::<_, nom::error::Error<&str>>('}')(r).map_err(|_| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
            })?;
            r
        } else {
            input
        };

        self.parse_align_env(input, env_name)
    }

    fn parse_gather_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((
            input,
            self.build_array_with_spacing(&[ColumnAlign::Center], rows, 100, true, 0, 0),
        ))
    }

    fn parse_equation_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let mut ids = SmallVec::<[NodeId; 8]>::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            match self.parse_cell_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        let (input, _) = ws(rest)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, self.flatten_single(&ids)))
    }

    fn parse_multline_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let num_rows = rows.len();
        let columns: SmallVec<[ColumnAlign; 4]> = if num_rows <= 1 {
            smallvec::smallvec![ColumnAlign::Center]
        } else {
            smallvec::smallvec![ColumnAlign::Left]
        };
        Ok((input, self.build_array(&columns, rows)))
    }

    // -----------------------------------------------------------------------
    // Table parsing helpers
    // -----------------------------------------------------------------------

    fn parse_table_body<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Vec<Vec<SmallVec<[NodeId; 8]>>>> {
        let (rest, (rows, _row_seps)) = self.parse_table_body_with_seps(input)?;
        Ok((rest, rows))
    }

    /// Like [`parse_table_body`], but also returns the horizontal-rule bitmask:
    /// bit `i` set means a solid `\hline` precedes row `i` (bit `num_rows`
    /// means a rule at the bottom edge).
    #[allow(clippy::type_complexity)]
    fn parse_table_body_with_seps<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, (Vec<Vec<SmallVec<[NodeId; 8]>>>, u32)> {
        let mut rows = Vec::with_capacity(4);
        let mut row_seps: u32 = 0;
        // Skip leading \hline / \hdashline before first row (top edge = bit 0)
        let (mut rest, top_hline) = take_table_directives(input);
        if top_hline {
            row_seps |= 1;
        }
        let (r, first) = self.parse_table_row(rest)?;
        rows.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, (extra, hline))) => {
                    // Apply \\[dim] extra inter-row space by adding an invisible
                    // depth strut to the row just completed.
                    if let Some(dim_str) = extra
                        && let Some(row) = rows.last_mut()
                        && let Some(cell) = row.last_mut()
                    {
                        let mut dp = LatexCompactParser::with_capacity(dim_str.len());
                        if let Ok((_, d)) = dp.parse_dimension(dim_str)
                            && d.as_f64() > 0.0
                        {
                            let strut = self.arena.push_node(CompactNode::Rule {
                                width: Rational::zero(),
                                height: d,
                                lift: Rational::new(-d.num, d.den),
                            });
                            cell.push(strut);
                        }
                    }
                    // A `\hline` after this separator precedes the next row.
                    if hline && rows.len() < 32 {
                        row_seps |= 1 << rows.len();
                    }
                    let (r, row) = self.parse_table_row(after_sep)?;
                    rows.push(row);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }
        // Drop a trailing rule recorded against a phantom empty row: if the last
        // row is entirely empty it was produced by a trailing `\\`; move its rule
        // bit down to the true bottom edge and discard the empty row.
        if rows.len() > 1
            && rows
                .last()
                .is_some_and(|r| r.iter().all(|c| c.is_empty()))
        {
            let last = rows.len();
            if row_seps & (1 << last) != 0 {
                row_seps &= !(1 << last);
                row_seps |= 1 << (last - 1);
            }
            // keep the empty row only if it carries no rule semantics; existing
            // behavior preserved otherwise (do not change row count here).
        }
        Ok((rest, (rows, row_seps)))
    }

    fn parse_table_row<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Vec<SmallVec<[NodeId; 8]>>> {
        let mut raw: Vec<(SmallVec<[NodeId; 8]>, ColortblCell)> = Vec::with_capacity(4);
        let mut rest = input;
        let (r, first) = self.parse_table_cell_colortbl(rest)?;
        raw.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_cell_separator(rest) {
                Ok((after_sep, ())) => {
                    let (r, cell) = self.parse_table_cell_colortbl(after_sep)?;
                    raw.push(cell);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }
        // A `\rowcolor` on any cell (conventionally the first) colors the row.
        let row_bg = raw.iter().find_map(|(_, cc)| cc.row_bg.clone());
        let mut cells = Vec::with_capacity(raw.len());
        for (ids, cc) in raw {
            // Cell precedence: `\cellcolor` overrides the row background.
            match cc.cell_bg.or_else(|| row_bg.clone()) {
                Some(color) if !color.is_empty() => cells.push(self.wrap_cell_bg(ids, &color)),
                _ => cells.push(ids),
            }
        }
        Ok((rest, cells))
    }

    /// Wrap a cell's content nodes in a [`CompactNode::Boxed`] with the given
    /// background color (no border), producing a single-element cell.
    fn wrap_cell_bg(&mut self, ids: SmallVec<[NodeId; 8]>, color: &str) -> SmallVec<[NodeId; 8]> {
        let span = self.arena.push_children(&ids);
        let content = self.arena.push_node(CompactNode::Row(span));
        let bg_color = self.arena.push_str(color);
        let border_color = self.arena.push_str("");
        let boxed = self.arena.push_node(CompactNode::Boxed {
            content,
            bg_color,
            border_color,
        });
        smallvec::smallvec![boxed]
    }

    /// Parse one table cell, first consuming any leading colortbl directives
    /// (`\cellcolor`, `\rowcolor`, `\columncolor`, `\arrayrulecolor`) and
    /// returning their resolved backgrounds alongside the cell body.
    fn parse_table_cell_colortbl<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, (SmallVec<[NodeId; 8]>, ColortblCell)> {
        let mut rest = input;
        let mut cc = ColortblCell::default();
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            if let Some(after) = strip_control_word(rest, "cellcolor")
                && let Ok((r2, color)) = self.parse_colortbl_color(after)
            {
                cc.cell_bg = Some(color);
                rest = r2;
                continue;
            }
            // Inline `\columncolor` (best-effort): color the cell it appears in.
            // Full `>{\columncolor{..}}` column-spec support is deferred.
            if let Some(after) = strip_control_word(rest, "columncolor")
                && let Ok((r2, color)) = self.parse_colortbl_color(after)
            {
                if cc.cell_bg.is_none() {
                    cc.cell_bg = Some(color);
                }
                rest = r2;
                continue;
            }
            if let Some(after) = strip_control_word(rest, "rowcolor")
                && let Ok((r2, color)) = self.parse_colortbl_color(after)
            {
                cc.row_bg = Some(color);
                rest = r2;
                continue;
            }
            // `\arrayrulecolor` is deferred: rule colors are not representable
            // on the Array node, so accept and ignore the argument gracefully.
            if let Some(after) = strip_control_word(rest, "arrayrulecolor")
                && let Ok((r2, _)) = self.parse_colortbl_color(after)
            {
                rest = r2;
                continue;
            }
            break;
        }
        let (rest, ids) = self.parse_table_cell(rest)?;
        Ok((rest, (ids, cc)))
    }

    fn parse_table_cell<'a>(&mut self, input: &'a str) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            match self.parse_cell_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        Ok((rest, ids))
    }

    fn parse_cell_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Stop at &, \\, or \end
        if input.starts_with('&')
            || input.starts_with("\\\\")
            || input.starts_with("\\end")
            || input.is_empty()
        {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Eof,
            )));
        }
        // Handle leading ^/_ /' with implicit empty base in table cells
        if input.starts_with('^') || input.starts_with('_') || input.starts_with('\'') {
            let base = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            return self.parse_scripts(input, base);
        }
        self.parse_element(input)
    }

    // -----------------------------------------------------------------------
    // Lenient recovery
    // -----------------------------------------------------------------------

    pub(crate) fn parse_lenient_remainder(
        &mut self,
        mut input: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> SmallVec<[NodeId; 8]> {
        let mut ids = SmallVec::new();
        while !input.is_empty() {
            match self.parse_node_list(input) {
                Ok(("", parsed)) => {
                    ids.extend(parsed);
                    break;
                },
                Ok((rest, parsed)) => {
                    ids.extend(parsed);
                    if let Some(ch) = rest.chars().next() {
                        diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("skipped unexpected character: {ch:?}"),
                            span: None,
                        });
                        input = &rest[ch.len_utf8()..];
                    } else {
                        break;
                    }
                },
                Err(_) => {
                    if let Some(ch) = input.chars().next() {
                        let sid = self.push_char_str(ch);
                        ids.push(
                            self.arena
                                .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                        );
                        diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("treated as text: {ch:?}"),
                            span: None,
                        });
                        input = &input[ch.len_utf8()..];
                    } else {
                        break;
                    }
                },
            }
        }
        ids
    }
}
