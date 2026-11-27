use crate::ast::arena::{AstArena, RuleDashes};
use crate::ast::compact::{self, CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, LatexProfile, Result, TypesettingPolicy};
use crate::unicode::fonts::style_to_latex_font;
use crate::unicode::symbols::{
    UNNAMED_NARY, is_latex_nary, latex_to_unicode, mapped_symbol_kind, nary_op_to_latex,
    unicode_to_latex,
};
use crate::writer::Writer;
use std::borrow::Cow;
use std::collections::HashSet;

mod buffer;
mod color_spec;
pub(crate) mod colors;
mod escape;
use buffer::LatexBuffer;
use escape::{
    Place, Spelling, ascii_needs_escape, is_no_token, needs_escape, write_char, write_char_code,
    write_char_token,
};

/// Writes LaTeX.
///
/// A color LaTeX cannot hold (`#f00`, `hsl(…)`, text that is no color) is not
/// written. [`convert`](crate::convert()) and
/// [`ConvertResult::to_latex`](crate::ConvertResult::to_latex) make the node the
/// group of its content, as if it had no color. The methods here do not know
/// whether the expression is normalized: a node with neighbours becomes the
/// ordinary atom `\mathord{…}`, which reads back the same either way.
pub struct LatexWriter;

impl LatexWriter {
    /// Fast internal bridge for an expression returned by a parser or checked
    /// conversion result. Public trait calls retain strict arena validation.
    pub(crate) fn write_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<String> {
        super::validate_compact_trusted(expr, config)?;
        write_expr_validated(expr, config, LossReport::default()).map(|(latex, _)| latex)
    }

    /// Trusted diagnostic compact writer used by the parse-once conversion
    /// result.
    #[allow(dead_code)]
    pub(crate) fn write_compact_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        let (latex, report) = write_expr_validated(expr, config, LossReport::collecting())?;
        Ok((latex, report.finish()))
    }

    /// Write an expression that was just read and not normalized, without the
    /// colors LaTeX cannot hold (see [`colors`]).
    ///
    /// `normalize` is how the expression was normalized all the same, if it
    /// was: sizes that cannot be written kept apart what that joins.
    pub(crate) fn write_read(
        expr: &CompactExpr,
        config: &Config,
        normalize: Option<Normalize>,
    ) -> Result<String> {
        super::validate_compact_trusted(expr, config)?;
        let grouped = colors::grouped(expr);
        let report = LossReport::default();
        write_lowered(grouped.as_ref().unwrap_or(expr), config, report, normalize)
            .map(|(latex, _)| latex)
    }

    /// [`write_read`](Self::write_read) with loss diagnostics.
    pub(crate) fn write_read_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
        normalize: Option<Normalize>,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        let mut report = LossReport::collecting();
        colors::report_dropped(expr, &mut report);
        let grouped = colors::grouped(expr);
        let expr = grouped.as_ref().unwrap_or(expr);
        let (latex, report) = write_lowered(expr, config, report, normalize)?;
        Ok((latex, report.finish()))
    }
}

impl Writer for LatexWriter {
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        super::validate_heap_for_write(expr, config)?;
        let compact = CompactExpr::from_math_expr_after_validation(expr)?;
        write_expr_validated(&compact, config, LossReport::default()).map(|(latex, _)| latex)
    }

    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        write_expr(expr, config, LossReport::default()).map(|(latex, _)| latex)
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let (latex, report) = write_expr(expr, config, LossReport::collecting())?;
        Ok((latex, report.finish()))
    }
}

fn write_expr(
    expr: &CompactExpr,
    config: &Config,
    report: LossReport,
) -> Result<(String, LossReport)> {
    super::validate_compact_for_write(expr, config)?;
    write_expr_validated(expr, config, report)
}

fn write_expr_validated(
    expr: &CompactExpr,
    config: &Config,
    mut report: LossReport,
) -> Result<(String, LossReport)> {
    colors::report_dropped(expr, &mut report);
    let with_atoms = colors::with_ordinary_atoms(expr);
    write_lowered(with_atoms.as_ref().unwrap_or(expr), config, report, None)
}

/// A way to normalize an expression.
pub(crate) type Normalize = fn(&mut CompactExpr) -> Result<()>;

/// `expr` with its font sizes as declarations, without those that have none.
#[cold]
pub(crate) fn lower_sizes(expr: &CompactExpr) -> Result<CompactExpr> {
    lower(
        expr,
        &mut LossReport::default(),
        crate::error::TypesettingPolicy::default(),
    )
}

fn lower(
    expr: &CompactExpr,
    report: &mut LossReport,
    policy: crate::error::TypesettingPolicy,
) -> Result<CompactExpr> {
    match super::sizes::lower_sizes(expr) {
        Ok(lowered) => {
            report.sizes(&lowered, policy);
            Ok(lowered.expr)
        },
        // The declarations make lists longer.
        Err(crate::error::Error::LimitExceeded { .. }) => {
            report.info(|| {
                "the font sizes were dropped: their declarations exceed the limits".to_owned()
            });
            let mut plain = expr.clone();
            plain.arena.clear_attrs();
            Ok(plain)
        },
        Err(error) => Err(error),
    }
}

/// Write an expression whose colors can all be written.
fn write_lowered(
    expr: &CompactExpr,
    config: &Config,
    mut report: LossReport,
    normalize: Option<Normalize>,
) -> Result<(String, LossReport)> {
    if expr.arena.has_attrs() {
        let mut lowered = lower(expr, &mut report, config.typesetting)?;
        if let Some(normalize) = normalize {
            normalize(&mut lowered)?;
        }
        super::validate_compact_for_write(&lowered, config)?;
        return write_lowered(&lowered, config, report, None);
    }
    let mut buf = LatexBuffer::new(
        config.undefined_commands == crate::UndefinedCommands::Keep,
        report,
        config.latex_profile,
    );
    write_sequence(
        &mut buf,
        &expr.arena,
        expr.arena.children(expr.roots),
        false,
    );
    let report = std::mem::take(&mut buf.report);
    Ok((buf.finish(), report))
}

/// What the output does not keep, when the caller asked for it. Reports are
/// bounded: one per message, [`LossReport::LIMIT`] messages, and the number of
/// the rest.
#[derive(Default)]
struct LossReport(Option<Box<Losses>>);

#[derive(Default)]
struct Losses {
    diagnostics: Vec<Diagnostic>,
    messages: HashSet<String>,
    rest: usize,
}

impl LossReport {
    const LIMIT: usize = 100;
    /// How many characters of a color are quoted.
    const QUOTED: usize = 40;

    fn collecting() -> Self {
        Self(Some(Box::default()))
    }

    fn is_collecting(&self) -> bool {
        self.0.is_some()
    }

    fn add(&mut self, message: impl FnOnce() -> String) {
        let Some(losses) = self.0.as_deref_mut() else {
            return;
        };
        let message = message();
        if losses.messages.contains(&message) {
            return;
        }
        if losses.diagnostics.len() >= Self::LIMIT {
            losses.rest += 1;
            return;
        }
        losses.messages.insert(message.clone());
        losses.diagnostics.push(warning(message));
    }

    /// Text that is no color was dropped.
    fn color(&mut self, spec: &str) {
        if spec.is_empty() {
            return;
        }
        self.add(|| {
            let quoted: String = spec.chars().take(Self::QUOTED).collect();
            let more = if quoted.len() < spec.len() { "…" } else { "" };
            format!("{quoted:?}{more} is not a color; it was dropped")
        });
    }

    #[cold]
    fn info(&mut self, message: impl FnOnce() -> String) {
        if let Some(losses) = self.0.as_deref_mut() {
            losses.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Info,
                message: message(),
                span: None,
            });
        }
    }

    /// Font sizes that no size command draws.
    #[cold]
    fn sizes(&mut self, lowered: &super::sizes::Lowered, policy: TypesettingPolicy) {
        let Some(losses) = self.0.as_deref_mut() else {
            return;
        };
        let mut info = |count: usize, what: &str| {
            if count > 0 {
                losses.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::Info,
                    message: format!("{count} font sizes were {what}"),
                    span: None,
                });
            }
        };
        info(
            lowered.dropped,
            "dropped: no LaTeX size command draws them there",
        );
        if policy == TypesettingPolicy::Preserve {
            info(
                lowered.approximated,
                "approximated by the nearest LaTeX size command",
            );
        }
    }

    /// A character that cannot be written in text was dropped.
    #[cold]
    fn char(&mut self, ch: char) {
        self.add(|| {
            format!(
                "the control character U+{:04X} cannot be written in text; it was dropped",
                ch as u32
            )
        });
    }

    fn finish(self) -> Vec<Diagnostic> {
        let Some(losses) = self.0 else {
            return Vec::new();
        };
        let mut diagnostics = losses.diagnostics;
        if losses.rest > 0 {
            diagnostics.push(warning(format!(
                "{} more colors or characters were dropped",
                losses.rest
            )));
        }
        diagnostics
    }
}

fn warning(message: String) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Warning,
        message,
        span: None,
    }
}

fn write_nodes(buf: &mut LatexBuffer, arena: &AstArena, span: NodeSpan) {
    write_node_slice(buf, arena, arena.children(span), false);
}

fn write_boundaries(buf: &mut LatexBuffer, boundaries: &str) {
    for boundary in span_boundaries(boundaries) {
        buf.push_str(boundary.latex());
    }
}

/// Write a sequence of sibling nodes (see `write_nodes`).
fn write_node_slice(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    children: &[NodeId],
    ends_with_command: bool,
) {
    write_sequence(buf, arena, children, ends_with_command);
}

fn write_sequence(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    mut children: &[NodeId],
    mut ends_with_command: bool,
) {
    // A trailing implicit script with no arguments emits nothing. Remove it
    // before deciding whether any preceding base needs grouping or a body.
    while let Some((&last, prefix)) = children.split_last() {
        if !is_script_with_empty_base(arena, last) || script_has_any_arg(arena, last) {
            break;
        }
        children = prefix;
    }
    // Output offsets just after each written row break, guarded at the end.
    let mut row_breaks: smallvec::SmallVec<[usize; 2]> = smallvec::SmallVec::new();
    // The atom the next sibling is spaced against, when it is not simply the
    // preceding child (a bare large operator written together with its body).
    let mut spacing_prev: Option<NodeId> = None;
    let mut i = 0;
    while i < children.len() {
        let id = children[i];
        if i > 0 && matches!(*arena.node(children[i - 1]), CompactNode::Newline) {
            row_breaks.push(buf.len());
        }
        let prev = spacing_prev
            .take()
            .or_else(|| i.checked_sub(1).map(|j| children[j]));

        if let Some(cmd) = scalar_negation_command(arena, &children[i..]) {
            buf.push_str("\\centernot");
            buf.command(cmd);
            ends_with_command = true;
            i += 2;
            continue;
        }

        // An empty-base script that no node before it took (see below). Write
        // it with an explicit `{}` base so that it stays a separate atom, as
        // TeX reads `x{}_1`, and remains stable across round-trips.
        if is_script_with_empty_base(arena, id) {
            let output_start = buf.len();
            write_standalone_empty_base_script(buf, arena, id);
            ends_with_command =
                appended_ends_with_letter_command(buf, output_start, ends_with_command);
            i += 1;
            continue;
        }

        // An empty-base script after a node keeps its own `{}` base, as in
        // TeX: `x{}_1` is x followed by an empty atom with a subscript, not
        // `x_1`. The standalone branch above writes it so. Two bases need
        // the pair written together: the parser would take the `{}` after an
        // operator with an empty body as its body, and a bare `\middle|`
        // would take the script. (Normalization still makes such a script
        // the limit of a function name or big operator before it, unlike
        // TeX: `\lim{}_x` is written `\lim_x`.)
        let middle = matches!(*arena.node(id), CompactNode::Middle(sid)
            if writes_as_nested_middle(arena.str_val(sid)));
        if i + 1 < children.len()
            && is_script_with_empty_base(arena, children[i + 1])
            && (middle
                || is_empty_body_nary(arena, id)
                    && children.get(i + 2).is_none_or(|&after| {
                        !crate::ast::normalize::can_take_prescripts(*arena.node(after))
                    }))
        {
            let output_start = buf.len();
            if i > 0 {
                let space = needs_space(arena, prev, id)
                    || (ends_with_command && node_starts_with_ascii_letter(arena, id));
                if space {
                    buf.push(' ');
                }
            }
            let script_id = children[i + 1];
            if middle {
                // A `\middle|` outside its fence reads a script after it as
                // its own, even after an empty group. Group the delimiter so
                // that the script keeps its own empty base.
                buf.push('{');
                write_node(buf, arena, id);
                buf.push('}');
                write_standalone_empty_base_script(buf, arena, script_id);
            } else {
                // The parser reads a following scripted empty group as the
                // integral's body. Group that whole body on the first write.
                write_node(buf, arena, id);
                buf.push_str(" {{}");
                write_standalone_empty_base_script_markers(buf, arena, script_id);
                buf.push('}');
            }
            ends_with_command =
                appended_ends_with_letter_command(buf, output_start, ends_with_command);
            i += 2;
            continue;
        }

        // Collapse a flattened ∫⋯∫ sequence back into the canonical \idotsint
        // command. Writing it as separate \int\cdots\int nodes is not stable
        // because the first \int greedily absorbs the \cdots as its body.
        if i + 3 <= children.len() && is_idotsint_triple(arena, &children[i..i + 3]) {
            let output_start = buf.len();
            if i > 0 {
                let space = needs_space(arena, prev, children[i])
                    || (ends_with_command && node_starts_with_ascii_letter(arena, children[i]));
                if space {
                    buf.push(' ');
                }
            }
            write_idotsint(buf);
            ends_with_command =
                appended_ends_with_letter_command(buf, output_start, ends_with_command);
            i += 3;
            continue;
        }

        let output_start = buf.len();
        if i > 0 {
            // A leading-script group is always set off by a space, so that the
            // spacing does not depend on how the node before is classified
            // (a minus read back as a binary operator gets one anyway).
            let space = needs_space(arena, prev, id)
                || prev.is_some_and(|prev| adjacent_number_nodes_need_separator(arena, prev, id))
                || (has_visible_prescripts(arena, id) && !buf.ends_with(' '))
                || (ends_with_command && node_starts_with_ascii_letter(arena, id));
            if space {
                buf.push(' ');
            }
        }

        // A bare large operator (from a single-token argument such as
        // `\braket\int`, or MathML `<mo>∫</mo>`) is read back as an n-ary
        // operator that takes the next sibling as its body. Write the pair
        // as that operator would be written; the next sibling is then spaced
        // as after an n-ary operator.
        if is_bare_large_op(arena, id) {
            let consumed = write_large_op_chain(buf, arena, &children[i..]);
            ends_with_command =
                appended_ends_with_letter_command(buf, output_start, ends_with_command);
            spacing_prev = Some(id);
            i += consumed;
            continue;
        }
        if is_script_node(arena, id) && !is_script_with_empty_base(arena, id) {
            write_script_node(buf, arena, id);
        } else {
            write_node(buf, arena, id);
            // An empty-body nary operator followed by another node needs an
            // explicit `{}` body, otherwise the next node would be re-parsed as
            // the operator's body.
            if i + 1 < children.len() && ends_with_empty_body_nary(arena, id) {
                buf.push_str("{}");
            }
        }
        ends_with_command = appended_ends_with_letter_command(buf, output_start, ends_with_command);
        i += 1;
    }
    // Insert from the back so earlier offsets stay valid.
    for &start in row_breaks.iter().rev() {
        guard_row_break_argument(buf, start);
    }
}

/// An undefined command as inert text: one ordinary atom holding the
/// spelling and each group between literal braces,
/// `\mathord{\text{\textbackslash{}foo}\{a\}}`. It reads back as that atom,
/// whose row the sequence writer spells the same way: the spaces it would
/// put after `\{` and before `\}` are put here too.
fn write_escaped_undefined_command(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    name: &str,
    groups: &[NodeId],
) {
    buf.push_str("\\mathord{");
    write_styled_text(buf, TextStyle::Normal, name);
    for &group in groups {
        buf.push_str("\\{");
        let mut children: &[NodeId] = match *arena.node(group) {
            CompactNode::Row(span) => arena.children(span),
            _ => std::slice::from_ref(&group),
        };
        // A leading script without a base (one that did not become the next
        // node's leading script) is written as the sequence writer writes it
        // after an escaped brace: as the markers alone.
        if let [first, _, ..] = *children
            && is_script_with_empty_base(arena, first)
        {
            write_empty_base_script_markers(buf, arena, first);
            children = &children[1..];
        }
        // A trailing script without a base becomes the leading script of
        // the `\}` on the next read; write it so already.
        let (children, trailing_script) = match children.split_last() {
            Some((&last, rest)) if is_script_with_empty_base(arena, last) => (rest, Some(last)),
            _ => (children, None),
        };
        // On the next read the group's first node follows the `\{` in one
        // row, so it gets the space `write_sequence` puts before a node with
        // leading scripts.
        if children
            .first()
            .is_some_and(|&first| has_visible_prescripts(arena, first))
        {
            buf.push(' ');
        }
        write_node_slice(buf, arena, children, false);
        // `\int\}` would read the brace (or the scripted brace standing for
        // the trailing script) as the operator's body.
        let ends_with_idotsint = is_idotsint_triple_at_end(arena, children);
        // The last node is a bare operator's body, spaced and braced as part
        // of it.
        let last_is_body = children.split_last().is_some_and(|(&last, before)| {
            before
                .last()
                .is_some_and(|&operator| is_bare_large_op(arena, operator))
                && reads_as_nary_body(arena, last)
                && !is_idotsint_triple_at_end(arena, before)
        });
        let empty_nary_body = !last_is_body
            && children
                .last()
                .is_some_and(|&last| ends_with_empty_body_nary(arena, last));
        if empty_nary_body {
            buf.push_str("{}");
        }
        if let Some(script) = trailing_script {
            if !buf.ends_with(' ') {
                buf.push(' ');
            }
            write_brace_prescripts(buf, arena, script);
            continue;
        }
        if !empty_nary_body
            && let Some(&last) = children.last()
            && !ends_with_idotsint
            && (is_bare_large_op(arena, last)
                || !last_is_body && needs_space(arena, Some(last), group))
        {
            buf.push(' ');
        }
        buf.push_str("\\}");
    }
    buf.push('}');
}

fn is_idotsint_triple_at_end(arena: &AstArena, nodes: &[NodeId]) -> bool {
    nodes
        .last_chunk::<3>()
        .is_some_and(|triple| is_idotsint_triple(arena, triple))
}

/// `\}` with the scripts of the empty-base script `script` as its leading
/// scripts, spelled as the writer spells such a node: `{{}^{2}\}}`.
fn write_brace_prescripts(buf: &mut LatexBuffer, arena: &AstArena, script: NodeId) {
    let (sub, sup) = match *arena.node(script) {
        CompactNode::Subscript { sub, .. } => (sub, NodeId::NONE),
        CompactNode::Superscript { sup, .. } => (NodeId::NONE, sup),
        CompactNode::SubSup { sub, sup, .. } => (sub, sup),
        _ => (NodeId::NONE, NodeId::NONE),
    };
    buf.push_str("{{}");
    for (mark, script) in [('_', sub), ('^', sup)] {
        if script.is_some() && !is_effectively_empty_arg(arena, script) {
            buf.push(mark);
            write_script_arg(buf, arena, script, ScriptArgGroup::Always);
        }
    }
    buf.push_str("\\}}");
}

/// One brace group after an undefined command.
fn write_undefined_command_group(buf: &mut LatexBuffer, arena: &AstArena, group: NodeId) {
    buf.push('{');
    match *arena.node(group) {
        // A group holding just a group (MathML `<mrow>` around a style row)
        // is written as that one group.
        CompactNode::Row(span) => match *arena.children(span) {
            [only] => match *arena.node(only) {
                CompactNode::Row(inner) => write_nodes(buf, arena, inner),
                _ => write_nodes(buf, arena, span),
            },
            _ => write_nodes(buf, arena, span),
        },
        _ => write_node(buf, arena, group),
    }
    buf.push('}');
}

/// A `[` after a row break `\\` (even past white space) is read as the
/// break's optional spacing argument by LaTeX and by this crate's table
/// parser, so the bracketed content would vanish. If the output written from
/// `start` (just after a `\\`) begins with `[`, put an empty group before
/// it: `\\ {}[b]` reads back as the literal bracket, and the empty group is
/// dropped by normalization.
///
/// A leading `*` can likewise be taken as the starred break `\\*`: LaTeX's
/// `\@ifstar` looks for the star with `\@ifnextchar`, which skips spaces.
/// `\relax` stops it and is no atom: `{}*` would turn the star into a spaced
/// binary operator, and `{*}` into an ordinary atom.
fn guard_row_break_argument(buf: &mut LatexBuffer, start: usize) {
    let Some(rest) = buf.get(start..) else {
        return;
    };
    let offset = rest.len() - rest.trim_start().len();
    let at = start + offset;
    match rest.as_bytes().get(offset) {
        Some(b'[') => buf.insert_text(at, "{}"),
        Some(b'*') => buf.insert_text(at, "\\relax "),
        _ => {},
    }
}

/// The preamble rule at a column boundary: arydshln's `:` when dashed.
fn column_rule(dashes: RuleDashes, boundary: usize) -> char {
    if dashes.cols & (1 << boundary) != 0 {
        ':'
    } else {
        '|'
    }
}

/// The rule command at a row boundary, with its trailing space.
fn row_rule(dashes: RuleDashes, boundary: usize) -> &'static str {
    if dashes.rows & (1 << boundary) != 0 {
        r"\hdashline "
    } else {
        r"\hline "
    }
}

/// Separate adjacent numeric tokens when their concatenation would contain
/// more than one decimal point. The parser ignores source whitespace, so this
/// keeps two authored decimal literals distinct without changing ordinary
/// integer-run canonicalization.
fn adjacent_number_nodes_need_separator(
    arena: &AstArena,
    previous: NodeId,
    current: NodeId,
) -> bool {
    let (CompactNode::Number(previous), CompactNode::Number(current)) =
        (*arena.node(previous), *arena.node(current))
    else {
        return false;
    };
    !crate::ast::normalize::can_merge_number_text(arena.str_val(previous), arena.str_val(current))
}

/// True if the node is, or its LaTeX ends with, a Nary operator with an
/// effectively empty body (a leading script's base is written last), so
/// that the next sibling would be read as its body.
#[inline]
fn ends_with_empty_body_nary(arena: &AstArena, id: NodeId) -> bool {
    matches!(
        *arena.node(id),
        CompactNode::PreScripts { .. } | CompactNode::Nary { .. }
    ) && output_ends_with_empty_body_nary(arena, id)
}

fn output_ends_with_empty_body_nary(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::PreScripts { base, .. } => {
            is_bare_large_op(arena, base) || output_ends_with_empty_body_nary(arena, base)
        },
        CompactNode::Nary { body, .. } if is_empty_body(arena, body) => {
            is_empty_body_nary(arena, id)
        },
        // Without a command: `\mathop{…}` and its body (`write_unnamed_nary`).
        CompactNode::Nary { op, body, .. } => {
            (is_bare_large_op(arena, body) || output_ends_with_empty_body_nary(arena, body))
                && !is_latex_nary(op)
        },
        _ => false,
    }
}

/// True if the node is a Nary operator, read back as one, with an
/// effectively empty body.
fn is_empty_body_nary(arena: &AstArena, id: NodeId) -> bool {
    matches!(
        *arena.node(id),
        CompactNode::Nary { op, body, .. }
            if is_empty_body(arena, body) && is_latex_nary(op)
    )
}

/// True for Subscript/Superscript/SubSup nodes.
fn is_script_node(arena: &AstArena, id: NodeId) -> bool {
    matches!(
        *arena.node(id),
        CompactNode::Subscript { .. }
            | CompactNode::Superscript { .. }
            | CompactNode::SubSup { .. }
    )
}

/// Write a script node in a row. Its arguments are spelled by
/// `write_script_arg`, so they cannot merge with the following sibling.
fn write_script_node(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Subscript { base, sub } => {
            if is_effectively_empty_arg(arena, sub) {
                // A script with no effective argument disappears; serialize
                // its base exactly as a normal sibling so transparent wrappers
                // (for example an empty color declaration) do not gain a
                // grouping pair that vanishes on the next parse.
                write_node(buf, arena, base);
                return;
            }
            write_postscript_base(buf, arena, base);
            buf.push('_');
            write_script_arg(buf, arena, sub, ScriptArgGroup::Minimal);
        },
        CompactNode::Superscript { base, sup } => {
            if is_effectively_empty_arg(arena, sup) {
                write_node(buf, arena, base);
                return;
            }
            write_postscript_base(buf, arena, base);
            buf.push('^');
            write_script_arg(buf, arena, sup, ScriptArgGroup::Minimal);
        },
        CompactNode::SubSup { base, sub, sup } => {
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if sub_empty && sup_empty {
                write_node(buf, arena, base);
                return;
            }
            write_postscript_base(buf, arena, base);
            if !sub_empty {
                buf.push('_');
                write_script_arg(buf, arena, sub, ScriptArgGroup::Minimal);
            }
            if !sup_empty {
                buf.push('^');
                write_script_arg(buf, arena, sup, ScriptArgGroup::Minimal);
            }
        },
        _ => write_node(buf, arena, id),
    }
}

/// Bracing an operator before its scripts turns it into an ordinary TeX
/// group. Write the operator token directly so its requested limit policy
/// stays in effect. A side-scripted compound operator needs explicit nolimits.
fn write_postscript_base(buf: &mut LatexBuffer, arena: &AstArena, base: NodeId) {
    // (Only an operator: other bases keep their own grouping.)
    let written = limitless_base(arena, base);
    let base = match *arena.node(written) {
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        } if matches!(
            *arena.node(written_script_content(arena, base)),
            CompactNode::Overscript { .. } | CompactNode::Underscript { .. }
        ) =>
        {
            written
        },
        _ => base,
    };
    match *arena.node(base) {
        CompactNode::Operator { .. } | CompactNode::StructuredOperator { .. } => {
            write_node(buf, arena, base)
        },
        // Without limits and body, such an operator is only its `\mathop`.
        CompactNode::Nary {
            op, sub, sup, body, ..
        } if !is_latex_nary(op)
            && is_empty_body(arena, body)
            && is_effectively_empty_arg(arena, sub)
            && is_effectively_empty_arg(arena, sup) =>
        {
            buf.push_str("{\\mathord{");
            write_mathop_glyph(buf, op, mapped_symbol_kind(op));
            buf.push_str("}}");
        },
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => {
            // The reader gives the name in `\mathop{\lim}\nolimits` the placement.
            if let CompactNode::Operator { name, .. } = *arena.node(content) {
                write_operator_name(buf, &kept_name(arena.str_val(name)), LimitStyle::SubSup);
                return;
            }
            write_math_atom_op(buf, arena, content);
            buf.push_str("\\nolimits");
        },
        // A font command is one atom already: `\mathbf{F}_{x}`.
        _ if matches!(
            *arena.node(written_script_content(arena, base)),
            CompactNode::Styled(..)
        ) =>
        {
            write_node(buf, arena, written_script_content(arena, base));
        },
        _ => write_script_base(buf, arena, base),
    }
}

/// The first argument of `\overset` or `\underset`, where the reader takes a
/// lone combining mark for an accent.
fn write_stacked_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    let glyph = match *arena.node(written_script_content(arena, content)) {
        CompactNode::Symbol(_, sid) => single_char(arena.str_val(sid)),
        _ => None,
    };
    if glyph.is_some_and(is_combining_mark) {
        buf.push_str("\\mathord{");
        write_atom_content(buf, arena, content);
        buf.push('}');
    } else {
        write_wrapped_content(buf, arena, content);
    }
}

