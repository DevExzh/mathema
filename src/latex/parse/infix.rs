//! TeX's infix generalized fractions: `\over`, `\atop`, `\above`,
//! `\choose`, `\brace`, `\brack` and `\overwithdelims`, `\atopwithdelims`,
//! `\abovewithdelims`.
//!
//! Each divides the math list it appears in, so the parser cannot build the
//! fraction when it reads the command. It records a parser-private
//! [`InfixFraction`] in a side table instead, keyed by the id of a placeholder
//! node: an empty `Row`, so a marker can never be written out as text. Every
//! list context resolves the markers in its list (`resolve_infix_fraction`),
//! and a final walk over the finished tree ([`finish_infix`]) resolves those of
//! any list a context missed. A marker that is not a member of a list at all
//! (`\sqrt\over`, `x^\over`) is misplaced: an error when strict, otherwise a
//! warning, and nothing is drawn for it.
//!
//! [`finish_infix`]: LatexCompactParser::finish_infix
use super::*;

/// One infix fraction command waiting for the math list it divides.
#[derive(Clone, Copy, Debug)]
pub(super) struct InfixFraction {
    /// The placeholder node standing in the list for the command.
    node: NodeId,
    /// The command's name, for diagnostics.
    command: &'static str,
    /// Where the command is in the source, when known.
    span: Option<Span>,
    /// Whether the fraction draws a rule (`\over`, a nonzero `\above`).
    has_rule: bool,
    /// The fence delimiters; empty strings for none.
    open: StrId,
    close: StrId,
}

/// The `'static` spelling of an infix fraction command name.
fn command_name(name: &str) -> &'static str {
    match name {
        "over" => "over",
        "atop" => "atop",
        "above" => "above",
        "choose" => "choose",
        "brace" => "brace",
        "brack" => "brack",
        "overwithdelims" => "overwithdelims",
        "atopwithdelims" => "atopwithdelims",
        _ => "abovewithdelims",
    }
}

type Parsed<'a, T> = std::result::Result<T, nom::Err<nom::error::Error<&'a str>>>;

