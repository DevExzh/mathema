use std::collections::HashMap;

use nom::IResult;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::satisfy;
use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, LimitKind, ParseResult, Result,
    Strictness,
};
use crate::reader::Reader;

pub struct UnicodeMathReader;

/// Maximum bytes to scan for a matching `)` in script and radical arguments.
///
/// The bound prevents malformed, unbalanced suffixes from being rescanned
/// recursively while retaining normal parenthesized UnicodeMath arguments.
const SCRIPT_PAREN_LOOKAHEAD: usize = 256;

/// Return the byte index immediately after the balanced closing parenthesis.
fn matching_paren_end(input: &str) -> Option<usize> {
    debug_assert!(input.starts_with('('));
    let mut depth = 1_u32;
    for (index, character) in input.char_indices().skip(1) {
        if index > SCRIPT_PAREN_LOOKAHEAD {
            return None;
        }
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index + character.len_utf8());
                }
            },
            _ => {},
        }
    }
    None
}

impl Reader for UnicodeMathReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        config.check_input_size(input)?;
        let input = input.trim();
        if input.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                DisplayStyle::Inline,
            )));
        }
        let mut parser = CompactParser::new(input, config);
        match parser.parse_expr_list(input) {
            Ok(ids) => {
                parser.arena.check_limits()?;
                let roots = parser.arena.push_children(&ids)?;
                Ok(CompactParseResult::new(CompactExpr::new(
                    parser.arena,
                    roots,
                    DisplayStyle::Inline,
                )))
            },
            // Resource limits hold in every mode.
            Err(e @ Error::LimitExceeded { .. }) => Err(e),
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let mut arena = AstArena::new();
                    let sid = arena.push_str(input);
                    let id = arena.push_node(CompactNode::Text(TextStyle::Normal, sid));
                    let roots = arena.push_children(&[id])?;
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(arena, roots, DisplayStyle::Inline),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("UnicodeMath parse error (recovered as text): {e}"),
                            span: None,
                        }],
                    ))
                } else {
                    Err(e)
                }
            },
        }
    }
}

// ── Compact parser ────────────────────────────────────────────────────
//
// The parser is a recursive descent with ordered alternatives: when one
// alternative fails, the next is tried at the same position. Without care
// this backtracking is exponential in the nesting depth (an unclosed `(` is
// parsed in full, fails, and is parsed again by the next alternative or by
// the caller's fallback, at every level). Two rules keep it linear:
//
// * Whether an atom (or a bracket group) starting at a given offset fails
//   does not depend on how it was reached, so each failure is recorded by
//   offset and never recomputed (see `memo_failure`).
// * Every `nom::Err::Failure` means a resource limit was hit. It aborts the
//   whole parse: nothing catches it to try another alternative.
//
// Nodes built by failed alternatives stay in the arena as garbage, so the
// node limit counts every node the parse creates.

/// A recorded failure: the atom or group at an offset failed, and while
/// parsing it the element nesting reached `depth_span` levels below the
/// depth at which it started (0 when no element was entered).
type DepthSpan = u32;

struct CompactParser {
    arena: AstArena,
    /// Byte length of the whole (trimmed) input. Every slice the parser sees
    /// is a suffix of it, so `source_len - rest.len()` is the offset of `rest`.
    source_len: usize,
    /// Current element nesting depth.
    depth: u32,
    /// Deepest element nesting entered by the attempt in progress, plus one
    /// (0 when none); see [`DepthSpan`].
    deepest: u32,
    /// Element nesting limit ([`Config::effective_max_depth`]). Each nested
    /// element is a child of the node that contains it, so a deeper nesting
    /// would exceed the AST depth limit anyway.
    max_depth: u32,
    /// [`Config::max_nodes`].
    max_nodes: Option<usize>,
    /// The limit that aborted the parse, if any.
    limit: Option<Error>,
    /// Offsets at which an atom or a bracket group is known to fail. One map
    /// serves both: an atom at a bracket fails exactly when its group fails.
    failed: HashMap<u32, DepthSpan>,
    /// Byte range of a run of `.` that is known not to start a number.
    dot_run: (usize, usize),
}

/// Try a parse method; propagate `Failure`, ignore `Error`, try next.
macro_rules! try_alt {
    ($self:expr, $input:expr, $( $method:ident ),+ $(,)?) => {
        $(
            match $self.$method($input) {
                Ok(r) => return Ok(r),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {},
            }
        )+
    }
}