/// Begin `\overset{…}{` or `\underset{…}{`, or the command whose decoration
/// `content` is on that side (the reader takes the glyph for it).
fn begin_stack(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId, side: AccentPosition) {
    let glyph = match *arena.node(written_script_content(arena, content)) {
        CompactNode::Symbol(_, sid) => single_char(arena.str_val(sid)),
        _ => None,
    };
    let command = match (side, glyph) {
        (AccentPosition::Above, Some('\u{203E}')) => "\\overline{",
        (AccentPosition::Above, Some('\u{23B4}')) => "\\overlinesegment{",
        (AccentPosition::Above, Some('\u{23DC}')) => "\\overgroup{",
        (AccentPosition::Above, Some('\u{23DE}')) => "\\overbrace{",
        (AccentPosition::Below, Some('\u{203E}')) => "\\underline{",
        (AccentPosition::Below, Some('\u{23B5}')) => "\\underlinesegment{",
        (AccentPosition::Below, Some('\u{23DD}')) => "\\undergroup{",
        (AccentPosition::Below, Some('\u{23DF}')) => "\\underbrace{",
        _ => {
            buf.push_str(match side {
                AccentPosition::Above => "\\overset{",
                AccentPosition::Below => "\\underset{",
            });
            write_stacked_content(buf, arena, content);
            "}{"
        },
    };
    buf.push_str(command);
}

/// The base under or over `content`, which an accent glyph makes an accent's.
fn write_stack_base(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId, base: NodeId) {
    let accent = matches!(*arena.node(written_script_content(arena, content)),
    CompactNode::Symbol(_, sid) if single_char(arena.str_val(sid)).is_some_and(|ch| {
        crate::unicode::symbols::is_accent_glyph(ch) && !is_combining_mark(ch)
    }));
    if accent && matches!(*arena.node(base), CompactNode::Operator { .. }) {
        write_accent_base_content(buf, arena, base);
    } else {
        write_wrapped_content(buf, arena, base);
    }
}

/// The node that is written for `base`: a `\mathop{x}` without its own limit
/// when that writes nothing.
fn limitless_base(arena: &AstArena, base: NodeId) -> NodeId {
    let mut base = written_script_content(arena, base);
    // `\mathop{x}` whose own limit is not written.
    while let CompactNode::Overscript {
        base: inner,
        over: limit,
        movable_limits: true,
    }
    | CompactNode::Underscript {
        base: inner,
        under: limit,
        movable_limits: true,
    } = *arena.node(base)
        && is_effectively_empty_arg(arena, limit)
    {
        base = written_script_content(arena, inner);
    }
    base
}

fn write_compound_limits(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    base: NodeId,
    lower: NodeId,
    upper: NodeId,
    movable: bool,
) -> bool {
    let placed = |limit| !is_effectively_empty_arg(arena, limit);
    let base = limitless_base(arena, base);
    // A placement with nothing to place is not read back.
    if !movable && !placed(lower) && !placed(upper) {
        return false;
    }
    let write_limits = |buf: &mut LatexBuffer| {
        for (marker, limit) in [('_', lower), ('^', upper)] {
            if placed(limit) {
                buf.push(marker);
                write_script_arg(buf, arena, limit, ScriptArgGroup::Minimal);
            }
        }
    };
    // The label of a horizontal brace is its limit. A script on the other
    // side is set beside the brace by KaTeX, so that one is stacked.
    if let Some(side) = crate::ast::brace_side(arena, base) {
        let (own, other, marker, other_side) = match side {
            AccentPosition::Above => (upper, lower, '^', AccentPosition::Below),
            AccentPosition::Below => (lower, upper, '_', AccentPosition::Above),
        };
        if !placed(own) {
            return false;
        }
        if placed(other) {
            begin_stack(buf, arena, other, other_side);
        }
        // As in `Overscript`: such a label is a decoration.
        let decoration = is_line_symbol(arena, own)
            || match side {
                AccentPosition::Above => is_overbrace(arena, own),
                AccentPosition::Below => is_underbrace(arena, own),
            };
        if decoration {
            begin_stack(buf, arena, own, side);
            write_node(buf, arena, base);
            buf.push('}');
        } else {
            write_node(buf, arena, base);
            buf.push(marker);
            write_script_arg(buf, arena, own, ScriptArgGroup::Minimal);
        }
        if placed(other) {
            buf.push('}');
        }
        return true;
    }
    let CompactNode::MathAtom {
        atom_type: MathAtomType::Op,
        content,
    } = *arena.node(base)
    else {
        return false;
    };
    // The reader gives a named operator in `\mathop{\lim}\limits^a` its own
    // placement (`\lim\limits^a`), a different tree. An explicit wrapper
    // (`\overset{a}{\mathop{\lim}}`, `\overline{…}`) reads back as written.
    if !movable && matches!(*arena.node(content), CompactNode::Operator { .. }) {
        return false;
    }
    write_math_atom_op(buf, arena, content);
    if !movable {
        buf.push_str("\\limits");
    }
    write_limits(buf);
    true
}

/// In an unbraced script a large-operator control word is one token. A
/// group around the same command instead gives the parser an n-ary body.
fn write_scalar_operator_arg(buf: &mut LatexBuffer, arena: &AstArena, arg: NodeId) -> bool {
    let CompactNode::Symbol(SymbolKind::LargeOp, sid) = *arena.node(arg) else {
        return false;
    };
    if let Some(cmd) = single_char(arena.str_val(sid)).and_then(nary_op_to_latex) {
        buf.command(cmd);
        true
    } else {
        false
    }
}

/// How a script argument is grouped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScriptArgGroup {
    /// The canonical spelling: a token on its own when TeX reads it as the
    /// whole argument, otherwise a brace group.
    Minimal,
    /// Always a brace group, for scripts that must stay isolated from the
    /// text around them (scripts without a base, leading scripts). A large
    /// operator's control word is still written bare.
    Always,
}

/// Write the argument of a `_`/`^` marker. Every script and limit argument
/// the writer emits goes through here, so it is spelled the same way in every
/// context.
///
/// TeX, KaTeX and MathJax take exactly one token after a script marker:
/// `x^22` is `x^{2}2` and `x^2.5` is `x^{2}.5`. The argument is therefore
/// written either as one token (`x^a`, `x^\alpha`) or as one brace group.
/// Numbers are always braced, so that neither a following digit or `.` nor
/// the argument's position in its row changes the spelling.
///
/// Returns whether the argument was written as a bare control word, which
/// needs a separator before a following letter.
fn write_script_arg(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    arg: NodeId,
    group: ScriptArgGroup,
) -> bool {
    let start = buf.len();
    match group {
        ScriptArgGroup::Always => {
            if !write_scalar_operator_arg(buf, arena, arg) {
                write_always_braced(buf, arena, arg);
                if buf.len() == start {
                    buf.push_str("{}");
                }
            }
        },
        ScriptArgGroup::Minimal => {
            let arg = written_script_content(arena, arg);
            if write_scalar_operator_arg(buf, arena, arg) {
                // A large-operator control word is one token.
            } else if is_numeric_argument(arena, arg) || is_ordinary_colon(arena, arg) {
                buf.push('{');
                write_node(buf, arena, arg);
                buf.push('}');
            } else {
                // Use the same token spelling and grouping rules as a script
                // base. In particular, an alphabetic Unicode symbol such as ℜ
                // emits the preferred control word `\Re` without braces, while
                // non-alphabetic symbols keep the braces needed to isolate
                // their control words.
                write_script_base(buf, arena, arg);
            }
        },
    }
    let written = &buf[start..];
    // A base may be a command and its argument, a script argument may not: TeX
    // would take the command alone. So a single glyph spelled `\char…`,
    // `\mathrel{…}` or `\mathop{…}` is grouped here. Every other spelling above
    // is one argument already; one that is not would render differently from
    // the AST, so it fails debug builds (tests and fuzzing), and release
    // builds still group it.
    if !is_one_tex_argument(written) {
        debug_assert!(
            is_script_base_token(arena, written_script_content(arena, arg)),
            "script argument `{written}` is not one TeX token"
        );
        buf.insert_text(start, "{");
        buf.push('}');
        debug_assert!(
            is_one_tex_argument(&buf[start..]),
            "script argument `{}` is not one group",
            &buf[start..]
        );
        return false;
    }
    let bytes = written.as_bytes();
    bytes.len() > 1 && bytes[0] == b'\\' && bytes[1..].iter().all(u8::is_ascii_alphabetic)
}

/// An ordinary colon is written `{:}`, which as an argument is a plain one.
fn is_ordinary_colon(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(SymbolKind::Ordinary, sid)
        if arena.str_val(sid) == ":")
}

/// Whether TeX reads `text`, written right after a `_`/`^` marker, as exactly
/// that marker's argument: a single character, a single control sequence, or
/// one brace group spanning all of `text`.
fn is_one_tex_argument(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        None => false,
        Some('{') => {
            let mut depth = 1usize;
            let mut chars = text[1..].chars();
            while let Some(ch) = chars.next() {
                match ch {
                    '\\' => {
                        chars.next();
                    },
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return chars.next().is_none();
                        }
                    },
                    _ => {},
                }
            }
            false
        },
        Some('\\') => match chars.next() {
            None => false,
            Some(ch) if ch.is_ascii_alphabetic() => chars.all(|ch| ch.is_ascii_alphabetic()),
            // A space command is no argument in KaTeX.
            Some(ch) => chars.next().is_none() && !matches!(ch, ',' | ':' | ';' | '!' | '>' | ' '),
        },
        Some(ch) => {
            chars.next().is_none()
                && !ch.is_whitespace()
                && !matches!(ch, '}' | '%' | '#' | '&' | '$' | '^' | '_')
        },
    }
}

/// A numeric script or large-operator argument needs an explicit token
/// boundary so the next sibling cannot extend its decimal literal.
fn is_numeric_argument(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(_) => true,
        CompactNode::Row(span) => match arena.children(span) {
            &[single] => is_numeric_argument(arena, single),
            _ => row_produces_only_digits(arena, span),
        },
        CompactNode::Scaled { content, .. } => is_numeric_argument(arena, content),
        _ => false,
    }
}

/// Write a stand-alone empty-base script with an explicit `{}` base.
fn write_standalone_empty_base_script(buf: &mut LatexBuffer, arena: &AstArena, script_id: NodeId) {
    if !script_has_any_arg(arena, script_id) {
        return;
    }
    buf.push_str("{}");
    write_standalone_empty_base_script_markers(buf, arena, script_id);
}

/// Write the `_`/`^` markers of a stand-alone empty-base script. Arguments are
/// always braced so the output is independent of the surrounding context and
/// remains stable when re-parsing normalizes adjacent tokens.
fn write_standalone_empty_base_script_markers(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    script_id: NodeId,
) {
    match *arena.node(script_id) {
        CompactNode::Subscript { sub, .. } if !is_effectively_empty_arg(arena, sub) => {
            buf.push('_');
            write_script_arg(buf, arena, sub, ScriptArgGroup::Always);
        },
        CompactNode::Superscript { sup, .. } if !is_effectively_empty_arg(arena, sup) => {
            buf.push('^');
            write_script_arg(buf, arena, sup, ScriptArgGroup::Always);
        },
        CompactNode::SubSup { sub, sup, .. } => {
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if !sub_empty {
                buf.push('_');
                write_script_arg(buf, arena, sub, ScriptArgGroup::Always);
            }
            if !sup_empty {
                buf.push('^');
                write_script_arg(buf, arena, sup, ScriptArgGroup::Always);
            }
        },
        _ => {},
    }
}

/// Write the `_`/`^` markers and arguments of an empty-base script node.
fn write_empty_base_script_markers(buf: &mut LatexBuffer, arena: &AstArena, script_id: NodeId) {
    match *arena.node(script_id) {
        CompactNode::Subscript { sub, .. } => {
            if is_effectively_empty_arg(arena, sub) {
                return;
            }
            buf.push('_');
            write_script_arg(buf, arena, sub, ScriptArgGroup::Minimal);
        },
        CompactNode::Superscript { sup, .. } => {
            if is_effectively_empty_arg(arena, sup) {
                return;
            }
            buf.push('^');
            write_script_arg(buf, arena, sup, ScriptArgGroup::Minimal);
        },
        CompactNode::SubSup { sub, sup, .. } => {
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if sub_empty && sup_empty {
                return;
            }
            if !sub_empty {
                buf.push('_');
                write_script_arg(buf, arena, sub, ScriptArgGroup::Minimal);
            }
            if !sup_empty {
                buf.push('^');
                write_script_arg(buf, arena, sup, ScriptArgGroup::Minimal);
            }
        },
        _ => {},
    }
}

fn needs_space(arena: &AstArena, prev: Option<NodeId>, curr: NodeId) -> bool {
    let Some(prev_id) = prev else { return false };
    // A spacing or newline node already provides/demands separation; adding an
    // extra literal space before it can produce trailing whitespace that the
    // parser trims away (e.g. `= \kern` writing `= `).
    if matches!(
        *arena.node(curr),
        CompactNode::Space(_) | CompactNode::Newline
    ) {
        return false;
    }
    match *arena.node(prev_id) {
        // A sizing wrapper around a relation/binary-op atom behaves like the
        // wrapped atom for spacing decisions (e.g. \Big\ggg-).
        CompactNode::Scaled { content, .. } => needs_space(arena, Some(content), curr),
        CompactNode::PreScripts { base, .. } => needs_space(arena, Some(base), curr),
        // Written as an operator atom and then its body, which is what `curr`
        // follows (see `write_unnamed_nary`).
        CompactNode::Nary { op, body, .. } => nary_needs_space(arena, op, body, curr),
        // An operatorname ending in a braced argument already delimits the
        // name, e.g. `\operatorname{rank}A`. Native control words retain
        // their usual separator.
        CompactNode::Operator { name, limits } => {
            is_operator_command_name(arena.str_val(name)) || limits == LimitStyle::AboveBelow
        },
        CompactNode::Symbol(SymbolKind::BinaryOp, _)
        | CompactNode::Symbol(SymbolKind::Relation, _) => true,
        // \mathrel / \mathbin wrappers behave like the relation/binary-op
        // symbols they contain, so they also need trailing space.
        CompactNode::MathAtom {
            atom_type: MathAtomType::Rel | MathAtomType::Bin,
            ..
        } => true,
        // Control words are separated using the emitted buffer in write_node_slice.
        _ => false,
    }
}

/// Write a cases-family array using its retained source environment. The
/// parser records this sparse provenance because a generic fenced array cannot
/// recover whether authored rows were cases, rcases, or their display-style
/// variants. The compact array shape remains the source of truth for cells.
fn write_cases_array(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    id: NodeId,
    right: bool,
    display: bool,
    starred: bool,
) -> bool {
    let CompactNode::Array {
        cells,
        num_cols,
        arraystretch_pct,
        add_jot,
        array_style,
        col_seps,
        row_seps,
        ..
    } = *arena.node(id)
    else {
        return false;
    };
    // Cases parsing uses the standard 1.2 row pitch. Preserve the provenance
    // only while that shape is intact. An edited array with a different pitch
    // needs the generic array writer, which can express its current metadata.
    if arraystretch_pct != 120 || add_jot || array_style || col_seps != 0 || row_seps != 0 {
        return false;
    }
    let env = match (right, display, starred) {
        (false, false, false) => "cases",
        (false, true, false) => "dcases",
        (false, true, true) => "dcases*",
        (true, false, false) => "rcases",
        (true, true, false) => "drcases",
        (true, true, true) => "rcases*",
        (false, false, true) => "cases*",
        (true, false, true) => "rcases*",
    };
    let columns = usize::from(num_cols.max(1));
    let rows = arena.children(cells);
    let authored_rows = arena.table_metadata(id).and_then(|metadata| {
        (!metadata.row_lengths.is_empty()).then_some(metadata.row_lengths.as_ref())
    });
    buf.push_str("\\begin{");
    buf.push_str(env);
    buf.push_str("} ");
    for (row_index, row) in rows.chunks(columns).enumerate() {
        if row_index != 0 {
            buf.push_str(" \\\\ ");
        }
        let authored_target = authored_rows
            .and_then(|lengths| lengths.get(row_index))
            .copied()
            .map_or(usize::MAX, usize::from);
        for (column, cell) in row.iter().take(authored_target).enumerate() {
            if column != 0 {
                buf.push_str(" & ");
            }
            let (_, nodes) = arena.table_cell_parts(cell);
            write_node_slice(buf, arena, nodes, false);
        }
    }
    buf.push_str(" \\end{");
    buf.push_str(env);
    buf.push('}');
    true
}

#[inline(never)]
fn nary_needs_space(arena: &AstArena, op: char, body: NodeId, curr: NodeId) -> bool {
    !is_empty_body(arena, body) && needs_space(arena, Some(body), curr) && !is_latex_nary(op)
}

/// A superscript/subscript with an empty base is meant to attach to the
/// preceding atom; inserting a space before it would cause re-parsing to bind
/// the script to that preceding atom, changing the output shape.
fn is_script_with_empty_base(arena: &AstArena, id: NodeId) -> bool {
    let base = match *arena.node(id) {
        CompactNode::Superscript { base, .. }
        | CompactNode::Subscript { base, .. }
        | CompactNode::SubSup { base, .. } => base,
        _ => return false,
    };
    is_empty_script_base(arena, base)
}

/// Whether the last node of `children` that is written is a script without a
/// base, or a group (which reading back may flatten) that ends with one.
fn ends_with_bare_script<'a>(arena: &'a AstArena, mut children: &'a [NodeId]) -> bool {
    while let Some((&last, before)) = children.split_last() {
        match *arena.node(last) {
            CompactNode::Row(span) if !arena.children(span).is_empty() => {
                children = arena.children(span);
            },
            // Leading scripts of nothing.
            CompactNode::PreScripts { base, .. }
                if has_visible_prescripts(arena, last) && is_effectively_empty_arg(arena, base) =>
            {
                return true;
            },
            _ if is_empty_row(arena, last) => children = before,
            _ if is_script_with_empty_base(arena, last) => {
                if script_has_any_arg(arena, last) {
                    return true;
                }
                children = before;
            },
            _ => return false,
        }
    }
    false
}

/// Update the control-word state after a writer operation appended to `buf`.
/// The scan is restricted to the newly appended suffix. A plain identifier
/// run therefore carries the previous state in constant time instead of
/// rescanning the whole row at every sibling boundary.
#[inline]
fn appended_ends_with_letter_command(buf: &str, start: usize, previous: bool) -> bool {
    if buf.len() == start {
        return previous;
    }
    let bytes = &buf.as_bytes()[start..];
    if !bytes.last().is_some_and(|byte| byte.is_ascii_alphabetic()) {
        return false;
    }
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_alphabetic() {
        i -= 1;
    }
    if i == 0 {
        return previous;
    }
    // An escaped backslash (``\char`\\``) starts no command.
    let backslashes = bytes[..i].iter().rev().take_while(|&&b| b == b'\\').count();
    backslashes % 2 == 1
}

/// Check if a node's first written character would be a plain ASCII letter.
/// (Only where a space is written early: `LatexBuffer` sets every letter apart
/// from the control word before it.)
/// Recursively handles compound nodes whose base is written via `write_braced`.
fn node_starts_with_ascii_letter(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            let ch = s.chars().next().unwrap_or('?');
            ch.is_ascii_alphabetic() && unicode_to_latex(ch).is_none()
        },
        // An ordinary letter is a character token, a large operator `\mathop`.
        CompactNode::Symbol(kind, sid) => {
            !matches!(kind, SymbolKind::Ordinary | SymbolKind::LargeOp)
                && matches!(arena.str_val(sid).as_bytes(), [b] if b.is_ascii_alphabetic())
        },
        CompactNode::Number(sid) => arena
            .str_val(sid)
            .as_bytes()
            .first()
            .is_some_and(|&b| b.is_ascii_alphabetic()),
        // An elided prescript writes only its base. Preserve the control-word
        // separator when that base begins with an ASCII letter.
        CompactNode::PreScripts { base, .. } if !has_visible_prescripts(arena, id) => {
            node_starts_with_ascii_letter(arena, base)
        },
        // Script nodes: first thing written is write_braced(base).
        // write_braced skips braces only for simple tokens (not Row or other compound).
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => {
            is_simple_token(arena, base) && node_starts_with_ascii_letter(arena, base)
        },
        // Rows are written by emitting their children in order, so the first
        // child determines what character would appear next.
        CompactNode::Row(span) => arena
            .children(span)
            .first()
            .is_some_and(|&first| node_starts_with_ascii_letter(arena, first)),
        // A sizing wrapper around a letter identifier will emit that identifier
        // if the sizing command is dropped (non-delimiter content), so the
        // wrapper itself can start with a letter.
        CompactNode::Scaled { content, .. } => node_starts_with_ascii_letter(arena, content),
        _ => false,
    }
}

/// Detect the row produced by `\idotsint` (∫⋯∫). Writing it expanded as
/// `\int\cdots\int` is not re-parseable because the first `\int` greedily
/// absorbs the `\cdots` as its body.
fn is_idotsint_row(arena: &AstArena, id: NodeId) -> bool {
    let CompactNode::Row(span) = *arena.node(id) else {
        return false;
    };
    let children = arena.children(span);
    children.len() == 3 && is_idotsint_triple(arena, children)
}

fn is_idotsint_triple(arena: &AstArena, ids: &[NodeId]) -> bool {
    if ids.len() != 3 {
        return false;
    }
    let is_int = |id: NodeId| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::LargeOp, sid)
                if arena.str_val(sid) == "\u{222B}"
        )
    };
    let is_dots = |id: NodeId| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::Ordinary, sid)
                if arena.str_val(sid) == "\u{22EF}"
        )
    };
    is_int(ids[0]) && is_dots(ids[1]) && is_int(ids[2])
}

/// Write the canonical `\idotsint` command for an idotsint row.
fn write_idotsint(buf: &mut LatexBuffer) {
    buf.push_str("\\idotsint");
}

#[derive(Clone, Copy)]
enum MathLeafKind {
    Number,
    Identifier,
    Symbol(SymbolKind),
}

fn write_leaf_with_math_spaces(buf: &mut LatexBuffer, s: &str, kind: MathLeafKind) -> bool {
    let Some((first_space, _)) = s
        .char_indices()
        .find(|&(_, ch)| crate::unicode::math_space_width(ch).is_some())
    else {
        return false;
    };

    // Keep contiguous non-space runs intact. A run may be a multi-character
    // operator (for example `lim`) or identifier; splitting it into one AST
    // node per character loses that information and allocates unnecessarily.
    let mut segment_start = 0;
    for (index, ch) in s
        .char_indices()
        .skip_while(|&(index, _)| index < first_space)
    {
        let Some(width) = crate::unicode::math_space_width(ch) else {
            continue;
        };
        if segment_start < index {
            write_math_leaf_segment(buf, &s[segment_start..index], kind);
        }
        write_space(buf, width);
        segment_start = index + ch.len_utf8();
    }
    if segment_start < s.len() {
        write_math_leaf_segment(buf, &s[segment_start..], kind);
    }
    true
}

fn write_math_leaf_segment(buf: &mut LatexBuffer, s: &str, kind: MathLeafKind) {
    match kind {
        MathLeafKind::Number => write_number_chars(buf, s),
        MathLeafKind::Identifier => write_identifier_text(buf, s),
        MathLeafKind::Symbol(symbol_kind) => write_symbol_text(buf, symbol_kind, s),
    }
}

/// A number is written as it stands (`1.5`, `1,000`, `½`), except for
/// characters that are not themselves in a math list.
fn write_number_chars(buf: &mut LatexBuffer, s: &str) {
    for ch in s.chars() {
        write_char(buf, ch, Place::Number);
    }
}

/// An unbraced centernot argument is one TeX token. Bracing a scalar large
/// operator would turn it into an n-ary expression with a body on reparse.
fn scalar_negation_command(arena: &AstArena, children: &[NodeId]) -> Option<&'static str> {
    let &[base, slash, ..] = children else {
        return None;
    };
    let CompactNode::Symbol(SymbolKind::Ordinary, mark) = *arena.node(slash) else {
        return None;
    };
    let CompactNode::Symbol(SymbolKind::LargeOp, symbol) = *arena.node(base) else {
        return None;
    };
    if arena.str_val(mark) != "\u{0338}" {
        return None;
    }
    let glyph = arena.str_val(symbol);
    let ch = glyph.chars().next()?;
    if glyph.len() != ch.len_utf8() {
        return None;
    }
    match ch {
        '∫' => Some("intop"),
        _ => nary_op_to_latex(ch),
    }
}

