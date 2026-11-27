mod bars;

pub(crate) use bars::{pair_bars_at, pairs_bars, reads_as_naryand};

use std::collections::BTreeSet;

use nom::IResult;
use nom::bytes::complete::take_while1;
use nom::character::complete::satisfy;
use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticCode, DiagnosticKind, DiagnosticMetadata,
    Error, Format, LimitKind, LocatedError, ParseResult, Result, Span, Strictness,
};
use crate::reader::Reader;

pub struct UnicodeMathReader;

/// Decode the text after an opening quote and return the byte offset of the
/// closing quote. UTN #28 sections 3.2 and 3.15 use `\"` for a quote and
/// `\\` for a backslash. A backslash before the last quote remains literal.
fn scan_quoted_text(body: &str, mut push: impl FnMut(char)) -> Option<usize> {
    let mut chars = body.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => return (index != 0).then_some(index),
            '\\' if body[index + 1..].starts_with('\\') => {
                push('\\');
                chars.next();
            },
            '\\' if body[index + 1..].starts_with('"') && body[index + 2..].contains('"') => {
                push('"');
                chars.next();
            },
            _ => push(ch),
        }
    }
    None
}

/// Characters outside quoted text, with byte offsets in the original input.
fn unquoted_chars(source: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut rest = source;
    std::iter::from_fn(move || {
        loop {
            let index = source.len() - rest.len();
            let ch = rest.chars().next()?;
            rest = &rest[ch.len_utf8()..];
            if ch == '"'
                && let Some(end) = scan_quoted_text(rest, |_| {})
            {
                rest = &rest[end + 1..];
                continue;
            }
            return Some((index, ch));
        }
    })
}

/// Index balanced parentheses and curly braces by opening offset for lookups
/// during parsing. Both indexes share one quote-aware scan so nested matching
/// stays linear in the input length.
///
/// A legacy one-sided cases spelling starts with a brace but has no matching
/// closing brace. Keeping this index lets that spelling coexist with an
/// ordinary braced group whose body starts with a matrix, without consuming
/// the group's trailing atoms as part of the legacy form.
fn delimiter_ends(source: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    if !source
        .as_bytes()
        .iter()
        .any(|byte| matches!(*byte, b'(' | b'{'))
    {
        return (Vec::new(), Vec::new());
    }
    let mut parens = Vec::new();
    let mut braces = Vec::new();
    let mut paren_open: Vec<usize> = Vec::new();
    let mut brace_open: Vec<usize> = Vec::new();
    for (index, character) in unquoted_chars(source) {
        match character {
            '(' => {
                paren_open.push(parens.len());
                parens.push((index, 0));
            },
            ')' => {
                if let Some(at) = paren_open.pop() {
                    parens[at].1 = index + 1;
                }
            },
            '{' => {
                brace_open.push(braces.len());
                braces.push((index, 0));
            },
            '}' => {
                if let Some(at) = brace_open.pop() {
                    braces[at].1 = index + 1;
                }
            },
            _ => {},
        }
    }
    parens.retain(|&(_, end)| end != 0);
    braces.retain(|&(_, end)| end != 0);
    (parens, braces)
}

#[cfg(test)]
fn paren_ends(source: &str) -> Vec<(usize, usize)> {
    delimiter_ends(source).0
}

/// Find the top-level `&` in the body of an nth-root argument. UnicodeMath
/// uses the same separator inside matrix and other grouped constructs, so only
/// an ampersand outside all nested delimiters belongs to the root itself.
fn root_separator(input: &str) -> Option<usize> {
    let mut round = 0_u32;
    let mut square = 0_u32;
    let mut curly = 0_u32;
    for (index, character) in unquoted_chars(input) {
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
        Self::parse_compact_located(input, config).map_err(LocatedError::into_error)
    }
}

