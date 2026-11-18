mod dimension;
mod hbox;

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
use crate::unicode::decode_script_scalar;
use crate::unicode::fonts::latex_font_to_style;
use crate::unicode::symbols::{
    is_operator_command, latex_to_unicode, lookup_command, operator_has_limits,
};

use super::error::LatexParseError;
use super::preprocess::{ExpansionFailure, preprocess_cow};

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

/// Trim leading/trailing ASCII whitespace like `str::trim`, but keep a single
/// trailing whitespace character when it immediately follows a backslash. This
/// prevents destroying backslash-space commands such as `\ ` or `\\ ` during
/// trimming. Non-ASCII spaces are literal math characters, not source spacing.
fn trim_latex_input(input: &str) -> &str {
    let trimmed_start = input.trim_start_matches(|c: char| c.is_ascii_whitespace());

    let mut end = trimmed_start.len();
    while end > 0 {
        let b = trimmed_start.as_bytes()[end - 1];
        if !b.is_ascii_whitespace() {
            break;
        }
        // Preserve the whitespace that belongs to a trailing backslash command.
        if end >= 2 && trimmed_start.as_bytes()[end - 2] == b'\\' {
            break;
        }
        end -= 1;
    }
    &trimmed_start[..end]
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

/// Boundaries at which a scanner-level `\relax` leaves an empty unbraced
/// script argument. The following token must remain for the enclosing parser
/// (or for a complementary script) rather than escaping as a sibling.
#[inline]
fn script_argument_boundary(input: &str) -> bool {
    is_table_boundary(input)
        || input.starts_with('}')
        || is_cr_row_separator(input)
        || matches!(input.as_bytes().first(), Some(b'^' | b'_' | b'\''))
        || strip_control_word(input, "right").is_some()
        || strip_control_word(input, "middle").is_some()
}

/// Consume consecutive `\\limits` and `\\nolimits` modifiers after an
/// operator. TeX permits repeated placement modifiers and the last modifier
/// determines the operator's placement. A control-word boundary is required
/// so that names such as `\\limitsize` remain ordinary commands.
#[inline]
fn consume_limit_modifiers(mut input: &str) -> (&str, Option<LimitStyle>) {
    let mut style = None;
    loop {
        let skip = skip_ws_bytes(input.as_bytes());
        let trimmed = &input[skip..];
        let Some((rest, next_style)) = [
            ("\\limits", LimitStyle::AboveBelow),
            ("\\nolimits", LimitStyle::SubSup),
        ]
        .into_iter()
        .find_map(|(command, next_style)| {
            let rest = trimmed.strip_prefix(command)?;
            rest.chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphabetic())
                .then_some((rest, next_style))
        }) else {
            // Preserve whitespace that was inspected but did not belong to a
            // following modifier. In particular, a top-level newline must
            // remain visible to parse_scripts and the row parser.
            return (input, style);
        };
        style = Some(next_style);
        input = rest;
    }
}

#[inline]
fn starts_limit_modifier(input: &str) -> bool {
    let trimmed = &input[skip_ws_bytes(input.as_bytes())..];
    ["\\limits", "\\nolimits"].into_iter().any(|command| {
        let Some(rest) = trimmed.strip_prefix(command) else {
            return false;
        };
        rest.chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphabetic())
    })
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
        || strip_control_word(input, "end").is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScriptMarker {
    Sub,
    Sup,
    Primes,
    UnicodeSub,
    UnicodeSup,
}

impl ScriptMarker {
    fn at(input: &str) -> Option<Self> {
        match *input.as_bytes().first()? {
            b'_' => Some(Self::Sub),
            b'^' => Some(Self::Sup),
            b'\'' => Some(Self::Primes),
            byte if !byte.is_ascii() => {
                let (sub, _) = decode_script_scalar(input.chars().next()?)?;
                Some(if sub {
                    Self::UnicodeSub
                } else {
                    Self::UnicodeSup
                })
            },
            _ => None,
        }
    }

    fn is_subscript(self) -> bool {
        matches!(self, Self::Sub | Self::UnicodeSub)
    }
    fn is_unicode(self) -> bool {
        matches!(self, Self::UnicodeSub | Self::UnicodeSup)
    }
}

/// True when an n-ary operator has reached a grammar boundary where an empty
/// body is valid. Other input must be parsed as a real element so malformed
/// bodies propagate their original parser error.
#[inline]
fn is_nary_body_boundary(input: &str) -> bool {
    is_table_boundary(input)
        || input.starts_with('}')
        || is_cr_row_separator(input)
        || strip_control_word(input, "right").is_some()
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
                        // Consume a control word. TeX drops source whitespace
                        // immediately following it; keep control symbols such
                        // as `\ ` and escaped punctuation on their existing
                        // paths so explicitly requested spaces stay visible.
                        i += 1; // skip backslash
                        let start = i;
                        while i < len && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        if i == start && i < len {
                            // A control symbol consumes only its own character.
                            let ch = raw[i..].chars().next().unwrap();
                            out.push(ch);
                            i += ch.len_utf8();
                            continue;
                        } else if &raw[start..i] == "KaTeX" {
                            out.push_str("KaTeX");
                        } else if &raw[start..i] == "LaTeX" {
                            out.push_str("LaTeX");
                        } else if &raw[start..i] == "TeX" {
                            out.push_str("TeX");
                        } else if &raw[start..i] == "relax" {
                            // Native TeX no-op: do not turn `\relax` into
                            // visible identifier letters in a text run.
                        } else if let Some(ch) =
                            crate::unicode::symbols::latex_to_unicode(&raw[start..i])
                        {
                            // A symbol command (`\int`, `\alpha`) keeps its
                            // character rather than vanishing from the text.
                            out.push(ch);
                        } else if let Some((text, _)) =
                            super::packages::lookup_package_symbol(&raw[start..i])
                        {
                            out.push_str(text);
                        }
                        // Otherwise drop the command name. A control word also
                        // consumes any following source whitespace, just as
                        // TeX does. Explicit `\ ` is handled above and is not
                        // consumed here.
                        while i < len && raw.as_bytes()[i].is_ascii_whitespace() {
                            i += 1;
                        }
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

/// Decode literal content of an operatorname argument. Grouping braces are
/// removed, escaped punctuation remains literal, and known core or package
/// commands contribute their Unicode glyph.
fn decode_operator_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' | '}' => {},
            '\\' => {
                let Some(next) = chars.peek().copied() else {
                    out.push('\\');
                    break;
                };
                if next.is_ascii_alphabetic() {
                    let mut command = String::new();
                    while let Some(next) = chars.peek().copied() {
                        if next.is_ascii_alphabetic() {
                            command.push(next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(unicode) = latex_to_unicode(&command) {
                        out.push(unicode);
                    } else if let Some((unicode, _)) =
                        crate::latex::packages::lookup_package_symbol(&command)
                    {
                        out.push_str(unicode);
                    } else {
                        match command.as_str() {
                            "thinspace" => out.push('\u{2009}'),
                            "medspace" => out.push('\u{205F}'),
                            "enspace" | "enskip" => out.push('\u{2002}'),
                            "quad" => out.push('\u{2001}'),
                            "qquad" => out.push_str("\u{2001}\u{2001}"),
                            _ => out.push_str(&command),
                        }
                    }
                } else {
                    let escaped = chars.next().unwrap_or(next);
                    out.push(match escaped {
                        ',' => '\u{2009}',
                        ':' | '>' => '\u{205F}',
                        ' ' => '\u{00A0}',
                        _ => escaped,
                    });
                }
            },
            _ => out.push(ch),
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
    let mut escaped = false;
    for (i, c) in opts.char_indices() {
        if escaped {
            escaped = false;
            bytes_end = i + c.len_utf8();
            continue;
        }
        match c {
            '\\' => escaped = true,
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
                // `box`, `innerbox` and `outerbox` all wrap the body in a box.
                "box" | "innerbox" | "outerbox" => box_spec = Some(val),
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

/// Inference-line style for a bussproofs inference (`\singleLine`/`\noLine`/
/// `\doubleLine`/…).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BussLine {
    /// A single solid rule (default; also `\solidLine`/`\dottedLine`).
    Single,
    /// Two stacked rules (`\doubleLine`).
    Double,
    /// No rule (`\noLine`): premises sit directly above the conclusion.
    None,
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

fn legacy_font_style(name: &str) -> Option<TextStyle> {
    Some(match name {
        "rm" => TextStyle::Normal,
        "it" => TextStyle::Italic,
        "bf" => TextStyle::Bold,
        "sf" => TextStyle::SansSerif,
        "tt" => TextStyle::Monospace,
        "cal" => TextStyle::Script,
        _ => return None,
    })
}

fn legacy_font_declaration(input: &str) -> Option<(&str, TextStyle)> {
    let tail = input.strip_prefix('\\')?;
    let len = super::control_word_len(tail);
    Some((&tail[len..], legacy_font_style(&tail[..len])?))
}

/// Parse an optional leading `[model]` color-model argument (xcolor color
/// model: `rgb`, `RGB`, `HTML`, `gray`, `cmyk`, …). Returns `(rest, None)` when
/// no bracket group is present.
fn parse_optional_model(input: &str) -> IResult<&str, Option<String>> {
    if input.starts_with('[') {
        let (input, _) = char('[')(input)?;
        let (input, model) = take_while(|c: char| c != ']')(input)?;
        let (input, _) = char(']')(input)?;
        Ok((input, Some(model.trim().to_string())))
    } else {
        Ok((input, None))
    }
}

/// Consume a `{ ... }` group starting at byte offset `start` in `s` (skipping
/// leading whitespace). Returns `(index_after_close_brace, inner)` or `None`
/// when no balanced group is present. Used by array column-spec scanning.
fn take_braced_at(s: &str, start: usize) -> Option<(usize, &str)> {
    let sub = &s[start..];
    let ws_len = sub.len() - sub.trim_start().len();
    let rest = sub[ws_len..].strip_prefix('{')?;
    let open_abs = start + ws_len + 1; // absolute index of first inner char
    let mut depth = 1i32;
    for (rel, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open_abs + rel + 1, &rest[..rel]));
                }
            },
            _ => {},
        }
    }
    None
}

/// Map a LaTeX `\paired-delimiter` open/close token to its glyph string
/// (used by `\DeclarePairedDelimiter`). `.` / empty → no delimiter.
fn delim_to_glyph(d: &str) -> String {
    let d = d.trim();
    match d {
        "" | "." => String::new(),
        "\\lceil" => "\u{2308}".into(),
        "\\rceil" => "\u{2309}".into(),
        "\\lfloor" => "\u{230A}".into(),
        "\\rfloor" => "\u{230B}".into(),
        "<" | "\\lt" | "\\langle" => "\u{27E8}".into(),
        ">" | "\\gt" | "\\rangle" => "\u{27E9}".into(),
        "\\{" | "\\lbrace" => "{".into(),
        "\\}" | "\\rbrace" => "}".into(),
        "\\|" | "\\Vert" | "\\lVert" | "\\rVert" => "\u{2016}".into(),
        "\\vert" | "\\lvert" | "\\rvert" => "|".into(),
        "\\backslash" => "\\".into(),
        "(" | ")" | "[" | "]" | "|" | "/" => d.into(),
        _ => d.chars().next().map(|c| c.to_string()).unwrap_or_default(),
    }
}

/// Map a base relation/operator glyph to its precomposed *negated* Unicode
/// codepoint, used by `\centernot` so the slash is centered by the font and the
/// result round-trips to a named negation (e.g. `↛` → `\nrightarrow`). Returns
/// `None` when no precomposed negation exists (the caller then overlays a
/// combining solidus).
fn centernot_precomposed(ch: char) -> Option<char> {
    Some(match ch {
        '=' => '\u{2260}',        // ≠
        '<' => '\u{226E}',        // ≮
        '>' => '\u{226F}',        // ≯
        '\u{2192}' => '\u{219B}', // → ↛
        '\u{2190}' => '\u{219A}', // ← ↚
        '\u{2194}' => '\u{21AE}', // ↔ ↮
        '\u{21D2}' => '\u{21CF}', // ⇒ ⇏
        '\u{21D0}' => '\u{21CD}', // ⇐ ⇍
        '\u{21D4}' => '\u{21CE}', // ⇔ ⇎
        '\u{2208}' => '\u{2209}', // ∈ ∉
        '\u{220B}' => '\u{220C}', // ∋ ∌
        '\u{2282}' => '\u{2284}', // ⊂ ⊄
        '\u{2283}' => '\u{2285}', // ⊃ ⊅
        '\u{2286}' => '\u{2288}', // ⊆ ⊈
        '\u{2287}' => '\u{2289}', // ⊇ ⊉
        '\u{2261}' => '\u{2262}', // ≡ ≢
        '\u{223C}' => '\u{2241}', // ∼ ≁
        '\u{2243}' => '\u{2244}', // ≃ ≄
        '\u{2245}' => '\u{2247}', // ≅ ≇
        '\u{2248}' => '\u{2249}', // ≈ ≉
        '\u{2203}' => '\u{2204}', // ∃ ∄
        '\u{2223}' => '\u{2224}', // ∣ ∤
        '\u{2225}' => '\u{2226}', // ∥ ∦
        '\u{227A}' => '\u{2280}', // ≺ ⊀
        '\u{227B}' => '\u{2281}', // ≻ ⊁
        '\u{2264}' => '\u{2270}', // ≤ ≰
        '\u{2265}' => '\u{2271}', // ≥ ≱
        _ => return None,
    })
}

/// Parse an array preamble into its column alignments and separator mask.
///
/// The alignment list is stored in an arena span whose length is a `u16`.
/// Check each append and repetition before allocating or iterating so a
/// compact input such as `*{usize::MAX}{c}` cannot consume unbounded CPU or
/// memory while constructing a preamble that could never be represented.
fn parse_column_spec(spec: &str) -> Result<(SmallVec<[ColumnAlign; 4]>, u32)> {
    parse_column_spec_at_depth(spec, 0)
}

fn parse_column_spec_at_depth(spec: &str, depth: u32) -> Result<(SmallVec<[ColumnAlign; 4]>, u32)> {
    if depth >= MAX_PARSE_DEPTH {
        return Err(Error::LimitExceeded {
            kind: crate::error::LimitKind::Depth,
            limit: MAX_PARSE_DEPTH as usize,
        });
    }
    let mut result = SmallVec::new();
    let mut seps: u32 = 0;
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'l' | 'c' | 'r' => {
                if result.len() == AstArena::MAX_CHILDREN {
                    return Err(Error::LimitExceeded {
                        kind: crate::error::LimitKind::ListLength,
                        limit: AstArena::MAX_CHILDREN,
                    });
                }
                result.push(match c {
                    'l' => ColumnAlign::Left,
                    'c' => ColumnAlign::Center,
                    _ => ColumnAlign::Right,
                });
            },
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
                    let count_text = n_str.trim();
                    let n = match count_text.parse::<u64>() {
                        Ok(n) => n,
                        Err(_)
                            if !count_text.is_empty()
                                && count_text.bytes().all(|b| b.is_ascii_digit()) =>
                        {
                            return Err(Error::LimitExceeded {
                                kind: crate::error::LimitKind::ListLength,
                                limit: AstArena::MAX_CHILDREN,
                            });
                        },
                        // Keep the parser's existing recovery for a malformed
                        // non-numeric repeat count (`*{foo}{c}`).
                        Err(_) => 1,
                    };
                    if chars.peek() == Some(&'{') {
                        chars.next(); // consume '{'
                        let mut pattern = String::new();
                        let mut brace_depth = 1u32;
                        for ch in chars.by_ref() {
                            if ch == '{' {
                                brace_depth += 1;
                            }
                            if ch == '}' {
                                brace_depth -= 1;
                                if brace_depth == 0 {
                                    break;
                                }
                            }
                            pattern.push(ch);
                        }
                        let (sub, _sub_seps) = parse_column_spec_at_depth(&pattern, depth + 1)?;
                        // An empty pattern has no columns; skip the loop
                        // entirely so even a huge repetition count is O(1).
                        if sub.is_empty() {
                            continue;
                        }
                        let max_repetitions =
                            (AstArena::MAX_CHILDREN - result.len()) / sub.len();
                        if n > max_repetitions as u64 {
                            return Err(Error::LimitExceeded {
                                kind: crate::error::LimitKind::ListLength,
                                limit: AstArena::MAX_CHILDREN,
                            });
                        }
                        for _ in 0..n {
                            result.extend_from_slice(&sub);
                        }
                    }
                },
            _ => {}, // skip :, @{}, and other decorations
        }
    }
    Ok((result, seps))
}