fn write_node(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Number(sid) => {
            let s = arena.str_val(sid);
            if s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'.' | b',')) {
                buf.push_str(s);
            } else if !write_leaf_with_math_spaces(buf, s, MathLeafKind::Number) {
                write_number_chars(buf, s);
            }
        },

        CompactNode::Identifier(sid) => write_identifier(buf, arena, sid),

        CompactNode::Operator { name, limits } => {
            write_operator_name(buf, arena.str_val(name), limits);
        },

        CompactNode::Symbol(kind, sid) => write_symbol(buf, arena, kind, sid),

        CompactNode::Text(style, sid) => write_styled_text(buf, style, arena.str_val(sid)),

        CompactNode::Space(r) => write_space_with_basis(buf, r, arena.space_basis(id)),

        CompactNode::Row(span) => {
            let children = arena.children(span);
            if children.len() == 2
                && let Some(cmd) = scalar_negation_command(arena, children)
            {
                buf.push_str("\\centernot");
                buf.command(cmd);
                return;
            }
            if is_idotsint_row(arena, id) {
                write_idotsint(buf);
                return;
            }
            if row_contains_style_switch(arena, span, 0)
                || matches!(children, [only] if matches!(*arena.node(*only), CompactNode::Newline))
            {
                // A group that is just `\\` is a break within a table cell,
                // where a bare `\\` would end the row.
                buf.push('{');
                write_nodes(buf, arena, span);
                buf.push('}');
            } else if children.len() == 1 {
                // Single-child Row: emit without braces to keep output canonical.
                // This avoids `{11}` vs `11` round-trip divergence.
                write_node(buf, arena, children[0]);
            } else if row_produces_only_digits(arena, span) {
                // Row produces only digit characters — braces would cause
                // re-parse divergence since digits merge into one Number token.
                write_nodes(buf, arena, span);
            } else {
                buf.push('{');
                write_nodes(buf, arena, span);
                buf.push('}');
            }
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            let open_str = arena.str_val(open);
            let close_str = arena.str_val(close);
            let content_ids = arena.children(content);

            if content_ids.len() == 1
                && let Some((right, display, starred)) = arena.cases_environment(content_ids[0])
                && write_cases_array(buf, arena, content_ids[0], right, display, starred)
            {
                return;
            }

            // A fenced barless fraction: `\binom`, or `\genfrac` with the
            // `\brace`/`\brack` delimiters (both KaTeX and MathJax read
            // these as `\genfrac` delimiters).
            let command = match (open_str, close_str) {
                ("(", ")") => Some("\\binom{"),
                ("{", "}") => Some("\\genfrac\\{\\}{0pt}{}{"),
                ("[", "]") => Some("\\genfrac[]{0pt}{}{"),
                _ => None,
            };
            if let Some(command) = command
                && content_ids.len() == 1
            {
                let child_id = content_ids[0];
                if let CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num,
                    den,
                } = *arena.node(child_id)
                    // A `\middle` belongs to the `\left…\right` pair, which
                    // `\binom` would not write.
                    && !holds_middle(arena, num)
                    && !holds_middle(arena, den)
                {
                    // Like `\frac`, the argument braces already group each
                    // part; `write_node` would brace a multi-node row again.
                    buf.push_str(command);
                    write_wrapped_content(buf, arena, num);
                    buf.push_str("}{");
                    write_wrapped_content(buf, arena, den);
                    buf.push('}');
                    return;
                }
            }

            buf.push_str("\\left");
            if writes_as_delimiter(open_str) {
                write_delim(buf, open_str);
                buf.push(' ');
            } else {
                buf.push_str(". ");
                write_literal_delimiter(buf, open_str, "\\mathopen{");
            }
            let literal_close = !writes_as_delimiter(close_str);
            let mut ends_with_command = false;
            // Output offsets just after each fenced row break; see
            // `write_sequence`.
            let mut row_breaks: smallvec::SmallVec<[usize; 2]> = smallvec::SmallVec::new();
            let mut next = 0;
            while let Some(&child_id) = content_ids.get(next) {
                let index = next;
                next += 1;
                if index > 0 && matches!(*arena.node(content_ids[index - 1]), CompactNode::Newline)
                {
                    row_breaks.push(buf.len());
                }
                match *arena.node(child_id) {
                    CompactNode::Middle(sid) => {
                        let delim = arena.str_val(sid);
                        if writes_as_delimiter(delim) {
                            buf.push_str(" \\middle");
                            write_delim(buf, delim);
                            buf.push(' ');
                        } else {
                            // The null delimiter still divides the fence.
                            buf.push_str(" \\middle. ");
                            write_literal_delimiter(buf, delim, "\\mathord{");
                        }
                        ends_with_command = false;
                    },
                    _ => {
                        // A command delimiter at the end of the previous child
                        // (e.g. an inner \right\rangle) would merge with a
                        // following plain letter. Insert a separator when needed.
                        let output_start = buf.len();
                        if (ends_with_command && node_starts_with_ascii_letter(arena, child_id))
                            || (index > 0
                                && adjacent_number_nodes_need_separator(
                                    arena,
                                    content_ids[index - 1],
                                    child_id,
                                ))
                        {
                            buf.push(' ');
                        }
                        if content_ids[index..]
                            .get(..3)
                            .is_some_and(|ids| is_idotsint_triple(arena, ids))
                        {
                            write_idotsint(buf);
                            next = index + 3;
                        } else if is_bare_large_op(arena, child_id) {
                            // As in a row: it is read with its body.
                            let run = &content_ids[index..];
                            let end = run
                                .iter()
                                .position(|&id| matches!(*arena.node(id), CompactNode::Middle(_)))
                                .unwrap_or(run.len());
                            next = index + write_large_op_chain(buf, arena, &run[..end]);
                            // Before a `\middle` too its body is empty.
                            if next == index + 1
                                && end == 1
                                && (literal_close || next < content_ids.len())
                            {
                                buf.push_str("{}");
                            }
                        } else {
                            write_node(buf, arena, child_id);
                        }
                        // A large operator with an empty body must keep an
                        // explicit empty group before the next fenced item.
                        // The normal row writer adds this boundary, while the
                        // fenced loop writes children directly.
                        if (literal_close || index + 1 < content_ids.len())
                            && ends_with_empty_body_nary(arena, child_id)
                        {
                            buf.push_str("{}");
                        }
                        ends_with_command =
                            appended_ends_with_letter_command(buf, output_start, ends_with_command);
                    },
                }
            }
            for &start in row_breaks.iter().rev() {
                guard_row_break_argument(buf, start);
            }
            if literal_close {
                // A script without a base would become the atom's leading one.
                if ends_with_bare_script(arena, content_ids) {
                    buf.push_str(" \\middle. ");
                }
                write_literal_delimiter(buf, close_str, "\\mathclose{");
                buf.push_str(" \\right.");
            } else {
                buf.push_str(" \\right");
                write_delim(buf, close_str);
            }
        },

        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => {
            if is_empty_script_base(arena, base) {
                write_standalone_empty_base_script(buf, arena, id);
            } else {
                write_script_node(buf, arena, id);
            }
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            let sub_visible = pre_sub.is_some() && !is_effectively_empty_arg(arena, pre_sub);
            let sup_visible = pre_sup.is_some() && !is_effectively_empty_arg(arena, pre_sup);
            if !sub_visible && !sup_visible {
                match *arena.node(base) {
                    CompactNode::Row(span) => write_nodes(buf, arena, span),
                    _ => write_node_slice(buf, arena, &[base], false),
                }
                return;
            }
            // A leading script is written as a group with its base,
            // `{{}^{14}_{6}C}`, so that reading it back cannot attach the
            // scripts to the node before (a function name, a big operator),
            // let `\int` take `{}` as its body, or join the base with the node
            // after. The base follows the scripts directly, so a script is
            // always braced: `{}^{6}6`, not `{}^66`.
            let bare_space_base = is_bare_space_prescript_base(arena, base);
            if bare_space_base {
                // A nonzero math space cannot absorb a script or adjacent
                // token. Avoid an unnecessary outer group, which disappears
                // on reparse and makes the first LaTeX write non-canonical.
                buf.push_str("{}");
            } else {
                buf.push_str("{{}");
            }
            let mut scalar_last = false;
            for (mark, script, visible) in
                [('_', pre_sub, sub_visible), ('^', pre_sup, sup_visible)]
            {
                if visible {
                    buf.push(mark);
                    scalar_last = write_script_arg(buf, arena, script, ScriptArgGroup::Always);
                }
            }
            if scalar_last && node_starts_with_ascii_letter(arena, base) {
                buf.push(' ');
            }
            // An identifier of several letters is read back as one letter
            // each, the scripts on the first only: `{{}^{\prime}v}a`.
            let word = match *arena.node(base) {
                CompactNode::Identifier(sid) => arena
                    .str_val(sid)
                    .char_indices()
                    .nth(1)
                    .map(|(at, _)| arena.str_val(sid).split_at(at)),
                _ => None,
            };
            if let Some((first, rest)) = word {
                write_identifier_text(buf, first);
                if !bare_space_base {
                    buf.push('}');
                }
                write_identifier_text(buf, rest);
                return;
            }
            // The base is written as a sequence of one node, as in a row,
            // so that its own scripts are braced as there.
            match *arena.node(base) {
                CompactNode::Row(span) => write_nodes(buf, arena, span),
                _ => write_node_slice(buf, arena, &[base], false),
            }
            if !bare_space_base {
                buf.push('}');
            }
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            // A decoration stretches over any base, an operator too.
            if movable_limits && write_compound_limits(buf, arena, base, NodeId::NONE, over, true) {
                return;
            }
            let decoration = if is_line_symbol(arena, over) {
                Some("\\overline{")
            } else if is_overbrace(arena, over) {
                Some("\\overbrace{")
            } else {
                None
            };
            if let Some(command) = decoration {
                buf.push_str(command);
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else if !write_compound_limits(buf, arena, base, NodeId::NONE, over, movable_limits) {
                begin_stack(buf, arena, over, AccentPosition::Above);
                write_stack_base(buf, arena, over, base);
                buf.push('}');
            }
        },

        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if movable_limits && write_compound_limits(buf, arena, base, under, NodeId::NONE, true)
            {
                return;
            }
            let decoration = if is_line_symbol(arena, under) {
                Some("\\underline{")
            } else if is_underbrace(arena, under) {
                Some("\\underbrace{")
            } else {
                None
            };
            if let Some(command) = decoration {
                buf.push_str(command);
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else if !write_compound_limits(buf, arena, base, under, NodeId::NONE, movable_limits)
            {
                begin_stack(buf, arena, under, AccentPosition::Below);
                write_stack_base(buf, arena, under, base);
                buf.push('}');
            }
        },

        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            if write_compound_limits(buf, arena, base, under, over, movable_limits) {
                return;
            }
            begin_stack(buf, arena, under, AccentPosition::Below);
            begin_stack(buf, arena, over, AccentPosition::Above);
            write_wrapped_content(buf, arena, base);
            buf.push_str("}}");
        },

        CompactNode::Fraction { kind, num, den } => {
            // A barless fraction is `\genfrac` with a zero rule: unlike the
            // infix `\atop`, it cannot capture the rest of its row.
            let cmd = match kind {
                FractionKind::Normal => "\\frac{",
                FractionKind::Display => "\\dfrac{",
                FractionKind::Inline => "\\tfrac{",
                FractionKind::NoBar => "\\genfrac{}{}{0pt}{}{",
                FractionKind::Skewed => "\\frac{",
            };
            buf.push_str(cmd);
            write_wrapped_content(buf, arena, num);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, den);
            buf.push('}');
        },

        CompactNode::Sqrt(inner) => {
            buf.push_str("\\sqrt{");
            write_wrapped_content(buf, arena, inner);
            buf.push('}');
        },

        CompactNode::Root { index, radicand } => {
            buf.push_str("\\sqrt[");
            let start = buf.len();
            write_wrapped_content(buf, arena, index);
            let argument = &buf[start..];
            // KaTeX strips one pair of braces from a bracketed argument that
            // starts with `{` and ends with `}`, matched or not, so
            // `[{\mathcal{L}}^{10}]` would lose its first and last brace.
            if (argument.starts_with('{') && argument.ends_with('}'))
                || (argument.as_bytes().contains(&b']')
                    && crate::latex::optional_argument_end(argument).is_some())
            {
                // Preserve a literal closing bracket or a nested root's index:
                // at this level TeX would otherwise finish the outer index.
                buf.insert_text(start, "{");
                buf.push('}');
            }
            buf.push_str("]{");
            write_wrapped_content(buf, arena, radicand);
            buf.push('}');
        },

        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let entry = crate::unicode::symbols::unicode_to_latex_entry(op);
            match entry {
                Some((cmd, SymbolKind::LargeOp)) => buf.command(cmd),
                None if is_latex_nary(op) => write_mathop_glyph(buf, op, None),
                _ => {
                    let kind = entry.map(|(_, kind)| kind);
                    write_unnamed_nary(buf, arena, (op, kind), [sub, sup], body, limits);
                    return;
                },
            }
            match limits {
                LimitStyle::AboveBelow => buf.push_str("\\limits"),
                LimitStyle::SubSup => buf.push_str("\\nolimits"),
                LimitStyle::Default => {},
            }
            if !is_effectively_empty_arg(arena, sub) {
                buf.push('_');
                write_script_arg(buf, arena, sub, ScriptArgGroup::Minimal);
            }
            if !is_effectively_empty_arg(arena, sup) {
                buf.push('^');
                write_script_arg(buf, arena, sup, ScriptArgGroup::Minimal);
            }
            // Only write the body if it's non-empty — avoids \prod vs \prod {} divergence
            if !is_empty_body(arena, body) {
                buf.push(' ');
                write_nary_body(buf, arena, body);
            }
        },

        CompactNode::Accent {
            base,
            accent,
            position,
            wide,
        } => {
            // Map (accent char, position, wide) back to its LaTeX command.
            // `None` means there is no dedicated command and the generic
            // overset/underset fallback is used (preserving the glyph).
            let cmd: Option<&str> = match position {
                AccentPosition::Above => match (accent, wide) {
                    ('\u{0302}', true) => Some("widehat"),
                    ('\u{0302}', false) => Some("hat"),
                    ('\u{0304}', _) => Some("bar"),
                    ('\u{20D7}', _) => Some("vec"),
                    ('\u{0307}', _) => Some("dot"),
                    ('\u{0308}', _) => Some("ddot"),
                    ('\u{20DB}', _) => Some("dddot"),
                    ('\u{20DC}', _) => Some("ddddot"),
                    ('\u{0303}', true) => Some("widetilde"),
                    ('\u{0303}', false) => Some("tilde"),
                    ('\u{0306}', _) => Some("breve"),
                    ('\u{030C}', true) => Some("widecheck"),
                    ('\u{030C}', false) => Some("check"),
                    ('\u{0301}', _) => Some("acute"),
                    ('\u{0300}', _) => Some("grave"),
                    ('\u{030A}', _) => Some("mathring"),
                    ('\u{0305}', _) => Some("overline"),
                    ('\u{20D0}', _) => Some("overleftharpoon"),
                    ('\u{20D1}', _) => Some("overrightharpoon"),
                    ('\u{20D6}', _) => Some("overleftarrow"),
                    ('\u{20E1}', _) => Some("overleftrightarrow"),
                    ('\u{2192}', _) => Some("overrightarrow"),
                    ('\u{2190}', _) => Some("overleftarrow"),
                    ('\u{2194}', _) => Some("overleftrightarrow"),
                    ('\u{21C0}', _) => Some("overrightharpoon"),
                    ('\u{21BC}', _) => Some("overleftharpoon"),
                    ('\u{21D2}', _) => Some("Overrightarrow"),
                    ('\u{23DC}', _) => Some("overgroup"),
                    ('\u{23DE}', _) => Some("overbrace"),
                    ('\u{23B4}', true) => Some("overbracket"),
                    ('\u{23B4}', false) => Some("overlinesegment"),
                    _ => None,
                },
                AccentPosition::Below => match accent {
                    '\u{2192}' => Some("underrightarrow"),
                    '\u{2190}' => Some("underleftarrow"),
                    '\u{2194}' => Some("underleftrightarrow"),
                    '\u{0330}' => Some("utilde"),
                    '\u{0331}' | '\u{0332}' => Some("underline"),
                    '\u{20EE}' => Some("underleftarrow"),
                    '\u{20EF}' => Some("underrightarrow"),
                    '\u{23DD}' => Some("undergroup"),
                    '\u{23DF}' => Some("underbrace"),
                    '\u{23B5}' if wide => Some("underbracket"),
                    '\u{23B5}' => Some("underlinesegment"),
                    _ => None,
                },
            };
            if let Some(cmd) = cmd {
                buf.command(cmd);
                buf.push('{');
                write_accent_base_content(buf, arena, base);
                buf.push('}');
            } else {
                // No dedicated command: preserve the accent glyph faithfully
                // via overset/underset rather than silently emitting \hat.
                buf.push_str(match position {
                    AccentPosition::Above => "\\overset{",
                    AccentPosition::Below => "\\underset{",
                });
                // A combining mark would join the brace before it.
                if is_joining_mark(accent) {
                    write_char_code(buf, accent, " ");
                } else {
                    write_char(buf, accent, Place::Accent);
                }
                buf.push_str("}{");
                write_accent_base_content(buf, arena, base);
                buf.push('}');
            }
        },

        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            write_node(buf, arena, content)
        },
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            add_jot,
            arraystretch_pct,
            array_style,
            col_seps,
            row_seps,
        } => {
            let columns = arena.children(col_aligns);
            let cell_ids = arena.children(cells);
            let dashes = arena.rule_dashes(id);
            let nc = usize::from(num_cols.max(1));
            let authored_rows = arena
                .array_environment(id)
                .is_none()
                .then(|| {
                    arena.table_metadata(id).and_then(|metadata| {
                        (!metadata.row_lengths.is_empty()).then_some(metadata.row_lengths.as_ref())
                    })
                })
                .flatten();
            let authored_stretch = (buf.latex_profile == LatexProfile::PdfLaTeX)
                .then(|| {
                    arena
                        .authored_arraystretch(id)
                        .filter(|value| (value - 1.0).abs() > f64::EPSILON)
                })
                .flatten();
            // Preserve the display-style alignment semantics when this table
            // came from an AMS environment. An ordinary array or an isolated
            // cell declaration cannot establish that style for the table.
            let ams = add_jot
                && !cell_ids.is_empty()
                && cell_ids
                    .iter()
                    .all(|cell| super::cell_after_style(arena, cell, 0).is_some());
            // Public arena callers can edit a parsed table. Trust its source
            // environment only while the current columns and spacing fit it.
            let matrix = arena
                .valid_matrix_environment(
                    id,
                    col_aligns,
                    cells,
                    num_cols,
                    arraystretch_pct,
                    add_jot,
                    array_style,
                    col_seps,
                    row_seps,
                )
                .map(|(source, align)| match (source, align) {
                    (MatrixKind::Matrix, ColumnAlign::Center) => ("matrix", ""),
                    (MatrixKind::Matrix, ColumnAlign::Left) => ("matrix*", "[l]"),
                    (MatrixKind::Matrix, ColumnAlign::Right) => ("matrix*", "[r]"),
                    (MatrixKind::SmallMatrix, ColumnAlign::Center) => ("smallmatrix", ""),
                    (MatrixKind::SmallMatrix, ColumnAlign::Left) => ("smallmatrix*", "[l]"),
                    (MatrixKind::SmallMatrix, ColumnAlign::Right) => ("smallmatrix*", "[r]"),
                });
            let (env, option) = if let Some(matrix) = matrix {
                matrix
            } else if ams
                && nc == 1
                && matches!(columns, [id] if matches!(*arena.node(*id), CompactNode::ColAlign(ColumnAlign::Center)))
            {
                ("gathered", "")
            } else if ams
                && nc >= 2
                && columns.len() == nc
                && columns.iter().enumerate().all(|(i, &id)| {
                    let expected = if i % 2 == 0 {
                        ColumnAlign::Right
                    } else {
                        ColumnAlign::Left
                    };
                    matches!(*arena.node(id), CompactNode::ColAlign(col) if col == expected)
                })
            {
                ("aligned", "")
            } else {
                ("array", "")
            };
            if let Some(stretch) = authored_stretch {
                buf.push_str("{\\renewcommand{\\arraystretch}{");
                buf.push_str(&stretch.to_string());
                buf.push_str("}");
            }
            buf.push_str(r"\begin{");
            buf.push_str(env);
            buf.push('}');
            buf.push_str(option);
            let trim_edges = env == "array"
                && !array_style
                && (arena.is_native_array(id)
                    || col_seps != 0
                    || row_seps != 0
                    || cell_ids.iter().any(|&cell| {
                        matches!(
                            arena.node(cell),
                            CompactNode::AlignedCell { .. } | CompactNode::SpannedCell { .. }
                        )
                    }));
            if env == "array" {
                buf.push('{');
                if trim_edges {
                    buf.push_str("@{}");
                }
                for (index, &id) in columns.iter().enumerate() {
                    if index < 32 && (col_seps & (1 << index)) != 0 {
                        buf.push(column_rule(dashes, index));
                    }
                    if let CompactNode::ColAlign(col) = *arena.node(id) {
                        buf.push(match col {
                            ColumnAlign::Left => 'l',
                            ColumnAlign::Center => 'c',
                            ColumnAlign::Right => 'r',
                        });
                    }
                }
                if columns.len() < 32 && (col_seps & (1 << columns.len())) != 0 {
                    buf.push(column_rule(dashes, columns.len()));
                }
                if trim_edges {
                    buf.push_str("@{}");
                }
                buf.push('}');
            }
            buf.push(' ');
            let num_rows = cell_ids.len().div_ceil(nc);
            for (row_idx, row) in cell_ids.chunks(nc).enumerate() {
                let row_start = if row_idx == 0 {
                    if row_seps & 1 != 0 {
                        buf.push_str(row_rule(dashes, 0));
                    }
                    None
                } else {
                    buf.push_str(" \\\\ ");
                    // `\hline` already separates the break from the row.
                    let start = buf.len();
                    if row_idx < 32 && (row_seps & (1 << row_idx)) != 0 {
                        buf.push_str(row_rule(dashes, row_idx));
                    }
                    Some(start)
                };
                let cells_start = buf.len();
                let mut col_idx = 0;
                let authored_target = authored_rows
                    .and_then(|rows| rows.get(row_idx))
                    .copied()
                    .map_or(usize::MAX, usize::from);
                let mut authored_count = 0usize;
                let mut emitted = false;
                while col_idx < row.len() && authored_count < authored_target {
                    let cell_id = &row[col_idx];
                    if emitted {
                        buf.push_str(" & ");
                    }
                    emitted = true;
                    let (alignment, nodes) = arena.table_cell_parts(cell_id);
                    let (cell_span, left_boundaries, right_boundaries) = match *arena.node(*cell_id)
                    {
                        CompactNode::SpannedCell {
                            span,
                            left_boundaries,
                            right_boundaries,
                            ..
                        } => (
                            arena.effective_cell_span_in_row(row, col_idx, span),
                            left_boundaries,
                            right_boundaries,
                        ),
                        _ => (1, StrId::EMPTY, StrId::EMPTY),
                    };
                    if let Some(alignment) = alignment {
                        buf.push_str(r"\multicolumn{");
                        buf.push_str(&cell_span.to_string());
                        buf.push_str("}{");
                        write_boundaries(buf, arena.str_val(left_boundaries));
                        if trim_edges && col_idx == 0 && arena.str_val(left_boundaries).is_empty() {
                            buf.push_str("@{}");
                        }
                        buf.push(match alignment {
                            ColumnAlign::Left => 'l',
                            ColumnAlign::Center => 'c',
                            ColumnAlign::Right => 'r',
                        });
                        write_boundaries(buf, arena.str_val(right_boundaries));
                        if trim_edges
                            && col_idx + cell_span == nc
                            && arena.str_val(right_boundaries).is_empty()
                        {
                            buf.push_str("@{}");
                        }
                        buf.push_str("}{");
                    }
                    let marker = match env {
                        "smallmatrix" | "smallmatrix*" => Some(2),
                        "aligned" | "gathered" => Some(0),
                        _ => None,
                    };
                    let nodes = marker
                        .and_then(|value| super::cell_after_style(arena, cell_id, value))
                        .unwrap_or(nodes);
                    if alignment.is_some() {
                        write_aligned_cell_content(buf, arena, nodes);
                        buf.push('}');
                    } else {
                        write_node_slice(buf, arena, nodes, false);
                    }
                    col_idx = col_idx.saturating_add(cell_span);
                    authored_count += 1;
                }
                if authored_rows.is_none() {
                    for _ in row.len()..nc {
                        buf.push_str(" & ");
                    }
                }
                if let Some(start) = row_start {
                    guard_row_break_argument(buf, start);
                }
                // Preserve a real final empty row with an explicit group. A
                // bare terminal `\\` is interpreted as a separator by the
                // reader, so writer-generated empty rows need visible content
                // to survive a write/read cycle. A bottom rule already forces
                // the preceding separator to introduce a row and needs no
                // extra marker.
                if row_idx > 0
                    && row_idx + 1 == num_rows
                    && !(num_rows < 32 && row_seps & (1 << num_rows) != 0)
                    && buf[cells_start..].trim().is_empty()
                {
                    buf.truncate_text(cells_start);
                    buf.push_str("{}");
                }
            }
            if num_rows < 32 && (row_seps & (1 << num_rows)) != 0 {
                buf.push_str(r" \\ ");
                buf.push_str(row_rule(dashes, num_rows).trim_end());
            }
            buf.push_str(r" \end{");
            buf.push_str(env);
            buf.push('}');
            if authored_stretch.is_some() {
                buf.push('}');
            }
        },

        CompactNode::Phantom(inner, kind) => {
            match kind {
                PhantomKind::Full => buf.push_str("\\phantom{"),
                PhantomKind::Vertical => buf.push_str("\\vphantom{"),
                PhantomKind::Horizontal => buf.push_str("\\hphantom{"),
                PhantomKind::SmashBoth => buf.push_str("\\smash{"),
                PhantomKind::SmashBottom => buf.push_str("\\smash[b]{"),
                PhantomKind::SmashTop => buf.push_str("\\smash[t]{"),
            }
            write_wrapped_content(buf, arena, inner);
            buf.push('}');
        },

        CompactNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            // A spec that would not read back as the same braced argument
            // is dropped, so the box keeps whichever color still reads back.
            let portable = buf.latex_profile == LatexProfile::PdfLaTeX;
            let [bg_spec, border_spec] = [bg_color, border_color].map(|spec| {
                let spec = arena.str_val(spec);
                let color = if portable {
                    colors::box_color(spec)
                } else {
                    colors::legacy_box_color(spec)
                };
                if color.is_none() {
                    buf.report.color(spec);
                }
                color
            });
            if let (Some((bg_model, bg)), Some((border_model, border))) =
                (bg_spec.as_ref(), border_spec.as_ref())
            {
                // xcolor accepts a second optional model after the frame
                // color. Use it for a hexadecimal fill next to a named frame,
                // or the `named` model for a literal named fill next to a
                // hexadecimal frame. A mix expression has no such model, so
                // define the hexadecimal side in a local group instead.
                if !bg_model.is_empty() || !border_model.is_empty() {
                    let bg_model = if border_model.is_empty() {
                        *bg_model
                    } else if bg_model.is_empty() && colors::is_named(bg) {
                        "[named]"
                    } else {
                        *bg_model
                    };
                    if border_model.is_empty() || !bg_model.is_empty() || colors::is_named(bg) {
                        buf.push_str("\\fcolorbox");
                        buf.push_str(border_model);
                        buf.push('{');
                        buf.push_str(border);
                        buf.push('}');
                        buf.push_str(bg_model);
                        buf.push('{');
                        buf.push_str(bg);
                        buf.push_str("}{");
                        write_box_content(buf, arena, inner);
                        buf.push('}');
                    } else {
                        buf.push('{');
                        let bg_name = if !bg_model.is_empty() {
                            let name = format!("mathemaHex{bg}");
                            buf.push_str("\\definecolor{");
                            buf.push_str(&name);
                            buf.push_str("}{HTML}{");
                            buf.push_str(bg);
                            buf.push('}');
                            name
                        } else {
                            bg.clone()
                        };
                        let border_name = if !border_model.is_empty() {
                            let name = format!("mathemaHex{border}");
                            buf.push_str("\\definecolor{");
                            buf.push_str(&name);
                            buf.push_str("}{HTML}{");
                            buf.push_str(border);
                            buf.push('}');
                            name
                        } else {
                            border.clone()
                        };
                        buf.push_str("\\fcolorbox{");
                        buf.push_str(&border_name);
                        buf.push_str("}{");
                        buf.push_str(&bg_name);
                        buf.push_str("}{");
                        write_box_content(buf, arena, inner);
                        buf.push_str("}}");
                    }
                } else {
                    // \fcolorbox{border}{bg}{content}
                    buf.push_str("\\fcolorbox{");
                    buf.push_str(border);
                    buf.push_str("}{");
                    buf.push_str(bg);
                    buf.push_str("}{");
                    write_box_content(buf, arena, inner);
                    buf.push('}');
                }
            } else if let Some((model, bg)) = bg_spec.as_ref() {
                // \colorbox{bg}{content}; hexadecimal values carry the
                // model explicitly because `#` is a TeX parameter marker.
                buf.push_str("\\colorbox");
                buf.push_str(model);
                buf.push('{');
                buf.push_str(bg);
                buf.push_str("}{");
                write_box_content(buf, arena, inner);
                buf.push('}');
            } else {
                buf.push_str("\\boxed{");
                write_wrapped_content(buf, arena, inner);
                buf.push('}');
            }
        },
        CompactNode::Actuarial { content: inner, .. } => {
            buf.push_str("\\angl{");
            write_hbox_content(buf, arena, inner);
            buf.push('}');
        },

        CompactNode::Reflect { content, text_mode } => {
            if text_mode {
                buf.push_str(r"\reflectbox{");
                write_hbox_content(buf, arena, content);
            } else {
                buf.push_str(r"\mathreflectbox{");
                write_box_math(buf, arena, &[content]);
            }
            buf.push('}');
        },

        CompactNode::Cancel { stroke, content } => {
            let cmd = match stroke {
                StrokeKind::Forward => "cancel",
                StrokeKind::Backward => "bcancel",
                StrokeKind::Cross => "xcancel",
                StrokeKind::Horizontal | StrokeKind::Vertical => "cancel",
            };
            buf.command(cmd);
            buf.push('{');
            write_wrapped_content(buf, arena, content);
            buf.push('}');
        },

        CompactNode::Scaled { factor, content } => {
            // Sizing commands only apply to delimiter symbols. If the parsed
            // content is not an Open/Close/Fence symbol, the original input was
            // invalid (e.g. \Big\mathrel{...}); emit the content alone so the
            // sizing command does not detach from its argument on re-parse.
            if let Some(cmd) = big_delim_command(arena, factor, content) {
                buf.command(cmd);
                write_big_delim(buf, arena, content);
            } else {
                write_node(buf, arena, content);
            }
        },
        CompactNode::RaiseBox {
            shift,
            height,
            depth,
            extents,
            content,
        } => {
            use std::fmt::Write as _;
            buf.push_str("\\raisebox{");
            let _ = write!(buf, "{}em", shift.as_f64());
            buf.push('}');
            if extents & 1 != 0 {
                buf.push('[');
                let _ = write!(buf, "{}em", height.as_f64());
                buf.push(']');
            }
            if extents & 2 != 0 {
                buf.push('[');
                let _ = write!(buf, "{}em", depth.as_f64());
                buf.push(']');
            }
            buf.push('{');
            write_wrapped_content(buf, arena, content);
            buf.push('}');
        },

        CompactNode::Styled(style, span) => {
            let cmd = style_to_latex_font(style);
            buf.command(cmd);
            buf.push('{');
            write_nodes(buf, arena, span);
            buf.push('}');
        },

        CompactNode::Color { color, content } => {
            // `with_ordinary_atoms` left only colors that can be written.
            let (model, spec) = colors::text_color(arena.str_val(color));
            buf.push_str(r"\textcolor");
            buf.push_str(model);
            buf.push('{');
            buf.push_str(spec);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, content);
            buf.push('}');
        },

        // Helper nodes — should not appear as standalone roots, but handle gracefully
        CompactNode::Middle(sid) => {
            // A middle delimiter can survive inside an infix-fraction
            // argument. Preserve its node class when emitted outside the
            // immediate `\left...\right` writer loop.
            let delim = arena.str_val(sid);
            if writes_as_nested_middle(delim) {
                buf.push_str("\\middle");
                write_delim(buf, delim);
            } else {
                buf.push_str("\\middle.");
                write_literal_delimiter(buf, delim, "\\mathord{");
            }
        },
        CompactNode::ColAlign(_) => {},
        CompactNode::Rule {
            width,
            height,
            lift,
        } => {
            use std::fmt::Write as _;
            buf.push_str("\\rule");
            if lift.as_f64() != 0.0 {
                buf.push('[');
                let _ = write!(buf, "{}em", lift.as_f64());
                buf.push(']');
            }
            buf.push('{');
            let _ = write!(buf, "{}em", width.as_f64());
            buf.push_str("}{");
            let _ = write!(buf, "{}em", height.as_f64());
            buf.push('}');
        },
        CompactNode::StyleSwitch(s) => match s {
            0 => buf.push_str("\\displaystyle "),
            1 => buf.push_str("\\textstyle "),
            2 => buf.push_str("\\scriptstyle "),
            3 => buf.push_str("\\scriptscriptstyle "),
            _ if crate::ast::style::MathStyle::marker_is_display(s) => {
                buf.push_str("\\displaystyle ")
            },
            _ => buf.push_str("\\scriptscriptstyle "),
        },
        CompactNode::SizeScale(idx) => {
            let cmd = match idx {
                0 => "\\tiny ",
                1 => "\\scriptsize ",
                2 => "\\footnotesize ",
                3 => "\\small ",
                4 => "\\normalsize ",
                5 => "\\large ",
                6 => "\\Large ",
                7 => "\\LARGE ",
                8 => "\\huge ",
                _ => "\\Huge ",
            };
            buf.push_str(cmd);
        },
        CompactNode::Newline => buf.push_str("\\\\ "),
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            buf.push_str("\\mathchoice{");
            write_wrapped_content(buf, arena, display);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, text);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, script);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, script_script);
            buf.push('}');
        },
        CompactNode::MathAtom { atom_type, content } => {
            if atom_type == MathAtomType::Ord
                && let Some(text) = authored_math_space_symbol(arena, content)
            {
                write_char_code(buf, text, " ");
                return;
            }
            if atom_type == MathAtomType::Op {
                write_math_atom_op(buf, arena, content);
                return;
            }
            // `\bigl`, `\bigr` and `\bigm` are TeX's `\mathopen`, `\mathclose`
            // and `\mathrel` around `\big`, so spell the pair as one command.
            if let Some(suffix) = big_delim_suffix(atom_type)
                && let CompactNode::Scaled {
                    factor,
                    content: delim,
                } = *arena.node(content)
                && let Some(cmd) = big_delim_command(arena, factor, delim)
            {
                buf.command_with_suffix(cmd, suffix);
                write_big_delim(buf, arena, delim);
                return;
            }
            let cmd = match atom_type {
                MathAtomType::Ord => "\\mathord",
                MathAtomType::Op => unreachable!(),
                MathAtomType::Bin => "\\mathbin",
                MathAtomType::Rel => "\\mathrel",
                MathAtomType::Open => "\\mathopen",
                MathAtomType::Close => "\\mathclose",
                MathAtomType::Punct => "\\mathpunct",
                MathAtomType::Inner => "\\mathinner",
            };
            buf.push_str(cmd);
            buf.push('{');
            write_atom_content(buf, arena, content);
            buf.push('}');
        },
        CompactNode::StructuredOperator { content, form } => {
            write_structured_operator(buf, arena, content, form)
        },
        CompactNode::UndefinedCommand { name, arguments } => {
            let name = arena.str_val(name);
            let groups = arena.children(arguments);
            // By default, and always for a name that is no undefined
            // control word or is a dangerous primitive, the command is
            // visible inert text and its groups literal braces (see
            // `ast::undefined`). The atom keeps the text from merging with
            // neighbouring text on the next read.
            if !buf.keep_undefined || !crate::ast::undefined::writes_as_command(name) {
                write_escaped_undefined_command(buf, arena, name, groups);
                return;
            }
            // A command read only in its own context (`\end`, `\right`) is
            // braced with its groups so it is not read in such a context.
            let braced = crate::ast::undefined::is_context_command(name);
            if braced {
                buf.push('{');
            }
            buf.push_str(name);
            for &group in groups {
                write_undefined_command_group(buf, arena, group);
            }
            if braced {
                buf.push('}');
            } else {
                buf.end_undefined_command();
            }
        },
    }
}

