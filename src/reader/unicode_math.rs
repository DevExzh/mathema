use nom::IResult;
use nom::bytes::complete::take_while1;
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

/// Find the top-level `&` in the body of an nth-root argument. UnicodeMath
/// uses the same separator inside matrix and other grouped constructs, so only
/// an ampersand outside all nested delimiters belongs to the root itself.
fn root_separator(input: &str) -> Option<usize> {
    let mut round = 0_u32;
    let mut square = 0_u32;
    let mut curly = 0_u32;
    let mut quoted = false;
    for (index, character) in input.char_indices() {
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match character {
            '(' => round = round.saturating_add(1),
            ')' => round = round.saturating_sub(1),
            '[' => square = square.saturating_add(1),
            ']' => square = square.saturating_sub(1),
            '{' => curly = curly.saturating_add(1),
            '}' => curly = curly.saturating_sub(1),
            '&' if round == 0 && square == 0 && curly == 0 => return Some(index),
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
        let lead = input.len() - input.trim_start().len();
        let input = input.trim();
        if input.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                DisplayStyle::Inline,
            )));
        }
        let mut parser = CompactParser::new(input, lead, config);
        match parser.parse_expr_list(input) {
            Ok(ids) => {
                parser.arena.check_limits()?;
                let roots = parser.arena.push_children(&ids)?;
                let diagnostics = parser.skip_diagnostics();
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, roots, DisplayStyle::Inline),
                    diagnostics,
                ))
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
// * A matrix skips what it cannot parse in Lenient mode. A walk over matrix
//   content that fails (at the end of the input without `)`, or in Strict
//   mode at content it cannot parse) makes any walk that reaches one of its
//   positions later fail too (it follows the same path), so those positions
//   are recorded.
// * Every `nom::Err::Failure` means a resource limit was hit. It aborts the
//   whole parse: nothing catches it to try another alternative.
//
// Nodes built by failed alternatives stay in the arena as garbage, so the
// node limit counts every node the parse creates. Skips recorded by a failed
// alternative are dropped with it.

/// Matrix content skipped in Lenient mode: `runs` runs of characters between
/// byte offsets `start` and `end` of one matrix.
struct MatrixSkips {
    start: usize,
    end: usize,
    runs: usize,
}

/// At most this many matrices get their own skip warning; the rest are
/// summed up in one more.
const MAX_SKIP_WARNINGS: usize = 32;

struct CompactParser {
    arena: AstArena,
    /// Byte length of the whole (trimmed) input. Every slice the parser sees
    /// is a suffix of it, so `source_len - rest.len()` is the offset of `rest`.
    source_len: usize,
    /// Leading whitespace trimmed from the input, added to reported offsets.
    lead: usize,
    strictness: Strictness,
    /// Current element nesting depth.
    depth: u32,
    /// Deepest element nesting entered by the attempt in progress, plus one
    /// (0 when none).
    deepest: u32,
    /// Element nesting limit ([`Config::effective_max_depth`]). Each nested
    /// element is a child of the node that contains it, so a deeper nesting
    /// would exceed the AST depth limit anyway.
    max_depth: u32,
    /// [`Config::max_nodes`].
    max_nodes: Option<usize>,
    /// The error that aborted the parse, if any.
    abort: Option<Error>,
    /// Per byte offset: 0 when the atom or bracket group there is not known
    /// to fail, otherwise one more than the number of element nesting levels
    /// its failed attempt entered (at most `max_depth + 2`, so it fits a byte).
    /// One table serves both: an atom at a bracket fails exactly when its
    /// group fails. Allocated on the first failure.
    failed: Vec<u8>,
    /// Bit per byte offset: a matrix content walk from there fails.
    dead_walk: Vec<u64>,
    /// Byte range of a run of `.` that is known not to start a number.
    dot_run: (usize, usize),
    /// Matrices of the result that skipped content (Lenient mode), in the
    /// order they were completed (a nested matrix before the one around it).
    /// Formatted into diagnostics only once the parse succeeded.
    matrix_skips: Vec<MatrixSkips>,
    /// Matrix cells created but not yet stored as nodes; they count towards
    /// [`Config::max_nodes`] as soon as they are created.
    pending_cells: usize,
    /// Units of work done, for tests of the parser's complexity.
    #[cfg(test)]
    steps: usize,
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

fn abort_failure(input: &str) -> nom::Err<nom::error::Error<&str>> {
    nom::Err::Failure(nom::error::Error::new(
        input,
        nom::error::ErrorKind::TooLarge,
    ))
}

impl CompactParser {
    fn new(source: &str, lead: usize, config: &Config) -> Self {
        Self {
            arena: AstArena::new(),
            source_len: source.len(),
            lead,
            strictness: config.strictness,
            depth: 0,
            deepest: 0,
            // `effective_max_depth` is at most `DEFAULT_MAX_DEPTH` (128), so
            // a recorded depth span stays below 255.
            max_depth: u32::try_from(config.effective_max_depth())
                .unwrap_or(u32::MAX)
                .min(250),
            max_nodes: config.max_nodes,
            abort: None,
            failed: Vec::new(),
            dead_walk: Vec::new(),
            dot_run: (0, 0),
            matrix_skips: Vec::new(),
            pending_cells: 0,
            #[cfg(test)]
            steps: 0,
        }
    }

    /// Count `units` of work (only in tests).
    #[inline]
    #[cfg_attr(not(test), allow(clippy::unused_self))]
    fn tick(&mut self, units: usize) {
        #[cfg(test)]
        {
            self.steps += units;
        }
        #[cfg(not(test))]
        let _ = units;
    }

    fn offset(&self, rest: &str) -> usize {
        self.source_len - rest.len()
    }

    /// Push a node, enforcing [`Config::max_nodes`]. Past the limit the node
    /// is still stored (callers need an id) but the parse is aborted at the
    /// next element.
    fn push_node(&mut self, node: CompactNode) -> NodeId {
        let id = self.arena.push_node(node);
        self.check_node_count(0);
        id
    }