impl UnicodeMathReader {
    /// Internal location-aware parser for the additive detailed error API.
    pub(crate) fn parse_compact_located(
        input: &str,
        config: &Config,
    ) -> std::result::Result<CompactParseResult, LocatedError> {
        config.check_input_size(input)?;
        // A typographic space is content even at the ends.
        let lead = input.len() - skip_blanks(input).len();
        let input = input.trim_matches(is_blank);
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
                let (diagnostics, metadata) = parser.skip_diagnostics();
                Ok(CompactParseResult::with_diagnostic_metadata(
                    CompactExpr::new(parser.arena, roots, DisplayStyle::Inline),
                    diagnostics,
                    metadata,
                ))
            },
            // Resource limits hold in every mode.
            Err(e @ Error::LimitExceeded { .. }) => Err(e.into()),
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
                    Err(LocatedError::with_span(
                        e,
                        Some(Format::UnicodeMath),
                        parser.last_failure_offset.map(|offset| {
                            let local = crate::error::safe_byte_span(input, offset);
                            Span::new(local.start + lead, local.end + lead)
                        }),
                    ))
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
const MAX_NARY_CONTROL_WARNINGS: usize = 32;
const MAX_EQUATION_NUMBER_WARNINGS: usize = 32;

/// The operand being read in a list.
#[derive(Default)]
struct OperandRun {
    /// Index in the list where it starts.
    start: usize,
    /// The open bars of the list: the offset of the closing one, and where
    /// the operand around the pair starts.
    open: SmallVec<[(usize, usize); 2]>,
}

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
    /// Whether to retain conservative ranges for source-backed nodes.
    source_ranges: bool,
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
    /// The balanced parentheses (see [`paren_ends`]).
    paren_ends: Vec<(usize, usize)>,
    /// The balanced curly braces (see [`brace_ends`]).
    brace_ends: Vec<(usize, usize)>,
    /// The paired bars and brackets (see [`bars`]).
    pairs: bars::Pairs,
    /// Offsets of pairs whose content does not parse.
    failed_pairs: BTreeSet<usize>,
    /// Byte range of a run of `.` that is known not to start a number.
    dot_run: (usize, usize),
    /// Matrices of the result that skipped content (Lenient mode), in the
    /// order they were completed (a nested matrix before the one around it).
    /// Formatted into diagnostics only once the parse succeeded.
    matrix_skips: Vec<MatrixSkips>,
    /// Unsupported UTN 28 n-ary control values, reported after parsing.
    unsupported_nary_controls: Vec<(usize, u8)>,
    /// Equation-number suffixes accepted by UTN 28 but not represented in the
    /// shared AST.
    equation_number_losses: Vec<(usize, usize)>,
    /// Legacy one-sided brace forms canonicalized as the UnicodeMath cases
    /// operator.
    legacy_case_warnings: Vec<(usize, usize)>,
    /// Structured input left over after the element parser stopped. The
    /// parser intentionally keeps this as text for compatibility, but the
    /// loss of structure must be observable to callers.
    literal_fallback: Option<Span>,
    /// Matrix cells created but not yet stored as nodes; they count towards
    /// [`Config::max_nodes`] as soon as they are created.
    pending_cells: usize,
    /// Whether the element about to be parsed may be a script with no base
    /// (`_1 F`): set by the element loops at the start of a list and after
    /// white space, and taken by `parse_element`.
    baseless_ok: bool,
    /// Byte offset of the latest strict parse attempt that failed.
    last_failure_offset: Option<usize>,
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
        let (paren_ends, brace_ends) = delimiter_ends(source);
        Self {
            arena: AstArena::new(),
            source_len: source.len(),
            pairs: bars::pair_bars(source),
            paren_ends,
            brace_ends,
            failed_pairs: BTreeSet::new(),
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
            source_ranges: config.source_ranges,
            abort: None,
            failed: Vec::new(),
            dead_walk: Vec::new(),
            dot_run: (0, 0),
            matrix_skips: Vec::new(),
            unsupported_nary_controls: Vec::new(),
            equation_number_losses: Vec::new(),
            legacy_case_warnings: Vec::new(),
            literal_fallback: None,
            pending_cells: 0,
            baseless_ok: false,
            last_failure_offset: None,
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

    /// The byte index in `input`, which starts with `(`, just after the `)`
    /// that closes it.
    fn matching_paren_end(&self, input: &str) -> Option<usize> {
        debug_assert!(input.starts_with('('));
        let at = self.offset(input);
        let found = self
            .paren_ends
            .binary_search_by_key(&at, |&(open, _)| open)
            .ok()?;
        Some(self.paren_ends[found].1 - at)
    }

    /// The byte index just after the `}` that closes a brace at `input`.
    fn matching_brace_end(&self, input: &str) -> Option<usize> {
        debug_assert!(input.starts_with('{'));
        let at = self.offset(input);
        let found = self
            .brace_ends
            .binary_search_by_key(&at, |&(open, _)| open)
            .ok()?;
        Some(self.brace_ends[found].1 - at)
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
        let mut run = OperandRun::default();
        let mut spaced = false;
        loop {
            // Whitespace separates UnicodeMath operands (it triggers "build up"
            // of the preceding subexpression); it is not literal text. Skip it
            // between elements so parsing continues past spaces.
            let trimmed = skip_blanks(remaining);
            spaced |= trimmed.len() != remaining.len();
            self.baseless_ok = ids.is_empty() || spaced;
            remaining = trimmed;
            if remaining.is_empty() {
                break;
            }
            if !ids.is_empty()
                && let Some((rest, start, end)) = self.take_equation_number(remaining)
            {
                if self.equation_number_losses.len() < MAX_EQUATION_NUMBER_WARNINGS {
                    self.equation_number_losses.push((start, end));
                }
                remaining = rest;
                spaced = true;
                continue;
            }
            // UTN #28 section 3.16: the other spaces are not removed on
            // build up (LaTeX `\,` is U+2009).
            if let Some((rest, width)) = unicode_math_space(remaining) {
                let at = self.offset(remaining);
                let id = self.push_node(CompactNode::Space(width));
                remaining = match self.push_operand(&mut ids, &mut run, true, at, id, rest) {
                    Ok(rest) => rest,
                    Err(_) => return Err(self.parse_failure()),
                };
                spaced = true;
                continue;
            }
            let spaced = std::mem::take(&mut spaced);
            let at = self.offset(remaining);
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    remaining = match self.push_operand(&mut ids, &mut run, spaced, at, id, rest) {
                        Ok(rest) => rest,
                        Err(_) => return Err(self.parse_failure()),
                    };
                },
                Err(nom::Err::Failure(_)) => return Err(self.parse_failure()),
                Err(_) => break,
            }
        }
        let remaining = remaining.trim_matches(is_blank);
        if !remaining.is_empty() {
            let start = self.offset(remaining);
            let end = start + remaining.len();
            self.literal_fallback = Some(Span::new(start, end));
            let sid = self.arena.push_str(remaining);
            ids.push(self.push_node(CompactNode::Text(TextStyle::Normal, sid)));
        }
        if let Some(error) = self.abort.take() {
            return Err(error);
        }
        Ok(ids)
    }

    /// Consume the UTN 28 equation-number suffix (#(30) or #30). The shared
    /// AST has no equation-number field, so the number is accepted and
    /// reported as an explicit metadata loss instead of turning the whole
    /// equation into literal text.
    fn take_equation_number<'a>(&self, input: &'a str) -> Option<(&'a str, usize, usize)> {
        let after_hash = input.strip_prefix('#')?;
        let consumed = if after_hash.starts_with('(') {
            self.matching_paren_end(after_hash)?
        } else {
            after_hash
                .char_indices()
                .find(|(_, ch)| ch.is_whitespace())
                .map_or(after_hash.len(), |(at, _)| at)
        };
        if consumed == 0 {
            return None;
        }
        let start = self.offset(input);
        Some((&after_hash[consumed..], start, start + 1 + consumed))
    }

    /// The error that aborted the parse.
    fn parse_failure(&mut self) -> Error {
        self.abort
            .take()
            .unwrap_or_else(|| Error::UnicodeMathParse {
                message: "UnicodeMath parse error".into(),
            })
    }

    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let source_start = self.offset(input) + self.lead;
        let result = self.parse_element_inner(input);
        if self.source_ranges
            && let Ok((rest, id)) = &result
        {
            let source_end = self.offset(rest) + self.lead;
            if source_start < source_end {
                self.arena
                    .set_source_range(*id, crate::error::Span::new(source_start, source_end));
            }
        }
        result
    }

    fn parse_element_inner<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.last_failure_offset = Some(self.offset(input));
        let baseless_ok = std::mem::take(&mut self.baseless_ok);
        self.enter(input)?;
        let result = self.parse_operand(input, baseless_ok);
        self.depth -= 1;
        result
    }

    /// One operand with its scripts: an atom, a prescripted operand, or (when
    /// `baseless_ok`) a script with no base.
    fn parse_operand<'a>(&mut self, input: &'a str, baseless_ok: bool) -> IResult<&'a str, NodeId> {
        if (input.starts_with("(_") || input.starts_with("(^"))
            && let Ok(found) = self.parse_prescripts(input)
        {
            return Ok(found);
        }
        // UTN #28 section 3.3: a script with no base before it (`_1 F`,
        // TeX's `{}_1 F`) is a script on an empty base; normalization makes
        // it a leading script of what follows. Only at the start of an
        // expression or after a space ("If a variable precedes the
        // prescript, you also need to precede the prescript with a space"):
        // right after a script it continues that script (`a_b_c`, `a^b_c`).
        if baseless_ok && input.starts_with(['_', '^']) {
            let empty = self.push_node(CompactNode::Row(NodeSpan::EMPTY));
            let (rest, id) = self.parse_attached_scripts(input, empty)?;
            if rest.len() == input.len() {
                return Err(parse_error(input, nom::error::ErrorKind::Char));
            }
            return Ok((rest, id));
        }
        let (input, base) = self.parse_atom(input)?;
        self.parse_attached_scripts(input, base)
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
                let (after, id) = self.parse_script_operand(after)?;
                *rest = after;
                Ok(id)
            };
        let pre_sub = script(&mut rest, '_')?;
        let pre_sup = script(&mut rest, '^')?;
        let rest = rest.strip_prefix(')').ok_or_else(|| fail(rest))?;
        if pre_sub.is_none() && pre_sup.is_none() {
            return Err(fail(input));
        }
        // The base is one operand: a fraction after it takes the prescripted
        // operand as its numerator (`(_1)a/b` is `\frac{{}_1 a}{b}`).
        self.enter(rest)?;
        let operand = self.parse_operand(rest, false);
        self.depth -= 1;
        let (rest, mut base) = operand?;
        // A grouped fraction is the base itself (`(_1)〖a/b〗`); a grouped
        // row keeps its group, as after `_1 `.
        if let CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            content,
        } = *self.arena.node(base)
            && matches!(*self.arena.node(content), CompactNode::Fraction { .. })
        {
            base = content;
        }
        let id = self.arena.push_node(CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        });
        Ok((rest, id))
    }

    /// The fraction `num` / denominator at `input` (which starts with `/`
    /// or `¦`), or `num` alone when no denominator follows.
    fn fraction_after<'a>(&mut self, input: &'a str, num: NodeId) -> IResult<&'a str, NodeId> {
        // A fraction: numerator / denominator, or the barless stack
        // numerator ¦ denominator.
        let fraction = if let Some(rest) = input.strip_prefix('/') {
            Some((rest, FractionKind::Normal))
        } else {
            input
                .strip_prefix('\u{A6}')
                .map(|rest| (rest, FractionKind::NoBar))
        };
        let Some((rest, kind)) = fraction else {
            return Ok((input, num));
        };
        match self.parse_denominator(rest) {
            Ok((rest, den)) => {
                let id = self.push_node(CompactNode::Fraction { kind, num, den });
                Ok((rest, id))
            },
            Err(nom::Err::Failure(e)) => Err(nom::Err::Failure(e)),
            Err(_) => Ok((input, num)),
        }
    }

    /// A fraction's denominator: an operand with its scripts (`a/b^2` is
    /// `\frac{a}{b^2}`). Parentheses around the whole denominator group it
    /// (`a/(b+c)`); before a script they are the script's visible base, as
    /// they are anywhere else (`a/(b+c)^2`), and so is an invisible group
    /// (`a/〖〗_1`).
    fn parse_denominator<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let pair = self.pair_end(input).is_some();
        let (mut rest, den) = self.parse_grouped_operand(input)?;
        // UTN #28 section 2.1: the denominator is the whole operand, the
        // factors up to a space or an operator (`(−b±√(b^2−4ac))/2a`).
        let mut factors = match *self.arena.node(den) {
            CompactNode::Row(span) if pair => SmallVec::from_slice(self.arena.children(span)),
            _ => SmallVec::<[NodeId; 8]>::from_slice(&[den]),
        };
        let ends_operand = |this: &Self, id: NodeId, read: &str| {
            this.breaks_operand(id) || read.ends_with(char::is_whitespace)
        };
        let mut done = ends_operand(self, den, &input[..input.len() - rest.len()]);
        while !done
            && rest.starts_with(|c: char| c.is_alphanumeric() || matches!(c, '(' | '〖' | '!'))
        {
            self.enter(rest)?;
            let factor = self.parse_operand(rest, false);
            self.depth -= 1;
            match factor {
                Ok((after, id)) if after.len() < rest.len() => {
                    done = ends_operand(self, id, &rest[..rest.len() - after.len()]);
                    factors.push(id);
                    rest = after;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                _ => break,
            }
        }
        Ok((rest, self.flatten_single(&factors)))
    }

    /// Whether `base` takes the primes after it as its superscript (UTN #28
    /// section 3.10: a prime follows a variable). A prime does not (`′′` is
    /// two primes), nor does an operator, an opening delimiter or
    /// punctuation (`a+′`).
    fn takes_prime(&self, base: NodeId) -> bool {
        match *self.arena.node(base) {
            CompactNode::Symbol(_, sid) => symbol_takes_prime(self.arena.str_val(sid)),
            _ => true,
        }
    }

    /// The closing fence symbol at the bottom of `id`'s scripts and accents
    /// (`|` in `|x|^2`), whose numerator is the whole fenced run.
    fn closing_fence(&self, mut id: NodeId) -> Option<char> {
        loop {
            id = match *self.arena.node(id) {
                CompactNode::Subscript { base, .. }
                | CompactNode::Superscript { base, .. }
                | CompactNode::SubSup { base, .. }
                | CompactNode::Accent { base, .. } => base,
                CompactNode::Symbol(_, sid) => {
                    let mut chars = self.arena.str_val(sid).chars();
                    return chars
                        .next()
                        .filter(|_| chars.next().is_none())
                        .filter(|&c| opening_fence(c).is_some());
                },
                _ => return None,
            };
        }
    }

    /// The content of a parenthesized numerator (`(a+b)/c`, UTN #28 section
    /// 2.1: the parentheses only group it), or `num` itself.
    fn unparenthesized(&self, num: NodeId) -> NodeId {
        if let CompactNode::Fenced {
            open,
            close,
            content,
        } = *self.arena.node(num)
            && self.arena.str_val(open) == "("
            && self.arena.str_val(close) == ")"
            && let [inner] = self.arena.children(content)
        {
            return *inner;
        }
        num
    }

    /// `id` without the group around a fraction when it follows a script
    /// with no base, whose leading script it takes (`_1 〖a/b〗` is
    /// `(_1)〖a/b〗`). A grouped row keeps its group, which keeps it one
    /// base (`〖〗_1〖a+b〗`).
    fn ungroup_after_leading_script(&self, ids: &[NodeId], id: NodeId) -> NodeId {
        let leading = ids
            .last()
            .is_some_and(|&last| match *self.arena.node(last) {
                CompactNode::Subscript { base, .. }
                | CompactNode::Superscript { base, .. }
                | CompactNode::SubSup { base, .. } => {
                    matches!(*self.arena.node(base), CompactNode::Row(span) if span.is_empty())
                },
                _ => false,
            });
        match *self.arena.node(id) {
            CompactNode::MathAtom {
                atom_type: MathAtomType::Inner,
                content,
            } if leading && matches!(*self.arena.node(content), CompactNode::Fraction { .. }) => {
                content
            },
            _ => id,
        }
    }

    /// Whether the element `id` ends an operand: an operator, punctuation, an
    /// opening delimiter, a big operator or a space. The factors between
    /// such elements (and white space) form one operand (UTN #28 section
    /// 2.1: `operand ← operand factor`).
    fn breaks_operand(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            // A struck operator is an operator (`≴`).
            CompactNode::Accent {
                base,
                accent: '\u{0338}',
                ..
            } => self.breaks_operand(base),
            // A grouped n-ary operator is one factor (`〖∑ a〗^2/b`).
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. }
                if matches!(*self.arena.node(base), CompactNode::Nary { body, .. }
                    if !matches!(*self.arena.node(body), CompactNode::Row(span) if span.is_empty())) =>
            {
                false
            },
            // A scripted operator is still an operator (`(^2)+`, `+_1`).
            CompactNode::PreScripts { base, .. }
            | CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. } => self.breaks_operand(base),
            // The kind is the character's class (see `breaks_operand_at`).
            CompactNode::Symbol(kind, sid) => {
                class_breaks_operand(kind) || self.arena.str_val(sid) == "?"
            },
            CompactNode::Nary { .. } | CompactNode::Space(_) | CompactNode::Text(..) => true,
            _ => false,
        }
    }

    /// Push the element `id` (at offset `at`, after white space when `spaced`) onto
    /// `ids`, where the current operand is `run`. When a
    /// fraction operator follows, the whole operand is the numerator (UTN #28
    /// section 2.1: `f(x)/2` is `\frac{f(x)}{2}`, `abc/d` is
    /// `\frac{abc}{d}`); parentheses around a numerator that is a whole
    /// operand only group it (`(a+b)/c`), and a closing fence takes the run
    /// from its opening fence (`|a+b|^2/c`). Fractions nest to the left
    /// (`a/b/c`). Returns the input after the element and its fractions.
    fn push_operand<'a, A: smallvec::Array<Item = NodeId>>(
        &mut self,
        ids: &mut SmallVec<A>,
        run: &mut OperandRun,
        spaced: bool,
        at: usize,
        id: NodeId,
        mut rest: &'a str,
    ) -> std::result::Result<&'a str, nom::Err<nom::error::Error<&'a str>>> {
        if spaced {
            run.start = ids.len();
        }
        let id = self.ungroup_after_leading_script(ids, id);
        ids.push(id);
        let fraction = rest.starts_with(['/', '\u{A6}']);
        // A dagger right before a fraction bar is the adjoint's (`a†/b`), as
        // no operand follows it.
        let adjoint = fraction
            && matches!(*self.arena.node(id), CompactNode::Symbol(_, sid)
                if matches!(self.arena.str_val(sid), "\u{2020}" | "\u{2021}"));
        let fence = !self.pairs.is_empty() && self.follow_fence(run, at, ids.len());
        if !fence {
            if self.breaks_operand(id) && !adjoint {
                run.start = ids.len();
                return Ok(rest);
            }
            // UTN #28 section 3.1 and table 3.1: a bar that divides is a
            // delimiter (`P(A|B/C)`).
            if !fraction
                && matches!(*self.arena.node(id), CompactNode::Symbol(SymbolKind::Fence, sid)
                    if matches!(self.arena.str_val(sid), "|" | "\u{2016}"))
            {
                run.start = ids.len();
                return Ok(rest);
            }
        }
        while rest.starts_with(['/', '\u{A6}']) && negated_operator(rest).is_none() {
            let (&last, before) = ids.split_last().expect("an element was pushed");
            let mut start = run.start.min(before.len());
            if !fence
                && let Some(open) = self
                    .closing_fence(last)
                    .filter(|c| !matches!(c, '|' | '\u{2016}'))
                    .and_then(opening_fence)
                && let Some(opener) = before.iter().rposition(|&id| {
                    matches!(*self.arena.node(id), CompactNode::Symbol(_, sid)
                        if self.arena.str_val(sid).chars().eq(std::iter::once(open)))
                })
            {
                start = start.min(opener);
            }
            // A zero-width space after other factors is an empty atom, not a
            // null numerator.
            if ids.len() - start > 1
                && matches!(*self.arena.node(last), CompactNode::Row(span) if span.is_empty())
            {
                let atom = self.push_node(CompactNode::MathAtom {
                    atom_type: MathAtomType::Ord,
                    content: last,
                });
                ids.pop();
                ids.push(atom);
            }
            let num = if ids.len() - start == 1 {
                self.unparenthesized(last)
            } else {
                let span = self.arena.push_children_deferred(&ids[start..]);
                self.push_node(CompactNode::Row(span))
            };
            match self.fraction_after(rest, num) {
                Ok((after, fraction)) if after.len() < rest.len() => {
                    ids.truncate(start);
                    ids.push(fraction);
                    run.start = start;
                    rest = after;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                _ => break,
            }
        }
        Ok(rest)
    }

    /// The offset of the closing delimiter of the bar pair, bra, ket or bracket
    /// group that opens at `input`.
    fn pair_end(&self, input: &str) -> Option<usize> {
        if !input.starts_with(['|', '\u{2016}', '\u{27E8}', '\u{2308}', '\u{230A}']) {
            return None;
        }
        let at = self.offset(input);
        let find = |pairs: &[(usize, usize)]| {
            let index = pairs.binary_search_by_key(&at, |&(open, _)| open).ok()?;
            Some(pairs[index].1).filter(|&close| close > at)
        };
        find(&self.pairs.bars).or_else(|| find(&self.pairs.groups))
    }

    /// The bar pair or bracket group that opens at `input` as one operand
    /// (appendix A `expBracket`), with the scripts of its closing delimiter
    /// when `scripts`. `None` when none opens there or its content does not
    /// parse.
    fn parse_pair<'a>(
        &mut self,
        input: &'a str,
        scripts: bool,
    ) -> std::result::Result<Option<(&'a str, NodeId)>, nom::Err<nom::error::Error<&'a str>>> {
        let Some(end) = self.pair_end(input) else {
            return Ok(None);
        };
        let start = self.offset(input);
        if self.failed_pairs.contains(&start) {
            return Ok(None);
        }
        self.enter(input)?;
        let parsed = self.parse_pair_content(input, end, scripts);
        self.depth -= 1;
        match parsed {
            Ok((rest, ids)) => Ok(Some((rest, self.flatten_single(&ids)))),
            Err(nom::Err::Failure(e)) => Err(nom::Err::Failure(e)),
            Err(_) => {
                self.failed_pairs.insert(start);
                Ok(None)
            },
        }
    }

    fn parse_pair_content<'a>(
        &mut self,
        input: &'a str,
        end: usize,
        scripts: bool,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut run = OperandRun::default();
        let mut rest = input;
        let mut spaced = false;
        loop {
            self.tick(1);
            let trimmed = skip_blanks(rest);
            spaced |= trimmed.len() != rest.len();
            rest = trimmed;
            let at = self.offset(rest);
            if at >= end {
                break;
            }
            if let Some((after, width)) = unicode_math_space(rest) {
                let id = self.push_node(CompactNode::Space(width));
                rest = self.push_operand(&mut ids, &mut run, true, at, id, after)?;
                spaced = true;
                continue;
            }
            self.baseless_ok = spaced;
            let (after, id) = self.parse_element(rest)?;
            let spaced = std::mem::take(&mut spaced);
            rest = self.push_operand(&mut ids, &mut run, spaced, at, id, after)?;
        }
        if self.offset(rest) != end {
            return Err(parse_error(rest, nom::error::ErrorKind::Verify));
        }
        let (rest, close) = if scripts {
            self.parse_element(rest)?
        } else {
            self.parse_symbol_char(rest)?
        };
        ids.push(close);
        Ok((rest, ids))
    }

    /// Follow a bar, bra or ket delimiter pushed as the last of `len` elements
    /// (at offset `at`): an opening one starts an operand (`|x/y|`), a
    /// closing one goes on with the operand the opening one was in
    /// (`a|x|/y`). Returns whether it was one. See [`bars`].
    fn follow_fence(&self, run: &mut OperandRun, at: usize, len: usize) -> bool {
        let find = |pairs: &[(usize, usize)]| {
            let index = pairs.binary_search_by_key(&at, |&(bar, _)| bar).ok()?;
            Some(pairs[index].1)
        };
        let Some(partner) = find(&self.pairs.bars).or_else(|| find(&self.pairs.groups)) else {
            return false;
        };
        if partner > at {
            run.open.push((partner, run.start));
            run.start = len;
            return true;
        }
        while run.open.last().is_some_and(|&(close, _)| close < at) {
            run.open.pop();
        }
        if let Some(&(close, outer)) = run.open.last()
            && close == at
        {
            run.open.pop();
            run.start = outer;
        }
        true
    }

    /// The script at `input`: `mark` and its operand, or a span of Unicode
    /// super- or subscript characters (UTN #28 section 3.12: `x²`, `a₁₂`).
    fn parse_script<'a>(
        &mut self,
        input: &'a str,
        mark: char,
    ) -> std::result::Result<Option<(&'a str, NodeId)>, nom::Err<nom::error::Error<&'a str>>> {
        if let Some(after) = input.strip_prefix(mark) {
            return self.parse_script_chain(after, mark);
        }
        if !starts_script_character(input, mark) {
            return Ok(None);
        }
        Ok(self.parse_script_characters(input, mark))
    }

    fn parse_script_characters<'a>(
        &mut self,
        mut input: &'a str,
        mark: char,
    ) -> Option<(&'a str, NodeId)> {
        let mut factors = SmallVec::<[NodeId; 8]>::new();
        // Factors before an open `⁽`, and the input from it.
        let mut open: Option<(SmallVec<[NodeId; 8]>, &'a str)> = None;
        let mut digits = String::new();
        let mut buf = [0_u8; 4];
        while let Some(c) = input.chars().next().filter(|c| !c.is_ascii())
            && let Some((character, _)) = script_character(c).filter(|&(_, of)| of == mark)
        {
            self.tick(1);
            if (character == '(' && open.is_some()) || (character == ')' && open.is_none()) {
                break;
            }
            let at = input;
            input = &input[c.len_utf8()..];
            if character.is_ascii_digit() {
                digits.push(character);
                continue;
            }
            self.push_script_digits(&mut factors, &mut digits);
            if character == '(' {
                open = Some((std::mem::take(&mut factors), at));
                continue;
            }
            let id = if character == ')' {
                let content = self.flatten_single(&factors);
                let (before, _) = open.take()?;
                factors = before;
                let content = self.arena.push_children_deferred(&[content]);
                let (open, close) = (self.arena.push_str("("), self.arena.push_str(")"));
                CompactNode::Fenced {
                    open,
                    close,
                    content,
                }
            } else {
                let sid = self.arena.push_str(character.encode_utf8(&mut buf));
                if character.is_alphabetic() {
                    CompactNode::Identifier(sid)
                } else {
                    CompactNode::Symbol(classify_unicode_symbol(character), sid)
                }
            };
            factors.push(self.push_node(id));
        }
        self.push_script_digits(&mut factors, &mut digits);
        // A `⁽` that nothing closes ends the span.
        if let Some((before, at)) = open {
            factors = before;
            input = at;
        }
        (!factors.is_empty()).then(|| (input, self.flatten_single(&factors)))
    }

    fn push_script_digits(&mut self, factors: &mut SmallVec<[NodeId; 8]>, digits: &mut String) {
        if !digits.is_empty() {
            let sid = self.arena.push_str(digits);
            factors.push(self.push_node(CompactNode::Number(sid)));
            digits.clear();
        }
    }

    /// A script argument at `input` (after `mark`), with the same scripts
    /// that follow it nested right to left (UTN #28 section 2.2: `a_b_c` is
    /// `a_(b_c)`, `a^b^c` is `a^(b^c)`). `None` when no argument parses.
    fn parse_script_chain<'a>(
        &mut self,
        input: &'a str,
        mark: char,
    ) -> std::result::Result<Option<(&'a str, NodeId)>, nom::Err<nom::error::Error<&'a str>>> {
        let (mut rest, mut factors) = match self.parse_script_factors(input) {
            Ok(found) => found,
            Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
            Err(_) => return Ok(None),
        };
        loop {
            let characters = starts_script_character(rest, mark);
            if !characters && !rest.starts_with(mark) {
                break;
            }
            self.enter(rest)?;
            let nested = self.parse_script(rest, mark);
            self.depth -= 1;
            let Some((after, script)) = nested? else {
                break;
            };
            // The script nests on the last factor (`a_ij_k` is
            // `a_(i j_k)`, as `ij_k` is `i j_k` outside a script).
            let base = factors.pop().expect("a script operand has a factor");
            factors.push(self.push_node(if mark == '_' {
                CompactNode::Subscript { base, sub: script }
            } else {
                CompactNode::Superscript { base, sup: script }
            }));
            rest = after;
            if !characters {
                break;
            }
            // Script characters end with their span (`W_δ₁ρ₁σ₂`).
            rest = self.continue_run(false, rest, &mut factors)?;
        }
        Ok(Some((rest, self.flatten_single(&factors))))
    }

    /// A script operand as one node (see [`Self::parse_script_factors`]).
    fn parse_script_operand<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, factors) = self.parse_script_factors(input)?;
        Ok((rest, self.flatten_single(&factors)))
    }

    /// The factors of a sub- or superscript operand. UTN #28 section 2.2: "A
    /// simple subscript operand consists of the string of one or more
    /// characters with the Unicode General Categories Lx (alphabetic) and Nd
    /// (decimal digits)" (`δ_μν`, `a_ij`, `x^2y`; `a_i+1` stops at `+`).
    /// Appendix A, `soperand ← operand | '∞' | '-' operand | "-∞"`, and
    /// a sign belongs to the operand after it, which for a function name is
    /// the name alone (`e^-sin x` is `e^{-\sin}x`: section 2.3 ends the script
    /// at the space, before the argument);
    /// section 3.22 ("math minus (U+2212) or plus if either starts a
    /// sub/superscript operand"): a sign before an operand is part of it
    /// (`e^-x`, `x^-1`); parentheses after the sign are visible (`x^-(a+b)`).
    fn parse_script_factors<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut factors = SmallVec::new();
        let mut rest = input;
        if let Some(sign) = input
            .chars()
            .next()
            .filter(|c| matches!(c, '-' | '+' | '\u{2212}'))
            && let after = &input[sign.len_utf8()..]
            && (after.starts_with(starts_signed_script_operand) || self.pair_end(after).is_some())
        {
            let (after, sign) = self.parse_symbol(input)?;
            factors.push(sign);
            let operand = if after.starts_with('(') {
                self.parse_group_as(after, false)
            } else {
                self.parse_script_atom(after)
            };
            match operand {
                Ok((after, id)) => {
                    factors.push(id);
                    rest = after;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => return Ok((after, factors)),
            }
        }
        if factors.is_empty() {
            let (after, id) = self.parse_script_atom(input)?;
            factors.push(id);
            rest = after;
        }
        let rest = self.continue_operand(input, rest, &mut factors)?;
        Ok((rest, factors))
    }

    /// The letters and digits at `rest` that go on with the simple operand
    /// `factors` (which ends in a letter when `after_letter`), and the input
    /// after them.
    fn continue_run<'a>(
        &mut self,
        mut after_letter: bool,
        mut rest: &'a str,
        factors: &mut SmallVec<[NodeId; 8]>,
    ) -> std::result::Result<&'a str, nom::Err<nom::error::Error<&'a str>>> {
        // UTN #28 does not say where a run meets a function name: section
        // 2.2 takes the whole run and section 3.5 recognizes function
        // names. Here the run ends before a maximal span of letters that
        // is a function name in full (`x^2sin x`), and goes on through
        // one that merely ends in a name (`x_kmax`, `x_basin`).
        while let Some(c) = rest.chars().next().filter(|&c| is_run_char(c))
            && (after_letter || !starts_function_span(rest))
        {
            let (after, id) = self.parse_single_char_node(rest)?;
            let (after, id) = self.with_accents(after, id);
            self.tick(1);
            factors.push(id);
            rest = after;
            after_letter = is_run_letter(c);
        }
        Ok(rest)
    }

    /// [`Self::continue_run`] when the last of `factors`, read from `input`,
    /// is a letter or a number.
    fn continue_operand<'a>(
        &mut self,
        input: &'a str,
        rest: &'a str,
        factors: &mut SmallVec<[NodeId; 8]>,
    ) -> std::result::Result<&'a str, nom::Err<nom::error::Error<&'a str>>> {
        if factors.last().is_some_and(|&id| self.is_run_factor(id)) {
            let read = &input[..input.len() - rest.len()];
            // A group is an operand of its own (`√(x)y`, `x_(a)b`).
            if read.ends_with([')', '〗']) {
                return Ok(rest);
            }
            let after_letter = read.chars().next_back().is_some_and(is_run_letter);
            return self.continue_run(after_letter, rest, factors);
        }
        Ok(rest)
    }

    /// The operand of a radical (UTN #28 section 3.6: `√abc`).
    fn parse_radicand<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, first) = self.parse_script_arg(input)?;
        let mut factors = SmallVec::from_slice(&[first]);
        let rest = self.continue_operand(input, rest, &mut factors)?;
        Ok((rest, self.flatten_single(&factors)))
    }

    /// The first atom of a script operand: a function name (`x_max`, read
    /// as in `x_(max)`) or a script argument.
    fn parse_script_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        match self.parse_operator_word(input) {
            // Its accents are part of it, as of any operand (`x_max̅`).
            Ok((rest, id)) => Ok(self.with_accents(rest, id)),
            Err(_) => self.parse_script_arg(input),
        }
    }

    /// Whether `id` is a letter or a number (possibly accented), which the
    /// letters and digits after it in a script continue.
    fn is_run_factor(&self, mut id: NodeId) -> bool {
        loop {
            id = match *self.arena.node(id) {
                CompactNode::Accent { base, .. } => base,
                CompactNode::Identifier(_) | CompactNode::Number(_) => return true,
                _ => return false,
            };
        }
    }

    fn parse_attached_scripts<'a>(
        &mut self,
        input: &'a str,
        base: NodeId,
    ) -> IResult<&'a str, NodeId> {
        // Most atoms have none.
        let mut input = input;
        match input.as_bytes().first() {
            None => return Ok((input, base)),
            Some(b'_' | b'^' | b'\'') => {},
            Some(b' ' | 0xE2) => {
                // UTN #28 section 3.5: a script after function apply is the
                // function name's (`sin⁡^2 x`).
                let after = skip_blanks(input);
                if after.len() < input.len()
                    && starts_script(after)
                    && input[..input.len() - after.len()].ends_with('\u{2061}')
                {
                    input = after;
                } else if input.starts_with(' ') {
                    return Ok((input, base));
                }
            },
            Some(byte) if byte.is_ascii() => return Ok((input, base)),
            Some(_) => {},
        }
        if input.as_bytes().first().is_some_and(|byte| *byte >= 0x80)
            && let Some((rest, accent, position, wide)) = unicode_accent_suffix(input)
        {
            let id = self.push_node(CompactNode::Accent {
                base,
                accent,
                position,
                wide,
            });
            return self.parse_attached_scripts(rest, id);
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
                let (tail, argument) = self.parse_script_operand(rest)?;
                forced.push((marker, argument));
                rest = tail;
            }
            let id = self.apply_forced_bounds(base, &forced);
            return self.parse_attached_scripts(rest, id);
        }

        // UTN #28 section 3.10: primes after a base are its superscript
        // (`x′`, LaTeX `x'`), merged with a superscript that follows
        // (`x′^c` is `x^{′c}`, LaTeX `x'^c`). As in TeX, a subscript may
        // come before or after the primes (`x′_1` and `x_1′` are both
        // `x_1^′`), and a superscript before a subscript (`a^b_c`).
        let takes_prime = self.takes_prime(base);
        let mut rest = input;
        let mut sup_parts = SmallVec::<[NodeId; 8]>::new();
        let mut sub = None;
        if takes_prime && rest.starts_with(is_prime) {
            (rest, sup_parts) = self.parse_primes(rest);
        }
        if let Some((after, id)) = self.parse_script(rest, '_')? {
            sub = Some(id);
            rest = after;
            if takes_prime && sup_parts.is_empty() && rest.starts_with(is_prime) {
                (rest, sup_parts) = self.parse_primes(rest);
            }
        }
        if let Some((after, id)) = self.parse_script(rest, '^')? {
            sup_parts.push(id);
            rest = after;
            // Primes after it are the same superscript's (`f²′`).
            if takes_prime && rest.starts_with(is_prime) {
                let (after, primes) = self.parse_primes(rest);
                sup_parts.extend(primes);
                rest = after;
            }
            if sub.is_none()
                && let Some((after, id)) = self.parse_script(rest, '_')?
            {
                sub = Some(id);
                rest = after;
            }
        }
        let sup = (!sup_parts.is_empty()).then(|| self.flatten_single(&sup_parts));
        let id = match (sub, sup) {
            (None, None) => return Ok((rest, base)),
            (Some(sub), None) => CompactNode::Subscript { base, sub },
            (None, Some(sup)) => CompactNode::Superscript { base, sup },
            (Some(sub), Some(sup)) => CompactNode::SubSup { base, sub, sup },
        };
        Ok((rest, self.push_node(id)))
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
        if let Some(found) = self.parse_pair(input, false)? {
            return Ok(found);
        }
        // Quoted text (`x_"max"`), as the writer spells it.
        if input.starts_with('"')
            && let Ok(quoted) = self.parse_text_quoted(input)
        {
            return Ok(quoted);
        }
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
        if input.starts_with('(') && self.matching_paren_end(input).is_some() {
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
        // A run of primes is one argument (`x^′′`), as after a base.
        if input.starts_with(is_prime) {
            let (rest, primes) = self.parse_primes(input);
            return Ok((rest, self.flatten_single(&primes)));
        }
        // Combining and wide accent suffixes are part of a single
        // UnicodeMath atom when used as a fraction/script operand. This
        // keeps a/b̅ (and `x^〖〗⃗`) and the corresponding structured operator
        // body from detaching the accent into a later text fallback.
        match self.parse_group_as(input, false) {
            Ok((rest, id)) => return Ok(self.with_accents(rest, id)),
            Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
            Err(_) => {},
        }
        if let Ok((rest, id)) = self.parse_single_char_node(input) {
            return Ok(self.with_accents(rest, id));
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// `id` with the accent suffixes at the start of `rest`.
    fn with_accents<'a>(&mut self, mut rest: &'a str, mut id: NodeId) -> (&'a str, NodeId) {
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
        (rest, id)
    }

    /// An operand of a fraction or an n-ary operator: parentheses around the
    /// whole operand only group it (`a/(b+c)`); otherwise an operand with
    /// its scripts, where parentheses before a script or an accent are
    /// visible (`a/(b+c)^2`).
    fn parse_grouped_operand<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if let Some(found) = self.parse_pair(input, true)? {
            return Ok(found);
        }
        let grouping = input.starts_with('(')
            && self.matching_paren_end(input).is_some_and(|end| {
                !starts_script(&input[end..]) && unicode_accent_suffix(&input[end..]).is_none()
            });
        if grouping {
            return self.parse_script_arg(input);
        }
        self.enter(input)?;
        let operand = self.parse_operand(input, false);
        self.depth -= 1;
        operand
    }

    /// Parse `(...)` in argument position, returning the inner content directly
    /// (the parentheses are absorbed as a grouping delimiter, not kept as a
    /// literal `Fenced`).
    fn parse_absorbing_paren_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // An operand may be prescripted (`√(_1)F`, `∑_n (_1)F_1`): `(_…)`
        // before a base is the prescript syntax, not a group.
        if (input.starts_with("(_") || input.starts_with("(^"))
            && let Ok(found) = self.parse_prescripts(input)
        {
            return Ok(found);
        }
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
        let mut spaced = true;
        let mut run = OperandRun::default();
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
                spaced = true;
                continue;
            }
            if remaining.starts_with(is_blank) {
                remaining = skip_blanks(remaining);
                spaced = true;
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
                spaced = true;
                continue;
            }
            self.baseless_ok = spaced;
            let at = self.offset(remaining);
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    let spaced = std::mem::take(&mut spaced);
                    remaining = self.push_operand(&mut children, &mut run, spaced, at, id, rest)?;
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
            parse_wide_accent,
            parse_box_phantom,
            parse_sqrt,
            parse_cbrt,
            parse_cases,
            parse_matrix,
            parse_legacy_cases,
            parse_text_quoted,
            parse_nary_op,
            parse_group,
            parse_number,
            parse_operator_word,
            parse_negated_operator,
            parse_symbol,
            parse_identifier,
            parse_prime,
            parse_zero_width_atom,
            parse_literal_solidus,
            parse_point,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Parse the prefix form of a wide over/under accent from UTN 28.  The
    /// brace glyph is an operator that takes one operand, for example
    /// `⏟(x+y)_(>0)` is an underbrace with a lower label.  The ordinary
    /// suffix spelling (`x┬⏟`) remains supported for output from this writer.
    fn parse_wide_accent<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (position, accent) = match input.chars().next() {
            Some('\u{23DE}') => (AccentPosition::Above, '\u{23DE}'),
            Some('\u{23DF}') => (AccentPosition::Below, '\u{23DF}'),
            _ => return Err(parse_error(input, nom::error::ErrorKind::Char)),
        };
        let rest = &input[accent.len_utf8()..];
        self.enter(rest)?;
        let parsed = if rest.starts_with('(') {
            // Parentheses around an accent operand are UnicodeMath grouping,
            // not visible fences. This is the standard spelling in UTN 28.
            self.parse_absorbing_paren_group(rest)
        } else {
            // Parse one atom without its attached scripts. Those scripts are
            // labels of the brace and must be consumed after the Accent node.
            self.parse_atom(rest)
        };
        self.depth -= 1;
        let (rest, base) = parsed?;
        let accented = self.push_node(CompactNode::Accent {
            base,
            accent,
            position,
            wide: true,
        });
        self.parse_attached_scripts(rest, accented)
    }

    /// UTN #28 sections 2.1 and 4.1: `/` before an operator negates it
    /// (`a/=b` is `a≠b`, `a/∈B` is `a∉B`).
    fn parse_legacy_cases<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let rest = input.strip_prefix('{').ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        // A matching brace means this is an ordinary grouped expression. In
        // particular, `{■(a&b)xy}z` must keep `xy` inside the group. The
        // legacy one-sided cases spelling is only available when the opening
        // brace is genuinely unmatched.
        if self.matching_brace_end(input).is_some() {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }
        let (after, array) = self.parse_matrix(rest)?;
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children_deferred(&[array]);
        let span = self.offset(after);
        self.legacy_case_warnings.push((self.offset(input), span));
        Ok((
            after,
            self.push_node(CompactNode::Fenced {
                open,
                close,
                content,
            }),
        ))
    }

    fn parse_negated_operator<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, negated) = negated_operator(input)
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Tag))?;
        let mut buf = [0_u8; 4];
        let sid = self.arena.push_str(negated.encode_utf8(&mut buf));
        let kind = classify_unicode_symbol(negated);
        let rest = if matches!(kind, SymbolKind::BinaryOp | SymbolKind::Relation) {
            skip_blanks(rest)
        } else {
            rest
        };
        Ok((rest, self.push_node(CompactNode::Symbol(kind, sid))))
    }

    /// A period that is not a decimal point: punctuation (UTN #28 section
    /// 3.14).
    fn parse_point<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let rest = input
            .strip_prefix('.')
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Char))?;
        let sid = self.arena.push_str(".");
        let point = CompactNode::Symbol(SymbolKind::Punctuation, sid);
        Ok((rest, self.push_node(point)))
    }

    /// UTN #28 sections 2.1 and 3.2: `\/` is a literal solidus (an
    /// ordinary symbol, as LaTeX `/`), not the fraction operator.
    fn parse_literal_solidus<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let rest = input
            .strip_prefix("\\/")
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Tag))?;
        let sid = self.arena.push_str("/");
        Ok((
            rest,
            self.push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
        ))
    }

    /// Parse a UnicodeMath box (`▭` U+25AD) or phantom (`⟡` U+27E1) operator
    /// Parse UnicodeMath box and phantom shortcut operators. A masked box uses
    /// ▭(mask&content), where mask 6 hides the bottom and left sides, leaving
    /// the actuarial top/right enclosure. Splitting occurs only at the mask
    /// separator, so ampersands inside the nested operand remain untouched.
    fn parse_box_phantom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut chars = input.chars();
        let marker = match chars.next() {
            Some(marker) => marker,
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            },
        };
        let (is_box, phantom_kind) = match marker {
            '\u{25AD}' => (true, None),                            // ▭ box
            '\u{27E1}' => (false, Some(PhantomKind::Full)),        // ⟡ phantom
            '\u{21F3}' => (false, Some(PhantomKind::Vertical)),    // ⇳ vphantom
            '\u{2B0C}' => (false, Some(PhantomKind::Horizontal)),  // ⬌ hphantom
            '\u{2B0D}' => (false, Some(PhantomKind::SmashBoth)),   // ⬍ smash
            '\u{2B07}' => (false, Some(PhantomKind::SmashBottom)), // ⬇ dsmash
            '\u{2B06}' => (false, Some(PhantomKind::SmashTop)),    // ⬆ asmash
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
            CompactNode::Phantom(content, phantom_kind.unwrap_or(PhantomKind::Full))
        };
        Ok((rest, self.push_node(node)))
    }

    /// A bracket group as an atom, which may be the base of a script.
    fn parse_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        self.parse_group_as(input, true)
    }

    /// A bracket group as an operand (`is_base` false: a script argument,
    /// which never takes the script after it).
    fn parse_group_as<'a>(&mut self, input: &'a str, is_base: bool) -> IResult<&'a str, NodeId> {
        if input.starts_with('〖') {
            let (rest, children) = self.parse_delimited(input, '〖', '〗')?;
            let is_base = is_base && starts_script(rest);
            // An empty invisible group before a script is the script's empty
            // base (`x〖〗_1`, LaTeX `x{}_1`): x and an empty atom carrying
            // the subscript, not a subscript on x.
            if children.is_empty() && is_base {
                return Ok((rest, self.push_node(CompactNode::Row(NodeSpan::EMPTY))));
            }
            let base = self.flatten_single(&children);
            // One atom that only needed the group to take an accent whole
            // (`〖x_1〗̂`) is the accent's base itself.
            if unicode_accent_suffix(rest).is_some()
                && matches!(
                    *self.arena.node(base),
                    CompactNode::Subscript { .. }
                        | CompactNode::Superscript { .. }
                        | CompactNode::SubSup { .. }
                        | CompactNode::Underscript { .. }
                        | CompactNode::Overscript { .. }
                        | CompactNode::UnderOver { .. }
                        | CompactNode::Fraction { .. }
                        | CompactNode::Nary { .. }
                        | CompactNode::Sqrt(_)
                        | CompactNode::Root { .. }
                        | CompactNode::Boxed { .. }
                        | CompactNode::Phantom(..)
                )
            {
                return Ok((rest, base));
            }
            // A subscript on an explicitly grouped named function belongs at
            // the side, even when its ungrouped name would take display
            // limits. Preserve this long-standing ordinary UnicodeMath rule;
            // decorated structured bodies are retained below as Inner atoms.
            if is_base && let CompactNode::Operator { name, .. } = *self.arena.node(base) {
                return Ok((
                    rest,
                    self.push_node(CompactNode::Operator {
                        name,
                        limits: LimitStyle::SubSup,
                    }),
                ));
            }
            // A fraction or a plain row is the base of the script itself
            // (`〖a+b〗^2` is LaTeX `{a+b}^2`): the writer groups such a base
            // because it has no other spelling.
            if is_base && self.is_plain_compound(base) {
                return Ok((rest, base));
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

    /// A fraction, or a row of atoms none of which is decorated (a
    /// decorated structured body keeps its group as an inner atom, and so
    /// does an empty group).
    fn is_plain_compound(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Fraction { .. } => true,
            // An n-ary operator with its n-aryand (`〖∏ cos〗_i`).
            CompactNode::Nary { body, .. } => {
                !matches!(*self.arena.node(body), CompactNode::Row(span) if span.is_empty())
            },
            CompactNode::Row(span) if span.is_empty() => false,
            CompactNode::Row(span) => !self.arena.children(span).iter().any(|&child| {
                matches!(
                    *self.arena.node(child),
                    CompactNode::Accent { .. }
                        | CompactNode::Overscript { .. }
                        | CompactNode::Underscript { .. }
                        | CompactNode::UnderOver { .. }
                        | CompactNode::StructuredOperator { .. }
                )
            }),
            _ => false,
        }
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
            && let Some(end) = self.matching_paren_end(input)
            && root_separator(&input[1..end - 1]).is_some()
        {
            // Parsed as suffixes of the input, as everything is (failures are
            // recorded by offset).
            self.enter(input)?;
            let parsed = self
                .parse_until_close(&input[1..], '&')
                .and_then(|(rest, index)| {
                    let (rest, radicand) = self.parse_until_close(rest, ')')?;
                    Ok((rest, index, radicand))
                });
            self.depth -= 1;
            let (rest, index, radicand) = parsed?;
            if index.is_empty() || radicand.is_empty() {
                return Err(parse_error(input, nom::error::ErrorKind::Verify));
            }
            let index = self.flatten_single(&index);
            let radicand = self.flatten_single(&radicand);
            let id = self.push_node(CompactNode::Root { index, radicand });
            return Ok((rest, id));
        }
        let (input, content) = self.parse_radicand(input)?;
        let id = self.push_node(CompactNode::Sqrt(content));
        Ok((input, id))
    }

    fn parse_cbrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, root) = satisfy(|c| matches!(c, '\u{221B}' | '\u{221C}'))(input)?; // ∛ ∜
        let (input, content) = self.parse_radicand(input)?;
        let s = self
            .arena
            .push_str(if root == '\u{221B}' { "3" } else { "4" });
        let index = self.push_node(CompactNode::Number(s));
        let id = self.push_node(CompactNode::Root {
            index,
            radicand: content,
        });
        Ok((input, id))
    }

    fn parse_matrix<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, marker) = satisfy(|c| matches!(c, '\u{25A0}' | '\u{2588}'))(input)?;
        let (input, _) = satisfy(|c| c == '(')(input)?;
        let (input, content) = self.parse_matrix_content(input, marker == '\u{2588}')?;
        let (input, _) = satisfy(|c| c == ')')(input)?;
        self.arena
            .mark_array_environment(content, MatrixKind::Matrix);
        if marker == '\u{2588}' {
            self.arena.mark_equation_array(content);
            // UTN 28 uses `&=` as the alignment boundary in an equation
            // array. The parser has already split the row at `&`, so retain
            // the corresponding right/left column alignment in the AST.
            let col_aligns = match *self.arena.node(content) {
                CompactNode::Array { col_aligns, .. } => col_aligns,
                _ => return Ok((input, content)),
            };
            let columns = self.arena.children(col_aligns).to_vec();
            for (index, column) in columns.into_iter().enumerate() {
                if let Some(CompactNode::ColAlign(alignment)) =
                    self.arena.try_node_mut_in_place(column)
                {
                    *alignment = if index % 2 == 0 {
                        ColumnAlign::Right
                    } else {
                        ColumnAlign::Left
                    };
                }
            }
        }
        Ok((input, content))
    }

    /// UTN #28's readable cases operator (`Ⓒ`) is a matrix with a one-sided
    /// left brace.  Keep the brace as a normal fenced AST node so LaTeX and
    /// MathML writers see the same structure as a LaTeX `cases` environment.
    /// The matrix body deliberately omits the `■` marker: `Ⓒ(a&b@c&d)` is the
    /// standard UnicodeMath spelling, unlike an ordinary matrix
    /// (`■(a&b@c&d)`).
    fn parse_cases<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{24B8}')(input)?; // Ⓒ
        let (input, _) = satisfy(|c| c == '(')(input)?;
        let (input, content) = self.parse_matrix_content(input, false)?;
        let (input, _) = satisfy(|c| c == ')')(input)?;
        self.arena
            .mark_array_environment(content, MatrixKind::Matrix);
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let fenced_content = self.arena.push_children_deferred(&[content]);
        let fenced = self.push_node(CompactNode::Fenced {
            open,
            close,
            content: fenced_content,
        });
        Ok((input, fenced))
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
    fn skip_diagnostics(&mut self) -> (Vec<Diagnostic>, Vec<DiagnosticMetadata>) {
        // Report in input order.
        self.matrix_skips.sort_unstable_by_key(|skip| skip.start);
        let lead = self.lead;
        let warning = |code, message: String, start: usize, end: usize| {
            debug_assert!(start <= end, "skip span {start}..{end}");
            (
                Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message,
                    span: Some(crate::error::Span::new(lead + start, lead + end)),
                },
                code,
            )
        };
        let mut diagnostics: Vec<_> = self
            .matrix_skips
            .iter()
            .take(MAX_SKIP_WARNINGS)
            .map(|skip| {
                warning(
                    DiagnosticCode::ReaderRecovery,
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
                DiagnosticCode::ReaderRecovery,
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
        if let Some(fallback) = self.literal_fallback {
            diagnostics.push(warning(DiagnosticCode::ReaderRecovery,
                format!(
                    "UnicodeMath parse fallback (recovered as text): unparsed input was kept as literal text at bytes {}..{}",
                    lead + fallback.start,
                    lead + fallback.end
                ),
                fallback.start,
                fallback.end,
            ));
        }
        for &(offset, value) in &self.unsupported_nary_controls {
            diagnostics.push(warning(DiagnosticCode::ReaderInterpretation,
                format!(
                    "UnicodeMath n-ary control digit {value} has unsupported placement flags; the representable limit placement was retained"
                ),
                offset,
                offset + 1,
            ));
        }
        for &(start, end) in &self.equation_number_losses {
            diagnostics.push(warning(DiagnosticCode::ReaderInterpretation,
                "UnicodeMath equation-number metadata was accepted but is not represented in the AST"
                    .into(),
                start,
                end,
            ));
        }
        for &(start, end) in &self.legacy_case_warnings {
            diagnostics.push(warning(
                DiagnosticCode::ReaderInterpretation,
                "legacy one-sided brace/matrix notation was canonicalized as UnicodeMath cases"
                    .into(),
                start,
                end,
            ));
        }
        diagnostics.sort_unstable_by_key(|(diagnostic, _)| {
            diagnostic.span.map_or(usize::MAX, |span| span.start)
        });
        let mut metadata = Vec::new();
        let diagnostics = diagnostics
            .into_iter()
            .enumerate()
            .map(|(index, (diagnostic, code))| {
                if code == DiagnosticCode::ReaderInterpretation {
                    metadata.push(DiagnosticMetadata::new(index, &diagnostic, code));
                }
                diagnostic
            })
            .collect();
        (diagnostics, metadata)
    }

    /// Start a new matrix cell, counting it towards the node limit. Returns
    /// whether the parse is aborted.
    fn add_cell(&mut self, cells: &mut usize) -> bool {
        *cells += 1;
        self.pending_cells += 1;
        self.check_node_count(0)
    }

    fn parse_matrix_content<'a>(
        &mut self,
        input: &'a str,
        allow_equation_number: bool,
    ) -> IResult<&'a str, NodeId> {
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
        // Start of the current operand in the cell, and whether white space
        // came before the next element.
        let mut operand = OperandRun::default();
        let mut spaced = false;
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
            if allow_equation_number
                && !current_cell.is_empty()
                && let Some((rest, start, end)) = self.take_equation_number(remaining)
            {
                if self.equation_number_losses.len() < MAX_EQUATION_NUMBER_WARNINGS {
                    self.equation_number_losses.push((start, end));
                }
                remaining = rest;
                continue;
            }
            if remaining.starts_with(')') {
                break;
            }
            let mut skipped = false;
            if let Some((rest, width)) = unicode_math_space(remaining) {
                // A typographic space is content (UTN #28 section 3.16).
                let id = self.push_node(CompactNode::Space(width));
                remaining =
                    self.push_operand(&mut current_cell, &mut operand, true, offset, id, rest)?;
                spaced = true;
            } else if let Some(rest) = remaining.strip_prefix(|c: char| c.is_whitespace()) {
                // Whitespace separates operands; it is not content.
                remaining = rest;
                spaced = true;
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
                self.baseless_ok = current_cell.is_empty() || spaced;
                match self.parse_element(remaining) {
                    Ok((rest, id)) => {
                        if current_cell.is_empty() {
                            operand = OperandRun::default();
                        }
                        let spaced = std::mem::take(&mut spaced);
                        remaining = self.push_operand(
                            &mut current_cell,
                            &mut operand,
                            spaced,
                            offset,
                            id,
                            rest,
                        )?;
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
        let body = input
            .strip_prefix('"')
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Char))?;
        let mut text = String::new();
        let end = scan_quoted_text(body, |ch| text.push(ch))
            .ok_or_else(|| parse_error(body, nom::error::ErrorKind::Char))?;
        self.tick(end);
        let rest = &body[end + 1..];
        let sid = self.arena.push_str(&text);
        let id = self.push_node(CompactNode::Text(TextStyle::Normal, sid));
        Ok((rest, id))
    }

    /// UTN #28 section 3.4: "The operand that follows this operator becomes
    /// the n-aryand", after white space or `▒`: the factors up to a space or
    /// an operator, by the operand rule of fractions (`∑_i a_i`,
    /// `∫_0^1 f(x)/x ⅆx`). "Unlike for the fraction numerator and
    /// denominator, the outermost parentheses of an n-aryand are not
    /// removed" (`∏_k (1+x_k)`, `∑ (n¦k)`); an invisible group around it
    /// only groups it. A nested n-ary operator is the whole n-aryand
    /// (`∫∫ f`).
    fn parse_naryand<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut rest = skip_blanks(input);
        if let Some(after) = rest.strip_prefix('\u{2592}') {
            rest = skip_blanks(after);
        }
        let mut ids = SmallVec::<[NodeId; 8]>::new();
        let mut run = OperandRun::default();
        let mut spaced = false;
        // The input before the space skipped to a function's argument.
        let mut skipped = None;
        // Where the bar pair or bracket group being read ends.
        let mut until = None;
        loop {
            let at = self.offset(rest);
            if let Some(end) = until
                && at <= end
            {
                let next = skip_blanks(rest);
                spaced |= next.len() != rest.len();
                rest = next;
                if let Some((after, width)) = unicode_math_space(rest) {
                    let id = self.push_node(CompactNode::Space(width));
                    rest = self.push_operand(&mut ids, &mut run, true, at, id, after)?;
                    spaced = true;
                    continue;
                }
            } else {
                until = None;
            }
            // UTN #28 section 3.5: a function name and its argument are one
            // operand (`∑ log k`); the space after the name only delimits it
            // (section 2.3). The differential ⅆ (section 3.4: "automatically
            // introduces a small space") ends the integrand it follows, so a
            // space before it does not end the n-aryand (`∫ sin x ⅆx`).
            if until.is_none() && rest.starts_with(is_blank) && !ids.is_empty() {
                let next = skip_blanks(rest);
                let goes_on = if next.starts_with(['\u{2145}', '\u{2146}']) {
                    true
                } else {
                    ids.last().is_some_and(|&id| self.is_function_head(id))
                        && next
                            .chars()
                            .next()
                            .is_some_and(|c| !c.is_whitespace() && !self.ends_naryand(&ids, next))
                };
                if goes_on {
                    skipped = Some(rest);
                    rest = next;
                    spaced = true;
                }
            }
            // Decide from the next character whether the operand goes on, so
            // no element is parsed and then dropped (which could repeat work).
            let Some(c) = rest.chars().next() else {
                break;
            };
            if until.is_none() {
                if c.is_whitespace() || self.ends_naryand(&ids, rest) {
                    break;
                }
                until = self.pair_end(rest);
            }
            // An operand of the n-ary element, at its nesting level (as a
            // script argument is); a nested n-ary operator is one deeper.
            let nested_nary = is_nary_char(c);
            if nested_nary {
                self.enter(rest)?;
            }
            let at = self.offset(rest);
            let operand = self.parse_operand(rest, false);
            if nested_nary {
                self.depth -= 1;
            }
            let (after, id) = match operand {
                Ok(found) => found,
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                // No argument after the space: it ends the n-aryand, and a
                // script after it has no base (`∑ sin _i`).
                Err(_) => {
                    rest = skipped.unwrap_or(rest);
                    break;
                },
            };
            skipped = None;
            let spaced = std::mem::take(&mut spaced);
            rest = self.push_operand(&mut ids, &mut run, spaced, at, id, after)?;
            // A nested n-ary operator is the whole n-aryand, and so is a lone
            // operator symbol, as LaTeX reads `\sum \cdot y`.
            if until.is_none() && ids.len() == 1 && self.breaks_operand(id) {
                break;
            }
        }
        let body = match ids.as_slice() {
            [] => self.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            // An invisible group around a compound n-aryand only groups it.
            [only] => match *self.arena.node(*only) {
                CompactNode::MathAtom {
                    atom_type: MathAtomType::Inner,
                    content,
                } if matches!(
                    *self.arena.node(content),
                    CompactNode::Row(span) if !span.is_empty()
                ) || matches!(*self.arena.node(content), CompactNode::Text(..))
                    || self.is_function_head(content) =>
                {
                    content
                },
                _ => *only,
            },
            _ => self.flatten_single(&ids),
        };
        Ok((rest, body))
    }

    /// Whether `id` is a function name, possibly with scripts or limits
    /// (`sin`, `sin^2`, `lim_(n→∞)`), which takes the operand after it as
    /// its argument (UTN #28 section 3.5).
    fn is_function_head(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Operator { .. } => true,
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. }
            | CompactNode::Underscript { base, .. }
            | CompactNode::Overscript { base, .. }
            | CompactNode::UnderOver { base, .. } => self.is_function_head(base),
            _ => false,
        }
    }

    /// Whether an n-aryand `ids` ends before the element at `rest`: at quoted
    /// text, and once it has begun, at an operator, punctuation, a divider or
    /// a big operator; at the closing delimiter of a pair around it (`|∑ a|`),
    /// but not at an opening bar (`∑ p|x|/2`).
    fn ends_naryand(&self, ids: &[NodeId], rest: &str) -> bool {
        let Some(c) = rest.chars().next() else {
            return true;
        };
        match c {
            '"' => true,
            '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}' => {
                let at = self.offset(rest);
                match self.pairs.bars.binary_search_by_key(&at, |&(bar, _)| bar) {
                    Ok(index) => self.pairs.bars[index].1 < at,
                    Err(_) => c == '\u{27E9}' || !ids.is_empty(),
                }
            },
            _ if opening_fence(c).is_some() => true,
            _ => breaks_naryand(c) && !ids.is_empty(),
        }
    }

    fn parse_nary_op<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, op) = satisfy(is_nary_char)(input)?;

        let default_limits = if is_integral(op) {
            LimitStyle::SubSup
        } else {
            LimitStyle::AboveBelow
        };
        let (input, control_limits) = self.parse_nary_control(input, default_limits);

        // Primes are a superscript of the operator (`∑′_i`, LaTeX
        // `\sum'_i`), merged with a superscript that follows.
        let (input, mut primes) = if input.starts_with(is_prime) {
            self.parse_primes(input)
        } else {
            (input, SmallVec::new())
        };

        // Scripts nest right to left, as on any base (`∑_a_b`, `∑^-1^′`).
        let mut rest = input;
        let mut sub = NodeId::NONE;
        if let Some((after, id)) = self.parse_script(rest, '_')? {
            sub = id;
            rest = after;
        }
        // Primes may follow the subscript too (`∑_i′`, as `x_1′`).
        if sub.is_some() && primes.is_empty() && rest.starts_with(is_prime) {
            (rest, primes) = self.parse_primes(rest);
        }
        if let Some((after, id)) = self.parse_script(rest, '^')? {
            primes.push(id);
            rest = after;
            if sub.is_none()
                && let Some((after, id)) = self.parse_script(rest, '_')?
            {
                sub = id;
                rest = after;
            }
        }
        let input = rest;
        let sup = if primes.is_empty() {
            NodeId::NONE
        } else {
            self.flatten_single(&primes)
        };

        let (input, body) = self.parse_naryand(input)?;

        let limits = control_limits.unwrap_or(default_limits);

        let id = self.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        });
        Ok((input, id))
    }

    /// Read the optional UTN 28 n-ary placement control. A digit is treated
    /// as a control only when it is followed by a script marker, the naryand
    /// concatenation operator, whitespace, or the end of the expression. A
    /// digit immediately followed by a letter remains a literal n-aryand
    /// operand such as `∑1x`.
    fn parse_nary_control<'a>(
        &mut self,
        input: &'a str,
        default: LimitStyle,
    ) -> (&'a str, Option<LimitStyle>) {
        let Some(digit) = input.as_bytes().first().copied() else {
            return (input, None);
        };
        if !digit.is_ascii_digit() {
            return (input, None);
        }
        let value = digit - b'0';
        let after = &input[1..];
        let continuation = after.is_empty()
            || after.starts_with(['_', '^', '\u{2592}'])
            || after.chars().next().is_some_and(char::is_whitespace);
        if !continuation {
            return (input, None);
        }
        let limits = match value {
            0 => default,
            1 => LimitStyle::AboveBelow,
            2 => LimitStyle::SubSup,
            _ => {
                if self.unsupported_nary_controls.len() < MAX_NARY_CONTROL_WARNINGS {
                    self.unsupported_nary_controls
                        .push((self.offset(input), value));
                }
                if value & 0b11 == 1 {
                    LimitStyle::AboveBelow
                } else if value & 0b11 == 2 {
                    LimitStyle::SubSup
                } else {
                    default
                }
            },
        };
        (&input[1..], Some(limits))
    }

    fn parse_number<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Neither a digit nor a decimal point starts other ASCII input.
        let first = *input
            .as_bytes()
            .first()
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Digit))?;
        if first.is_ascii() {
            if !matches!(first, b'0'..=b'9' | b'.') {
                return Err(parse_error(input, nom::error::ErrorKind::Digit));
            }
        } else if let Some(c) = input.chars().next().filter(|&c| is_vulgar_fraction(c)) {
            let (number, rest) = input.split_at(c.len_utf8());
            let sid = self.arena.push_str(number);
            return Ok((rest, self.push_node(CompactNode::Number(sid))));
        }
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
        let (rest, id) = self.parse_symbol_char(input)?;
        // Skip blanks after a binary operator or a relation.
        let skips = matches!(
            *self.arena.node(id),
            CompactNode::Symbol(SymbolKind::BinaryOp | SymbolKind::Relation, _)
        );
        Ok((if skips { skip_blanks(rest) } else { rest }, id))
    }

    /// A symbol and nothing after it: a space after one in a script ends the
    /// script (`x^- 1/2`).
    fn parse_symbol_char<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) = satisfy(is_math_symbol)(input)?;
        let mut buf = [0_u8; 4];
        let sid = self.arena.push_str(ch.encode_utf8(&mut buf));
        let symbol = CompactNode::Symbol(classify_unicode_symbol(ch), sid);
        Ok((rest, self.push_node(symbol)))
    }

    fn parse_identifier<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) = satisfy(|c| {
            c.is_alphabetic()
                && !is_math_symbol(c)
                && !is_nary_char(c)
                && script_character(c).is_none()
        })(input)?;
        let sid = self.arena.push_str(&ch.to_string());
        let id = self.push_node(CompactNode::Identifier(sid));
        Ok((rest, id))
    }

    /// A prime with no base before it: an ordinary symbol, as LaTeX `'`
    /// reads.
    fn parse_prime<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) = satisfy(is_prime)(input)?;
        let ch = if ch == '\'' { '\u{2032}' } else { ch };
        let mut buf = [0_u8; 4];
        let sid = self.arena.push_str(ch.encode_utf8(&mut buf));
        Ok((
            rest,
            self.push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
        ))
    }

    /// The run of primes at the start of `input` (which starts with one),
    /// one ordinary symbol each.
    fn parse_primes<'a>(&mut self, mut input: &'a str) -> (&'a str, SmallVec<[NodeId; 8]>) {
        let mut primes = SmallVec::new();
        while let Ok((rest, id)) = self.parse_prime(input) {
            self.tick(1);
            primes.push(id);
            input = rest;
        }
        (input, primes)
    }

    /// UTN #28 section 3.16: the zero-width space is an operand character
    /// that fills a null argument (`x​_1` is `x〖〗_1`, LaTeX `x{}_1`): an
    /// empty atom.
    fn parse_zero_width_space<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let rest = input
            .strip_prefix('\u{200B}')
            .ok_or_else(|| parse_error(input, nom::error::ErrorKind::Char))?;
        Ok((rest, self.push_node(CompactNode::Row(NodeSpan::EMPTY))))
    }

    /// A zero-width space outside a null argument: an empty ordinary atom
    /// (LaTeX `\mathord{}`), which keeps the letters around it apart.
    fn parse_zero_width_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, null) = self.parse_zero_width_space(input)?;
        if starts_script(rest.strip_prefix('\u{2061}').unwrap_or(rest))
            || rest.starts_with(['/', '\u{A6}', '┬', '┴'])
            || unicode_accent_suffix(rest).is_some()
        {
            return Ok((rest, null));
        }
        let atom = CompactNode::MathAtom {
            atom_type: MathAtomType::Ord,
            content: null,
        };
        Ok((rest, self.push_node(atom)))
    }

    fn parse_single_char_node<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_number,
            parse_symbol_char,
            parse_identifier,
            parse_prime,
            parse_zero_width_space,
            parse_point,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }
}