fn parse_error(input: &str, kind: nom::error::ErrorKind) -> nom::Err<nom::error::Error<&str>> {
    nom::Err::Error(nom::error::Error::new(input, kind))
}

fn limit_failure(input: &str) -> nom::Err<nom::error::Error<&str>> {
    nom::Err::Failure(nom::error::Error::new(
        input,
        nom::error::ErrorKind::TooLarge,
    ))
}

impl CompactParser {
    fn new(source: &str, config: &Config) -> Self {
        Self {
            arena: AstArena::new(),
            source_len: source.len(),
            depth: 0,
            deepest: 0,
            max_depth: u32::try_from(config.effective_max_depth()).unwrap_or(u32::MAX),
            max_nodes: config.max_nodes,
            limit: None,
            failed: HashMap::new(),
            dot_run: (0, 0),
        }
    }

    fn offset(&self, rest: &str) -> usize {
        self.source_len - rest.len()
    }

    /// Push a node, enforcing [`Config::max_nodes`]. Past the limit the node
    /// is still stored (callers need an id) but the parse is aborted at the
    /// next element.
    fn push_node(&mut self, node: CompactNode) -> NodeId {
        let id = self.arena.push_node(node);
        if let Some(limit) = self.max_nodes
            && self.arena.node_count() > limit
            && self.limit.is_none()
        {
            self.limit = Some(Error::LimitExceeded {
                kind: LimitKind::Nodes,
                limit,
            });
        }
        id
    }

