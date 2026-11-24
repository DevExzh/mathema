//! TeX's infix generalized fractions: `\over`, `\atop`, `\above`,
//! `\choose`, `\brace`, `\brack` and `\overwithdelims`, `\atopwithdelims`,
//! `\abovewithdelims`.
//!
//! Each divides the math list it appears in, so the parser cannot build the
//! fraction when it reads the command. It records a parser-private
//! [`InfixFraction`] in a side table instead, keyed by the id of a placeholder
//! node: an empty `Row`, so a marker can never be written out as text. Every
//! list context resolves the markers in its list (`resolve_infix_fraction`),
//! and a final pass over the finished tree ([`finish_infix`]) resolves those
//! of any list a context missed. A marker that is not a member of a list at
//! all (`\sqrt\over`, `x^\over`) is misplaced: an error when strict,
//! otherwise a warning, and nothing is drawn for it. Declarations that scope
//! the rest of a list without a group (old-style fonts, `\color`) split
//! around a marker, so the fraction divides the enclosing list as in TeX.
//!
//! [`finish_infix`]: LatexCompactParser::finish_infix
use super::*;

/// An infix fraction command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InfixCommand {
    Over,
    Atop,
    Above,
    Choose,
    Brace,
    Brack,
    OverWithDelims,
    AtopWithDelims,
    AboveWithDelims,
}

impl InfixCommand {
    const ALL: [Self; 9] = [
        Self::Over,
        Self::Atop,
        Self::Above,
        Self::Choose,
        Self::Brace,
        Self::Brack,
        Self::OverWithDelims,
        Self::AtopWithDelims,
        Self::AboveWithDelims,
    ];

    /// The control word, without its backslash.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Over => "over",
            Self::Atop => "atop",
            Self::Above => "above",
            Self::Choose => "choose",
            Self::Brace => "brace",
            Self::Brack => "brack",
            Self::OverWithDelims => "overwithdelims",
            Self::AtopWithDelims => "atopwithdelims",
            Self::AboveWithDelims => "abovewithdelims",
        }
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name() == name)
    }
}

/// One infix fraction command waiting for the math list it divides.
#[derive(Clone, Copy, Debug)]
pub(super) struct InfixFraction {
    /// The placeholder node standing in the list for the command.
    node: NodeId,
    command: InfixCommand,
    /// Where the command is in the source, when known.
    span: Option<Span>,
    /// The address of the input after the command, which identifies it
    /// across speculative re-parses of the same text.
    at: usize,
    /// Whether the fraction draws a rule (`\over`, a nonzero `\above`).
    has_rule: bool,
    /// The fence delimiters; empty strings for none.
    open: StrId,
    close: StrId,
}

type Parsed<'a, T> = std::result::Result<T, nom::Err<nom::error::Error<&'a str>>>;