// ── Pure helper functions (no arena needed) ───────────────────────────

pub(crate) fn is_nary_char(c: char) -> bool {
    // The LaTeX commands' big operators, and only those blocks hold any.
    c == crate::unicode::symbols::UNNAMED_NARY
        || matches!(c, '\u{2200}'..='\u{22FF}' | '\u{2A00}'..='\u{2AFF}')
            && crate::unicode::symbols::nary_op_to_latex(c).is_some()
}

#[cfg(test)]
fn is_common_nary_char(c: char) -> bool {
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

/// UTN #28 section 4.1: the operator `/` negates when it comes before one
/// (`/=` is `≠`), and the input after both.
fn negated_operator(input: &str) -> Option<(&str, char)> {
    let rest = input.strip_prefix('/')?;
    let op = rest.chars().next()?;
    let negated = crate::unicode::symbols::precomposed_negation(op)?;
    Some((&rest[op.len_utf8()..], negated))
}

/// The primes UTN #28 superscripts automatically after a base (′ ″ ‴ ⁗), and
/// the ASCII apostrophe that stands for ′ (section 3.10).
pub(crate) fn is_prime(c: char) -> bool {
    matches!(c, '\'' | '\u{2032}'..='\u{2034}' | '\u{2057}')
}

/// The opening fence a closing fence `c` pairs with (`|` and `‖` pair
/// with themselves).
pub(crate) fn opening_fence(c: char) -> Option<char> {
    Some(match c {
        '|' | '\u{2016}' => c,
        ')' => '(',
        ']' => '[',
        '}' => '{',
        '\u{27E9}' => '\u{27E8}',
        '\u{2309}' => '\u{2308}',
        '\u{230B}' => '\u{230A}',
        _ => return None,
    })
}

/// Whether a symbol atom spelled `glyph` takes the primes after it as its
/// superscript: not a prime, an operator, an opening delimiter or
/// punctuation (`a+′` is `a+` and a prime). The writer spells a prime
/// superscript of such a base `^′`.
pub(crate) fn symbol_takes_prime(glyph: &str) -> bool {
    let mut chars = glyph.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return true;
    };
    !is_prime(c)
        && !breaks_operand_at(c)
        // Their operand would be the prime (`√′`).
        && !matches!(c, '√' | '∛' | '∜' | '▭' | '⟡')
        && !matches!(c, '(' | '[' | '{' | '\u{27E8}' | '\u{2308}' | '\u{230A}')
}