/// True if `input` begins with a `\cr` row separator (not `\cross` etc.).
/// Used by table-cell parsing to stop a cell at `\cr`, mirroring the `\cr`
/// branch of [`parse_row_separator`].
fn is_cr_row_separator(input: &str) -> bool {
    input.starts_with("\\cr")
        && !input
            .as_bytes()
            .get(3)
            .is_some_and(|b| b.is_ascii_alphabetic())
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
            b'\\' if depth == 0 && strip_control_word(&input[i..], "end").is_some() => return i,
            _ => {},
        }
        i += 1;
    }
    input.len()
}

/// Split a diagram body into rows on top-level `\\` or `\cr` separators.
/// Escaped braces remain literal tokens, and xymatrix may additionally protect
/// row separators inside optional direction brackets.
fn split_diagram_rows(body: &str, track_brackets: bool) -> SmallVec<[&str; 4]> {
    let bytes = body.as_bytes();
    let mut rows: SmallVec<[&str; 4]> = SmallVec::new();
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => brace_depth += 1,
            b'}' if brace_depth > 0 => brace_depth -= 1,
            b'[' if track_brackets && brace_depth == 0 => bracket_depth += 1,
            b']' if track_brackets && brace_depth == 0 && bracket_depth > 0 => {
                bracket_depth -= 1;
            },
            b'\\' => {
                let top_level = brace_depth == 0 && (!track_brackets || bracket_depth == 0);
                if bytes
                    .get(i + 1)
                    .is_some_and(|b| matches!(b, b'{' | b'}' | b'[' | b']'))
                {
                    i += 2;
                    continue;
                }
                if top_level {
                    let is_double_backslash = bytes.get(i + 1) == Some(&b'\\');
                    let is_cr = is_cr_row_separator(&body[i..]);
                    if is_double_backslash || is_cr {
                        rows.push(&body[start..i]);
                        i += if is_double_backslash { 2 } else { 3 };
                        start = i;
                        continue;
                    }
                }
            },
            _ => {},
        }
        i += 1;
    }
    rows.push(&body[start..]);
    rows
}

/// Split a `CD` body. Connector syntax may use brackets as token content, so
/// only braces shield row separators here, preserving the historical contract.
fn cd_split_rows(body: &str) -> SmallVec<[&str; 4]> {
    split_diagram_rows(body, false)
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

/// Split an xymatrix body. Optional direction brackets shield their contents
/// from row splitting, while a standalone `\cr` remains a row separator.
fn xy_split_rows(body: &str) -> SmallVec<[&str; 4]> {
    split_diagram_rows(body, true)
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

/// Skip optional `\ar` shaft/curve/offset modifiers: `@{->}`, `@/^/`, `@(u,d)`,
/// `@<1ex>`, `@[white]`, or a bare style char (`@2`, `@.`, `@-`, …). The arrow
/// glyph/direction is still taken from the following `[dir]`.
fn xy_skip_ar_modifiers(input: &str) -> &str {
    let mut s = input;
    loop {
        let t = s.trim_start();
        let Some(after_at) = t.strip_prefix('@') else {
            return t;
        };
        let Some(first) = after_at.chars().next() else {
            return after_at;
        };
        s = match first {
            '{' => xy_skip_pair(after_at, '{', '}'),
            '(' => xy_skip_pair(after_at, '(', ')'),
            '<' => xy_skip_pair(after_at, '<', '>'),
            '[' => xy_skip_pair(after_at, '[', ']'),
            '/' => match after_at[1..].find('/') {
                Some(p) => &after_at[1 + p + 1..],
                None => "",
            },
            _ => &after_at[first.len_utf8()..],
        };
    }
}

/// Skip a balanced `open..close` group at the start of `s`.
fn xy_skip_pair(s: &str, open: char, close: char) -> &str {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return &s[i + c.len_utf8()..];
            }
        }
    }
    ""
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

fn expansion_error(failure: ExpansionFailure) -> Error {
    Error::from(match failure {
        ExpansionFailure::OutputBytes { limit } => LatexParseError::ExpansionLimit { limit },
        ExpansionFailure::Work { limit } => LatexParseError::ExpansionWorkLimit { limit },
        ExpansionFailure::Depth { limit } => LatexParseError::ExpansionDepthLimit { limit },
        ExpansionFailure::Arguments { command } => LatexParseError::MacroArguments {
            command: command.into(),
        },
        ExpansionFailure::Syntax { command, kind } => LatexParseError::MalformedDefinition {
            command: command.as_str().to_owned(),
            detail: kind.as_str().to_owned(),
        },
    })
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
    let trimmed = trim_latex_input(input);
    if trimmed.is_empty() {
        return Ok(CompactParseResult::new(CompactExpr::new(
            AstArena::new(),
            NodeSpan::EMPTY,
            config.display,
        )));
    }
    let (processed, warning) =
        match preprocess_cow(trimmed) {
            Ok(processed) => (processed, None),
            Err(
                failure @ (ExpansionFailure::Syntax { .. } | ExpansionFailure::Arguments { .. }),
            ) if config.strictness == Strictness::Lenient => (
                std::borrow::Cow::Borrowed(trimmed),
                Some(expansion_error(failure)),
            ),
            Err(failure) => return Err(expansion_error(failure)),
        };
    let mut parser = LatexCompactParser::with_capacity(processed.len(), config.max_nodes);
    if let Some(error) = warning {
        parser.warn(format!(
            "{error}; parsed the original source without macro expansion"
        ));
    }
    match parser.parse_node_list(&processed) {
        Ok(("", ids)) => {
            if let Some(error) = parser.abort.take() {
                return Err(error);
            }
            let ids = parser.resolve_infix_fraction(ids);
            let ids: SmallVec<[NodeId; 8]> = if parser.infix_marker_seen {
                ids.into_iter()
                    .map(|id| parser.cleanup_infix_markers(id))
                    .collect()
            } else {
                ids
            };
            parser.check_node_limit();
            if let Some(error) = parser.abort.take() {
                return Err(error);
            }
            parser.arena.check_limits()?;
            let roots = parser.arena.push_children(&ids)?;
            let diagnostics = std::mem::take(&mut parser.diagnostics);
            Ok(CompactParseResult::with_diagnostics(
                CompactExpr::new(parser.arena, roots, config.display),
                diagnostics,
            ))
        },
        Ok((rest, ids)) => {
            if let Some(error) = parser.abort.take() {
                return Err(error);
            }
            let start = processed.len().saturating_sub(rest.len());
            let end = processed.len();
            if config.strictness == Strictness::Strict {
                Err(Error::from(LatexParseError::UnexpectedToken {
                    token: rest.chars().take(20).collect(),
                    span: Some(Span::new(start, end)),
                }))
            } else {
                let mut diagnostics = vec![Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message: format!("ignored trailing input: {rest:?}"),
                    span: Some(Span::new(start, end)),
                }];
                let mut result_ids = ids;
                let recovered = parser.parse_lenient_remainder(rest, &mut diagnostics);
                if let Some(error) = parser.abort.take() {
                    return Err(error);
                }
                result_ids.extend(recovered);
                let result_ids = parser.resolve_infix_fraction(result_ids);
                let result_ids: SmallVec<[NodeId; 8]> = if parser.infix_marker_seen {
                    result_ids
                        .into_iter()
                        .map(|id| parser.cleanup_infix_markers(id))
                        .collect()
                } else {
                    result_ids
                };
                parser.check_node_limit();
                if let Some(error) = parser.abort.take() {
                    return Err(error);
                }
                parser.arena.check_limits()?;
                let roots = parser.arena.push_children(&result_ids)?;
                diagnostics.extend(std::mem::take(&mut parser.diagnostics));
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, roots, config.display),
                    diagnostics,
                ))
            }
        },
        Err(ref e) => {
            if let Some(error) = parser.abort.take() {
                return Err(error);
            }
            if config.strictness == Strictness::Lenient {
                let mut diagnostics = Vec::new();
                let ids = parser.parse_lenient_remainder(&processed, &mut diagnostics);
                if let Some(error) = parser.abort.take() {
                    return Err(error);
                }
                diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message: format!("parse error (recovered): {e}"),
                    span: None,
                });
                let ids: SmallVec<[NodeId; 8]> = if parser.infix_marker_seen {
                    ids.into_iter()
                        .map(|id| parser.cleanup_infix_markers(id))
                        .collect()
                } else {
                    ids
                };
                parser.check_node_limit();
                if let Some(error) = parser.abort.take() {
                    return Err(error);
                }
                parser.arena.check_limits()?;
                let roots = parser.arena.push_children(&ids)?;
                diagnostics.extend(std::mem::take(&mut parser.diagnostics));
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, roots, config.display),
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
    /// A typed resource error raised by a parser helper that communicates
    /// through nom's error channel. Resource failures must remain fatal in
    /// lenient mode; recovering by treating the input as text would defeat
    /// the configured bound.
    abort: Option<Error>,
    /// Maximum number of nodes created while parsing, if configured.
    max_nodes: Option<usize>,
    /// User-defined colors registered via `\definecolor`/`\colorlet`,
    /// keyed by exact name, resolved to RGB bytes.
    colors: HashMap<String, [u8; 3]>,
    /// Warnings accumulated during parsing (e.g. metadata-dropping macros such
    /// as `\href`/`\includegraphics`), merged into the `ParseResult`.
    diagnostics: Vec<Diagnostic>,
    /// Active `\rowcolors{start}{odd}{even}` alternating-row coloring (xcolor
    /// `[table]`), captured for the next `array`/`tabular`. Cleared once that
    /// array consumes it.
    rowcolors: Option<(usize, String, String)>,
    /// User paired delimiters declared via `\DeclarePairedDelimiter`, mapping
    /// the command name to its `(open, close)` delimiter glyph strings.
    paired_delims: HashMap<String, (String, String)>,
    /// The last atom returned by an unstarred `\operatorname`. Placement
    /// modifiers after their scripts are accepted by KaTeX but deliberately
    /// remain side-script metadata.
    last_unstarred_operator: Option<NodeId>,
    /// Set to `true` the first time an infix-fraction marker (`\over`/`\atop`/
    /// `\choose`/`\brace`/`\brack`/`\above`) is produced. The vast majority of
    /// inputs contain none, so `check_infix_fraction` can skip its O(n) scan of
    /// every braced group unless this flag is set.
    infix_marker_seen: bool,
    /// Set by `parse_element` to the script it parsed when that script had no
    /// base of its own (`^`, `_` or `'` after white space), which TeX attaches to the
    /// preceding atom.
    implicit_base_script: Option<NodeId>,
    /// Whether the current old-style font declaration began in the top-level
    /// expression. Such scopes retain Mathema's existing hard source-newline
    /// behavior while nested group parsers continue to treat newlines as
    /// ordinary whitespace.
    font_scope_preserve_breaks: bool,
    /// A middle delimiter can remain nested in a fraction within this fence.
    in_left_right: bool,
    /// Used to identify the rare font argument that opened an alignment.
    arrays_built: usize,
}

impl LatexCompactParser {
    /// Create a parser with pre-allocated arena capacity based on input length.
    ///
    /// Heuristic: a typical LaTeX formula produces roughly 1 node per 3 input bytes,
    /// with similar proportions for children and strings.
    fn with_capacity(input_len: usize, max_nodes: Option<usize>) -> Self {
        let est_nodes = input_len / 3 + 4;
        let est_nodes = max_nodes.map_or(est_nodes, |limit| est_nodes.min(limit));
        let est_children = est_nodes;
        let est_strings = input_len;
        Self {
            arena: AstArena::with_capacity(est_nodes, est_children, est_strings),
            depth: 0,
            abort: None,
            max_nodes,
            colors: HashMap::new(),
            diagnostics: Vec::new(),
            rowcolors: None,
            paired_delims: HashMap::new(),
            last_unstarred_operator: None,
            infix_marker_seen: false,
            implicit_base_script: None,
            font_scope_preserve_breaks: false,
            in_left_right: false,
            arrays_built: 0,
        }
    }

