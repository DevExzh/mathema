use nom::IResult;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::complete::char;
use smallvec::SmallVec;

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
use super::preprocess::preprocess;

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

fn classify_char(c: char) -> SymbolKind {
    match c {
        '+' | '-' | '*' => SymbolKind::BinaryOp,
        '/' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '!' => SymbolKind::Relation,
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
            b'{' | b'}' => { i += 1; } // strip grouping braces
            b'~' => { out.push('\u{00A0}'); i += 1; } // non-breaking space
            b'\\' if i + 1 < len => {
                match bytes[i + 1] {
                    b' ' => { out.push(' '); i += 2; }
                    b'&' => { out.push('&'); i += 2; }
                    b'#' => { out.push('#'); i += 2; }
                    b'$' => { out.push('$'); i += 2; }
                    b'%' => { out.push('%'); i += 2; }
                    b'_' => { out.push('_'); i += 2; }
                    b'\\' => { out.push(' '); i += 2; } // \\ = line break → space
                    b',' => { out.push('\u{2009}'); i += 2; } // thin space
                    b';' => { out.push('\u{2005}'); i += 2; } // medium space
                    _ => {
                        // Skip unknown commands: consume \word
                        i += 1; // skip backslash
                        let start = i;
                        while i < len && bytes[i].is_ascii_alphabetic() { i += 1; }
                        if i == start {
                            // Single non-alpha char after \, pass through
                            out.push(bytes[i] as char);
                            i += 1;
                        }
                        // Otherwise silently drop the command name
                    }
                }
            }
            b'-' if i + 2 < len && bytes[i + 1] == b'-' && bytes[i + 2] == b'-' => {
                out.push('\u{2014}'); // em-dash
                i += 3;
            }
            b'-' if i + 1 < len && bytes[i + 1] == b'-' => {
                out.push('\u{2013}'); // en-dash
                i += 2;
            }
            b'`' if i + 1 < len && bytes[i + 1] == b'`' => {
                out.push('\u{201C}'); // left double quote
                i += 2;
            }
            b'\'' if i + 1 < len && bytes[i + 1] == b'\'' => {
                out.push('\u{201D}'); // right double quote
                i += 2;
            }
            _ => {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    out
}

fn parse_column_spec(spec: &str) -> SmallVec<[ColumnAlign; 4]> {
    let mut result = SmallVec::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'l' => result.push(ColumnAlign::Left),
            'c' => result.push(ColumnAlign::Center),
            'r' => result.push(ColumnAlign::Right),
            '*' => {
                // *{n}{pattern} — repeat pattern n times
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut n_str = String::new();
                    for ch in chars.by_ref() {
                        if ch == '}' { break; }
                        n_str.push(ch);
                    }
                    let n: usize = n_str.trim().parse().unwrap_or(1);
                    if chars.peek() == Some(&'{') {
                        chars.next(); // consume '{'
                        let mut pattern = String::new();
                        let mut depth = 1u32;
                        for ch in chars.by_ref() {
                            if ch == '{' { depth += 1; }
                            if ch == '}' {
                                depth -= 1;
                                if depth == 0 { break; }
                            }
                            pattern.push(ch);
                        }
                        let sub = parse_column_spec(&pattern);
                        for _ in 0..n {
                            result.extend_from_slice(&sub);
                        }
                    }
                }
            },
            _ => {}, // skip |, :, and other decorations
        }
    }
    result
}

fn parse_row_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    // Accept \\ or \cr as row separator
    let input = if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\cr")(input) {
        // \cr must not be followed by alphabetic (to avoid matching \cross etc.)
        if rest.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic()) {
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
    // Optionally consume [dimension] like [1ex], [3pt]
    let input = if input.starts_with('[') {
        if let Some(end) = input.find(']') {
            &input[end + 1..]
        } else {
            input
        }
    } else {
        input
    };
    let (input, _) = ws(input)?;
    // Consume any \hline or \hdashline directives that follow
    let input = skip_table_directives(input);
    Ok((input, ()))
}