/// Whether an element starting with `c` (not a fence) ends an n-aryand that
/// has begun: an operator, punctuation or a big operator.
pub(crate) fn breaks_naryand(c: char) -> bool {
    is_nary_char(c) || (is_math_symbol(c) && breaks_operand_at(c))
}

/// Whether the symbol `c` ends an operand: the rule for numerators, n-aryands
/// and primes, in the reader and the writer.
pub(crate) fn breaks_operand_at(c: char) -> bool {
    matches!(c, '?' | '.') || class_breaks_operand(classify_unicode_symbol(c))
}

fn class_breaks_operand(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::BinaryOp
            | SymbolKind::Relation
            | SymbolKind::Punctuation
            | SymbolKind::LargeOp
            | SymbolKind::Open
    )
}

/// A letter or decimal digit, which continues a simple script operand
/// (UTN #28 section 2.2).
pub(crate) fn is_run_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || (!c.is_ascii()
            && is_alphanumeric(c)
            && script_character(c).is_none()
            && !is_math_symbol(c)
            && !is_nary_char(c)
            && c.is_alphabetic())
}

/// [`char::is_alphanumeric`], without its table search for the spaces, arrows
/// and operators that make up most other characters of a formula.
pub(crate) fn is_alphanumeric(c: char) -> bool {
    match c {
        '\u{2000}'..='\u{206F}' | '\u{2190}'..='\u{23FF}' | '\u{27C0}'..='\u{2AFF}' => false,
        '〖' | '〗' => false,
        _ => c.is_alphanumeric(),
    }
}