fn write_structured_operator(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    content: NodeId,
    form: OperatorNameForm,
) {
    buf.push_str(if form.is_starred() {
        r"\operatorname*{"
    } else {
        r"\operatorname{"
    });
    let start = buf.len();
    let previous = std::mem::replace(&mut buf.in_operator_name, true);
    write_atom_content(buf, arena, content);
    buf.in_operator_name = previous;
    separate_leading_mark(buf, start);
    // A constructed structured name may contain only plain letters. Keep a
    // TeX atom boundary so another import can recover its form and placement.
    if !crate::latex::parse::operator_name_needs_math(&buf[start..]) {
        buf.insert_text(start, r"\mathord{");
        buf.push('}');
    }
    buf.push('}');
    if let OperatorNameForm::Starred(style) = form {
        match style {
            LimitStyle::AboveBelow => buf.push_str(r"\limits"),
            LimitStyle::SubSup => buf.push_str(r"\nolimits"),
            LimitStyle::Default => {},
        }
    }
}

#[inline]
fn write_identifier(buf: &mut LatexBuffer, arena: &AstArena, sid: compact::StrId) {
    let s = arena.str_val(sid);
    if write_leaf_with_math_spaces(buf, s, MathLeafKind::Identifier) {
        return;
    }
    write_identifier_text(buf, s);
}

fn write_aligned_cell_content(buf: &mut LatexBuffer, arena: &AstArena, nodes: &[NodeId]) {
    // An identifier of several letters is one upright identifier (a MathML
    // `<mi>`), whatever else the cell contains: letters of TeX are nodes of
    // their own.
    let writes_upright_text = |id: &NodeId| match *arena.node(*id) {
        CompactNode::Identifier(sid) => arena.str_val(sid).chars().nth(1).is_some(),
        CompactNode::Text(TextStyle::Normal, _) => true,
        _ => false,
    };
    let mut start = 0;
    for (index, &id) in nodes.iter().enumerate() {
        let CompactNode::Identifier(sid) = *arena.node(id) else {
            continue;
        };
        let name = arena.str_val(sid);
        if name.chars().nth(1).is_none() {
            continue;
        }
        if start < index {
            write_node_slice(buf, arena, &nodes[start..index], false);
        }
        // Adjacent text commands coalesce on reimport. Keep a distinct
        // ordinary math atom for each identifier at such a boundary.
        let separate = nodes[..index].last().is_some_and(writes_upright_text)
            || nodes.get(index + 1).is_some_and(writes_upright_text);
        if separate {
            buf.push_str(r"\mathord{");
        }
        write_styled_text(buf, TextStyle::Normal, name);
        if separate {
            buf.push('}');
        }
        start = index + 1;
    }
    if start < nodes.len() {
        write_node_slice(buf, arena, &nodes[start..], false);
    }
}

fn write_identifier_text(buf: &mut LatexBuffer, s: &str) {
    if !s.chars().any(|ch| unicode_to_latex(ch).is_some()) {
        for ch in s.chars() {
            if needs_escape(ch, Place::Identifier) {
                write_char(buf, ch, Place::Identifier);
            } else {
                buf.push(ch);
            }
        }
        return;
    }

    // Translate every mapped character in a mixed identifier. A command word
    // consumes following ASCII letters, so insert an ignored source space
    // before an unmapped ASCII word character; another backslash already
    // terminates a preceding command naturally.
    let mut previous_was_command = false;
    for ch in s.chars() {
        if needs_escape(ch, Place::Identifier) {
            write_char(buf, ch, Place::Identifier);
            previous_was_command = false;
        } else if let Some(cmd) = unicode_to_latex(ch) {
            buf.command(cmd);
            previous_was_command = true;
        } else {
            if previous_was_command && ch.is_ascii_alphabetic() {
                buf.push(' ');
            }
            buf.push(ch);
            previous_was_command = false;
        }
    }
}

/// The native command that reproduces this exact operator name.
fn latex_operator_command(name: &str) -> Option<&str> {
    match name {
        "arccos" | "arcsin" | "arctan" | "arg" | "cos" | "cosh" | "cot" | "coth" | "csc"
        | "deg" | "det" | "dim" | "exp" | "gcd" | "hom" | "inf" | "ker" | "lg" | "lim" | "ln"
        | "log" | "max" | "min" | "Pr" | "sec" | "sin" | "sinh" | "sup" | "tan" | "tanh" => {
            Some(name)
        },
        _ => crate::unicode::symbols::compound_limit_command(name),
    }
}

/// Whether `text` may hold a control character (U+0000–1F, 7F, 80–9F).
#[inline]
fn has_control(text: &str) -> bool {
    text.bytes().any(|b| b < b' ' || b == 0x7F || b == 0xC2) && text.contains(char::is_control)
}

/// Whether a function name is written as its command.
#[inline]
fn is_operator_command_name(name: &str) -> bool {
    latex_operator_command(name).is_some()
        || (has_control(name) && latex_operator_command(&kept_name(name)).is_some())
}

/// The characters of a function name that are written: control characters are
/// not, or as a space.
fn kept_name(name: &str) -> Cow<'_, str> {
    if !has_control(name) {
        return Cow::Borrowed(name);
    }
    let mut kept = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_control() {
            if let Spelling::Fixed(text) = escape::spelling(ch, Place::Name) {
                kept.push_str(text);
            }
        } else {
            kept.push(ch);
        }
    }
    Cow::Owned(kept)
}

/// A function name: its LaTeX command when one exists, else
/// `\operatorname{…}`, or `\operatorname*{…}` for one whose scripts are
/// limits (`argmax`).
fn write_operator_name(buf: &mut LatexBuffer, name: &str, limits: LimitStyle) {
    // Without its control characters the name may be a command's.
    let plain = name
        .bytes()
        .all(|b| b.is_ascii() && !ascii_needs_escape(b, Place::Name));
    if !plain && has_control(name) {
        let kept = kept_name(name);
        name.chars()
            .filter(|&ch| escape::spelling(ch, Place::Name) == Spelling::Dropped)
            .for_each(|ch| buf.report.char(ch));
        write_operator_name(buf, &kept, limits);
        return;
    }
    if let Some(command) = latex_operator_command(name)
        && !(limits == LimitStyle::Default && !crate::unicode::symbols::operator_has_limits(name))
    {
        buf.command(command);
        match limits {
            LimitStyle::AboveBelow => buf.push_str("\\limits"),
            // A native limit command needs an explicit modifier to force
            // side scripts; ordinary native functions already have that
            // placement by default.
            LimitStyle::SubSup if crate::unicode::symbols::operator_has_limits(name) => {
                buf.push_str("\\nolimits");
            },
            LimitStyle::Default | LimitStyle::SubSup => {},
        }
    } else {
        match limits {
            LimitStyle::AboveBelow => {
                // Unstarred `\operatorname` ignores placement modifiers in
                // KaTeX/amsopn. Use the starred form when the AST requests
                // fixed above/below placement, then make that choice explicit.
                buf.push_str("\\operatorname*{");
                write_operator_name_content(buf, name, plain);
                buf.push_str("}\\limits");
            },
            LimitStyle::SubSup => {
                buf.push_str("\\operatorname{");
                write_operator_name_content(buf, name, plain);
                buf.push('}');
            },
            LimitStyle::Default => {
                buf.push_str("\\operatorname*{");
                write_operator_name_content(buf, name, plain);
                buf.push('}');
            },
        }
    }
}

/// A combining mark the lexer joins to the character before it.
fn is_joining_mark(ch: char) -> bool {
    matches!(ch, '\u{0300}'..='\u{036F}')
}

/// Whether `text` ends with a brace that is not part of a control symbol.
fn ends_with_brace(text: &str) -> bool {
    ends_with_token(text, &['{', '}'])
}

/// Whether `text` ends with one of `tokens` that is not part of a control
/// symbol.
fn ends_with_token(text: &str, tokens: &[char]) -> bool {
    text.strip_suffix(tokens).is_some_and(|before| {
        let backslashes = before.len() - before.trim_end_matches('\\').len();
        backslashes.is_multiple_of(2)
    })
}

/// Write a combining mark at the start of a function name, at `start`, by its
/// code: as itself it would join the brace before it.
fn separate_leading_mark(buf: &mut LatexBuffer, start: usize) {
    let Some(mark) = buf[start..]
        .chars()
        .next()
        .filter(|&ch| is_joining_mark(ch))
    else {
        return;
    };
    buf.replace_char(start, &escape::char_code(mark, " "));
}

fn write_operator_name_content(buf: &mut LatexBuffer, name: &str, plain: bool) {
    if plain {
        buf.push_str(name);
        return;
    }
    // A character token makes the reader take the name as a math list.
    if name.starts_with(is_joining_mark) || name.contains(['\\', '^', '~']) {
        let start = buf.len();
        write_operator_name_list(buf, name);
        separate_leading_mark(buf, start);
        return;
    }
    for ch in name.chars() {
        if !write_char(buf, ch, Place::Name) {
            buf.report.char(ch);
        }
    }
}

/// A function name as the math list it reads back as: an ordinary atom for
/// each character, and no white space.
fn write_operator_name_list(buf: &mut LatexBuffer, name: &str) {
    let outer = std::mem::replace(&mut buf.in_operator_name, true);
    let start = buf.len();
    // White space is nothing in a math list; without it runs of letters meet.
    let name: String = name
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let mut rest = name.as_str();
    while let Some(ch) = rest.chars().next() {
        let letters = rest
            .find(|ch: char| {
                !ch.is_alphabetic()
                    || !matches!(mapped_symbol_kind(ch), None | Some(SymbolKind::Alphabetic))
            })
            .unwrap_or(rest.len());
        let (run, after) = rest.split_at(if letters > 0 { letters } else { ch.len_utf8() });
        rest = after;
        if letters > 0 {
            if ch.is_ascii() && appended_ends_with_letter_command(buf, start, false) {
                buf.push(' ');
            }
            write_identifier_text(buf, run);
        } else if let Some(width) = crate::unicode::math_space_width(ch) {
            write_space(buf, width);
        } else if ch.is_ascii_digit() {
            buf.push(ch);
        } else {
            write_symbol_text(buf, SymbolKind::Ordinary, run);
        }
    }
    buf.in_operator_name = outer;
}

fn write_symbol(buf: &mut LatexBuffer, arena: &AstArena, kind: SymbolKind, sid: compact::StrId) {
    let s = arena.str_val(sid);
    if write_leaf_with_math_spaces(buf, s, MathLeafKind::Symbol(kind)) {
        return;
    }
    write_symbol_text(buf, kind, s);
}

fn write_symbol_text(buf: &mut LatexBuffer, kind: SymbolKind, s: &str) {
    // The reader knows a large operator by its command only; any other glyph
    // needs an explicit operator atom.
    if kind == SymbolKind::LargeOp
        && let Some(ch) = single_char(s)
        && nary_op_to_latex(ch).is_none()
    {
        write_mathop_glyph(buf, ch, mapped_symbol_kind(ch));
        return;
    }
    if kind == SymbolKind::Ordinary && buf.in_operator_name {
        match s {
            "'" | "-" | "*" => {
                buf.push_str(s);
                return;
            },
            "−" => {
                write_char_token(buf, '−');
                return;
            },
            _ => {},
        }
    }
    // A colon is a relation unless braced.
    if kind == SymbolKind::Ordinary && s == ":" && !buf.in_operator_name {
        buf.push_str("{:}");
        return;
    }
    if kind == SymbolKind::Ordinary
        && let [byte] = s.as_bytes()
        && needs_escape(char::from(*byte), Place::Ordinary)
    {
        write_char(buf, char::from(*byte), Place::Ordinary);
        return;
    }
    write_symbol_spelling(buf, kind, s);
}

fn write_symbol_spelling(buf: &mut LatexBuffer, kind: SymbolKind, s: &str) {
    // A null `\middle.` below the fence's own list.
    if kind == SymbolKind::Fence && s.is_empty() {
        buf.push_str("\\mathord{}");
        return;
    }
    let ch = s.chars().next().unwrap_or('?');
    let is_compound = s.len() > ch.len_utf8();
    // Invisible operators (U+2061 FUNCTION APPLICATION .. U+2064 INVISIBLE
    // PLUS) have no glyph or width in LaTeX. Emit an empty ordinary atom
    // rather than a literal space: source whitespace is discarded by the
    // parser, so `A\u{2062}B` would otherwise become `AB` on a second
    // parse/write pass. `\\mathord{}` is zero width and keeps the original
    // ordinary-token boundary, including when the operator is leading or
    // trailing.
    if matches!(s, "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}") {
        buf.push_str("\\mathord{}");
        return;
    }
    // Hot path: a single non-ASCII relation, operator or delimiter whose
    // canonical command has its class (`\leq`, `\cdot`, `\langle`). None of
    // the special spellings below apply to such a glyph: they concern
    // compounds, ASCII, ordinary atoms, the minus sign and the dagger pair.
    // ASCII glyphs and the math minus never reach the table spelling (their
    // branches below return first), so skip the lookup for the most common
    // symbols (`+ = ( ) −`).
    let canonical = if ch.is_ascii() || ch == '\u{2212}' {
        None
    } else {
        crate::unicode::symbols::unicode_to_latex_entry(ch)
    };
    if !is_compound
        && !ch.is_ascii()
        && kind != SymbolKind::Ordinary
        && let Some((name, class)) = canonical
        && class == kind
        && !matches!(ch, '\u{2020}' | '\u{2021}')
    {
        buf.command(name);
        return;
    }
    // A compound glyph contributed by a package (`\colonapprox` is `∶≈`)
    // reparses to the same single symbol only through its command name.
    if is_compound && let Some(cmd) = crate::latex::packages::package_command_for_symbol(s, kind) {
        buf.command(cmd);
        return;
    }
    // A scalar large-operator glyph represented as an ordinary symbol can
    // come from a middle delimiter or an imported/direct AST. Emit an
    // explicit ordinary atom so the LaTeX command cannot be reparsed as an
    // n-ary operator that absorbs the following element as its body.
    if kind == SymbolKind::Ordinary
        && !is_compound
        && let Some(cmd) = nary_op_to_latex(ch)
    {
        buf.push_str("\\mathord{");
        buf.command(cmd);
        buf.push('}');
        return;
    }
    // A word such as `<mo>lim</mo>` is a function name: `\lim` when LaTeX
    // has the command, else `\operatorname{…}` (writing only its first
    // letter would lose the rest).
    let first_is_ascii_letter = ch.is_ascii_alphabetic();
    let has_operator_name_escape = s.chars().any(|ch| {
        matches!(
            ch,
            '%' | '#' | '$' | '&' | '_' | '^' | '{' | '}' | '~' | '\\'
        )
    });
    if s.chars().count() > 1
        && (s.chars().all(char::is_alphabetic)
            || (kind == SymbolKind::Ordinary
                && (first_is_ascii_letter || has_operator_name_escape)))
    {
        // A generic MathML `<mo>` does not carry TeX's starred
        // `operatorname` distinction.  Preserve the historical unstarred
        // spelling for ordinary/custom names, while native limit commands
        // retain their established defaults (`<mo>lim</mo>` -> `\lim`).
        let limits = if crate::unicode::symbols::operator_has_limits(s) {
            LimitStyle::Default
        } else {
            LimitStyle::SubSup
        };
        write_operator_name(buf, s, limits);
        return;
    }
    // Any other multi-character operator (`<mo>:=</mo>`, `<mo>≈:</mo>`) must
    // keep every character: the single-glyph spellings below look only at the
    // first one. Raw Unicode reparses losslessly, so write the characters
    // literally (escaping TeX specials) inside one atom of the original class.
    if is_compound {
        write_compound_symbol(buf, kind, s);
        return;
    }
    // A handful of ASCII characters are active or classed specially in math
    // mode.  Printing them literally would change an ordinary AST symbol when
    // the output is parsed again (`~` becomes a space, `^` starts a script,
    // `'` becomes a prime, and `<`/`>`/`|` acquire relation/fence classes).
    // KaTeX exposes TeX's `\char` primitive in math mode, so use its backtick
    // form to emit the glyph without changing the public node shape.  Restrict
    // this to ordinary symbols: real binary and relation tokens must continue
    // using their canonical operators and spacing.
    if kind == SymbolKind::Ordinary && needs_escape(ch, Place::Identifier) {
        write_char(buf, ch, Place::Identifier);
        return;
    }
    // KaTeX's (and TeX's) spelling of the opening quote in math.
    if ch == '\u{2018}' && kind == SymbolKind::Ordinary {
        buf.push('`');
        return;
    }
    // Math minus sign → ASCII hyphen-minus for LaTeX output. A literal
    // Unicode minus can arrive as an Ordinary symbol (the ASCII `-` parser
    // maps to BinaryOp), so retain that class explicitly; otherwise a run
    // such as `−-B` would write as `-- B` and reparse with two BinaryOps.
    if ch == '\u{2212}' {
        if kind == SymbolKind::Ordinary {
            buf.push_str("\\mathord{-}");
        } else {
            buf.push('-');
        }
        return;
    }
    // A bar's opening, closing, neutral-fence and ordinary forms have
    // different math spacing even though they use the same glyph.
    if let Some(command) = match (s, kind) {
        ("|", SymbolKind::Open) => Some(r"\lvert"),
        ("|", SymbolKind::Close) => Some(r"\rvert"),
        ("‖", SymbolKind::Ordinary) => Some(r"\Vert"),
        ("‖", SymbolKind::Close) => Some(r"\rVert"),
        ("‖", SymbolKind::Fence) => Some(r"\|"),
        _ => None,
    } {
        buf.push_str(command);
        return;
    }
    // ASCII symbols are written directly
    if ch.is_ascii() {
        match ch {
            // A literal dot after a digit must stay a separate atom even
            // when the following node starts with another digit.
            '.' if buf.as_bytes().last().is_some_and(u8::is_ascii_digit) => buf.push_str(" ."),
            // `\&` is an ordinary atom; KaTeX's `\And` is its binary form.
            '&' if kind == SymbolKind::BinaryOp => buf.push_str("\\And"),
            // A full stop is ordinary in math; `\ldotp` is its punctuation form.
            '.' if kind == SymbolKind::Punctuation => buf.push_str("\\ldotp"),
            ':' if kind == SymbolKind::Punctuation => buf.push_str("\\colon"),
            // A `/` is ordinary in math, a `:` a relation.
            '/' => write_class_char(buf, kind, ch),
            ':' if kind != SymbolKind::Relation => write_class_char(buf, kind, ch),
            '\'' | '`' | '\\' | '^' | '_' | '~' | '\0'..=' ' | '\u{7F}' => {
                write_class_char(buf, kind, ch)
            },
            _ => {
                write_char(buf, ch, Place::Class);
            },
        }
        return;
    }
    // The same dagger glyph has ordinary and binary TeX commands.
    let canonical_command = canonical.map(|(name, _)| name);
    let canonical_kind = canonical.map(|(_, class)| class);
    // Common case: the canonical command already has the symbol's class.
    if let Some((name, class)) = canonical
        && class == kind
        && !matches!(ch, '†' | '‡')
    {
        buf.command(name);
        return;
    }
    let command = match (kind, ch) {
        (SymbolKind::Ordinary, '†') => Some("dag"),
        (SymbolKind::Ordinary, '‡') => Some("ddag"),
        // Many core commands (`\boxdot`) have no canonical reverse entry;
        // any command with the same glyph and class reparses identically.
        _ => canonical_command.or_else(|| crate::unicode::symbols::command_for_class(ch, kind)),
    };
    if let Some(cmd) = command {
        // For example, U+2223 has the relation command `\mid`. Preserve
        // the ordinary atom class when the source gave it ordinary spacing.
        if kind == SymbolKind::Ordinary
            && canonical_command == Some(cmd)
            && canonical_kind.is_some_and(|mapped| {
                !matches!(mapped, SymbolKind::Ordinary | SymbolKind::Alphabetic)
            })
        {
            if let Some(alias) = crate::unicode::symbols::command_for_class(ch, kind) {
                buf.command(alias);
                return;
            }
            buf.push_str("\\mathord{");
            buf.command(cmd);
            buf.push('}');
            return;
        }
        // Likewise for the other atom classes: `\vartriangle` is a relation
        // spelled with the glyph of the ordinary `\triangle`, and `\cdotp` is
        // punctuation spelled with the binary `\cdot`.
        // The canonical spelling's class is in the same reverse entry; only
        // the dagger special cases need a forward lookup.
        let command_kind = if canonical_command == Some(cmd) {
            canonical_kind
        } else {
            crate::unicode::symbols::lookup_command(cmd).map(|(_, kind)| kind)
        };
        if let Some(wrapper) = explicit_class_wrapper(kind)
            && command_kind.is_some_and(|command_kind| command_kind != kind)
        {
            if let Some(alias) = crate::unicode::symbols::command_for_class(ch, kind) {
                buf.command(alias);
                return;
            }
            buf.push_str(wrapper);
            buf.command(cmd);
            buf.push('}');
            return;
        }
        buf.command(cmd);
        return;
    }
    if needs_escape(ch, Place::Class) && !is_no_token(ch) {
        write_class_char(buf, kind, ch);
        return;
    }
    if let Some(cmd) = crate::latex::packages::package_command_for_symbol(s, kind) {
        buf.command(cmd);
        return;
    }
    // For relation/binary operators that have no direct LaTeX command in our
    // table, wrap the raw Unicode character so its math class is preserved
    // across round-trips. Without this, the parser would re-read the bare
    // character as Ordinary and spacing would change.
    match kind {
        SymbolKind::Relation => {
            buf.push_str("\\mathrel{");
            write_char(buf, ch, Place::Class);
            buf.push('}');
        },
        SymbolKind::BinaryOp => {
            buf.push_str("\\mathbin{");
            write_char(buf, ch, Place::Class);
            buf.push('}');
        },
        _ => buf.push_str(s),
    }
}

/// The character token of `ch` in an atom of class `kind`.
#[cold]
fn write_class_char(buf: &mut LatexBuffer, kind: SymbolKind, ch: char) {
    let wrapper = explicit_class_wrapper(kind);
    buf.push_str(wrapper.unwrap_or_default());
    write_char(buf, ch, Place::Class);
    if wrapper.is_some() {
        buf.push('}');
    }
}

/// The class command that forces an atom of `kind`, for the classes that a
/// bare glyph or command might not reparse as.
fn explicit_class_wrapper(kind: SymbolKind) -> Option<&'static str> {
    match kind {
        SymbolKind::Relation => Some("\\mathrel{"),
        SymbolKind::BinaryOp => Some("\\mathbin{"),
        SymbolKind::Open => Some("\\mathopen{"),
        SymbolKind::Close => Some("\\mathclose{"),
        SymbolKind::Punctuation => Some("\\mathpunct{"),
        SymbolKind::LargeOp | SymbolKind::Ordinary | SymbolKind::Fence | SymbolKind::Alphabetic => {
            None
        },
    }
}

/// Write a multi-character symbol as one atom of class `kind` without losing
/// any of its characters.
#[cold]
fn write_compound_symbol(buf: &mut LatexBuffer, kind: SymbolKind, s: &str) {
    let wrapper = match kind {
        SymbolKind::LargeOp => Some("\\mathop{"),
        SymbolKind::Ordinary | SymbolKind::Fence | SymbolKind::Alphabetic => {
            // A run of plain non-ASCII glyphs reparses as ordinary on its
            // own; anything carrying its own class (`:`, `≈`) or a combining
            // mark that could attach elsewhere needs the explicit atom.
            s.chars()
                .any(|ch| {
                    ch.is_ascii()
                        || is_combining_mark(ch)
                        || mapped_symbol_kind(ch).is_some_and(|kind| {
                            !matches!(kind, SymbolKind::Ordinary | SymbolKind::Alphabetic)
                        })
                })
                .then_some("\\mathord{")
        },
        _ => explicit_class_wrapper(kind),
    };
    if let Some(wrapper) = wrapper {
        buf.push_str(wrapper);
    }
    for ch in s.chars() {
        if let Some(cmd) = nary_op_to_latex(ch) {
            // The reader would take what follows as the operator's body.
            buf.push_str("\\mathord{");
            buf.command(cmd);
            buf.push('}');
        } else {
            write_char(buf, ch, Place::Compound);
        }
    }
    if wrapper.is_some() {
        buf.push('}');
    }
}