/// Skip `\hline` and `\hdashline` table directives (silently consumed).
fn skip_table_directives(mut input: &str) -> &str {
    loop {
        let trimmed = input.trim_start();
        if let Some(rest) = trimmed.strip_prefix("\\hline") {
            // Ensure it's not a prefix of a longer command
            if rest.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic()) {
                break;
            }
            input = rest;
        } else if let Some(rest) = trimmed.strip_prefix("\\hdashline") {
            if rest.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic()) {
                break;
            }
            input = rest;
        } else {
            break;
        }
    }
    input
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
    let processed = preprocess(trimmed);
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
            let (r, _) = ws(rest)?;
            rest = r;
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
        // Handle leading ^/_ with implicit empty base (e.g., {^3+1})
        if input.starts_with('^') || input.starts_with('_') {
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
    #[inline]
    fn parse_scripts<'a>(&mut self, input: &'a str, base: NodeId) -> IResult<&'a str, NodeId> {
        // Skip whitespace, then check for ^ or _ via byte inspection
        let skip = skip_ws_bytes(input.as_bytes());
        let input = &input[skip..];
        let first = match input.as_bytes().first() {
            Some(b'^') => '^',
            Some(b'_') => '_',
            _ => return Ok((input, base)),
        };
        let input = &input[1..];

        let (input, first_arg) = self.parse_script_arg(input)?;

        // Check for the complementary script
        let skip2 = skip_ws_bytes(input.as_bytes());
        let input2 = &input[skip2..];
        let second_ch = if first == '^' { b'_' } else { b'^' };
        let has_second = input2.as_bytes().first() == Some(&second_ch);

        if !has_second {
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

        // The body is the next atom (or empty row if nothing follows).
        // Allow scripts on the body: \int_0^1 x^2 → body is x^2, not just x.
        let (input, body_id) = match self.parse_atom(input) {
            Ok((rest, id)) => self.parse_scripts(rest, id)?,
            Err(_) => {
                let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                (input, row_id)
            },
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
        let skip = skip_ws_bytes(input.as_bytes());
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
                Ok((&input[1..], self.arena.push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid))))
            },
            // Non-ASCII: Unicode letters → identifiers, other → symbols
            Some(&b) if b > 127 => {
                let ch = input.chars().next().unwrap();
                let len = ch.len_utf8();
                let sid = self.arena.push_str(&input[..len]);
                if ch.is_alphabetic() {
                    Ok((&input[len..], self.arena.push_node(CompactNode::Identifier(sid))))
                } else {
                    Ok((&input[len..], self.arena.push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid))))
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
        let sid = self.push_char_str(c);
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
                return Ok((rest, self.arena.push_node(CompactNode::Space(Rational::zero()))));
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
            "hat" | "widehat" => self.parse_accent(input, '\u{0302}', AccentPosition::Above),
            "bar" => self.parse_accent(input, '\u{0304}', AccentPosition::Above),
            "overline" => self.parse_overunder(input, true),
            "underline" | "underbar" => self.parse_overunder(input, false),
            "vec" => self.parse_accent(input, '\u{20D7}', AccentPosition::Above),
            "dot" => self.parse_accent(input, '\u{0307}', AccentPosition::Above),
            "ddot" => self.parse_accent(input, '\u{0308}', AccentPosition::Above),
            "dddot" => self.parse_accent(input, '\u{20DB}', AccentPosition::Above),
            "ddddot" => self.parse_accent(input, '\u{20DC}', AccentPosition::Above),
            "tilde" | "widetilde" => self.parse_accent(input, '\u{0303}', AccentPosition::Above),
            "breve" => self.parse_accent(input, '\u{0306}', AccentPosition::Above),
            "check" | "widecheck" => self.parse_accent(input, '\u{030C}', AccentPosition::Above),
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
            "text" | "textrm" | "textit" | "textbf" | "texttt" | "textsf"
            | "textnormal" | "textup" | "mbox" | "hbox" => {
                self.parse_text(input, name)
            },
            "mathrm" | "mathbf" | "mathit" | "mathtt" | "mathsf" | "mathbb" | "mathcal"
            | "mathscr" | "mathfrak" | "boldsymbol" | "bm" | "mathnormal" | "mathup"
            | "mathsfup" | "mathsfit" | "mathbfup" | "mathbfit" | "mathbfcal"
            | "mathbffrak" | "mathbfscr" | "mathbfsfit" | "mathbfsfup" => {
                self.parse_math_font(input, name)
            },

            // --- Decorators ---
            "phantom" | "vphantom" | "hphantom" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                Ok((input, self.arena.push_node(CompactNode::Phantom(arg_id))))
            },
            "mathstrut" => {
                // \mathstrut — zero-width strut; emit as phantom of "("
                let sid = self.arena.push_str("(");
                let inner = self.arena.push_node(CompactNode::Symbol(SymbolKind::Open, sid));
                Ok((input, self.arena.push_node(CompactNode::Phantom(inner))))
            },
            // \ensuremath{content} — transparent wrapper; content already in math mode
            "ensuremath" => self.parse_required_group(input),
            "smash" => {
                // \smash[b]{...} or \smash{...} — optional [b]/[t] arg
                let (input, _) = ws(input)?;
                let input = if input.starts_with('[') {
                    if let Some(end) = input.find(']') {
                        &input[end + 1..]
                    } else {
                        input
                    }
                } else {
                    input
                };
                self.parse_required_group(input)
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
                                if depth == 0 { end = i + 1; break; }
                            },
                            _ => {},
                        }
                    }
                    &inner[end..]
                } else {
                    input
                };
                Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
            },
            "boxed" | "fbox" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                Ok((input, self.arena.push_node(CompactNode::Boxed(arg_id))))
            },
            "cancel" => self.parse_cancel(input, StrokeKind::Forward),
            "bcancel" => self.parse_cancel(input, StrokeKind::Backward),
            "xcancel" => self.parse_cancel(input, StrokeKind::Cross),

            // --- Environments ---
            "begin" => self.parse_environment(input),

            // --- Overset / Underset ---
            "overset" | "stackrel" => self.parse_overset(input),
            "underset" => self.parse_underset(input),

            // --- Operatorname ---
            "operatorname" => self.parse_operatorname(input),

            // --- Additional binomials ---
            "dbinom" => self.parse_styled_binom(input, FractionKind::Display),
            "tbinom" => self.parse_styled_binom(input, FractionKind::Inline),

            // --- Math atom type wrappers ---
            "mathop" | "mathrel" | "mathbin" | "mathord" | "mathpunct" | "mathopen"
            | "mathclose" | "mathinner" => self.parse_required_group(input),

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
                Ok((input, self.arena.push_node(CompactNode::Symbol(SymbolKind::Fence, sid))))
            },

            // --- Color (consume color arg, return content) ---
            "color" => self.parse_color_discard(input),
            "textcolor" => self.parse_textcolor(input),

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
            "sout" => self.parse_cancel(input, StrokeKind::Cross),

            // --- Poor man's bold (render as bold font) ---
            "pmb" => self.parse_math_font(input, "mathbf"),

            // --- Raisebox (consume dimension, return content) ---
            "raisebox" => self.parse_raisebox(input),

            // --- Centernot (negation overlay) ---
            "centernot" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                // Overlay a combining solidus on the argument
                let slash_sid = self.arena.push_str("\u{0338}");
                let slash_id = self.arena.push_node(CompactNode::Symbol(SymbolKind::Ordinary, slash_sid));
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
                            if depth == 0 { end = i; break; }
                        },
                        _ => {},
                    }
                }
                let text = &input[..end];
                let rest = &input[end + 1..];
                let sid = self.arena.push_str(text);
                Ok((rest, self.arena.push_node(CompactNode::Text(TextStyle::Monospace, sid))))
            },

            // --- HTML attributes (transparent wrappers) ---
            "htmlId" | "htmlClass" | "htmlData" | "htmlStyle" => {
                // \htmlId{id}{content} — consume attribute arg, return content
                let (input, _attr_id) = self.parse_required_group(input)?;
                self.parse_required_group(input)
            },

            // --- DeclareMathOperator (consume and discard) ---
            "DeclareMathOperator" | "newcommand" | "renewcommand" | "providecommand"
            | "newenvironment" | "renewenvironment" => {
                self.parse_declaration(input)
            },

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
                Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
            },

            // --- Logo text commands ---
            "TeX" => {
                let sid = self.arena.push_str("TeX");
                Ok((input, self.arena.push_node(CompactNode::Text(TextStyle::Normal, sid))))
            },
            "LaTeX" => {
                let sid = self.arena.push_str("LaTeX");
                Ok((input, self.arena.push_node(CompactNode::Text(TextStyle::Normal, sid))))
            },
            "KaTeX" => {
                let sid = self.arena.push_str("KaTeX");
                Ok((input, self.arena.push_node(CompactNode::Text(TextStyle::Normal, sid))))
            },

            // --- Idotsint (integral with dots) ---
            "idotsint" => {
                let int_sid = self.push_char_str('\u{222B}');
                let dots_sid = self.push_char_str('\u{22EF}');
                let int1 = self.arena.push_node(CompactNode::Symbol(SymbolKind::LargeOp, int_sid));
                let dots = self.arena.push_node(CompactNode::Symbol(SymbolKind::Ordinary, dots_sid));
                let int_sid2 = self.push_char_str('\u{222B}');
                let int2 = self.arena.push_node(CompactNode::Symbol(SymbolKind::LargeOp, int_sid2));
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
            "rm" | "it" | "bf" | "sf" | "tt" => {
                self.parse_old_font_switch(input, name)
            },

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
            "hline" | "hdashline" => {
                Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
            },

            // --- Label (consume and discard) ---
            "label" => {
                let (input, _) = self.parse_required_group(input)?;
                Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
            },

            // --- Mathchoice: pick first (display) argument ---
            "mathchoice" => {
                // \mathchoice{D}{T}{S}{SS} — pick display argument, consume rest
                let (input, display_id) = self.parse_required_group(input)?;
                let (input, _text_id) = self.parse_required_group(input)?;
                let (input, _script_id) = self.parse_required_group(input)?;
                let (input, _sscript_id) = self.parse_required_group(input)?;
                Ok((input, display_id))
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
                    let line_id = self.arena
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
                    Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
                }
            },

            // --- Phase angle ---
            "phase" => {
                // \phase{angle} — render as underline with angle content
                let (input, content_id) = self.parse_required_group(input)?;
                let angle_sid = self.arena.push_str("\u{2220}");
                let angle_id = self.arena
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
            "cr" => {
                Ok((input, self.arena.push_node(CompactNode::Space(Rational::zero()))))
            },

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
            // Line break / no-op commands consumed silently
            "newline" | "nonumber" | "notag" | "nobreak" | "allowbreak" | "goodbreak"
            | "limits" | "nolimits" => {
                Some(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)))
            },
            // Font sizing commands — meaningless in math mode; consume silently
            "Huge" | "huge" | "LARGE" | "Large" | "large" | "normalsize" | "small"
            | "footnotesize" | "scriptsize" | "tiny" => {
                Some(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)))
            },
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
                Ok((&input[1..], self.arena.push_node(CompactNode::Identifier(sid))))
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
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Accent {
                base: base_id,
                accent: accent_char,
                position,
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
                ids.push(
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Fence, sid)),
                );
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
        } else if input.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic()) {
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
            self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits,
            }),
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
        if i == digit_start && !(digit_start > 0 && (bytes[digit_start - 1] == b'+' || bytes[digit_start - 1] == b'-')) {
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
            return Ok((after, self.arena.push_node(CompactNode::Space(Rational::zero()))));
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
                Ok((input, self.arena.push_node(CompactNode::Space(Rational::zero()))))
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
        Ok((rest, self.arena.push_node(CompactNode::Space(Rational::zero()))))
    }

    /// `\rule[lift]{width}{height}` — parse dimensions and emit a filled rectangle.
    fn parse_rule<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional [lift] — consume but ignore
        let input = if input.starts_with('[') {
            if let Some(end) = input.find(']') {
                &input[end + 1..]
            } else {
                input
            }
        } else {
            input
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
        let width = wp.parse_dimension(width_str).map(|(_, r)| r).unwrap_or(Rational::zero());

        // Required {height}
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let height_end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let height_str = &input[..height_end];
        let input = &input[height_end + 1..];
        let mut hp = LatexCompactParser::with_capacity(height_str.len());
        let height = hp.parse_dimension(height_str).map(|(_, r)| r).unwrap_or(Rational::zero());

        Ok((input, self.arena.push_node(CompactNode::Rule { width, height })))
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
                Ok((after_sep, ())) => {
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
        Ok((input, self.build_array(&[ColumnAlign::Center], table_rows)))
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
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let color_sid = self.arena.push_str(color);
        // \color{X}{content} — if followed by braced group, parse just that group.
        // Otherwise, parse remaining elements but stop at cell/row/env boundaries
        // to avoid consuming past table structure (e.g., \\, &, \end).
        let (input, _) = ws(input)?;
        if input.starts_with('{') {
            let (input, content_id) = self.parse_braced_group(input)?;
            Ok((
                input,
                self.arena
                    .push_node(CompactNode::Color { color: color_sid, content: content_id }),
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
                self.arena
                    .push_node(CompactNode::Color { color: color_sid, content: content_id }),
            ))
        }
    }

    fn parse_textcolor<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let color_sid = self.arena.push_str(color);
        let (input, content_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Color { color: color_sid, content: content_id }),
        ))
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
                self.arena
                    .push_node(CompactNode::Color { color: color_sid, content: content_id }),
            ))
        } else {
            // Consume the next single token
            match self.parse_single_token(input) {
                Ok((rest, content_id)) => Ok((
                    rest,
                    self.arena
                        .push_node(CompactNode::Color { color: color_sid, content: content_id }),
                )),
                Err(_) => {
                    // No content follows; just emit an empty node
                    let empty = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    Ok((
                        input,
                        self.arena
                            .push_node(CompactNode::Color { color: color_sid, content: empty }),
                    ))
                },
            }
        }
    }

    /// `\colorbox{color}{content}` — consume color arg, emit content in a box.
    fn parse_colorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        Ok((input, self.arena.push_node(CompactNode::Boxed(content_id))))
    }

    /// `\fcolorbox{framecolor}{bgcolor}{content}` — consume two color args, emit content in a box.
    fn parse_fcolorbox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _frame_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _bg_color) = take_while(|c: char| c != '}')(input)?;
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        Ok((input, self.arena.push_node(CompactNode::Boxed(content_id))))
    }

    /// `\raisebox{dimension}{content}` — consume dimension, emit content.
    fn parse_raisebox<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Consume {dimension}
        let (input, _) = char('{')(input)?;
        let end = input.find('}').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let rest = &input[end + 1..];
        // Optional [height][depth] arguments
        let rest = if rest.starts_with('[') {
            if let Some(e) = rest.find(']') {
                let r = &rest[e + 1..];
                if r.starts_with('[') {
                    if let Some(e2) = r.find(']') {
                        &r[e2 + 1..]
                    } else { r }
                } else { r }
            } else { rest }
        } else { rest };
        self.parse_required_group(rest)
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
            Ok((rest, self.arena.push_node(CompactNode::Text(TextStyle::Monospace, sid))))
        } else {
            // No closing delimiter found; consume to end
            let sid = self.arena.push_str(after_delim);
            Ok(("", self.arena.push_node(CompactNode::Text(TextStyle::Monospace, sid))))
        }
    }

    /// Old-style font switches: `\rm`, `\it`, `\bf`, `\sf`, `\tt`.
    /// These apply to the rest of the current group.
    fn parse_old_font_switch<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, NodeId> {
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
        if input.is_empty() || input.starts_with('}') || input.starts_with('&')
            || input.starts_with("\\\\") || input.starts_with("\\end")
        {
            return Ok((input, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))));
        }
        // Parse remaining content in current group
        let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            if rest.is_empty() || rest.starts_with('}') || rest.starts_with('&')
                || rest.starts_with("\\\\") || rest.starts_with("\\end")
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
            return Ok((rest, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))));
        }
        let content_span = self.arena.push_children(&ids);
        Ok((rest, self.arena.push_node(CompactNode::Styled(style, content_span))))
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
        Ok((rest, self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))))
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
            "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Bmatrix" | "Vmatrix"
            | "matrix*" | "pmatrix*" | "bmatrix*" | "vmatrix*" | "Bmatrix*" | "Vmatrix*" => {
                self.parse_matrix_env(input, env_name)
            },
            "cases" => self.parse_cases_env(input),
            "aligned" | "align" | "align*" => self.parse_align_env(input, env_name),
            "alignedat" => self.parse_alignedat_env(input, env_name),
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
                        '}' => { end = Some(i); break; },
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
            (input, SmallVec::new())
        };

        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, self.build_array(&columns, rows)))
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

    fn parse_cases_env<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag("cases")(input)?;
        let (input, _) = char('}')(input)?;

        let array_id = self.build_array(&[ColumnAlign::Left, ColumnAlign::Left], rows);
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

        Ok((input, self.build_array(&columns, rows)))
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
        let input = if let Ok((r, _)) =
            char::<_, nom::error::Error<&str>>('{')(input)
        {
            let (r, _) =
                take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(r)
                    .unwrap_or((r, ""));
            let (r, _) = char::<_, nom::error::Error<&str>>('}')(r)
                .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;
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

        Ok((input, self.build_array(&[ColumnAlign::Center], rows)))
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
        let mut rows = Vec::with_capacity(4);
        // Skip leading \hline / \hdashline before first row
        let mut rest = skip_table_directives(input);
        let (r, first) = self.parse_table_row(rest)?;
        rows.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, ())) => {
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
        Ok((rest, rows))
    }

    fn parse_table_row<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Vec<SmallVec<[NodeId; 8]>>> {
        let mut cells = Vec::with_capacity(4);
        let mut rest = input;
        let (r, first) = self.parse_table_cell(rest)?;
        cells.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_cell_separator(rest) {
                Ok((after_sep, ())) => {
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
        Ok((rest, cells))
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
        // Handle leading ^/_ with implicit empty base in table cells
        if input.starts_with('^') || input.starts_with('_') {
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