fn is_run_letter(c: char) -> bool {
    is_run_char(c) && !c.is_ascii_digit()
}

/// Whether the maximal span of letters at the start of `text` is the name of
/// a function.
fn starts_function_span(text: &str) -> bool {
    let end = text.find(|c: char| !is_run_letter(c)).unwrap_or(text.len());
    crate::unicode::symbols::unicode_math_operator_prefix(&text[..end])
        .is_some_and(|(word, _)| word.len() == end)
}

/// Whether the reader takes `text` (letters and digits) as one script
/// operand: it does not end at a function name in it, nor is it a function
/// name itself. The writer decides by the same rule.
pub(crate) fn reads_as_script_run(text: &str) -> bool {
    let mut after_letter = false;
    for (index, c) in text.char_indices() {
        if !is_run_char(c) {
            // The decimal point of a number.
            if c == '.' && text[..index].ends_with(|c: char| c.is_ascii_digit()) {
                after_letter = false;
                continue;
            }
            return false;
        }
        // At the start only a name with nothing after it is a function.
        if !after_letter
            && starts_function_span(&text[index..])
            && (index > 0 || text.chars().all(is_run_letter))
        {
            return false;
        }
        after_letter = is_run_letter(c);
    }
    !text.is_empty()
}

/// Whether `c` starts the operand of a sign that begins a script operand
/// (`e^-x`, `x^-(a+b)`, `x^-∞`); otherwise the sign is the whole operand
/// (`x^-` alone, `x^- y`).
pub(crate) fn starts_signed_script_operand(c: char) -> bool {
    is_run_char(c) || is_vulgar_fraction(c) || matches!(c, '(' | '[' | '{' | '〖' | '\u{221E}')
}