/// Combining diacritical marks, which would attach to whatever precedes them
/// if written outside an explicit atom.
fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

fn single_math_space(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && crate::unicode::math_space_width(ch).is_some()).then_some(ch)
}

fn authored_math_space_symbol(arena: &AstArena, id: NodeId) -> Option<char> {
    match *arena.node(id) {
        CompactNode::Symbol(SymbolKind::Ordinary, sid) => single_math_space(arena.str_val(sid)),
        _ => None,
    }
}

fn write_space(buf: &mut LatexBuffer, r: Rational) {
    // Named TeX spaces are retained for their canonical widths. Every other
    // rational width uses a fixed em kern so reimport preserves the AST.
    match r {
        r if r == Rational::new(1, 18) => buf.push_str(r"\mkern1mu "),
        r if r == Rational::new(-3, 18) => buf.push_str(r"\!"),
        r if r == Rational::new(-4, 18) => buf.push_str(r"\negmedspace "),
        r if r == Rational::new(-5, 18) => buf.push_str(r"\negthickspace "),
        r if r == Rational::new(3, 18) => buf.push_str(r"\,"),
        r if r == Rational::new(4, 18) => buf.push_str(r"\:"),
        r if r == Rational::new(5, 18) => buf.push_str(r"\;"),
        r if r == Rational::new(1, 3) => buf.push_str(r"\mkern6mu "),
        r if r == Rational::new(1, 4) => buf.push_str(r"\ "),
        r if r == Rational::new(1, 2) => buf.push_str(r"\enspace "),
        r if r == Rational::new(1, 1) => buf.push_str(r"\quad "),
        r if r == Rational::new(2, 1) => buf.push_str(r"\qquad "),
        r if r == Rational::zero() => {},
        r => {
            buf.push_str(r"\hspace{");
            write_rational_dimension(buf, r);
            buf.push('}');
        },
    }
}

fn write_rational_dimension(buf: &mut LatexBuffer, width: Rational) {
    buf.push_decimal(width, 1);
    buf.push_str("em");
}

fn write_space_with_basis(buf: &mut LatexBuffer, width: Rational, basis: Option<SpaceBasis>) {
    if width.num == 0 {
        return;
    }
    let (cmd, scale, unit) = match basis {
        None => return write_space(buf, width),
        Some(SpaceBasis::TextEm) => (r"\kern", 1, "em "),
        Some(SpaceBasis::Physical) => (r"\kern", 10, "pt "),
        Some(SpaceBasis::MathMu) => (r"\mkern", 18, "mu "),
    };
    buf.push_str(cmd);
    buf.push_decimal(width, scale);
    buf.push_str(unit);
}

/// The `\big`-family command name for a scaled delimiter, or `None` when the
/// factor is not one of the four TeX sizes or the content is not a delimiter.
fn big_delim_command(arena: &AstArena, factor: Rational, content: NodeId) -> Option<&'static str> {
    let CompactNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, sid) =
        *arena.node(content)
    else {
        return None;
    };
    // The reader refuses any other glyph after a sizing command.
    let delim = arena.str_val(sid);
    if !delim.is_empty()
        && !single_char(delim).is_some_and(|ch| {
            matches!(ch, '<' | '>') || crate::unicode::symbols::is_tex_delimiter(ch)
        })
    {
        return None;
    }
    crate::ast::BIG_DELIM_SIZES
        .iter()
        .find(|big| big.scale == factor)
        .map(|big| big.command)
}

/// The class suffix of `\bigl`/`\bigr`/`\bigm` for an atom type.
fn big_delim_suffix(atom_type: MathAtomType) -> Option<&'static str> {
    match atom_type {
        MathAtomType::Open => Some("l"),
        MathAtomType::Close => Some("r"),
        MathAtomType::Rel => Some("m"),
        _ => None,
    }
}

/// Write the delimiter after a `\big`-family command, preferring a named
/// command with the symbol's own class (`\lvert`, `\llbracket`).
fn write_big_delim(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    let CompactNode::Symbol(kind, sid) = *arena.node(content) else {
        return;
    };
    write_delim_with_class(buf, arena.str_val(sid), Some(kind));
}

/// The control word that the delimiter reader maps back to the glyph `ch`,
/// with the class `kind` when one is known. ASCII glyphs (and the escaped
/// `\|`) keep their short spelling.
fn delimiter_command(ch: char, kind: Option<SymbolKind>) -> Option<&'static str> {
    match (ch, kind) {
        ('|', Some(SymbolKind::Open)) => return Some("lvert"),
        ('|', Some(SymbolKind::Close)) => return Some("rvert"),
        ('\u{2016}', Some(SymbolKind::Open)) => return Some("lVert"),
        ('\u{2016}', Some(SymbolKind::Close)) => return Some("rVert"),
        ('\u{23D0}', _) => return Some("arrowvert"),
        ('\u{23AA}', _) => return Some("bracevert"),
        _ if ch.is_ascii() || ch == '\u{2016}' => return None,
        _ => {},
    }
    let canonical = crate::unicode::symbols::unicode_to_latex_entry(ch);
    let named = match (canonical, kind) {
        (Some((name, class)), Some(kind)) if class == kind => Some(name),
        (_, Some(kind)) => crate::unicode::symbols::command_for_class(ch, kind),
        _ => None,
    }
    .or(canonical.map(|(name, _)| name))
    .filter(|name| latex_to_unicode(name) == Some(ch));
    if named.is_some() {
        return named;
    }
    let mut utf8 = [0; 4];
    let glyph: &str = ch.encode_utf8(&mut utf8);
    kind.into_iter()
        .chain([SymbolKind::Open, SymbolKind::Close, SymbolKind::Fence])
        .find_map(|class| crate::latex::packages::package_command_for_symbol(glyph, class))
}

fn write_delim_with_class(buf: &mut LatexBuffer, delim: &str, kind: Option<SymbolKind>) {
    let mut chars = delim.chars();
    // Only TeX delimiters get a command name; any other glyph that the lenient
    // reader allowed keeps its literal spelling.
    if let (Some(ch), None) = (chars.next(), chars.next())
        && crate::unicode::symbols::is_tex_delimiter(ch)
        && let Some(name) = delimiter_command(ch, kind)
    {
        buf.command(name);
        return;
    }
    match delim {
        "" => buf.push('.'),
        "\\" => buf.push_str("\\backslash"),
        "%" | "$" | "#" | "&" | "_" | "^" | "~" => {
            buf.command(delim);
        },
        "{" => buf.push_str("\\{"),
        "}" => buf.push_str("\\}"),
        "\u{2016}" => buf.push_str("\\|"),
        s => buf.push_str(s),
    }
}

fn write_delim(buf: &mut LatexBuffer, delim: &str) {
    write_delim_with_class(buf, delim, None);
}

/// Whether `\left`, `\middle` and `\right` can be given `delim`: nothing, or one
/// glyph that TeX reads as one token after them. A letter would join the
/// command's name (a non-ASCII one too, in XeTeX and LuaTeX), and the control
/// symbols of `# $ % & _ ^ ~` are no delimiters (`\^` is an accent and takes
/// what follows). MathML's `<mfenced>` and the other formats allow any text.
fn writes_as_delimiter(delim: &str) -> bool {
    match delim.as_bytes() {
        [] => true,
        [byte] => {
            byte.is_ascii_punctuation()
                && !matches!(byte, b'#' | b'$' | b'%' | b'&' | b'_' | b'^' | b'~')
        },
        _ => single_char(delim).is_some_and(|ch| {
            !ch.is_whitespace() && !ch.is_control() && !ch.is_alphabetic() && !is_combining_mark(ch)
        }),
    }
}

/// [`writes_as_delimiter`] for a `\middle` below the fence's own list, where the
/// reader takes a `'` after it for a prime.
fn writes_as_nested_middle(delim: &str) -> bool {
    delim != "'" && writes_as_delimiter(delim)
}

/// Text that is no delimiter, as an atom of the delimiter's class inside a
/// fence that has none there.
#[cold]
fn write_literal_delimiter(buf: &mut LatexBuffer, delim: &str, wrapper: &str) {
    buf.push_str(wrapper);
    match single_char(delim) {
        Some(ch) => write_literal_glyph(buf, ch, mapped_symbol_kind(ch)),
        None => delim.chars().for_each(|ch| {
            write_char(buf, ch, Place::Delimiter);
        }),
    }
    buf.push('}');
}

/// A leading-script base that is a nonzero math space; its scripts are
/// written without the usual enclosing group.
fn is_bare_space_prescript_base(arena: &AstArena, base: NodeId) -> bool {
    match *arena.node(base) {
        CompactNode::Space(width) => width.num != 0,
        CompactNode::Symbol(_, sid) => single_math_space(arena.str_val(sid)).is_some(),
        CompactNode::MathAtom {
            atom_type: MathAtomType::Ord,
            content,
        } => authored_math_space_symbol(arena, content).is_some(),
        _ => false,
    }
}

/// Write the (non-empty) body of an n-ary operator, after its separator.
fn write_nary_body(buf: &mut LatexBuffer, arena: &AstArena, body: NodeId) {
    if let CompactNode::PreScripts { base, .. } = *arena.node(body)
        && has_visible_prescripts(arena, body)
        && !is_bare_space_prescript_base(arena, base)
    {
        // Leading scripts are already written as one `{{}_{C}f}` group.
        write_node(buf, arena, body);
    } else if is_numeric_argument(arena, body) {
        write_always_braced(buf, arena, body);
    } else if is_script_with_empty_base(arena, body) {
        // The group is read back as a row holding one stand-alone script,
        // so spell it as one: `\int {{}^{2}}`, not `\int {{}^2}`.
        buf.push('{');
        write_node_slice(buf, arena, &[body], false);
        buf.push('}');
    } else {
        write_braced(buf, arena, body);
    }
}

/// The only character of `text`.
fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

/// True for a large operator symbol that is not yet an n-ary node. One
/// without a command is written `\mathop{…}`, which takes no body.
#[inline(always)]
fn is_bare_large_op(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(SymbolKind::LargeOp, sid)
        if is_named_large_op(arena.str_val(sid)))
}

fn is_named_large_op(text: &str) -> bool {
    single_char(text).is_some_and(is_latex_nary)
}

/// An n-ary operator whose glyph is no large operator of its own (MTEF and
/// OMML accept any character): the operator atom `\mathop{…}` with its
/// limits, then the body as the next sibling. A placement control has
/// nothing to place without scripts and is left out, as it is when the
/// result is read back.
#[cold]
fn write_unnamed_nary(
    buf: &mut LatexBuffer,
    arena: &AstArena,
    op: (char, Option<SymbolKind>),
    scripts: [NodeId; 2],
    body: NodeId,
    limits: LimitStyle,
) {
    let start = buf.len();
    write_mathop_glyph(buf, op.0, op.1);
    let visible = scripts.map(|script| !is_effectively_empty_arg(arena, script));
    if visible != [false; 2] {
        match limits {
            LimitStyle::AboveBelow => buf.push_str("\\limits"),
            LimitStyle::SubSup => buf.push_str("\\nolimits"),
            LimitStyle::Default => {},
        }
    }
    for ((marker, script), visible) in ['_', '^'].into_iter().zip(scripts).zip(visible) {
        if visible {
            buf.push(marker);
            write_script_arg(buf, arena, script, ScriptArgGroup::Minimal);
        }
    }
    if is_empty_body(arena, body) {
        return;
    }
    if (has_visible_prescripts(arena, body) && !buf.ends_with(' '))
        || (appended_ends_with_letter_command(buf, start, false)
            && node_starts_with_ascii_letter(arena, body))
    {
        buf.push(' ');
    }
    match *arena.node(body) {
        CompactNode::Nary { .. } => write_node(buf, arena, body),
        // A group after an atom would be opened when it is normalized again.
        CompactNode::Row(span)
            if arena.children(span).iter().all(|&id| {
                matches!(
                    *arena.node(id),
                    CompactNode::Identifier(_) | CompactNode::Number(_) | CompactNode::Symbol(..)
                )
            }) && matches!(
                arena.children(span).first().map(|&id| *arena.node(id)),
                Some(CompactNode::Identifier(_) | CompactNode::Number(_))
            ) =>
        {
            write_node_slice(buf, arena, arena.children(span), false);
        },
        _ => write_node_slice(buf, arena, &[body], false),
    }
}

/// `ch` as one large-operator atom, for a glyph no command reads back as a
/// large operator.
#[cold]
fn write_mathop_glyph(buf: &mut LatexBuffer, ch: char, kind: Option<SymbolKind>) {
    buf.push_str("\\mathop{");
    write_literal_glyph(buf, ch, kind);
    buf.push('}');
}

/// Write `ch` so that the reader (and TeX) sees that one glyph and nothing
/// else, with the class the glyph has on its own, `kind` in the symbol table.
/// The spelling reads back as a node that is written the same way.
#[cold]
fn write_literal_glyph(buf: &mut LatexBuffer, ch: char, kind: Option<SymbolKind>) {
    match ch {
        _ if ch.is_ascii() || needs_escape(ch, Place::Atom) => {
            write_char(buf, ch, Place::Atom);
        },
        _ => match kind {
            None if ch.is_alphabetic() => buf.push(ch),
            Some(SymbolKind::Alphabetic) => write_identifier_text(buf, ch.encode_utf8(&mut [0; 4])),
            kind => write_symbol_text(
                buf,
                kind.unwrap_or(SymbolKind::Ordinary),
                ch.encode_utf8(&mut [0; 4]),
            ),
        },
    }
}

/// Whether the reader takes `id` as the body of a preceding n-ary operator.
/// A row or cell break ends the body; `\middle` is not read as a fence
/// separator there; an empty-base script would become a limit; a size or
/// style declaration applies to the rest of the list, as in the parser's
/// `starts_declaration`.
fn reads_as_nary_body(arena: &AstArena, id: NodeId) -> bool {
    !matches!(
        *arena.node(id),
        CompactNode::Newline
            | CompactNode::ColAlign(_)
            | CompactNode::Middle(_)
            | CompactNode::SizeScale(_)
            | CompactNode::StyleSwitch(_)
    ) && !is_script_with_empty_base(arena, id)
}

/// Nesting limit for a chain of bare large operators. Longer chains start a
/// new chain, which keeps the output well within the reader's depth limit
/// (and the writer iterative) while every chain still reads back the same.
const MAX_LARGE_OP_NESTING: usize = 32;

/// Write a bare large operator `ops[0]` together with the body the reader
/// will give it, and return the number of sibling nodes written. A body
/// that is itself a bare operator nests its own body, `\int {\int x}`, as
/// the reader groups `\int\int x`; a `∫⋯∫` triple is one `\idotsint` body.
/// A sibling that cannot be a body gets an explicit empty body, as an
/// empty-body Nary does.
fn write_large_op_chain(buf: &mut LatexBuffer, arena: &AstArena, ops: &[NodeId]) -> usize {
    let mut open = 0;
    let mut i = 0;
    let consumed = loop {
        write_node(buf, arena, ops[i]);
        let rest = &ops[i + 1..];
        let Some(&body) = rest.first() else {
            break i + 1;
        };
        if !reads_as_nary_body(arena, body) {
            buf.push_str("{}");
            break i + 1;
        }
        buf.push(' ');
        if rest.len() >= 3 && is_idotsint_triple(arena, &rest[..3]) {
            // `\idotsint` takes no body of its own.
            buf.push('{');
            write_idotsint(buf);
            buf.push('}');
            break i + 4;
        }
        if is_bare_large_op(arena, body)
            && open < MAX_LARGE_OP_NESTING
            && rest
                .get(1)
                .is_some_and(|&next| reads_as_nary_body(arena, next))
        {
            buf.push('{');
            open += 1;
            i += 1;
            continue;
        }
        write_nary_body(buf, arena, body);
        break i + 2;
    };
    for _ in 0..open {
        buf.push('}');
    }
    consumed
}

/// Write a node wrapped in braces. If the node is a single simple token,
/// skip the braces for cleaner output. A single-child Row containing a
/// non-simple node keeps the braces so the child is not unwrapped in contexts
/// where grouping matters (e.g. a script base that is a Row wrapping a Color
/// or command node).
#[inline]
fn write_braced(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    if is_simple_token(arena, id) {
        write_node(buf, arena, id);
    } else if let CompactNode::Row(span) = *arena.node(id) {
        let children = arena.children(span);
        if children.len() == 1 && is_simple_token(arena, children[0]) {
            // Single simple child — unwrap to avoid `{a}` round-trip churn.
            write_node(buf, arena, id);
        } else {
            buf.push('{');
            write_nodes(buf, arena, span);
            buf.push('}');
        }
    } else {
        buf.push('{');
        write_node(buf, arena, id);
        buf.push('}');
    }
}

/// Write the contents of a braced argument, unwrapping a top-level Row so that
/// commands like \boxed{abc} do not end up as \boxed{{abc}} after re-parsing.
#[inline]
fn write_wrapped_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    if let CompactNode::Row(span) = *arena.node(content) {
        write_nodes(buf, arena, span);
    } else {
        write_node(buf, arena, content);
    }
}

/// Write a colored-box body in math mode. xcolor builds the body in a text
/// `\hbox`, even when the surrounding fragment is already in math mode, so a
/// plain `x^2` body would either fail or lose its mathematical styling.
#[inline]
fn write_math_wrapped_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    buf.push_str(r"\ensuremath{");
    write_wrapped_content(buf, arena, content);
    buf.push('}');
}

#[inline]
fn write_box_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    if buf.latex_profile == LatexProfile::PdfLaTeX {
        write_math_wrapped_content(buf, arena, content);
    } else {
        write_wrapped_content(buf, arena, content);
    }
}

fn row_contains_style_switch(arena: &AstArena, span: NodeSpan, depth: usize) -> bool {
    if depth > crate::ast::MAX_AST_DEPTH {
        return false;
    }
    arena
        .children(span)
        .iter()
        .any(|&id| match *arena.node(id) {
            CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_) => true,
            CompactNode::Row(inner) | CompactNode::Styled(_, inner) => {
                row_contains_style_switch(arena, inner, depth + 1)
            },
            _ => false,
        })
}

/// Whether `id` is a `\middle` or holds one anywhere below it (a row, a
/// color, a script, an array cell, …) outside any nested fence, which owns
/// its own `\middle`s.
fn holds_middle(arena: &AstArena, id: NodeId) -> bool {
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        let node = arena.node(id);
        match *node {
            CompactNode::Middle(_) => return true,
            CompactNode::Fenced { .. } => {},
            _ => stack.extend(crate::ast::compact_children(arena, node)),
        }
    }
    false
}

/// Write a compound operator as `\mathop{...}`.  The wrapper is always kept:
/// in TeX `\mathop{\hat x}` is an Op atom (operator spacing, movable
/// limits) while a bare `\hat{x}` is an ordinary accent atom, so dropping
/// the wrapper around an accent would change the class on the next read.
fn write_math_atom_op(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    buf.push_str("\\mathop{");
    let start = buf.len();
    write_atom_content(buf, arena, content);
    // `\mathop{⅀}` is the operator.
    if single_char(&buf[start..]) == Some(UNNAMED_NARY) {
        buf.insert_text(start, "{");
        buf.push('}');
    }
    buf.push('}');
}

/// The content of a class command. A lone glyph right after its brace must be
/// a token of its own.
fn write_atom_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    if let CompactNode::Symbol(SymbolKind::Ordinary, sid) = *arena.node(content)
        && let Some(ch) = single_char(arena.str_val(sid))
        && is_no_token(ch)
    {
        write_char_code(buf, ch, " ");
    } else {
        write_wrapped_content(buf, arena, content);
    }
}

/// Accent bases keep the accent attached to the core operator, so an
/// automatically inserted `AboveBelow` marker is not needed in their emitted
/// LaTeX.  This also avoids redundant `\lim\limits` in canonical accents.
fn write_accent_base_content(buf: &mut LatexBuffer, arena: &AstArena, content: NodeId) {
    match *arena.node(content) {
        // Compact-to-heap reconstruction represents an explicit row break as
        // a zero-width space. Keep that operand visible inside an accent
        // argument so `\underbracket\\` does not collapse to `{}`.
        CompactNode::Space(width) if width.num == 0 => buf.push_str("\\\\ "),
        CompactNode::Operator {
            name,
            limits: LimitStyle::AboveBelow | LimitStyle::Default,
        } => {
            let name = &*kept_name(arena.str_val(name));
            let style = if crate::unicode::symbols::operator_has_limits(name) {
                LimitStyle::Default
            } else {
                LimitStyle::SubSup
            };
            write_operator_name(buf, name, style);
        },
        _ => write_wrapped_content(buf, arena, content),
    }
}

fn write_box_math(buf: &mut LatexBuffer, arena: &AstArena, nodes: &[NodeId]) {
    if let [only] = nodes
        && let CompactNode::Row(span) = *arena.node(*only)
    {
        write_nodes(buf, arena, span);
    } else {
        write_node_slice(buf, arena, nodes, false);
    }
}

/// Emit upright text directly in a text box and keep math runs in inline math mode.
fn write_hbox_content(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    let one = [id];
    let children = match *arena.node(id) {
        CompactNode::Row(span) => arena.children(span),
        _ => &one,
    };
    // Text that is dropped entirely leaves the math around it one run.
    let dropped = |id: NodeId| {
        let CompactNode::Text(style, sid) = *arena.node(id) else {
            return None;
        };
        let text = arena.str_val(sid);
        // The typewriter face writes every character.
        let written = if style == TextStyle::Monospace {
            !text.is_empty()
        } else {
            !is_dropped_text(text)
        };
        (!written).then_some(sid)
    };
    let kept: smallvec::SmallVec<[NodeId; 8]>;
    let mut rest = children;
    if children.iter().any(|&id| dropped(id).is_some()) {
        for &id in children {
            if let Some(sid) = dropped(id) {
                arena
                    .str_val(sid)
                    .chars()
                    .for_each(|ch| buf.report.char(ch));
            }
        }
        kept = children
            .iter()
            .copied()
            .filter(|&id| dropped(id).is_none())
            .collect();
        rest = &kept;
    }
    while let Some((&first, tail)) = rest.split_first() {
        if let CompactNode::Text(style, sid) = *arena.node(first) {
            // Text that only dropped text kept apart is read as one.
            let mut text = std::borrow::Cow::Borrowed(arena.str_val(sid));
            rest = tail;
            while let Some((&next, tail)) = rest.split_first()
                && let CompactNode::Text(next_style, next_sid) = *arena.node(next)
                && next_style == style
            {
                text.to_mut().push_str(arena.str_val(next_sid));
                rest = tail;
            }
            if style == TextStyle::Normal {
                write_text_content(buf, &text, false);
            } else {
                write_styled_text(buf, style, &text);
            }
        } else if matches!(
            *arena.node(first),
            CompactNode::Reflect {
                text_mode: true,
                ..
            }
        ) {
            write_node(buf, arena, first);
            rest = tail;
        } else {
            let n = rest
                .iter()
                .take_while(|&&id| {
                    !matches!(
                        *arena.node(id),
                        CompactNode::Text(..)
                            | CompactNode::Reflect {
                                text_mode: true,
                                ..
                            }
                    )
                })
                .count();
            let (math, tail) = rest.split_at(n);
            buf.push('$');
            let start = buf.len();
            write_box_math(buf, arena, math);
            // A combining mark would join the math shift or brace before it.
            let joined: smallvec::SmallVec<[usize; 2]> = buf[start..]
                .match_indices(is_joining_mark)
                .map(|(at, _)| start + at)
                .filter(|&at| at == start || ends_with_brace(&buf[start..at]))
                .collect();
            buf.insert_text_at_each(&joined, r"\relax ");
            buf.push('$');
            rest = tail;
        }
    }
}

/// Write a node inside a pair of braces, always keeping the braces. Unlike
/// `write_braced`, this never unwraps a single-child Row, so it is safe for
/// stand-alone empty-base scripts whose arguments must stay isolated from the
/// surrounding text.
#[inline]
fn write_always_braced(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    if is_empty_row(arena, id) {
        return;
    }
    buf.push('{');
    if let CompactNode::Row(span) = *arena.node(id) {
        write_nodes(buf, arena, span);
    } else {
        write_node(buf, arena, id);
    }
    buf.push('}');
}

/// Whether nothing of `text` is written.
fn is_dropped_text(text: &str) -> bool {
    text.chars()
        .all(|ch| escape::spelling(ch, Place::Text) == Spelling::Dropped)
}

fn write_styled_text(buf: &mut LatexBuffer, style: TextStyle, text: &str) {
    use TextStyle::*;
    let wrappers: &[&str] = match style {
        Bold => &[r"\textbf{"],
        Italic => &[r"\textit{"],
        BoldItalic => &[r"\textbf{", r"\textit{"],
        Monospace => &[r"\texttt{"],
        SansSerif => &[r"\textsf{"],
        SansSerifBold => &[r"\textsf{", r"\textbf{"],
        SansSerifItalic => &[r"\textsf{", r"\textit{"],
        SansSerifBoldItalic => &[r"\textsf{", r"\textbf{", r"\textit{"],
        _ => &[r"\text{"],
    };
    // Around no text, the reader does not combine nested faces, and the outer
    // one would join its neighbours: keep it an atom of its own.
    if let [outer, _, ..] = wrappers
        && is_dropped_text(text)
    {
        buf.push_str(r"\mathord{");
        buf.push_str(outer);
        buf.push_str("}}");
        text.chars().for_each(|ch| buf.report.char(ch));
        return;
    }
    for wrapper in wrappers {
        buf.push_str(wrapper);
    }
    write_text_content(buf, text, style == Monospace);
    for _ in wrappers {
        buf.push('}');
    }
}

/// Write decoded text so that text-mode syntax and ligatures cannot change it.
fn write_text_content(buf: &mut LatexBuffer, text: &str, monospace: bool) {
    let place = if monospace {
        Place::Monospace
    } else {
        Place::Text
    };
    let literal_marks = monospace && text.contains(['\\', '{', '}']);
    let mut rest = text;
    let mut previous = None;
    loop {
        let plain = rest
            .bytes()
            .position(|b| {
                !b.is_ascii() || matches!(b, b'-' | b'`' | b'\'') || ascii_needs_escape(b, place)
            })
            .unwrap_or(rest.len());
        let (run, after) = rest.split_at(plain);
        buf.push_str(run);
        // Only a ligature character can matter before the next one.
        if !run.is_empty() {
            previous = None;
        }
        let Some(ch) = after.chars().next() else {
            break;
        };
        rest = &after[ch.len_utf8()..];
        if previous == Some(ch) && matches!(ch, '-' | '`' | '\'') {
            buf.push_str("{}");
        }
        // A combining mark would join the brace or math shift before it. In
        // literal source text (what `\verb` reads) it is a character like any
        // other.
        let joins = |buf: &LatexBuffer| buf.ends_with(['{', '}']) || ends_with_token(buf, &['$']);
        if is_joining_mark(ch) && (literal_marks || joins(buf)) {
            write_char_code(buf, ch, "{}");
        } else if !write_char(buf, ch, place) {
            buf.report.char(ch);
            continue;
        }
        previous = Some(ch);
    }
}

/// Write the base of a sub/superscript. Single-glyph identifiers and symbols
/// use the same token spelling and canonical bracing, including through a
/// single-child Row or a sizing wrapper that will be dropped. Bases that would
/// merge with the preceding output remain explicitly grouped.
#[inline]
fn write_script_base(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    let id = written_script_content(arena, id);
    if is_idotsint_row(arena, id) {
        write_idotsint(buf);
        return;
    }
    if !is_script_base_token(arena, id) {
        write_braced(buf, arena, id);
        return;
    }
    let token = id;
    let braced =
        script_base_needs_braces(arena, token) || would_merge_with_buffer(buf, arena, token);
    if braced {
        buf.push('{');
    }
    write_script_base_token(buf, arena, token);
    if braced {
        buf.push('}');
    }
}

/// Choose braces for what the script will actually emit. Rows containing
/// one significant child are transparent.
fn written_script_content(arena: &AstArena, mut id: NodeId) -> NodeId {
    loop {
        id = match *arena.node(id) {
            CompactNode::Subscript { base, .. }
            | CompactNode::Superscript { base, .. }
            | CompactNode::SubSup { base, .. }
                if !script_has_any_arg(arena, id) =>
            {
                base
            },
            CompactNode::Row(span) => {
                let mut children = arena
                    .children(span)
                    .iter()
                    .copied()
                    .filter(|&child| !is_empty_row(arena, child));
                match (children.next(), children.next()) {
                    (Some(only), None) => only,
                    _ => return id,
                }
            },
            _ => return id,
        };
    }
}