    /// Abort if the nodes built, the pending matrix cells and `more` nodes
    /// about to be built exceed [`Config::max_nodes`]. Returns whether the
    /// parse is aborted.
    fn check_node_count(&mut self, more: usize) -> bool {
        if let Some(limit) = self.max_nodes
            && self
                .arena
                .node_count()
                .saturating_add(self.pending_cells)
                .saturating_add(more)
                > limit
            && self.abort.is_none()
        {
            self.abort = Some(Error::LimitExceeded {
                kind: LimitKind::Nodes,
                limit,
            });
        }
        self.abort.is_some()
    }

    fn depth_limit(&mut self) {
        if self.abort.is_none() {
            self.abort = Some(Error::LimitExceeded {
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
        self.tick(1);
        if self.abort.is_some() {
            return Err(abort_failure(input));
        }
        self.deepest = self.deepest.max(self.depth + 1);
        if self.depth > self.max_depth {
            self.depth_limit();
            return Err(abort_failure(input));
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
        self.tick(1);
        let offset = self.offset(input);
        if let Some(&recorded) = self.failed.get(offset)
            && recorded > 0
        {
            let span = u32::from(recorded) - 1;
            if span > 0 {
                // The attempt entered elements down to `depth + span - 1`.
                let deepest = self.depth + span;
                self.deepest = self.deepest.max(deepest);
                if deepest - 1 > self.max_depth {
                    self.depth_limit();
                    return Err(abort_failure(input));
                }
            }
            return Err(parse_error(input, nom::error::ErrorKind::Alt));
        }
        let outer = std::mem::take(&mut self.deepest);
        let skips = self.matrix_skips.len();
        let result = parse(self, input);
        let span = self.deepest.saturating_sub(self.depth);
        self.deepest = self.deepest.max(outer);
        if matches!(result, Err(nom::Err::Error(_))) {
            self.matrix_skips.truncate(skips);
            if self.failed.is_empty() {
                self.failed = vec![0; self.source_len + 1];
            }
            // `span <= max_depth + 2 <= 252`; see `new`.
            self.failed[offset] = u8::try_from(span + 1).unwrap_or(u8::MAX);
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
                        .abort
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
        if let Some(error) = self.abort.take() {
            return Err(error);
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
        if input.as_bytes().first().is_some_and(|byte| *byte >= 0x80)
            && let Some((rest, accent, position, wide)) = unicode_accent_suffix(input)
        {
            let id = self.push_node(CompactNode::Accent {
                base,
                accent,
                position,
                wide,
            });
            return self.parse_scripts(rest, id);
        }

        // UTN #28 uses ┬ (U+252C) and ┴ (U+2534) for explicitly forced
        // below/above placement. These markers differ from _/^ because they
        // remain stacked in inline math.
        if input.starts_with(['┬', '┴']) {
            let mut rest = input;
            let mut forced = SmallVec::<[(char, NodeId); 4]>::new();
            while let Some(marker @ ('┬' | '┴')) = rest.chars().next() {
                if forced.len() >= self.max_depth as usize {
                    self.depth_limit();
                    return Err(abort_failure(rest));
                }
                rest = &rest[marker.len_utf8()..];
                let (tail, argument) = self.parse_script_arg(rest)?;
                forced.push((marker, argument));
                rest = tail;
            }
            let id = self.apply_forced_bounds(base, &forced);
            return self.parse_scripts(rest, id);
        }

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
                // Check for fraction: base / denominator, or the barless
                // stack base ¦ denominator.
                let fraction = if let Some(rest) = input.strip_prefix('/') {
                    Some((rest, FractionKind::Normal))
                } else {
                    input
                        .strip_prefix('\u{A6}')
                        .map(|rest| (rest, FractionKind::NoBar))
                };
                if let Some((rest, kind)) = fraction {
                    match self.parse_script_arg(rest) {
                        Ok((rest, den)) => {
                            let id = self.push_node(CompactNode::Fraction {
                                kind,
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

    /// Build forced above/below chains from right to left. A complementary
    /// pair immediately following a base is the compact spelling for one
    /// UnderOver node. Repeated markers otherwise associate to the right,
    /// matching UTN 28's sub/superscript rule.
    fn apply_forced_bounds(&mut self, mut base: NodeId, mut bounds: &[(char, NodeId)]) -> NodeId {
        let mut deferred = SmallVec::<[(char, NodeId); 4]>::new();
        while let Some((&(mark, arg), rest)) = bounds.split_first() {
            if let Some((&(other, second), tail)) = rest.split_first()
                && other != mark
            {
                let (under, over) = if mark == '┬' {
                    (arg, second)
                } else {
                    (second, arg)
                };
                base = self.push_node(CompactNode::UnderOver {
                    base,
                    under,
                    over,
                    movable_limits: false,
                });
                bounds = tail;
            } else {
                deferred.push((mark, base));
                base = arg;
                bounds = rest;
            }
        }
        for (mark, parent) in deferred.into_iter().rev() {
            if let Some(accent) = self.wide_bound_accent(base) {
                base = self.push_node(CompactNode::Accent {
                    base: parent,
                    accent,
                    position: if mark == '┬' {
                        AccentPosition::Below
                    } else {
                        AccentPosition::Above
                    },
                    wide: true,
                });
                continue;
            }
            base = self.push_node(if mark == '┬' {
                CompactNode::Underscript {
                    base: parent,
                    under: base,
                    movable_limits: false,
                }
            } else {
                CompactNode::Overscript {
                    base: parent,
                    over: base,
                    movable_limits: false,
                }
            });
        }
        base
    }

    fn wide_bound_accent(&self, id: NodeId) -> Option<char> {
        let CompactNode::Symbol(_, sid) = *self.arena.node(id) else {
            return None;
        };
        let glyph = self.arena.str_val(sid);
        (glyph.chars().count() == 1)
            .then(|| glyph.chars().next().unwrap())
            .filter(|ch| matches!(ch, '\u{23B4}'..='\u{23B5}' | '\u{23DC}'..='\u{23DF}'))
    }

    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Tables are single UnicodeMath atoms. They can therefore be used as
        // a script argument without an additional grouping pair, just like a
        // box or radical emitted by this writer.
        if input.starts_with('\u{25A0}') {
            return self.parse_matrix(input);
        }
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
        if input.starts_with(['▭', '⟡']) {
            return self.parse_box_phantom(input);
        }
        match self.parse_group(input) {
            Ok(result) => return Ok(result),
            Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
            Err(_) => {},
        }
        if let Ok((mut rest, mut id)) = self.parse_single_char_node(input) {
            // Combining and wide accent suffixes are part of a single
            // UnicodeMath atom when used as a fraction/script operand. This
            // keeps a/b̅ and the corresponding structured operator body
            // from detaching the accent into a later text fallback.
            if rest.as_bytes().first().is_some_and(|byte| *byte >= 0x80) {
                while let Some((after, accent, position, wide)) = unicode_accent_suffix(rest) {
                    id = self.push_node(CompactNode::Accent {
                        base: id,
                        accent,
                        position,
                        wide,
                    });
                    rest = after;
                }
            }
            return Ok((rest, id));
        }
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
        self.memo_failure(input, |this, _| this.parse_until_close(content, close))
    }

    fn parse_until_close<'a>(
        &mut self,
        content: &'a str,
        close: char,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut remaining = content;
        let mut children: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            self.tick(1);
            // ASCII whitespace separates UnicodeMath atoms. Check for it by
            // byte before calling the general trim matcher so ordinary ASCII
            // groups avoid the Unicode-space path entirely.
            if remaining
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            {
                remaining = remaining.trim_start_matches([' ', '\t', '\r', '\n']);
                continue;
            }
            if remaining.starts_with(close) {
                break;
            }
            // The typographic spaces emitted by the writer (thin/en/em) are
            // meaningful content inside an invisible group. Their UTF-8
            // encodings are non-ASCII, so ASCII atoms skip this lookup.
            if remaining
                .as_bytes()
                .first()
                .is_some_and(|byte| *byte >= 0x80)
                && let Some((rest, width)) = unicode_math_space(remaining)
            {
                children.push(self.push_node(CompactNode::Space(width)));
                remaining = rest;
                continue;
            }
            match self.parse_element(remaining) {
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
    /// emitted by this crate's UnicodeMath writer. A masked box uses
    /// ▭(mask&content), where mask 6 hides the bottom and left sides, leaving
    /// the actuarial top/right enclosure. Splitting occurs only at the mask
    /// separator, so ampersands inside the nested operand remain untouched.
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
        // An ordinary parenthesized operand is not a mask. Only claim this
        // syntax once a decimal mask and its separator have both been read.
        if is_box && let Some(params) = rest.strip_prefix('(') {
            let n = params.bytes().take_while(u8::is_ascii_digit).count();
            if n != 0
                && let Some(body) = params[n..].strip_prefix('&')
            {
                if params[..n].parse::<u8>() != Ok(6) {
                    return Err(parse_error(rest, nom::error::ErrorKind::Tag));
                }
                self.enter(body)?;
                let parsed = self.parse_until_close(body, ')');
                self.depth -= 1;
                let (after, ids) = parsed?;
                let content = self.flatten_single(&ids);
                return Ok((
                    after,
                    self.push_node(CompactNode::Actuarial {
                        content,
                        text_mode: false,
                    }),
                ));
            }
        }
        // The content is a child of the box, one level deeper.
        self.enter(rest)?;
        let content = if rest.starts_with('(') {
            self.parse_absorbing_paren_group(rest)
        } else {
            self.parse_atom(rest)
        };
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
        if input.starts_with('〖') {
            let (rest, children) = self.parse_delimited(input, '〖', '〗')?;
            // An empty invisible group before a script is the script's empty
            // base (`x〖〗_1`, LaTeX `x{}_1`): x and an empty atom carrying
            // the subscript, not a subscript on x.
            if children.is_empty() && rest.starts_with(['_', '^']) {
                return Ok((rest, self.push_node(CompactNode::Row(NodeSpan::EMPTY))));
            }
            let base = self.flatten_single(&children);
            // A subscript on an explicitly grouped named function belongs at
            // the side, even when its ungrouped name would take display
            // limits. Preserve this long-standing ordinary UnicodeMath rule;
            // decorated structured bodies are retained below as Inner atoms.
            if rest.starts_with(['_', '^'])
                && let CompactNode::Operator { name, .. } = *self.arena.node(base)
            {
                return Ok((
                    rest,
                    self.push_node(CompactNode::Operator {
                        name,
                        limits: LimitStyle::SubSup,
                    }),
                ));
            }
            // Retain explicit UnicodeMath invisible grouping as an inner
            // atom. This gives a later script a stable base without adding a
            // private marker or a visible fence.
            let grouped = self.push_node(CompactNode::MathAtom {
                atom_type: MathAtomType::Inner,
                content: base,
            });
            return Ok((rest, grouped));
        }
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
        // The writer's general nth-root spelling is `√(index&radicand)`.
        // Parentheses following a radical normally denote a grouping argument,
        // so recognize the separator before handing the input to the ordinary
        // radical parser. The separator search is depth-aware so nested
        // grouped operands and matrix contents remain intact.
        if input.starts_with('(')
            && let Some(end) = matching_paren_end(input)
            && let Some(separator) = root_separator(&input[1..end - 1])
        {
            let inner = &input[1..end - 1];
            let (_, index) = self.parse_root_operand(&inner[..separator])?;
            let (_, radicand) = self.parse_root_operand(&inner[separator + 1..])?;
            let id = self.push_node(CompactNode::Root { index, radicand });
            return Ok((&input[end..], id));
        }
        let (input, content) = self.parse_script_arg(input)?;
        let id = self.push_node(CompactNode::Sqrt(content));
        Ok((input, id))
    }

    /// Parse one complete operand from one side of `√(index&radicand)`.
    fn parse_root_operand<'a>(&mut self, mut input: &'a str) -> IResult<&'a str, NodeId> {
        let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            input = input.trim_start();
            if input.is_empty() {
                break;
            }
            let before = input.len();
            let (rest, id) = self.parse_element(input)?;
            if rest.len() >= before {
                return Err(parse_error(input, nom::error::ErrorKind::Verify));
            }
            ids.push(id);
            input = rest;
        }
        if ids.is_empty() {
            return Err(parse_error(input, nom::error::ErrorKind::Verify));
        }
        Ok((input, self.flatten_single(&ids)))
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
        self.arena
            .mark_array_environment(content, MatrixKind::Matrix);
        Ok((input, content))
    }

    fn is_dead_walk(&self, offset: usize) -> bool {
        self.dead_walk
            .get(offset / 64)
            .is_some_and(|word| word & (1 << (offset % 64)) != 0)
    }

    /// Record that matrix content walks from `offsets` fail.
    fn mark_dead_walks(&mut self, offsets: &[u32]) {
        if self.dead_walk.is_empty() {
            self.dead_walk = vec![0; self.source_len / 64 + 1];
        }
        for &offset in offsets {
            let offset = offset as usize;
            self.dead_walk[offset / 64] |= 1 << (offset % 64);
        }
    }

    /// Warnings for the skipped matrix content of the result, at most
    /// [`MAX_SKIP_WARNINGS`] plus a summary.
    fn skip_diagnostics(&mut self) -> Vec<Diagnostic> {
        // Report in input order.
        self.matrix_skips.sort_unstable_by_key(|skip| skip.start);
        let lead = self.lead;
        let warning = |message: String, start: usize, end: usize| {
            debug_assert!(start <= end, "skip span {start}..{end}");
            Diagnostic {
                kind: DiagnosticKind::Warning,
                message,
                span: Some(crate::error::Span::new(lead + start, lead + end)),
            }
        };
        let mut diagnostics: Vec<Diagnostic> = self
            .matrix_skips
            .iter()
            .take(MAX_SKIP_WARNINGS)
            .map(|skip| {
                warning(
                    format!(
                        "skipped {} run(s) of matrix content that is not UnicodeMath \
                         at bytes {}..{}",
                        skip.runs,
                        lead + skip.start,
                        lead + skip.end
                    ),
                    skip.start,
                    skip.end,
                )
            })
            .collect();
        if let Some(rest) = self.matrix_skips.get(MAX_SKIP_WARNINGS..)
            && let Some(first) = rest.first()
        {
            // Sorted by start; a nested matrix may end after a later one.
            let start = first.start;
            let end = rest.iter().map(|skip| skip.end).max().unwrap_or(start);
            diagnostics.push(warning(
                format!(
                    "skipped content in {} more matrices at bytes {}..{}",
                    rest.len(),
                    lead + start,
                    lead + end
                ),
                start,
                end,
            ));
        }
        diagnostics
    }

    /// Start a new matrix cell, counting it towards the node limit. Returns
    /// whether the parse is aborted.
    fn add_cell(&mut self, cells: &mut usize) -> bool {
        *cells += 1;
        self.pending_cells += 1;
        self.check_node_count(0)
    }

    fn parse_matrix_content<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 4]>>> = Vec::new();
        let mut current_row: Vec<SmallVec<[NodeId; 4]>> = Vec::new();
        let mut current_cell: SmallVec<[NodeId; 4]> = SmallVec::new();

        let mut remaining = input;
        // Loop positions of this walk, recorded as failing if it fails.
        let mut walk: Vec<u32> = Vec::new();
        // Cells created by this walk, pending until the matrix is built.
        let mut cells = 0_usize;
        // Lenient skips: runs, first and last offset, and whether the
        // previous position was skipped.
        let mut skips = MatrixSkips {
            start: 0,
            end: 0,
            runs: 0,
        };
        let mut in_run = false;
        loop {
            self.tick(1);
            let offset = self.offset(remaining);
            if remaining.is_empty() || self.is_dead_walk(offset) {
                // No closing `)`: the matrix fails, and so does any later
                // walk that reaches one of these positions.
                self.mark_dead_walks(&walk);
                return Err(parse_error(remaining, nom::error::ErrorKind::Char));
            }
            if let Ok(offset) = u32::try_from(offset) {
                walk.push(offset);
            }
            if remaining.starts_with(')') {
                break;
            }
            let mut skipped = false;
            if let Some(rest) = remaining.strip_prefix(|c: char| c.is_whitespace()) {
                // Whitespace separates operands; it is not content.
                remaining = rest;
            } else if remaining.starts_with('@') {
                current_row.push(std::mem::take(&mut current_cell));
                rows.push(std::mem::take(&mut current_row));
                remaining = &remaining[1..];
                if self.add_cell(&mut cells) {
                    return Err(abort_failure(remaining));
                }
            } else if remaining.starts_with('&') {
                current_row.push(std::mem::take(&mut current_cell));
                remaining = &remaining[1..];
                if self.add_cell(&mut cells) {
                    return Err(abort_failure(remaining));
                }
            } else {
                match self.parse_element(remaining) {
                    Ok((rest, id)) => {
                        current_cell.push(id);
                        remaining = rest;
                    },
                    Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                    Err(_) if self.strictness == Strictness::Strict => {
                        // Strict mode does not drop content: the matrix fails
                        // like any other construct, and the caller keeps the
                        // input (as text at the top level).
                        self.mark_dead_walks(&walk);
                        return Err(parse_error(remaining, nom::error::ErrorKind::Char));
                    },
                    Err(_) => {
                        let character = remaining.chars().next().unwrap_or_default();
                        remaining = &remaining[character.len_utf8()..];
                        if !in_run {
                            if skips.runs == 0 {
                                skips.start = offset;
                            }
                            skips.runs += 1;
                        }
                        skips.end = self.offset(remaining);
                        skipped = true;
                    },
                }
            }
            in_run = skipped;
        }
        // Finalize
        if !current_cell.is_empty() || !current_row.is_empty() {
            current_row.push(current_cell);
            rows.push(current_row);
        }
        // The cells become nodes below; padding short rows adds more. Refuse
        // a matrix whose padded cells exceed what one node can hold before
        // building any of them.
        self.pending_cells -= cells;
        let widest = rows.iter().map(Vec::len).max().unwrap_or(1);
        let padded = rows.len().saturating_mul(widest);
        if padded > AstArena::MAX_CHILDREN || widest > AstArena::MAX_CHILDREN {
            if self.abort.is_none() {
                self.abort = Some(Error::LimitExceeded {
                    kind: LimitKind::ListLength,
                    limit: AstArena::MAX_CHILDREN,
                });
            }
            return Err(abort_failure(remaining));
        }
        if self.check_node_count(padded + widest + 1) {
            return Err(abort_failure(remaining));
        }
        if skips.runs > 0 {
            self.matrix_skips.push(skips);
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
        self.tick(text.len());
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
        self.tick(num.len());
        if num.chars().all(|c| c == '.') {
            self.dot_run = (offset, offset + num.len());
            return Err(parse_error(input, nom::error::ErrorKind::Digit));
        }
        let sid = self.arena.push_str(num);
        let id = self.push_node(CompactNode::Number(sid));
        Ok((rest, id))
    }

    fn parse_operator_word<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (word, limits) = crate::unicode::symbols::unicode_math_operator_prefix(input)
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Tag))?;
        let name = self.arena.push_str(word);
        let id = self.push_node(CompactNode::Operator { name, limits });
        Ok((&input[word.len()..], id))
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

fn unicode_accent_suffix(input: &str) -> Option<(&str, char, AccentPosition, bool)> {
    let ch = input.chars().next()?;
    let (position, wide) = match ch {
        '\u{0332}' => (AccentPosition::Below, false),
        '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}' => (AccentPosition::Above, false),
        '\u{23B4}' | '\u{23DE}' => (AccentPosition::Above, true),
        '\u{23B5}' | '\u{23DF}' => (AccentPosition::Below, true),
        _ => return None,
    };
    Some((&input[ch.len_utf8()..], ch, position, wide))
}

fn unicode_math_space(input: &str) -> Option<(&str, Rational)> {
    let ch = input.chars().next()?;
    let width = match ch {
        '\u{00A0}' => Rational::new(1, 4),
        '\u{2002}' => Rational::new(1, 2),
        '\u{2003}' => Rational::new(1, 1),
        '\u{2009}' => Rational::new(3, 18),
        _ => return None,
    };
    Some((&input[ch.len_utf8()..], width))
}

fn is_math_symbol(c: char) -> bool {
    matches!(c,
        '+' | '-' | '=' | '<' | '>' | ',' | ';' | '!' | '?' | ':' | '|' | '\u{2016}' |
        '\u{00B1}' | '\u{00D7}' | '\u{00F7}' | // ±, ×, ÷
        '\u{2200}'..='\u{22FF}' | // Mathematical Operators
        '\u{2190}'..='\u{21FF}' | // Arrows
        '\u{23B4}'..='\u{23DF}' | // horizontal braces and bracket accents
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
        _ => crate::unicode::symbols::operator_glyph_kind(ch).unwrap_or(SymbolKind::Ordinary),
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
    fn operator_glyphs_take_symbol_table_class() {
        let nodes = parse("a ≨ b ⋒ c");
        assert_eq!(nodes[1], MathNode::Symbol(SymbolKind::Relation, "≨".into()));
        assert_eq!(nodes[3], MathNode::Symbol(SymbolKind::BinaryOp, "⋒".into()));
        // Bracket glyphs keep the reader's own fence reading.
        assert_eq!(
            parse("a | b")[1],
            MathNode::Symbol(SymbolKind::Fence, "|".into())
        );
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
    fn ordinary_complex_boxes_are_not_masks_or_literal_fences() {
        for input in ["▭(x+y)", "▭(6+x)", "▭(6&x+y)"] {
            let ml =
                crate::convert(input, crate::Format::UnicodeMath, crate::Format::MathML).unwrap();
            assert!(
                ml.contains("menclose") && !ml.contains("<mfenced"),
                "{input}: {ml}"
            );
            let next = crate::convert(
                input,
                crate::Format::UnicodeMath,
                crate::Format::UnicodeMath,
            )
            .unwrap();
            assert_eq!(next, input);
        }
    }

    #[test]
    fn reads_masked_actuarial_box_without_consuming_nested_ampersands() {
        let nodes = parse("▭(6&x)");
        assert!(matches!(
            &nodes[0],
            MathNode::Actuarial { content: inner, .. }
                if matches!(&**inner, MathNode::Identifier(s) if s == "x")
        ));
        let nested = parse("▭(6&■(a&b@c&d))");
        assert!(matches!(&nested[0], MathNode::Actuarial { .. }));
        let limited = Config::default().with_max_nodes(Some(1));
        assert!(matches!(
            UnicodeMathReader::parse("▭(6&x)", &limited),
            Err(Error::LimitExceeded { .. })
        ));
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
    fn nth_root_separator_builds_a_root_node() {
        let nodes = parse("√(204&206)");
        assert!(matches!(
            &nodes[0],
            MathNode::Root { index, radicand }
                if matches!(&**index, MathNode::Number(value) if value == "204")
                    && matches!(&**radicand, MathNode::Number(value) if value == "206")
        ));

        let converted = crate::from_unicode_math("√(204&206)").unwrap();
        assert_eq!(converted.to_unicode_math().unwrap(), "√(204&206)");
    }

    #[test]
    fn nth_root_separator_ignores_nested_group_ampersands() {
        let nodes = parse("√(■(1&2)&(x+y))");
        let MathNode::Root { index, radicand } = &nodes[0] else {
            panic!("expected nth root, got {nodes:?}");
        };
        assert!(matches!(&**index, MathNode::Matrix { .. }));
        assert!(matches!(&**radicand, MathNode::Fenced { .. }));
    }

    #[test]
    fn nth_root_separator_ignores_quoted_ampersands() {
        let nodes = parse("√(\"a&b\"&x)");
        let MathNode::Root { index, radicand } = &nodes[0] else {
            panic!("expected nth root, got {nodes:?}");
        };
        assert!(matches!(&**index, MathNode::Text(_, text) if text == "a&b"));
        assert!(matches!(&**radicand, MathNode::Identifier(text) if text == "x"));
    }

    #[test]
    fn latex_nth_root_unicode_roundtrip_preserves_mathml() {
        let config = Config::default().with_display(DisplayStyle::Inline);
        let source = crate::from_latex_with(r"\sqrt[204]{206}", &config).unwrap();
        let unicode = source.to_unicode_math().unwrap();
        assert_eq!(unicode, "√(204&206)");
        let reparsed = crate::from_unicode_math_with(&unicode, &config).unwrap();
        assert_eq!(reparsed.to_mathml().unwrap(), source.to_mathml().unwrap());
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
        assert!(matches!(&nodes[0], MathNode::Matrix { .. }));
        if let MathNode::Matrix { rows, .. } = &nodes[0] {
            assert_eq!(rows.len(), 2);
        }
    }

    #[test]
    fn matrix_is_a_script_atom() {
        let nodes = parse("x^\u{25A0}(a&b@c&d)");
        assert!(matches!(
            &nodes[0],
            MathNode::Superscript { base, sup }
                if matches!(base.as_ref(), MathNode::Identifier(value) if value == "x")
                    && matches!(sup.as_ref(), MathNode::Matrix { rows, .. } if rows.len() == 2)
        ));

        let config = Config::default().with_display(DisplayStyle::Inline);
        let source =
            crate::from_latex_with(r"x^{\begin{matrix}a&b\\c&d\end{matrix}}", &config).unwrap();
        let unicode = source.to_unicode_math().unwrap();
        let reparsed = crate::from_unicode_math_with(&unicode, &config).unwrap();
        assert_eq!(reparsed.to_mathml().unwrap(), source.to_mathml().unwrap());
    }

    #[test]
    fn vertical_bars_inside_fences_are_math_atoms() {
        for source in ["(273|275)", "[282|286]", "(x|y)"] {
            let nodes = parse(source);
            assert!(
                matches!(&nodes[0], MathNode::Fenced { .. }),
                "{source}: {nodes:?}"
            );
            let result = crate::from_unicode_math(source).unwrap();
            assert_eq!(result.to_unicode_math().unwrap(), source);
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
    fn compound_limit_words_are_single_operators() {
        for source in [
            "lim\u{2009}sup x",
            "lim\u{00a0}sup x",
            "lim\u{2009}inf x",
            "lim\u{00a0}inf x",
            "inj\u{2009}lim x",
            "inj\u{00a0}lim x",
            "proj\u{2009}lim x",
            "proj\u{00a0}lim x",
            "limsup x",
            "liminf x",
            "injlim x",
            "projlim x",
            "plim x",
        ] {
            let nodes = parse(source);
            assert!(
                matches!(&nodes[0], MathNode::Operator(_)),
                "{source}: {nodes:?}"
            );
            assert!(
                matches!(&nodes[1], MathNode::Identifier(value) if value == "x"),
                "{source}: {nodes:?}"
            );
            if source.contains('\u{00a0}') {
                assert_eq!(
                    crate::from_unicode_math(source)
                        .unwrap()
                        .to_unicode_math()
                        .unwrap(),
                    source,
                    "no-break name spacing changed: {source:?}"
                );
            }
        }

        let prefix = parse("limsupremo");
        assert!(
            !prefix
                .iter()
                .any(|node| matches!(node, MathNode::Operator(_))),
            "compound prefix must remain an identifier run: {prefix:?}"
        );
        assert_eq!(
            crate::from_unicode_math("limsupremo")
                .unwrap()
                .to_unicode_math()
                .unwrap(),
            "limsupremo"
        );
        let ascii_space = parse("lim sup");
        assert!(
            ascii_space
                .iter()
                .filter(|node| matches!(node, MathNode::Operator(_)))
                .count()
                >= 2,
            "ASCII space keeps the two ordinary words separate: {ascii_space:?}"
        );
    }

    #[test]
    fn compound_limit_scripts_survive_unicode_math_round_trip() {
        for latex in [
            r"\limsup_{n} a_n",
            r"\liminf_{n} a_n",
            r"\injlim_{i} f_i",
            r"\projlim_{i} f_i",
            r"\plim_{n} X_n",
        ] {
            let config = Config::default().with_display(DisplayStyle::Inline);
            let source = crate::from_latex_with(latex, &config).unwrap();
            let unicode = source.to_unicode_math().unwrap();
            let reparsed = crate::from_unicode_math_with(&unicode, &config).unwrap();
            if latex.starts_with(r"\plim") {
                assert_eq!(
                    reparsed.to_unicode_math().unwrap(),
                    unicode,
                    "{latex}: {unicode}"
                );
            } else {
                assert_eq!(
                    reparsed.to_mathml().unwrap(),
                    source.to_mathml().unwrap(),
                    "{latex}: {unicode}"
                );
            }
        }
    }

    #[test]
    fn forced_stack_markers_bind_to_named_limits() {
        let below = parse("lim\u{252C}(n→∞) a_n");
        assert!(
            matches!(
                &below[0],
                MathNode::Underscript {
                    movable_limits: false,
                    ..
                }
            ),
            "{below:?}"
        );
        let both = parse("lim\u{252C}(n)\u{2534}(m) x");
        assert!(
            matches!(
                &both[0],
                MathNode::UnderOver {
                    movable_limits: false,
                    ..
                }
            ),
            "{both:?}"
        );

        let source = crate::from_latex_with(
            r"\lim\limits_{n}^{m} x",
            &Config::default().with_display(DisplayStyle::Inline),
        )
        .unwrap();
        let unicode = source.to_unicode_math().unwrap();
        assert!(
            unicode.contains('\u{252C}') && unicode.contains('\u{2534}'),
            "{unicode}"
        );
        let reparsed = crate::from_unicode_math(&unicode).unwrap();
        assert_eq!(reparsed.to_unicode_math().unwrap(), unicode);
    }

    #[test]
    fn forced_bound_chains_associate_right_to_left_without_losing_operands() {
        let same_below = parse("lim┬a┬b x");
        assert!(
            matches!(
                &same_below[0],
                MathNode::Underscript {
                    base,
                    under,
                    movable_limits: false,
                } if matches!(**base, MathNode::Operator(_))
                    && matches!(**under, MathNode::Underscript { .. })
            ),
            "{same_below:?}"
        );

        let same_above = parse("lim┴a┴b x");
        assert!(
            matches!(
                &same_above[0],
                MathNode::Overscript {
                    base,
                    over,
                    movable_limits: false,
                } if matches!(**base, MathNode::Operator(_))
                    && matches!(**over, MathNode::Overscript { .. })
            ),
            "{same_above:?}"
        );

        for source in ["lim┬a┴b x", "lim┴a┬b x"] {
            let nodes = parse(source);
            assert!(
                matches!(
                    nodes[0],
                    MathNode::UnderOver {
                        movable_limits: false,
                        ..
                    }
                ),
                "{source}: {nodes:?}"
            );
        }

        for source in ["lim┬a┬b x", "lim┴a┴b x", "lim┬a┴b x", "lim┴a┬b x"] {
            let first = crate::from_unicode_math(source).unwrap();
            let written = first.to_unicode_math().unwrap();
            let second = crate::from_unicode_math(&written).unwrap();
            assert_eq!(
                second.to_unicode_math().unwrap(),
                written,
                "{source}: {written}"
            );
        }

        let config = Config::default().with_max_depth(4);
        let too_deep = format!("lim{}x", "┬a".repeat(8));
        assert!(matches!(
            crate::from_unicode_math_with(&too_deep, &config),
            Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::Depth,
                ..
            })
        ));
    }

    #[test]
    fn recorded_failure_still_reports_the_depth_limit() {
        // `(((x` fails (no closing parens) after entering elements at 3
        // depths: its start and 2 levels below. Reached again deeper, a recorded failure must
        // abort exactly when re-parsing it would cross the depth limit.
        let source = "(((x";
        let config = Config::default().with_max_depth(10);
        let mut parser = CompactParser::new(source, 0, &config);
        assert!(matches!(parser.parse_atom(source), Err(nom::Err::Error(_))));
        assert_eq!(parser.failed.first(), Some(&4), "span 3, stored plus one");
        let nodes = parser.arena.node_count();
        // Deepest element at 8 + 2 = 10: within the limit.
        parser.depth = 8;
        assert!(matches!(parser.parse_atom(source), Err(nom::Err::Error(_))));
        assert!(parser.abort.is_none());
        assert_eq!(parser.arena.node_count(), nodes, "not re-parsed");
        // Deepest element at 9 + 2 = 11: past the limit.
        parser.depth = 9;
        assert!(matches!(
            parser.parse_atom(source),
            Err(nom::Err::Failure(_))
        ));
        assert!(matches!(
            parser.abort,
            Some(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: 10
            })
        ));
    }

    /// Work done to parse `input`, counted by the parser.
    fn steps(input: &str, strictness: Strictness) -> usize {
        let config = Config::default().with_strictness(strictness);
        let mut parser = CompactParser::new(input, 0, &config);
        let _ = parser.parse_expr_list(input);
        parser.steps
    }

    /// The cases below need at most 6; re-walking nested matrices needed
    /// hundreds.
    const STEPS_PER_BYTE: usize = 16;

    /// Doubling the repeated part of the input at most doubles the work.
    fn assert_linear(name: &str, prefix: &str, unit: &str, count: usize, strictness: Strictness) {
        let build = |n: usize| format!("{prefix}{}", unit.repeat(n));
        let small = steps(&build(count), strictness);
        let large_input = build(2 * count);
        let large = steps(&large_input, strictness);
        // Each byte is visited a bounded number of times, however deep the
        // nesting of the constructs around it.
        assert!(
            large <= STEPS_PER_BYTE * large_input.len(),
            "{name} ({strictness:?}): {large} steps for {} bytes",
            large_input.len()
        );
        assert!(
            large <= 2 * small,
            "{name} ({strictness:?}): {small} -> {large} steps"
        );
        // Strict mode may refuse the input at once; Lenient reads it all.
        if strictness == Strictness::Lenient {
            assert!(small >= count, "{name}: {small} steps");
        }
    }

    #[test]
    fn work_is_linear_in_the_input() {
        let nested_matrices = "\u{25A0}(".repeat(126);
        let unclosed_in_matrix = format!("{}#", "x\u{2211}^(n(ln".repeat(12));
        let cases: [(&str, &str, &str, usize); 8] = [
            ("well-formed", "", "(a_i+b^2)/c ", 500),
            ("unclosed radicands", "", "\u{221A}(a+", 60),
            ("unclosed n-ary scripts", "", "\u{2211}^(n(l", 60),
            ("unclosed in matrix", "\u{25A0}(", &unclosed_in_matrix, 50),
            ("dots in matrix", "\u{25A0}(", ".", 5_000),
            ("nested matrices, junk tail", &nested_matrices, "#", 5_000),
            ("nested matrices, dot tail", &nested_matrices, ". ", 5_000),
            ("nested matrices, letters", &nested_matrices, "x", 5_000),
        ];
        for (name, prefix, unit, count) in cases {
            for strictness in [Strictness::Strict, Strictness::Lenient] {
                assert_linear(name, prefix, unit, count, strictness);
            }
        }
    }

    fn parse_with(input: &str, strictness: Strictness) -> Result<ParseResult> {
        UnicodeMathReader::parse(input, &Config::default().with_strictness(strictness))
    }

    /// The text of every `Text` node, concatenated.
    fn text_of(nodes: &[MathNode]) -> String {
        nodes
            .iter()
            .filter_map(|n| match n {
                MathNode::Text(_, s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn strict_matrix_keeps_content_it_cannot_parse_as_text() {
        // The matrix fails like any other construct, so nothing is dropped:
        // the rest of the input from the matrix on is kept as text.
        for (input, text) in [
            ("\u{25A0}(a#b)", "\u{25A0}(a#b)"),
            ("\u{25A0}(a(b&c)", "\u{25A0}(a(b&c)"),
            ("x+\u{25A0}(a#b", "\u{25A0}(a#b"),
            ("(\u{25A0}(a#b)", "(\u{25A0}(a#b)"),
            ("!\u{25A0}(#", "\u{25A0}(#"),
        ] {
            let result = parse_with(input, Strictness::Strict).unwrap();
            assert!(
                !result
                    .expr
                    .nodes
                    .iter()
                    .any(|n| matches!(n, MathNode::Matrix { .. })),
                "{input}: {:?}",
                result.expr.nodes
            );
            assert_eq!(text_of(&result.expr.nodes), text, "{input}");
            assert!(result.diagnostics.is_empty());
        }
        // Whitespace is not content.
        let nodes = parse("\u{25A0}(a & b @ c & d)");
        assert!(
            matches!(&nodes[0], MathNode::Matrix { rows, .. } if rows.len() == 2 && rows[0].len() == 2),
            "{nodes:?}"
        );
    }

    #[test]
    fn lenient_matrix_reports_its_skipped_runs_once() {
        let result = parse_with("x \u{25A0}(a##b&c#)", Strictness::Lenient).unwrap();
        assert!(matches!(&result.expr.nodes[1], MathNode::Matrix { .. }));
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        let warning = &result.diagnostics[0];
        assert_eq!(warning.kind, DiagnosticKind::Warning);
        assert_eq!(warning.span.map(|s| (s.start, s.end)), Some((7, 13)));
        assert!(
            warning.message.starts_with("skipped 2 run(s)"),
            "{}",
            warning.message
        );
    }

    #[test]
    fn lenient_skip_warnings_are_capped() {
        let input = "\u{25A0}(a#)".repeat(MAX_SKIP_WARNINGS + 10);
        let result = parse_with(&input, Strictness::Lenient).unwrap();
        assert_eq!(result.diagnostics.len(), MAX_SKIP_WARNINGS + 1);
        let summary = result.diagnostics.last().unwrap();
        assert!(
            summary
                .message
                .starts_with("skipped content in 10 more matrices"),
            "{}",
            summary.message
        );
        // Millions of skipped runs in one matrix make one warning.
        let input = format!("\u{25A0}({})", "# ".repeat(200_000));
        let result = parse_with(&input, Strictness::Lenient).unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert!(
            result.diagnostics[0]
                .message
                .starts_with("skipped 200000 run(s)")
        );
    }

    #[test]
    fn skip_warnings_are_in_input_order_with_ordered_spans() {
        // The inner matrix is completed, and recorded, before the outer one.
        for count in [1, MAX_SKIP_WARNINGS] {
            let input = format!("{}\u{25A0}(#\u{25A0}(b#))", "\u{25A0}(a#)".repeat(count));
            let result = parse_with(&input, Strictness::Lenient).unwrap();
            let spans: Vec<(usize, usize)> = result
                .diagnostics
                .iter()
                .map(|d| d.span.map(|s| (s.start, s.end)).unwrap())
                .collect();
            // One warning per matrix, the last two summed up past the cap.
            let expected = if count == MAX_SKIP_WARNINGS {
                count + 1
            } else {
                count + 2
            };
            assert_eq!(spans.len(), expected, "{spans:?}");
            for &(start, end) in &spans {
                assert!(start <= end && end <= input.len(), "{spans:?}");
                // Callers may slice the input with the span.
                let _ = &input[start..end];
            }
            assert!(spans.windows(2).all(|w| w[0].0 <= w[1].0), "{spans:?}");
            let outer = input.len() - "\u{25A0}(#\u{25A0}(b#))".len();
            let last = *spans.last().unwrap();
            if count == MAX_SKIP_WARNINGS {
                // The summary covers both remaining matrices.
                assert_eq!(last, (outer + 4, input.len() - 2));
            }
        }
    }

    #[test]
    fn skips_in_a_discarded_matrix_are_not_reported() {
        // The unclosed `(` makes the group around the matrix fail, so the
        // matrix is not part of the result and nothing of it was skipped.
        let result = parse_with("(\u{25A0}(a#b)", Strictness::Lenient).unwrap();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn matrix_cells_count_towards_the_node_limit() {
        let config = |strictness| {
            Config::default()
                .with_strictness(strictness)
                .with_max_nodes(Some(1_000))
        };
        let cells = format!("\u{25A0}({})", "&".repeat(5_000));
        let nested = format!("{}{}", "\u{25A0}(".repeat(126), "&".repeat(5_000));
        for input in [&cells, &nested] {
            for strictness in [Strictness::Strict, Strictness::Lenient] {
                let mut parser = CompactParser::new(input, 0, &config(strictness));
                assert!(matches!(
                    parser.parse_expr_list(input),
                    Err(Error::LimitExceeded {
                        kind: LimitKind::Nodes,
                        limit: 1_000
                    })
                ));
                // Refused once the limit was reached, not at the end.
                assert!(parser.steps < 2_000, "{}", parser.steps);
            }
        }
    }

    #[test]
    fn padding_short_matrix_rows_is_bounded() {
        // One wide row and many short ones would pad rows × columns cells.
        let input = format!("\u{25A0}({}{})", "&".repeat(300), "@x".repeat(300));
        let unlimited = Config::default().with_max_nodes(None);
        assert!(matches!(
            UnicodeMathReader::parse(&input, &unlimited),
            Err(Error::LimitExceeded {
                kind: LimitKind::ListLength,
                ..
            })
        ));
        let input = format!("\u{25A0}({}{})", "&".repeat(30), "@x".repeat(30));
        assert!(matches!(
            UnicodeMathReader::parse(&input, &Config::default().with_max_nodes(Some(500))),
            Err(Error::LimitExceeded {
                kind: LimitKind::Nodes,
                limit: 500
            })
        ));
        assert!(UnicodeMathReader::parse(&input, &Config::default()).is_ok());
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = UnicodeMathReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }
}