/// Whether `rest` continues with a script of the atom before it: a
/// subscript, a superscript or a prime.
fn starts_script(rest: &str) -> bool {
    rest.starts_with(['_', '^'])
        || rest.starts_with(is_prime)
        || rest.starts_with(|c| script_character(c).is_some())
}

/// Whether `input` starts with a script character of `mark`.
fn starts_script_character(input: &str, mark: char) -> bool {
    input.as_bytes().first().is_some_and(|byte| *byte >= 0x80)
        && input
            .chars()
            .next()
            .and_then(script_character)
            .is_some_and(|(_, of)| of == mark)
}

/// The character that the super- or subscript character `c` is a script of,
/// and the script mark (UTN #28 section 3.12).
pub(crate) fn script_character(c: char) -> Option<(char, char)> {
    const SIGNS: [char; 5] = ['+', '\u{2212}', '=', '(', ')'];
    Some(match c {
        '\u{B9}' => ('1', '^'),
        '\u{B2}' => ('2', '^'),
        '\u{B3}' => ('3', '^'),
        '\u{2071}' => ('i', '^'),
        '\u{207F}' => ('n', '^'),
        '\u{2070}' | '\u{2074}'..='\u{208E}' => {
            let (offset, mark) = match c as u32 {
                code @ 0x2080.. => (code - 0x2080, '_'),
                code => (code - 0x2070, '^'),
            };
            let character = match offset {
                0..=9 => char::from_digit(offset, 10)?,
                10..=14 => SIGNS[offset as usize - 10],
                _ => return None,
            };
            (character, mark)
        },
        // The script letters Unicode has besides, as in LaTeX input (`xₖ`).
        '\u{2B0}'..='\u{2E4}'
        | '\u{1D2C}'..='\u{1DBF}'
        | '\u{2090}'..='\u{209C}'
        | '\u{2C7C}'..='\u{2C7D}' => {
            let (sub, character) = crate::unicode::decode_script_scalar(c)
                .filter(|(_, character)| character.is_alphabetic())?;
            (character, if sub { '_' } else { '^' })
        },
        _ => return None,
    })
}