impl LatexCompactParser {
    /// `\over`, `\atop`, `\above<dimen>`, `\choose`, `\brace`, `\brack`.
    pub(super) fn parse_infix_command<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, has_rule, open, close) = match name {
            "over" => (input, true, "", ""),
            "atop" => (input, false, "", ""),
            "choose" => (input, false, "(", ")"),
            "brace" => (input, false, "{", "}"),
            "brack" => (input, false, "[", "]"),
            _ => {
                // The AST has no rule thickness: a zero one reads as
                // `\atop`, any other as `\over`.
                let (input, has_rule) = self.parse_infix_rule(input, name)?;
                (input, has_rule, "", "")
            },
        };
        let open = self.arena.push_str(open);
        let close = self.arena.push_str(close);
        Ok((input, self.push_infix_marker(name, has_rule, open, close)))
    }

    /// `\overwithdelims<delim><delim>`, `\atopwithdelims<delim><delim>` and
    /// `\abovewithdelims<delim><delim><dimen>`: infix `\over`/`\atop`/`\above`
    /// with fences, read as the equivalent `\genfrac`. A `.` delimiter is
    /// empty, as with `\left.`.
    pub(super) fn parse_infix_withdelims<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, open) = self.parse_infix_delimiter(input, name)?;
        let (input, close) = self.parse_infix_delimiter(input, name)?;
        let (input, has_rule) = match name {
            "overwithdelims" => (input, true),
            "atopwithdelims" => (input, false),
            _ => self.parse_infix_rule(input, name)?,
        };
        Ok((input, self.push_infix_marker(name, has_rule, open, close)))
    }

    /// One delimiter of a `\…withdelims`. As TeX does ("Missing delimiter
    /// (. inserted)"), a missing one reads as the null delimiter `.` when
    /// lenient, consuming nothing; it is an error when strict.
    fn parse_infix_delimiter<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, StrId> {
        let (input, _) = ws(input)?;
        match self.parse_delimiter_into(input) {
            Ok(parsed) => Ok(parsed),
            Err(nom::Err::Failure(failure)) => Err(nom::Err::Failure(failure)),
            Err(_) => {
                self.infix_error(
                    input,
                    format!("missing delimiter after \\{name}"),
                    self.source_span_of_command(name),
                    "used a null delimiter",
                )?;
                Ok((input, self.arena.push_str("")))
            },
        }
    }

    /// The `<dimen>` of `\above`/`\abovewithdelims`: whether the fraction
    /// has a rule. As with `\genfrac`, a zero thickness drops the rule and
    /// any other is drawn at the default thickness, with a warning. A
    /// dimension register (`\fboxrule`) is a dimension the parser cannot
    /// evaluate, so it too is drawn at the default thickness. No dimension
    /// at all is an error when strict, as in TeX ("Missing number") and
    /// MathJax ("Missing dimension"); lenient, a warning and the rule.
    fn parse_infix_rule<'a>(&mut self, input: &'a str, name: &str) -> IResult<&'a str, bool> {
        let (input, _) = ws(input)?;
        if let Ok((rest, width)) = dimension::width(input) {
            if width.num == 0 {
                return Ok((rest, false));
            }
            let text = input[..input.len() - rest.len()].trim();
            self.warn(format!(
                "\\{name} rule thickness `{text}` is drawn at the default thickness"
            ));
            return Ok((rest, true));
        }
        if let Some(register) = dimension_register(input) {
            self.warn(format!(
                "\\{name} rule thickness `{register}` is drawn at the default thickness"
            ));
            let (rest, _) = ws(&input[register.len()..])?;
            return Ok((rest, true));
        }
        self.infix_error(
            input,
            format!("missing dimension after \\{name}"),
            self.source_span_of_command(name),
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
        self.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: format!("{message}; {recovery}"),
            span,
        });
        Ok(())
    }

    fn push_infix_marker(
        &mut self,
        name: &str,
        has_rule: bool,
        open: StrId,
        close: StrId,
    ) -> NodeId {
        let node = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
        // Ids grow with every push, keeping the table sorted. A full arena
        // returns an earlier id; the parse then fails on its limit.
        if self
            .infix_markers
            .last()
            .is_none_or(|last| last.node.raw() < node.raw())
        {
            self.infix_markers.push(InfixFraction {
                node,
                command: command_name(name),
                span: self.source_span_of_command(name),
                has_rule,
                open,
                close,
            });
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

    /// The math list `ids` with its first infix fraction command applied: the
    /// items before it are the numerator, those after it the denominator.
    fn check_infix_fraction(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        let (pos, marker) = ids
            .iter()
            .enumerate()
            .find_map(|(i, &id)| self.infix_marker(id).map(|marker| (i, marker)))?;

        // Further commands in either side (`1 \over 2 \over 3`) divide that
        // side.
        let num_ids = self.resolve_infix_fraction(ids[..pos].into());
        let den_ids = self.resolve_infix_fraction(ids[pos + 1..].into());
        let num = self.flatten_single(&num_ids);
        let den = self.flatten_single(&den_ids);

        // The same nodes as the equivalent `\genfrac`: `\choose` is
        // `\genfrac(){0pt}{}`, `\overwithdelims()` is `\genfrac(){}{}`.
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
            return Some(frac_id);
        }
        let content = self.arena.push_children_deferred(&[frac_id]);
        Some(self.arena.push_node(CompactNode::Fenced {
            open: marker.open,
            close: marker.close,
            content,
        }))
    }

    /// A font declaration can produce multiple styled spans separated by an
    /// infix marker. Real groups resolve their own fractions before returning,
    /// so only the immediate row needs to be spliced into its enclosing list.
    fn flatten_infix_marker_rows(&self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        if self.infix_markers.is_empty() {
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
        self.infix_error(
            input,
            format!("misplaced \\{}", marker.command),
            marker.span,
            "dropped it",
        )
    }

    /// Resolve every infix fraction command left in the finished top-level
    /// list `ids` and the tree below it. List contexts resolve their own
    /// lists while parsing; this pass covers any list one missed, so a
    /// placeholder never reaches a writer. A placeholder outside any list is
    /// misplaced (see [`stray_infix_marker`](Self::stray_infix_marker)); a
    /// strict error is left in `abort`.
    pub(super) fn finish_infix(&mut self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        if self.infix_markers.is_empty() {
            return ids;
        }
        self.finish_infix_list(ids)
    }

    fn finish_infix_list(&mut self, ids: SmallVec<[NodeId; 8]>) -> SmallVec<[NodeId; 8]> {
        let ids = self.flatten_infix_marker_rows(ids);
        let walked: SmallVec<[NodeId; 8]> = ids
            .into_iter()
            .map(|id| {
                if self.is_infix_marker_id(id) {
                    id
                } else {
                    self.finish_infix_node(id)
                }
            })
            .collect();
        self.resolve_infix_fraction(walked)
    }

    /// A list of children: resolved as a math list.
    fn finish_infix_span(&mut self, span: NodeSpan) -> NodeSpan {
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        let resolved = self.finish_infix_list(ids.clone());
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
        if id.is_none() {
            return id;
        }
        match self.infix_marker(id) {
            Some(marker) => {
                let _ = self.misplaced_infix("", marker);
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
            },
            None => self.finish_infix_node(id),
        }
    }

    /// Resolve the placeholders below `id`, rewriting its fields in place.
    /// The match is exhaustive so that a new node kind with children cannot
    /// be skipped.
    fn finish_infix_node(&mut self, id: NodeId) -> NodeId {
        if id.is_none() || self.abort.is_some() {
            return id;
        }
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
        id
    }
}

/// A dimension register such as `\fboxrule` at the start of `input`: a
/// control word the reader does not otherwise define. A defined command
/// (`\alpha`) is not a dimension.
fn dimension_register(input: &str) -> Option<&str> {
    let name = input.strip_prefix('\\')?;
    let len = name.bytes().take_while(u8::is_ascii_alphabetic).count();
    let spelling = &input[..len + 1];
    (len > 0 && reads_as_undefined_command(spelling)).then_some(spelling)
}

/// Whether `input` starts with an infix fraction command.
pub(super) fn starts_infix_command(input: &str) -> bool {
    input.strip_prefix('\\').is_some_and(|name| {
        let len = name.bytes().take_while(u8::is_ascii_alphabetic).count();
        matches!(
            &name[..len],
            "over"
                | "atop"
                | "above"
                | "choose"
                | "brace"
                | "brack"
                | "overwithdelims"
                | "atopwithdelims"
                | "abovewithdelims"
        )
    })
}