    /// Record a non-fatal parse warning (e.g. a metadata-dropping macro).
    fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: message.into(),
            span: None,
        });
    }

    /// Turn an arena node-budget hit into a typed parser error. The parser
    /// keeps the last node temporarily so every in-flight ID remains valid,
    /// then unwinds before any partial tree can be returned to the caller.
    #[inline(never)]
    fn check_node_limit(&mut self) -> bool {
        if let Some(limit) = self.max_nodes
            && self.arena.node_count() > limit
            && self.abort.is_none()
        {
            self.abort = Some(Error::LimitExceeded {
                kind: crate::error::LimitKind::Nodes,
                limit,
            });
        }
        self.abort.is_some()
    }

    fn ensure_node_budget<'a>(
        &mut self,
        input: &'a str,
    ) -> std::result::Result<(), nom::Err<nom::error::Error<&'a str>>> {
        if self.check_node_limit() {
            Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )))
        } else {
            Ok(())
        }
    }

    #[inline]
    fn take_resource_error(&mut self) -> Option<Error> {
        self.check_node_limit();
        self.abort.take()
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
            let span = self.arena.push_children_deferred(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    /// Check whether a parsed node list contains an infix fraction marker
    /// (`\over`, `\atop`, `\choose`, `\brace`, `\brack`). If found, split
    /// into numerator/denominator and produce the appropriate node.
    fn check_infix_fraction(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        // Fast path: if no infix-fraction marker was ever produced during this
        // parse, there is nothing to find — skip the O(n) scan entirely. Infix
        // operators (`\over`/`\atop`/…) are rare, so this short-circuits the
        // common case (every braced group + the top-level list would otherwise
        // be scanned).
        if !self.infix_marker_seen {
            return None;
        }
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

        // Recursively resolve further infix markers in each side so chained
        // operators (e.g. `1 \over 2 \over 3`) never leak a `__infix_*__`
        // sentinel into the numerator/denominator.
        let num_ids = self.resolve_infix_fraction(num_ids);
        let den_ids = self.resolve_infix_fraction(den_ids);

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
                let content = self.arena.push_children_deferred(&[frac_id]);
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
                let content = self.arena.push_children_deferred(&[frac_id]);
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
                let content = self.arena.push_children_deferred(&[frac_id]);
                Some(self.arena.push_node(CompactNode::Fenced {
                    open,
                    close,
                    content,
                }))
            },
            _ => None,
        }
    }

    /// A font declaration can produce multiple styled spans separated by an
    /// infix marker. Real groups resolve their own fractions before returning,
    /// so only the immediate row needs to be spliced into its enclosing list.
    fn flatten_infix_marker_rows(&self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        if !self.infix_marker_seen {
            return ids;
        }
        let mut result = SmallVec::new();
        for id in ids {
            if let CompactNode::Row(span) = *self.arena.node(id) {
                let children = self.arena.children(span);
                if children.iter().any(|&child| self.is_infix_marker_id(child)) {
                    result.extend_from_slice(children);
                    continue;
                }
            }
            result.push(id);
        }
        result
    }

    /// Resolve a top-level/group infix fraction operator into the corresponding node.
    fn resolve_infix_fraction(&mut self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        let ids = self.flatten_infix_marker_rows(ids);
        match self.check_infix_fraction(&ids) {
            Some(node) => {
                let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
                out.push(node);
                out
            },
            None => ids,
        }
    }

    /// True if `id` is an infix-fraction sentinel identifier (`__infix_*__`).
    fn is_infix_marker_id(&self, id: NodeId) -> bool {
        if let CompactNode::Identifier(sid) = *self.arena.node(id) {
            let s = self.arena.str_val(sid);
            s.starts_with("__infix_") && s.ends_with("__")
        } else {
            false
        }
    }

    /// Replace a stray infix-fraction sentinel with an empty Row so it can be
    /// used as a script base or argument without leaking the internal marker
    /// string into output.
    fn resolve_stray_infix_marker(&mut self, id: NodeId) -> NodeId {
        if self.is_infix_marker_id(id) {
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
        } else {
            id
        }
    }

    /// Recursively resolve any infix-fraction sentinels that leaked out of a
    /// top-level/braced context (for example inside `\left...\right` fences or
    /// nested rows).  Any remaining stray marker that is not part of a resolvable
    /// fraction is replaced by an empty Row.
    fn cleanup_infix_markers(&mut self, id: NodeId) -> NodeId {
        if id == NodeId::NONE {
            return id;
        }
        let new_id = self.cleanup_infix_markers_in_node(id);
        if self.is_infix_marker_id(new_id) {
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
        } else {
            new_id
        }
    }

    fn cleanup_infix_markers_in_node(&mut self, id: NodeId) -> NodeId {
        let node = *self.arena.node(id);

        let has_marker =
            |this: &Self, ids: &[NodeId]| ids.iter().any(|&c| this.is_infix_marker_id(c));

        let clean_list = |this: &mut Self, ids: &[NodeId]| {
            let mut changed = false;
            let mut out: SmallVec<[NodeId; 8]> = SmallVec::with_capacity(ids.len());
            for &child in ids {
                let cleaned = this.cleanup_infix_markers(child);
                changed |= cleaned != child;
                out.push(cleaned);
            }
            (out, changed)
        };

        match node {
            CompactNode::Row(span) => {
                let children: Vec<NodeId> = self.arena.children(span).to_vec();
                let (cleaned, changed) = clean_list(self, &children);
                if has_marker(self, &cleaned) {
                    let resolved = self.resolve_infix_fraction(cleaned);
                    debug_assert_eq!(resolved.len(), 1);
                    resolved[0]
                } else if changed {
                    let new_span = self.arena.push_children_deferred(&cleaned);
                    self.arena.push_node(CompactNode::Row(new_span))
                } else {
                    id
                }
            },
            CompactNode::Fenced {
                open,
                close,
                content,
            } => {
                let children: Vec<NodeId> = self.arena.children(content).to_vec();
                let (cleaned, changed) = clean_list(self, &children);
                let new_content = if has_marker(self, &cleaned) {
                    let resolved = self.resolve_infix_fraction(cleaned);
                    debug_assert_eq!(resolved.len(), 1);
                    self.arena.push_children_deferred(&resolved)
                } else if changed {
                    self.arena.push_children_deferred(&cleaned)
                } else {
                    content
                };
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::Fenced {
                        open,
                        close,
                        content: new_content,
                    })
                }
            },
            CompactNode::Fraction { kind, num, den } => {
                let new_num = self.cleanup_infix_markers(num);
                let new_den = self.cleanup_infix_markers(den);
                if new_num == num && new_den == den {
                    id
                } else {
                    self.arena.push_node(CompactNode::Fraction {
                        kind,
                        num: new_num,
                        den: new_den,
                    })
                }
            },
            CompactNode::Subscript { base, sub } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_sub = self.cleanup_infix_markers(sub);
                if new_base == base && new_sub == sub {
                    id
                } else {
                    self.arena.push_node(CompactNode::Subscript {
                        base: new_base,
                        sub: new_sub,
                    })
                }
            },
            CompactNode::Superscript { base, sup } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_sup = self.cleanup_infix_markers(sup);
                if new_base == base && new_sup == sup {
                    id
                } else {
                    self.arena.push_node(CompactNode::Superscript {
                        base: new_base,
                        sup: new_sup,
                    })
                }
            },
            CompactNode::SubSup { base, sub, sup } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_sub = self.cleanup_infix_markers(sub);
                let new_sup = self.cleanup_infix_markers(sup);
                if new_base == base && new_sub == sub && new_sup == sup {
                    id
                } else {
                    self.arena.push_node(CompactNode::SubSup {
                        base: new_base,
                        sub: new_sub,
                        sup: new_sup,
                    })
                }
            },
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_pre_sub = self.cleanup_infix_markers(pre_sub);
                let new_pre_sup = self.cleanup_infix_markers(pre_sup);
                if new_base == base && new_pre_sub == pre_sub && new_pre_sup == pre_sup {
                    id
                } else {
                    self.arena.push_node(CompactNode::PreScripts {
                        base: new_base,
                        pre_sub: new_pre_sub,
                        pre_sup: new_pre_sup,
                    })
                }
            },
            CompactNode::Overscript {
                base,
                over,
                movable_limits,
            } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_over = self.cleanup_infix_markers(over);
                if new_base == base && new_over == over {
                    id
                } else {
                    self.arena.push_node(CompactNode::Overscript {
                        base: new_base,
                        over: new_over,
                        movable_limits,
                    })
                }
            },
            CompactNode::Underscript {
                base,
                under,
                movable_limits,
            } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_under = self.cleanup_infix_markers(under);
                if new_base == base && new_under == under {
                    id
                } else {
                    self.arena.push_node(CompactNode::Underscript {
                        base: new_base,
                        under: new_under,
                        movable_limits,
                    })
                }
            },
            CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits,
            } => {
                let new_base = self.cleanup_infix_markers(base);
                let new_under = self.cleanup_infix_markers(under);
                let new_over = self.cleanup_infix_markers(over);
                if new_base == base && new_under == under && new_over == over {
                    id
                } else {
                    self.arena.push_node(CompactNode::UnderOver {
                        base: new_base,
                        under: new_under,
                        over: new_over,
                        movable_limits,
                    })
                }
            },
            CompactNode::Sqrt(inner) => {
                let new_inner = self.cleanup_infix_markers(inner);
                if new_inner == inner {
                    id
                } else {
                    self.arena.push_node(CompactNode::Sqrt(new_inner))
                }
            },
            CompactNode::Root { index, radicand } => {
                let new_index = self.cleanup_infix_markers(index);
                let new_radicand = self.cleanup_infix_markers(radicand);
                if new_index == index && new_radicand == radicand {
                    id
                } else {
                    self.arena.push_node(CompactNode::Root {
                        index: new_index,
                        radicand: new_radicand,
                    })
                }
            },
            CompactNode::Nary {
                op,
                sub,
                sup,
                body,
                limits,
            } => {
                let new_sub = self.cleanup_infix_markers(sub);
                let new_sup = self.cleanup_infix_markers(sup);
                let new_body = self.cleanup_infix_markers(body);
                if new_sub == sub && new_sup == sup && new_body == body {
                    id
                } else {
                    self.arena.push_node(CompactNode::Nary {
                        op,
                        sub: new_sub,
                        sup: new_sup,
                        body: new_body,
                        limits,
                    })
                }
            },
            CompactNode::Accent {
                base,
                accent,
                position,
                wide,
            } => {
                let new_base = self.cleanup_infix_markers(base);
                if new_base == base {
                    id
                } else {
                    self.arena.push_node(CompactNode::Accent {
                        base: new_base,
                        accent,
                        position,
                        wide,
                    })
                }
            },
            CompactNode::Array {
                col_aligns,
                cells,
                num_cols,
                arraystretch_pct,
                add_jot,
                array_style,
                col_seps,
                row_seps,
            } => {
                let cell_ids: Vec<NodeId> = self.arena.children(cells).to_vec();
                let (cleaned, changed) = clean_list(self, &cell_ids);
                if changed {
                    let new_cells = self.arena.push_children_deferred(&cleaned);
                    self.arena.push_node(CompactNode::Array {
                        col_aligns,
                        cells: new_cells,
                        num_cols,
                        arraystretch_pct,
                        add_jot,
                        array_style,
                        col_seps,
                        row_seps,
                    })
                } else {
                    id
                }
            },
            CompactNode::Phantom(inner, kind) => {
                let new_inner = self.cleanup_infix_markers(inner);
                if new_inner == inner {
                    id
                } else {
                    self.arena.push_node(CompactNode::Phantom(new_inner, kind))
                }
            },
            CompactNode::Boxed {
                content,
                bg_color,
                border_color,
            } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::Boxed {
                        content: new_content,
                        bg_color,
                        border_color,
                    })
                }
            },
            CompactNode::Cancel { stroke, content } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::Cancel {
                        stroke,
                        content: new_content,
                    })
                }
            },
            CompactNode::Scaled { factor, content } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::Scaled {
                        factor,
                        content: new_content,
                    })
                }
            },
            CompactNode::RaiseBox {
                shift,
                height,
                depth,
                extents,
                content,
            } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::RaiseBox {
                        shift,
                        height,
                        depth,
                        extents,
                        content: new_content,
                    })
                }
            },
            CompactNode::Styled(style, span) => {
                let children: Vec<NodeId> = self.arena.children(span).to_vec();
                let (cleaned, changed) = clean_list(self, &children);
                if changed {
                    let new_span = self.arena.push_children_deferred(&cleaned);
                    self.arena.push_node(CompactNode::Styled(style, new_span))
                } else {
                    id
                }
            },
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => {
                let new_display = self.cleanup_infix_markers(display);
                let new_text = self.cleanup_infix_markers(text);
                let new_script = self.cleanup_infix_markers(script);
                let new_script_script = self.cleanup_infix_markers(script_script);
                if new_display == display
                    && new_text == text
                    && new_script == script
                    && new_script_script == script_script
                {
                    id
                } else {
                    self.arena.push_node(CompactNode::MathChoice {
                        display: new_display,
                        text: new_text,
                        script: new_script,
                        script_script: new_script_script,
                    })
                }
            },
            CompactNode::Color { color, content } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::Color {
                        color,
                        content: new_content,
                    })
                }
            },
            CompactNode::MathAtom { atom_type, content } => {
                let new_content = self.cleanup_infix_markers(content);
                if new_content == content {
                    id
                } else {
                    self.arena.push_node(CompactNode::MathAtom {
                        atom_type,
                        content: new_content,
                    })
                }
            },
            // Leaves and helper nodes never contain children.
            _ => id,
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
        self.build_array_with_spacing(columns, rows, 100, false, 0, 0, false)
    }

    fn build_array_with_seps_rows(
        &mut self,
        columns: &[ColumnAlign],
        col_seps: u32,
        row_seps: u32,
        array_style: bool,
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
    ) -> NodeId {
        self.build_array_with_spacing(columns, rows, 100, false, col_seps, row_seps, array_style)
    }

    /// Record a table environment that overrides the default text style of its cells.
    ///
    /// TeX table environments form a fresh math list for each cell.  The
    /// declaration is therefore local to the cell and must be represented in
    /// the AST, instead of being inferred later from row spacing or alignment
    /// metadata.  A leading `StyleSwitch` also lets an explicit declaration in
    /// the cell override the environment default in the usual style-run logic.
    fn apply_table_cell_style(&mut self, rows: &mut [Vec<SmallVec<[NodeId; 8]>>], style: u8) {
        let marker = self.arena.push_node(CompactNode::StyleSwitch(style));
        for row in rows {
            for cell in row {
                cell.insert(0, marker);
            }
        }
    }

    /// Build an Array node with explicit spacing parameters.
    #[allow(clippy::too_many_arguments)]
    fn build_array_with_spacing(
        &mut self,
        columns: &[ColumnAlign],
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
        arraystretch_pct: u8,
        add_jot: bool,
        col_seps: u32,
        row_seps: u32,
        array_style: bool,
    ) -> NodeId {
        // Compute the actual number of columns as the maximum of the declared
        // column spec and the widest row.  This ensures the flat cell array
        // stride is correct even when rows contain more `&` separators than
        // the column spec declares (e.g. multi-pair `aligned` environments).
        let max_row_len = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let num_cols = columns.len().max(max_row_len).max(1) as u16;

        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for &col in columns {
            if self.check_node_limit() {
                break;
            }
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(col)));
        }
        let col_aligns = self.arena.push_children_deferred(&align_ids);

        let mut cell_ids: SmallVec<[NodeId; 16]> = SmallVec::new();
        for row in &rows {
            if self.check_node_limit() {
                break;
            }
            for cell in row {
                if self.check_node_limit() {
                    break;
                }
                let span = self.arena.push_children_deferred(cell);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
            // Pad short rows to num_cols
            for _ in row.len()..(num_cols as usize) {
                if self.check_node_limit() {
                    break;
                }
                cell_ids.push(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)));
            }
        }
        let cells = self.arena.push_children_deferred(&cell_ids);
        self.arrays_built += 1;
        self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            col_seps,
            row_seps,
            array_style,
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
                    '%' | '$' | '#' | '&' | '_' | '^' | '~' => Ok((rest2, self.push_char_str(c2))),
                    '{' => Ok((rest2, self.arena.push_str("{"))),
                    '}' => Ok((rest2, self.arena.push_str("}"))),
                    '|' => Ok((rest2, self.arena.push_str("\u{2016}"))),
                    _ if c2.is_ascii_alphabetic() => {
                        let end = rest
                            .find(|ch: char| !ch.is_ascii_alphabetic())
                            .unwrap_or(rest.len());
                        let name = &rest[..end];
                        let after = &rest[end..];
                        // A backslash-delimited delimiter must name a known
                        // math symbol (e.g. \langle, \Vert). Unknown command
                        // names are not valid delimiters and would produce
                        // unparseable output when written back.
                        let ch = if name == "backslash" {
                            Some('\\')
                        } else {
                            latex_to_unicode(name)
                        }
                        .ok_or_else(|| {
                            nom::Err::Error(nom::error::Error::new(
                                input,
                                nom::error::ErrorKind::Char,
                            ))
                        })?;
                        let ch = match ch {
                            '<' => '\u{27E8}',
                            '>' => '\u{27E9}',
                            other => other,
                        };
                        let sid = self.push_char_str(ch);
                        Ok((after, sid))
                    },
                    _ => Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Char,
                    ))),
                }
            },
            // Any remaining non-whitespace, non-alphanumeric character that is
            // not a backslash or brace is treated as a literal delimiter string.
            // This keeps round-trips stable for delimiters produced by macros
            // such as \genfrac, which may place arbitrary math symbols (e.g. the
            // prime \u{2032}) or punctuation (e.g. ';') in the delimiter position.
            c if !c.is_ascii_alphanumeric()
                && !c.is_whitespace()
                && !matches!(c, '\\' | '{' | '}') =>
            {
                Ok((rest, self.push_char_str(c)))
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
            if self.check_node_limit() {
                return Err(nom::Err::Failure(nom::error::Error::new(
                    rest,
                    nom::error::ErrorKind::TooLarge,
                )));
            }
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
                    self.push_element(&mut ids, id);
                    self.ensure_node_budget(r)?;
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        Ok((rest, ids))
    }

    /// Append a parsed element. A script with no base of its own belongs to
    /// the preceding atom, as in TeX white space does not separate them; an
    /// explicit empty group (`{}^2 L`) stays a base of its own.
    fn push_element(&mut self, ids: &mut SmallVec<[NodeId; 8]>, id: NodeId) {
        let implicit = self.implicit_base_script.take() == Some(id)
            && matches!(
                *self.arena.node(id),
                CompactNode::Subscript { .. }
                    | CompactNode::Superscript { .. }
                    | CompactNode::SubSup { .. }
            );
        if implicit
            && let Some(&prev) = ids.last()
            && crate::ast::normalize::can_carry_script(*self.arena.node(prev))
        {
            ids.pop();
            ids.push(crate::ast::normalize::make_script_with_base(
                &mut self.arena,
                prev,
                id,
            ));
            return;
        }
        ids.push(id);
    }

    /// Parse one element: an atom optionally followed by scripts.
    #[inline]
    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if self.check_node_limit() {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        // Handle leading ^/_ /' with implicit empty base (e.g., {^3+1})
        if input.starts_with('^') || input.starts_with('_') || input.starts_with('\'') {
            let base = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            let result = self.parse_scripts(input, base);
            if let Ok((_, id)) = result {
                self.implicit_base_script = Some(id);
            }
            return result;
        }
        let grouped = input.starts_with('{');
        let (input, base) = self.parse_atom(input)?;
        if grouped
            && matches!(
                *self.arena.node(base),
                CompactNode::Operator { .. }
                    | CompactNode::MathAtom {
                        atom_type: MathAtomType::Op,
                        ..
                    }
            )
            && ScriptMarker::at(&input[self.skip_script_ws(input.as_bytes())..]).is_some()
        {
            // Braces make a TeX ordinary atom. Preserve that boundary when
            // flattening a singleton operator group would move its scripts.
            let ordinary = self.arena.push_node(CompactNode::MathAtom {
                atom_type: MathAtomType::Ord,
                content: base,
            });
            return self.parse_scripts(input, ordinary);
        }

        // N-ary operators consume scripts + body specially
        if self.is_nary_id(base) {
            return self.parse_nary_rest(input, base);
        }

        // Named operators retain ordinary script nodes; their Operator.limits
        // field carries the effective modifier for writers/renderers.
        if let CompactNode::Operator { name, limits } = *self.arena.node(base) {
            let unstarred_operator = self.last_unstarred_operator == Some(base);
            let (rest, ((sub, sup), modifier)) =
                self.parse_script_fields_with_limits(input, false)?;
            // An unstarred custom name ignores placement controls. Only
            // allocate another operator when its actual placement changes.
            let operator = if let Some(new_limits) =
                modifier.filter(|&v| !unstarred_operator && v != limits)
            {
                self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: new_limits,
                })
            } else {
                base
            };
            return Ok((rest, self.make_script_node(operator, sub, sup)));
        }

        // Compound `\mathop` bases need an under/over wrapper because the
        // existing MathAtom node has no limit-style field.
        if let Some(default_style) = self.operator_limit_style(base) {
            let (rest, ((sub, sup), modifier)) =
                self.parse_script_fields_with_limits(input, false)?;
            let style = modifier.unwrap_or(default_style);
            let scripted = self.make_script_node(base, sub, sup);
            return Ok((rest, self.apply_limit_style(scripted, style)));
        }

        let (rest, scripted) = self.parse_scripts(input, base)?;
        // Placement controls are only meaningful after an operator. Keep the
        // permissive empty command for standalone package input, but reject it
        // when it follows an ordinary scripted atom instead of silently
        // turning the control into an empty row.
        if starts_limit_modifier(rest) && consume_limit_modifiers(rest).1.is_some() {
            return Err(nom::Err::Error(nom::error::Error::new(
                rest,
                nom::error::ErrorKind::Tag,
            )));
        }
        Ok((rest, scripted))
    }

    /// Parse one KaTeX-compatible Unicode script run. KaTeX stops a run when
    /// the subscript/superscript kind changes, then lets the normal script
    /// loop consume the complementary kind. This keeps `x₂³₄` from silently
    /// merging both subscripts into one row.
    fn parse_unicode_script_arg_run<'a>(
        &mut self,
        input: &'a str,
        is_sub: bool,
    ) -> IResult<&'a str, NodeId> {
        let mut glyphs = input
            .char_indices()
            .map_while(|(offset, scalar)| {
                let (sub, glyph) = decode_script_scalar(scalar)?;
                (sub == is_sub).then_some((offset + scalar.len_utf8(), glyph))
            })
            .fuse()
            .peekable();
        let mut ids = SmallVec::<[NodeId; 4]>::new();
        let mut consumed = 0;
        while let Some((end, glyph)) = glyphs.next() {
            consumed = end;
            let id = if glyph.is_ascii_digit() {
                // Store a contiguous number as one atom, just like ASCII
                // input. Each digit is already a single decoded ASCII byte.
                let mut digits = SmallVec::<[u8; 16]>::new();
                digits.push(glyph as u8);
                while let Some((end, digit)) = glyphs.next_if(|(_, c)| c.is_ascii_digit()) {
                    digits.push(digit as u8);
                    consumed = end;
                }
                let text = std::str::from_utf8(&digits).expect("script digits are ASCII");
                let sid = self.arena.push_str(text);
                self.arena.push_node(CompactNode::Number(sid))
            } else if glyph.is_alphabetic() {
                let sid = self.push_char_str(glyph);
                self.arena.push_node(CompactNode::Identifier(sid))
            } else {
                self.push_ascii_math_symbol(glyph)
            };
            ids.push(id);
            self.ensure_node_budget(&input[consumed..])?;
        }
        Ok((&input[consumed..], self.flatten_single(&ids)))
    }

    fn operator_limit_style(&self, id: NodeId) -> Option<LimitStyle> {
        match *self.arena.node(id) {
            CompactNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => Some(LimitStyle::Default),
            _ => None,
        }
    }

    fn make_script_node(&mut self, base: NodeId, sub: NodeId, sup: NodeId) -> NodeId {
        match (sub.is_some(), sup.is_some()) {
            (true, true) => self.arena.push_node(CompactNode::SubSup { base, sub, sup }),
            (true, false) => self.arena.push_node(CompactNode::Subscript { base, sub }),
            (false, true) => self.arena.push_node(CompactNode::Superscript { base, sup }),
            (false, false) => base,
        }
    }

    fn apply_limit_style(&mut self, id: NodeId, style: LimitStyle) -> NodeId {
        match style {
            LimitStyle::SubSup => id,
            LimitStyle::AboveBelow | LimitStyle::Default => {
                let movable_limits = style == LimitStyle::Default;
                match *self.arena.node(id) {
                    CompactNode::Subscript { base, sub } => {
                        self.arena.push_node(CompactNode::Underscript {
                            base,
                            under: sub,
                            movable_limits,
                        })
                    },
                    CompactNode::Superscript { base, sup } => {
                        self.arena.push_node(CompactNode::Overscript {
                            base,
                            over: sup,
                            movable_limits,
                        })
                    },
                    CompactNode::SubSup { base, sub, sup } => {
                        self.arena.push_node(CompactNode::UnderOver {
                            base,
                            under: sub,
                            over: sup,
                            movable_limits,
                        })
                    },
                    _ => id,
                }
            },
        }
    }

    #[inline]
    fn is_empty_row(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if self.arena.children(span).is_empty())
    }

    /// Attach the current math atom's scripts. Braces or a new atom start
    /// another list of postfix fields.
    #[inline]
    fn parse_scripts<'a>(&mut self, input: &'a str, base: NodeId) -> IResult<&'a str, NodeId> {
        let start = &input[self.skip_script_ws(input.as_bytes())..];
        if ScriptMarker::at(start).is_none() {
            return Ok((start, base));
        };
        let base = self.resolve_stray_infix_marker(base);
        let (rest, (sub, sup)) = self.script_fields(start, false, self.is_empty_row(base), None)?;
        let node = match (sub.is_some(), sup.is_some()) {
            (true, true) => CompactNode::SubSup { base, sub, sup },
            (true, false) => CompactNode::Subscript { base, sub },
            (false, true) => CompactNode::Superscript { base, sup },
            (false, false) => return Ok((rest, base)),
        };
        Ok((rest, self.arena.push_node(node)))
    }

    /// Parse a sequence of scripts with `\limits`/`\nolimits` controls that
    /// may occur before, between, or after the script fields. The last
    /// placement modifier wins, matching TeX's scanner.
    fn parse_script_fields_with_limits<'a>(
        &mut self,
        input: &'a str,
        keep_empty_limits: bool,
    ) -> IResult<&'a str, ((NodeId, NodeId), Option<LimitStyle>)> {
        let mut placement = None;
        let (rest, fields) =
            self.script_fields(input, keep_empty_limits, false, Some(&mut placement))?;
        Ok((rest, (fields, placement)))
    }

    /// Collect all postfix fields of one base. Operators additionally accept
    /// placement controls between fields, with the final control taking effect.
    fn script_fields<'a>(
        &mut self,
        mut input: &'a str,
        keep_empty_limits: bool,
        implicit_base: bool,
        mut placement: Option<&mut Option<LimitStyle>>,
    ) -> IResult<&'a str, (NodeId, NodeId)> {
        let (mut sub, mut sup) = (NodeId::NONE, NodeId::NONE);
        let mut saw_unicode = false;
        loop {
            let next = &input[self.skip_script_ws(input.as_bytes())..];
            if next.starts_with('\\')
                && let Some(current) = placement.as_deref_mut()
            {
                let (rest, modifier) = consume_limit_modifiers(next);
                if let Some(modifier) = modifier {
                    *current = Some(modifier);
                    input = rest;
                    continue;
                }
            }
            let Some(marker) = ScriptMarker::at(next) else {
                break;
            };
            let is_sub = marker.is_subscript();
            let slot = if is_sub { &mut sub } else { &mut sup };
            if slot.is_some() && !marker.is_unicode() {
                // Older Mathema expressions use a detached empty-base script
                // after whitespace, a bare limit operator, or an invisible
                // formatting atom. Preserve those distinct atoms on reimport.
                if !saw_unicode
                    && (implicit_base
                        || input.len() != next.len()
                        || marker == ScriptMarker::Primes
                        || crate::ast::normalize::is_effectively_empty_node(&self.arena, *slot)
                        || crate::ast::normalize::takes_limits(
                            &self.arena,
                            *self.arena.node(*slot),
                        ))
                {
                    break;
                }
                return Err(nom::Err::Error(nom::error::Error::new(
                    next,
                    nom::error::ErrorKind::Verify,
                )));
            }
            saw_unicode |= marker.is_unicode();
            let (rest, arg) = match marker {
                ScriptMarker::UnicodeSub | ScriptMarker::UnicodeSup => {
                    self.parse_unicode_script_arg_run(next, is_sub)?
                },
                ScriptMarker::Sub | ScriptMarker::Sup => self.parse_script_arg(&next[1..])?,
                ScriptMarker::Primes => self.parse_prime_argument(next)?,
            };
            // KaTeX assigns the newest contiguous Unicode run to its slot.
            // Operator templates retain explicitly empty script arguments.
            if keep_empty_limits || !self.is_empty_row(arg) {
                *slot = arg;
            }
            input = rest;
        }
        Ok((input, (sub, sup)))
    }

    fn parse_prime_argument<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let count = input.bytes().take_while(|&b| b == b'\'').count();
        let rest = &input[count..];
        let mut ids = SmallVec::<[NodeId; 4]>::new();
        for _ in 0..count {
            self.ensure_node_budget(rest)?;
            let sid = self.arena.push_str("\u{2032}");
            ids.push(
                self.arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
            );
        }
        let next = &rest[self.skip_script_ws(rest.as_bytes())..];
        let rest = if let Some(arg) = next.strip_prefix('^') {
            let (rest, extra) = self.parse_script_arg(arg)?;
            ids.push(extra);
            rest
        } else {
            rest
        };
        Ok((rest, self.flatten_single(&ids)))
    }

    #[inline]
    fn skip_script_ws(&self, input: &[u8]) -> usize {
        if self.depth == 1 || (self.font_scope_preserve_breaks && self.depth <= 2) {
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

        // TeX accepts placement modifiers before, between, and after script
        // fields. The last modifier wins.
        let (input, ((sub_id, sup_id), modifier)) =
            self.parse_script_fields_with_limits(input, true)?;
        let limit_style = modifier.unwrap_or(LimitStyle::Default);
        let (input, _) = ws(input)?;

        // The body is the next element, or an empty row at a grammar boundary.
        // Use `parse_element` (not bare `parse_atom`) so a nested n-ary operator
        // body — e.g. the 2nd integral in `\int\limits.. \int\limits.. f` — is
        // itself routed through `parse_nary_rest` and keeps its \limits/limits.
        //
        // Inside a `\left...\right` fence the body must not consume the closing
        // `\right`; treat it as an empty body so the fence parser can terminate.
        let (input, body_id) = if is_nary_body_boundary(input) {
            let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            (input, row_id)
        } else {
            self.parse_element(input)?
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
        self.script_fields(input, true, true, None)
    }

    /// Parse a single script argument: either a single token or a braced group.
    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (mut input, _) = ws(input)?;
        let mut skipped_relax = false;
        while let Some(rest) = strip_control_word(input, "relax") {
            skipped_relax = true;
            let (rest, _) = ws(rest)?;
            input = rest;
        }
        if skipped_relax && script_argument_boundary(input) {
            return Ok((
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            ));
        }
        if legacy_font_declaration(input).is_some() {
            // A legacy font declaration is a scope command, not an argument.
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        let argument_start = input;
        let (input, arg) = self.parse_atom(input)?;
        if matches!(self.arena.node(arg), CompactNode::StyleSwitch(_)) {
            // TeX requires a braced math list around a style declaration in
            // a subscript or superscript. A braced empty scope is a Row.
            return Err(nom::Err::Error(nom::error::Error::new(
                argument_start,
                nom::error::ErrorKind::Tag,
            )));
        }
        if self.check_node_limit() {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        // A bare infix-fraction primitive (`\over`, `\atop`, ...) cannot be a
        // valid script argument on its own. Treat it as empty so the script
        // marker is omitted and the `__infix_*__` sentinel never leaks into
        // output.
        Ok((input, self.resolve_stray_infix_marker(arg)))
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
        if self.check_node_limit() {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
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
            // Non-ASCII: math spaces → explicit widths, letters → identifiers,
            // other characters → symbols
            Some(&b) if b > 127 => {
                let ch = input.chars().next().unwrap();
                let len = ch.len_utf8();
                let scripted_nbsp = ch == '\u{00a0}' && {
                    let rest = &input[len..];
                    ScriptMarker::at(&rest[self.skip_script_ws(rest.as_bytes())..]).is_some()
                };
                if decode_script_scalar(ch).is_some() || scripted_nbsp {
                    let sid = self.push_char_str(ch);
                    return Ok((
                        &input[len..],
                        self.arena
                            .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                    ));
                }
                if let Some(width) = crate::unicode::math_space_width(ch) {
                    return Ok((
                        &input[len..],
                        self.arena.push_node(CompactNode::Space(width)),
                    ));
                }
                let sid = self.arena.push_str(&input[..len]);
                // Letterlike symbols such as ℜ and ℑ have named ordinary
                // atoms. Use the same class for their literals and commands,
                // leaving unmapped Unicode letters as identifiers.
                let kind = crate::unicode::symbols::mapped_symbol_kind(ch);
                let node = match kind {
                    None | Some(SymbolKind::Alphabetic) if ch.is_alphabetic() => {
                        CompactNode::Identifier(sid)
                    },
                    _ => CompactNode::Symbol(kind.unwrap_or(SymbolKind::Ordinary), sid),
                };
                Ok((&input[len..], self.arena.push_node(node)))
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

        let ids = self.resolve_infix_fraction(ids);
        let content =
            if ids.len() == 1 && matches!(*self.arena.node(ids[0]), CompactNode::StyleSwitch(_)) {
                // Keep an empty declaration group scoped. Flattening
                // `{"\scriptstyle"}` to a bare marker would make the declaration
                // affect the following sibling.
                let span = self.arena.push_children_deferred(&ids);
                self.arena.push_node(CompactNode::Row(span))
            } else {
                self.flatten_single(&ids)
            };
        Ok((input, content))
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
        let end = super::optional_argument_end(inner);
        let end = end.ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let content = &inner[..end];
        let rest = &inner[end + 1..];
        let (leftover, ids) = self.parse_node_list(content)?;
        // leftover should be empty; if not, ignore trailing content
        let _ = leftover;
        Ok((rest, self.resolve_infix_fraction(ids)))
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
        Ok((rest, self.push_ascii_math_symbol(c)))
    }

    fn push_ascii_math_symbol(&mut self, c: char) -> NodeId {
        let kind = classify_char(c);
        // Remap ASCII characters that have distinct math semantics.
        let ch = match c {
            '-' => '\u{2212}',  // hyphen-minus -> minus sign
            '\'' => '\u{2032}', // apostrophe -> prime
            _ => c,
        };
        let sid = self.push_char_str(ch);
        self.arena.push_node(CompactNode::Symbol(kind, sid))
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
        let name_len = super::control_word_len(after_backslash);
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
            ':' | '>' => CompactNode::Space(Rational::new(4, 18)),
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
        // User-declared paired delimiters (`\DeclarePairedDelimiter`).
        if let Some((open, close)) = self.paired_delims.get(name).cloned() {
            return self.parse_paired_delim(input, &open, &close);
        }
        // Leaf commands that produce a node without consuming additional input.
        if let Some(id) = self.try_leaf_command(name) {
            return Ok((input, id));
        }
        if let Some(style) = super::text_style(name) {
            return self.parse_text(input, style);
        }
        match name {
            // --- mathtools / physics paired delimiters (auto-scaling) ---
            // Each accepts an optional `*` (auto-size; a no-op here since the
            // engine always auto-sizes) before the single argument.
            "abs" => self.parse_paired_delim(input, "|", "|"),
            "norm" => self.parse_paired_delim(input, "\u{2016}", "\u{2016}"),
            "ceil" => self.parse_paired_delim(input, "\u{2308}", "\u{2309}"),
            "floor" => self.parse_paired_delim(input, "\u{230A}", "\u{230B}"),
            // `\DeclarePairedDelimiter\cmd{open}{close}` — register a delimiter.
            "DeclarePairedDelimiter" => self.parse_declare_paired_delim(input),
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

            // These packages translate their arguments to ordinary LaTeX.
            "ce" | "pu" | "dv" | "pdv" | "fdv" | "num" | "si" | "SI" | "qty" | "unit" | "ang"
            | "numrange" | "SIrange" | "qtyrange" | "numlist" | "SIlist" | "qtylist"
            | "sisetup" => self.parse_package_translation(input, name),
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
            "reflectbox" | "mathreflectbox" => self.parse_reflect(input, name == "reflectbox"),
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
            "dbinom" => self.parse_styled_binom(input, 0),
            "tbinom" => self.parse_styled_binom(input, 1),

            // --- Math atom type wrappers ---
            "@binrel" => self.parse_binrel(input),
            // Unexpanded internal commands have no math-glyph fallback.
            _ if name.starts_with('@') => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            ))),
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

            // `\char` is a math-mode primitive used by the LaTeX writer for
            // literal ASCII glyphs whose source characters have active TeX
            // meanings (`~`, `^`, `'`, ...).  The writer uses the backtick
            // form (`\char`\~`) so the following sibling cannot be consumed
            // as part of a hexadecimal character number.  Keep the result an
            // ordinary symbol: unlike `\mathord`, this preserves the public
            // AST shape when a direct ordinary symbol is written and read
            // back.
            "char" => self.parse_char(input),

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
                self.infix_marker_seen = true;
                Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
            },

            // --- Mod operators ---
            "pmod" | "pod" => self.parse_pmod(input),

            // --- Kern/space dimension commands ---
            "kern" | "mkern" | "hskip" | "mskip" => self.parse_kern(input, name),
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
            // extpfeil: the true long double rightarrow ⟹ (U+27F9), distinct
            // from the standard double arrow ⇒ (U+21D2) of `\xRightarrow`.
            "xLongrightarrow" => self.parse_extensible_arrow(input, '\u{27F9}'),

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
                // Characters that are active script/prime markers in math mode
                // cannot be written as bare delimiters after an atom: a literal
                // `^` would be re-parsed as a superscript marker. Drop the
                // \middle command and let the following marker be parsed as a
                // script on the preceding atom.
                if let Some(ch) = input.chars().next()
                    && matches!(ch, '^' | '_' | '\'')
                {
                    return Ok((
                        input,
                        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                    ));
                }
                let (input, sid) = self.parse_delimiter_into(input)?;
                let ch = self.arena.str_val(sid).chars().next().unwrap_or('\0');
                // Braces may disappear during normalization. Only an enclosing
                // left/right pair gives this delimiter a stable fence context.
                if self.in_left_right {
                    return Ok((input, self.arena.push_node(CompactNode::Middle(sid))));
                }
                // The active-character `~` is a non-breaking space in LaTeX, not a
                // delimiter. Treating it as a bare symbol would write a literal `~`
                // that re-parses as a space and breaks round-trip idempotency.
                if ch == '~' {
                    return Ok((
                        input,
                        self.arena
                            .push_node(CompactNode::Space(Rational::new(1, 4))),
                    ));
                }
                // A delimiter character that is not a recognized math symbol
                // cannot be written as a bare atom outside a fence without
                // producing unparseable output. Treat the whole \middle as
                // unparseable and stop.
                let symbol = if ch.is_ascii() {
                    if !SYMBOL_BYTES.contains(&(ch as u8)) {
                        return Err(nom::Err::Error(nom::error::Error::new(
                            input,
                            nom::error::ErrorKind::Char,
                        )));
                    }
                    self.push_ascii_math_symbol(ch)
                } else {
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid))
                };
                Ok((input, symbol))
            },

            // --- Color (consume color arg, return content) ---
            "color" => self.parse_color_discard(input),
            "textcolor" => self.parse_textcolor(input),
            "definecolor" | "providecolor" => self.parse_definecolor(input),
            "colorlet" => self.parse_colorlet(input),
            // `\rowcolors{start}{odd}{even}` — alternating-row coloring for the
            // next array/tabular (xcolor `[table]`).
            "rowcolors" | "rowcolors*" => self.parse_rowcolors(input),
            // `\arrayrulecolor{c}` at top level — accepted and ignored (rule
            // colors are not representable on the Array node).
            "arrayrulecolor" => {
                let (input, _) = self.parse_colortbl_color(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },
            // `\empheqset{...}` — global empheq option setup; accepted/ignored.
            "empheqset" => {
                let (input, _) = ws(input)?;
                let input = match super::mhchem::capture_braced(input) {
                    Some((rest, _)) => rest,
                    None => input,
                };
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },

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

            // --- Poor man's bold: bold-italic for math letters (amsbsy) ---
            "pmb" => self.parse_math_font(input, "pmb"),

            // --- Raisebox (consume dimension, return content) ---
            "raisebox" => self.parse_raisebox(input),

            // --- Centernot (negation overlay) ---
            "centernot" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                // Prefer a precomposed negated codepoint (e.g. → ↛, = ≠) when
                // one exists: it is correctly centered by the font and round-
                // trips cleanly (↛ → \nrightarrow). Otherwise fall back to a
                // combining-solidus overlay (U+0338).
                if let Some(ch) = self.node_single_char(arg_id)
                    && let Some(neg) = centernot_precomposed(ch)
                {
                    let sid = self.push_char_str(neg);
                    return Ok((
                        input,
                        self.arena
                            .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
                    ));
                }
                // Overlay a combining solidus on the argument
                let slash_sid = self.arena.push_str("\u{0338}");
                let slash_id = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, slash_sid));
                let children = self.arena.push_children_deferred(&[arg_id, slash_id]);
                Ok((input, self.arena.push_node(CompactNode::Row(children))))
            },

            // --- Hyperlinks and URLs (render visible text only) ---
            "href" => {
                // \href{url}{text} — consume url, return text
                let (input, _url_id) = self.parse_required_group(input)?;
                self.warn("\\href target URL was dropped; only the visible text is rendered");
                self.parse_required_group(input)
            },
            "url" => {
                // \url{text} — render as text
                let (input, _) = ws(input)?;
                let (input, _) = char('{')(input)?;
                let mut depth = 1u32;
                let mut end = None;
                for (i, c) in input.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i);
                                break;
                            }
                        },
                        _ => {},
                    }
                }
                let Some(end) = end else {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Tag,
                    )));
                };
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
                self.warn(format!(
                    "\\{name} attribute was dropped; only the content is rendered"
                ));
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
                self.warn("\\includegraphics was dropped; images are not rendered in math output");
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
                let children = self.arena.push_children_deferred(&[int1, dots, int2]);
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

            // --- Old-style font switches (TeX declarations) ---
            "rm" | "it" | "bf" | "sf" | "tt" | "cal" => self.parse_old_font_switch(input, name),

            // --- Above (infix with dimension) ---
            "above" => {
                // \above <dimen> — like \over but with specified bar thickness
                // Consume optional dimension, produce infix marker (treated like \over)
                let (input, _) = ws(input)?;
                // Try to consume a dimension
                let input = match dimension::width(input) {
                    Ok((rest, _)) => rest,
                    Err(_) => input,
                };
                let sid = self.arena.push_str("__infix_over__");
                self.infix_marker_seen = true;
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
                // \angl{n} is the actuarial angle enclosure: a top and right
                // rule around the argument.
                if name == "angln" {
                    let n = self.arena.push_str("n");
                    let content_id = self
                        .arena
                        .push_node(CompactNode::Text(TextStyle::Normal, n));
                    return Ok((
                        input,
                        self.arena.push_node(CompactNode::Actuarial {
                            content: content_id,
                            text_mode: true,
                        }),
                    ));
                }
                let (rest, content) = self.parse_hbox_argument(input)?;
                Ok((
                    rest,
                    self.arena.push_node(CompactNode::Actuarial {
                        content,
                        text_mode: true,
                    }),
                ))
            },

            // --- Phase angle ---
            "phase" => {
                // \phase{angle} — render as underline with angle content
                let (input, content_id) = self.parse_required_group(input)?;
                let angle_sid = self.arena.push_str("\u{2220}");
                let angle_id = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, angle_sid));
                let children = self.arena.push_children_deferred(&[angle_id, content_id]);
                Ok((input, self.arena.push_node(CompactNode::Row(children))))
            },

            // --- Braket/Set (physics notation) ---
            "Braket" | "braket" => {
                // \Braket{...} → ⟨...⟩
                let (input, content_id) = self.parse_required_group(input)?;
                let open = self.arena.push_str("\u{27E8}");
                let close = self.arena.push_str("\u{27E9}");
                let content = self.arena.push_children_deferred(&[content_id]);
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
                let content = self.arena.push_children_deferred(&[content_id]);
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
            "cr" => Ok((input, self.arena.push_node(CompactNode::Newline))),

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
                    let limit_style = if name == "plim" {
                        LimitStyle::AboveBelow
                    } else if operator_has_limits(name) {
                        LimitStyle::Default
                    } else {
                        LimitStyle::SubSup
                    };
                    // amsmath's aliases carry an internal thin mathematical
                    // space. Keep it in the operator name so MathML and the
                    // layout engine see the same spacing as `arg\,min`.
                    let canonical_name = match name {
                        "argmin" => "arg\u{2009}min",
                        "argmax" => "arg\u{2009}max",
                        "limsup" => "lim\u{2009}sup",
                        "liminf" => "lim\u{2009}inf",
                        "injlim" => "inj\u{2009}lim",
                        "projlim" => "proj\u{2009}lim",
                        _ => name,
                    };
                    let sid = self.arena.push_str(canonical_name);
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
        let mut space = |num, den| {
            Some(
                self.arena
                    .push_node(CompactNode::Space(Rational::new(num, den))),
            )
        };
        match name {
            // Spacing
            "quad" => space(1, 1),
            "qquad" => space(2, 1),
            "enspace" | "enskip" => space(1, 2),
            "thinspace" => space(3, 18),
            "medspace" => space(4, 18),
            "thickspace" => space(5, 18),
            "negthinspace" => space(-3, 18),
            "negmedspace" => space(-4, 18),
            "negthickspace" => space(-5, 18),
            // Style commands — emit StyleSwitch nodes that the layout engine uses
            // to change the math style for subsequent siblings in the same row.
            "displaystyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(0))),
            "textstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(1))),
            "scriptstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(2))),
            "scriptscriptstyle" => Some(self.arena.push_node(CompactNode::StyleSwitch(3))),
            // Explicit line break.
            "newline" => Some(self.arena.push_node(CompactNode::Newline)),
            // TeX's `\relax` terminates scanners and contributes no glyph or
            // spacing.  Keep it as an empty row so scripts can recognize an
            // empty argument without turning the command name into letters.
            "relax" => Some(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))),
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
            if legacy_font_declaration(input).is_some() {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )));
            }
            self.parse_single_token(input)
        }
    }

    fn parse_reflect<'a>(&mut self, input: &'a str, text_mode: bool) -> IResult<&'a str, NodeId> {
        let (rest, content) = if text_mode {
            self.parse_hbox_argument(input)?
        } else {
            self.parse_required_group(input)?
        };
        Ok((
            rest,
            self.arena
                .push_node(CompactNode::Reflect { content, text_mode }),
        ))
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
            Some(&b) if !b.is_ascii() => self.parse_atom(input),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alt,
            ))),
        }
    }

    /// Apply the first argument's binary/relation/ordinary class to the second
    /// argument. Like KaTeX, inspect the first atom of the outer argument group.
    fn parse_binrel<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let grouped_first = input
            .strip_prefix('{')
            .is_some_and(|body| body[skip_ws_bytes(body.as_bytes())..].starts_with('{'));
        let (rest, sample) = self.parse_required_group(input)?;
        let first = match *self.arena.node(sample) {
            CompactNode::Row(span) => self.arena.children(span).first().copied(),
            _ => Some(sample),
        };
        let atom_type = match first
            .filter(|_| !grouped_first)
            .map(|id| *self.arena.node(id))
        {
            Some(CompactNode::Symbol(SymbolKind::BinaryOp, _)) => MathAtomType::Bin,
            Some(CompactNode::Symbol(SymbolKind::Relation, _)) => MathAtomType::Rel,
            _ => MathAtomType::Ord,
        };
        let (rest, content) = self.parse_required_group(rest)?;
        let id = self
            .arena
            .push_node(CompactNode::MathAtom { atom_type, content });
        Ok((rest, id))
    }

    /// Parse the character forms emitted by the LaTeX writer's math-mode
    /// literal-symbol path.  TeX accepts several number syntaxes after
    /// `\char`; supporting the backtick form is sufficient for writer output,
    /// while decimal/hex forms make the reader useful for ordinary KaTeX/TeX
    /// input as well (`\char"7e`, `\char126`).
    fn parse_char<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let Some(first) = input.chars().next() else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        };

        let (rest, codepoint) = if first == '`' {
            let after_tick = &input[first.len_utf8()..];
            let Some(token) = after_tick.chars().next() else {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            };
            let mut rest = &after_tick[token.len_utf8()..];
            let codepoint = if token == '\\' {
                // `\char`\\` names a literal backslash; an escaped brace or
                // punctuation is handled the same way.  A control word is
                // intentionally reduced to its first character, matching
                // TeX's character-token interpretation for this form.
                let Some(escaped) = rest.chars().next() else {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Char,
                    )));
                };
                rest = &rest[escaped.len_utf8()..];
                escaped
            } else {
                token
            };
            (rest, codepoint as u32)
        } else if first == '"' {
            let digits = &input[first.len_utf8()..];
            let count = digits
                .bytes()
                .take_while(|byte| byte.is_ascii_hexdigit())
                .count();
            if count == 0 {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::HexDigit,
                )));
            }
            let value = u32::from_str_radix(&digits[..count], 16).map_err(|_| {
                nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::HexDigit,
                ))
            })?;
            (&digits[count..], value)
        } else {
            let digits = input.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Digit,
                )));
            }
            let value = input[..digits].parse::<u32>().map_err(|_| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
            })?;
            (&input[digits..], value)
        };

        let Some(ch) = char::from_u32(codepoint) else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        };
        let sid = self.push_char_str(ch);
        Ok((
            rest,
            self.arena
                .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
        ))
    }

    /// Keep translation buffers and preprocessing errors off the recursive
    /// command dispatch frame for ordinary commands such as fractions.
    fn parse_package_translation<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (rest, latex) = match name {
            "ce" | "pu" => {
                let Some((rest, body)) = super::mhchem::capture_braced(input) else {
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
                (rest, latex)
            },
            "dv" | "pdv" | "fdv" => super::physics::translate_derivative(name, input),
            _ => super::siunitx::translate(name, input),
        };
        let processed = preprocess_cow(&latex).map_err(|failure| {
            self.abort = Some(expansion_error(failure));
            nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
        })?;
        let id = match self.parse_node_list(&processed) {
            Ok((_, ids)) => self.flatten_single(&ids),
            Err(_) => {
                self.check_node_limit();
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
            },
        };
        Ok((rest, id))
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
            let content = self.arena.push_children_deferred(&[frac_id]);
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
    /// Maps `<`/`>` to the same angle-bracket codepoints used by
    /// `parse_delimiter_into`, so the writer emits \langle/\rangle and the
    /// output round-trips stably.
    fn extract_delimiter_str(&self, id: NodeId) -> String {
        match self.arena.node(id) {
            CompactNode::Symbol(_, sid) => match self.arena.str_val(*sid) {
                "<" => "\u{27E8}".to_string(),
                ">" => "\u{27E9}".to_string(),
                s => s.to_string(),
            },
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
        let content = self.arena.push_children_deferred(&[frac_id]);
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
        self.fix_accent_base(base_id);
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
        self.parse_accent_wide(input, '\u{23B4}', AccentPosition::Above, true)
    }

    fn parse_underbracket<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.parse_accent_wide(input, '\u{23B5}', AccentPosition::Below, true)
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
        let enclosing_fence = self.in_left_right;
        self.in_left_right = true;
        let result = self.parse_left_right_inner(input);
        self.in_left_right = enclosing_fence;
        self.depth -= 1;
        result
    }

    fn parse_left_right_inner<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, open) = self.parse_delimiter_into(input)?;
        let (input, content_ids) = self.parse_until_right(input)?;
        let (input, close) = self.parse_delimiter_into(input)?;

        let content = self.arena.push_children_deferred(&content_ids);
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
            self.ensure_node_budget(rest)?;
            let (r, _) = ws(rest)?;
            rest = r;
            // A command such as \rightarrow is an operand, not a closing fence.
            if let Some(after_right) = strip_control_word(rest, "right") {
                let (after_right, _) = ws(after_right)?;
                return Ok((after_right, self.resolve_infix_fraction(ids)));
            }
            if let Some(after_mid) = strip_control_word(rest, "middle") {
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

    fn parse_text<'a>(&mut self, input: &'a str, style: TextStyle) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let invalid_arg =
            || nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char));
        let braced = input.starts_with('{');
        let (rest, raw) = hbox::argument(input)?;
        if !braced && matches!(raw, "$" | r"\\") {
            return Err(invalid_arg());
        }
        // Text groups can contain local font changes and inline math.
        if braced && raw.contains(['\\', '$']) {
            let mut nodes = SmallVec::new();
            self.hbox_runs(raw, style, false, &mut nodes)?;
            if !nodes.is_empty() {
                let content = self.flatten_single(&nodes);
                self.ensure_node_budget(rest)?;
                return Ok((rest, content));
            }
        }
        let text = clean_text_content(raw);
        if !braced && text.is_empty() {
            return Err(invalid_arg());
        }
        let sid = self.arena.push_str(&text);
        Ok((rest, self.arena.push_node(CompactNode::Text(style, sid))))
    }

    fn parse_math_font<'a>(&mut self, input: &'a str, cmd_name: &str) -> IResult<&'a str, NodeId> {
        let style = latex_font_to_style(cmd_name).unwrap_or(TextStyle::Normal);
        let previous_arrays = self.arrays_built;
        let (input, content_id) = self.parse_required_group(input)?;
        if self.arrays_built != previous_arrays {
            return Ok((input, self.font_argument_with_table(style, content_id)));
        }
        // If content is a Row, use its children span; otherwise wrap in single-element span
        let span = match *self.arena.node(content_id) {
            CompactNode::Row(s) => s,
            _ => self.arena.push_children_deferred(&[content_id]),
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
            Err(_) => (input, {
                self.check_node_limit();
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
            }),
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
            _ => (
                op_id,
                NodeId::NONE,
                NodeId::NONE,
                NodeId::NONE,
                LimitStyle::Default,
            ),
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
            (true, false) => self.arena.push_node(CompactNode::Subscript {
                base: with_pre,
                sub: r_sub,
            }),
            (false, true) => self.arena.push_node(CompactNode::Superscript {
                base: with_pre,
                sup: r_sup,
            }),
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

        let span = self.arena.push_children_deferred(&items);
        Ok((input, self.arena.push_node(CompactNode::Row(span))))
    }

    fn parse_overset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, over_id) = self.parse_required_group(input)?;
        let (input, base_id) = self.parse_required_group(input)?;
        if let Some(ch) = self.single_accent_glyph(over_id) {
            self.fix_accent_base(base_id);
            return Ok((
                input,
                self.arena.push_node(CompactNode::Accent {
                    base: base_id,
                    accent: ch,
                    position: AccentPosition::Above,
                    wide: false,
                }),
            ));
        }
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
        if let Some(ch) = self.single_accent_glyph(under_id) {
            self.fix_accent_base(base_id);
            return Ok((
                input,
                self.arena.push_node(CompactNode::Accent {
                    base: base_id,
                    accent: ch,
                    position: AccentPosition::Below,
                    wide: false,
                }),
            ));
        }
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    /// A TeX accent stays attached to its base even in inline math. In
    /// native MathML a movable <mo> would move the accent to a side script.
    fn fix_accent_base(&mut self, base: NodeId) {
        if let CompactNode::Operator { limits, .. } = self.arena.node_mut(base)
            && *limits == LimitStyle::Default
        {
            *limits = LimitStyle::AboveBelow;
        }
    }

    fn single_accent_glyph(&self, id: NodeId) -> Option<char> {
        match *self.arena.node(id) {
            CompactNode::Symbol(_, sid) => {
                let mut chars = self.arena.str_val(sid).chars();
                let ch = chars.next()?;
                (chars.next().is_none() && crate::unicode::symbols::is_accent_glyph(ch))
                    .then_some(ch)
            },
            CompactNode::Row(span) => {
                let &[only] = self.arena.children(span) else {
                    return None;
                };
                self.single_accent_glyph(only)
            },
            _ => None,
        }
    }

    fn parse_operatorname<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // A starred operatorname uses TeX's context-dependent movable limits;
        // an unstarred operatorname is an ordinary side-script operator. The
        // distinction matters for following \limits/\nolimits: KaTeX accepts
        // those tokens after an unstarred name but deliberately ignores them.
        let (input, starred) = if let Some(rest) = input.strip_prefix('*') {
            (rest, true)
        } else {
            (input, false)
        };
        let (input, _) = ws(input)?;
        // Accept both braced {name} and unbraced single letter
        let (input, name) = if input.starts_with('{') {
            let Some((input, raw_name)) = super::mhchem::capture_braced(input) else {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            };
            let name = decode_operator_name(raw_name);
            (input, name)
        } else if input
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            (&input[1..], input[..1].to_owned())
        } else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Alpha,
            )));
        };
        // Consume placement modifiers at the operatorname site. Unstarred
        // names always stay side-script operators; starred names honor the
        // final modifier just like native named operators.
        let (input, modifier) = consume_limit_modifiers(input);
        let limits = if starred {
            modifier.unwrap_or(LimitStyle::Default)
        } else {
            LimitStyle::SubSup
        };
        let sid = self.arena.push_str(&name);
        let operator = self
            .arena
            .push_node(CompactNode::Operator { name: sid, limits });
        if !starred {
            self.last_unstarred_operator = Some(operator);
        }
        Ok((input, operator))
    }

    fn parse_styled_binom<'a>(&mut self, input: &'a str, style: u8) -> IResult<&'a str, NodeId> {
        let (rest, binom) = self.parse_binom(input)?;
        let marker = self.arena.push_node(CompactNode::StyleSwitch(style));
        let span = self.arena.push_children_deferred(&[marker, binom]);
        Ok((rest, self.arena.push_node(CompactNode::Row(span))))
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
        let content = self
            .arena
            .push_children_deferred(&[mod_id, space_id, content_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    /// A kern takes a fixed dimension. Glue additionally accepts stretch and
    /// shrink components. An unstretched math list uses the natural width.
    fn parse_kern<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let group = if input.starts_with('{') {
            Some(super::mhchem::capture_braced(input).ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
            })?)
        } else {
            None
        };
        let dimension = group.map_or(input, |(_, body)| body);
        let (mut rest, parsed) = match dimension::parse(dimension) {
            Ok(parsed) => parsed,
            Err(nom::Err::Error(_)) => {
                return Ok((
                    group.map_or(input, |(tail, _)| tail),
                    self.arena.push_node(CompactNode::Space(Rational::zero())),
                ));
            },
            Err(error) => return Err(error),
        };
        let is_mu = matches!(name, "mkern" | "mskip");
        if let Some(unit) = parsed.unit {
            self.warn_kern_unit(is_mu, unit);
        }
        if matches!(name, "hskip" | "mskip") {
            let fixed_end = rest;
            for keyword in ["plus", "minus"] {
                let trimmed = &rest[skip_ws_bytes(rest.as_bytes())..];
                let Some((word, operand)) = trimmed.split_at_checked(keyword.len()) else {
                    continue;
                };
                if !word.eq_ignore_ascii_case(keyword)
                    || operand.starts_with(|c: char| c.is_ascii_alphabetic())
                {
                    continue;
                }
                if let Some((tail, unit)) = dimension::glue_component(operand) {
                    self.warn_kern_unit(is_mu, unit);
                    rest = tail;
                } else {
                    if group.is_none() {
                        self.warn(
                            "incomplete glue component; following tokens were retained as math",
                        );
                    }
                    break;
                }
            }
            if rest.len() != fixed_end.len() {
                self.warn("only the natural width is representable; glue stretch and shrink components were dropped");
            }
        }
        if let Some((tail, _)) = group {
            if !rest.trim().is_empty() {
                self.warn("extra tokens in the braced spacing dimension were dropped");
            }
            rest = tail;
        }
        let basis = match parsed.unit {
            Some("mu") => SpaceBasis::MathMu,
            Some("em" | "ex") => SpaceBasis::TextEm,
            _ => SpaceBasis::Physical,
        };
        Ok((rest, self.arena.push_space(parsed.width, basis)))
    }

    fn warn_kern_unit(&mut self, is_mu: bool, unit: &str) {
        let wrong = !matches!(unit, "fil" | "fill" | "filll") && (unit == "mu") != is_mu;
        if wrong {
            self.warn(if is_mu {
                "\\mkern/\\mskip spacing expects a mu unit"
            } else {
                "\\kern/\\hskip spacing does not use a mu unit"
            });
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
        if let Ok((_, r)) = dimension::width(dim_str) {
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
            let lift = dimension::width(lift_str)
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
        let width = dimension::width(width_str)
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
        let height = dimension::width(height_str)
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

        let mut table_rows: Vec<Vec<SmallVec<[NodeId; 8]>>> =
            cells.into_iter().map(|cell| vec![cell]).collect();
        self.apply_table_cell_style(&mut table_rows, 2);
        Ok((
            input,
            self.build_array_with_spacing(
                &[ColumnAlign::Center],
                table_rows,
                50,
                false,
                0,
                0,
                false,
            ),
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
                '/' => (SymbolKind::Fence, "/"),
                _ => {
                    if input.starts_with('\\') {
                        // Use parse_element so that n-ary operators like \int get
                        // their normal scripts/body consumed. parse_command alone
                        // would return a bare LargeOp symbol, which then diverges
                        // when the sizing wrapper is later dropped and the symbol
                        // is re-parsed as a full n-ary (e.g. \big \int\ ).
                        return self.parse_element(input);
                    }
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Char,
                    )));
                },
            };
            let sid = self.arena.push_str(s);
            return Ok((rest, self.arena.push_node(CompactNode::Symbol(kind, sid))));
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Eof,
        )))
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
                self.ensure_node_budget(rest)?;
                let (r, _) = ws(rest)?;
                rest = r;
                if rest.is_empty()
                    || rest.starts_with('&')
                    || rest.starts_with("\\\\")
                    || strip_control_word(rest, "end").is_some()
                    || strip_control_word(rest, "right").is_some()
                    || strip_control_word(rest, "middle").is_some()
                    || rest.starts_with('}')
                {
                    break;
                }
                match self.parse_element(rest) {
                    Ok((r, id)) => {
                        self.push_element(&mut ids, id);
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
                    self.check_node_limit();
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

    /// Resolve a `\colorbox`/`\fcolorbox` color spec, honouring an optional
    /// `[model]` (`rgb`/`RGB`/`HTML`/`gray`/`cmyk`). Without a model the spec is
    /// resolved as a named/user/xcolor-expression color.
    fn resolve_box_color(&self, model: Option<&str>, spec: &str) -> String {
        match model {
            Some(m) => crate::css_colors::resolve_color_model(m.trim(), spec.trim())
                .map(crate::css_colors::rgb_to_hex)
                .unwrap_or_else(|| self.resolve_color(spec)),
            None => self.resolve_color(spec),
        }
    }

    /// `\rowcolors[cmds]{start}{odd}{even}` — capture alternating-row coloring
    /// for the next array/tabular. Produces no visible output.
    fn parse_rowcolors<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional `[commands]` argument (e.g. `\hline`); accepted and ignored.
        let input = if input.starts_with('[') {
            input.find(']').map(|e| &input[e + 1..]).unwrap_or(input)
        } else {
            input
        };
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, start_s) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, odd) = self.parse_colortbl_color(input)?;
        let (input, even) = self.parse_colortbl_color(input)?;
        let start = start_s.trim().parse::<usize>().unwrap_or(1).max(1);
        self.rowcolors = Some((start, odd, even));
        Ok((
            input,
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        ))
    }

    /// Extract per-column background colors from `>{\columncolor{c}}` prefixes in
    /// an array column spec, returning the spec with those `>{...}` (and any
    /// `<{...}`) groups removed plus a per-column color vector aligned to the
    /// alignment columns. Specs using `*{n}{...}` repetition are left untouched
    /// (column indices cannot be statically aligned) and yield no colors.
    fn extract_column_colors(&self, spec: &str) -> (String, Vec<Option<String>>) {
        if spec.contains('*') {
            return (spec.to_string(), Vec::new());
        }
        let bytes = spec.as_bytes();
        let mut i = 0usize;
        let mut cleaned = String::with_capacity(spec.len());
        let mut col_bg: Vec<Option<String>> = Vec::new();
        let mut pending: Option<String> = None;
        while i < bytes.len() {
            let c = bytes[i] as char;
            match c {
                'l' | 'c' | 'r' => {
                    cleaned.push(c);
                    col_bg.push(pending.take());
                    i += 1;
                },
                'p' | 'm' | 'b' => {
                    // Column with a {width} argument: copy the letter and the
                    // braced width verbatim.
                    cleaned.push(c);
                    col_bg.push(pending.take());
                    i += 1;
                    if let Some((after, inner)) = take_braced_at(spec, i) {
                        cleaned.push('{');
                        cleaned.push_str(inner);
                        cleaned.push('}');
                        i = after;
                    }
                },
                '>' => {
                    // `>{...}` pre-column directive: capture \columncolor, strip.
                    if let Some((after, inner)) = take_braced_at(spec, i + 1) {
                        if let Some(color) = self.columncolor_in(inner) {
                            pending = Some(color);
                        }
                        i = after;
                    } else {
                        cleaned.push(c);
                        i += 1;
                    }
                },
                '<' => {
                    // `<{...}` post-column directive: strip.
                    if let Some((after, _)) = take_braced_at(spec, i + 1) {
                        i = after;
                    } else {
                        cleaned.push(c);
                        i += 1;
                    }
                },
                '@' | '!' => {
                    // Inter-column material: keep verbatim.
                    cleaned.push(c);
                    i += 1;
                    if let Some((after, inner)) = take_braced_at(spec, i) {
                        cleaned.push('{');
                        cleaned.push_str(inner);
                        cleaned.push('}');
                        i = after;
                    }
                },
                _ => {
                    cleaned.push(c);
                    i += 1;
                },
            }
        }
        (cleaned, col_bg)
    }

    /// If `s` contains a `\columncolor{c}` / `\columncolor[model]{spec}`
    /// directive, return the resolved color string.
    fn columncolor_in(&self, s: &str) -> Option<String> {
        let after = s
            .find("\\columncolor")
            .map(|p| &s[p + "\\columncolor".len()..])?;
        let after = after.trim_start();
        // Optional [model]
        let (model, after) = if let Some(rest) = after.strip_prefix('[') {
            let end = rest.find(']')?;
            (Some(&rest[..end]), &rest[end + 1..])
        } else {
            (None, after)
        };
        let after = after.trim_start();
        let rest = after.strip_prefix('{')?;
        let end = rest.find('}')?;
        let spec = &rest[..end];
        Some(self.resolve_box_color(model, spec))
    }

    /// Apply captured `\rowcolors` and per-column `\columncolor` backgrounds to
    /// table `rows`, wrapping only cells that are not already explicitly colored
    /// (`\cellcolor`/`\rowcolor`). Consumes (clears) `self.rowcolors`.
    fn apply_table_colors(
        &mut self,
        rows: &mut [Vec<SmallVec<[NodeId; 8]>>],
        col_bg: &[Option<String>],
    ) {
        let rowcolors = self.rowcolors.take();
        if rowcolors.is_none() && col_bg.iter().all(Option::is_none) {
            return;
        }
        for (r, row) in rows.iter_mut().enumerate() {
            if self.check_node_limit() {
                return;
            }
            // Row-level alternating color (1-based row numbering).
            let row_color: Option<String> = rowcolors.as_ref().and_then(|(start, odd, even)| {
                let n = r + 1;
                if n < *start {
                    return None;
                }
                let c = if (n - start) % 2 == 0 { odd } else { even };
                if c.is_empty() { None } else { Some(c.clone()) }
            });
            for (col, cell) in row.iter_mut().enumerate() {
                if self.check_node_limit() {
                    return;
                }
                if self.cell_is_boxed(cell) {
                    continue; // already colored by \cellcolor/\rowcolor
                }
                let color = col_bg
                    .get(col)
                    .and_then(|c| c.clone())
                    .or_else(|| row_color.clone());
                if let Some(color) = color
                    && !color.is_empty()
                {
                    *cell = self.wrap_cell_bg(std::mem::take(cell), &color);
                }
            }
        }
    }

    /// Whether a parsed cell is already a single background-`Boxed` node.
    fn cell_is_boxed(&self, cell: &SmallVec<[NodeId; 8]>) -> bool {
        cell.len() == 1
            && matches!(
                *self.arena.node(cell[0]),
                CompactNode::Boxed { bg_color, .. } if !self.arena.str_val(bg_color).is_empty()
            )
    }

    /// If `id` resolves to a node carrying exactly one character (a `Symbol` or
    /// single-char `Identifier`, possibly wrapped in a one-child `Row`), return
    /// that char. Used by `\centernot` to look up a precomposed negation.
    fn node_single_char(&self, id: NodeId) -> Option<char> {
        match *self.arena.node(id) {
            CompactNode::Symbol(_, sid) | CompactNode::Identifier(sid) => {
                let s = self.arena.str_val(sid);
                let mut it = s.chars();
                let c = it.next()?;
                it.next().is_none().then_some(c)
            },
            CompactNode::Row(span) => {
                let kids = self.arena.children(span);
                if kids.len() == 1 {
                    let k = kids[0];
                    self.node_single_char(k)
                } else {
                    None
                }
            },
            _ => None,
        }
    }

    /// Parse a paired-delimiter command body: an optional `*` (auto-size, a
    /// no-op here) followed by a single required group, wrapped in a
    /// [`CompactNode::Fenced`] with the given open/close delimiters.
    fn parse_paired_delim<'a>(
        &mut self,
        input: &'a str,
        open: &str,
        close: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let input = input.strip_prefix('*').unwrap_or(input);
        let (input, arg_id) = self.parse_required_group(input)?;
        let open_sid = self.arena.push_str(open);
        let close_sid = self.arena.push_str(close);
        let content = self.arena.push_children_deferred(&[arg_id]);
        Ok((
            input,
            self.arena.push_node(CompactNode::Fenced {
                open: open_sid,
                close: close_sid,
                content,
            }),
        ))
    }

    /// `\DeclarePairedDelimiter\cmd{open}{close}` (or `{\cmd}` form) — register a
    /// user paired delimiter so `\cmd{x}` / `\cmd*{x}` auto-scale `open x close`.
    fn parse_declare_paired_delim<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let empty = |me: &mut Self| me.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
        let mut rest = input.trim_start();
        // Command name: `{\name}` or `\name`.
        let name = if rest.starts_with('{') {
            match super::mhchem::capture_braced(rest) {
                Some((r, inner)) => {
                    rest = r;
                    inner.trim().trim_start_matches('\\').to_string()
                },
                None => return Ok((input, empty(self))),
            }
        } else if let Some(after) = rest.strip_prefix('\\') {
            let len = after.bytes().take_while(u8::is_ascii_alphabetic).count();
            if len == 0 {
                return Ok((input, empty(self)));
            }
            let n = after[..len].to_string();
            rest = &after[len..];
            n
        } else {
            return Ok((input, empty(self)));
        };
        // Skip optional `[...]` groups (xparse arg-count / default-arg specs).
        loop {
            let t = rest.trim_start();
            match t
                .strip_prefix('[')
                .and_then(|r| r.find(']').map(|e| &r[e + 1..]))
            {
                Some(after) => rest = after,
                None => {
                    rest = t;
                    break;
                },
            }
        }
        // `{open}{close}` delimiter groups.
        let Some((rest, open)) = super::mhchem::capture_braced(rest) else {
            return Ok((rest, empty(self)));
        };
        let Some((rest, close)) = super::mhchem::capture_braced(rest) else {
            return Ok((rest, empty(self)));
        };
        self.paired_delims
            .insert(name, (delim_to_glyph(open), delim_to_glyph(close)));
        Ok((rest, empty(self)))
    }

    /// `\colorbox{color}{content}` or `\colorbox[model]{spec}{content}` —
    /// consume an optional color model and the color arg, emit content in a
    /// colored box.
    fn parse_colorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, model) = parse_optional_model(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        let bg_color = self.resolve_box_color(model.as_deref(), color);
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

    /// `\fcolorbox{framecolor}{bgcolor}{content}` or
    /// `\fcolorbox[model]{frame}{bg}{content}` — consume an optional color model
    /// and two color args, emit content in a colored box.
    fn parse_fcolorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, model) = parse_optional_model(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, frame_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, bg_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        let bg = self.resolve_box_color(model.as_deref(), bg_color);
        let frame = self.resolve_box_color(model.as_deref(), frame_color);
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
        let shift = dimension::width(dim_str)
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
        let dim = dimension::width(dim_str).map(|(_, r)| r).ok();
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

    /// Parse font declarations as runs in the enclosing list. Switching
    /// fonts at the same brace level does not introduce a new TeX group.
    fn parse_old_font_switch<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let style = legacy_font_style(name).expect("legacy font command");
        let (input, _) = ws(input)?;
        let previous = self.font_scope_preserve_breaks;
        self.font_scope_preserve_breaks = self.depth == 1;
        let scoped = self.parse_font_scope(input, style);
        self.font_scope_preserve_breaks = previous;
        let (rest, ids) = scoped?;
        Ok((rest, self.flatten_single(&ids)))
    }

    /// Parse the expression controlled by an old-style font declaration. This
    /// is deliberately separate from `parse_node_list`: table/environment
    /// sentinels such as `\end` are valid input to the outer parser but must
    /// terminate a declaration before they are interpreted as ordinary
    /// commands.
    fn parse_font_scope<'a>(
        &mut self,
        input: &'a str,
        mut style: TextStyle,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let result = (|| {
            let mut ids = SmallVec::new();
            let mut run = SmallVec::new();
            let mut rest = input;
            loop {
                self.ensure_node_budget(rest)?;
                if self.font_scope_preserve_breaks {
                    rest = &rest[skip_horizontal_ws_bytes(rest.as_bytes())..];
                    if let Some(after) = consume_source_newline(rest) {
                        if source_newline_is_hard_break(input, rest) {
                            self.flush_font_run(style, &mut run, &mut ids);
                            ids.push(self.arena.push_node(CompactNode::Newline));
                        }
                        rest = after;
                        continue;
                    }
                } else {
                    rest = ws(rest)?.0;
                }
                if is_table_boundary(rest)
                    || rest.starts_with('}')
                    || is_cr_row_separator(rest)
                    || strip_control_word(rest, "right").is_some()
                    || strip_control_word(rest, "middle").is_some()
                {
                    break;
                }
                if let Some((after, next_style)) = legacy_font_declaration(rest) {
                    if next_style != style {
                        self.flush_font_run(style, &mut run, &mut ids);
                        style = next_style;
                    }
                    rest = after;
                    continue;
                }
                let previous_arrays = self.arrays_built;
                let (after, id) = self.parse_element(rest)?;
                if previous_arrays != self.arrays_built {
                    self.flush_font_run(style, &mut run, &mut ids);
                    ids.push(self.font_argument_with_table(style, id));
                } else if self.infix_marker_seen && self.is_infix_marker_id(id) {
                    // The fraction belongs to the enclosing list; the font
                    // remains in force for the denominator after the marker.
                    self.flush_font_run(style, &mut run, &mut ids);
                    ids.push(id);
                } else {
                    self.push_element(&mut run, id);
                }
                rest = after;
            }
            self.flush_font_run(style, &mut run, &mut ids);
            Ok((rest, ids))
        })();
        self.depth -= 1;
        result
    }

    fn font_argument_with_table(&mut self, style: TextStyle, id: NodeId) -> NodeId {
        // This transformation walks the newly parsed argument before the
        // public reader performs its final tree validation.
        if let Err(error) =
            crate::ast::check_compact_depth(&self.arena, id, 0, crate::ast::MAX_AST_DEPTH)
        {
            self.abort = Some(error);
            return id;
        }
        self.font_node_outside_tables(style, id)
    }

    /// Called only for an argument in which the parser actually built an
    /// alignment. Most font runs can be wrapped without walking their AST.
    fn contains_table(&self, id: NodeId) -> bool {
        let node = self.arena.node(id);
        matches!(node, CompactNode::Array { .. })
            || crate::ast::compact_children(&self.arena, node)
                .into_iter()
                .any(|child| self.contains_table(child))
    }

    fn flush_font_run(
        &mut self,
        style: TextStyle,
        run: &mut SmallVec<[NodeId; 8]>,
        out: &mut SmallVec<[NodeId; 8]>,
    ) {
        if !run.is_empty() {
            let span = self.arena.push_children_deferred(run);
            out.push(self.arena.push_node(CompactNode::Styled(style, span)));
            run.clear();
        }
    }

    fn font_row_outside_tables(&mut self, style: TextStyle, span: NodeSpan) -> NodeSpan {
        let children: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        let mut output = SmallVec::new();
        let mut run = SmallVec::new();
        for child in children {
            if self.contains_table(child)
                || matches!(self.arena.node(child), CompactNode::Middle(_))
            {
                self.flush_font_run(style, &mut run, &mut output);
                output.push(self.font_node_outside_tables(style, child));
            } else {
                run.push(child);
            }
        }
        self.flush_font_run(style, &mut run, &mut output);
        self.arena.push_children_deferred(&output)
    }

    /// Arrays establish new math lists. Preserve the enclosing font in each
    /// other positional child, keeping consecutive styled siblings together.
    fn font_node_outside_tables(&mut self, style: TextStyle, id: NodeId) -> NodeId {
        if !id.is_some() {
            return id;
        }
        let mut node = *self.arena.node(id);
        if matches!(node, CompactNode::Array { .. } | CompactNode::Middle(_)) {
            return id;
        }
        if !self.contains_table(id) {
            let span = self.arena.push_children_deferred(&[id]);
            return self.arena.push_node(CompactNode::Styled(style, span));
        }
        let children: &mut [&mut NodeId] = match &mut node {
            CompactNode::Row(span) | CompactNode::Fenced { content: span, .. } => {
                *span = self.font_row_outside_tables(style, *span);
                &mut []
            },
            CompactNode::Styled(local, span) => {
                let span = self.font_row_outside_tables(*local, *span);
                return self.arena.push_node(CompactNode::Row(span));
            },
            CompactNode::Fraction { num, den, .. } => &mut [num, den],
            CompactNode::Subscript { base, sub }
            | CompactNode::Superscript { base, sup: sub }
            | CompactNode::Underscript {
                base, under: sub, ..
            }
            | CompactNode::Overscript {
                base, over: sub, ..
            } => &mut [base, sub],
            CompactNode::SubSup { base, sub, sup }
            | CompactNode::PreScripts {
                base,
                pre_sub: sub,
                pre_sup: sup,
            }
            | CompactNode::UnderOver {
                base,
                under: sub,
                over: sup,
                ..
            } => &mut [base, sub, sup],
            CompactNode::Root { radicand, index } => &mut [radicand, index],
            CompactNode::Nary { sub, sup, body, .. } => &mut [sub, sup, body],
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => &mut [display, text, script, script_script],
            CompactNode::Sqrt(content)
            | CompactNode::Phantom(content, _)
            | CompactNode::Accent { base: content, .. }
            | CompactNode::Boxed { content, .. }
            | CompactNode::Actuarial { content, .. }
            | CompactNode::Reflect { content, .. }
            | CompactNode::Cancel { content, .. }
            | CompactNode::Scaled { content, .. }
            | CompactNode::RaiseBox { content, .. }
            | CompactNode::Color { content, .. }
            | CompactNode::MathAtom { content, .. } => &mut [content],
            _ => return id,
        };
        for child in children {
            **child = self.font_node_outside_tables(style, **child);
        }
        self.arena.push_node(node)
    }

    /// Parse \\DeclareMathOperator and similar declaration commands — consume all arguments.
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
            "alignedat" | "alignat" | "alignat*" => self.parse_alignedat_env(input, env_name),
            "gathered" => self.parse_gather_env(input, env_name),
            "gather" | "gather*" => self.parse_gather_env(input, env_name),
            "equation" | "equation*" => self.parse_equation_env(input, env_name),
            "multline" | "multline*" => self.parse_multline_env(input, env_name),
            // mathtools `multlined`: paragraph-style multline. Skip optional
            // `[pos]` / `[width]` brackets, then lay out like `multline`.
            "multlined" | "multlined*" => {
                let mut inp = input;
                for _ in 0..2 {
                    let (i, _) = ws(inp)?;
                    match i
                        .strip_prefix('[')
                        .and_then(|r| r.find(']').map(|e| &r[e + 1..]))
                    {
                        Some(after) => inp = after,
                        None => break,
                    }
                }
                self.parse_multline_env(inp, env_name)
            },
            "split" => self.parse_align_env(input, env_name),
            _ => self.parse_array_env(input, env_name),
        }
    }

    fn parse_array_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, columns, col_bg) = 'col: {
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
                        // Extract `>{\columncolor{..}}` column backgrounds and
                        // strip them so the alignment spec parses cleanly.
                        let (cleaned, col_bg) = self.extract_column_colors(spec);
                        let (aligns, col_seps) = match parse_column_spec(&cleaned) {
                            Ok(parsed) => parsed,
                            Err(error) => {
                                self.abort = Some(error);
                                return Err(nom::Err::Failure(nom::error::Error::new(
                                    input,
                                    nom::error::ErrorKind::TooLarge,
                                )));
                            },
                        };
                        // LaTeX `array` adds `\arraycolsep` on both outer edges
                        // unless the preamble suppresses it with a leading `@{}`
                        // (as in `@{}cc@{}`).
                        let array_style = !cleaned.trim_start().starts_with('@');
                        break 'col (rest, (aligns, col_seps, array_style), col_bg);
                    }
                }
            }
            (input, (SmallVec::new(), 0u32, true), Vec::new())
        };

        let (input, (mut rows, row_seps)) = self.parse_table_body_with_seps(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        // Apply `\rowcolors` alternating rows and `\columncolor` backgrounds.
        self.apply_table_colors(&mut rows, &col_bg);

        Ok((
            input,
            self.build_array_with_seps_rows(&columns.0, columns.1, row_seps, columns.2, rows),
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
            let content = self.arena.push_children_deferred(&[array_id]);
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
            self.ensure_node_budget(rest)?;
            let tokens = cd_tokenize_row(row_str);
            if tokens.is_empty() {
                continue;
            }
            // Place tokens into alternating object/connector columns.
            let mut placed: Vec<(usize, SmallVec<[NodeId; 8]>)> = Vec::with_capacity(tokens.len());
            let mut col = 0usize;
            for tok in &tokens {
                self.ensure_node_budget(rest)?;
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
        let array = self.build_array_with_spacing(&columns, rows, 150, false, 0, 0, false);
        Ok((rest, array))
    }

    /// Parse one `CD` token (object text or `@`-connector) into a cell.
    fn parse_cd_cell(&mut self, tok: &CdToken<'_>) -> SmallVec<[NodeId; 8]> {
        if let Some(conn) = tok.text.strip_prefix('@') {
            self.parse_cd_connector(conn)
        } else {
            match self.parse_node_list(tok.text) {
                Ok((_, ids)) => ids,
                Err(_) => {
                    self.check_node_limit();
                    SmallVec::new()
                },
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
            // `@|` / `@\vert` / `@\Vert` vertical line. `\vert` is a *single*
            // bar `|`; bare `@|`, `@\|` and `@\Vert` are the double bar `‖`.
            Some(b'|') | Some(b'\\') => {
                let glyph = if conn.trim() == "\\vert" {
                    '\u{007C}' // single bar |
                } else {
                    '\u{2016}' // double bar ‖
                };
                let sid = self.push_char_str(glyph);
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
            Err(_) => {
                self.check_node_limit();
                None
            },
            Ok((_, _)) => None,
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
        let node = match self.build_bussproofs(body) {
            Ok(node) => node,
            Err(error) => {
                self.abort = Some(error);
                return Err(nom::Err::Failure(nom::error::Error::new(
                    rest,
                    nom::error::ErrorKind::TooLarge,
                )));
            },
        };
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
        let node = match self.build_bussproofs(&body) {
            Ok(node) => node,
            Err(error) => {
                self.abort = Some(error);
                return Err(nom::Err::Failure(nom::error::Error::new(
                    &input[end..],
                    nom::error::ErrorKind::TooLarge,
                )));
            },
        };
        Ok((&input[end..], node))
    }

    /// Tokenize a bussproofs body into commands and run the proof-construction
    /// stack. Stack underflow and leftover entries never panic: missing
    /// premises are padded with empty rows and leftover stack entries are
    /// wrapped in a `Row`.
    fn build_bussproofs(&mut self, body: &str) -> Result<NodeId> {
        let mut stack: Vec<NodeId> = Vec::new();
        let mut right_label: Option<NodeId> = None;
        let mut left_label: Option<NodeId> = None;
        let mut line_style = BussLine::Single;
        let mut rest = body;

        while let Some(bs) = rest.find('\\') {
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
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
                    let leaf = self.buss_content(arg.unwrap_or(""))?;
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
                        &mut line_style,
                    )?;
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
                        &mut line_style,
                    )?;
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
                        &mut line_style,
                    )?;
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
                        &mut line_style,
                    )?;
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
                        &mut line_style,
                    )?;
                    rest = after;
                },
                "RightLabel" | "RL" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    right_label = Some(self.buss_content(arg.unwrap_or(""))?);
                    rest = after;
                },
                "LeftLabel" | "LL" => {
                    let (after, arg) = bussproofs_take_arg(rest);
                    left_label = Some(self.buss_content(arg.unwrap_or(""))?);
                    rest = after;
                },
                // Inference line styles (apply to the next inference).
                "noLine" => line_style = BussLine::None,
                "doubleLine" => line_style = BussLine::Double,
                "singleLine" | "solidLine" | "dottedLine" => line_style = BussLine::Single,
                // \DisplayProof / \DP take no argument; everything else is
                // skipped along with any braced argument it might carry.
                "DisplayProof" | "DP" => {},
                _ => {
                    let (after, _) = bussproofs_take_arg(rest);
                    rest = after;
                },
            }
        }

        if let Some(error) = self.take_resource_error() {
            return Err(error);
        }
        let node = match stack.len() {
            1 => stack[0],
            0 => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            _ => {
                let span = self.arena.push_children_deferred(&stack);
                self.arena.push_node(CompactNode::Row(span))
            },
        };
        if let Some(error) = self.take_resource_error() {
            return Err(error);
        }
        Ok(node)
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
        line: &mut BussLine,
    ) -> Result<()> {
        let avail = stack.len().min(arity);
        let split = stack.len() - avail;
        let mut premises: SmallVec<[NodeId; 4]> = stack.drain(split..).collect();
        while premises.len() < arity {
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
            let empty = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            premises.insert(0, empty);
        }
        let concl = self.buss_content(concl_src)?;
        let node = self.buss_make_node(&premises, concl, left.take(), right.take(), *line);
        if let Some(error) = self.take_resource_error() {
            return Err(error);
        }
        // Line style resets to the default after each inference.
        *line = BussLine::Single;
        stack.push(node);
        Ok(())
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
        line: BussLine,
    ) -> NodeId {
        let prem_row = self.buss_hstack(premises);
        let width = self
            .buss_width(prem_row)
            .max(self.buss_width(concl))
            .max(0.6)
            + 0.3;
        // Build the inference line according to the requested style.
        let make_rule = |me: &mut Self| {
            me.arena.push_node(CompactNode::Rule {
                width: rat_em(width),
                height: Rational::new(4, 100),
                lift: Rational::new(6, 100),
            })
        };
        let rule: NodeId = match line {
            // No rule: a thin vertical gap so premises sit above the conclusion.
            BussLine::None => self
                .arena
                .push_node(CompactNode::Space(Rational::new(1, 10))),
            // Double rule: two stacked thin lines.
            BussLine::Double => {
                let r1 = make_rule(self);
                let gap = self
                    .arena
                    .push_node(CompactNode::Space(Rational::new(1, 20)));
                let r2 = make_rule(self);
                let cols = [ColumnAlign::Center];
                let rows = vec![
                    vec![smallvec::smallvec![r1]],
                    vec![smallvec::smallvec![gap]],
                    vec![smallvec::smallvec![r2]],
                ];
                self.build_array(&cols, rows)
            },
            BussLine::Single => make_rule(self),
        };
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
    fn buss_content(&mut self, src: &str) -> Result<NodeId> {
        let mut s = src.trim();
        if let Some(inner) = s.strip_prefix('$').and_then(|x| x.strip_suffix('$')) {
            s = inner.trim();
        } else if let Some(inner) = s.strip_prefix("\\(").and_then(|x| x.strip_suffix("\\)")) {
            s = inner.trim();
        }
        if s.is_empty() {
            let node = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
            return Ok(node);
        }
        match self.parse_node_list(s) {
            Ok((_, ids)) => {
                if !ids.is_empty() {
                    let node = self.flatten_single(&ids);
                    if let Some(error) = self.take_resource_error() {
                        return Err(error);
                    }
                    Ok(node)
                } else {
                    let node = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    if let Some(error) = self.take_resource_error() {
                        return Err(error);
                    }
                    Ok(node)
                }
            },
            Err(_) => {
                if let Some(error) = self.take_resource_error() {
                    return Err(error);
                }
                let node = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                if let Some(error) = self.take_resource_error() {
                    return Err(error);
                }
                Ok(node)
            },
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
        let node = match self.build_xymatrix(body) {
            Ok(node) => node,
            Err(error) => {
                self.abort = Some(error);
                return Err(nom::Err::Failure(nom::error::Error::new(
                    rest,
                    nom::error::ErrorKind::TooLarge,
                )));
            },
        };
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
                Some((_, inner)) => match self.build_xymatrix(inner) {
                    Ok(node) => node,
                    Err(error) => {
                        self.abort = Some(error);
                        return Err(nom::Err::Failure(nom::error::Error::new(
                            rest,
                            nom::error::ErrorKind::TooLarge,
                        )));
                    },
                },
                None => self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            }
        } else {
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
        };
        Ok((rest, node))
    }

    /// Build a centered [`CompactNode::Array`] grid from an xymatrix body,
    /// rendering `\ar[dir]` commands as directional arrow glyphs.
    fn build_xymatrix(&mut self, body: &str) -> Result<NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::new();
        let mut num_cols = 0usize;
        for row_src in xy_split_rows(body) {
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
            let cells_src = xy_split_cells(row_src);
            let mut row: Vec<SmallVec<[NodeId; 8]>> = Vec::with_capacity(cells_src.len());
            for cell_src in &cells_src {
                row.push(self.parse_xy_cell(cell_src)?);
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
        let node = self.build_array_with_spacing(&columns, rows, 130, false, 0, 0, false);
        if let Some(error) = self.take_resource_error() {
            return Err(error);
        }
        Ok(node)
    }

    /// Parse one xymatrix cell: leading object content followed by any
    /// `\ar[dir]^{..}_{..}` arrows rendered as glyphs.
    fn parse_xy_cell(&mut self, cell: &str) -> Result<SmallVec<[NodeId; 8]>> {
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        let head_end = xy_find_ar(cell);
        let head = cell[..head_end].trim();
        if !head.is_empty() {
            match self.parse_node_list(head) {
                Ok((_, ids)) => out.extend(ids),
                Err(_) => {
                    if let Some(error) = self.take_resource_error() {
                        return Err(error);
                    }
                },
            }
        }
        let mut rest = &cell[head_end..];
        while rest.starts_with("\\ar") {
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
            rest = &rest[3..];
            // Skip optional `\ar` shaft/curve/offset modifiers (`@{->}`,
            // `@/^/`, `@(u,d)`, `@<1ex>`, `@2`, …) — the arrow direction is
            // still taken from the following `[dir]`.
            rest = xy_skip_ar_modifiers(rest);
            let (after_dir, dir) = xy_take_bracket(rest);
            rest = after_dir;
            let mut over = None;
            let mut under = None;
            loop {
                let t = rest.trim_start();
                if let Some(r) = t.strip_prefix('^') {
                    let (after, lab) = xy_take_script(r);
                    over = self.xy_label(lab)?;
                    rest = after;
                } else if let Some(r) = t.strip_prefix('_') {
                    let (after, lab) = xy_take_script(r);
                    under = self.xy_label(lab)?;
                    rest = after;
                } else {
                    rest = t;
                    break;
                }
            }
            out.push(self.xy_arrow(dir, over, under));
            if let Some(error) = self.take_resource_error() {
                return Err(error);
            }
            let next = xy_find_ar(rest);
            let between = rest[..next].trim();
            if !between.is_empty() {
                match self.parse_node_list(between) {
                    Ok((_, ids)) => out.extend(ids),
                    Err(_) => {
                        if let Some(error) = self.take_resource_error() {
                            return Err(error);
                        }
                    },
                }
            }
            rest = &rest[next..];
        }
        if let Some(error) = self.take_resource_error() {
            return Err(error);
        }
        Ok(out)
    }

    /// Parse an arrow label into a single node, or `None` when empty.
    fn xy_label(&mut self, src: &str) -> Result<Option<NodeId>> {
        let s = src.trim();
        if s.is_empty() {
            return Ok(None);
        }
        match self.parse_node_list(s) {
            Ok((_, ids)) => {
                if ids.is_empty() {
                    Ok(None)
                } else {
                    let node = self.flatten_single(&ids);
                    if let Some(error) = self.take_resource_error() {
                        return Err(error);
                    }
                    Ok(Some(node))
                }
            },
            Err(_) => {
                if let Some(error) = self.take_resource_error() {
                    return Err(error);
                }
                Ok(None)
            },
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
            false,
        );
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children_deferred(&[array_id]);
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
            false,
        );
        let open = self.arena.push_str("");
        let close = self.arena.push_str("}");
        let content = self.arena.push_children_deferred(&[array_id]);
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
    /// equation-number column. Since this is a single-formula converter with no
    /// document-wide counter, the master number starts at `1`:
    ///
    /// * `numcases` numbers each row `(1)`, `(2)`, `(3)`, … (one master number
    ///   per case row).
    /// * `subnumcases` shares a single master number `1` and increments the
    ///   sub-letter per row: `(1a)`, `(1b)`, `(1c)`, ….
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

        let sub = env_name == "subnumcases";

        // Append an upright parenthesized equation-number cell to each row.
        let mut numbered_rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::with_capacity(rows.len());
        for (i, mut row) in rows.into_iter().enumerate() {
            self.ensure_node_budget(input)?;
            let label = if sub {
                // Shared master number 1, incrementing sub-letter per row.
                let letter = char::from(b'a' + (i % 26) as u8);
                format!("(1{letter})")
            } else {
                // One master number per row: (1), (2), (3), …
                format!("({})", i + 1)
            };
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
            false,
        );
        self.ensure_node_budget(input)?;
        // Big left brace, no right delimiter (cases style).
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children_deferred(&[array_id]);
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
        let row_span = self.arena.push_children_deferred(&[left_id, eq_id, fenced]);
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
            // Scan to the top-level closing `]` (brace-depth aware, skipping
            // backslash-escaped braces such as `\{` in `left=\{`).
            let mut depth = 0i32;
            let mut end = None;
            let mut escaped = false;
            for (i, c) in after.char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                match c {
                    '\\' => escaped = true,
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
            let content = self.arena.push_children_deferred(&[node]);
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
        let (input, mut rows) = self.parse_table_body(input)?;
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

        self.apply_table_cell_style(&mut rows, 2);
        Ok((
            input,
            self.build_array_with_spacing(&columns, rows, 50, false, 0, 0, false),
        ))
    }

    fn parse_align_env<'a>(&mut self, input: &'a str, env_name: &str) -> IResult<&'a str, NodeId> {
        let (input, mut rows) = self.parse_table_body(input)?;
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

        self.apply_table_cell_style(&mut rows, 0);
        Ok((
            input,
            self.build_array_with_spacing(&columns, rows, 100, true, 0, 0, false),
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
        let (input, mut rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        self.apply_table_cell_style(&mut rows, 0);
        Ok((
            input,
            self.build_array_with_spacing(&[ColumnAlign::Center], rows, 100, true, 0, 0, false),
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

        let ids = self.resolve_infix_fraction(ids);
        Ok((input, self.flatten_single(&ids)))
    }

    fn parse_multline_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, mut rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;
        self.apply_table_cell_style(&mut rows, 0);

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
            self.ensure_node_budget(rest)?;
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, (extra, hline))) => {
                    // Apply \\[dim] extra inter-row space by adding an invisible
                    // depth strut to the row just completed.
                    if let Some(dim_str) = extra
                        && let Some(row) = rows.last_mut()
                        && let Some(cell) = row.last_mut()
                        && let Ok((_, d)) = dimension::width(dim_str)
                        && d.num > 0
                    {
                        let strut = self.arena.push_node(CompactNode::Rule {
                            width: Rational::zero(),
                            height: d,
                            lift: Rational::new(-d.num, d.den),
                        });
                        cell.push(strut);
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
        if rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(|c| c.is_empty())) {
            let last = rows.len();
            // row_seps is a u32, so only row indices < 32 can carry a bit.  The
            // hline setter above already caps rows.len() < 32, but a pathological
            // input can produce more rows than that; guard the shift to avoid a
            // left-shift overflow panic.
            if last < 32 && row_seps & (1 << last) != 0 {
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
            self.ensure_node_budget(rest)?;
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
            if self.check_node_limit() {
                break;
            }
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
        let span = self.arena.push_children_deferred(&ids);
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
            self.ensure_node_budget(rest)?;
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
        Ok((rest, self.resolve_infix_fraction(ids)))
    }

    fn parse_cell_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Stop at &, \\, \cr, or \end
        if input.starts_with('&')
            || input.starts_with("\\\\")
            || strip_control_word(input, "end").is_some()
            || is_cr_row_separator(input)
            || input.is_empty()
        {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Eof,
            )));
        }
        // Handle leading ^/_ /' with implicit empty base in table cells
        if input.starts_with('^') || input.starts_with('_') || input.starts_with('\'') {
            self.ensure_node_budget(input)?;
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
            if self.check_node_limit() {
                break;
            }
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
                Err(error) => {
                    // A scoped declaration can fail after consuming a large
                    // prefix (for example, a deeply nested legacy-font
                    // reproducer). Resume at the parser's error cursor when
                    // it is a strict suffix of the current input; retrying
                    // from the first byte would repeatedly rebuild the same
                    // discarded prefix until the node budget is exhausted.
                    let error_input = match error {
                        nom::Err::Error(inner) | nom::Err::Failure(inner) => inner.input,
                        nom::Err::Incomplete(_) => input,
                    };
                    if let Some(consumed) = input.strip_suffix(error_input)
                        && !consumed.is_empty()
                    {
                        let sid = self.arena.push_str(consumed);
                        ids.push(
                            self.arena
                                .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                        );
                        diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: "treated malformed scoped input as literal text".into(),
                            span: None,
                        });
                        input = error_input;
                        continue;
                    }
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
                        if self.check_node_limit() {
                            break;
                        }
                    } else {
                        break;
                    }
                },
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::{LatexCompactParser, consume_limit_modifiers};
    use crate::ast::LimitStyle;
    use crate::error::Error;

    #[test]
    fn limit_modifier_scan_preserves_unmatched_newline() {
        let (rest, style) = consume_limit_modifiers("\\limits\nx");
        assert_eq!(style, Some(LimitStyle::AboveBelow));
        assert_eq!(rest, "\nx");
    }

    #[test]
    fn node_limit_stops_array_materialization_at_the_next_node() {
        let input = r"\begin{matrix}a&b\\c&d\end{matrix}";
        let mut parser = LatexCompactParser::with_capacity(input.len(), Some(2));
        let result = parser.parse_node_list(input);

        assert!(matches!(result, Err(nom::Err::Failure(_))));
        assert_eq!(parser.arena.node_count(), 3);
        assert!(matches!(
            parser.abort,
            Some(Error::LimitExceeded {
                kind: crate::error::LimitKind::Nodes,
                limit: 2
            })
        ));
    }

    #[test]
    fn specialized_builders_stop_at_small_node_limits() {
        let cases = [
            (
                r"\begin{prooftree}\AxiomC{$A$}\AxiomC{$B$}\BinaryInfC{$C$}\end{prooftree}",
                7,
            ),
            (r"\xymatrix{A\ar[r]^B & C\\D & E}", 5),
            (r"\begin{numcases}{f(x)=}x&x>0\\-x&x\le0\end{numcases}", 5),
        ];
        for (input, expected_nodes) in cases {
            let mut parser = LatexCompactParser::with_capacity(input.len(), Some(4));
            let result = parser.parse_node_list(input);
            assert!(matches!(result, Err(nom::Err::Failure(_))));
            assert!(matches!(
                parser.abort,
                Some(Error::LimitExceeded {
                    kind: crate::error::LimitKind::Nodes,
                    limit: 4
                })
            ));
            assert_eq!(parser.arena.node_count(), expected_nodes);
        }
    }
}