/// A vulgar fraction (`½`), an operand of its own.
fn is_vulgar_fraction(c: char) -> bool {
    matches!(c, '\u{BC}'..='\u{BE}' | '\u{2150}'..='\u{215E}' | '\u{2189}')
}

fn is_integral(c: char) -> bool {
    matches!(c, '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{222E}')
}

fn unicode_accent_suffix(input: &str) -> Option<(&str, char, AccentPosition, bool)> {
    let ch = input.chars().next()?;
    let (position, wide) = match ch {
        _ if crate::unicode::symbols::is_mark_below(ch) => (AccentPosition::Below, false),
        '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}' => (AccentPosition::Above, false),
        '\u{23B4}' | '\u{23DE}' => (AccentPosition::Above, true),
        '\u{23B5}' | '\u{23DF}' => (AccentPosition::Below, true),
        _ => return None,
    };
    Some((&input[ch.len_utf8()..], ch, position, wide))
}

/// The space characters of UTN #28 section 3.16 with their widths in em.
/// Unlike the ASCII space they are not removed on build up.
fn space_width(ch: char) -> Option<Rational> {
    Some(match ch {
        '\u{00A0}' => Rational::new(1, 4),
        '\u{200A}' => Rational::new(1, 18),
        '\u{2009}' => Rational::new(3, 18),
        '\u{205F}' => Rational::new(4, 18),
        '\u{2005}' => Rational::new(5, 18),
        '\u{2004}' => Rational::new(6, 18),
        '\u{2002}' => Rational::new(1, 2),
        '\u{2003}' => Rational::new(1, 1),
        _ => return None,
    })
}