impl LatexCompactParser {
    /// Any infix fraction command, by name.
    pub(super) fn parse_infix_command<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let Some(command) = InfixCommand::from_name(name) else {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        };
        let at = input.as_ptr() as usize;
        let span = self.source_span_of_command(name);
        let (input, open, close) = match command {
            InfixCommand::OverWithDelims
            | InfixCommand::AtopWithDelims
            | InfixCommand::AboveWithDelims => {
                let (input, open) = self.parse_infix_delimiter(input, command, span)?;
                let (input, close) = self.parse_infix_delimiter(input, command, span)?;
                (input, open, close)
            },
            _ => {
                let (open, close) = match command {
                    InfixCommand::Choose => ("(", ")"),
                    InfixCommand::Brace => ("{", "}"),
                    InfixCommand::Brack => ("[", "]"),
                    _ => ("", ""),
                };
                (input, self.arena.push_str(open), self.arena.push_str(close))
            },
        };
        // The AST has no rule thickness: a zero one reads as `\atop`, any
        // other as `\over`.
        let (input, has_rule) = match command {
            InfixCommand::Over | InfixCommand::OverWithDelims => (input, true),
            InfixCommand::Above | InfixCommand::AboveWithDelims => {
                self.parse_infix_rule(input, command, span)?
            },
            InfixCommand::Atop
            | InfixCommand::Choose
            | InfixCommand::Brace
            | InfixCommand::Brack
            | InfixCommand::AtopWithDelims => (input, false),
        };
        let fraction = InfixFraction {
            node: NodeId::NONE,
            command,
            span,
            at,
            has_rule,
            open,
            close,
        };
        Ok((input, self.push_infix_marker(fraction)))
    }

    /// One delimiter of a `\…withdelims`. As TeX does ("Missing delimiter
    /// (. inserted)"), a missing one reads as the null delimiter `.` when
    /// lenient, consuming nothing; it is an error when strict.
    fn parse_infix_delimiter<'a>(
        &mut self,
        input: &'a str,
        command: InfixCommand,
        span: Option<Span>,
    ) -> IResult<&'a str, StrId> {
        let (input, _) = ws(input)?;
        match self.parse_delimiter_into(input) {
            Ok(parsed) => Ok(parsed),
            Err(nom::Err::Failure(failure)) => Err(nom::Err::Failure(failure)),
            Err(_) => {
                self.infix_error(
                    input,
                    format!("missing delimiter after \\{}", command.name()),
                    span,
                    "used a null delimiter",
                )?;
                Ok((input, self.arena.push_str("")))
            },
        }
    }

    /// The `<dimen>` of `\above`/`\abovewithdelims`: whether the fraction
    /// has a rule. As with `\genfrac`, a zero thickness drops the rule and
    /// any other is drawn at the default thickness, with a warning. As in
    /// KaTeX and MathJax the dimension may be braced (`\above{1pt}`). A
    /// dimension register (`\fboxrule`) is a dimension the parser cannot
    /// evaluate, so it too is drawn at the default thickness. No dimension
    /// at all is an error when strict, as in TeX ("Missing number") and
    /// MathJax ("Missing dimension"); lenient, a warning and the rule. So is
    /// a number without a unit (TeX's "Illegal unit of measure"), which
    /// lenient reads as points.
    fn parse_infix_rule<'a>(
        &mut self,
        input: &'a str,
        command: InfixCommand,
        span: Option<Span>,
    ) -> IResult<&'a str, bool> {
        let name = command.name();
        let (input, _) = ws(input)?;
        let braced = crate::latex::capture_braced(input).and_then(|(rest, body)| {
            match dimension::parse(body.trim()) {
                Ok((tail, dim)) if tail.trim().is_empty() => Some((rest, body.trim(), dim)),
                _ => None,
            }
        });
        let parsed = match braced {
            Some(parsed) => Some(parsed),
            None => match dimension::parse(input) {
                Ok((rest, dim)) => Some((rest, input[..input.len() - rest.len()].trim(), dim)),
                Err(nom::Err::Failure(failure)) => return Err(nom::Err::Failure(failure)),
                Err(_) => None,
            },
        };
        if let Some((rest, text, dim)) = parsed {
            if dim.unit.is_none() {
                self.infix_error(
                    input,
                    format!("illegal unit of measure in `{text}` after \\{name}"),
                    span,
                    "read it as points",
                )?;
            }
            if dim.width.num == 0 {
                return Ok((rest, false));
            }
            self.infix_warn(
                input,
                format!("\\{name} rule thickness `{text}` is drawn at the default thickness"),
                None,
            );
            return Ok((rest, true));
        }
        if let Some(register) = dimension_register(input) {
            self.infix_warn(
                input,
                format!("\\{name} rule thickness `{register}` is drawn at the default thickness"),
                None,
            );
            let (rest, _) = ws(&input[register.len()..])?;
            return Ok((rest, true));
        }
        self.infix_error(
            input,
            format!("missing dimension after \\{name}"),
            span,
            "drew the default rule",
        )?;
        Ok((input, true))
    }

    /// An error when strict; otherwise a warning noting the recovery.
    fn infix_error<'a>(
        &mut self,
        input: &'a str,
        message: String,
        span: Option<Span>,
        recovery: &str,
    ) -> Parsed<'a, ()> {
        if !self.lenient {
            if self.abort.is_none() {
                self.abort = Some(Error::LatexParse { message, span });
            }
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }
        self.infix_warn(input, format!("{message}; {recovery}"), span);
        Ok(())
    }

    /// A warning about the source at `at`, given once however often a
    /// speculative parse reads that source again.
    fn infix_warn(&mut self, at: &str, message: String, span: Option<Span>) {
        self.infix_warn_at(at.as_ptr() as usize, message, span);
    }

    fn infix_warn_at(&mut self, at: usize, message: String, span: Option<Span>) {
        if self.infix_reported.insert((at, message.clone())) {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Warning,
                message,
                span,
            });
        }
    }

    fn push_infix_marker(&mut self, mut fraction: InfixFraction) -> NodeId {
        let before = self.arena.node_count();
        let node = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
        // A full arena stores nothing and returns an existing node, which
        // must not become a marker; the parse then fails on its limit. Ids
        // grow with every push, keeping the table sorted.
        if self.arena.node_count() > before {
            debug_assert!(
                self.infix_markers
                    .last()
                    .is_none_or(|last| last.node.raw() < node.raw())
            );
            fraction.node = node;
            self.infix_markers.push(fraction);
        }
        node
    }

    /// The infix fraction whose placeholder is `id`, if it is one.
    #[inline]
    pub(super) fn infix_marker(&self, id: NodeId) -> Option<InfixFraction> {
        if self.infix_markers.is_empty() {
            return None;
        }
        self.infix_markers
            .binary_search_by_key(&id.raw(), |marker| marker.node.raw())
            .ok()
            .map(|index| self.infix_markers[index])
    }

    /// True if `id` is the placeholder of an infix fraction command.
    #[inline]
    pub(super) fn is_infix_marker_id(&self, id: NodeId) -> bool {
        self.infix_marker(id).is_some()
    }

    /// True if `id` is a row with an infix fraction command among its own
    /// children: a scope that splits around it (see
    /// [`split_scope_at_infix`](Self::split_scope_at_infix)).
    pub(super) fn row_holds_infix_marker(&self, id: NodeId) -> bool {
        if self.infix_markers.is_empty() {
            return false;
        }
        matches!(*self.arena.node(id), CompactNode::Row(span)
            if self.arena.children(span).iter().any(|&child| self.is_infix_marker_id(child)))
    }

    /// The math list `ids` with its infix fraction command applied: the
    /// items before it are the numerator, those after it the denominator.
    /// TeX and KaTeX reject a second command in one list ("Ambiguous; you
    /// need another { and }"): an error when strict; lenient, each later one
    /// divides the denominator of the one before. Built without recursion,
    /// however long the chain.
    fn check_infix_fraction(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        let markers: SmallVec<[(usize, InfixFraction); 2]> = ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| self.infix_marker(id).map(|marker| (i, marker)))
            .collect();
        let (&(_, first), later) = markers.split_first()?;
        if let Some(&(_, second)) = later.first() {
            self.ambiguous_infix(first, second);
        }
        let (&(last_pos, last), _) = markers.split_last()?;
        let mut den = self.flatten_single(&ids[last_pos + 1..]);
        let mut end = last_pos;
        let mut marker = last;
        for index in (0..markers.len()).rev() {
            let start = if index == 0 {
                0
            } else {
                markers[index - 1].0 + 1
            };
            let num = self.flatten_single(&ids[start..end]);
            den = self.make_infix_fraction(marker, num, den);
            if index > 0 {
                (end, marker) = markers[index - 1];
            }
        }
        Some(den)
    }

    /// The same nodes as the equivalent `\genfrac`: `\choose` is
    /// `\genfrac(){0pt}{}`, `\overwithdelims()` is `\genfrac(){}{}`.
    fn make_infix_fraction(&mut self, marker: InfixFraction, num: NodeId, den: NodeId) -> NodeId {
        let kind = if marker.has_rule {
            FractionKind::Normal
        } else {
            FractionKind::NoBar
        };
        let frac_id = self
            .arena
            .push_node(CompactNode::Fraction { kind, num, den });
        if self.arena.str_val(marker.open).is_empty() && self.arena.str_val(marker.close).is_empty()
        {
            return frac_id;
        }
        let content = self.arena.push_children_deferred(&[frac_id]);
        self.arena.push_node(CompactNode::Fenced {
            open: marker.open,
            close: marker.close,
            content,
        })
    }

    #[cold]
    #[inline(never)]
    fn ambiguous_infix(&mut self, first: InfixFraction, second: InfixFraction) {
        let message = format!(
            "ambiguous \\{} after \\{}: you need another {{ and }}",
            second.command.name(),
            first.command.name()
        );
        if !self.lenient {
            if self.abort.is_none() {
                self.abort = Some(Error::LatexParse {
                    message,
                    span: second.span,
                });
            }
            return;
        }
        self.infix_warn_at(
            second.at,
            format!("{message}; it divides the denominator"),
            second.span,
        );
    }

    /// Declarations that scope the rest of a list without a group (old-style
    /// font switches, `\color`) can produce a row of styled spans separated
    /// by an infix marker. Real groups resolve their own fractions before
    /// returning, so only the immediate row needs to be spliced into its
    /// enclosing list.
    pub(super) fn flatten_infix_marker_rows(
        &self,
        ids: SmallVec<[NodeId; 8]>,
    ) -> SmallVec<[NodeId; 8]> {
        if self.infix_markers.is_empty() {
            return ids;
        }
        let mut result = SmallVec::new();
        for id in ids {
            if self.row_holds_infix_marker(id) {
                let CompactNode::Row(span) = *self.arena.node(id) else {
                    unreachable!("row_holds_infix_marker checked for a row");
                };
                result.extend_from_slice(self.arena.children(span));
                continue;
            }
            result.push(id);
        }
        result
    }

    /// Wrap the runs between the infix markers of the scope `ids` with
    /// `wrap`, keeping the markers between them: a row the enclosing list
    /// splices, so that the fraction divides that list, the declaration
    /// remaining in force on both sides. Without a marker, the whole scope
    /// is wrapped.
    pub(super) fn split_scope_at_infix(
        &mut self,
        ids: &[NodeId],
        mut wrap: impl FnMut(&mut Self, &[NodeId]) -> NodeId,
    ) -> NodeId {
        if !ids.iter().any(|&id| self.is_infix_marker_id(id)) {
            return wrap(self, ids);
        }
        let mut parts: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut start = 0;
        for (index, &id) in ids.iter().enumerate() {
            if self.is_infix_marker_id(id) {
                if start < index {
                    parts.push(wrap(self, &ids[start..index]));
                }
                parts.push(id);
                start = index + 1;
            }
        }
        if start < ids.len() {
            parts.push(wrap(self, &ids[start..]));
        }
        let span = self.arena.push_children_deferred(&parts);
        self.arena.push_node(CompactNode::Row(span))
    }

    /// Apply the infix fraction commands of the complete math list `ids`.
    pub(super) fn resolve_infix_fraction(
        &mut self,
        ids: SmallVec<[NodeId; 8]>,
    ) -> SmallVec<[NodeId; 8]> {
        if self.infix_markers.is_empty() {
            return ids;
        }
        let ids = self.flatten_infix_marker_rows(ids);
        match self.check_infix_fraction(&ids) {
            Some(node) => smallvec::smallvec![node],
            None => ids,
        }
    }

    /// Parse a complete math list (the body of a group or argument), with
    /// its infix fraction commands applied.
    pub(super) fn parse_math_list<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let (rest, ids) = self.parse_node_list(input)?;
        Ok((rest, self.resolve_infix_fraction(ids)))
    }

    /// `id` read where a single atom is required (a script, an unbraced
    /// argument). An infix fraction command there divides no list:
    /// misplaced, an error when strict; otherwise a warning, and an empty
    /// group in its place.
    pub(super) fn stray_infix_marker<'a>(
        &mut self,
        input: &'a str,
        id: NodeId,
    ) -> IResult<&'a str, NodeId> {
        match self.infix_marker(id) {
            None => Ok((input, id)),
            Some(marker) => {
                self.misplaced_infix(input, marker)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
                ))
            },
        }
    }

    #[cold]
    #[inline(never)]
    fn misplaced_infix<'a>(&mut self, input: &'a str, marker: InfixFraction) -> Parsed<'a, ()> {
        let message = format!("misplaced \\{}", marker.command.name());
        if !self.lenient {
            return self.infix_error(input, message, marker.span, "");
        }
        self.infix_warn_at(marker.at, format!("{message}; dropped it"), marker.span);
        Ok(())
    }

    /// Resolve every infix fraction command left in the finished top-level
    /// list `ids` and the tree below it. List contexts resolve their own
    /// lists while parsing; this pass covers any list one missed, so a
    /// placeholder never reaches a writer. A placeholder outside any list is
    /// misplaced (see [`stray_infix_marker`](Self::stray_infix_marker)); a
    /// strict error is left in `abort`.
    ///
    /// The tree is walked with an explicit stack, and the nodes holding a
    /// placeholder are then rewritten in place, deepest first, so a deep
    /// tree (a long fraction chain) cannot overflow the call stack.
    pub(super) fn finish_infix(&mut self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        if self.infix_markers.is_empty() {
            return ids;
        }
        // Rows spliced into the list that holds them, as by
        // `flatten_infix_marker_rows`: resolved with that list, not alone.
        let mut spliced: HashSet<NodeId> = HashSet::new();
        let mut holders: Vec<NodeId> = Vec::new();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        for &id in &ids {
            if self.row_holds_infix_marker(id) {
                spliced.insert(id);
            }
            if !self.is_infix_marker_id(id) {
                stack.push(id);
            }
        }
        while let Some(id) = stack.pop() {
            if id.is_none() || !visited.insert(id) {
                continue;
            }
            let node = *self.arena.node(id);
            let is_list = matches!(
                node,
                CompactNode::Row(_) | CompactNode::Fenced { .. } | CompactNode::Styled(..)
            );
            let mut holds = false;
            for child in crate::ast::compact_children(&self.arena, &node) {
                if self.is_infix_marker_id(child) {
                    holds = true;
                    continue;
                }
                if is_list && self.row_holds_infix_marker(child) {
                    spliced.insert(child);
                    holds = true;
                }
                stack.push(child);
            }
            if holds {
                holders.push(id);
            }
        }
        // Preorder: each holder precedes its descendants.
        for &id in holders.iter().rev() {
            if self.abort.is_some() {
                break;
            }
            if !spliced.contains(&id) {
                self.finish_infix_node(id);
            }
        }
        self.resolve_infix_fraction(ids)
    }

    /// Resolve the placeholders among the children of `id`, in place: those
    /// of a list are applied to it, any other is misplaced. The match is
    /// exhaustive so that a new node kind with children cannot be skipped.
    fn finish_infix_node(&mut self, id: NodeId) {
        let original = *self.arena.node(id);
        let mut node = original;
        match &mut node {
            CompactNode::Number(_)
            | CompactNode::Identifier(_)
            | CompactNode::Operator { .. }
            | CompactNode::Symbol(..)
            | CompactNode::Text(..)
            | CompactNode::Space(_)
            | CompactNode::Newline
            | CompactNode::Middle(_)
            | CompactNode::ColAlign(_)
            | CompactNode::Rule { .. }
            | CompactNode::StyleSwitch(_)
            | CompactNode::SizeScale(_) => {},
            CompactNode::Row(span)
            | CompactNode::Fenced { content: span, .. }
            | CompactNode::Styled(_, span) => *span = self.finish_infix_span(*span),
            CompactNode::Array { cells: span, .. }
            | CompactNode::UndefinedCommand {
                arguments: span, ..
            } => *span = self.finish_infix_fields(*span),
            CompactNode::Subscript { base, sub: other }
            | CompactNode::Superscript { base, sup: other }
            | CompactNode::Overscript {
                base, over: other, ..
            }
            | CompactNode::Underscript {
                base, under: other, ..
            }
            | CompactNode::Fraction {
                num: base,
                den: other,
                ..
            }
            | CompactNode::Root {
                index: base,
                radicand: other,
            } => {
                *base = self.finish_infix_field(*base);
                *other = self.finish_infix_field(*other);
            },
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
            }
            | CompactNode::Nary {
                body: base,
                sub,
                sup,
                ..
            } => {
                *base = self.finish_infix_field(*base);
                *sub = self.finish_infix_field(*sub);
                *sup = self.finish_infix_field(*sup);
            },
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => {
                for field in [display, text, script, script_script] {
                    *field = self.finish_infix_field(*field);
                }
            },
            CompactNode::Sqrt(content)
            | CompactNode::Accent { base: content, .. }
            | CompactNode::AlignedCell { content, .. }
            | CompactNode::SpannedCell { content, .. }
            | CompactNode::Phantom(content, _)
            | CompactNode::Reflect { content, .. }
            | CompactNode::Boxed { content, .. }
            | CompactNode::Actuarial { content, .. }
            | CompactNode::Cancel { content, .. }
            | CompactNode::Scaled { content, .. }
            | CompactNode::RaiseBox { content, .. }
            | CompactNode::Color { content, .. }
            | CompactNode::MathAtom { content, .. }
            | CompactNode::StructuredOperator { content, .. } => {
                *content = self.finish_infix_field(*content);
            },
        }
        if node != original {
            *self.arena.node_mut(id) = node;
        }
    }

    /// A list of children: resolved as a math list.
    fn finish_infix_span(&mut self, span: NodeSpan) -> NodeSpan {
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        let resolved = self.resolve_infix_fraction(ids.clone());
        if resolved == ids {
            span
        } else {
            self.arena.push_children_deferred(&resolved)
        }
    }

    /// A list of children each of which is a field of its own (table cells,
    /// command arguments).
    fn finish_infix_fields(&mut self, span: NodeSpan) -> NodeSpan {
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        let resolved: SmallVec<[NodeId; 8]> =
            ids.iter().map(|&id| self.finish_infix_field(id)).collect();
        if resolved == ids {
            span
        } else {
            self.arena.push_children_deferred(&resolved)
        }
    }

    /// A single-node field: a placeholder there is misplaced.
    fn finish_infix_field(&mut self, id: NodeId) -> NodeId {
        match self.infix_marker(id) {
            Some(marker) => {
                let _ = self.misplaced_infix("", marker);
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
            },
            None => id,
        }
    }
}