/// Whether this node is emitted as one atomic script-base token rather than a
/// compound expression. Non-ASCII symbols are included; their canonical braces
/// are decided separately from the written glyph, not from the AST node kind.
fn is_script_base_token(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(_) => true,
        CompactNode::Identifier(_) | CompactNode::Symbol(_, _) => {
            script_base_char(arena, id).is_some()
        },
        CompactNode::Scaled { content, .. }
            if !matches!(
                *arena.node(content),
                CompactNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, ..)
            ) =>
        {
            is_script_base_token(arena, content)
        },
        _ => false,
    }
}

/// Preserve the established grouping of non-alphabetic Unicode symbols. ASCII
/// and alphabetic glyphs need no node-type-specific braces: both their raw and
/// preferred-command forms are single atoms before a script marker.
fn script_base_needs_braces(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(_) => script_base_char(arena, id).is_some_and(|ch| {
            (!ch.is_ascii() && !ch.is_alphabetic())
                || (ch.is_ascii() && needs_escape(ch, Place::Identifier))
        }),
        CompactNode::Symbol(kind, _) => script_base_char(arena, id).is_some_and(|ch| {
            if kind == SymbolKind::LargeOp && !is_latex_nary(ch) {
                // `\mathop{…}`, whose limits a group would take away.
                return false;
            }
            (!ch.is_ascii() && !ch.is_alphabetic())
                || (kind == SymbolKind::Ordinary
                    && ch.is_ascii()
                    && !matches!(
                        escape::spelling(ch, Place::Identifier),
                        Spelling::Raw | Spelling::Backslash
                    ))
                || symbol_is_class_atom(kind, ch)
        }),
        CompactNode::Scaled { content, .. } => script_base_needs_braces(arena, content),
        _ => false,
    }
}

/// Whether the symbol writer spells the glyph `ch` as a class command with its
/// argument (`\mathrel{…}`) or a character token. Such an atom is grouped like
/// the `MathAtom` it reads back as. (A large operator's `\mathop{…}` is not:
/// its scripts are limits, which a group would take away.)
fn symbol_is_class_atom(kind: SymbolKind, ch: char) -> bool {
    match kind {
        SymbolKind::LargeOp | SymbolKind::Ordinary => false,
        SymbolKind::Relation | SymbolKind::BinaryOp if !ch.is_ascii() => {
            mapped_symbol_kind(ch) != Some(kind)
        },
        _ => matches!(
            escape::spelling(ch, Place::Class),
            Spelling::Token | Spelling::Code
        ),
    }
}

fn script_base_char(arena: &AstArena, id: NodeId) -> Option<char> {
    let (kind, sid) = match *arena.node(id) {
        CompactNode::Identifier(sid) => (None, sid),
        CompactNode::Symbol(kind, sid) => (Some(kind), sid),
        _ => return None,
    };
    let mut chars = arena.str_val(sid).chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return None;
    };
    // The LaTeX spelling of a binary U+2212 is the ASCII hyphen-minus. Treat
    // it as the emitted ASCII token when deciding whether a script base needs
    // grouping; otherwise the first pass writes `{-}^{...}` while the parser
    // turns that spelling back into U+2212 and the next pass changes shape.
    if kind == Some(SymbolKind::BinaryOp) && ch == '\u{2212}' {
        Some('-')
    } else {
        Some(ch)
    }
}

/// Write one atomic base while preserving the canonical glyph spelling for the
/// node. ASCII identifiers use the symbol writer so punctuation stays literal
/// (`:` rather than the `\colon` command); Unicode identifiers use the regular
/// identifier writer so their preferred commands (`\ell`, `\Re`, …) survive.
/// Sizing wrappers around non-delimiters are dropped by the regular node writer
/// and therefore recurse into their content here as well.
fn write_script_base_token(buf: &mut LatexBuffer, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Identifier(sid) => {
            let text = arena.str_val(sid);
            if text.is_ascii() {
                // Identifier spelling is independent of the math class
                // carried by an explicit Symbol from the character primitive.
                write_symbol_spelling(buf, SymbolKind::Ordinary, text);
            } else {
                write_identifier(buf, arena, sid);
            }
        },
        CompactNode::Scaled { content, .. } => write_script_base_token(buf, arena, content),
        _ => write_node(buf, arena, id),
    }
}

/// Check whether writing `id` immediately after the current buffer content
/// would be parsed as a single token with the previous character.
fn would_merge_with_buffer(buf: &str, arena: &AstArena, id: NodeId) -> bool {
    let last = match buf.as_bytes().last() {
        Some(&b) => b as char,
        None => return false,
    };
    match *arena.node(id) {
        // A dropped \big-like wrapper around a simple token merges exactly like
        // its inner content, so check the wrapped node.
        CompactNode::Scaled { content, .. }
            if !matches!(*arena.node(content), CompactNode::Symbol(..)) =>
        {
            would_merge_with_buffer(buf, arena, content)
        },
        CompactNode::Number(_) => last.is_ascii_digit(),
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            let first = s.chars().next().unwrap_or('?');
            last.is_ascii_alphanumeric() && first.is_ascii_alphabetic()
        },
        _ => false,
    }
}

/// Check if a node is an empty Row.
fn is_empty_row(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Row(span) if arena.children(span).is_empty())
}

/// Check whether a script base writes no visible content.
///
/// A zero-width `\kern` is represented as `Space(0)` but serializes to the
/// same `{}` base as an empty row. Treating both forms alike keeps the script
/// argument's bracing stable after re-parsing.
fn is_empty_script_base(arena: &AstArena, id: NodeId) -> bool {
    is_empty_row(arena, id)
        || matches!(*arena.node(id), CompactNode::Space(width) if width.num == 0)
}

/// Check if a Nary body is effectively empty (empty Row, zero-width Space, or
/// NONE node).
fn is_empty_body(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    is_empty_script_base(arena, id)
}

fn has_visible_prescripts(arena: &AstArena, id: NodeId) -> bool {
    let CompactNode::PreScripts {
        pre_sub, pre_sup, ..
    } = *arena.node(id)
    else {
        return false;
    };
    (pre_sub.is_some() && !is_effectively_empty_arg(arena, pre_sub))
        || (pre_sup.is_some() && !is_effectively_empty_arg(arena, pre_sup))
}

/// True for a missing node or an empty Row (an absent/effectively-empty script
/// argument).
fn is_effectively_empty_arg(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    match *arena.node(id) {
        CompactNode::Row(span) => arena
            .children(span)
            .iter()
            .all(|&child| is_effectively_empty_arg(arena, child)),
        // A color declaration with no following content is a transparent
        // wrapper around an empty row. Unwrap only this transparent wrapper;
        // nonzero spaces and zero-width atoms carry grouping/spacing meaning
        // and must remain script arguments. Zero-width spaces are elided
        // by normalization and are safe to treat as empty here.
        CompactNode::Color { content, .. } => is_effectively_empty_arg(arena, content),
        CompactNode::Scaled { content, .. } => is_effectively_empty_arg(arena, content),
        CompactNode::Space(width) => width.num == 0,
        // A nested script whose markers disappear writes just its base.
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => {
            is_effectively_empty_arg(arena, base) && !script_has_any_arg(arena, id)
        },
        _ => false,
    }
}

/// True if a script node has at least one non-empty subscript or superscript
/// argument.
fn script_has_any_arg(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Subscript { sub, .. } => !is_effectively_empty_arg(arena, sub),
        CompactNode::Superscript { sup, .. } => !is_effectively_empty_arg(arena, sup),
        CompactNode::SubSup { sub, sup, .. } => {
            !is_effectively_empty_arg(arena, sub) || !is_effectively_empty_arg(arena, sup)
        },
        _ => false,
    }
}

fn is_simple_token(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(_) => true,
        CompactNode::Identifier(sid) => arena.str_val(sid).chars().count() == 1,
        CompactNode::Symbol(kind, sid) => single_char(arena.str_val(sid)).is_some_and(|ch| {
            (ch.is_ascii() || ch.is_alphabetic())
                && !symbol_is_class_atom(kind, ch)
                && (kind != SymbolKind::LargeOp || nary_op_to_latex(ch).is_some())
        }),
        // An invalid \big-like sizing wrapper around a simple non-delimiter
        // should be treated as simple so that surrounding braces are not added
        // when the sizing command is dropped during writing.
        CompactNode::Scaled { content, .. }
            if !matches!(*arena.node(content), CompactNode::Symbol(..)) =>
        {
            is_simple_token(arena, content)
        },
        _ => false,
    }
}

/// Check if a Row span would produce only ASCII digit characters when written.
/// Used to detect Rows that should be emitted without braces to avoid
/// re-parse divergence (digits merge into a single Number token).
fn row_produces_only_digits(arena: &AstArena, span: NodeSpan) -> bool {
    for &id in arena.children(span) {
        if !node_produces_only_digits(arena, id) {
            return false;
        }
    }
    true
}

fn node_produces_only_digits(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(sid) => arena.str_val(sid).bytes().all(|b| b.is_ascii_digit()),
        CompactNode::Row(span) => row_produces_only_digits(arena, span),
        _ => false,
    }
}

fn is_line_symbol(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == "\u{203E}")
}

fn is_overbrace(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == "\u{23DE}")
}