fn unicode_math_space(input: &str) -> Option<(&str, Rational)> {
    if !input.as_bytes().first().is_some_and(|byte| *byte >= 0x80) {
        return None;
    }
    let ch = input.chars().next()?;
    Some((&input[ch.len_utf8()..], space_width(ch)?))
}

/// White space that only separates operands: any but the spaces of
/// [`space_width`].
fn is_blank(ch: char) -> bool {
    // Function apply only binds a function's argument (UTN #28 section 3.5),
    // as a space after its name does.
    ch == '\u{2061}' || ch.is_whitespace() && space_width(ch).is_none()
}

fn skip_blanks(input: &str) -> &str {
    input.trim_start_matches(is_blank)
}

fn is_math_symbol(c: char) -> bool {
    (matches!(c,
        '+' | '-' | '=' | '<' | '>' | ',' | ';' | '!' | '?' | ':' | '|' | '\u{2016}' |
        '\u{00B1}' | '\u{00D7}' | '\u{00F7}' | // ±, ×, ÷
        // UTN #28 table 3.1: `*`, `·` and `•` are operators with the
        // precedence of `+` (section 3.22: "explicit multiplication by
        // asterisk and raised dot has a precedence equal to that of plus").
        // Section 3.10 displays a math-zone `*` as U+2217; the reader keeps
        // the `*` glyph so that LaTeX `*`, which differs from `\ast` (∗),
        // round-trips through UnicodeMath.
        '*' | '\u{00B7}' | '\u{2022}' |
        '\u{2200}'..='\u{22FF}' | // Mathematical Operators
        '\u{2190}'..='\u{21FF}' | // Arrows
        '\u{23B4}'..='\u{23DF}' | // horizontal braces and bracket accents
        '\u{27C0}'..='\u{27EF}' | // Misc Math Symbols-A
        '\u{2980}'..='\u{29FF}' | // Misc Math Symbols-B
        '\u{2A00}'..='\u{2AFF}' | // Supplemental Math Operators
        '\u{2900}'..='\u{297F}' | // Supplemental Arrows-B
        '\u{2B00}'..='\u{2B4F}'   // Miscellaneous Symbols and Arrows
    ) || is_other_symbol(c))
        && !is_nary_char(c)
}

/// A symbol outside the operator blocks that a command spells (`†`, `…`,
/// `¬`): not a letter, nor a character with a meaning of its own here.
fn is_other_symbol(c: char) -> bool {
    !c.is_ascii()
        && !is_alphanumeric(c)
        && !c.is_whitespace()
        && !is_prime(c)
        && !matches!(
            c,
            '\u{A6}' | '\u{200B}' | '\u{2061}'..='\u{2064}' | '\u{0300}'..='\u{036F}'
            | '\u{20D0}'..='\u{20FF}' | '▒' | '■' | '▭' | '┬' | '┴' | '〖' | '〗'
        )
        && crate::unicode::symbols::is_symbol_glyph(c)
}

/// The class of a symbol, by [`crate::unicode::symbols::glyph_class`].
fn classify_unicode_symbol(ch: char) -> SymbolKind {
    crate::unicode::symbols::glyph_class(ch).unwrap_or(SymbolKind::Ordinary)
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
    fn nth_root_separator_ignores_quoted_closing_parentheses() {
        let nodes = parse(r#"√(n&"a)")"#);
        let MathNode::Root { index, radicand } = &nodes[0] else {
            panic!("expected nth root, got {nodes:?}");
        };
        assert!(matches!(&**index, MathNode::Identifier(text) if text == "n"));
        assert!(matches!(&**radicand, MathNode::Text(_, text) if text == "a)"));
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
    fn parentheses_are_matched_however_far() {
        assert_eq!(paren_ends("(x)y"), [(0, 3)]);
        assert_eq!(paren_ends("((x))y"), [(0, 5), (1, 4)]);
        assert_eq!(paren_ends("(x"), []);
        assert_eq!(paren_ends(")(x)"), [(1, 4)]);
        let terms = "x+".repeat(400) + "x";
        let result = crate::from_unicode_math(&format!("x^({terms})")).unwrap();
        let config = Config::default().with_display(DisplayStyle::Inline);
        let expected = crate::from_latex_with(&format!("x^{{{terms}}}"), &config).unwrap();
        assert_eq!(result.to_mathml().unwrap(), expected.to_mathml().unwrap());
        let written = result.to_unicode_math().unwrap();
        let reread = crate::from_unicode_math(&written).unwrap();
        assert_eq!(reread.expr(), result.expr());
    }

    #[test]
    fn quoted_parentheses_do_not_close_a_script_group() {
        for source in [r#"x^("a)")"#, r#"x^("a\")b")"#, r#"x^("a\\)b")"#] {
            let result = crate::from_unicode_math(source).unwrap();
            let written = result.to_unicode_math().unwrap();
            let reread = crate::from_unicode_math(&written).unwrap();
            assert_eq!(reread.expr(), result.expr(), "{source}: {written}");
        }
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
        let cases: [(&str, &str, &str, usize); 14] = [
            ("well-formed", "", "(a_i+b^2)/c ", 500),
            ("script run", "x_", "ij2", 500),
            ("signed scripts", "", "e^-x_-1+", 500),
            ("negated operators", "a", "/=b/\u{2208}", 500),
            ("function n-aryand", "\u{2211} ", "sin ", 500),
            ("differentials", "\u{222B} ", "x \u{2146}", 500),
            ("typographic spaces", "", "x\u{2009}y\u{2003}", 500),
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
            assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
            assert!(
                result.diagnostics[0].message.contains("recovered as text"),
                "{:?}",
                result.diagnostics
            );
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
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert!(
            result.diagnostics[0].message.contains("recovered as text"),
            "{:?}",
            result.diagnostics
        );
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
    fn every_math_character_ends_or_continues_an_operand() {
        use crate::unicode::math_class::math_class;
        use crate::unicode::symbols::is_symbol_glyph;
        let mut failures = Vec::new();
        for c in ('\u{80}'..=char::MAX).filter(|&c| is_symbol_glyph(c)) {
            let Some(class) = math_class(c) else { continue };
            // Structure, n-ary operators, radicals, accents and primes.
            if is_nary_char(c)
                || is_prime(c)
                || unicode_accent_suffix(&c.to_string()).is_some()
                || "\u{A6}\u{2016}\u{221A}\u{221B}\u{221C}▒■▭┬┴〖〗⟡⇳⬆⬇⬌⬍".contains(c)
            {
                continue;
            }
            let ends = matches!(
                class,
                SymbolKind::Relation
                    | SymbolKind::BinaryOp
                    | SymbolKind::Punctuation
                    | SymbolKind::Open
                    | SymbolKind::LargeOp
            );
            let nodes = parse(&format!("a{c}b/c"));
            let found = match &nodes[..] {
                [.., MathNode::Fraction { num, .. }] => {
                    matches!(&**num, MathNode::Identifier(name) if name == "b")
                },
                _ => {
                    failures.push(format!("{c} U+{:04X}: {nodes:?}", c as u32));
                    continue;
                },
            };
            if found != ends && !LATEX_CLASS.contains(c) {
                failures.push(format!("{c} U+{:04X}: {class:?}, ends {found}", c as u32));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Characters whose LaTeX command has another class than Unicode gives.
    const LATEX_CLASS: &str = "–—†‡⋮⋯⋰⋱⎱▲△▼◊◯★⧫";

    #[test]
    fn nary_characters_are_the_table() {
        // Every one the reader listed by hand is in the table it now uses.
        for c in '\0'..=char::MAX {
            assert!(!is_common_nary_char(c) || is_nary_char(c), "{c:?}");
        }
    }

    #[test]
    fn unicode_math_alphanumeric_matches_std() {
        for c in '\0'..=char::MAX {
            assert_eq!(is_alphanumeric(c), c.is_alphanumeric(), "{c:?}");
        }
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = UnicodeMathReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }
}