    fn depth_limit(&mut self) {
        if self.limit.is_none() {
            self.limit = Some(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: self.max_depth as usize,
            });
        }
    }

    /// Enter one level of element nesting, or abort past the depth limit.
    fn enter<'a>(
        &mut self,
        input: &'a str,
    ) -> std::result::Result<(), nom::Err<nom::error::Error<&'a str>>> {
        if self.limit.is_some() {
            return Err(limit_failure(input));
        }
        self.deepest = self.deepest.max(self.depth + 1);
        if self.depth > self.max_depth {
            self.depth_limit();
            return Err(limit_failure(input));
        }
        self.depth += 1;
        Ok(())
    }

    /// Run `parse` at `input`, or fail at once if it is known to fail there.
    ///
    /// `parse` must depend only on the input from `input` on: a recorded
    /// `Error` is then returned again, and re-running it would only matter
    /// if it now hit the depth limit, which the recorded depth span decides
    /// without re-running it.
    fn memo_failure<'a, T>(
        &mut self,
        input: &'a str,
        parse: impl FnOnce(&mut Self, &'a str) -> IResult<&'a str, T>,
    ) -> IResult<&'a str, T> {
        let Ok(offset) = u32::try_from(self.offset(input)) else {
            return parse(self, input);
        };
        if let Some(&span) = (!self.failed.is_empty())
            .then(|| self.failed.get(&offset))
            .flatten()
        {
            if span > 0 {
                // The attempt entered elements down to `depth + span - 1`.
                let deepest = self.depth + span;
                self.deepest = self.deepest.max(deepest);
                if deepest - 1 > self.max_depth {
                    self.depth_limit();
                    return Err(limit_failure(input));
                }
            }
            return Err(parse_error(input, nom::error::ErrorKind::Alt));
        }
        let outer = std::mem::take(&mut self.deepest);
        let result = parse(self, input);
        let span = self.deepest.saturating_sub(self.depth);
        self.deepest = self.deepest.max(outer);
        if matches!(result, Err(nom::Err::Error(_))) {
            self.failed.insert(offset, span);
        }
        result
    }

    fn parse_expr_list(&mut self, input: &str) -> Result<SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut remaining = input;
        loop {
            // Whitespace separates UnicodeMath operands (it triggers "build up"
            // of the preceding subexpression); it is not literal text. Skip it
            // between elements so parsing continues past spaces.
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                break;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    ids.push(id);
                    remaining = rest;
                },
                Err(nom::Err::Failure(_)) => {
                    return Err(self
                        .limit
                        .take()
                        .unwrap_or_else(|| Error::UnicodeMathParse {
                            message: "UnicodeMath parse error".into(),
                        }));
                },
                Err(_) => break,
            }
        }
        if !remaining.trim().is_empty() {
            let sid = self.arena.push_str(remaining.trim());
            ids.push(self.push_node(CompactNode::Text(TextStyle::Normal, sid)));
        }
        if let Some(limit) = self.limit.take() {
            return Err(limit);
        }
        Ok(ids)
    }

    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.enter(input)?;
        let result = (|| {
            if (input.starts_with("(_") || input.starts_with("(^"))
                && let Ok(found) = self.parse_prescripts(input)
            {
                return Ok(found);
            }
            let (input, base) = self.parse_atom(input)?;
            self.parse_scripts(input, base)
        })();
        self.depth -= 1;
        result
    }

    /// UTN #28 section 3.3: `(_c^b)a` gives the base `a` the leading
    /// subscript `c` and superscript `b`; either script may be left out
    /// (`(_1^)F`).
    fn parse_prescripts<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let fail =
            |input| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char));
        let mut rest = input.strip_prefix('(').ok_or_else(|| fail(input))?;
        let mut script =
            |rest: &mut &'a str,
             mark: char|
             -> std::result::Result<NodeId, nom::Err<nom::error::Error<&'a str>>> {
                let Some(after) = rest.strip_prefix(mark) else {
                    return Ok(NodeId::NONE);
                };
                if after.starts_with(')') || after.starts_with('^') {
                    *rest = after;
                    return Ok(NodeId::NONE);
                }
                // A script that is only a space (`(_ ^v)L`).
                let spaces = after.len() - after.trim_start_matches([' ', '\u{00A0}']).len();
                if spaces > 0 && after[spaces..].starts_with([')', '^']) {
                    *rest = &after[spaces..];
                    return Ok(self
                        .arena
                        .push_node(CompactNode::Space(Rational::new(1, 4))));
                }
                let (after, id) = self.parse_script_arg(after)?;
                *rest = after;
                Ok(id)
            };
        let pre_sub = script(&mut rest, '_')?;
        let pre_sup = script(&mut rest, '^')?;
        let rest = rest.strip_prefix(')').ok_or_else(|| fail(rest))?;
        if pre_sub.is_none() && pre_sup.is_none() {
            return Err(fail(input));
        }
        let (rest, base) = self.parse_element(rest)?;
        let id = self.arena.push_node(CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        });
        Ok((rest, id))
    }

    fn parse_scripts<'a>(&mut self, input: &'a str, base: NodeId) -> IResult<&'a str, NodeId> {
        // Try subscript
        let (input, sub) = if let Some(rest) = input.strip_prefix('_') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, Some(id)),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, None),
            }
        } else {
            (input, None)
        };

        // Try superscript
        let (input, sup) = if let Some(rest) = input.strip_prefix('^') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, Some(id)),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, None),
            }
        } else {
            (input, None)
        };

        match (sub, sup) {
            (None, None) => {
                // Check for fraction: base / denominator
                if let Some(rest) = input.strip_prefix('/') {
                    match self.parse_script_arg(rest) {
                        Ok((rest, den)) => {
                            let id = self.push_node(CompactNode::Fraction {
                                kind: FractionKind::Normal,
                                num: base,
                                den,
                            });
                            Ok((rest, id))
                        },
                        Err(nom::Err::Failure(e)) => Err(nom::Err::Failure(e)),
                        Err(_) => Ok((input, base)),
                    }
                } else {
                    Ok((input, base))
                }
            },
            (Some(sub), None) => {
                let id = self.push_node(CompactNode::Subscript { base, sub });
                Ok((input, id))
            },
            (None, Some(sup)) => {
                let id = self.push_node(CompactNode::Superscript { base, sup });
                Ok((input, id))
            },
            (Some(sub), Some(sup)) => {
                let id = self.push_node(CompactNode::SubSup { base, sub, sup });
                Ok((input, id))
            },
        }
    }

    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // In argument position (after √/∛ or ^/_), a parenthesized group is a
        // grouping delimiter, not literal parentheses (UTN#28): absorb the parens
        // and return the inner content. This keeps the writer's `√(x)` / `x^(a+b)`
        // wrapping invertible (otherwise re-reading would nest a literal Fenced
        // and the next write would double the parentheses).
        if input.starts_with('(') && matching_paren_end(input).is_some() {
            match self.parse_absorbing_paren_group(input) {
                Ok(r) => return Ok(r),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                // The group's content does not parse: fall back to a literal
                // token, which `(` is not, so the argument is missing.
                Err(_) => return self.parse_single_char_node(input),
            }
        }
        try_alt!(self, input, parse_group, parse_single_char_node);
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Parse `(...)` in argument position, returning the inner content directly
    /// (the parentheses are absorbed as a grouping delimiter, not kept as a
    /// literal `Fenced`).
    fn parse_absorbing_paren_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (remaining, children) = self.parse_delimited(input, '(', ')')?;
        // An empty `()` has no sensible "inner content"; keep it as a literal
        // empty fence so it is not silently dropped.
        if children.is_empty() {
            let open = self.arena.push_str("(");
            let close = self.arena.push_str(")");
            let content_span = self.arena.push_children_deferred(&[]);
            let id = self.push_node(CompactNode::Fenced {
                open,
                close,
                content: content_span,
            });
            return Ok((remaining, id));
        }
        let content_id = self.flatten_single(&children);
        Ok((remaining, content_id))
    }

    /// Parse the elements between `open` and the matching `close`.
    ///
    /// Fails with `Error` unless `input` starts with `open` and its elements
    /// are followed by `close`. The outcome depends only on `input`, so a
    /// failure is recorded and not recomputed: the same group is otherwise
    /// re-parsed by every fallback that reaches its offset.
    fn parse_delimited<'a>(
        &mut self,
        input: &'a str,
        open: char,
        close: char,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let Some(content) = input.strip_prefix(open) else {
            return Err(parse_error(input, nom::error::ErrorKind::Char));
        };
        self.memo_failure(input, |this, _| {
            let mut remaining = content;
            let mut children: SmallVec<[NodeId; 8]> = SmallVec::new();
            loop {
                if remaining.starts_with(close) {
                    break;
                }
                match this.parse_element(remaining) {
                    Ok((rest, id)) => {
                        children.push(id);
                        remaining = rest;
                    },
                    Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                    Err(_) => break,
                }
            }
            // Require the closing delimiter; otherwise this is not a balanced
            // group and the caller falls back to literal handling.
            let (remaining, _) = satisfy(|c| c == close)(remaining)?;
            Ok((remaining, children))
        })
    }

    /// Parse one atom. Whether an atom fails at an offset depends only on
    /// the input from there on, so failures are recorded by offset.
    fn parse_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.memo_failure(input, Self::parse_atom_alternatives)
    }

    fn parse_atom_alternatives<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_box_phantom,
            parse_sqrt,
            parse_cbrt,
            parse_matrix,
            parse_text_quoted,
            parse_nary_op,
            parse_group,
            parse_number,
            parse_operator_word,
            parse_symbol,
            parse_identifier,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Parse a UnicodeMath box (`▭` U+25AD) or phantom (`⟡` U+27E1) operator
    /// emitted by this crate's UnicodeMath writer, so a write→read round-trip
    /// recovers the `Boxed`/`Phantom` structure instead of dropping it as an
    /// unknown symbol. The marker is followed by its content (parenthesized only
    /// when complex), which we parse as the next atom.
    fn parse_box_phantom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut chars = input.chars();
        let is_box = match chars.next() {
            Some('\u{25AD}') => true,  // ▭ box
            Some('\u{27E1}') => false, // ⟡ phantom
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            },
        };
        let rest = chars.as_str();
        // The content is a child of the box, one level deeper.
        self.enter(rest)?;
        let content = self.parse_atom(rest);
        self.depth -= 1;
        let (rest, content) = content?;
        let node = if is_box {
            let empty = self.arena.push_str("");
            CompactNode::Boxed {
                content,
                bg_color: empty,
                border_color: empty,
            }
        } else {
            CompactNode::Phantom(content, PhantomKind::Full)
        };
        Ok((rest, self.push_node(node)))
    }

    fn parse_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_paren_group,
            parse_bracket_group,
            parse_brace_group,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Flatten a single-element slice into its element, or wrap in Row.
    fn flatten_single(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children_deferred(ids);
            self.push_node(CompactNode::Row(span))
        }
    }

    fn parse_paren_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.parse_fenced_group(input, '(', ')')
    }

    fn parse_bracket_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.parse_fenced_group(input, '[', ']')
    }

    fn parse_brace_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.parse_fenced_group(input, '{', '}')
    }

    /// Parse a bracket group kept as a literal `Fenced` node.
    fn parse_fenced_group<'a>(
        &mut self,
        input: &'a str,
        open: char,
        close: char,
    ) -> IResult<&'a str, NodeId> {
        let (remaining, children) = self.parse_delimited(input, open, close)?;
        let content_id = self.flatten_single(&children);
        let mut buf = [0_u8; 4];
        let open = self.arena.push_str(open.encode_utf8(&mut buf));
        let close = self.arena.push_str(close.encode_utf8(&mut buf));
        let content_span = self.arena.push_children_deferred(&[content_id]);
        let id = self.push_node(CompactNode::Fenced {
            open,
            close,
            content: content_span,
        });
        Ok((remaining, id))
    }

    fn parse_sqrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{221A}')(input)?; // √
        let (input, content) = self.parse_script_arg(input)?;
        let id = self.push_node(CompactNode::Sqrt(content));
        Ok((input, id))
    }

    fn parse_cbrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{221B}')(input)?; // ∛
        let (input, content) = self.parse_script_arg(input)?;
        let s = self.arena.push_str("3");
        let index = self.push_node(CompactNode::Number(s));
        let id = self.push_node(CompactNode::Root {
            index,
            radicand: content,
        });
        Ok((input, id))
    }

    fn parse_matrix<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{25A0}')(input)?; // ■
        let (input, _) = satisfy(|c| c == '(')(input)?;
        let (input, content) = self.parse_matrix_content(input)?;
        let (input, _) = satisfy(|c| c == ')')(input)?;
        Ok((input, content))
    }

    fn parse_matrix_content<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 4]>>> = Vec::new();
        let mut current_row: Vec<SmallVec<[NodeId; 4]>> = Vec::new();
        let mut current_cell: SmallVec<[NodeId; 4]> = SmallVec::new();

        let mut remaining = input;
        loop {
            if remaining.is_empty() || remaining.starts_with(')') {
                break;
            }
            if remaining.starts_with('@') {
                current_row.push(std::mem::take(&mut current_cell));
                rows.push(std::mem::take(&mut current_row));
                remaining = &remaining[1..];
                continue;
            }
            if remaining.starts_with('&') {
                current_row.push(std::mem::take(&mut current_cell));
                remaining = &remaining[1..];
                continue;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    current_cell.push(id);
                    remaining = rest;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    // Skip one char
                    remaining = &remaining[remaining
                        .char_indices()
                        .nth(1)
                        .map_or(remaining.len(), |(i, _)| i)..];
                },
            }
        }
        // Finalize
        if !current_cell.is_empty() || !current_row.is_empty() {
            current_row.push(current_cell);
            rows.push(current_row);
        }

        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(1) as u16;

        // Column alignments
        let mut col_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for _ in 0..num_cols {
            col_ids.push(self.push_node(CompactNode::ColAlign(ColumnAlign::Center)));
        }
        let col_aligns = self.arena.push_children_deferred(&col_ids);

        // Cells stored flat in row-major order, padded
        let mut cell_ids: SmallVec<[NodeId; 16]> = SmallVec::new();
        for row in &rows {
            for cell in row {
                let span = self.arena.push_children_deferred(cell);
                cell_ids.push(self.push_node(CompactNode::Row(span)));
            }
            // Pad short rows
            for _ in row.len()..(num_cols as usize) {
                cell_ids.push(self.push_node(CompactNode::Row(NodeSpan::EMPTY)));
            }
        }
        let cells = self.arena.push_children_deferred(&cell_ids);

        let id = self.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        Ok((remaining, id))
    }

    fn parse_text_quoted<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if !input.starts_with('"') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let rest = &input['"'.len_utf8()..];
        let (rest, text) = take_while1(|c: char| c != '"')(rest)?;
        if !rest.starts_with('"') {
            return Err(nom::Err::Error(nom::error::Error::new(
                rest,
                nom::error::ErrorKind::Char,
            )));
        }
        let rest = &rest['"'.len_utf8()..];
        let sid = self.arena.push_str(text);
        let id = self.push_node(CompactNode::Text(TextStyle::Normal, sid));
        Ok((rest, id))
    }

    fn parse_nary_op<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, op) = satisfy(is_nary_char)(input)?;

        // Try subscript
        let (input, sub) = if let Some(rest) = input.strip_prefix('_') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, id),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, NodeId::NONE),
            }
        } else {
            (input, NodeId::NONE)
        };

        // Try superscript
        let (input, sup) = if let Some(rest) = input.strip_prefix('^') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, id),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, NodeId::NONE),
            }
        } else {
            (input, NodeId::NONE)
        };

        // Skip optional whitespace before body
        let input = input.trim_start();

        let (input, body) = if input.is_empty() {
            let row_id = self.push_node(CompactNode::Row(NodeSpan::EMPTY));
            (input, row_id)
        } else {
            match self.parse_script_arg(input) {
                Ok((rest, id)) => (rest, id),
                Err(_) => {
                    let row_id = self.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    (input, row_id)
                },
            }
        };

        let limits = if is_integral(op) {
            LimitStyle::SubSup
        } else {
            LimitStyle::AboveBelow
        };

        let id = self.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        });
        Ok((input, id))
    }

    fn parse_number<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Every suffix of a run of dots is also a run of dots, so once a run
        // is rejected its suffixes are rejected without rescanning it.
        let offset = self.offset(input);
        if (self.dot_run.0..self.dot_run.1).contains(&offset) {
            return Err(parse_error(input, nom::error::ErrorKind::Digit));
        }
        let (rest, num) = take_while1(|c: char| c.is_ascii_digit() || c == '.')(input)?;
        if num.chars().all(|c| c == '.') {
            self.dot_run = (offset, offset + num.len());
            return Err(parse_error(input, nom::error::ErrorKind::Digit));
        }
        let sid = self.arena.push_str(num);
        let id = self.push_node(CompactNode::Number(sid));
        Ok((rest, id))
    }

    fn parse_operator_word<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let ops = [
            "sin", "cos", "tan", "sec", "csc", "cot", "sinh", "cosh", "tanh", "coth", "arcsin",
            "arccos", "arctan", "log", "ln", "lg", "exp", "lim", "liminf", "limsup", "sup", "inf",
            "max", "min", "det", "dim", "ker", "hom", "arg", "deg", "gcd", "lcm", "sgn", "Pr",
            "Var", "Cov", "supp", "mod",
        ];
        for op in &ops {
            if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>(*op)(input)
                && rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
            {
                let has_limits = matches!(
                    *op,
                    "lim" | "liminf" | "limsup" | "sup" | "inf" | "max" | "min"
                );
                let name = self.arena.push_str(op);
                let id = self.push_node(CompactNode::Operator {
                    name,
                    limits: if has_limits {
                        LimitStyle::AboveBelow
                    } else {
                        LimitStyle::Default
                    },
                });
                return Ok((rest, id));
            }
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }

    fn parse_symbol<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) = satisfy(is_math_symbol)(input)?;
        let kind = classify_unicode_symbol(ch);
        // Skip whitespace after binary/relation operators
        let rest = if matches!(kind, SymbolKind::BinaryOp | SymbolKind::Relation) {
            rest.trim_start()
        } else {
            rest
        };
        let sid = self.arena.push_str(&ch.to_string());
        let id = self.push_node(CompactNode::Symbol(kind, sid));
        Ok((rest, id))
    }

    fn parse_identifier<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) =
            satisfy(|c| c.is_alphabetic() && !is_math_symbol(c) && !is_nary_char(c) && c != '"')(
                input,
            )?;
        let sid = self.arena.push_str(&ch.to_string());
        let id = self.push_node(CompactNode::Identifier(sid));
        Ok((rest, id))
    }

    fn parse_single_char_node<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(self, input, parse_number, parse_symbol, parse_identifier);
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }
}