fn is_underbrace(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == "\u{23DF}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::normalize::{normalize_compact_expr, normalize_expr};
    use crate::reader::Reader;
    use crate::reader::latex::LatexReader;

    fn roundtrip(input: &str) -> String {
        let config = Config::default();
        let parsed = LatexReader::parse(input, &config).unwrap();
        LatexWriter::write(&parsed.expr, &config).unwrap()
    }

    fn portable_roundtrip(input: &str) -> String {
        let config = Config::default().with_latex_profile(LatexProfile::PdfLaTeX);
        let parsed = LatexReader::parse(input, &config).unwrap();
        LatexWriter::write(&parsed.expr, &config).unwrap()
    }

    #[test]
    fn text_that_is_no_box_color_is_dropped() {
        // A box color is an argument TeX expands, so text that is no color
        // is left out; the box keeps a color that is one.
        let boxed = |bg: Option<&str>, border: Option<&str>| {
            let expr = MathExpr::new(
                vec![MathNode::Boxed {
                    content: Box::new(MathNode::Identifier("x".into())),
                    bg_color: bg.map(Into::into),
                    border_color: border.map(Into::into),
                }],
                DisplayStyle::Inline,
            );
            LatexWriter::write(&expr, &Config::default()).unwrap()
        };
        for (bg, border, want) in [
            (Some(r"a\"), None, "x"),
            (Some("a}"), None, "x"),
            (Some("{a"), None, "x"),
            (Some("a%b"), None, "x"),
            (Some(r"a\ "), None, "x"),
            (Some(r"a\%"), None, "x"),
            (Some("{red}"), None, "x"),
            (Some("red"), Some(r"b\"), r"\colorbox{red}{x}"),
            (Some(r"r\"), Some("blue"), r"\boxed{x}"),
            (Some("red"), Some("blue"), r"\fcolorbox{blue}{red}{x}"),
            (
                Some("#ff0000"),
                Some("red!50!blue"),
                r"\fcolorbox{red!50!blue}{#ff0000}{x}",
            ),
            (Some("rgb(1, 2, 3)"), None, r"\colorbox{rgb(1, 2, 3)}{x}"),
            (Some(r"\input{pwn}"), Some("red"), r"\boxed{x}"),
            (Some("red"), Some(r"\write18{ls}"), r"\colorbox{red}{x}"),
            (Some("^^5cinput"), None, "x"),
            (Some("#1"), None, "x"),
            (Some("red\n\nblue"), None, "x"),
            (Some(r"\def\ "), None, "x"),
            (Some(r"\newcommand\left"), None, "x"),
            (Some(r"\verb|%"), Some("red"), r"\boxed{x}"),
            (Some("red"), Some(r"\verb|%"), r"\colorbox{red}{x}"),
            (Some(r"\global"), None, "x"),
            (Some("\\\u{e9}"), None, "x"),
            (Some("a\\\u{e9}\\"), None, "x"),
            (Some(r"\defx"), None, "x"),
        ] {
            let out = boxed(bg, border);
            assert_eq!(out, want, "{bg:?} {border:?}");
            LatexReader::parse(&out, &Config::default())
                .unwrap_or_else(|e| panic!("{out:?} does not re-parse: {e}"));
        }
    }

    #[test]
    fn portable_box_profile_protects_math_content() {
        let config = Config::default().with_latex_profile(LatexProfile::PdfLaTeX);
        for (input, expected) in [
            (
                r"\colorbox{red}{x^2+\frac{a}{b}}",
                r"\colorbox{red}{\ensuremath{x^{2}+ \frac{a}{b}}}",
            ),
            (
                r"\fcolorbox{red}{#ff0000}{x^2+\frac{a}{b}}",
                r"\fcolorbox{red}[HTML]{ff0000}{\ensuremath{x^{2}+ \frac{a}{b}}}",
            ),
            (
                r"\fcolorbox{#ff0000}{green}{x^2}",
                r"\fcolorbox[HTML]{ff0000}[named]{green}{\ensuremath{x^{2}}}",
            ),
        ] {
            let parsed = LatexReader::parse(input, &config).unwrap();
            let first = LatexWriter::write(&parsed.expr, &config).unwrap();
            assert_eq!(first, expected, "{input}");
            let reparsed = LatexReader::parse(&first, &config).unwrap();
            assert_eq!(
                LatexWriter::write(&reparsed.expr, &config).unwrap(),
                first,
                "{input}"
            );
        }
    }

    #[test]
    fn one_tex_argument_is_one_token_or_one_group() {
        for one in [
            "2", "a", "'", r"\alpha", r"\{", "{22}", "{a{b}c}", r"{\}}", "~",
        ] {
            assert!(is_one_tex_argument(one), "{one}");
        }
        for not_one in [
            "",
            "22",
            "2.5",
            r"\alpha b",
            r"\mathrm{d}",
            "{a}{b}",
            "{a",
            " ",
            "}",
            "^",
        ] {
            assert!(!is_one_tex_argument(not_one), "{not_one}");
        }
    }

    #[test]
    fn unwritable_color_nary_body_is_spelled_as_it_reparses() {
        use crate::{Format, convert};
        // An unwritable color is written transparently, so a single-letter
        // n-ary body must not keep braces that the reparse would drop.
        // A numeric body keeps its braces whatever color it wears, and an
        // unwritable `#f00` is dropped like any other.
        for (src, want) in [
            (r"\int \textcolor{#f00}{c}", r"\int c"),
            (r"\int \textcolor{a_b}{c}", r"\int c"),
            (r"\int \textcolor{#f00}{2}", r"\int {2}"),
            (r"\sum_{i=1}^n \textcolor{#f00}{12}", r"\sum_{i= 1}^n {12}"),
        ] {
            let first = convert(src, Format::LaTeX, Format::LaTeX).unwrap();
            let second = convert(&first, Format::LaTeX, Format::LaTeX).unwrap();
            assert_eq!(first, second, "{src}");
            assert_eq!(first, want, "{src}");
        }
    }

    #[test]
    fn adjacent_upright_cell_identifiers_remain_separate_on_reimport() {
        use crate::{Format, convert};
        // Undefined commands stay commands; they never become identifiers.
        let keep = Config::default().with_undefined_commands(crate::UndefinedCommands::Keep);
        for contents in [r"\foo\word", r"\text{AV}\word", r"\foo\text{AV}"] {
            let src =
                format!(r"\begin{{array}}{{cc}}\multicolumn{{2}}{{r}}{{{contents}}}\end{{array}}");
            let first = crate::convert_with(&src, Format::LaTeX, Format::LaTeX, &keep).unwrap();
            assert!(first.contains(contents), "{first}");
            assert_eq!(
                first,
                crate::convert_with(&first, Format::LaTeX, Format::LaTeX, &keep).unwrap()
            );
        }
        let src = r#"<math><mtable><mtr><mn>1</mn></mtr><mtr><mtd columnalign="right"><mi>AV</mi><mi>end</mi></mtd></mtr></mtable></math>"#;
        let tex = convert(src, Format::MathML, Format::LaTeX).unwrap();
        assert!(
            tex.contains(r"\mathord{\text{AV}}\mathord{\text{end}}"),
            "{tex}"
        );
        assert_eq!(tex, convert(&tex, Format::LaTeX, Format::LaTeX).unwrap());
    }

    #[test]
    fn aligned_cell_keeps_native_identifier_font_semantics() {
        let config = Config::default();
        let native = r"\begin{array}{ll}\multicolumn{1}{r}{LONG}&c\end{array}";
        let mut native_expr = LatexReader::parse(native, &config).unwrap().expr;
        normalize_expr(&mut native_expr);
        let native_latex = LatexWriter::write(&native_expr, &config).unwrap();
        assert!(
            !native_latex.contains(r"\text{LONG}"),
            "native TeX letters must remain math italic: {native_latex}"
        );
        assert_eq!(
            native_latex,
            LatexWriter::write(
                &LatexReader::parse(&native_latex, &config).unwrap().expr,
                &config
            )
            .unwrap()
        );

        let mathml = r#"<math><mtable columnalign="left left"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd columnalign="right"><mi>LONG</mi></mtd><mtd><mi>c</mi></mtd></mtr></mtable></math>"#;
        let mut mathml_expr = crate::from_mathml(mathml).unwrap().expr();
        normalize_expr(&mut mathml_expr);
        let mathml_latex = LatexWriter::write(&mathml_expr, &config).unwrap();
        assert!(
            mathml_latex.contains(r"\text{LONG}"),
            "MathML multi-letter mi must remain upright: {mathml_latex}"
        );

        let italic_mathml = mathml.replace("<mi>LONG</mi>", "<mi mathvariant=\"italic\">LONG</mi>");
        let italic_latex = crate::from_mathml(&italic_mathml)
            .unwrap()
            .to_latex()
            .unwrap();
        assert!(
            italic_latex.contains(r"\mathit{LONG}"),
            "an explicit MathML italic variant must survive: {italic_latex}"
        );
    }

    #[test]
    fn aligned_cell_normalizes_other_runs_and_mixed_mathml_identifiers() {
        let config = Config::default();
        for (source, expected) in [
            (
                r"\begin{array}{l}\multicolumn{1}{r}{1{2}}\end{array}",
                r"12",
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{r}{\text{A}\text{B}}\end{array}",
                r"\text{AB}",
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{r}{{}^{a}x}\end{array}",
                r"{}^{a}x",
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{r}{L{ON}G}\end{array}",
                r"LONG",
            ),
        ] {
            let mut expr = LatexReader::parse(source, &config).unwrap().expr;
            normalize_expr(&mut expr);
            let output = LatexWriter::write(&expr, &config).unwrap();
            assert!(output.contains(expected), "{source} -> {output}");
            assert!(
                !output.contains(r"\text{ON}"),
                "native letters became text: {output}"
            );

            let mut compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            normalize_compact_expr(&mut compact).unwrap();
            let compact_output = LatexWriter::write_compact(&compact, &config).unwrap();
            assert!(
                compact_output.contains(expected),
                "{source} -> {compact_output}"
            );
            assert!(
                !compact_output.contains(r"\text{ON}"),
                "native letters became text: {compact_output}"
            );
        }

        let mixed = r#"<math><mtable columnalign="left"><mtr><mtd><mi>a</mi></mtd></mtr><mtr><mtd columnalign="right"><mrow><mi>LONG</mi><mo>+</mo><mi>x</mi></mrow></mtd></mtr></mtable></math>"#;
        let mut mixed_expr = crate::from_mathml(mixed).unwrap().expr();
        normalize_expr(&mut mixed_expr);
        let mixed_latex = LatexWriter::write(&mixed_expr, &config).unwrap();
        assert!(
            mixed_latex.contains(r"\text{LONG}"),
            "mixed MathML multi-letter mi lost its upright semantics: {mixed_latex}"
        );
        assert!(mixed_latex.contains('+'), "{mixed_latex}");
        assert!(mixed_latex.contains('x'), "{mixed_latex}");

        let mut mixed_compact = crate::reader::mathml::MathmlReader::parse_compact(mixed, &config)
            .unwrap()
            .expr;
        normalize_compact_expr(&mut mixed_compact).unwrap();
        let mixed_compact_latex = LatexWriter::write_compact(&mixed_compact, &config).unwrap();
        assert!(
            mixed_compact_latex.contains(r"\text{LONG}"),
            "compact mixed MathML lost its upright semantics: {mixed_compact_latex}"
        );
    }

    fn math_space_cases() -> [(char, &'static str, Rational); 13] {
        [
            ('\u{00A0}', r"\ ", Rational::new(1, 4)),
            ('\u{2000}', r"\enspace ", Rational::new(1, 2)),
            ('\u{2001}', r"\quad ", Rational::new(1, 1)),
            ('\u{2002}', r"\enspace ", Rational::new(1, 2)),
            ('\u{2003}', r"\quad ", Rational::new(1, 1)),
            ('\u{2004}', r"\mkern6mu ", Rational::new(1, 3)),
            ('\u{2005}', r"\ ", Rational::new(1, 4)),
            ('\u{2006}', r"\,", Rational::new(1, 6)),
            ('\u{2009}', r"\,", Rational::new(1, 6)),
            ('\u{200A}', r"\mkern1mu ", Rational::new(1, 18)),
            ('\u{202F}', r"\,", Rational::new(1, 6)),
            ('\u{205F}', r"\:", Rational::new(2, 9)),
            ('\u{3000}', r"\quad ", Rational::new(1, 1)),
        ]
    }

    #[test]
    fn number() {
        assert_eq!(roundtrip("42"), "42");
    }

    #[test]
    fn identifier() {
        assert_eq!(roundtrip("x"), "x");
    }

    #[test]
    fn greek() {
        let output = roundtrip("\\alpha");
        assert_eq!(output, "\\alpha");
    }

    #[test]
    fn fraction() {
        let output = roundtrip("\\frac{a}{b}");
        assert_eq!(output, "\\frac{a}{b}");
    }

    #[test]
    fn sqrt() {
        assert_eq!(roundtrip("\\sqrt{x}"), "\\sqrt{x}");
    }

    #[test]
    fn superscript() {
        // Number script args are always braced to avoid decimal-merge and
        // trailing/non-trailing grouping instabilities.
        assert_eq!(roundtrip("x^2"), "x^{2}");
    }

    #[test]
    fn subscript() {
        assert_eq!(roundtrip("x_i"), "x_i");
    }

    #[test]
    fn subsup() {
        let output = roundtrip("x_i^2");
        assert_eq!(output, "x_i^{2}");
    }

    #[test]
    fn named_operator() {
        let output = roundtrip("\\sin");
        assert_eq!(output, "\\sin");
    }

    #[test]
    fn named_operator_limit_styles_have_stable_canonical_spellings() {
        for (input, expected) in [
            (r"\operatorname*{sin}", r"\operatorname*{sin}"),
            (r"\operatorname{sin}", r"\sin"),
            (r"\operatorname*{lim}", r"\lim"),
            (r"\operatorname{lim}", r"\lim\nolimits"),
            (r"\operatorname*{rate}", r"\operatorname*{rate}"),
            (r"\operatorname{rate}", r"\operatorname{rate}"),
            (
                r"\operatorname*{rate}\limits",
                r"\operatorname*{rate}\limits",
            ),
        ] {
            assert_eq!(roundtrip(input), expected, "wrong spelling for {input:?}");
        }
    }

    #[test]
    fn fixed_space_widths_survive_both_public_reimport_paths() {
        for (unit, expected) in [
            (".004em", Rational::new(1, 250)),
            (".5mu", Rational::new(1, 36)),
            ("1sp", Rational::new(1, 655360)),
            ("1bp", Rational::new(803, 8000)),
            ("1dd", Rational::new(1238, 11570)),
            ("-.001in", Rational::new(-7227, 1_000_000)),
            ("+ - .125EM", Rational::new(-1, 8)),
        ] {
            let src = format!(r"a\hskip{unit} b");
            let parsed = crate::from_latex(&src).unwrap();
            let space = |expr: MathExpr| {
                expr.nodes.into_iter().find_map(|n| match n {
                    MathNode::Space(r) | MathNode::ExplicitSpace { width: r, .. } => Some(r),
                    _ => None,
                })
            };
            assert_eq!(space(parsed.expr()), Some(expected), "{src}");
            let latex = parsed.to_latex().unwrap();
            let xml = parsed.to_mathml().unwrap();
            assert_eq!(
                space(crate::from_latex(&latex).unwrap().expr()),
                Some(expected),
                "{latex}"
            );
            assert_eq!(
                space(crate::from_mathml(&xml).unwrap().expr()),
                Some(expected),
                "{xml}"
            );
        }
        for width in [
            Rational::new(1, 7),
            Rational::new(-1, 37),
            Rational::new(1, 65537),
        ] {
            let source = MathExpr::inline(vec![MathNode::Space(width)]);
            let latex = LatexWriter::write(&source, &Config::default()).unwrap();
            assert_eq!(
                crate::from_latex(&latex).unwrap().expr().nodes,
                source.nodes
            );
        }
    }

    #[test]
    fn escaped_punctuation_delimiters_keep_their_glyph_after_export() {
        for glyph in ["%", "#", "&", "$", "_", "^", "~"] {
            let input = format!(r"\left. \frac{{a}}{{b}}\right\{glyph}");
            let formula = crate::from_latex(&input).unwrap();
            let first = formula.to_latex().unwrap();
            // No engine takes these as delimiters: `\right\^` is an accent.
            assert!(
                first.contains(r"\mathclose{") && first.ends_with(r"} \right."),
                "{input}: {first}"
            );
            let reparsed = crate::from_latex(&first).unwrap();
            assert_eq!(reparsed.to_latex().unwrap(), first);
            assert!(reparsed.to_mathml().unwrap().contains(match glyph {
                "&" => "&amp;",
                glyph => glyph,
            }));
        }
    }

    #[test]
    fn word_sized_spaces_in_operator_names_round_trip_without_growing() {
        for (input, expected) in [
            (r"\operatorname{a\quad{}b}", r"\operatorname{a\quad{}b}"),
            (
                r"\operatorname*{a\enskip{}b}",
                r"\operatorname*{a\enspace{}b}",
            ),
            (
                r"\operatorname{a\qquad{} b}",
                r"\operatorname{a\quad{}\quad{} b}",
            ),
        ] {
            let parsed = crate::from_latex(input).unwrap();
            let canonical = parsed.to_latex().unwrap();
            assert_eq!(canonical, expected);
            let again = crate::from_latex(&canonical).unwrap();
            assert_eq!(again.to_latex().unwrap(), canonical);
            assert_eq!(again.expr(), parsed.expr());
        }
    }

    #[test]
    fn unicode_minus_preserves_ordinary_class() {
        let first = roundtrip("\u{2212}-B");
        let second = roundtrip(&first);
        assert_eq!(first, r"\mathord{-}- B");
        assert_eq!(first, second);

        // A standalone Unicode minus remains stable, while ASCII minus keeps
        // its normal binary-operator spelling.
        assert_eq!(roundtrip("\u{2212}"), r"\mathord{-}");
        assert_eq!(roundtrip("--B"), "- - B");

        // Text-mode dash ligatures continue to use their existing canonical
        // Unicode spelling; the math-minus fix must not affect text content.
        let text_first = roundtrip(r"\text{−--}");
        assert_eq!(text_first, r"\text{−–}");
        assert_eq!(text_first, roundtrip(&text_first));
    }

    fn normalized_latex(input: &str) -> String {
        let config = Config::default();
        let mut parsed = LatexReader::parse_compact(input, &config).unwrap();
        crate::ast::normalize::normalize_compact_expr(&mut parsed.expr).unwrap();
        LatexWriter::write_compact(&parsed.expr, &config).unwrap()
    }

    #[test]
    fn literal_bracket_after_a_row_break_is_not_a_spacing_argument() {
        for (input, expected) in [
            (
                r"\begin{array}{c}a\\{[}b]\end{array}",
                r"\begin{array}{c} a \\ {}[b] \end{array}",
            ),
            (
                r"\xymatrix{a\\[b]c}",
                r"\begin{array}{c} a \\ {}[b]c \end{array}",
            ),
            (
                r"\begin{matrix}a\\{[}b]\end{matrix}",
                r"\begin{matrix} a \\ {}[b] \end{matrix}",
            ),
            (
                r"\begin{aligned}a&=b\\{[}c]&=d\end{aligned}",
                r"\begin{aligned} a & = b \\ {}[c] & = d \end{aligned}",
            ),
            (r"a\\{[}b]", r"a\\ {}[b]"),
            // `\hline` already ends the break, and later cells are unaffected.
            (
                r"\begin{array}{|c|}\hline a\\\hline{[}b]\end{array}",
                r"\begin{array}{|c|} \hline a \\ \hline [b] \end{array}",
            ),
            (
                r"\begin{array}{cc}a&b\\&{[}d]\end{array}",
                r"\begin{array}{cc} a & b \\  & [d] \end{array}",
            ),
        ] {
            let first = normalized_latex(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(normalized_latex(&first), first, "{input}");
            assert!(first.contains('['), "bracket lost for {input}");
        }
    }

    #[test]
    fn literal_bracket_after_a_fenced_row_break_is_not_a_spacing_argument() {
        for (input, expected) in [
            (r"\left(a\\{[}b]\right)", r"\left( a\\ {}[b] \right)"),
            (
                r"\left(a\middle|b\\{[}c]\right)",
                r"\left( a \middle| b\\ {}[c] \right)",
            ),
        ] {
            let first = normalized_latex(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(normalized_latex(&first), first, "{input}");
        }
    }

    #[test]
    fn braced_line_break_in_a_table_cell_stays_a_line_break() {
        for (input, expected) in [
            (
                r"\begin{array}{c}{\large a\\}\end{array}",
                r"\begin{array}{c} {\large a\\ } \end{array}",
            ),
            (
                r"\begin{matrix}{{a\\}}\\b\end{matrix}",
                r"\begin{matrix} {a\\ } \\ b \end{matrix}",
            ),
            (
                r"\begin{array}{cc}x&{a\\b}c\end{array}",
                r"\begin{array}{cc} x & {a\\ b}c \end{array}",
            ),
            // An unbraced break is a row separator and needs no group.
            (
                r"\begin{array}{c}\large a\\b\end{array}",
                r"\begin{array}{c} \large a \\ b \end{array}",
            ),
        ] {
            let first = normalized_latex(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(normalized_latex(&first), first, "{input}");
        }
    }

    #[test]
    fn literal_star_after_a_row_break_is_not_a_starred_break() {
        for (input, expected) in [
            (
                r"\begin{array}{c}a\\\relax*b\end{array}",
                r"\begin{array}{c} a \\ \relax * b \end{array}",
            ),
            (
                r"\begin{matrix}a\\\relax*^2b\end{matrix}",
                r"\begin{matrix} a \\ \relax *^{2}b \end{matrix}",
            ),
            (r"a\\\relax*b", r"a\\ \relax * b"),
            (r"\left(a\\\relax*b\right)", r"\left( a\\ \relax *b \right)"),
            // A star later in the row is not at the break.
            (
                r"\begin{array}{c}a\\b*c\end{array}",
                r"\begin{array}{c} a \\ b* c \end{array}",
            ),
        ] {
            let first = normalized_latex(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(normalized_latex(&first), first, "{input}");
        }
    }

    #[test]
    fn ordinary_middle_delimiter_keeps_compact_roundtrip_spacing() {
        let first = normalized_latex(r" ' \middle \mid\midfle' oe'");
        assert_eq!(first, normalized_latex(&first));
        assert_eq!(
            first,
            r"{{}^{\prime}\mathord{\mid}}{\mathord{\text{\textbackslash{}midfle}}}^{\prime}o{e}^{\prime}"
        );
    }

    #[test]
    fn literal_unicode_script_glyphs_keep_their_ordinary_atom() {
        for glyph in ['³', '₂', '⁺', 'ᵃ', 'ᵦ'] {
            for (prefix, suffix) in [("", "f"), ("x", "7"), ("", "^2")] {
                let literal = format!(r"{prefix}\char`{glyph}{suffix}");
                let middle = format!(r"{prefix}\middle{glyph}{suffix}");
                let canonical = normalized_latex(&middle);
                assert_eq!(canonical, normalized_latex(&literal), "{middle}");
                assert!(
                    canonical.contains(&format!(r"\char`{glyph}")),
                    "{canonical}"
                );
                assert_eq!(canonical, normalized_latex(&canonical));
                let xml = |s: &str| crate::from_latex(s).unwrap().to_mathml().unwrap();
                assert_eq!(xml(&middle), xml(&canonical), "{middle}");
            }
        }
        let xml = r"<math><mi>x</mi><mo>³</mo><mi>f</mi></math>";
        let ordinary = crate::from_mathml(xml).unwrap();
        let tex = ordinary.to_latex().unwrap();
        assert_eq!(
            ordinary.to_mathml().unwrap(),
            crate::from_latex(&tex).unwrap().to_mathml().unwrap()
        );
        let id = crate::from_mathml("<math><mi>x₂</mi></math>")
            .unwrap()
            .to_latex()
            .unwrap();
        assert_eq!(normalized_latex(&id), id);

        assert_eq!(normalized_latex("x³"), normalized_latex("x^3"));
        let fenced = r"\left(x\middle³f\right)";
        let canonical = normalized_latex(fenced);
        assert!(canonical.contains(r"\middle³"), "{canonical}");
        assert_eq!(canonical, normalized_latex(&canonical));
    }

    #[test]
    fn unfenced_middle_minus_uses_the_math_minus_glyph() {
        for (input, literal) in [
            (r"\intop\middle -!", r"\intop-!"),
            (r"x\middle -y", "x-y"),
            (r"x^{\middle -}", "x^{-}"),
        ] {
            let canonical = normalized_latex(input);
            assert_eq!(canonical, normalized_latex(literal), "{input}");
            assert_eq!(canonical, normalized_latex(&canonical), "{input}");
            assert_eq!(
                crate::from_latex(input).unwrap().to_mathml().unwrap(),
                crate::from_latex(literal).unwrap().to_mathml().unwrap(),
                "{input}"
            );
        }
    }

    #[test]
    fn middle_delimiters_follow_the_enclosing_fence_across_normalization() {
        for (input, fence_count) in [
            (r"{uw//suA\middle|3}1", 0),
            (r"{a\middle|b}c", 0),
            (r"\left( a\middle|b \right){c\middle|d}", 1),
            (r"\left(\frac{a\middle|b}{c}\right)", 1),
            (
                r"\left[\left(x\middle|y\right)\frac{a\middle|b}{c}\right]",
                2,
            ),
        ] {
            let first = normalized_latex(input);
            assert_eq!(first.matches(r"\middle").count(), fence_count, "{input}");
            assert_eq!(first, normalized_latex(&first), "{input}");
        }
    }

    #[test]
    fn binary_minus_script_bases_use_one_canonical_spelling() {
        for (input, expected) in [
            ("-'", r"-^{\prime}"),
            ("\u{2212}'", r"{\mathord{-}}^{\prime}"),
            (r"\middle-'", r"-^{\prime}"),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected, "unexpected spelling for {input:?}");
            assert_eq!(first, roundtrip(&first), "unstable spelling for {input:?}");
        }

        for (minus, expected) in [
            ("<mo>−</mo>", r"-^{\prime}"),
            (
                r#"<mo data-mjx-texclass="ORD">−</mo>"#,
                r"{\mathord{-}}^{\prime}",
            ),
        ] {
            let xml = format!("<math><msup>{minus}<mo>′</mo></msup></math>");
            let first = crate::from_mathml(&xml).unwrap().to_latex().unwrap();
            assert_eq!(first, expected, "{xml}");
            assert_eq!(
                first,
                crate::from_latex(&first).unwrap().to_latex().unwrap()
            );
        }
    }

    #[test]
    fn ordinary_minus_script_bases_keep_their_math_class() {
        let config = Config::default();
        let expr = MathExpr::inline(vec![MathNode::Superscript {
            base: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{2212}".into())),
            sup: Box::new(MathNode::Number("2".into())),
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let first = LatexWriter::write_compact(&compact, &config).unwrap();
        assert_eq!(first, r"{\mathord{-}}^{2}");
        assert_eq!(first, roundtrip(&first));
    }

    #[test]
    fn dagger_commands_preserve_math_class() {
        for (ordinary, text, binary) in [
            (r"\dag", r"\textdagger", r"\dagger"),
            (r"\ddag", r"\textdaggerdbl", r"\ddagger"),
        ] {
            for (input, expected) in [
                (format!("a{text}*b"), format!("a{ordinary}* b")),
                (format!("a{ordinary}*b"), format!("a{ordinary}* b")),
                (format!("a{binary}*b"), format!("a{binary} * b")),
            ] {
                let first = roundtrip(&input);
                assert_eq!(first, expected);
                assert_eq!(roundtrip(&first), first);
            }
        }
    }

    #[test]
    fn backslash_before_mathord_commands_is_stable() {
        for (input, expected) in [
            ("\\textbackslash\u{2062}A", r"\char`\\\mathord{}A"),
            ("\\textbackslash\u{2212}A", r"\char`\\\mathord{-}A"),
            ("\\textbackslash A", r"\char`\\A"),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected);
            assert_eq!(first, roundtrip(&first));
        }
    }

    #[test]
    fn math_mode_literal_ascii_symbols_keep_ordinary_atoms() {
        let config = Config::default();
        let cases = [
            (r"\textasciitilde", "~", r"\char`\~"),
            (r"\textasciicircum", "^", r"\char`\^"),
            (r"\textquotesingle", "'", r"\char`'"),
            (r"\textless", "<", r"\char`<"),
            (r"\textgreater", ">", r"\char`>"),
            (r"\textbar", "|", r"\char`|"),
        ];
        for (source, literal, expected) in cases {
            let direct =
                MathExpr::inline(vec![MathNode::Symbol(SymbolKind::Ordinary, literal.into())]);
            let output = LatexWriter::write(&direct, &config).unwrap();
            assert_eq!(output, expected, "direct symbol: {source:?}");
            let reparsed = LatexReader::parse(&output, &config).unwrap();
            assert_eq!(
                reparsed.expr.nodes, direct.nodes,
                "direct symbol changed after writing: {source:?}"
            );

            let parsed = LatexReader::parse(source, &config).unwrap();
            assert_eq!(parsed.expr.nodes, direct.nodes, "source: {source:?}");
            let command_output = LatexWriter::write(&parsed.expr, &config).unwrap();
            assert_eq!(command_output, expected, "literal command: {source:?}");
            assert_eq!(
                LatexWriter::write(
                    &LatexReader::parse(&command_output, &config).unwrap().expr,
                    &config,
                )
                .unwrap(),
                expected,
                "not idempotent: {source:?}"
            );
        }

        for (literal, expected) in [
            ("_", r"\char`\_"),
            ("{", r"\char`\{"),
            ("}", r"\char`\}"),
            ("\\", r"\char`\\"),
        ] {
            let direct =
                MathExpr::inline(vec![MathNode::Symbol(SymbolKind::Ordinary, literal.into())]);
            let output = LatexWriter::write(&direct, &config).unwrap();
            assert_eq!(output, expected, "direct symbol: {literal:?}");
            assert_eq!(
                LatexReader::parse(&output, &config).unwrap().expr.nodes,
                direct.nodes,
                "direct symbol changed after writing: {literal:?}"
            );
        }

        for (literal, expected) in [
            ("~", r"\char`\~"),
            ("^", r"\char`\^"),
            ("'", r"\char`'"),
            ("<", r"\char`<"),
            (">", r"\char`>"),
            ("|", r"\char`|"),
        ] {
            let expr = MathExpr::inline(vec![MathNode::Identifier(literal.into())]);
            let output = LatexWriter::write(&expr, &config).unwrap();
            assert_eq!(output, expected, "direct identifier: {literal:?}");
            assert_eq!(
                LatexWriter::write(&LatexReader::parse(&output, &config).unwrap().expr, &config,)
                    .unwrap(),
                expected,
                "identifier output was not stable: {literal:?}"
            );
        }

        // The text writer remains text-mode aware; these must not be changed
        // into math `\char` nodes merely because their glyphs overlap.
        let text = MathExpr::inline(vec![MathNode::Text(
            TextStyle::Normal,
            "~^'|<>_\\{}".into(),
        )]);
        let text_output = LatexWriter::write(&text, &config).unwrap();
        assert!(text_output.starts_with(r"\text{"), "{text_output}");
        assert!(text_output.contains(r"\textasciitilde{}"), "{text_output}");
        assert!(text_output.contains(r"\textasciicircum{}"), "{text_output}");
        assert!(!text_output.contains(r"\char`"), "{text_output}");
    }

    #[test]
    fn control_symbols_use_bounded_numeric_char_tokens() {
        let config = Config::default();
        for (code, expected) in [
            ('\u{0000}', r"\char0 "),
            ('\u{0002}', r"\char2 "),
            ('\u{001f}', r"\char31 "),
            ('\u{007f}', r"\char127 "),
            ('\u{0085}', r"\char133 "),
        ] {
            let expr = MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::Ordinary,
                code.to_string(),
            )]);
            let output = LatexWriter::write(&expr, &config).unwrap();
            assert_eq!(output, expected, "control U+{:04X}", code as u32);
            let reparsed = LatexReader::parse(&output, &config).unwrap();
            assert_eq!(
                reparsed.expr.nodes, expr.nodes,
                "control U+{:04X}",
                code as u32
            );
            assert_eq!(
                LatexWriter::write(&reparsed.expr, &config).unwrap(),
                expected,
                "control U+{:04X} changed after a second write",
                code as u32
            );
        }

        for source in [
            r"\char2 34",
            r"\char2a",
            r"\char2\alpha",
            r"\char2^3",
            r"\char2_3^4",
            r#"\char"7f 8"#,
        ] {
            let first = roundtrip(source);
            let second = roundtrip(&first);
            assert_eq!(first, second, "source {source:?} was not stable");
            assert_eq!(
                crate::from_latex(source).unwrap().to_mathml().unwrap(),
                crate::from_latex(&first).unwrap().to_mathml().unwrap(),
                "source {source:?} changed its math structure as {first:?}",
            );
        }
    }
    #[test]
    fn math_mode_literal_symbols_survive_public_formats_and_scripts() {
        for (input, expected) in [
            (r"\textasciitilde", r"\char`\~"),
            (r"\textasciicircum", r"\char`\^"),
            (r"\textquotesingle", r"\char`'"),
            (r"\textless", r"\char`<"),
            (r"\textgreater", r"\char`>"),
            (r"\textbar", r"\char`|"),
        ] {
            let output = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX)
                .expect("LaTeX output should succeed");
            assert_eq!(output, expected, "{input}");
            assert_eq!(
                crate::convert(&output, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
                expected,
                "not stable: {input}"
            );

            let mathml = crate::convert(input, crate::Format::LaTeX, crate::Format::MathML)
                .expect("MathML output should succeed");
            assert!(
                mathml.contains(match input {
                    r"\textasciitilde" => "~",
                    r"\textasciicircum" => "^",
                    r"\textquotesingle" => "&apos;",
                    r"\textless" => "&lt;",
                    r"\textgreater" => "&gt;",
                    r"\textbar" => "|",
                    _ => unreachable!(),
                }),
                "glyph was lost for {input}: {mathml}"
            );
        }

        // Script bases and arguments must remain ordinary symbols, while an
        // actual prime remains U+2032 and relation/binary tokens retain their
        // spacing classes.
        for (input, expected) in [
            (r"\textasciitilde^2", r"{\char`\~}^{2}"),
            (r"x_{\textasciicircum}", r"x_{\char`\^}"),
            (r"\textquotesingle^2", r"{\char`'}^{2}"),
            (r"x'", r"x^{\prime}"),
            ("a+b", "a+ b"),
            ("a<b", "a< b"),
        ] {
            let output = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX)
                .expect("LaTeX output should succeed");
            assert_eq!(output, expected, "{input}");
            assert_eq!(
                crate::convert(&output, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
                expected,
                "not stable: {input}"
            );
        }

        let mathml = crate::from_mathml("<math><mo>~</mo><mo>&lt;</mo><mo>|</mo></math>")
            .unwrap()
            .to_latex()
            .unwrap();
        assert_eq!(mathml, r"\char`\~< |");
    }

    #[test]
    fn invisible_operators_use_stable_zero_width_boundaries() {
        for invisible in ['\u{2061}', '\u{2062}', '\u{2063}', '\u{2064}'] {
            for input in [
                format!("A{invisible}"),
                format!("A{invisible}B"),
                format!(r"\sin{invisible}x"),
            ] {
                let first = roundtrip(&input);
                let second = roundtrip(&first);
                assert_eq!(
                    first, second,
                    "invisible operator was not stable: {input:?}"
                );
            }
        }

        // A trailing invisible operator must not leave whitespace that is
        // trimmed before the next parse/write pass.
        assert_eq!(roundtrip("A\u{2062}"), r"A\mathord{}");
        assert_eq!(roundtrip("\u{2062}A"), r"\mathord{}A");
        assert_eq!(roundtrip("\u{2062}"), r"\mathord{}");
        // A middle invisible operator must keep adjacent identifiers from
        // merging into one token while remaining zero width.
        assert_eq!(roundtrip("A\u{2062}B"), r"A\mathord{}B");
        // Scripts on an invisible atom must not attach to the preceding
        // identifier when the output is parsed again.
        assert_eq!(roundtrip("A\u{2062}^2"), r"A{\mathord{}}^{2}");
    }

    #[test]
    fn arbitrary_space_widths_round_trip_without_thickspace_quantization() {
        for width in [
            Rational::new(1, 10),
            Rational::new(-1, 10),
            Rational::new(7, 20),
            Rational::new(13, 100),
        ] {
            let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![
                MathNode::Identifier("a".into()),
                MathNode::Space(width),
                MathNode::Identifier("b".into()),
            ]))
            .unwrap();
            let first = LatexWriter::write_compact(&compact, &Config::default()).unwrap();
            let reparsed = LatexReader::parse_compact(&first, &Config::default()).unwrap();
            assert_eq!(
                reparsed.expr.to_math_expr().nodes,
                compact.to_math_expr().nodes,
                "{first}"
            );
            assert!(first.contains(r"\hspace{"), "{first}");
        }
    }

    #[test]
    fn unicode_math_spaces_use_exact_stable_latex_spacing() {
        let config = Config::default();
        for &(space, command, width) in math_space_cases().iter() {
            let raw = MathExpr::inline(vec![MathNode::Symbol(
                SymbolKind::Ordinary,
                space.to_string(),
            )]);
            assert_eq!(LatexWriter::write(&raw, &config).unwrap(), command);

            // The empty `$` prefix writes no content, leaving the space first in
            // the output where raw Unicode whitespace was previously trimmed.
            let first = roundtrip(&format!("${space}-"));
            assert_eq!(first, format!("{command}-"), "space: {space:?}");

            let parsed = LatexReader::parse(&first, &config).unwrap();
            assert!(
                parsed.expr.nodes.first().is_some_and(|node| match node {
                    MathNode::Space(value) | MathNode::ExplicitSpace { width: value, .. } =>
                        *value == width,
                    _ => false,
                }),
                "space width changed: {space:?}"
            );
            assert_eq!(roundtrip(&first), first, "space: {space:?}");

            // A binary operator before the space must not add a separator on
            // only the first pass.
            let parsed = LatexReader::parse(&format!("-{space}"), &config).unwrap();
            assert_eq!(parsed.expr.nodes.len(), 2, "{:?}", parsed.expr.nodes);
            assert!(
                parsed.expr.nodes.last().is_some_and(|node| match node {
                    MathNode::Space(value) | MathNode::ExplicitSpace { width: value, .. } =>
                        *value == width,
                    _ => false,
                }),
                "literal space was not classified: {space:?}"
            );
            let trailing = roundtrip(&format!("-{space}"));
            assert_eq!(trailing, format!("-{command}"), "space: {space:?}");
            assert_eq!(roundtrip(&trailing), trailing, "space: {space:?}");
        }
    }

    #[test]
    fn unicode_four_per_em_space_has_literal_quarter_em_width() {
        assert_eq!(
            crate::unicode::math_space_width('\u{2005}'),
            Some(Rational::new(1, 4))
        );
        assert_eq!(roundtrip("\u{2005}"), r"\ ");
    }

    #[test]
    fn mathml_embedded_unicode_math_spaces_are_stable_in_math_leaves() {
        for &(space, command, _) in math_space_cases().iter() {
            for (tag, before, after) in [("mo", "𝔸", "𝔹"), ("mi", "𝔸", "𝔹"), ("mn", "1", "2")]
            {
                let cases = [
                    (
                        format!("{before}{space}{after}"),
                        format!("{before}{command}{after}"),
                    ),
                    (
                        format!("{before}{space}{space}{after}"),
                        format!("{before}{command}{command}{after}"),
                    ),
                ];
                for (content, expected) in cases {
                    let input = format!("<math><{tag}>{content}</{tag}></math>");
                    let first = crate::convert(&input, crate::Format::MathML, crate::Format::LaTeX)
                        .unwrap();
                    assert_eq!(first, expected, "{tag} space: {space:?}");
                    let second =
                        crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
                    assert_eq!(second, first, "{tag} space: {space:?}");
                }
            }
        }
    }

    #[test]
    fn mixed_math_leaf_runs_keep_operator_and_identifier_semantics() {
        let cases = [
            (
                "<math><mo>lim\u{2004}sup</mo></math>",
                r"\lim\mkern6mu \sup",
            ),
            (
                "<math><mi>α\u{2004}beta</mi></math>",
                r"\alpha\mkern6mu beta",
            ),
            (
                "<math><mi>α\u{2004}βγ</mi></math>",
                r"\alpha\mkern6mu \beta\gamma",
            ),
            ("<math><mi>αx</mi></math>", r"\alpha x"),
            ("<math><mi>αx</mi><mi>y</mi></math>", r"\alpha xy"),
            ("<math><mi>aβ</mi><mi>y</mi></math>", r"a\beta y"),
        ];
        for (input, expected) in cases {
            let first = crate::convert(input, crate::Format::MathML, crate::Format::LaTeX)
                .expect("MathML input should convert");
            assert_eq!(first, expected, "input: {input}");
            let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX)
                .expect("LaTeX output should reparse");
            assert_eq!(second, first, "input: {input}");
        }
    }

    #[test]
    fn control_word_boundaries_survive_empty_and_nested_rows() {
        let cases = [
            (
                "<math><mrow><mi>α</mi><mrow></mrow><mi>a</mi><mi>b</mi></mrow></math>",
                r"\alpha ab",
            ),
            (
                "<math><mfenced separators=''><mi>α</mi><mi>a</mi><mi>b</mi></mfenced></math>",
                r"\left( \alpha ab \right)",
            ),
            (
                "<math><mfenced separators=''><mi>α</mi><mrow><mi>a</mi><mi>b</mi></mrow></mfenced></math>",
                r"\left( \alpha ab \right)",
            ),
        ];
        for (input, expected) in cases {
            let first = crate::convert(input, crate::Format::MathML, crate::Format::LaTeX)
                .expect("MathML input should convert");
            let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX)
                .expect("LaTeX output should reparse");
            assert_eq!(first, second, "control-word boundary changed for {input}");
            assert_eq!(first, expected, "unexpected output for {input}");
        }

        for nodes in [
            vec![
                MathNode::Symbol(SymbolKind::Ordinary, "α".into()),
                MathNode::Identifier("a".into()),
                MathNode::Identifier("b".into()),
            ],
            vec![
                MathNode::Symbol(SymbolKind::Ordinary, "α".into()),
                MathNode::Row(vec![]),
                MathNode::Identifier("a".into()),
                MathNode::Identifier("b".into()),
            ],
        ] {
            let expr = MathExpr::inline(nodes);
            let output = LatexWriter::write(&expr, &Config::default()).unwrap();
            assert_eq!(output, r"\alpha ab");
        }
    }

    #[test]
    fn embedded_math_space_edges_are_stable_in_ast_leaves() {
        let command = r"\mkern6mu ";
        let cases = [
            (
                MathNode::Symbol(
                    SymbolKind::Ordinary,
                    "\u{2004}𝔸\u{2004}\u{2004}𝔹\u{2004}".into(),
                ),
                format!("{command}𝔸{command}{command}𝔹{command}"),
            ),
            (
                MathNode::Identifier("\u{2004}𝔸\u{2004}\u{2004}𝔹\u{2004}".into()),
                format!("{command}𝔸{command}{command}𝔹{command}"),
            ),
            (
                MathNode::Number("\u{2004}1\u{2004}\u{2004}2\u{2004}".into()),
                format!("{command}1{command}{command}2{command}"),
            ),
        ];
        let config = Config::default();
        for (node, expected) in cases {
            let first = LatexWriter::write(&MathExpr::inline(vec![node]), &config).unwrap();
            assert_eq!(first, expected);
            let second =
                crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
            assert_eq!(second, first);
        }
    }

    #[test]
    fn font_relative_unicode_spaces_remain_literal_and_stable() {
        for space in ['\u{2007}', '\u{2008}'] {
            let first = roundtrip(&format!("${space}-"));
            assert_eq!(first, format!("{space}-"), "space: {space:?}");
            assert_eq!(roundtrip(&first), first, "space: {space:?}");

            for (tag, before, after) in [("mo", "𝔸", "𝔹"), ("mi", "𝔸", "𝔹"), ("mn", "1", "2")]
            {
                let content = format!("{before}{space}{space}{after}");
                let input = format!("<math><{tag}>{content}</{tag}></math>");
                let first =
                    crate::convert(&input, crate::Format::MathML, crate::Format::LaTeX).unwrap();
                assert_eq!(first, content, "{tag} space: {space:?}");
                let second =
                    crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
                assert_eq!(second, first, "{tag} space: {space:?}");
            }
        }
    }

    #[test]
    fn decoded_text_round_trips_without_reinterpreting_literals() {
        let config = Config::default();
        let literal = r"---''~^\{}#%_$&x";
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, literal.into())]);
        let latex = LatexWriter::write(&expr, &config).unwrap();
        assert_eq!(
            LatexReader::parse(&latex, &config).unwrap().expr.nodes,
            expr.nodes
        );
        assert_eq!(roundtrip(&latex), latex);

        for (input, literal) in [
            (r"\text\textbackslash", "\\"),
            (r"\text\textasciitilde", "~"),
            (r"\text\textasciicircum", "^"),
            (r"\text{\\{x}}", " x"),
        ] {
            let output = roundtrip(input);
            assert_eq!(
                LatexReader::parse(&output, &config).unwrap().expr.nodes,
                vec![MathNode::Text(TextStyle::Normal, literal.into())],
                "{input}"
            );
            assert_eq!(roundtrip(&output), output);
        }
    }

    #[test]
    fn relax_and_text_control_words_follow_tex_lexing() {
        for (input, expected) in [
            (r"\relax", ""),
            (r"x\relax", "x"),
            (r"\relax x", "x"),
            (r"x^\relax", "x"),
            (r"x_\relax", "x"),
            (r"x^{\relax}", "x"),
            (r"x_{\relax}", "x"),
            (r"\relax^2", r"{}^{2}"),
            // Scanner-level \relax is skipped in an unbraced script argument;
            // the following atom therefore remains the superscript.
            (r"x^\relax y", "x^y"),
            (r"x^\relax_2", r"x_{2}"),
            (r"\text{a\relax b}", r"\text{ab}"),
            (r"\text{a \relax b}", r"\text{a b}"),
            (r"\text{a\relax\ b}", r"\text{a b}"),
            (r"\text{a\relax{ }b}", r"\text{a b}"),
            (r"\hbox{a\relax b}", r"\text{ab}"),
            (r"\text{\KaTeX}", r"\text{KaTeX}"),
            (r"\text{a\KaTeX b}", r"\text{aKaTeXb}"),
            (r"\text{a \KaTeX b}", r"\text{a KaTeXb}"),
            (r"\text{a\KaTeX\ b}", r"\text{aKaTeX b}"),
            (r"\text{a\KaTeX{ }b}", r"\text{aKaTeX b}"),
            (r"\text{\textellipsis}", r"\ldots"),
            (r"\text{a\textellipsis b}", r"{\text{a}\ldots\text{b}}"),
            (r"\text{a \textellipsis b}", r"{\text{a }\ldots\text{b}}"),
            (r"\text{a\textellipsis\ b}", r"{\text{a}\ldots\text{ b}}"),
            (r"\kern2\relax", r"\kern2pt "),
            (r"\kern2\relax em", r"\kern2pt em"),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected, "unexpected output for {input:?}");
            assert_eq!(
                first,
                roundtrip(&first),
                "output was not stable for {input:?}"
            );
        }

        // The ellipsis command must retain its glyph rather than disappearing
        // as an unknown text command.
        let mathml = crate::convert(
            r"\text{a\textellipsis b}",
            crate::Format::LaTeX,
            crate::Format::MathML,
        )
        .unwrap();
        assert!(mathml.contains('…'), "ellipsis glyph was lost: {mathml}");
    }

    #[test]
    fn text_mode_math_spaces_are_not_split() {
        let first = crate::convert(
            "<math><mtext>𝔸\u{2004}𝔹</mtext></math>",
            crate::Format::MathML,
            crate::Format::LaTeX,
        )
        .unwrap();
        assert_eq!(first, "\\text{𝔸\u{2004}𝔹}");
        let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(second, first);
    }

    #[test]
    fn text() {
        assert_eq!(roundtrip("\\text{hello}"), "\\text{hello}");
    }

    #[test]
    fn sum_with_limits() {
        let output = roundtrip("\\sum_{i=0}^{n} a");
        assert!(output.contains("\\sum"));
        assert!(output.contains("_"));
        assert!(output.contains("^"));
    }

    #[test]
    fn hat_accent() {
        assert_eq!(roundtrip("\\hat{x}"), "\\hat{x}");
    }

    #[test]
    fn phantom() {
        assert_eq!(roundtrip("\\phantom{x}"), "\\phantom{x}");
    }

    #[test]
    fn under_accents_roundtrip() {
        // Below accents must not collapse to \hat (regression: catch-all bug).
        assert_eq!(roundtrip("\\underrightarrow{x}"), "\\underrightarrow{x}");
        assert_eq!(roundtrip("\\underleftarrow{x}"), "\\underleftarrow{x}");
        assert_eq!(
            roundtrip("\\underleftrightarrow{x}"),
            "\\underleftrightarrow{x}"
        );
        assert_eq!(roundtrip("\\utilde{x}"), "\\utilde{x}");
    }

    #[test]
    fn varinjlim_varprojlim_roundtrip() {
        // amsmath \varinjlim / \varprojlim expand to under-arrow accents over \lim.
        assert_eq!(
            roundtrip(r"\varinjlim"),
            r"\operatorname*{\underrightarrow{lim}}"
        );
        assert_eq!(
            roundtrip(r"\varprojlim"),
            r"\operatorname*{\underleftarrow{lim}}"
        );
    }

    #[test]
    fn over_accents_roundtrip() {
        // These Above accents previously fell through to \hat.
        assert_eq!(roundtrip("\\overrightarrow{x}"), "\\overrightarrow{x}");
        assert_eq!(roundtrip("\\overleftarrow{x}"), "\\overleftarrow{x}");
        assert_eq!(roundtrip("\\dddot{x}"), "\\dddot{x}");
        assert_eq!(roundtrip("\\ddddot{x}"), "\\ddddot{x}");
        assert_eq!(roundtrip("\\mathring{x}"), "\\mathring{x}");
    }

    #[test]
    fn brackets_remain_brackets_and_limits_remain_limits() {
        for (input, expected) in [
            (r"\overbracket{x}", r"\overbracket{x}"),
            (r"\underbracket{x}", r"\underbracket{x}"),
            (
                r"\overbracket{\mathop{f+g}}",
                r"\overbracket{\mathop{f+ g}}",
            ),
            (r"\overlinesegment{x}", r"\overlinesegment{x}"),
            (r"\underlinesegment{x}", r"\underlinesegment{x}"),
            (r"\overbracket{x}^2", r"\overbracket{x}^{2}"),
            (r"\underbracket{x}_2", r"\underbracket{x}_{2}"),
        ] {
            assert_eq!(roundtrip(input), expected, "{input}");
            assert_eq!(roundtrip(expected), expected, "{input}");
        }
        assert_eq!(roundtrip(r"\underbracket\\"), r"\underbracket{\\ }");

        let limit = roundtrip(r"\mathop{a+b}\limits^⎴");
        assert!(limit.contains(r"\mathop{a+ b}\limits^{⎴}"), "{limit}");
        assert!(!limit.contains("overbracket"), "{limit}");
    }

    #[test]
    fn boxed() {
        assert_eq!(roundtrip("\\boxed{x}"), "\\boxed{x}");
    }

    #[test]
    fn colorbox_roundtrip() {
        assert_eq!(roundtrip("\\colorbox{teal}{B}"), r"\colorbox{teal}{B}");
        assert_eq!(
            roundtrip("\\colorbox{#ff0000}{B}"),
            r"\colorbox{#ff0000}{B}"
        );
        assert_eq!(roundtrip(r"\colorbox{#f00}{B}"), r"\colorbox{#f00}{B}");
        assert_eq!(
            portable_roundtrip(r"\colorbox{#ff0000}{B}"),
            r"\colorbox[HTML]{ff0000}{\ensuremath{B}}"
        );
    }

    #[test]
    fn fcolorbox_roundtrip() {
        assert_eq!(
            roundtrip("\\fcolorbox{blue}{red}{C}"),
            r"\fcolorbox{blue}{red}{C}"
        );
        for (input, expected) in [
            (
                r"\fcolorbox{red}{#ff0000}{C}",
                r"\fcolorbox{red}{#ff0000}{C}",
            ),
            (
                r"\fcolorbox{#ff0000}{red}{C}",
                r"\fcolorbox{#ff0000}{red}{C}",
            ),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(roundtrip(&first), first, "{input}");
        }
        assert_eq!(
            portable_roundtrip(r"\fcolorbox{red}{#ff0000}{C}"),
            r"\fcolorbox{red}[HTML]{ff0000}{\ensuremath{C}}"
        );
    }

    #[test]
    fn unnormalized_empty_group_script_bases_are_stable() {
        for (input, expected) in [
            ("\u{2113}{}^2", r"\ell{}^{2}"),
            (r"\ell{}^2", r"\ell{}^{2}"),
            (r"\Finv{}^2", r"\Finv{}^{2}"),
            (r"\textohm{}^2", r"\Omega{}^{2}"),
        ] {
            let out1 = crate::from_latex(input).unwrap().to_latex().unwrap();
            let out2 = crate::from_latex(&out1).unwrap().to_latex().unwrap();
            assert_eq!(out1, expected, "non-canonical output for {input:?}");
            assert_eq!(out1, out2, "unnormalized round-trip failed for {input:?}");
        }
    }

    #[test]
    fn direct_compact_row_script_bases_use_token_rules() {
        let config = Config::default();
        for (base, expected) in [
            (MathNode::Identifier("\u{2113}".into()), r"\ell^{2}"),
            (MathNode::Identifier("\u{211C}".into()), r"\Re^{2}"),
            (MathNode::Identifier("\u{03B1}".into()), r"\alpha^{2}"),
            (MathNode::Identifier(":".into()), ":^{2}"),
            (
                MathNode::Symbol(SymbolKind::Ordinary, "\u{2132}".into()),
                r"\Finv^{2}",
            ),
        ] {
            let expr =
                CompactExpr::from_math_expr(&MathExpr::inline(vec![MathNode::Superscript {
                    base: Box::new(MathNode::Row(vec![base])),
                    sup: Box::new(MathNode::Number("2".into())),
                }]))
                .unwrap();
            let out1 = LatexWriter::write_compact(&expr, &config).unwrap();
            let reparsed = LatexReader::parse_compact(&out1, &config).unwrap();
            let out2 = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
            assert_eq!(out1, expected);
            assert_eq!(out1, out2);
        }
    }

    #[test]
    fn empty_row_before_trailing_empty_base_script_is_safe() {
        let config = Config::default();
        let expr = MathExpr::inline(vec![
            MathNode::Row(Vec::new()),
            MathNode::Superscript {
                base: Box::new(MathNode::Row(Vec::new())),
                sup: Box::new(MathNode::Number("2".into())),
            },
        ]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let out1 = LatexWriter::write_compact(&compact, &config).unwrap();
        let reparsed = LatexReader::parse_compact(&out1, &config).unwrap();
        let out2 = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
        assert_eq!(out1, "{}^{2}");
        assert_eq!(out1, out2);
    }

    #[test]
    fn empty_nary_body_with_following_empty_base_script_is_stable() {
        let config = Config::default();
        let valid = crate::from_latex(r"\int{}_{=}^{\prime}").unwrap();
        let valid_latex = valid.to_latex().unwrap();
        assert_eq!(
            valid.to_mathml().unwrap(),
            crate::from_latex(&valid_latex)
                .unwrap()
                .to_mathml()
                .unwrap()
        );
        for input in [
            r"\int$$_p_'?",
            "'\n=\r\\intop\r\n\r\r\r\n\n$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$'_\n=\r\\intop\r\n\r\r\r\r\n\r\r\r\n\n\r\n\n\n\r_'?",
        ] {
            let mut parsed = LatexReader::parse_compact(input, &config).unwrap();
            crate::ast::normalize::normalize_compact_expr(&mut parsed.expr).unwrap();
            let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
            let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
            crate::ast::normalize::normalize_compact_expr(&mut reparsed.expr).unwrap();
            let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
            assert_eq!(
                first, second,
                "unstable n-ary empty-body output for {input:?}"
            );
        }
    }

    #[test]
    fn effectively_empty_color_script_argument_is_omitted() {
        let config = Config::default();
        let input = ",_v76_\\color{\n5sf\n_v}";

        let mut compact = LatexReader::parse_compact(input, &config).unwrap().expr;
        crate::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        let first = LatexWriter::write_compact(&compact, &config).unwrap();
        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap().expr;
        crate::ast::normalize::normalize_compact_expr(&mut reparsed).unwrap();
        let second = LatexWriter::write_compact(&reparsed, &config).unwrap();
        assert_eq!(first, ",_v76");
        assert_eq!(first, second);

        let heap = LatexReader::parse(input, &config).unwrap();
        assert_eq!(LatexWriter::write(&heap.expr, &config).unwrap(), ",_v76");

        for (input, expected) in [
            (r"x_\color{red}y", r"x_{\textcolor{red}{y}}"),
            (
                r"x_\color{red}\mathord{}",
                r"x_{\textcolor{red}{\mathord{}}}",
            ),
            ("x_\\ ", r"x_{\ }"),
        ] {
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            let second = crate::from_latex(&first).unwrap().to_latex().unwrap();
            assert_eq!(first, expected, "unexpected script output for {input:?}");
            assert_eq!(first, second, "script output was not stable for {input:?}");
        }
    }

    #[test]
    fn multicolumn_groups_are_only_interpreted_at_table_cell_boundaries() {
        let canonical = |input: &str| {
            let config = Config::default().with_undefined_commands(crate::UndefinedCommands::Keep);
            let mut expr = LatexReader::parse_compact(input, &config).unwrap().expr;
            crate::ast::normalize::normalize_compact_expr(&mut expr).unwrap();
            LatexWriter::write_compact(&expr, &config).unwrap()
        };
        for (source, overrides) in [
            (r"x\multicolumn{1}{r}{\kern1pt y}+z", 0),
            (
                r"\begin{array}{l}a\\\multicolumn{1}{r}{\kern1pt y}\end{array}",
                1,
            ),
            (
                r"\begin{array}{l}\frac{x}{\multicolumn{1}{r}{ab}}\end{array}",
                0,
            ),
        ] {
            let first = canonical(source);
            // A misplaced `\multicolumn` is kept as an undefined command,
            // braced with its groups.
            let misplaced = first.matches(r"{\multicolumn").count();
            assert_eq!(
                first.matches(r"\multicolumn").count() - misplaced,
                overrides,
                "{first}"
            );
            assert_eq!(misplaced, 1 - overrides, "{first}");
            assert_eq!(canonical(&first), first);
        }
    }

    #[test]
    #[cfg(feature = "render")]
    fn positive_multicolumn_span_renders_without_a_second_cell() {
        let source = r"\begin{array}{ll}a&b\\\multicolumn{2}{r}{LONG}\end{array}";
        let svg = crate::from_latex(source).unwrap().to_svg().unwrap();
        assert!(svg.contains("<use"), "{svg}");
        let width = svg
            .split_once("width=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .and_then(|(value, _)| value.trim_end_matches("px").parse::<f64>().ok())
            .unwrap();
        assert!(width > 30.0, "{width}");
    }

    #[test]
    fn multicolumn_boundary_order_and_multiplicity_survive_latex_roundtrip() {
        let config = Config::default();
        for (source, expected_spec, expected_left, expected_right) in [
            (
                r"\begin{array}{l}\multicolumn{1}{@{}|l}{x}\end{array}",
                r"\multicolumn{1}{@{}|l}",
                vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
                Vec::new(),
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{|@{}l}{x}\end{array}",
                r"\multicolumn{1}{|@{}l}",
                vec![SpanBoundary::Rule, SpanBoundary::SuppressGlue],
                Vec::new(),
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{||l}{x}\end{array}",
                r"\multicolumn{1}{||l}",
                vec![SpanBoundary::Rule, SpanBoundary::Rule],
                Vec::new(),
            ),
            (
                r"\begin{array}{l}\multicolumn{1}{l|@{}}{x}\end{array}",
                r"\multicolumn{1}{l|@{}}",
                Vec::new(),
                vec![SpanBoundary::Rule, SpanBoundary::SuppressGlue],
            ),
        ] {
            let compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            let root = compact.arena.children(compact.roots)[0];
            let CompactNode::Array { cells, .. } = *compact.arena.node(root) else {
                panic!("expected array");
            };
            let cell = compact.arena.children(cells)[0];
            let CompactNode::SpannedCell {
                left_boundaries,
                right_boundaries,
                ..
            } = *compact.arena.node(cell)
            else {
                panic!("expected spanned cell");
            };
            assert_eq!(
                parse_span_boundary_spelling(compact.arena.str_val(left_boundaries)),
                expected_left,
                "left boundary order changed for {source}"
            );
            assert_eq!(
                parse_span_boundary_spelling(compact.arena.str_val(right_boundaries)),
                expected_right,
                "right boundary order changed for {source}"
            );
            let first = LatexWriter::write_compact(&compact, &config).unwrap();
            assert_eq!(
                first,
                LatexWriter::write_compact(
                    &LatexReader::parse_compact(&first, &config).unwrap().expr,
                    &config,
                )
                .unwrap()
            );
            assert!(first.contains(expected_spec), "{first}");
        }
        let long_rules = "|".repeat(64);
        let long_source =
            format!(r"\begin{{array}}{{l}}\multicolumn{{1}}{{{long_rules}l}}{{x}}\end{{array}}");
        let long = LatexReader::parse_compact(&long_source, &config)
            .unwrap()
            .expr;
        let long_out = LatexWriter::write_compact(&long, &config).unwrap();
        assert_eq!(long_out.matches('|').count(), 64);
    }

    #[test]
    fn nested_empty_color_script_roundtrip_stays_canonical() {
        let config = Config::default();
        let input = r"A\teal^\teal^2";
        let mut compact = LatexReader::parse_compact(input, &config).unwrap().expr;
        crate::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        let first = LatexWriter::write_compact(&compact, &config).unwrap();
        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap().expr;
        crate::ast::normalize::normalize_compact_expr(&mut reparsed).unwrap();
        let second = LatexWriter::write_compact(&reparsed, &config).unwrap();
        // The double superscript's `^2` keeps the empty base it was read with.
        assert_eq!(first, r"A\textcolor{teal}{}{}^{2}");
        assert_eq!(first, second);

        let heap_first = crate::from_latex(input).unwrap().to_latex().unwrap();
        let heap_second = crate::from_latex(&heap_first).unwrap().to_latex().unwrap();
        assert_eq!(heap_first, first);
        assert_eq!(heap_first, heap_second);
    }

    #[test]
    fn scripts_without_effective_arguments_reuse_the_normal_base_writer() {
        for (input, expected) in [
            (r"\teal^\teal", r"\textcolor{teal}{}"),
            (r"\teal_\teal", r"\textcolor{teal}{}"),
            (r"\teal_\teal^\teal", r"\textcolor{teal}{}"),
            (r"x^{}", "x"),
            (r"x_{}^{}", "x"),
            (r"z \teal^\teal", r"z\textcolor{teal}{}"),
            (r"\teal^x", r"{\textcolor{teal}{}}^x"),
        ] {
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            let second = crate::from_latex(&first).unwrap().to_latex().unwrap();
            assert_eq!(first, expected, "unexpected script output for {input:?}");
            assert_eq!(first, second, "script output was not stable for {input:?}");
        }
    }

    #[test]
    fn elided_scripts_attach_one_trailing_implicit_script() {
        for (input, expected) in [
            // The double superscript's last script keeps its empty base.
            (
                r"\teal^\teal^\utilde'",
                r"\textcolor{teal}{}{}^{\utilde{\prime}}",
            ),
            (r"x^{}^\utilde'", r"x^{\utilde{\prime}}"),
            (
                r"\color{red}{}^\utilde'",
                r"{\textcolor{red}{}}^{\utilde{\prime}}",
            ),
        ] {
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            let second = crate::from_latex(&first).unwrap().to_latex().unwrap();
            assert_eq!(first, expected, "unexpected trailing script for {input:?}");
            assert_eq!(
                first, second,
                "trailing script was not stable for {input:?}"
            );
        }

        for input in [r"a \teal^\teal^\utilde' b", r"a x^{}^\utilde' b"] {
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            let second = crate::from_latex(&first).unwrap().to_latex().unwrap();
            assert_eq!(first, second, "surrounding sibling changed {input:?}");
        }
    }

    #[test]
    fn script_arguments_keep_preferred_command_grouping() {
        for (input, expected) in [
            ("x_\u{211C}", r"x_\Re"),
            (r"x_\Re", r"x_\Re"),
            ("x_\u{211C}b", r"x_\Re b"),
        ] {
            let first = crate::from_latex(input).unwrap().to_latex().unwrap();
            let second = crate::from_latex(&first).unwrap().to_latex().unwrap();
            assert_eq!(first, expected, "unexpected script argument for {input:?}");
            assert_eq!(
                first, second,
                "script argument was not stable for {input:?}"
            );
        }

        let input = "'_\u{211C}?'_\u{211C}";
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, r"{{}_{\Re}^{\prime}?_\Re^{\prime}}");
        assert_eq!(first, second, "normalized script arguments were not stable");

        let mathml =
            crate::from_mathml("<math><msub><mi>x</mi><mo>\u{211C}</mo></msub></math>").unwrap();
        let first = mathml.to_latex().unwrap();
        assert_eq!(first, r"x_\Re");
        assert_eq!(
            first,
            crate::from_latex(&first).unwrap().to_latex().unwrap()
        );
    }

    #[test]
    fn direct_ast_script_symbol_uses_preferred_command() {
        let config = Config::default();
        let expr = MathExpr::inline(vec![MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{211C}".into())),
        }]);
        let first = LatexWriter::write(&expr, &config).unwrap();
        assert_eq!(first, r"x_\Re");
        assert_eq!(
            first,
            crate::from_latex(&first).unwrap().to_latex().unwrap()
        );
    }

    #[test]
    fn reflected_math_groups_have_stable_latex_spelling() {
        for input in [r"\reflectbox{$x^2$}", r"\mathreflectbox{\frac{a}{b}}"] {
            let first = roundtrip(input);
            let command = if input.starts_with(r"\reflectbox") {
                r"\reflectbox{"
            } else {
                r"\mathreflectbox{"
            };
            assert!(first.starts_with(command), "{first:?}");
            assert_eq!(first, roundtrip(&first));
        }
    }

    #[test]
    fn reflection_keeps_the_argument_mode_and_inline_math() {
        for (input, expected) in [
            (r"\reflectbox{abc}", r"\reflectbox{abc}"),
            (r"\reflectbox{$x^2$}", r"\reflectbox{$x^{2}$}"),
            (
                r"\reflectbox{pay \$5 or $x^2$}",
                r"\reflectbox{pay \$5 or $x^{2}$}",
            ),
            (
                r"\reflectbox{a\reflectbox{b}c}",
                r"\reflectbox{a\reflectbox{b}c}",
            ),
            (r"\mathreflectbox{x^2}", r"\mathreflectbox{x^{2}}"),
            (r"\mathreflectbox λ", r"\mathreflectbox{\lambda}"),
        ] {
            let first = crate::from_latex(input).unwrap();
            assert_eq!(first.to_latex().unwrap(), expected, "{input}");
            let second = crate::from_latex(expected).unwrap();
            assert_eq!(
                second.to_mathml().unwrap(),
                first.to_mathml().unwrap(),
                "{input}"
            );
        }
        assert!(crate::from_latex(r"\reflectbox{x^2}").is_err());
    }

    #[test]
    fn reflected_mathml_requires_an_actual_transform() {
        for (style, reflected) in [
            ("transform: scaleX( -1.0 )", true),
            ("--note: scaleX(-1)", false),
            ("transform:scaleX(-1) rotate(90deg)", false),
            ("transform:scaleX(-1);transform:none", false),
            ("transform:scaleX(-1) !important; transform:none", true),
        ] {
            let ml = format!(r#"<math><mrow style="{style}"><mi>x</mi></mrow></math>"#);
            let tex = crate::convert(&ml, crate::Format::MathML, crate::Format::LaTeX).unwrap();
            assert_eq!(
                tex.contains(r"\mathreflectbox"),
                reflected,
                "{style}: {tex}"
            );
        }
    }

    #[test]
    fn reflected_modes_round_trip_through_mathml_metadata() {
        for (input, mode, command) in [
            (r"\reflectbox{abc}", "text", r"\reflectbox{"),
            (r"\mathreflectbox{x^2}", "math", r"\mathreflectbox{"),
        ] {
            let mathml =
                crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap();
            assert!(mathml.contains(&format!("data-mathema-reflect=\"{mode}\"")));
            let latex =
                crate::convert(&mathml, crate::Format::MathML, crate::Format::LaTeX).unwrap();
            assert!(latex.starts_with(command), "{latex:?}");
            assert_eq!(
                latex,
                crate::convert(&latex, crate::Format::LaTeX, crate::Format::LaTeX).unwrap()
            );
        }
        let style_only = r#"<math><mrow style="display:inline-block; transform:scaleX(-1)"><mi>x</mi></mrow></math>"#;
        let latex =
            crate::convert(style_only, crate::Format::MathML, crate::Format::LaTeX).unwrap();
        assert!(latex.starts_with(r"\mathreflectbox{"), "{latex:?}");
    }

    #[test]
    fn multi_character_operators_keep_every_character() {
        // Each `<mo>` used to be spelled from its first character alone, so
        // `:=` became `:` and `≈:` became `\approx`.
        for (mo, glyphs, expected) in [
            (":=", ":=", r"a\mathord{:=}b"),
            ("=:", "=:", r"a\mathrel{=:} b"),
            ("\u{2248}:", "\u{2248}:", "a\\mathrel{\u{2248}:} b"),
            ("\u{2212}:", "\u{2212}:", "a\\mathbin{\u{2212}:} b"),
            (
                "\u{2260}\u{2260}",
                "\u{2260}\u{2260}",
                "a\\mathrel{\u{2260}\u{2260}} b",
            ),
            ("{&amp;}", "{&amp;}", r"a\mathopen{\{\&\}}b"),
        ] {
            let xml = format!("<math><mi>a</mi><mo>{mo}</mo><mi>b</mi></math>");
            let latex = crate::from_mathml(&xml).unwrap().to_latex().unwrap();
            assert_eq!(latex, expected, "{mo}");
            // The LaTeX reader splits the class group into one atom per
            // glyph, so the first reparse is a fixed point that still carries
            // every character.
            let reparsed = crate::from_latex(&latex).unwrap();
            let glyph_text: String = reparsed
                .to_mathml()
                .unwrap()
                .split('<')
                .filter_map(|tag| tag.split_once('>').map(|(_, text)| text))
                .collect();
            assert!(
                glyph_text.contains(&format!("a{glyphs}b")),
                "{mo}: {glyph_text}"
            );
            let canonical = reparsed.to_latex().unwrap();
            let again = crate::from_latex(&canonical).unwrap().to_latex().unwrap();
            assert_eq!(again, canonical, "{mo} is not stable");
        }
    }

    #[test]
    fn compound_symbols_escape_and_keep_their_class() {
        // Direct trees: the readers never build these, but writers must not
        // emit unparsable or lossy LaTeX for them.
        for (kind, text) in [
            (SymbolKind::Ordinary, "\"=\""),
            (SymbolKind::Relation, "a b"),
            (SymbolKind::Ordinary, "\u{2236}\u{2248}"),
            (SymbolKind::Ordinary, "\u{2248}\u{0338}"),
        ] {
            let expr = CompactExpr::from_math_expr(&MathExpr::inline(vec![MathNode::Symbol(
                kind,
                text.into(),
            )]))
            .unwrap();
            let latex = LatexWriter::write_compact(&expr, &Config::default()).unwrap();
            let reparsed = crate::from_latex(&latex)
                .unwrap_or_else(|e| panic!("{text:?} wrote unparsable {latex:?}: {e}"));
            let glyphs: String = reparsed
                .to_mathml()
                .unwrap()
                .split('<')
                .filter_map(|tag| tag.split_once('>').map(|(_, text)| text))
                .collect::<String>()
                .replace("&quot;", "\"");
            assert!(glyphs.contains(text), "{text:?} via {latex:?}: {glyphs}");
            assert!(
                latex.starts_with('\\'),
                "{text:?} must stay one atom: {latex:?}"
            );
        }
    }

    #[test]
    fn writer_never_emits_commands_katex_lacks() {
        // bboldx's `\bbone` and textcomp's `\textmu` are not KaTeX math commands.
        for glyph in ["\u{1D7D9}", "\u{00B5}"] {
            let xml = format!("<math><mi>{glyph}</mi><mo>{glyph}</mo></math>");
            let latex = crate::from_mathml(&xml).unwrap().to_latex().unwrap();
            assert!(
                !latex.contains(r"\bb") && !latex.contains(r"\text"),
                "{latex}"
            );
        }
    }

    #[test]
    fn big_delimiters_keep_their_class_suffix() {
        for (input, expected) in [
            (
                r"\bigl\llbracket a\bigr\rrbracket",
                r"\bigl\llbracket a\bigr\rrbracket",
            ),
            (r"\Bigl\lBrace x\Bigr\rBrace", r"\Bigl\lBrace x\Bigr\rBrace"),
            (r"\bigl⟦ a\bigr⟧", r"\bigl\llbracket a\bigr\rrbracket"),
            (r"\biggl( x\biggr)", r"\biggl(x\biggr)"),
            (r"a\bigm| b", r"a\bigm| b"),
            (r"\Biggm\| x", r"\Biggm\| x"),
            (r"\big( x\big)", r"\big(x\big)"),
            (r"\bigl. x\bigr|", r"\bigl.x\bigr|"),
            (r"\big\vert", r"\big|"),
            (r"\big\lvert x\big\rvert", r"\big\lvert x\big\rvert"),
            (r"\big\backslash", r"\big\backslash"),
            (r"\big\uparrow", r"\big\uparrow"),
            (r"\big<", r"\big\langle"),
            (r"\mathopen{\big(}x\mathclose{\big)}", r"\bigl(x\bigr)"),
            (r"\big\Arrowvert\big\arrowvert", r"\big\|\big\arrowvert"),
            (r"\Bigl\bracevert x", r"\Bigl\bracevert x"),
            (
                r"\big\ulcorner x\big\urcorner",
                r"\big\ulcorner x\big\urcorner",
            ),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(roundtrip(&first), first, "{input}");
        }
    }

    #[test]
    fn left_delimiters_use_their_command_names() {
        for (input, expected) in [
            (
                r"\left\ulcorner x\right\urcorner",
                r"\left\ulcorner x \right\urcorner",
            ),
            (r"\left⟦ a\right⟧x", r"\left\llbracket a \right\rrbracket x"),
            (
                r"\left\Arrowvert x\right\bracevert",
                r"\left\| x \right\bracevert",
            ),
            (
                r"\left\langle x\middle⦀ y\right.",
                r"\left\langle x \middle⦀ y \right.",
            ),
        ] {
            let first = roundtrip(input);
            assert_eq!(first, expected, "{input}");
            assert_eq!(roundtrip(&first), first, "{input}");
        }
    }
}