/// A dimension register such as `\fboxrule` at the start of `input`: a
/// control word the reader does not otherwise define, named like a TeX or
/// LaTeX length (`…rule`, `…skip`, `…sep`, `…width`, …) or one of a few
/// well-known registers. Anything else, in particular a structural command
/// such as `\end` or `\right`, is not a dimension.
fn dimension_register(input: &str) -> Option<&str> {
    const SUFFIXES: [&str; 10] = [
        "rule",
        "skip",
        "sep",
        "width",
        "height",
        "depth",
        "thickness",
        "space",
        "indent",
        "size",
    ];
    const NAMES: [&str; 2] = ["jot", "maxdimen"];
    let name = input.strip_prefix('\\')?;
    let len = name.bytes().take_while(u8::is_ascii_alphabetic).count();
    let word = &name[..len];
    let register =
        len > 0 && (NAMES.contains(&word) || SUFFIXES.iter().any(|suffix| word.ends_with(suffix)));
    let spelling = &input[..len + 1];
    (register && reads_as_undefined_command(spelling)).then_some(spelling)
}

/// Whether `input` starts with an infix fraction command.
pub(super) fn starts_infix_command(input: &str) -> bool {
    input.strip_prefix('\\').is_some_and(|name| {
        let len = name.bytes().take_while(u8::is_ascii_alphabetic).count();
        InfixCommand::from_name(&name[..len]).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_arena_makes_no_marker() {
        let mut parser = LatexCompactParser::with_capacity(0, None);
        parser.arena = AstArena::with_entry_limit(4);
        let existing: Vec<NodeId> = (0..4)
            .map(|_| parser.arena.push_node(CompactNode::Newline))
            .collect();
        let (_, id) = parser.parse_infix_command("", "over").unwrap();
        assert!(existing.contains(&id));
        assert!(!parser.is_infix_marker_id(id));
        assert!(parser.infix_markers.is_empty());
        assert!(parser.arena.check_limits().is_err());
    }
}