// ── Pure helper functions (no arena needed) ───────────────────────────

fn is_nary_char(c: char) -> bool {
    matches!(
        c,
        '\u{2211}' // sum
        | '\u{220F}' // product
        | '\u{2210}' // coproduct
        | '\u{222B}' // integral
        | '\u{222C}' // double integral
        | '\u{222D}' // triple integral
        | '\u{222E}' // contour integral
        | '\u{22C0}' // big wedge
        | '\u{22C1}' // big vee
        | '\u{22C2}' // big intersection
        | '\u{22C3}' // big union
        | '\u{2A00}' // big circled dot
        | '\u{2A01}' // big circled plus
        | '\u{2A02}' // big circled times
    )
}

fn is_integral(c: char) -> bool {
    matches!(c, '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{222E}')
}

fn is_math_symbol(c: char) -> bool {
    matches!(c,
        '+' | '-' | '=' | '<' | '>' | ',' | ';' | '!' | '?' | ':' |
        '\u{00B1}' | '\u{00D7}' | '\u{00F7}' | // ±, ×, ÷
        '\u{2200}'..='\u{22FF}' | // Mathematical Operators
        '\u{2190}'..='\u{21FF}' | // Arrows
        '\u{27C0}'..='\u{27EF}' | // Misc Math Symbols-A
        '\u{2980}'..='\u{29FF}' | // Misc Math Symbols-B
        '\u{2A00}'..='\u{2AFF}'   // Supplemental Math Operators
    ) && !is_nary_char(c)
}

fn classify_unicode_symbol(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2227}'
        | '\u{2228}' | '\u{2295}' | '\u{2297}' | '\u{2229}' | '\u{222A}' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2260}' | '\u{2264}' | '\u{2265}' | '\u{226A}' | '\u{226B}'
        | '\u{2248}' | '\u{2261}' | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}'
        | '\u{2208}' | '\u{2209}' | '\u{21D2}' | '\u{21D4}' | '\u{2192}' | '\u{2190}' => {
            SymbolKind::Relation
        },
        '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}'
        | '\u{2308}' | '\u{2309}' | '\u{230A}' | '\u{230B}' => SymbolKind::Fence,
        _ => SymbolKind::Ordinary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        UnicodeMathReader::parse(input, &config).unwrap().expr.nodes
    }

    #[test]
    fn number() {
        let nodes = parse("42");
        assert!(matches!(&nodes[0], MathNode::Number(s) if s == "42"));
    }

    #[test]
    fn identifier() {
        let nodes = parse("x");
        assert!(matches!(&nodes[0], MathNode::Identifier(s) if s == "x"));
    }

    #[test]
    fn reads_back_box_and_phantom_sentinels() {
        // The UnicodeMath writer emits ▭ (U+25AD) for \boxed and ⟡ (U+27E1) for
        // \phantom; the reader must recognize them so the structure round-trips.
        let nodes = parse("\u{25AD}x");
        assert!(
            matches!(&nodes[0], MathNode::Boxed { content, .. } if matches!(&**content, MathNode::Identifier(s) if s == "x")),
            "▭x should parse to Boxed{{x}}, got {:?}",
            nodes
        );
        let nodes = parse("\u{27E1}y");
        assert!(
            matches!(&nodes[0], MathNode::Phantom(inner, _) if matches!(&**inner, MathNode::Identifier(s) if s == "y")),
            "⟡y should parse to Phantom(y), got {:?}",
            nodes
        );
    }

    #[test]
    fn simple_addition() {
        let nodes = parse("a+b");
        assert_eq!(nodes.len(), 3);
        assert!(matches!(
            &nodes[1],
            MathNode::Symbol(SymbolKind::BinaryOp, _)
        ));
    }

    #[test]
    fn spaced_operators_are_not_text() {
        // Regression: a space used to send the rest of the input into a
        // Text node ("a + b" -> [a, Text("+ b")]). Spaces separate operands
        // in UnicodeMath and must not swallow following math.
        let nodes = parse("a + b");
        assert_eq!(nodes.len(), 3, "got {nodes:?}");
        assert!(matches!(&nodes[0], MathNode::Identifier(s) if s == "a"));
        assert!(matches!(
            &nodes[1],
            MathNode::Symbol(SymbolKind::BinaryOp, _)
        ));
        assert!(matches!(&nodes[2], MathNode::Identifier(s) if s == "b"));
        assert!(
            !nodes.iter().any(|n| matches!(n, MathNode::Text(_, _))),
            "no Text node expected: {nodes:?}"
        );
    }

    #[test]
    fn spaced_expression_keeps_fraction() {
        // "x^2 + 1/2" with spaces should still produce a superscript, an
        // operator and a fraction (not literal text).
        let nodes = parse("x^2 + 1/2");
        assert!(
            matches!(&nodes[0], MathNode::Superscript { .. }),
            "{nodes:?}"
        );
        assert!(
            nodes.iter().any(|n| matches!(n, MathNode::Fraction { .. })),
            "fraction expected: {nodes:?}"
        );
        assert!(
            !nodes.iter().any(|n| matches!(n, MathNode::Text(_, _))),
            "no Text node expected: {nodes:?}"
        );
    }

    #[test]
    fn superscript() {
        let nodes = parse("x^2");
        assert!(matches!(&nodes[0], MathNode::Superscript { .. }));
    }

    #[test]
    fn subscript() {
        let nodes = parse("x_i");
        assert!(matches!(&nodes[0], MathNode::Subscript { .. }));
    }

    #[test]
    fn fraction() {
        let nodes = parse("a/b");
        assert!(matches!(&nodes[0], MathNode::Fraction { .. }));
    }

    #[test]
    fn sqrt() {
        let nodes = parse("\u{221A}x");
        assert!(matches!(&nodes[0], MathNode::Sqrt(_)));
    }

    #[test]
    fn sqrt_absorbs_parenthesized_argument() {
        // `√(x+y)`: the parentheses are a grouping delimiter (UTN#28), so the
        // radicand is the inner content, NOT a literal Fenced of `(x+y)`.
        let nodes = parse("\u{221A}(x+y)");
        match &nodes[0] {
            MathNode::Sqrt(inner) => assert!(
                !matches!(inner.as_ref(), MathNode::Fenced { .. }),
                "radicand parens should be absorbed, got {inner:?}"
            ),
            other => panic!("expected Sqrt, got {other:?}"),
        }
        // Bare parentheses (not in argument position) stay literal.
        let lit = parse("(x)");
        assert!(matches!(&lit[0], MathNode::Fenced { .. }));
    }

    #[test]
    fn script_parenthesis_lookahead_is_balanced_and_bounded() {
        assert_eq!(matching_paren_end("(x)y"), Some(3));
        assert_eq!(matching_paren_end("((x))y"), Some(5));
        assert_eq!(matching_paren_end("(x"), None);
        let distant_close = format!("({})", "x".repeat(SCRIPT_PAREN_LOOKAHEAD + 1));
        assert_eq!(matching_paren_end(&distant_close), None);
    }

    #[test]
    fn parentheses() {
        let nodes = parse("(x)");
        assert!(matches!(&nodes[0], MathNode::Fenced { .. }));
    }

    #[test]
    fn sum_with_limits() {
        let nodes = parse("\u{2211}_(i=0)^n x");
        assert!(matches!(&nodes[0], MathNode::Nary { .. }));
    }

    #[test]
    fn matrix() {
        let nodes = parse("\u{25A0}(a&b@c&d)");
        assert!(matches!(&nodes[0], MathNode::Array { .. }));
        if let MathNode::Array { rows, .. } = &nodes[0] {
            assert_eq!(rows.len(), 2);
        }
    }

    #[test]
    fn quoted_text() {
        let nodes = parse("\"hello\"");
        assert!(matches!(&nodes[0], MathNode::Text(_, s) if s == "hello"));
    }

    #[test]
    fn operator() {
        let nodes = parse("sin x");
        assert!(matches!(&nodes[0], MathNode::Operator(_)));
    }

    #[test]
    fn recorded_failure_still_reports_the_depth_limit() {
        // `(((x` fails (no closing parens) after entering elements at 3
        // depths: its start and 2 levels below. Reached again deeper, a recorded failure must
        // abort exactly when re-parsing it would cross the depth limit.
        let source = "(((x";
        let config = Config::default().with_max_depth(10);
        let mut parser = CompactParser::new(source, &config);
        assert!(matches!(parser.parse_atom(source), Err(nom::Err::Error(_))));
        assert_eq!(parser.failed.get(&0), Some(&3));
        let nodes = parser.arena.node_count();
        // Deepest element at 8 + 2 = 10: within the limit.
        parser.depth = 8;
        assert!(matches!(parser.parse_atom(source), Err(nom::Err::Error(_))));
        assert!(parser.limit.is_none());
        assert_eq!(parser.arena.node_count(), nodes, "not re-parsed");
        // Deepest element at 9 + 2 = 11: past the limit.
        parser.depth = 9;
        assert!(matches!(
            parser.parse_atom(source),
            Err(nom::Err::Failure(_))
        ));
        assert!(matches!(
            parser.limit,
            Some(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: 10
            })
        ));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = UnicodeMathReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }
}
