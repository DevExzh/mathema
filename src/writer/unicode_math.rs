use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::style::MathStyle;
use crate::ast::visit::{CompactVisitor, walk_compact_node};
use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Result, TypesettingPolicy};
use crate::writer::Writer as MathWriter;

/// Classifies a compact node that the UnicodeMath writer cannot fully or
/// round-trippably represent. UnicodeMath is a lossy linear notation.
fn unicode_math_loss(arena: &AstArena, node: &CompactNode) -> Option<String> {
    match node {
        CompactNode::Styled(TextStyle::Normal, span)
            if styled_content_has_identifier(arena, *span) => Some(
                "UnicodeMath cannot represent an upright math alphabet for an identifier; the content was written without its upright style".into(),
            ),
        CompactNode::Space(width) if unicode_space_is_approximate(*width) => Some(format!(
            "UnicodeMath cannot represent explicit space width {width}em exactly; the output uses a bounded approximation"
        )),
        CompactNode::Reflect { .. } => {
            Some("UnicodeMath writes the content of a reflected box without mirroring it".into())
        },
        CompactNode::Rule { .. } => {
            Some("UnicodeMath cannot represent a rule (\\rule/\\hline); it was dropped".into())
        },
        CompactNode::Cancel { .. } => Some(
            "UnicodeMath cannot represent a cancellation stroke; only the content is written"
                .into(),
        ),
        CompactNode::Boxed { .. } => Some(
            "UnicodeMath box (▭) round-trips structurally but drops background/border color".into(),
        ),
        CompactNode::Phantom(..) => Some(
            "UnicodeMath phantom (⟡) round-trips structurally but drops the phantom kind (v/h)"
                .into(),
        ),
        CompactNode::Color { .. } => {
            Some("UnicodeMath cannot represent color; only the content is written".into())
        },
        CompactNode::Scaled { .. } => {
            Some("UnicodeMath does not preserve the \\scalebox factor".into())
        },
        CompactNode::RaiseBox { .. } => {
            Some("UnicodeMath does not preserve the \\raisebox shift".into())
        },
        CompactNode::MathChoice { .. } => {
            Some("UnicodeMath keeps only the branch of \\mathchoice active at this style".into())
        },
        CompactNode::StructuredOperator { .. } => Some(
            "UnicodeMath has no source-level StructuredOperator marker; the body and representable bounds were written with standard grouping and script syntax".into(),
        ),
        CompactNode::Array { .. } => table_loss(arena, node),
        CompactNode::MathAtom { .. } => {
            Some("UnicodeMath does not preserve the math atom spacing class".into())
        },
        CompactNode::Symbol(kind, sid) => symbol_loss(*kind, arena.str_val(*sid)),
        CompactNode::Overscript { over, movable_limits: true, .. } if !is_overunder_line(arena, *over) => Some(
            "UnicodeMath uses a superscript for a movable upper bound; placement depends on its base".into(),
        ),
        CompactNode::Underscript { under, movable_limits: true, .. } if !is_overunder_line(arena, *under) => Some(
            "UnicodeMath uses a subscript for a movable lower bound; placement depends on its base".into(),
        ),
        _ => None,
    }
}

fn unicode_space_is_approximate(width: Rational) -> bool {
    if width.num < 0 {
        return true;
    }
    width.num != 0
        && unicode_space_units(width).is_none_or(|units| unicode_space_parts(units).is_none())
}

/// Whether a styled run contains an identifier whose upright math alphabet is
/// observable in presentation MathML. Ordinary variables are italic by
/// default, while UnicodeMath has no general escape for an upright Latin or
/// Greek identifier. Symbols and numbers do not need this warning because
/// their UnicodeMath spelling already carries the relevant glyph semantics.
fn styled_content_has_identifier(arena: &AstArena, span: crate::ast::compact::NodeSpan) -> bool {
    arena
        .children(span)
        .iter()
        .copied()
        .any(|id| styled_node_has_identifier(arena, id))
}

fn styled_node_has_identifier(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(_) => true,
        CompactNode::Row(inner) => styled_content_has_identifier(arena, inner),
        CompactNode::Styled(TextStyle::Normal, inner) => {
            styled_content_has_identifier(arena, inner)
        },
        CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. } => {
            styled_node_has_identifier(arena, content)
        },
        _ => false,
    }
}

/// UnicodeMath reads an operator character with the spacing of its glyph.
fn symbol_loss(kind: SymbolKind, text: &str) -> Option<String> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        // Even an operator followed by a combining accent becomes two AST nodes.
        return Some(
            "UnicodeMath cannot preserve a multi-character symbol as one operator token".into(),
        );
    }
    let read = match ch {
        '/' | '?' | '!' => SymbolKind::Ordinary,
        '.' => SymbolKind::Punctuation,
        // Paired bracket endpoints are handled by the enclosing sequence.
        '@' | '(' | ')' | '[' | ']' | '{' | '}' => {
            return Some(
                "UnicodeMath reads this symbol as text and loses its math atom spacing class"
                    .into(),
            );
        },
        _ => crate::unicode::symbols::glyph_class(ch)?,
    };
    (read != kind).then(|| "UnicodeMath does not preserve the math atom spacing class".into())
}

pub struct UnicodeMathWriter;

const ZERO_WIDTH_SPACE: char = '\u{200B}';

impl UnicodeMathWriter {
    pub(crate) fn compact_diagnostics(expr: &CompactExpr, config: &Config) -> Vec<Diagnostic> {
        let mut visitor = UnicodeMathLossVisitor::default();
        visitor.visit_expr(expr);
        let mut diagnostics = visitor.diagnostics;
        if expr.arena.has_attrs() && config.typesetting == TypesettingPolicy::Preserve {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Info,
                message: "UnicodeMath has no font sizes; they were dropped".into(),
                span: None,
            });
        }
        diagnostics
    }

    /// Fast internal bridge for an expression returned by a parser or checked
    /// conversion result. Public trait calls retain strict arena validation.
    pub(crate) fn write_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<String> {
        super::validate_compact_trusted(expr, config)?;
        write_compact_validated(expr)
    }

    /// Trusted diagnostic compact writer used by the parse-once conversion
    /// result.
    #[allow(dead_code)]
    pub(crate) fn write_compact_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        let out = write_compact_validated(expr)?;
        Ok((out, Self::compact_diagnostics(expr, config)))
    }
}

#[derive(Default)]
struct UnicodeMathLossVisitor {
    diagnostics: Vec<Diagnostic>,
}

impl UnicodeMathLossVisitor {
    fn visit_sequence(&mut self, arena: &AstArena, ids: &[NodeId]) {
        let mut opens: SmallVec<[(&str, NodeId); 4]> = SmallVec::new();
        for &id in ids {
            match *arena.node(id) {
                CompactNode::Symbol(SymbolKind::Open, sid)
                    if matches!(arena.str_val(sid), "(" | "[" | "{") =>
                {
                    opens.push((arena.str_val(sid), id));
                },
                CompactNode::Symbol(SymbolKind::Close, sid)
                    if opens
                        .last()
                        .is_some_and(|&(open, _)| is_bracket_pair(open, arena.str_val(sid))) =>
                {
                    opens.pop();
                },
                _ => self.visit_node(arena, id),
            }
        }
        for (_, id) in opens {
            self.visit_node(arena, id);
        }
    }
}

impl CompactVisitor for UnicodeMathLossVisitor {
    fn visit_expr(&mut self, expr: &CompactExpr) {
        self.visit_sequence(&expr.arena, expr.arena.children(expr.roots));
    }

    fn visit_node(&mut self, arena: &AstArena, id: NodeId) {
        let node = arena.node(id);
        if let Some(message) = unicode_math_loss(arena, node) {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Warning,
                message,
                span: None,
            });
        }
        match *node {
            CompactNode::Row(span)
            | CompactNode::Styled(_, span)
            | CompactNode::Fenced { content: span, .. } => {
                self.visit_sequence(arena, arena.children(span));
            },
            _ => walk_compact_node(self, arena, id),
        }
    }
}

fn write_unicode_space(out: &mut String, width: Rational) {
    // UnicodeMath has no negative space (LaTeX `\!`): it is dropped rather
    // than written as a positive one. The zero-width space (UTN #28 section
    // 3.16) keeps the letters around it apart.
    if width.num <= 0 {
        out.push(ZERO_WIDTH_SPACE);
        return;
    }
    // The one-em character is repeatable.  Emit an exact sequence whenever
    // the width is representable by the UTN #28 space characters, so a
    // `\qquad` (2em) does not silently become a single em.
    if let Some(units) = unicode_space_units(width)
        && write_exact_unicode_space(out, units)
    {
        return;
    }
    let em = width.as_f64();
    if em >= 1.0 {
        out.push('\u{2003}');
    } else if em >= 0.5 {
        out.push('\u{2002}');
    } else {
        out.push('\u{2009}');
    }
}

/// Write a non-negative width in thirty-sixths of an em. The available
/// UnicodeMath widths are 1/18, 1/6, 2/9, 5/18, 1/3, 1/4, 1/2 and 1 em.
/// The bounded decomposition keeps the common one-character spellings
/// unchanged.
fn write_exact_unicode_space(out: &mut String, units: u32) -> bool {
    let Some(parts) = unicode_space_parts(units) else {
        return false;
    };

    for glyph in parts {
        out.push(glyph);
    }
    true
}

fn unicode_space_units(width: Rational) -> Option<u32> {
    if width.num <= 0 {
        return None;
    }
    let scaled = i128::from(width.num) * 36;
    let denominator = i128::from(width.den);
    if scaled % denominator != 0 {
        return None;
    }
    let units = scaled / denominator;
    (units <= i128::from(36 * 64)).then_some(u32::try_from(units).ok()?)
}

/// Find a short exact spelling for a positive width in thirty-sixths of an
/// em. Most widths split into whole ems plus a sub-em remainder. An odd
/// remainder just over an em can borrow one whole em, as in 37/36em = 1/2 +
/// 1/4 + 5/18. The dynamic program only covers the small remainder range.
fn unicode_space_parts(units: u32) -> Option<SmallVec<[char; 8]>> {
    let whole = units / 36;
    let remainder = units % 36;
    let mut result = SmallVec::new();
    if let Some(parts) = unicode_space_remainder(remainder) {
        result.extend(std::iter::repeat_n('\u{2003}', whole as usize));
        result.extend(parts);
        return Some(result);
    }
    if whole == 0 {
        return None;
    }
    let parts = unicode_space_remainder(remainder + 36)?;
    result.extend(std::iter::repeat_n('\u{2003}', (whole - 1) as usize));
    result.extend(parts);
    Some(result)
}

fn unicode_space_remainder(units: u32) -> Option<SmallVec<[char; 8]>> {
    const WIDTHS: &[(usize, char)] = &[
        (18, '\u{2002}'), // en space, 1/2 em
        (12, '\u{2004}'), // three-per-em space, 1/3 em
        (10, '\u{2005}'), // four-per-em space, 5/18 em
        (9, '\u{00A0}'),  // no-break space, 1/4 em
        (8, '\u{205F}'),  // medium mathematical space, 2/9 em
        (6, '\u{2009}'),  // thin space, 1/6 em
        (2, '\u{200A}'),  // hair space, 1/18 em
    ];
    let target = usize::try_from(units).ok()?;
    if target > 71 {
        return None;
    }
    let mut best = [u8::MAX; 72];
    let mut predecessor = [u8::MAX; 72];
    best[0] = 0;
    for value in 1..=target {
        for (index, (width, _)) in WIDTHS.iter().enumerate() {
            if *width <= value && best[value - width] != u8::MAX {
                let candidate = best[value - width] + 1;
                if candidate < best[value] {
                    best[value] = candidate;
                    predecessor[value] = index as u8;
                }
            }
        }
    }
    if best[target] == u8::MAX {
        return None;
    }
    let mut result = SmallVec::new();
    let mut value = target;
    while value > 0 {
        let index = usize::from(predecessor[value]);
        let (width, glyph) = WIDTHS[index];
        result.push(glyph);
        value -= width;
    }
    result.reverse();
    Some(result)
}

impl MathWriter for UnicodeMathWriter {
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        super::validate_heap_for_write(expr, config)?;
        // UnicodeMath has no font sizes: the compact writer looks past them.
        if crate::ast::has_attributes(expr) {
            let compact = CompactExpr::from_math_expr_after_validation(expr)?;
            return write_compact_validated(&compact);
        }
        let mut out = String::with_capacity(256);
        write_nodes_with_style(&mut out, &expr.nodes, MathStyle::from(expr.display));
        Ok(out)
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_for_write(expr, config)?;
        let out = write_compact_validated(expr);
        let out = out?;
        Ok((out, Self::compact_diagnostics(expr, config)))
    }

    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        super::validate_compact_for_write(expr, config)?;
        write_compact_validated(expr)
    }
}

fn write_compact_validated(expr: &CompactExpr) -> Result<String> {
    let mut out = String::with_capacity(256);
    let arena = &expr.arena;
    write_nodes_c_with_style(&mut out, arena, expr.roots, MathStyle::from(expr.display));
    Ok(out)
}

fn table_loss(arena: &AstArena, table: &CompactNode) -> Option<String> {
    let CompactNode::Array {
        cells,
        num_cols,
        col_aligns,
        array_style,
        col_seps,
        row_seps,
        ..
    } = *table
    else {
        return None;
    };
    let columns = arena.children(col_aligns);
    let stride = usize::from(num_cols.max(1));
    let mut alignment_lost = false;
    let mut spans_lost = false;
    let mut edges_lost = false;
    for row in arena.children(cells).chunks(stride) {
        for (index, &cell) in row.iter().enumerate() {
            let declared = columns
                .get(index)
                .and_then(|&id| match *arena.node(id) {
                    CompactNode::ColAlign(align) => Some(align),
                    _ => None,
                })
                .unwrap_or(ColumnAlign::Center);
            alignment_lost |= super::effective_cell_alignment(arena, cell, declared)
                .is_some_and(|align| align != ColumnAlign::Center);
            if let CompactNode::SpannedCell {
                span,
                left_boundaries,
                right_boundaries,
                ..
            } = *arena.node(cell)
            {
                spans_lost |= arena.effective_cell_span_in_row(row, index, span) > 1;
                edges_lost |= !arena.str_val(left_boundaries).is_empty()
                    || !arena.str_val(right_boundaries).is_empty();
            }
        }
    }
    let mut messages = [
        (
            alignment_lost,
            "UnicodeMath centers table columns with left or right cell alignment.",
        ),
        (
            spans_lost,
            "UnicodeMath expands a spanning cell into separate matrix entries.",
        ),
        (edges_lost, "UnicodeMath omits per-cell boundary material."),
        (
            col_seps != 0 || row_seps != 0,
            "UnicodeMath omits table rules.",
        ),
        (array_style, "UnicodeMath omits TeX outer padding."),
    ]
    .into_iter()
    .filter_map(|(lost, message)| lost.then_some(message));
    let mut message = messages.next()?.to_owned();
    for rest in messages {
        message.push(' ');
        message.push_str(rest);
    }
    Some(message)
}

/// Forced bounds keep their position in inline math. Movable bounds use
/// ordinary scripts; the UnicodeMath function name determines their layout.
fn write_bound(
    out: &mut String,
    position: AccentPosition,
    movable: bool,
    line: bool,
    write_arg: impl FnOnce(&mut String),
) {
    let mark = match (position, movable, line) {
        (AccentPosition::Above, _, true) => '\u{0305}',
        (AccentPosition::Below, _, true) => '\u{0332}',
        (AccentPosition::Above, true, false) => '^',
        (AccentPosition::Below, true, false) => '_',
        (AccentPosition::Above, false, false) => '┴',
        (AccentPosition::Below, false, false) => '┬',
    };
    out.push(mark);
    if !line {
        write_arg(out);
    }
}

/// An undefined LaTeX command's name as quoted text. UnicodeMath text has no
/// escape for its quote, so a `"` in a name that is no control word (only
/// possible through the API) is written as a full-width quote.
fn write_undefined_command_name(out: &mut String, name: &str) {
    out.push('"');
    for ch in name.chars() {
        out.push(if ch == '"' { '\u{FF02}' } else { ch });
    }
    out.push('"');
}

/// Quoted text, with `\"` for a quote and `\\` for a backslash in it (UTN
/// #28 sections 3.2 and 3.15).
fn write_quoted_text(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

fn write_node(out: &mut String, node: &MathNode, style: MathStyle) {
    match node {
        MathNode::Number(s) => out.push_str(s),
        MathNode::Identifier(s) => write_identifier(out, s),
        MathNode::Operator(attr) => out.push_str(&attr.name),
        // A literal solidus is quoted: `/` alone is the fraction operator
        // (UTN #28 sections 2.1 and 3.2).
        MathNode::Symbol(_, s) if s == "/" => out.push_str("\\/"),
        MathNode::Symbol(_, s) => out.push_str(s),
        MathNode::Text(_, s) => write_quoted_text(out, s),
        MathNode::StyleSwitch(_) | MathNode::SizeSwitch(_) | MathNode::LineBreak => {},
        MathNode::Attributed { content, .. } => write_node(out, content, style),
        MathNode::Space(r) => {
            write_unicode_space(out, *r);
        },
        MathNode::ExplicitSpace { width, basis } => {
            write_unicode_space(
                out,
                crate::ast::style::local_space_width(*width, Some(*basis), style),
            );
        },
        MathNode::Rule { .. } => {},

        MathNode::Row(children) => write_nodes_with_style(out, children, style),
        // Plain text, then each possible argument in parentheses.
        MathNode::UndefinedCommand { name, arguments } => {
            write_undefined_command_name(out, name);
            for argument in arguments {
                out.push('(');
                write_nodes_with_style(out, argument, style);
                out.push(')');
            }
        },

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            if open == "{"
                && close.is_empty()
                && let Some(array) = cases_array(content)
            {
                // UTN #28's cases operator keeps the table structure while
                // avoiding the unmatched left brace that the LaTeX shape
                // uses internally (`Ⓒ(value&condition@…)`).
                write_heap_table(out, array, 'Ⓒ');
                return;
            }
            out.push_str(open);
            let start = out.len();
            let mut segment: SmallVec<[&MathNode; 8]> = SmallVec::new();
            for item in content {
                match item {
                    FencedItem::Element(node) => segment.push(node),
                    // A `\middle` delimiter starts a new segment, with the
                    // fence's style.
                    FencedItem::Middle(delim) => {
                        write_nodes_tracking(out, &segment, style, None);
                        segment.clear();
                        out.push_str(delim);
                    },
                }
            }
            write_nodes_tracking(out, &segment, style, None);
            if let Some(FencedItem::Element(first)) = content.first()
                && operand_edge(first) == OperandEdge::Fraction
            {
                separate_bar_content(out, start);
            }
            out.push_str(close);
        },

        MathNode::Subscript { base, sub } => {
            write_script_base(out, base, style);
            if has_forced_operator_limits(base, style) {
                out.push('┬');
            } else {
                out.push('_');
            }
            write_script_arg(out, sub, style.sub());
        },

        MathNode::Superscript { base, sup } => {
            write_script_base(out, base, style);
            if !has_forced_operator_limits(base, style)
                && let Some((primes, rest)) = prime_superscript(base, sup)
            {
                // UTN #28 section 3.10: `x′`, `x′′`, `x′^c` (LaTeX `x'^c`).
                for prime in primes {
                    write_node(out, prime, style.sup());
                }
                if let Some(rest) = rest {
                    out.push('^');
                    write_script_arg(out, rest, style.sup());
                }
                return;
            }
            out.push(if has_forced_operator_limits(base, style) {
                '┴'
            } else {
                '^'
            });
            write_script_arg(out, sup, style.sup());
        },

        MathNode::SubSup { base, sub, sup } => {
            // `x′_1^(n-1)`: primes inside the superscript would merge into
            // its row.
            if !has_forced_operator_limits(base, style)
                && let Some((primes, Some(rest))) = prime_superscript(base, sup)
                && !matches!(
                    select_mathchoice(rest, style),
                    MathNode::Identifier(_) | MathNode::Number(_) | MathNode::Symbol(..)
                )
            {
                write_script_base(out, base, style);
                for prime in primes {
                    write_node(out, prime, style.sup());
                }
                out.push('_');
                write_script_arg(out, sub, style.sub());
                out.push('^');
                write_script_arg(out, rest, style.sup());
                return;
            }
            write_script_base(out, base, style);
            out.push(if has_forced_operator_limits(base, style) {
                '┬'
            } else {
                '_'
            });
            write_script_arg(out, sub, style.sub());
            out.push(if has_forced_operator_limits(base, style) {
                '┴'
            } else {
                '^'
            });
            write_script_arg(out, sup, style.sup());
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // UTN #28 section 3.3: `(_sub^sup)base`.
            out.push('(');
            if let Some(sub) = pre_sub {
                out.push('_');
                write_script_arg(out, sub, style.sub());
            }
            if let Some(sup) = pre_sup {
                out.push('^');
                write_script_arg(out, sup, style.sup());
            }
            out.push(')');
            // The reader takes one operand as the base: group a word, a row
            // or a fraction (`(_1)〖xy〗`), and `(_1)` with nothing after it
            // is not a prescript.
            let grouped = match select_mathchoice(base, style) {
                MathNode::Identifier(name) => name.chars().nth(1).is_some(),
                MathNode::Row(children) => !children.is_empty(),
                // `(_1)a/b` is a fraction with a prescripted numerator.
                MathNode::Fraction { .. } => true,
                _ => false,
            };
            let start = out.len();
            if grouped {
                out.push('〖');
            }
            // Its n-aryand must end with the base (`(^′)∑ 〖log〗 x`).
            let selected = select_mathchoice(base, style);
            if matches!(selected, MathNode::Nary { .. }) {
                write_nary(out, selected, style, true);
            } else {
                write_node(out, base, style);
            }
            if grouped {
                out.push('〗');
            } else {
                group_null_operand(out, start, false);
            }
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            write_bound_base(out, base, style);
            write_bound(
                out,
                AccentPosition::Above,
                *movable_limits,
                is_overunder_line_node(over),
                |out| write_script_arg(out, over, style.sup()),
            );
        },
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            write_bound_base(out, base, style);
            write_bound(
                out,
                AccentPosition::Below,
                *movable_limits,
                is_overunder_line_node(under),
                |out| write_script_arg(out, under, style.sub()),
            );
        },
        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            write_bound_base(out, base, style);
            write_bound(
                out,
                AccentPosition::Below,
                *movable_limits,
                is_overunder_line_node(under),
                |out| write_script_arg(out, under, style.sub()),
            );
            write_bound(
                out,
                AccentPosition::Above,
                *movable_limits,
                is_overunder_line_node(over),
                |out| write_script_arg(out, over, style.sup()),
            );
        },

        MathNode::Fraction { num, den, kind } => {
            let style = kind.forced_style().unwrap_or(style);
            write_fraction_arg(out, num, style.fracnum());
            out.push(fraction_operator(*kind));
            let start = out.len();
            write_fraction_arg(out, den, style.fracden());
            group_negatable(out, start);
        },

        MathNode::Sqrt(inner) => {
            out.push('\u{221A}');
            write_paren_if_complex(out, inner, style.cramped());
        },

        MathNode::Root { index, radicand } => {
            // Use cube root symbol for index=3, fourth root for index=4
            match index.as_ref() {
                MathNode::Number(s) if s == "3" => {
                    out.push('\u{221B}');
                    write_paren_if_complex(out, radicand, style.cramped());
                },
                MathNode::Number(s) if s == "4" => {
                    out.push('\u{221C}');
                    write_paren_if_complex(out, radicand, style.cramped());
                },
                _ => {
                    // General nth root: √(n&radicand) notation
                    out.push('\u{221A}');
                    out.push('(');
                    let start = out.len();
                    write_node(out, index, style.sup().sup());
                    group_null_operand(out, start, false);
                    out.push('&');
                    let start = out.len();
                    write_node(out, radicand, style.cramped());
                    group_null_operand(out, start, false);
                    out.push(')');
                },
            }
        },

        MathNode::Nary { .. } => write_nary(out, node, style, false),

        MathNode::Accent {
            base,
            accent,
            position,
            ..
        } => {
            let start = out.len();
            write_accent_base(out, base, style);
            if out.len() == start {
                out.push_str("〖〗");
            }
            if !is_unicode_combining_accent(*accent) {
                out.push(match position {
                    AccentPosition::Above => '┴',
                    AccentPosition::Below => '┬',
                });
            }
            out.push(*accent);
        },

        MathNode::Array { .. } | MathNode::Matrix { .. } | MathNode::NativeArray { .. } => {
            write_heap_table(out, node, '■');
        },

        MathNode::Phantom(inner, _) => {
            out.push('\u{27E1}'); // phantom operator
            write_paren_if_complex(out, inner, style);
        },

        MathNode::Boxed { content: inner, .. } => {
            out.push('\u{25AD}'); // rectangle
            write_paren_if_complex(out, inner, style);
        },
        MathNode::Actuarial { content: inner, .. } => {
            out.push('\u{25AD}');
            out.push('(');
            out.push_str("6&");
            write_node(out, inner, style);
            out.push(')');
        },

        MathNode::Cancel { content, .. } => {
            write_node(out, content, style);
        },

        MathNode::Scaled { content, .. } | MathNode::RaiseBox { content, .. } => {
            write_node(out, content, style);
        },

        MathNode::Styled(text_style, children) => {
            // Apply Unicode math alphabet mapping
            let mut current_style = style;
            let mut previous_operator = false;
            for child in children {
                if let MathNode::StyleSwitch(value) = child {
                    current_style = MathStyle::branch_from_switch(*value);
                    continue;
                }
                let start = out.len();
                let terminates = needs_operator_terminator(child, current_style);
                match child {
                    MathNode::Identifier(s) | MathNode::Number(s) | MathNode::Text(_, s) => {
                        for ch in s.chars() {
                            let styled =
                                crate::unicode::fonts::styled_char(ch, *text_style).unwrap_or(ch);
                            out.push(styled);
                        }
                    },
                    _ => write_node(out, child, current_style),
                }
                separate_after_script(out, start, previous_operator);
                previous_operator = terminates;
            }
        },
        MathNode::Color { content, .. } => write_node(out, content, style),
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let branch = style.choose(display, text, script, script_script);
            write_node(out, branch, style);
        },
        // An explicit invisible group reads back as an inner atom; without
        // its brackets the content would merge into what is around it
        // (`〖〖〗_1〗y` is not `〖〗_1 y`, a leading script of y).
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            content,
        } => {
            out.push('〖');
            write_node(out, content, style);
            out.push('〗');
        },
        MathNode::MathAtom { atom_type, content } => {
            let start = out.len();
            write_node(out, content, style);
            // An empty ordinary atom keeps what is around it apart.
            if *atom_type == MathAtomType::Ord && out.len() == start {
                out.push(ZERO_WIDTH_SPACE);
            }
        },
        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            write_node(out, content, style)
        },
        MathNode::StructuredOperator { content, .. } => {
            // UnicodeMath's invisible grouping delimiters preserve the
            // operator body as one script base. The form itself is carried by
            // the ordinary or forced script markers emitted by the parent.
            let grouped = structured_content_needs_group(content);
            if grouped {
                out.push('〖');
            }
            write_structured_content(out, content, style);
            if grouped {
                out.push('〗');
            }
        },
        MathNode::Reflect { content, .. } => write_node(out, content, style),
    }
}

fn structured_content_needs_group(node: &MathNode) -> bool {
    match node {
        MathNode::Row(children) => children.len() > 1,
        MathNode::Styled(_, children) => children.len() > 1,
        MathNode::Color { content, .. }
        | MathNode::MathAtom { content, .. }
        | MathNode::StructuredOperator { content, .. } => structured_content_needs_group(content),
        MathNode::Accent { accent, .. } => !is_unicode_combining_accent(*accent),
        MathNode::Overscript {
            over,
            movable_limits,
            ..
        } => !*movable_limits || !is_overunder_line_node(over),
        MathNode::Underscript {
            under,
            movable_limits,
            ..
        } => !*movable_limits || !is_overunder_line_node(under),
        MathNode::UnderOver { .. } => true,
        MathNode::Fraction { .. }
        | MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. } => false,
        _ => false,
    }
}

fn write_structured_nodes_with_style(out: &mut String, nodes: &[MathNode], style: MathStyle) {
    let mut current_style = style;
    let mut wrote_node = false;
    let mut previous_operator = false;
    let mut takes_prime = false;
    let mut edge = OperandEdge::Break;
    let mut identifier_run = String::new();
    for node in nodes {
        if let MathNode::StyleSwitch(value) = node {
            if !identifier_run.is_empty() {
                write_structured_identifier_run(out, &identifier_run);
                identifier_run.clear();
                wrote_node = true;
                previous_operator = false;
                takes_prime = true;
                edge = OperandEdge::Operand;
            }
            current_style = MathStyle::branch_from_switch(*value);
            continue;
        }
        if let MathNode::Identifier(value) = node {
            identifier_run.push_str(value);
            continue;
        }
        if !identifier_run.is_empty() {
            write_structured_identifier_run(out, &identifier_run);
            identifier_run.clear();
            wrote_node = true;
            previous_operator = false;
            takes_prime = true;
            edge = OperandEdge::Operand;
        }
        if wrote_node
            && !matches!(node, MathNode::Text(_, _))
            && needs_space_before_with_style(node, current_style)
        {
            out.push(' ');
        }
        let start = out.len();
        write_structured_content(out, node, current_style);
        let current = operand_edge(select_mathchoice(node, current_style));
        separate_following_operand(out, start, || previous_operator, takes_prime, edge, current);
        previous_operator = needs_operator_terminator(node, current_style);
        takes_prime = !is_prime_symbol(node);
        edge = current;
        wrote_node = true;
    }
    if !identifier_run.is_empty() {
        write_structured_identifier_run(out, &identifier_run);
    }
}

fn write_structured_identifier_run(out: &mut String, value: &str) {
    // Normalization can turn a row of letters into one Identifier. Both
    // representations must remain one operand when a decoration follows.
    if value.is_empty() {
        return;
    }
    let single = value.chars().nth(1).is_none();
    let function = || {
        crate::unicode::symbols::unicode_math_operator_prefix(value)
            .is_some_and(|(word, _)| word == value)
    };
    let styled = || {
        !value.is_ascii()
            && value
                .chars()
                .all(|ch| crate::unicode::fonts::unstyled_char(ch).is_some())
    };
    if single || function() || styled() {
        out.push_str(value);
    } else {
        out.push('"');
        out.push_str(value);
        out.push('"');
    }
}

fn write_accent(
    out: &mut String,
    base: &MathNode,
    accent: char,
    position: AccentPosition,
    style: MathStyle,
) {
    if !is_unicode_combining_accent(accent) {
        write_structured_decorated_base(out, base, style);
        out.push(match position {
            AccentPosition::Above => '┴',
            AccentPosition::Below => '┬',
        });
        out.push(accent);
    } else {
        write_structured_decorated_base(out, base, style);
        out.push(accent);
    }
}

fn is_unicode_combining_accent(accent: char) -> bool {
    matches!(accent, '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}')
}

fn structured_decorated_base_needs_group(node: &MathNode) -> bool {
    match node {
        MathNode::Row(children) | MathNode::Styled(_, children) => children.len() > 1,
        MathNode::Fraction { .. }
        | MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. }
        | MathNode::Accent { .. }
        | MathNode::StructuredOperator { .. } => true,
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        MathNode::Color { content, .. } | MathNode::MathAtom { content, .. } => {
            structured_decorated_base_needs_group(content)
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => [display, text, script, script_script]
            .into_iter()
            .any(|branch| structured_decorated_base_needs_group(branch)),
        _ => false,
    }
}

fn write_structured_decorated_base(out: &mut String, node: &MathNode, style: MathStyle) {
    if structured_decorated_base_needs_group(node) {
        out.push('〖');
        write_structured_content(out, node, style);
        out.push('〗');
    } else {
        write_structured_content(out, node, style);
    }
}

/// Write a bound in a structured body. A run of letters and digits is one
/// script operand (UTN #28 section 2.2) and is written as is (`┴nⅆx`), not
/// quoted as text.
fn write_structured_bound(out: &mut String, node: &MathNode, style: MathStyle) {
    match node {
        MathNode::Identifier(name) if reads_as_script_run(name) => out.push_str(name),
        MathNode::Row(children) if write_simple_script(out, script_items(children)) => {},
        MathNode::Row(children) if children.len() > 1 => write_script_arg(out, node, style),
        MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. } => write_script_arg(out, node, style),
        _ => write_structured_content(out, node, style),
    }
}

/// Compact counterpart of [`write_structured_bound`].
fn write_structured_bound_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    match *arena.node(id) {
        CompactNode::Identifier(sid) if reads_as_script_run(arena.str_val(sid)) => {
            out.push_str(arena.str_val(sid));
        },
        CompactNode::Row(span) if write_simple_script(out, script_items_c(arena, span)) => {},
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            write_script_arg_c(out, arena, id, style);
        },
        CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. } => write_script_arg_c(out, arena, id, style),
        _ => write_structured_content_c(out, arena, id, style),
    }
}

/// Write a structured operator's body without introducing the literal
/// parentheses used by ordinary UnicodeMath grouping. Decorations and nested
/// rows remain linear and can be recovered by the UnicodeMath parser.
fn write_structured_content(out: &mut String, node: &MathNode, style: MathStyle) {
    match node {
        MathNode::Identifier(value) => write_structured_identifier_run(out, value),
        MathNode::Row(children) => write_structured_nodes_with_style(out, children, style),
        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if is_overunder_line_node(over) || ends_in_bound(base) {
                write_structured_decorated_base(out, base, style);
            } else {
                write_structured_content(out, base, style);
            }
            write_bound(
                out,
                AccentPosition::Above,
                *movable_limits,
                is_overunder_line_node(over),
                |out| {
                    write_structured_bound(out, over, style.sup());
                },
            );
        },
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if is_overunder_line_node(under) || ends_in_bound(base) {
                write_structured_decorated_base(out, base, style);
            } else {
                write_structured_content(out, base, style);
            }
            write_bound(
                out,
                AccentPosition::Below,
                *movable_limits,
                is_overunder_line_node(under),
                |out| {
                    write_structured_bound(out, under, style.sub());
                },
            );
        },
        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            if ends_in_bound(base) {
                write_structured_decorated_base(out, base, style);
            } else {
                write_structured_content(out, base, style);
            }
            write_bound(
                out,
                AccentPosition::Below,
                *movable_limits,
                is_overunder_line_node(under),
                |out| {
                    write_structured_bound(out, under, style.sub());
                },
            );
            write_bound(
                out,
                AccentPosition::Above,
                *movable_limits,
                is_overunder_line_node(over),
                |out| {
                    write_structured_bound(out, over, style.sup());
                },
            );
        },
        MathNode::Accent {
            base,
            accent,
            position,
            ..
        } => {
            write_accent(out, base, *accent, *position, style);
        },
        MathNode::Styled(text_style, children) => {
            let mut current_style = style;
            for child in children {
                if let MathNode::StyleSwitch(value) = child {
                    current_style = MathStyle::branch_from_switch(*value);
                    continue;
                }
                match child {
                    MathNode::Identifier(s) | MathNode::Number(s) | MathNode::Text(_, s) => {
                        for ch in s.chars() {
                            out.push(
                                crate::unicode::fonts::styled_char(ch, *text_style).unwrap_or(ch),
                            );
                        }
                    },
                    _ => write_structured_content(out, child, current_style),
                }
            }
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => write_structured_content(
            out,
            style.choose(display, text, script, script_script),
            style,
        ),
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            content,
        } => {
            out.push('〖');
            // The group already keeps a word one operand: no quotes.
            if let MathNode::Identifier(word) = &**content {
                out.push_str(word);
            } else {
                write_structured_content(out, content, style);
            }
            out.push('〗');
        },
        MathNode::Color { content, .. }
        | MathNode::MathAtom { content, .. }
        | MathNode::StructuredOperator { content, .. } => {
            write_structured_content(out, content, style);
        },
        _ => write_node(out, node, style),
    }
}

fn needs_side_group(name: &str, limits: LimitStyle) -> bool {
    limits == LimitStyle::SubSup
        && crate::unicode::symbols::unicode_math_operator_prefix(name)
            .is_some_and(|(word, default)| word == name && default != LimitStyle::SubSup)
}

/// A compound `\mathop{...}` body must be an invisible UnicodeMath group
/// before a script. Without it, `Res_x` is parsed as a subscript on only the
/// final `s` of the operator name.
fn operator_body_needs_group(node: &MathNode, style: MathStyle) -> bool {
    match select_mathchoice(node, style) {
        MathNode::Identifier(name) | MathNode::Symbol(_, name) => name.chars().nth(1).is_some(),
        MathNode::Number(_) | MathNode::Operator(_) => false,
        MathNode::Styled(_, children) | MathNode::Row(children) => {
            children.len() != 1 || operator_body_needs_group(&children[0], style)
        },
        MathNode::Color { content, .. }
        | MathNode::MathAtom { content, .. }
        | MathNode::StructuredOperator { content, .. } => operator_body_needs_group(content, style),
        _ => true,
    }
}

fn operator_body_needs_group_c(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Identifier(sid) | CompactNode::Symbol(_, sid) => {
            arena.str_val(sid).chars().nth(1).is_some()
        },
        CompactNode::Number(_) | CompactNode::Operator { .. } => false,
        CompactNode::Styled(_, span) | CompactNode::Row(span) => {
            let children = arena.children(span);
            children.len() != 1 || operator_body_needs_group_c(arena, children[0], style)
        },
        CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. } => {
            operator_body_needs_group_c(arena, content, style)
        },
        _ => true,
    }
}

fn write_base(out: &mut String, node: &MathNode, style: MathStyle) {
    if let MathNode::MathAtom {
        atom_type: MathAtomType::Op,
        content,
    } = node
        && operator_body_needs_group(content, style)
    {
        out.push('〖');
        write_node(out, node, style);
        out.push('〗');
        return;
    }
    match select_mathchoice(node, style) {
        MathNode::Operator(attr) if needs_side_group(&attr.name, attr.limit_style) => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. } => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        // The reader takes no other fence than brackets as one (`〖|x|〗^2`).
        MathNode::Fenced { open, close, .. } if !is_bracket_pair(open, close) => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        // A script after a prescripted base would be the base's own.
        MathNode::PreScripts { .. } => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        // A compound base is an invisible group: `()` would read back as
        // visible parentheses (`〖a+b〗^2` is LaTeX `{a+b}^2`, `x〖〗_1` is
        // `x{}_1`).
        MathNode::Row(_) | MathNode::Fraction { .. } => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        // The script would join the n-aryand.
        MathNode::Nary { body, .. } if !matches!(&**body, MathNode::Row(children) if children.is_empty()) =>
        {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        _ => write_node(out, node, style),
    }
}

/// Write the base of a combining accent, grouped when the accent would take
/// only the last part of what it writes.
fn write_accent_base(out: &mut String, node: &MathNode, style: MathStyle) {
    let grouped = match select_mathchoice(node, style) {
        MathNode::Identifier(name) => name.chars().nth(1).is_some(),
        MathNode::Row(children) => !children.is_empty(),
        MathNode::Styled(_, children) => children.len() > 1,
        MathNode::Space(_) | MathNode::ExplicitSpace { .. } => true,
        MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. }
        | MathNode::PreScripts { .. }
        | MathNode::Fraction { .. }
        | MathNode::Sqrt(_)
        | MathNode::Root { .. }
        | MathNode::Boxed { .. }
        | MathNode::Phantom(..)
        | MathNode::Nary { .. } => true,
        MathNode::Fenced { open, close, .. } => !is_bracket_pair(open, close),
        _ => false,
    };
    let grouped = grouped || ends_in_bound(node);
    if grouped {
        out.push('〖');
    }
    write_node(out, node, style);
    if grouped {
        out.push('〗');
    }
}

/// Compact counterpart of [`write_accent_base`].
fn write_accent_base_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    let grouped = match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Identifier(sid) => arena.str_val(sid).chars().nth(1).is_some(),
        CompactNode::Row(span) => !span.is_empty(),
        CompactNode::Styled(_, span) => arena.children(span).len() > 1,
        CompactNode::Space(_) => true,
        CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. }
        | CompactNode::PreScripts { .. }
        | CompactNode::Fraction { .. }
        | CompactNode::Sqrt(_)
        | CompactNode::Root { .. }
        | CompactNode::Boxed { .. }
        | CompactNode::Phantom(..)
        | CompactNode::Nary { .. } => true,
        CompactNode::Fenced { open, close, .. } => {
            !is_bracket_pair(arena.str_val(open), arena.str_val(close))
        },
        _ => false,
    };
    let grouped = grouped || ends_in_bound_c(arena, id);
    if grouped {
        out.push('〖');
    }
    write_node_c(out, arena, id, style);
    if grouped {
        out.push('〗');
    }
}

/// The base of a bound. An accent is itself written with a bound's mark
/// (`x┴⎴`), which a second one would join (`x┴(⎴┴n)`).
fn write_bound_base(out: &mut String, node: &MathNode, style: MathStyle) {
    if ends_in_bound(node) {
        out.push('〖');
        write_node(out, node, style);
        out.push('〗');
    } else {
        write_script_base(out, node, style);
    }
}

/// Whether `node` is written as an accent with a bound's mark.
fn ends_in_bound(node: &MathNode) -> bool {
    match node {
        MathNode::Accent { accent, .. } => !is_unicode_combining_accent(*accent),
        MathNode::Row(children) | MathNode::Styled(_, children) => {
            matches!(children.as_slice(), [only] if ends_in_bound(only))
        },
        MathNode::Color { content, .. } | MathNode::Attributed { content, .. } => {
            ends_in_bound(content)
        },
        MathNode::MathAtom { content, atom_type } => {
            *atom_type != crate::ast::MathAtomType::Inner && ends_in_bound(content)
        },
        _ => false,
    }
}

/// Compact counterpart of [`write_bound_base`].
fn write_bound_base_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    if ends_in_bound_c(arena, id) {
        out.push('〖');
        write_node_c(out, arena, id, style);
        out.push('〗');
    } else {
        write_script_base_c(out, arena, id, style);
    }
}

/// Compact counterpart of [`ends_in_bound`].
fn ends_in_bound_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Accent { accent, .. } => !is_unicode_combining_accent(accent),
        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            matches!(*arena.children(span), [only] if ends_in_bound_c(arena, only))
        },
        CompactNode::Color { content, .. } => ends_in_bound_c(arena, content),
        CompactNode::MathAtom { content, atom_type } => {
            atom_type != crate::ast::MathAtomType::Inner && ends_in_bound_c(arena, content)
        },
        _ => false,
    }
}

/// Write the base of a subscript or superscript. A word is grouped: the
/// script would take only its last letter (`〖ab〗_1` is `{ab}_1`, `ab_1`
/// is `a b_1`).
fn write_script_base(out: &mut String, node: &MathNode, style: MathStyle) {
    match select_mathchoice(node, style) {
        MathNode::Identifier(name) if name.chars().nth(1).is_some() => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        MathNode::Symbol(_, glyph) if glyph.chars().nth(1).is_some() => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        // A space takes no script of its own (`〖\u{2009}〗′`).
        MathNode::Space(_) | MathNode::ExplicitSpace { .. } => {
            out.push('〖');
            write_node(out, node, style);
            out.push('〗');
        },
        _ => write_base(out, node, style),
    }
}

use crate::reader::unicode_math::{
    breaks_naryand, breaks_operand_at, is_alphanumeric, is_nary_char, is_prime as is_prime_char,
    is_run_char, opening_fence, pair_bars_at, pairs_bars, reads_as_naryand, reads_as_script_run,
    starts_signed_script_operand, symbol_takes_prime,
};

/// A symbol node that starts with a prime, which the reader never gives
/// the primes after it as a superscript.
fn is_prime_symbol(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, glyph) if glyph.starts_with(is_prime_char))
}

fn is_prime_symbol_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(_, sid)
        if arena.str_val(sid).starts_with(is_prime_char))
}

fn is_prime_glyph(glyph: &str) -> bool {
    let mut chars = glyph.chars();
    chars.next().is_some_and(is_prime_char) && chars.next().is_none()
}

/// Heap counterpart of [`prime_superscript_c`].
fn prime_superscript<'a>(
    base: &MathNode,
    sup: &'a MathNode,
) -> Option<(&'a [MathNode], Option<&'a MathNode>)> {
    let is_prime = |node: &MathNode| matches!(node, MathNode::Symbol(SymbolKind::Ordinary, glyph) if is_prime_glyph(glyph));
    // Primes right after an n-ary operator are its limit.
    if matches!(base, MathNode::Nary { .. }) {
        return None;
    }
    // The reader gives such a base the primes after it (not `′′` or `+′`).
    if matches!(base, MathNode::Symbol(_, glyph) if !symbol_takes_prime(glyph)) {
        return None;
    }
    if is_prime(sup) {
        return Some((std::slice::from_ref(sup), None));
    }
    let MathNode::Row(children) = sup else {
        return None;
    };
    let primes = children.iter().take_while(|node| is_prime(node)).count();
    match &children[primes..] {
        _ if primes == 0 => None,
        [] => Some((children, None)),
        [rest] => Some((&children[..primes], Some(rest))),
        _ => None,
    }
}

/// Write a script argument. One that writes as nothing is an empty group:
/// `x^` or `(_1)` alone reads back as text.
fn write_script_arg(out: &mut String, node: &MathNode, style: MathStyle) {
    let start = out.len();
    write_script_arg_unguarded(out, node, style);
    let fenced = matches!(select_mathchoice(node, style), MathNode::Fenced { .. });
    group_null_operand(out, start, !fenced);
}

fn write_script_arg_unguarded(out: &mut String, node: &MathNode, style: MathStyle) {
    match select_mathchoice(node, style) {
        // The reader skips a space after `^`; in parentheses it is content.
        MathNode::Identifier(name) => write_script_word(out, name),
        MathNode::Space(_) | MathNode::ExplicitSpace { .. } => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        MathNode::Row(children) if write_simple_script(out, script_items(children)) => {},
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        // The reader takes no other fence than brackets as one operand, and
        // scripts after a prescripted operand as its own.
        MathNode::Fenced { open, close, .. } if !is_bracket_pair(open, close) => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        MathNode::PreScripts { .. } => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        // An accent on more than an atom or a group: the script would end
        // before it.
        MathNode::Accent { base, .. }
            if !(matches!(select_mathchoice(base, style),
                MathNode::Symbol(_, glyph) if !needs_script_group(glyph))
                || matches!(
                    select_mathchoice(base, style),
                    MathNode::Identifier(_)
                        | MathNode::Number(_)
                        | MathNode::Operator(_)
                        | MathNode::MathAtom { .. }
                        | MathNode::Accent { .. }
                )) =>
        {
            let start = out.len();
            write_node(out, node, style);
            if !out[start..].starts_with('〖') {
                out.insert(start, '(');
                out.push(')');
            }
        },
        // A symbol of several characters, or a quoted solidus.
        MathNode::Symbol(_, glyph) if needs_script_group(glyph) => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        // The reader drops parentheses around a whole operand.
        MathNode::Fenced { open, close, .. } if open == "(" && close == ")" => {
            let start = out.len();
            write_node(out, node, style);
            // It keeps empty ones.
            if &out[start..] != "()" {
                out.insert(start, '(');
                out.push(')');
            }
        },
        MathNode::Sqrt(_)
        | MathNode::Root { .. }
        | MathNode::Fraction { .. }
        | MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. }
        | MathNode::Phantom(..)
        | MathNode::Boxed { .. }
        | MathNode::Actuarial { .. }
        | MathNode::Reflect { .. } => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        _ => {
            let start = out.len();
            write_node(out, node, style);
            group_spaced_script(out, start);
        },
    }
}

/// A sign the reader takes as the start of a script operand (`e^-x`).
fn is_script_sign(glyph: &str) -> bool {
    matches!(glyph, "-" | "+" | "\u{2212}")
}

/// A child of a script argument row, as [`write_simple_script`] sees it.
enum ScriptItem<'a> {
    Word(&'a str),
    Number(&'a str),
    Symbol(&'a str),
    Other,
}

fn script_items(children: &[MathNode]) -> impl Iterator<Item = ScriptItem<'_>> {
    children.iter().map(|child| match child {
        MathNode::Identifier(name) => ScriptItem::Word(name),
        MathNode::Number(digits) => ScriptItem::Number(digits),
        MathNode::Symbol(_, glyph) => ScriptItem::Symbol(glyph),
        _ => ScriptItem::Other,
    })
}

/// Compact counterpart of [`script_items`].
fn script_items_c(
    arena: &AstArena,
    span: crate::ast::compact::NodeSpan,
) -> impl Iterator<Item = ScriptItem<'_>> {
    arena
        .children(span)
        .iter()
        .map(|&child| match *arena.node(child) {
            CompactNode::Identifier(sid) => ScriptItem::Word(arena.str_val(sid)),
            CompactNode::Number(sid) => ScriptItem::Number(arena.str_val(sid)),
            CompactNode::Symbol(_, sid) => ScriptItem::Symbol(arena.str_val(sid)),
            _ => ScriptItem::Other,
        })
}

/// Write a script argument row if it is an operand the reader takes whole
/// without parentheses (UTN #28 section 2.2, appendix A `soperand`): letters
/// and digits (`a_ij`, `x^2y`), possibly after a sign (`e^-x`, `x^-1`), or a
/// signed infinity (`∑_(n=-∞)`, `x^-∞`). Otherwise nothing is written.
fn write_simple_script<'a>(out: &mut String, items: impl Iterator<Item = ScriptItem<'a>>) -> bool {
    let start = out.len();
    let mut run = start;
    let mut after_number = false;
    let mut items = items.enumerate().peekable();
    let mut simple = true;
    let mut infinity = false;
    while let Some((index, item)) = items.next() {
        match item {
            ScriptItem::Symbol(sign) if index == 0 && is_script_sign(sign) => {
                out.push_str(sign);
                run = out.len();
                continue;
            },
            ScriptItem::Symbol("\u{221E}")
                if index == 1 && run > start && items.peek().is_none() =>
            {
                out.push('\u{221E}');
                infinity = true;
            },
            // A word is letters that normalization joined (`aaa`).
            ScriptItem::Word(name)
                if !name.is_empty()
                    && name.chars().all(|c| is_run_char(c) && !c.is_ascii_digit()) =>
            {
                out.push_str(name);
                after_number = false;
            },
            // Adjacent numbers would read as one.
            ScriptItem::Number(digits)
                if !after_number
                    && digits.starts_with(|c: char| c.is_ascii_digit())
                    && digits.chars().all(|c| c.is_ascii_digit() || c == '.') =>
            {
                out.push_str(digits);
                after_number = true;
            },
            _ => {
                simple = false;
                break;
            },
        }
    }
    simple &= infinity || reads_as_script_run(&out[run..]);
    if !simple {
        out.truncate(start);
    }
    simple
}

/// Write a word as a script operand: as it is when the reader takes it
/// whole, else grouped if a space has to break it (`x_(m ax)`).
fn write_script_word(out: &mut String, name: &str) {
    let start = out.len();
    if reads_as_script_run(name) {
        out.push_str(name);
    } else {
        write_identifier(out, name);
        if out[start..].contains(' ') {
            out.insert(start, '(');
            out.push(')');
        }
    }
}

/// Replace an operand written as nothing from `start` on, or as nothing but
/// dropped spaces (`x^{\!}`), by an empty group. `grouping`: parentheses
/// around it are the writer's.
fn group_null_operand(out: &mut String, start: usize, grouping: bool) {
    // Most operands start with a letter, a digit or an operator.
    if out[start..].starts_with(|c: char| c != '(' && c.is_ascii()) {
        return;
    }
    let written = &out[start..];
    let inner = written
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .filter(|_| grouping);
    if written.is_empty() || is_null_operand(inner.unwrap_or(written)) || inner == Some("") {
        out.replace_range(start.., "〖〗");
    }
}

/// Parenthesize a denominator whose operator the fraction bar would negate
/// (`x/(∣⃗)`).
fn group_negatable(out: &mut String, start: usize) {
    let first = out[start..].chars().next();
    if first.is_some_and(|c| crate::unicode::symbols::precomposed_negation(c).is_some()) {
        out.insert(start, '(');
        out.push(')');
    }
}

/// Whether the reader takes a fence of `open` and `close` as one group.
fn is_bracket_pair(open: &str, close: &str) -> bool {
    matches!((open, close), ("(", ")") | ("[", "]") | ("{", "}"))
}

/// A LaTeX cases/numcases fence is represented in the shared AST as a
/// one-sided left brace around one table. UnicodeMath has a dedicated
/// operator for exactly that shape; return the table when this is the shape
/// we can spell without losing the fence's structure.
fn cases_array(content: &[FencedItem]) -> Option<&MathNode> {
    let [FencedItem::Element(node)] = content else {
        return None;
    };
    matches!(
        node,
        MathNode::Array { .. } | MathNode::Matrix { .. } | MathNode::NativeArray { .. }
    )
    .then_some(node)
}

/// Write a heap table using the requested UnicodeMath marker. Ordinary
/// matrices use U+25A0; cases use U+24B8 and share this exact body formatter.
fn write_heap_table(out: &mut String, node: &MathNode, marker: char) {
    let rows = match node {
        MathNode::Array { rows, .. }
        | MathNode::Matrix { rows, .. }
        | MathNode::NativeArray { rows, .. } => rows,
        _ => return,
    };
    out.push(marker);
    out.push('(');
    // A merged cell projects to its content followed by empty columns. Match
    // the compact rectangle so later operands retain their logical column
    // even when the source was ragged.
    let expanded = rows.iter().flatten().any(|cell| table_cell_span(cell) > 1);
    let width = if expanded {
        let declared = match node {
            MathNode::Array { columns, .. } | MathNode::NativeArray { columns, .. } => {
                columns.len()
            },
            _ => 0,
        };
        rows.iter()
            .map(|r| table_row_width(r))
            .max()
            .unwrap_or(0)
            .max(declared)
    } else {
        0
    };
    let mut row_start = 0;
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push('@');
        }
        row_start = out.len();
        let mut column = 0;
        for cell in row {
            if column != 0 {
                out.push('&');
            }
            write_nodes_with_style(out, cell, MathStyle::Text);
            let span = table_cell_span(cell);
            out.extend(std::iter::repeat_n('&', span - 1));
            column += span;
        }
        out.extend(std::iter::repeat_n(
            '&',
            width.saturating_sub(column.max(1)),
        ));
    }
    // The reader drops an empty last row that no at-sign ends.
    if !rows.is_empty() && out.len() == row_start {
        out.push('@');
    }
    out.push(')');
}

/// Compact counterpart of cases_array.
fn cases_array_c(arena: &AstArena, content: crate::ast::compact::NodeSpan) -> Option<NodeId> {
    let [id] = arena.children(content) else {
        return None;
    };
    matches!(*arena.node(*id), CompactNode::Array { .. }).then_some(*id)
}

/// Write a compact table using the requested UnicodeMath marker. The body is
/// shared by ordinary matrices and cases, so nested cases are emitted once
/// while their cells are traversed.
fn write_compact_table(out: &mut String, arena: &AstArena, id: NodeId, marker: char) {
    let CompactNode::Array {
        cells, num_cols, ..
    } = *arena.node(id)
    else {
        return;
    };
    out.push(marker);
    out.push('(');
    let cell_ids = arena.children(cells);
    let stride = usize::from(num_cols.max(1));
    let mut row_start = 0;
    for (i, row) in cell_ids.chunks(stride).enumerate() {
        if i > 0 {
            out.push('@');
        }
        row_start = out.len();
        for (j, &cell_id) in row.iter().enumerate() {
            if j > 0 {
                out.push('&');
            }
            if let CompactNode::Row(cell_span) = *arena.node(cell_id) {
                write_nodes_c_with_style(out, arena, cell_span, MathStyle::Text);
            } else {
                write_node_c(out, arena, cell_id, MathStyle::Text);
            }
        }
        out.extend(std::iter::repeat_n('&', stride - row.len()));
    }
    // The reader drops an empty last row that no at-sign ends.
    if !cell_ids.is_empty() && out.len() == row_start {
        out.push('@');
    }
    out.push(')');
}

/// A symbol that is no script operand by itself: of several characters, a
/// quoted solidus, or one the reader takes an operand after.
fn needs_script_group(glyph: &str) -> bool {
    let mut chars = glyph.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => {
            matches!(
                c,
                '/' | '√' | '∛' | '∜' | '▭' | '⟡' | '\u{2308}' | '\u{230A}'
            ) || pairs_bars(c)
                || is_nary_char(c)
        },
        (first, _) => first.is_some(),
    }
}

/// Parenthesize a script operand written with a space outside brackets,
/// which would end it (`x_(∑ a)`).
fn group_spaced_script(out: &mut String, start: usize) {
    if out[start..].contains(' ') && !continues_naryand(out, start, &[]) {
        out.insert(start, '(');
        out.push(')');
    }
}

/// Whether `written` is only the zero-width spaces of dropped spaces.
fn is_null_operand(written: &str) -> bool {
    !written.is_empty() && written.chars().all(|c| c == ZERO_WIDTH_SPACE)
}

/// Whether [`write_identifier`] breaks `name`: a function name ends it or is
/// in it.
fn breaks_word(name: &str) -> bool {
    name.char_indices().any(|(index, _)| {
        crate::unicode::symbols::unicode_math_operator_prefix(&name[index..]).is_some()
    })
}

/// Write a word, broken by a space where it would read as a function name
/// (the word `inf` is `i nf`, not the function inf). The reader joins the
/// letters into one word again.
fn write_identifier(out: &mut String, name: &str) {
    if name.len() == 1 {
        out.push_str(name);
        return;
    }
    let mut chars = name.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        out.push(c);
        if chars.peek().is_some()
            && crate::unicode::symbols::unicode_math_operator_prefix(&name[index..]).is_some()
        {
            out.push(' ');
        }
    }
}

/// The letter a prescripted base ends in, which the letters after it would
/// spell a function name with (`(_1)s in`).
fn prescripted_letter(node: &MathNode, style: MathStyle) -> Option<&str> {
    match select_mathchoice(node, style) {
        MathNode::PreScripts { base, .. } => match select_mathchoice(base, style) {
            MathNode::Identifier(name) if name.chars().nth(1).is_none() => Some(name),
            base => prescripted_letter(base, style),
        },
        _ => None,
    }
}

/// Compact counterpart of [`prescripted_letter`].
fn prescripted_letter_c(arena: &AstArena, id: NodeId, style: MathStyle) -> Option<&str> {
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::PreScripts { base, .. } => {
            match *arena.node(select_mathchoice_c(arena, base, style)) {
                CompactNode::Identifier(sid) => {
                    Some(arena.str_val(sid)).filter(|name| name.chars().nth(1).is_none())
                },
                _ => prescripted_letter_c(arena, base, style),
            }
        },
        _ => None,
    }
}

/// A symbol struck through (LaTeX `\not`): one character where there is one,
/// else with the combining solidus after it.
fn negated_glyph(glyph: &str) -> Option<String> {
    let mut chars = glyph.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match crate::unicode::symbols::precomposed_negation(c) {
        Some(negated) => negated.to_string(),
        None => format!("{c}\u{0338}"),
    })
}

/// Follow the letters of adjacent words: `word` is where they start. The
/// node written from `start` on ends them unless it is a word itself.
fn track_word(out: &mut String, word: &mut Option<usize>, start: usize, is_word: bool) {
    if is_word {
        word.get_or_insert(start);
    } else if let Some(from) = word.take() {
        break_function_names(out, from, start);
    }
}

/// Whether `node` writes as letters and digits only: a word, or a row of
/// them that only normalization joins (`{max}y`).
fn is_word(node: &MathNode) -> bool {
    match node {
        MathNode::Identifier(_) => true,
        MathNode::Row(children) => {
            !children.is_empty()
                && children
                    .iter()
                    .all(|child| matches!(child, MathNode::Identifier(_) | MathNode::Number(_)))
                && !children
                    .windows(2)
                    .any(|pair| matches!(pair, [MathNode::Number(_), MathNode::Number(_)]))
        },
        _ => false,
    }
}

/// Compact counterpart of [`is_word`].
fn is_word_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(_) => true,
        CompactNode::Row(span) => {
            let number = |id: NodeId| matches!(*arena.node(id), CompactNode::Number(_));
            let children = arena.children(span);
            !children.is_empty()
                && children
                    .iter()
                    .all(|&id| number(id) || matches!(*arena.node(id), CompactNode::Identifier(_)))
                && !children
                    .windows(2)
                    .any(|pair| number(pair[0]) && number(pair[1]))
        },
        _ => false,
    }
}

/// Write a node that is a word ([`is_word`]) as it is.
fn write_word(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Identifier(text) | MathNode::Number(text) => out.push_str(text),
        MathNode::Row(children) => children.iter().for_each(|child| write_word(out, child)),
        _ => {},
    }
}

/// Compact counterpart of [`write_word`].
fn write_word_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Identifier(sid) | CompactNode::Number(sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Row(span) => {
            for &child in arena.children(span) {
                write_word_c(out, arena, child);
            }
        },
        _ => {},
    }
}

/// Break the letters `out[from..to]`, written by adjacent identifiers, where
/// they would read as a function name with what follows (`i nf(x)`).
fn break_function_names(out: &mut String, from: usize, mut to: usize) {
    let mut start = from;
    while start < to {
        let letters = |bytes: &[u8]| bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let end = start + letters(&out.as_bytes()[start..]);
        if end - start > 1 {
            // A name ends where its letters do and has at most 8.
            let mut index = start.max(end.saturating_sub(8));
            while index < end.min(to) {
                index += 1;
                if crate::unicode::symbols::unicode_math_operator_prefix(&out[index - 1..])
                    .is_some()
                {
                    out.insert(index, ' ');
                    to += 1;
                    break;
                }
            }
            if index < end {
                // Go on after the break.
                start = index.max(start + 1);
                continue;
            }
        }
        start = end + 1;
        while start < to && !out.is_char_boundary(start) {
            start += 1;
        }
    }
}

/// Whether `written` ends with a script that is only a sign (`x^-`), which
/// the reader would make the sign of an operand right after it (`x^-y`).
fn ends_with_script_sign(written: &str) -> bool {
    let mut chars = written.chars().rev();
    chars
        .next()
        .is_some_and(|c| matches!(c, '-' | '+' | '\u{2212}'))
        && chars
            .next()
            .is_some_and(|c| matches!(c, '_' | '^' | '┬' | '┴'))
}

/// The UnicodeMath operator between a fraction's numerator and denominator:
/// `¦` stacks them without a bar (`\atop`).
fn fraction_operator(kind: FractionKind) -> char {
    if kind == FractionKind::NoBar {
        '\u{A6}'
    } else {
        '/'
    }
}

fn is_closing_fence(glyph: &str) -> bool {
    let mut chars = glyph.chars();
    chars.next().is_some_and(|c| opening_fence(c).is_some()) && chars.next().is_none()
}

/// Whether `node` is a closing fence symbol, possibly with scripts.
fn ends_in_fence(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(_, glyph) => is_closing_fence(glyph),
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
        | MathNode::Accent { base, .. } => ends_in_fence(base),
        _ => false,
    }
}

/// Compact counterpart of [`ends_in_fence`].
fn ends_in_fence_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Symbol(_, sid) => is_closing_fence(arena.str_val(sid)),
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
        | CompactNode::Accent { base, .. } => ends_in_fence_c(arena, base),
        _ => false,
    }
}

/// Write a fraction operand. The reader drops parentheses around one
/// (UTN #28 section 2.1), so a compound operand is parenthesized and so is a
/// parenthesized one (`((a+b))/c` keeps its parentheses); an operand that
/// writes as nothing is an empty group (`〖〗/y`).
fn write_fraction_arg(out: &mut String, node: &MathNode, style: MathStyle) {
    let start = out.len();
    match select_mathchoice(node, style) {
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        MathNode::Fraction { .. } => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        MathNode::Fenced { open, close, .. } if open == "(" || !is_bracket_pair(open, close) => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        // An operator alone is not an operand (`(+)/2`), and an n-ary
        // operator would take the fraction as its n-aryand (`(∑ a)/b`).
        // A closing fence would take the run from its opening one.
        node if matches!(node, MathNode::Nary { .. })
            || operand_edge(node) == OperandEdge::Break
            || ends_in_fence(node) =>
        {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        _ => write_node(out, node, style),
    }
    let word = out[start..].contains(' ')
        && matches!(select_mathchoice(node, style), MathNode::Identifier(_));
    // A broken word is one operand (`(i nf)/2`).
    if word {
        out.insert(start, '(');
        out.push(')');
    }
    group_null_operand(out, start, true);
}

/// Write a radical or enclosure operand, parenthesized unless it is one
/// letter or digit. One that writes as nothing is an empty group (`√〖〗`):
/// `√()` reads back as visible parentheses.
fn write_paren_if_complex(out: &mut String, node: &MathNode, style: MathStyle) {
    let start = out.len();
    write_paren_if_complex_unguarded(out, node, style);
    group_null_operand(out, start, true);
}

fn write_paren_if_complex_unguarded(out: &mut String, node: &MathNode, style: MathStyle) {
    match select_mathchoice(node, style) {
        // Already bracketed.
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => write_node(out, node, style),
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
        MathNode::Identifier(s) if s.chars().count() == 1 => {
            write_node(out, node, style);
        },
        MathNode::Number(s) if s.chars().count() == 1 => {
            write_node(out, node, style);
        },
        _ => {
            out.push('(');
            write_node(out, node, style);
            out.push(')');
        },
    }
}

fn select_mathchoice(node: &MathNode, style: MathStyle) -> &MathNode {
    let mut node = node;
    loop {
        match node {
            MathNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => node = style.choose(display, text, script, script_script),
            // A row of one child writes as the child; only normalization
            // removes it (`\hat{\max}` from the builder).
            MathNode::Row(children) if children.len() == 1 => node = &children[0],
            // These write as their content.
            MathNode::MathAtom { atom_type, content }
                if *atom_type != MathAtomType::Inner
                    && !matches!(&**content, MathNode::Row(children) if children.is_empty()) =>
            {
                node = content;
            },
            MathNode::Color { content, .. }
            | MathNode::Cancel { content, .. }
            | MathNode::Scaled { content, .. }
            | MathNode::RaiseBox { content, .. }
            | MathNode::Reflect { content, .. } => node = content,
            _ => return node,
        }
    }
}

fn write_nodes_with_style(out: &mut String, nodes: &[MathNode], style: MathStyle) {
    write_nodes_tracking(out, nodes, style, None);
}

/// Write `nodes`; with `applied`, record the offsets of the spaces written
/// between a function name and its argument (see [`continues_naryand`]).
fn write_nodes_tracking<N: std::borrow::Borrow<MathNode>>(
    out: &mut String,
    nodes: &[N],
    style: MathStyle,
    mut applied: Option<&mut SmallVec<[usize; 4]>>,
) {
    let mut current_style = style;
    let mut wrote_node = false;
    let mut previous = None;
    let mut takes_prime = false;
    let mut function = false;
    let mut edge = OperandEdge::Break;
    // Start of the letters of the identifiers written last.
    let mut word = None;
    let mut struck = false;
    for (index, node) in nodes.iter().enumerate() {
        let node = node.borrow();
        if let MathNode::StyleSwitch(value) = node {
            current_style = MathStyle::branch_from_switch(*value);
            continue;
        }
        if std::mem::take(&mut struck) {
            continue;
        }
        if let MathNode::Symbol(_, glyph) = node
            && glyph == "\u{0338}"
            && let Some(next) = nodes.get(index + 1).map(std::borrow::Borrow::borrow)
            && let MathNode::Symbol(_, next) = select_mathchoice(next, current_style)
            && let Some(negated) = negated_glyph(next)
        {
            track_word(out, &mut word, out.len(), false);
            out.push_str(&negated);
            edge = symbol_edge(next);
            previous = None;
            takes_prime = true;
            function = false;
            wrote_node = true;
            struck = true;
            continue;
        }
        let selected = select_mathchoice(node, current_style);
        let identifier = is_word(selected);
        let mut before = out.len();
        if wrote_node && needs_space_before_with_style(node, current_style) {
            out.push(' ');
        }
        let mut start = out.len();
        let next = || {
            nodes[index + 1..]
                .iter()
                .map(std::borrow::Borrow::borrow)
                .find(|node| match node {
                    MathNode::StyleSwitch(_) => false,
                    MathNode::Row(children) => !children.is_empty(),
                    _ => true,
                })
        };
        let open = matches!(node, MathNode::Nary { .. }) && ends_in_function(node, current_style);
        let grouped = open && next().is_some_and(|next| splits_naryand(next, current_style));
        // Within a list a word is broken once what follows it is written.
        if identifier {
            write_word(out, selected);
        } else if grouped {
            write_nary(out, node, current_style, true);
        } else {
            write_node(out, node, current_style);
        }
        if matches!(selected, MathNode::Row(row) if row.len() > 1)
            && index > 0
            && matches!(nodes[index - 1].borrow(), MathNode::Symbol(_, s) if s == "\u{2061}")
        {
            start = group_argument(out, start);
            before = before.min(start);
        }
        let current = operand_edge(selected);
        let current = if open && !grouped && current == OperandEdge::Naryand {
            OperandEdge::NaryandFunction
        } else {
            current
        };
        // Asked only where it matters: it follows the previous node down.
        let previous_operator =
            || previous.is_some_and(|(node, style)| needs_operator_terminator(node, style));
        separate_following_operand(out, start, previous_operator, takes_prime, edge, current);
        if let Some(applied) = applied.as_deref_mut() {
            record_applied_space(applied, out, before, function);
        }
        // A node written as nothing separates nothing.
        if out.len() == before {
            continue;
        }
        track_word(out, &mut word, start, identifier);
        if matches!(selected, MathNode::PreScripts { .. })
            && let Some(letter) = prescripted_letter(selected, current_style)
        {
            word = Some(out.len() - letter.len());
        }
        previous = Some((node, current_style));
        takes_prime = !is_prime_symbol(node);
        function = applied.is_some() && is_function_head(selected);
        edge = current;
        wrote_node = true;
    }
    if let Some(from) = word {
        break_function_names(out, from, out.len());
    }
}

/// Whether `node` writes as a function name the reader takes the operand
/// after as the argument of (`sin`, `sin^2`, `lim_(n→∞)`; UTN #28 section
/// 3.5).
fn is_function_head(node: &MathNode) -> bool {
    match node {
        MathNode::Operator(attr) => is_unicode_function(&attr.name),
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
        | MathNode::Underscript { base, .. }
        | MathNode::Overscript { base, .. }
        | MathNode::UnderOver { base, .. } => is_function_head(base),
        _ => false,
    }
}

/// Compact counterpart of [`is_function_head`].
fn is_function_head_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Operator { name, .. } => is_unicode_function(arena.str_val(name)),
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
        | CompactNode::Underscript { base, .. }
        | CompactNode::Overscript { base, .. }
        | CompactNode::UnderOver { base, .. } => is_function_head_c(arena, base),
        _ => false,
    }
}

/// Record the space at the start of `out[before..]`, written between two
/// n-aryand factors, when the reader goes on past it (UTN #28 sections 3.4
/// and 3.5): after a function name (`∑ log k`) and before a differential
/// (`∫ x^2 ⅆx`).
fn record_applied_space(
    applied: &mut SmallVec<[usize; 4]>,
    out: &str,
    before: usize,
    function: bool,
) {
    let written = &out[before..];
    let Some(next) = written.strip_prefix(' ') else {
        return;
    };
    // As the reader decides: not at a closing fence, quoted text, an
    // operator or white space.
    let continues = next.starts_with(['\u{2145}', '\u{2146}'])
        || (function
            && next.starts_with(|c: char| {
                !c.is_whitespace()
                    && !breaks_naryand(c)
                    && !matches!(c, '"' | ')' | ']' | '}' | '|' | '\u{2016}' | '〗')
            }));
    if continues {
        applied.push(before);
    }
}

/// Whether the n-aryand written at `out[start..]` is read back whole: it has
/// no space outside brackets but those of `applied` (in increasing order).
/// Spaces inside a balanced bracket pair are read with the group.
fn continues_naryand(out: &str, start: usize, applied: &[usize]) -> bool {
    let mut applied = applied.iter().peekable();
    let mut closers = SmallVec::<[char; 8]>::new();
    for (index, c) in out[start..].char_indices() {
        match c {
            '(' => closers.push(')'),
            '[' => closers.push(']'),
            '{' => closers.push('}'),
            '〖' => closers.push('〗'),
            ')' | ']' | '}' | '〗' => {
                if closers.pop() != Some(c) {
                    return false;
                }
            },
            // Quoted text may hold anything.
            '"' => return false,
            ' ' if closers.is_empty() && applied.next_if_eq(&&(start + index)).is_none() => {
                return false;
            },
            _ => {},
        }
    }
    closers.is_empty()
}

/// Insert an ignored source separator where the reader would otherwise
/// join the node written from `start` on to what precedes it: an operand
/// after a named function, a digit after a digit, and a prime after a node
/// that takes it as a superscript (UTN #28 reads `y′` as `y` primed, so the
/// sibling prime of LaTeX `y{'}` is written `y ′`).
fn separate_following_operand(
    out: &mut String,
    start: usize,
    previous_operator: impl FnOnce() -> bool,
    takes_prime: bool,
    previous: OperandEdge,
    current: OperandEdge,
) {
    let Some(first) = out[start..].chars().next() else {
        return;
    };
    let alphanumeric = is_alphanumeric(first);
    let signed = !alphanumeric
        && (starts_signed_script_operand(first)
            || pairs_bars(first)
            || matches!(first, '\u{2308}' | '\u{230A}'));
    let previous_operator = (alphanumeric || signed || first == '.') && previous_operator();
    // Adjacent numbers would read as one (`1 12′` is not `112′`).
    let digits = first.is_ascii_digit() && out[..start].ends_with(|c: char| c.is_ascii_digit());
    // A point would go on with the number that ends a script, a denominator
    // or an n-aryand (`x^2 .5` is not `x^2.5`).
    let point = first == '.';
    // A fraction's numerator and denominator are whole operands (UTN #28
    // section 2.1): a fraction after an operand (`a b/c`) and a factor
    // after a fraction (`a/b c`, `a/b (c)`) are set apart.
    let fraction = (current == OperandEdge::Fraction
        && (previous != OperandEdge::Break || out[..start].ends_with('.')))
        || (matches!(previous, OperandEdge::Fraction | OperandEdge::Naryand)
            && (alphanumeric || matches!(first, '(' | '〖' | '!' | '.')))
        // An n-aryand goes on at anything but an operator or a closing
        // fence (`∑ a [b]`, `∑ a/b ∞`).
        || (previous == OperandEdge::NaryandFunction && alphanumeric)
        || (previous == OperandEdge::Naryand
            && !first.is_whitespace()
            && match first {
                // An opening bar or bra goes on with it, a closing one does
                // not (`|∑ a|`).
                '|' | '\u{2016}' => !in_open_bar(&out[..start]),
                '\u{27E8}' => true,
                '"' | ')' | ']' | '}' | '〗' | '\u{27E9}' | '\u{2309}' | '\u{230B}' => false,
                _ => !breaks_naryand(first),
            });
    // A script that is only a sign takes an operand after it (`x^- y` is
    // not `x^-y`, UTN #28 appendix A `soperand`).
    let sign = previous_operator
        && (signed || starts_signed_script_operand(first))
        && ends_with_script_sign(&out[..start]);
    // A radical or box sign written alone (not a radical, which writes its
    // radicand) would take the operand after it (`√ y` is not `√y`).
    let before = &out[..start];
    let prefix = before.as_bytes().last().is_some_and(|byte| *byte >= 0x80)
        && before.ends_with(['√', '∛', '∜', '▭', '⟡'])
        && !first.is_whitespace();
    let joins = prefix
        || (previous_operator && (alphanumeric || point))
        || (takes_prime && is_prime_char(first))
        || digits
        || fraction
        || sign;
    if joins && !out[..start].ends_with(' ') {
        out.insert(start, ' ');
    }
}

/// Set the fence content at `out[start..]`, which starts with a fraction, apart
/// from an opening bar that the reader would not pair with the closing one
/// (`| a/b+|`, `|a| b/c||`).
fn separate_bar_content(out: &mut String, start: usize) {
    let Some(bar) = out[..start].chars().next_back().filter(|&c| is_bar(c)) else {
        return;
    };
    let open = start - bar.len_utf8();
    // Its bracket level up to it, or as much of it as is near.
    let mut level = 0;
    let mut depth = 0_usize;
    for (read, (index, c)) in out[..open].char_indices().rev().enumerate() {
        match c {
            ')' | ']' | '}' | '〗' => depth += 1,
            '(' | '[' | '{' | '〖' if depth == 0 => level = index + c.len_utf8(),
            '(' | '[' | '{' | '〖' => depth -= 1,
            _ if read == 128 => level = index,
            _ => {},
        }
        if level > 0 {
            break;
        }
    }
    let written = &out[level..];
    let pairs = |after: &str| {
        let text = format!("{written}{bar}{after}");
        pair_bars_at(&text, open - level) == Some(written.len())
    };
    if written.len() > 2048 || !pairs("") || !pairs("x") {
        out.insert(start, ' ');
    }
}

fn is_bar(c: char) -> bool {
    matches!(c, '|' | '\u{2016}')
}

/// Whether an odd number of bars are at the bracket level that `before` ends
/// in.
fn in_open_bar(before: &str) -> bool {
    let mut depth = 0_usize;
    let mut bars = 0_usize;
    for c in before.chars().rev().take(64) {
        match c {
            ')' | ']' | '}' | '〗' => depth += 1,
            '(' | '[' | '{' | '〖' if depth == 0 => break,
            '(' | '[' | '{' | '〖' => depth -= 1,
            '|' | '\u{2016}' if depth == 0 => bars += 1,
            _ => {},
        }
    }
    bars % 2 == 1
}

/// Separate the sibling written from `start` on from a script before it
/// that would take it (`x_i y`, not `x_iy`), in a list written without
/// [`write_nodes_with_style`] (a fence or a styled run).
fn separate_after_script(out: &mut String, start: usize, previous_operator: bool) {
    separate_following_operand(
        out,
        start,
        || previous_operator,
        false,
        OperandEdge::Break,
        OperandEdge::Break,
    );
}

/// Write an n-ary operator. `grouped`: group an n-aryand that ends in a
/// function name, which the reader would give the operand after it.
fn write_nary(out: &mut String, node: &MathNode, style: MathStyle, grouped: bool) {
    if let MathNode::Nary {
        op, sub, sup, body, ..
    } = node
    {
        out.push(*op);
        if let Some(s) = sub {
            out.push('_');
            write_script_arg(out, s, style.sub());
        }
        if let Some(s) = sup {
            out.push('^');
            write_script_arg(out, s, style.sup());
        }
        out.push(' ');
        write_naryand(out, body, style, grouped);
    }
}

/// Whether `node` is an n-aryand whose last factor is a function name.
fn ends_in_function(node: &MathNode, style: MathStyle) -> bool {
    let node = select_mathchoice(node, style);
    match node {
        MathNode::Row(children) => children
            .last()
            .is_some_and(|last| is_function_head(select_mathchoice(last, style))),
        MathNode::Nary { body, .. } => ends_in_function(body, style),
        _ => is_function_head(node),
    }
}

/// Whether the n-aryand before `next` must be kept from taking it: a script
/// with no base of its own joins what it follows there (`∏ 〖cos〗 〖〗_i`).
fn splits_naryand(next: &MathNode, style: MathStyle) -> bool {
    match select_mathchoice(next, style) {
        MathNode::Row(children) | MathNode::Styled(_, children) => children
            .first()
            .is_none_or(|first| splits_naryand(first, style)),
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. } => splits_naryand(base, style),
        MathNode::PreScripts { .. } => true,
        _ => false,
    }
}

/// Compact counterpart of [`write_nary`].
fn write_nary_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle, grouped: bool) {
    if let CompactNode::Nary {
        op, sub, sup, body, ..
    } = *arena.node(id)
    {
        out.push(op);
        if sub.is_some() {
            out.push('_');
            write_script_arg_c(out, arena, sub, style.sub());
        }
        if sup.is_some() {
            out.push('^');
            write_script_arg_c(out, arena, sup, style.sup());
        }
        out.push(' ');
        write_naryand_c(out, arena, body, style, grouped);
    }
}

/// Compact counterpart of [`ends_in_function`].
fn ends_in_function_c(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    let id = select_mathchoice_c(arena, id, style);
    match *arena.node(id) {
        CompactNode::Row(span) => arena.children(span).last().is_some_and(|&last| {
            is_function_head_c(arena, select_mathchoice_c(arena, last, style))
        }),
        CompactNode::Nary { body, .. } => ends_in_function_c(arena, body, style),
        _ => is_function_head_c(arena, id),
    }
}

/// Compact counterpart of [`splits_naryand`].
fn splits_naryand_c(arena: &AstArena, next: NodeId, style: MathStyle) -> bool {
    match *arena.node(select_mathchoice_c(arena, next, style)) {
        CompactNode::Row(span) | CompactNode::Styled(_, span) => arena
            .children(span)
            .first()
            .is_none_or(|&first| splits_naryand_c(arena, first, style)),
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => splits_naryand_c(arena, base, style),
        CompactNode::PreScripts { .. } => true,
        _ => false,
    }
}

/// Write an n-ary operator's n-aryand, which the reader takes as the
/// operand after the operator (UTN #28 section 3.4): a row of factors as is
/// (`∑_i a_i b_i`), and anything else that is not one operand, or that is
/// written with a space, in an invisible group (`∑ 〖a+b〗`): the reader
/// keeps parentheses around an n-aryand (`∏_k (1+x_k)`).
fn write_naryand(out: &mut String, node: &MathNode, style: MathStyle, grouped: bool) {
    let node = select_mathchoice(node, style);
    let plain = match node {
        // A fraction is a factor too (`∑ log 1/k`, `∫ f/x ⅆx`); the spaces
        // that would set it apart are checked below.
        MathNode::Row(children) => children.iter().all(|child| {
            !matches!(child, MathNode::StyleSwitch(_))
                && matches!(
                    operand_edge(select_mathchoice(child, style)),
                    OperandEdge::Operand | OperandEdge::Fraction
                )
        }),
        MathNode::Text(..) => false,
        // A word broken by a space (`i nf`) is not one operand.
        MathNode::Identifier(name) => !breaks_word(name),
        // A nested n-ary operator is a whole n-aryand (`∫∫ f`); a lone
        // symbol (LaTeX `\sum (`) has no better spelling.
        _ => true,
    };
    let start = out.len();
    let mut applied = SmallVec::new();
    if let MathNode::Row(children) = node {
        write_nodes_tracking(out, children, style, Some(&mut applied));
    } else if matches!(node, MathNode::Nary { .. }) {
        write_nary(out, node, style, grouped);
    } else {
        write_node(out, node, style);
    }
    // A space would end the n-aryand early (`∑ 〖y′ sin x〗`), but for one
    // after a function name (`∑ log k`) or before a differential.
    // An operator in a fence the reader does not take as one ends the
    // n-aryand (`∮ 〖|⋅|〗`).
    let is_bars = |node: &MathNode| {
        matches!(select_mathchoice(node, style), MathNode::Fenced { open, close, .. }
            if !is_bracket_pair(open, close))
    };
    let bars = match node {
        MathNode::Row(children) => children.iter().any(is_bars),
        node => is_bars(node),
    } && out[start..].contains(|c| c == ' ' || breaks_naryand(c));
    if reads_whole_with_bars(out, start, grouped) {
        return;
    }
    if bars
        || !plain
        || (grouped && is_function_head(node))
        || (matches!(node, MathNode::Row(_))
            && (!continues_naryand(out, start, &applied)
                || (grouped && ends_in_function(node, style))))
    {
        out.insert(start, '〖');
        out.push('〗');
    }
}

/// Whether the n-aryand written at `out[start..]` starts with a bar or a
/// bracket that pairs and needs no group (`∑_n |n⟩⟨n|`): the reader takes it whole, and no bar around it
/// pairs with its bars.
fn reads_whole_with_bars(out: &str, start: usize, grouped: bool) -> bool {
    let written = &out[start..];
    !grouped
        && written.starts_with(|c| pairs_bars(c) || matches!(c, '\u{2308}' | '\u{230A}'))
        && written.len() > 4
        && written.len() <= 256
        && !in_open_bar(&out[..start])
        && reads_as_naryand(written)
}

/// Compact counterpart of [`write_naryand`].
fn write_naryand_c(
    out: &mut String,
    arena: &AstArena,
    id: NodeId,
    style: MathStyle,
    grouped: bool,
) {
    let id = select_mathchoice_c(arena, id, style);
    let plain = match *arena.node(id) {
        // A fraction is a factor too (`∑ log 1/k`, `∫ f/x ⅆx`); the spaces
        // that would set it apart are checked below.
        CompactNode::Row(span) => arena.children(span).iter().all(|&child| {
            !matches!(*arena.node(child), CompactNode::StyleSwitch(_))
                && matches!(
                    operand_edge_c(arena, select_mathchoice_c(arena, child, style)),
                    OperandEdge::Operand | OperandEdge::Fraction
                )
        }),
        CompactNode::Text(..) => false,
        // A word broken by a space (`i nf`) is not one operand.
        CompactNode::Identifier(sid) => !breaks_word(arena.str_val(sid)),
        // A nested n-ary operator is a whole n-aryand (`∫∫ f`); a lone
        // symbol (LaTeX `\sum (`) has no better spelling.
        _ => true,
    };
    let start = out.len();
    let mut applied = SmallVec::new();
    let row = if let CompactNode::Row(span) = *arena.node(id) {
        write_nodes_c_tracking(out, arena, arena.children(span), style, Some(&mut applied));
        true
    } else if matches!(*arena.node(id), CompactNode::Nary { .. }) {
        write_nary_c(out, arena, id, style, grouped);
        false
    } else {
        write_node_c(out, arena, id, style);
        false
    };
    // A space would end the n-aryand early (`∑ 〖y′ sin x〗`), but for one
    // after a function name (`∑ log k`) or before a differential.
    // An operator in a fence the reader does not take as one ends the
    // n-aryand (`∮ 〖|⋅|〗`).
    let is_bars = |id: NodeId| {
        matches!(*arena.node(select_mathchoice_c(arena, id, style)),
            CompactNode::Fenced { open, close, .. }
                if !is_bracket_pair(arena.str_val(open), arena.str_val(close)))
    };
    let bars = match *arena.node(id) {
        CompactNode::Row(span) => arena.children(span).iter().any(|&child| is_bars(child)),
        _ => is_bars(id),
    } && out[start..].contains(|c| c == ' ' || breaks_naryand(c));
    if reads_whole_with_bars(out, start, grouped) {
        return;
    }
    if bars
        || !plain
        || (grouped && is_function_head_c(arena, id))
        || (row
            && (!continues_naryand(out, start, &applied)
                || (grouped && ends_in_function_c(arena, id, style))))
    {
        out.insert(start, '〖');
        out.push('〗');
    }
}

/// How a written node meets the operand around it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OperandEdge {
    /// An operator, punctuation, an opening delimiter, a big operator or a
    /// space: it ends the operand before it (as the reader's
    /// `breaks_operand`).
    Break,
    /// A factor of an operand.
    Operand,
    /// A fraction: its operands are whole, so it is not a factor of the
    /// operand around it.
    Fraction,
    /// An n-ary operator ending in its n-aryand, a whole operand that a
    /// factor after it would join.
    Naryand,
    /// An n-ary operator whose n-aryand ends in a function name: the reader
    /// gives it the operand after it, spaced or not.
    NaryandFunction,
}

fn is_bar_glyph(glyph: &str) -> bool {
    matches!(glyph, "|" | "\u{2016}")
}

fn symbol_edge(glyph: &str) -> OperandEdge {
    // The reader knows only the glyph, whatever the symbol's class here: an
    // ordinary minus (`\mathord{-}`) reads back as the operator, and `△`
    // (`\vartriangle`) as an ordinary symbol.
    // An opening bracket starts a group, where an operand starts too.
    let breaks = |c| breaks_operand_at(c) || matches!(c, '(' | '[' | '{');
    if !glyph.is_empty() && glyph.chars().all(breaks) {
        OperandEdge::Break
    } else {
        OperandEdge::Operand
    }
}

fn operand_edge(node: &MathNode) -> OperandEdge {
    match node {
        MathNode::Fraction { .. } => OperandEdge::Fraction,
        MathNode::Symbol(_, glyph) => symbol_edge(glyph),
        // Leading scripts end where their base ends.
        MathNode::PreScripts { base, .. } => operand_edge(base),
        // So is an n-ary operator with its n-aryand.
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
            if matches!(select_mathchoice(base, MathStyle::Text), MathNode::Nary { body, .. }
                if !matches!(&**body, MathNode::Row(children) if children.is_empty())) =>
        {
            OperandEdge::Operand
        },
        // So is a base with scripts of its own.
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
            if matches!(
                select_mathchoice(base, MathStyle::Text),
                MathNode::Subscript { .. }
                    | MathNode::Superscript { .. }
                    | MathNode::SubSup { .. }
                    | MathNode::Underscript { .. }
                    | MathNode::Overscript { .. }
                    | MathNode::UnderOver { .. }
            ) =>
        {
            OperandEdge::Operand
        },
        // A space is grouped to take a script.
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
            if matches!(
                select_mathchoice(base, MathStyle::Text),
                MathNode::Space(_) | MathNode::ExplicitSpace { .. }
            ) =>
        {
            OperandEdge::Operand
        },
        // A scripted operator is still an operator, as in the reader.
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. } => match operand_edge(base) {
            OperandEdge::Break => OperandEdge::Break,
            _ => OperandEdge::Operand,
        },
        // A bar it opens with goes on to its closing one (`∑ |a|`).
        MathNode::Nary { body, .. } if matches!(&**body, MathNode::Symbol(_, glyph) if is_bar_glyph(glyph)) => {
            OperandEdge::Operand
        },
        // A nested one ends as it does.
        MathNode::Nary { body, .. } if matches!(&**body, MathNode::Nary { .. }) => {
            match operand_edge(body) {
                OperandEdge::Operand => OperandEdge::Operand,
                _ => OperandEdge::Naryand,
            }
        },
        // An n-ary operator ends with its n-aryand, a whole operand that a
        // factor after it would join (`∑_i a_i b`), like a denominator.
        // (A lone operator symbol body, as LaTeX `\sum (` gives, does not.)
        MathNode::Nary { body, .. }
            if !matches!(&**body, MathNode::Row(c) if c.is_empty())
                && (operand_edge(body) != OperandEdge::Break
                    || matches!(&**body, MathNode::Text(..))) =>
        {
            OperandEdge::Naryand
        },
        // A dropped space is an operand character (U+200B).
        MathNode::Space(width) | MathNode::ExplicitSpace { width, .. } if width.as_f64() <= 0.0 => {
            OperandEdge::Operand
        },
        MathNode::Nary { .. }
        | MathNode::Space(_)
        | MathNode::ExplicitSpace { .. }
        | MathNode::Text(..) => OperandEdge::Break,
        _ => OperandEdge::Operand,
    }
}

fn operand_edge_c(arena: &AstArena, id: NodeId) -> OperandEdge {
    match *arena.node(id) {
        CompactNode::Fraction { .. } => OperandEdge::Fraction,
        CompactNode::Symbol(_, sid) => symbol_edge(arena.str_val(sid)),
        // Leading scripts end where their base ends.
        CompactNode::PreScripts { base, .. } => operand_edge_c(arena, base),
        // So is an n-ary operator with its n-aryand.
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
            if matches!(*arena.node(select_mathchoice_c(arena, base, MathStyle::Text)), CompactNode::Nary { body, .. }
                if !matches!(*arena.node(body), CompactNode::Row(span) if span.is_empty())) =>
        {
            OperandEdge::Operand
        },
        // So is a base with scripts of its own.
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
            if matches!(
                *arena.node(select_mathchoice_c(arena, base, MathStyle::Text)),
                CompactNode::Subscript { .. }
                    | CompactNode::Superscript { .. }
                    | CompactNode::SubSup { .. }
                    | CompactNode::Underscript { .. }
                    | CompactNode::Overscript { .. }
                    | CompactNode::UnderOver { .. }
            ) =>
        {
            OperandEdge::Operand
        },
        // A space is grouped to take a script.
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
            if matches!(
                *arena.node(select_mathchoice_c(arena, base, MathStyle::Text)),
                CompactNode::Space(_)
            ) =>
        {
            OperandEdge::Operand
        },
        // A scripted operator is still an operator, as in the reader.
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. } => match operand_edge_c(arena, base) {
            OperandEdge::Break => OperandEdge::Break,
            _ => OperandEdge::Operand,
        },
        // An n-ary operator ends with its n-aryand, a whole operand that a
        // factor after it would join (`∑_i a_i b`), like a denominator.
        // (A lone operator symbol body, as LaTeX `\sum (` gives, does not.)
        // A bar it opens with goes on to its closing one (`∑ |a|`).
        CompactNode::Nary { body, .. }
            if matches!(*arena.node(body), CompactNode::Symbol(_, sid)
                if is_bar_glyph(arena.str_val(sid))) =>
        {
            OperandEdge::Operand
        },
        // A nested one ends as it does.
        CompactNode::Nary { body, .. } if matches!(*arena.node(body), CompactNode::Nary { .. }) => {
            match operand_edge_c(arena, body) {
                OperandEdge::Operand => OperandEdge::Operand,
                _ => OperandEdge::Naryand,
            }
        },
        CompactNode::Nary { body, .. }
            if !matches!(*arena.node(body), CompactNode::Row(span) if span.is_empty())
                && (operand_edge_c(arena, body) != OperandEdge::Break
                    || matches!(*arena.node(body), CompactNode::Text(..))) =>
        {
            OperandEdge::Naryand
        },
        // A dropped space is an operand character (U+200B).
        CompactNode::Space(width) if width.as_f64() <= 0.0 => OperandEdge::Operand,
        CompactNode::Nary { .. } | CompactNode::Space(_) | CompactNode::Text(..) => {
            OperandEdge::Break
        },
        _ => OperandEdge::Operand,
    }
}

fn is_unicode_function(name: &str) -> bool {
    crate::unicode::symbols::unicode_math_operator_prefix(name)
        .is_some_and(|(word, _)| word == name)
}

fn needs_operator_terminator(node: &MathNode, style: MathStyle) -> bool {
    match select_mathchoice(node, style) {
        MathNode::Operator(attr) => is_unicode_function(&attr.name),
        // A radicand goes on through letters and digits (`√x y`).
        MathNode::Root { index, .. } if !matches!(&**index, MathNode::Number(s) if s == "3" || s == "4") => {
            false
        },
        MathNode::Sqrt(radicand) | MathNode::Root { radicand, .. } => matches!(
            select_mathchoice(radicand, style),
            MathNode::Identifier(_) | MathNode::Number(_)
        ),
        MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. }
        | MathNode::Underscript { .. }
        | MathNode::Overscript { .. }
        | MathNode::UnderOver { .. } => true,
        // An inner atom is bracketed: nothing joins it.
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        MathNode::Color { content, .. }
        | MathNode::MathAtom { content, .. }
        | MathNode::StructuredOperator { content, .. }
        | MathNode::AlignedCell { content, .. }
        | MathNode::SpannedCell { content, .. } => needs_operator_terminator(content, style),
        // Its n-aryand ends it (`∑ =^m a`).
        MathNode::Nary { body, .. } => needs_operator_terminator(body, style),
        MathNode::Cancel { content, .. }
        | MathNode::Scaled { content, .. }
        | MathNode::RaiseBox { content, .. }
        | MathNode::Reflect { content, .. } => needs_operator_terminator(content, style),
        MathNode::Row(children) | MathNode::Styled(_, children) => children
            .last()
            .is_some_and(|last| needs_operator_terminator(last, style)),
        // `(_1)lim b`: the prescripted base ends the written node.
        MathNode::PreScripts { base, .. } => needs_operator_terminator(base, style),
        // A denominator would take a digit or letter after it (`x/y^3 2`).
        MathNode::Fraction { .. } => true,
        _ => false,
    }
}

fn needs_space_before_with_style(node: &MathNode, style: MathStyle) -> bool {
    match select_mathchoice(node, style) {
        MathNode::Operator(_) | MathNode::Text(_, _) => true,
        MathNode::Row(children) => {
            let mut current_style = style;
            for child in children {
                if let MathNode::StyleSwitch(value) = child {
                    current_style = MathStyle::branch_from_switch(*value);
                    continue;
                }
                return needs_space_before_with_style(child, current_style);
            }
            false
        },
        MathNode::Styled(_, children) => {
            let mut current_style = style;
            for child in children {
                if let MathNode::StyleSwitch(value) = child {
                    current_style = MathStyle::branch_from_switch(*value);
                    continue;
                }
                return needs_space_before_with_style(child, current_style);
            }
            false
        },
        MathNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        MathNode::MathAtom { content, .. } | MathNode::StructuredOperator { content, .. } => {
            needs_space_before_with_style(content, style)
        },
        _ => false,
    }
}

// ── Compact arena-based write helpers ────────────────────────────────

// Performance note: the compact UnicodeMath writer is measurably slower than
// the legacy one (~147 ns vs ~111 ns on the quadratic formula), the only
// compact writer that does not beat its legacy counterpart. This is *not*
// avoidable redundant work: the dispatch (`match *arena.node(id)`) and the
// parenthesization logic are identical to the other compact writers and to the
// legacy path. The gap is the fixed arena-indirection cost (`arena.node` /
// `arena.str_val` lookups) showing through on the *cheapest* writer — the
// legacy UnicodeMath writer just appends `&str`s with almost no other work, so
// it has nothing to amortize the indirection against, unlike the LaTeX / MathML
// / OMML writers (whose legacy paths pay for escaping and tag construction).
// The absolute cost is sub-150 ns; closing the gap would require restructuring
// the recursive helpers to thread `&CompactNode` instead of `NodeId`, which is
// not worth the churn/regression risk for this format.

/// True when an over/under decoration node is the stretchy overline/underline
/// rule (U+203E), as produced by `\overline`/`\underline`/`\bar`. Such nodes are
/// rendered as a combining overline/underline; any other over/under content
/// (`\overset`, `\overbrace`, arrows, …) must be emitted, not dropped.
fn is_overunder_line(arena: &AstArena, id: NodeId) -> bool {
    matches!(arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(*sid) == "\u{203E}")
}

fn is_overunder_line_node(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, text) if text == "\u{203E}")
}

fn has_forced_operator_limits(node: &MathNode, style: MathStyle) -> bool {
    match node {
        MathNode::Operator(attr) => attr.limit_style == LimitStyle::AboveBelow,
        MathNode::StructuredOperator { form, .. } => {
            form.limit_style() == LimitStyle::AboveBelow
                || (style.is_display()
                    && matches!(form, OperatorNameForm::Starred(LimitStyle::Default)))
        },
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => has_forced_operator_limits(content, style),
        MathNode::Row(children) if children.len() == 1 => {
            has_forced_operator_limits(&children[0], style)
        },
        _ => false,
    }
}

#[inline(never)]
fn has_forced_operator_limits_c(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    match *arena.node(id) {
        CompactNode::Operator { limits, .. } => limits == LimitStyle::AboveBelow,
        CompactNode::StructuredOperator { form, .. } => {
            form.limit_style() == LimitStyle::AboveBelow
                || (style.is_display()
                    && matches!(form, OperatorNameForm::Starred(LimitStyle::Default)))
        },
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => has_forced_operator_limits_c(arena, content, style),
        CompactNode::Row(span) if arena.children(span).len() == 1 => {
            has_forced_operator_limits_c(arena, arena.children(span)[0], style)
        },
        _ => false,
    }
}

fn write_node_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    match *arena.node(id) {
        CompactNode::Number(sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Identifier(sid) => write_identifier(out, arena.str_val(sid)),
        CompactNode::Operator { name, .. } => out.push_str(arena.str_val(name)),
        // A literal solidus is quoted: `/` alone is the fraction operator
        // (UTN #28 sections 2.1 and 3.2).
        CompactNode::Symbol(_, sid) if arena.str_val(sid) == "/" => out.push_str("\\/"),
        CompactNode::Symbol(_, sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Text(_, sid) => write_quoted_text(out, arena.str_val(sid)),
        CompactNode::Space(r) => {
            let basis = arena.space_basis(id);
            write_unicode_space(out, crate::ast::style::local_space_width(r, basis, style));
        },

        CompactNode::Row(span) => write_nodes_c_with_style(out, arena, span, style),
        // Plain text, then each possible argument in parentheses.
        CompactNode::UndefinedCommand { name, arguments } => {
            write_undefined_command_name(out, arena.str_val(name));
            for &group in arena.children(arguments) {
                out.push('(');
                write_node_c(out, arena, group, style);
                out.push(')');
            }
        },
        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            write_node_c(out, arena, content, style)
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            if arena.str_val(open) == "{"
                && arena.str_val(close).is_empty()
                && let Some(array) = cases_array_c(arena, content)
            {
                // See the heap writer above.  Keep this path in lockstep with
                // it because ConvertResult normally writes the compact tree.
                write_compact_table(out, arena, array, 'Ⓒ');
                return;
            }
            out.push_str(arena.str_val(open));
            let start = out.len();
            // A `\middle` delimiter starts a new segment, with the fence's
            // style.
            let children = arena.children(content);
            let segments = children
                .split_inclusive(|&child| matches!(*arena.node(child), CompactNode::Middle(_)));
            for segment in segments {
                let (nodes, middle) = match segment.split_last() {
                    Some((&last, nodes)) if matches!(*arena.node(last), CompactNode::Middle(_)) => {
                        (nodes, Some(last))
                    },
                    _ => (segment, None),
                };
                write_nodes_c_tracking(out, arena, nodes, style, None);
                if let Some(CompactNode::Middle(sid)) = middle.map(|id| *arena.node(id)) {
                    out.push_str(arena.str_val(sid));
                }
            }
            if let Some(&first) = children.first()
                && operand_edge_c(arena, first) == OperandEdge::Fraction
            {
                separate_bar_content(out, start);
            }
            out.push_str(arena.str_val(close));
        },

        CompactNode::Subscript { base, sub } => {
            write_script_base_c(out, arena, base, style);
            out.push(if has_forced_operator_limits_c(arena, base, style) {
                '┬'
            } else {
                '_'
            });
            write_script_arg_c(out, arena, sub, style.sub());
        },

        CompactNode::Superscript { base, sup } => {
            write_script_base_c(out, arena, base, style);
            if !has_forced_operator_limits_c(arena, base, style)
                && let Some((primes, rest)) = prime_superscript_c(arena, base, sup)
            {
                // UTN #28 section 3.10: `x′`, `x′′`, `x′^c` (LaTeX `x'^c`).
                for &prime in &primes {
                    write_node_c(out, arena, prime, style.sup());
                }
                if let Some(rest) = rest {
                    out.push('^');
                    write_script_arg_c(out, arena, rest, style.sup());
                }
                return;
            }
            out.push(if has_forced_operator_limits_c(arena, base, style) {
                '┴'
            } else {
                '^'
            });
            write_script_arg_c(out, arena, sup, style.sup());
        },

        CompactNode::SubSup { base, sub, sup } => {
            // `x′_1^(n-1)`: primes inside the superscript would merge into
            // its row.
            if !has_forced_operator_limits_c(arena, base, style)
                && let Some((primes, Some(rest))) = prime_superscript_c(arena, base, sup)
                && !matches!(
                    *arena.node(select_mathchoice_c(arena, rest, style)),
                    CompactNode::Identifier(_) | CompactNode::Number(_) | CompactNode::Symbol(..)
                )
            {
                write_script_base_c(out, arena, base, style);
                for &prime in &primes {
                    write_node_c(out, arena, prime, style.sup());
                }
                out.push('_');
                write_script_arg_c(out, arena, sub, style.sub());
                out.push('^');
                write_script_arg_c(out, arena, rest, style.sup());
                return;
            }
            write_script_base_c(out, arena, base, style);
            out.push(if has_forced_operator_limits_c(arena, base, style) {
                '┬'
            } else {
                '_'
            });
            write_script_arg_c(out, arena, sub, style.sub());
            out.push(if has_forced_operator_limits_c(arena, base, style) {
                '┴'
            } else {
                '^'
            });
            write_script_arg_c(out, arena, sup, style.sup());
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // UTN #28 section 3.3: `(_sub^sup)base`.
            out.push('(');
            if pre_sub.is_some() {
                out.push('_');
                write_script_arg_c(out, arena, pre_sub, style.sub());
            }
            if pre_sup.is_some() {
                out.push('^');
                write_script_arg_c(out, arena, pre_sup, style.sup());
            }
            out.push(')');
            // The reader takes one operand as the base: group a word, a row
            // or a fraction (`(_1)〖xy〗`), and `(_1)` with nothing after it
            // is not a prescript.
            let grouped = match *arena.node(select_mathchoice_c(arena, base, style)) {
                CompactNode::Identifier(sid) => arena.str_val(sid).chars().nth(1).is_some(),
                CompactNode::Row(span) => !arena.children(span).is_empty(),
                // `(_1)a/b` is a fraction with a prescripted numerator.
                CompactNode::Fraction { .. } => true,
                _ => false,
            };
            let start = out.len();
            if grouped {
                out.push('〖');
            }
            // Its n-aryand must end with the base (`(^′)∑ 〖log〗 x`).
            let selected = select_mathchoice_c(arena, base, style);
            if matches!(*arena.node(selected), CompactNode::Nary { .. }) {
                write_nary_c(out, arena, selected, style, true);
            } else {
                write_node_c(out, arena, base, style);
            }
            if grouped {
                out.push('〗');
            } else {
                group_null_operand(out, start, false);
            }
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            write_bound_base_c(out, arena, base, style);
            write_bound(
                out,
                AccentPosition::Above,
                movable_limits,
                is_overunder_line(arena, over),
                |out| write_script_arg_c(out, arena, over, style.sup()),
            );
        },
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            write_bound_base_c(out, arena, base, style);
            write_bound(
                out,
                AccentPosition::Below,
                movable_limits,
                is_overunder_line(arena, under),
                |out| write_script_arg_c(out, arena, under, style.sub()),
            );
        },
        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            write_bound_base_c(out, arena, base, style);
            write_bound(
                out,
                AccentPosition::Below,
                movable_limits,
                is_overunder_line(arena, under),
                |out| write_script_arg_c(out, arena, under, style.sub()),
            );
            write_bound(
                out,
                AccentPosition::Above,
                movable_limits,
                is_overunder_line(arena, over),
                |out| write_script_arg_c(out, arena, over, style.sup()),
            );
        },

        CompactNode::Fraction { num, den, kind } => {
            let style = kind.forced_style().unwrap_or(style);
            write_fraction_arg_c(out, arena, num, style.fracnum());
            out.push(fraction_operator(kind));
            let start = out.len();
            write_fraction_arg_c(out, arena, den, style.fracden());
            group_negatable(out, start);
        },

        CompactNode::Sqrt(inner) => {
            out.push('\u{221A}');
            write_paren_if_complex_c(out, arena, inner, style.cramped());
        },

        CompactNode::Root { index, radicand } => {
            let idx_node = *arena.node(index);
            match idx_node {
                CompactNode::Number(sid) if arena.str_val(sid) == "3" => {
                    out.push('\u{221B}');
                    write_paren_if_complex_c(out, arena, radicand, style.cramped());
                },
                CompactNode::Number(sid) if arena.str_val(sid) == "4" => {
                    out.push('\u{221C}');
                    write_paren_if_complex_c(out, arena, radicand, style.cramped());
                },
                _ => {
                    out.push('\u{221A}');
                    out.push('(');
                    let start = out.len();
                    write_node_c(out, arena, index, style.sup().sup());
                    group_null_operand(out, start, false);
                    out.push('&');
                    let start = out.len();
                    write_node_c(out, arena, radicand, style.cramped());
                    group_null_operand(out, start, false);
                    out.push(')');
                },
            }
        },

        CompactNode::Nary { .. } => write_nary_c(out, arena, id, style, false),

        CompactNode::Accent {
            base,
            accent,
            position,
            ..
        } => {
            let start = out.len();
            write_accent_base_c(out, arena, base, style);
            if out.len() == start {
                out.push_str("〖〗");
            }
            if !is_unicode_combining_accent(accent) {
                out.push(match position {
                    AccentPosition::Above => '┴',
                    AccentPosition::Below => '┬',
                });
            }
            out.push(accent);
        },

        CompactNode::Array { .. } => {
            write_compact_table(out, arena, id, '\u{25A0}');
        },

        CompactNode::Phantom(inner, _) => {
            out.push('\u{27E1}');
            write_paren_if_complex_c(out, arena, inner, style);
        },

        CompactNode::Boxed { content: inner, .. } => {
            out.push('\u{25AD}');
            write_paren_if_complex_c(out, arena, inner, style);
        },
        CompactNode::Actuarial { content: inner, .. } => {
            out.push('\u{25AD}');
            out.push('(');
            out.push_str("6&");
            write_node_c(out, arena, inner, style);
            out.push(')');
        },

        CompactNode::Cancel { content, .. } => {
            write_node_c(out, arena, content, style);
        },

        CompactNode::Scaled { content, .. } => {
            write_node_c(out, arena, content, style);
        },
        CompactNode::RaiseBox { content, .. } => {
            write_node_c(out, arena, content, style);
        },

        CompactNode::Styled(text_style, span) => {
            let mut current_style = style;
            let mut previous_operator = false;
            for &child_id in arena.children(span) {
                let child = *arena.node(child_id);
                if let CompactNode::StyleSwitch(value) = child {
                    current_style = MathStyle::branch_from_switch(value);
                    continue;
                }
                let start = out.len();
                let terminates = needs_operator_terminator_c(arena, child_id, current_style);
                match child {
                    CompactNode::Identifier(sid)
                    | CompactNode::Number(sid)
                    | CompactNode::Text(_, sid) => {
                        for ch in arena.str_val(sid).chars() {
                            let styled =
                                crate::unicode::fonts::styled_char(ch, text_style).unwrap_or(ch);
                            out.push(styled);
                        }
                    },
                    _ => write_node_c(out, arena, child_id, current_style),
                }
                separate_after_script(out, start, previous_operator);
                previous_operator = terminates;
            }
        },

        CompactNode::Color { content, .. } => {
            // Plain-text UnicodeMath has no portable color syntax, so the color
            // is dropped (see `unicode_math_loss`); the inner content is always
            // emitted so nothing is lost from the rendered expression.
            write_node_c(out, arena, content, style);
        },

        CompactNode::MathChoice { .. } => {
            write_node_c(out, arena, select_mathchoice_c(arena, id, style), style);
        },

        // An explicit invisible group reads back as an inner atom; without
        // its brackets the content would merge into what is around it
        // (`〖〖〗_1〗y` is not `〖〗_1 y`, a leading script of y).
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            content,
        } => {
            out.push('〖');
            write_node_c(out, arena, content, style);
            out.push('〗');
        },
        CompactNode::MathAtom { atom_type, content } => {
            let start = out.len();
            write_node_c(out, arena, content, style);
            // An empty ordinary atom keeps what is around it apart.
            if atom_type == MathAtomType::Ord && out.len() == start {
                out.push(ZERO_WIDTH_SPACE);
            }
        },
        CompactNode::StructuredOperator { content, .. } => {
            write_structured_operator_c(out, arena, content, style);
        },
        CompactNode::Reflect { content, .. } => write_node_c(out, arena, content, style),

        CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_)
        | CompactNode::Newline => {},
    }
}

#[cold]
#[inline(never)]
fn write_structured_operator_c(
    out: &mut String,
    arena: &AstArena,
    content: NodeId,
    style: MathStyle,
) {
    let grouped = structured_content_needs_group_c(arena, content);
    if grouped {
        out.push('〖');
    }
    write_structured_content_c(out, arena, content, style);
    if grouped {
        out.push('〗');
    }
}

fn structured_content_needs_group_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Row(span) | CompactNode::Styled(_, span) => arena.children(span).len() > 1,
        CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. } => {
            structured_content_needs_group_c(arena, content)
        },
        CompactNode::Accent { accent, .. } => !is_unicode_combining_accent(accent),
        CompactNode::Overscript {
            over,
            movable_limits,
            ..
        } => !movable_limits || !is_overunder_line(arena, over),
        CompactNode::Underscript {
            under,
            movable_limits,
            ..
        } => !movable_limits || !is_overunder_line(arena, under),
        CompactNode::UnderOver { .. } => true,
        CompactNode::Fraction { .. }
        | CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. } => false,
        _ => false,
    }
}

fn write_structured_nodes_c_with_style(
    out: &mut String,
    arena: &AstArena,
    span: crate::ast::compact::NodeSpan,
    style: MathStyle,
) {
    let mut current_style = style;
    let mut wrote_node = false;
    let mut previous_operator = false;
    let mut takes_prime = false;
    let mut edge = OperandEdge::Break;
    let mut identifier_run = String::new();
    for &id in arena.children(span) {
        if let CompactNode::StyleSwitch(value) = *arena.node(id) {
            if !identifier_run.is_empty() {
                write_structured_identifier_run(out, &identifier_run);
                identifier_run.clear();
                wrote_node = true;
                previous_operator = false;
                takes_prime = true;
                edge = OperandEdge::Operand;
            }
            current_style = MathStyle::branch_from_switch(value);
            continue;
        }
        if let CompactNode::Identifier(sid) = *arena.node(id) {
            identifier_run.push_str(arena.str_val(sid));
            continue;
        }
        if !identifier_run.is_empty() {
            write_structured_identifier_run(out, &identifier_run);
            identifier_run.clear();
            wrote_node = true;
            previous_operator = false;
            takes_prime = true;
            edge = OperandEdge::Operand;
        }
        if wrote_node
            && !matches!(*arena.node(id), CompactNode::Text(_, _))
            && needs_space_before_c_with_style(arena, id, current_style)
        {
            out.push(' ');
        }
        let start = out.len();
        write_structured_content_c(out, arena, id, current_style);
        let current = operand_edge_c(arena, select_mathchoice_c(arena, id, current_style));
        separate_following_operand(out, start, || previous_operator, takes_prime, edge, current);
        previous_operator = needs_operator_terminator_c(arena, id, current_style);
        takes_prime = !is_prime_symbol_c(arena, id);
        edge = current;
        wrote_node = true;
    }
    if !identifier_run.is_empty() {
        write_structured_identifier_run(out, &identifier_run);
    }
}

fn write_accent_c(
    out: &mut String,
    arena: &AstArena,
    base: NodeId,
    accent: char,
    position: AccentPosition,
    style: MathStyle,
) {
    if !is_unicode_combining_accent(accent) {
        write_structured_decorated_base_c(out, arena, base, style);
        out.push(match position {
            AccentPosition::Above => '┴',
            AccentPosition::Below => '┬',
        });
        out.push(accent);
    } else {
        write_structured_decorated_base_c(out, arena, base, style);
        out.push(accent);
    }
}

fn structured_decorated_base_needs_group_c(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Row(span) | CompactNode::Styled(_, span) => arena.children(span).len() > 1,
        CompactNode::Fraction { .. }
        | CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. }
        | CompactNode::Accent { .. }
        | CompactNode::StructuredOperator { .. } => true,
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        CompactNode::Color { content, .. } | CompactNode::MathAtom { content, .. } => {
            structured_decorated_base_needs_group_c(arena, content)
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => [display, text, script, script_script]
            .into_iter()
            .any(|branch| structured_decorated_base_needs_group_c(arena, branch)),
        _ => false,
    }
}

fn write_structured_decorated_base_c(
    out: &mut String,
    arena: &AstArena,
    id: NodeId,
    style: MathStyle,
) {
    if structured_decorated_base_needs_group_c(arena, id) {
        out.push('〖');
        write_structured_content_c(out, arena, id, style);
        out.push('〗');
    } else {
        write_structured_content_c(out, arena, id, style);
    }
}

fn write_structured_content_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    match *arena.node(id) {
        CompactNode::Identifier(value) => {
            write_structured_identifier_run(out, arena.str_val(value))
        },
        CompactNode::Row(span) => write_structured_nodes_c_with_style(out, arena, span, style),
        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if is_overunder_line(arena, over) || ends_in_bound_c(arena, base) {
                write_structured_decorated_base_c(out, arena, base, style);
            } else {
                write_structured_content_c(out, arena, base, style);
            }
            write_bound(
                out,
                AccentPosition::Above,
                movable_limits,
                is_overunder_line(arena, over),
                |out| write_structured_bound_c(out, arena, over, style.sup()),
            );
        },
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if is_overunder_line(arena, under) || ends_in_bound_c(arena, base) {
                write_structured_decorated_base_c(out, arena, base, style);
            } else {
                write_structured_content_c(out, arena, base, style);
            }
            write_bound(
                out,
                AccentPosition::Below,
                movable_limits,
                is_overunder_line(arena, under),
                |out| write_structured_bound_c(out, arena, under, style.sub()),
            );
        },
        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            if ends_in_bound_c(arena, base) {
                write_structured_decorated_base_c(out, arena, base, style);
            } else {
                write_structured_content_c(out, arena, base, style);
            }
            write_bound(
                out,
                AccentPosition::Below,
                movable_limits,
                is_overunder_line(arena, under),
                |out| write_structured_bound_c(out, arena, under, style.sub()),
            );
            write_bound(
                out,
                AccentPosition::Above,
                movable_limits,
                is_overunder_line(arena, over),
                |out| write_structured_bound_c(out, arena, over, style.sup()),
            );
        },
        CompactNode::Accent {
            base,
            accent,
            position,
            ..
        } => write_accent_c(out, arena, base, accent, position, style),
        CompactNode::Styled(text_style, span) => {
            let mut current_style = style;
            for &child in arena.children(span) {
                if let CompactNode::StyleSwitch(value) = *arena.node(child) {
                    current_style = MathStyle::branch_from_switch(value);
                    continue;
                }
                match *arena.node(child) {
                    CompactNode::Identifier(sid)
                    | CompactNode::Number(sid)
                    | CompactNode::Text(_, sid) => {
                        for ch in arena.str_val(sid).chars() {
                            out.push(
                                crate::unicode::fonts::styled_char(ch, text_style).unwrap_or(ch),
                            );
                        }
                    },
                    _ => write_structured_content_c(out, arena, child, current_style),
                }
            }
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => write_structured_content_c(
            out,
            arena,
            style.choose(display, text, script, script_script),
            style,
        ),
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            content,
        } => {
            out.push('〖');
            // The group already keeps a word one operand: no quotes.
            if let CompactNode::Identifier(word) = *arena.node(content) {
                out.push_str(arena.str_val(word));
            } else {
                write_structured_content_c(out, arena, content, style);
            }
            out.push('〗');
        },
        CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. } => {
            write_structured_content_c(out, arena, content, style);
        },
        _ => write_node_c(out, arena, id, style),
    }
}

fn write_base_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    if let CompactNode::MathAtom {
        atom_type: MathAtomType::Op,
        content,
    } = *arena.node(id)
        && operator_body_needs_group_c(arena, content, style)
    {
        out.push('〖');
        write_node_c(out, arena, id, style);
        out.push('〗');
        return;
    }
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Operator { name, limits } if needs_side_group(arena.str_val(name), limits) => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. } => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        // The reader takes no other fence than brackets as one (`〖|x|〗^2`).
        CompactNode::Fenced { open, close, .. }
            if !is_bracket_pair(arena.str_val(open), arena.str_val(close)) =>
        {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        // A script after a prescripted base would be the base's own.
        CompactNode::PreScripts { .. } => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        // A compound base is an invisible group: `()` would read back as
        // visible parentheses (`〖a+b〗^2` is LaTeX `{a+b}^2`, `x〖〗_1` is
        // `x{}_1`).
        CompactNode::Row(_) | CompactNode::Fraction { .. } => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        // The script would join the n-aryand.
        CompactNode::Nary { body, .. } if !matches!(*arena.node(body), CompactNode::Row(span) if span.is_empty()) =>
        {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        _ => write_node_c(out, arena, id, style),
    }
}

/// Compact counterpart of [`write_script_base`].
fn write_script_base_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    // A letter or a number is an operand as it is.
    if let CompactNode::Identifier(sid) | CompactNode::Number(sid) = *arena.node(id)
        && arena.str_val(sid).len() == 1
    {
        out.push_str(arena.str_val(sid));
        return;
    }
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Identifier(sid) if arena.str_val(sid).chars().nth(1).is_some() => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        CompactNode::Symbol(_, sid) if arena.str_val(sid).chars().nth(1).is_some() => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        // A space takes no script of its own (`〖\u{2009}〗′`).
        CompactNode::Space(_) => {
            out.push('〖');
            write_node_c(out, arena, id, style);
            out.push('〗');
        },
        _ => write_base_c(out, arena, id, style),
    }
}

/// A superscript UTN #28 writes as primes after its base: a run of primes
/// (LaTeX `x''`), optionally followed by one more superscript atom (LaTeX
/// `x'^2`, which the reader merges back into the same superscript).
fn prime_superscript_c(
    arena: &AstArena,
    base: NodeId,
    sup: NodeId,
) -> Option<(SmallVec<[NodeId; 4]>, Option<NodeId>)> {
    let is_prime = |id: NodeId| {
        matches!(*arena.node(id), CompactNode::Symbol(SymbolKind::Ordinary, sid)
            if is_prime_glyph(arena.str_val(sid)))
    };
    // Primes right after an n-ary operator are its limit.
    if matches!(*arena.node(base), CompactNode::Nary { .. }) {
        return None;
    }
    // The reader gives such a base the primes after it (not `′′` or `+′`).
    if matches!(*arena.node(base), CompactNode::Symbol(_, sid)
        if !symbol_takes_prime(arena.str_val(sid)))
    {
        return None;
    }
    if is_prime(sup) {
        return Some((SmallVec::from_slice(&[sup]), None));
    }
    let CompactNode::Row(span) = *arena.node(sup) else {
        return None;
    };
    let children = arena.children(span);
    let primes = children.iter().take_while(|&&id| is_prime(id)).count();
    match &children[primes..] {
        _ if primes == 0 => None,
        [] => Some((SmallVec::from_slice(children), None)),
        [rest] => Some((SmallVec::from_slice(&children[..primes]), Some(*rest))),
        _ => None,
    }
}

/// Compact counterpart of [`write_script_arg`].
fn write_script_arg_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    // A letter or a number is an operand as it is.
    if let CompactNode::Identifier(sid) | CompactNode::Number(sid) = *arena.node(id)
        && arena.str_val(sid).len() == 1
    {
        out.push_str(arena.str_val(sid));
        return;
    }
    let start = out.len();
    write_script_arg_unguarded_c(out, arena, id, style);
    let fenced = matches!(
        *arena.node(select_mathchoice_c(arena, id, style)),
        CompactNode::Fenced { .. }
    );
    group_null_operand(out, start, !fenced);
}

fn write_script_arg_unguarded_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        // The reader skips a space after `^`; in parentheses it is content.
        CompactNode::Space(_) => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        CompactNode::Identifier(sid) => write_script_word(out, arena.str_val(sid)),
        CompactNode::Row(span) if write_simple_script(out, script_items_c(arena, span)) => {},
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        // The reader takes no other fence than brackets as one operand, and
        // scripts after a prescripted operand as its own.
        CompactNode::Fenced { open, close, .. }
            if !is_bracket_pair(arena.str_val(open), arena.str_val(close)) =>
        {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        CompactNode::PreScripts { .. } => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        // An accent on more than an atom or a group: the script would end
        // before it.
        CompactNode::Accent { base, .. }
            if !(matches!(*arena.node(select_mathchoice_c(arena, base, style)),
                CompactNode::Symbol(_, sid) if !needs_script_group(arena.str_val(sid)))
                || matches!(
                    *arena.node(select_mathchoice_c(arena, base, style)),
                    CompactNode::Identifier(_)
                        | CompactNode::Number(_)
                        | CompactNode::Operator { .. }
                        | CompactNode::MathAtom { .. }
                        | CompactNode::Accent { .. }
                )) =>
        {
            let start = out.len();
            write_node_c(out, arena, id, style);
            if !out[start..].starts_with('〖') {
                out.insert(start, '(');
                out.push(')');
            }
        },
        // A symbol of several characters, or a quoted solidus.
        CompactNode::Symbol(_, sid) if needs_script_group(arena.str_val(sid)) => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        // The reader drops parentheses around a whole operand.
        CompactNode::Fenced { open, close, .. }
            if arena.str_val(open) == "(" && arena.str_val(close) == ")" =>
        {
            let start = out.len();
            write_node_c(out, arena, id, style);
            // It keeps empty ones.
            if &out[start..] != "()" {
                out.insert(start, '(');
                out.push(')');
            }
        },
        CompactNode::Sqrt(_)
        | CompactNode::Root { .. }
        | CompactNode::Fraction { .. }
        | CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. }
        | CompactNode::Phantom(..)
        | CompactNode::Boxed { .. }
        | CompactNode::Actuarial { .. }
        | CompactNode::Reflect { .. } => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        _ => {
            let start = out.len();
            write_node_c(out, arena, id, style);
            group_spaced_script(out, start);
        },
    }
}

/// Compact counterpart of [`write_fraction_arg`].
fn write_fraction_arg_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    // A letter or a number is an operand as it is.
    if let CompactNode::Identifier(sid) | CompactNode::Number(sid) = *arena.node(id)
        && arena.str_val(sid).len() == 1
    {
        out.push_str(arena.str_val(sid));
        return;
    }
    let id = select_mathchoice_c(arena, id, style);
    let start = out.len();
    match *arena.node(id) {
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        CompactNode::Fraction { .. } => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        CompactNode::Fenced { open, close, .. }
            if arena.str_val(open) == "("
                || !is_bracket_pair(arena.str_val(open), arena.str_val(close)) =>
        {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        // An operator alone is not an operand (`(+)/2`), and an n-ary
        // operator would take the fraction as its n-aryand (`(∑ a)/b`).
        // A closing fence would take the run from its opening one.
        _ if matches!(*arena.node(id), CompactNode::Nary { .. })
            || operand_edge_c(arena, id) == OperandEdge::Break
            || ends_in_fence_c(arena, id) =>
        {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        _ => write_node_c(out, arena, id, style),
    }
    let word = out[start..].contains(' ') && matches!(*arena.node(id), CompactNode::Identifier(_));
    // A broken word is one operand (`(i nf)/2`).
    if word {
        out.insert(start, '(');
        out.push(')');
    }
    group_null_operand(out, start, true);
}

/// Compact counterpart of [`write_paren_if_complex`].
fn write_paren_if_complex_c(out: &mut String, arena: &AstArena, id: NodeId, style: MathStyle) {
    let start = out.len();
    write_paren_if_complex_unguarded_c(out, arena, id, style);
    group_null_operand(out, start, true);
}

fn write_paren_if_complex_unguarded_c(
    out: &mut String,
    arena: &AstArena,
    id: NodeId,
    style: MathStyle,
) {
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        // Already bracketed.
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => write_node_c(out, arena, id, style),
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
        CompactNode::Identifier(sid) if arena.str_val(sid).chars().count() == 1 => {
            write_node_c(out, arena, id, style);
        },
        CompactNode::Number(sid) if arena.str_val(sid).chars().count() == 1 => {
            write_node_c(out, arena, id, style);
        },
        _ => {
            out.push('(');
            write_node_c(out, arena, id, style);
            out.push(')');
        },
    }
}

#[inline]
fn select_mathchoice_c(arena: &AstArena, id: NodeId, style: MathStyle) -> NodeId {
    // Most nodes are themselves.
    match *arena.node(id) {
        CompactNode::Identifier(_)
        | CompactNode::Number(_)
        | CompactNode::Symbol(..)
        | CompactNode::Operator { .. }
        | CompactNode::Space(_)
        | CompactNode::Text(..)
        | CompactNode::Superscript { .. }
        | CompactNode::Subscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Fraction { .. } => id,
        _ => select_operand_c(arena, id, style),
    }
}

fn select_operand_c(arena: &AstArena, id: NodeId, style: MathStyle) -> NodeId {
    let mut id = id;
    loop {
        id = match *arena.node(id) {
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => style.choose(display, text, script, script_script),
            // As in `select_mathchoice`.
            CompactNode::Row(span) if arena.children(span).len() == 1 => arena.children(span)[0],
            CompactNode::MathAtom { atom_type, content }
                if atom_type != MathAtomType::Inner
                    && !matches!(*arena.node(content), CompactNode::Row(span) if span.is_empty()) =>
            {
                content
            },
            CompactNode::Color { content, .. }
            | CompactNode::Cancel { content, .. }
            | CompactNode::Scaled { content, .. }
            | CompactNode::RaiseBox { content, .. }
            | CompactNode::Reflect { content, .. } => content,
            _ => return id,
        };
    }
}

fn write_nodes_c_with_style(
    out: &mut String,
    arena: &AstArena,
    span: crate::ast::compact::NodeSpan,
    style: MathStyle,
) {
    write_nodes_c_tracking(out, arena, arena.children(span), style, None);
}

/// The argument of a function that is several nodes is one in `〖…〗`
/// (`sin⁡〖xy〗`, UTN #28): what is written from `start` on, after U+2061 and
/// perhaps a space, which the group takes the place of. Returns where the
/// argument starts now.
fn group_argument(out: &mut String, start: usize) -> usize {
    if out.len() == start {
        return start;
    }
    let before = out[..start].trim_end().len();
    out.replace_range(before..start, "〖");
    out.push('〗');
    before
}

/// Compact counterpart of [`write_nodes_tracking`].
fn write_nodes_c_tracking(
    out: &mut String,
    arena: &AstArena,
    children: &[NodeId],
    style: MathStyle,
    mut applied: Option<&mut SmallVec<[usize; 4]>>,
) {
    let mut current_style = style;
    let mut wrote_node = false;
    let mut previous = None;
    let mut takes_prime = false;
    let mut function = false;
    let mut edge = OperandEdge::Break;
    // Start of the letters of the identifiers written last.
    let mut word = None;
    let mut struck = false;
    for (index, &id) in children.iter().enumerate() {
        if let CompactNode::StyleSwitch(value) = *arena.node(id) {
            current_style = MathStyle::branch_from_switch(value);
            continue;
        }
        // LaTeX `\not` strikes the symbol after it: `≉`, or `≴`.
        if std::mem::take(&mut struck) {
            continue;
        }
        if let CompactNode::Symbol(_, sid) = *arena.node(id)
            && arena.str_val(sid) == "\u{0338}"
            && let Some(&next) = children.get(index + 1)
            && let CompactNode::Symbol(_, next) =
                *arena.node(select_mathchoice_c(arena, next, current_style))
            && let Some(negated) = negated_glyph(arena.str_val(next))
        {
            track_word(out, &mut word, out.len(), false);
            out.push_str(&negated);
            edge = symbol_edge(arena.str_val(next));
            previous = None;
            takes_prime = true;
            function = false;
            wrote_node = true;
            struck = true;
            continue;
        }
        // A letter after a letter: nothing comes between, and what both meet
        // stays the same.
        if let CompactNode::Identifier(sid) = *arena.node(id)
            && word.is_some()
            && out.as_bytes().last().is_some_and(u8::is_ascii_alphabetic)
            && arena
                .str_val(sid)
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
        {
            out.push_str(arena.str_val(sid));
            continue;
        }
        let selected = select_mathchoice_c(arena, id, current_style);
        let node = *arena.node(selected);
        // A letter, number or symbol needs none of the checks of a compound.
        let atom = matches!(
            node,
            CompactNode::Identifier(_)
                | CompactNode::Number(_)
                | CompactNode::Symbol(..)
                | CompactNode::Space(_)
        );
        let identifier = if atom {
            matches!(node, CompactNode::Identifier(_))
        } else {
            is_word_c(arena, selected)
        };
        let mut before = out.len();
        if wrote_node && !atom && needs_space_before_c_with_style(arena, selected, current_style) {
            out.push(' ');
        }
        let mut start = out.len();
        let next = || {
            children[index + 1..]
                .iter()
                .find(|&&next| match *arena.node(next) {
                    CompactNode::StyleSwitch(_) => false,
                    CompactNode::Row(span) => !span.is_empty(),
                    _ => true,
                })
        };
        let open = matches!(node, CompactNode::Nary { .. })
            && ends_in_function_c(arena, selected, current_style);
        let grouped =
            open && next().is_some_and(|&next| splits_naryand_c(arena, next, current_style));
        // Within a list a word is broken once what follows it is written.
        if let CompactNode::Identifier(sid) | CompactNode::Number(sid) = node {
            out.push_str(arena.str_val(sid));
        } else if let CompactNode::Symbol(_, sid) = node
            && arena.str_val(sid) != "/"
        {
            out.push_str(arena.str_val(sid));
        } else if identifier {
            write_word_c(out, arena, selected);
        } else if grouped {
            write_nary_c(out, arena, selected, current_style, true);
        } else {
            write_node_c(out, arena, selected, current_style);
        }
        if matches!(node, CompactNode::Row(span) if span.len() > 1)
            && index > 0
            && matches!(*arena.node(children[index - 1]), CompactNode::Symbol(_, sid)
                if arena.str_val(sid) == "\u{2061}")
        {
            start = group_argument(out, start);
            before = before.min(start);
        }
        let current = match node {
            CompactNode::Identifier(_) | CompactNode::Number(_) => OperandEdge::Operand,
            _ => operand_edge_c(arena, selected),
        };
        let current = if open && !grouped && current == OperandEdge::Naryand {
            OperandEdge::NaryandFunction
        } else {
            current
        };
        // Asked only where it matters: it follows the previous node down.
        let previous_operator = || {
            previous.is_some_and(|(node, style)| needs_operator_terminator_c(arena, node, style))
        };
        separate_following_operand(out, start, previous_operator, takes_prime, edge, current);
        if let Some(applied) = applied.as_deref_mut() {
            record_applied_space(applied, out, before, function);
        }
        // A node written as nothing separates nothing.
        if out.len() == before {
            continue;
        }
        track_word(out, &mut word, start, identifier);
        if matches!(node, CompactNode::PreScripts { .. })
            && let Some(letter) = prescripted_letter_c(arena, selected, current_style)
        {
            word = Some(out.len() - letter.len());
        }
        previous = (!atom).then_some((selected, current_style));
        takes_prime =
            !(matches!(node, CompactNode::Symbol(..)) && is_prime_symbol_c(arena, selected));
        function = applied.is_some() && !atom && is_function_head_c(arena, selected);
        edge = current;
        wrote_node = true;
    }
    if let Some(from) = word {
        break_function_names(out, from, out.len());
    }
}

fn needs_operator_terminator_c(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    // Follows the node written last, so one path.
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Operator { name, .. } => is_unicode_function(arena.str_val(name)),
        // A radicand goes on through letters and digits (`√x y`).
        CompactNode::Root { index, .. }
            if !matches!(*arena.node(index), CompactNode::Number(sid)
                if matches!(arena.str_val(sid), "3" | "4")) =>
        {
            false
        },
        CompactNode::Sqrt(radicand) | CompactNode::Root { radicand, .. } => matches!(
            *arena.node(select_mathchoice_c(arena, radicand, style)),
            CompactNode::Identifier(_) | CompactNode::Number(_)
        ),
        CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. }
        | CompactNode::Underscript { .. }
        | CompactNode::Overscript { .. }
        | CompactNode::UnderOver { .. } => true,
        // A denominator would take a digit or letter after it (`x/y^3 2`).
        CompactNode::Fraction { .. } => true,
        // An inner atom is bracketed: nothing joins it.
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        // An n-ary operator ends with its n-aryand (`∑ =^m a`), and a
        // prescript with its base (`(_1)lim b`).
        CompactNode::Nary { body: content, .. }
        | CompactNode::PreScripts { base: content, .. }
        | CompactNode::Cancel { content, .. }
        | CompactNode::Scaled { content, .. }
        | CompactNode::RaiseBox { content, .. }
        | CompactNode::Reflect { content, .. }
        | CompactNode::AlignedCell { content, .. }
        | CompactNode::SpannedCell { content, .. }
        | CompactNode::Color { content, .. }
        | CompactNode::MathAtom { content, .. }
        | CompactNode::StructuredOperator { content, .. } => {
            needs_operator_terminator_c(arena, content, style)
        },
        CompactNode::Row(span) | CompactNode::Styled(_, span) => arena
            .children(span)
            .last()
            .is_some_and(|&last| needs_operator_terminator_c(arena, last, style)),
        _ => false,
    }
}

fn needs_space_before_c_with_style(arena: &AstArena, id: NodeId, style: MathStyle) -> bool {
    // Follows the node written first, so one path.
    match *arena.node(select_mathchoice_c(arena, id, style)) {
        CompactNode::Operator { .. } | CompactNode::Text(_, _) => true,
        CompactNode::MathAtom {
            atom_type: MathAtomType::Inner,
            ..
        } => false,
        CompactNode::MathAtom { content, .. } | CompactNode::StructuredOperator { content, .. } => {
            needs_space_before_c_with_style(arena, content, style)
        },
        CompactNode::Row(span) | CompactNode::Styled(_, span) => {
            let mut current_style = style;
            for &child in arena.children(span) {
                if let CompactNode::StyleSwitch(value) = *arena.node(child) {
                    current_style = MathStyle::branch_from_switch(value);
                    continue;
                }
                return needs_space_before_c_with_style(arena, child, current_style);
            }
            false
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UnicodeMath of `latex` from the builder, `convert`, and the heap
    /// writer for each.
    fn by_path(latex: &str) -> Option<[String; 4]> {
        use crate::reader::{Reader, latex::LatexReader};
        use crate::{Format, convert};
        let config = Config::default();
        let mut heap = LatexReader::parse(latex, &config).ok()?.expr;
        let builder_heap = UnicodeMathWriter::write(&heap, &config).ok()?;
        crate::ast::normalize::normalize_expr(&mut heap);
        Some([
            crate::from_latex(latex).ok()?.to_unicode_math().ok()?,
            convert(latex, Format::LaTeX, Format::UnicodeMath).ok()?,
            builder_heap,
            UnicodeMathWriter::write(&heap, &config).ok()?,
        ])
    }

    /// A formula's MathML as a tree, with rows kept where they are arguments
    /// (`\frac{a-b}{c}` is not `\frac{a}{-bc}`), no attributes, and rows
    /// spliced where they only group (`\left`, an n-aryand).
    fn structure(input: &str, from: crate::Format) -> String {
        let mathml = crate::convert(input, from, crate::Format::MathML).unwrap();
        let mut tokens = mathml
            .split('<')
            .skip(1)
            .map(|token| token.split_once('>').unwrap());
        fn element<'a>(
            tokens: &mut impl Iterator<Item = (&'a str, &'a str)>,
            tag: &str,
            text: &str,
        ) -> (String, Vec<String>) {
            let name = tag.split(' ').next().unwrap();
            let name = name.trim_end_matches('/');
            let mut children: Vec<String> = Vec::new();
            let leaf = matches!(name, "mi" | "mn" | "mo" | "mtext");
            if !tag.ends_with('/') {
                while let Some((tag, text)) = tokens.next() {
                    if tag.starts_with('/') {
                        break;
                    }
                    let (name_inner, inner) = element(tokens, tag, text);
                    // A row in a row is spliced; one that is an argument is
                    // kept, with a single child as the child.
                    let splices = matches!(name, "math" | "mrow" | "msqrt" | "mstyle" | "mtd")
                        || name == "mphantom";
                    if name_inner == "mrow" && (splices || inner.len() == 1) {
                        children.extend(inner);
                    } else {
                        children.push(format!("{name_inner}({})", inner.join(" ")));
                    }
                }
            }
            // A symbol of several characters reads back as one for each.
            let mut joined: Vec<String> = Vec::new();
            for child in children {
                match joined.last_mut() {
                    Some(last)
                        if matches!(name, "math" | "mrow")
                            && last.starts_with("mo(")
                            && child.starts_with("mo(") =>
                    {
                        last.pop();
                        last.push_str(&child[3..]);
                    },
                    _ => joined.push(child),
                }
            }
            if leaf {
                joined = vec![text.to_owned()];
            }
            (name.to_owned(), joined)
        }
        let (tag, text) = tokens.next().unwrap();
        element(&mut tokens, tag, text).1.join(" ")
    }

    /// The kinds of the symbols spelled `glyph` in a formula.
    fn kinds(input: &str, from: crate::Format, glyph: &str) -> Vec<SymbolKind> {
        use crate::reader::{Reader, latex::LatexReader, unicode_math::UnicodeMathReader};
        struct Kinds<'a>(&'a str, Vec<SymbolKind>);
        impl crate::ast::visit::Visitor for Kinds<'_> {
            fn visit_node(&mut self, node: &MathNode) {
                if let MathNode::Symbol(kind, text) = node
                    && text == self.0
                {
                    self.1.push(*kind);
                }
                crate::ast::visit::walk_node(self, node);
            }
        }
        let config = Config::default();
        let expr = if from == crate::Format::LaTeX {
            LatexReader::parse(input, &config)
        } else {
            UnicodeMathReader::parse(input, &config)
        }
        .unwrap()
        .expr;
        let mut kinds = Kinds(glyph, Vec::new());
        for node in &expr.nodes {
            crate::ast::visit::Visitor::visit_node(&mut kinds, node);
        }
        kinds.1
    }

    const TEMPLATES: &[&str] = &[
        "a OP b",
        "a OP \\frac{b}{c}",
        "a_1 OP \\frac{b}{c}",
        "\\sum a OP \\frac{b}{c}",
        "\\frac{1}{2} OP \\frac{b}{c}",
        "\\frac{a OP b}{c}",
        "a OP b^2",
        "x^{OP}y",
        "x_{i OP 1}y",
        "OP \\frac{b}{c}",
        "a OP",
        "\\sqrt{a OP b}",
        "\\sin a OP \\frac{b}{c}",
    ];

    /// Every operator, delimiter, punctuation mark and arrow a LaTeX command
    /// spells keeps the structure around it on every path, and its class.
    #[test]
    fn every_symbol_command_keeps_structure_and_class() {
        use crate::Format::{LaTeX, UnicodeMath};
        let mut failures = Vec::new();
        for (name, glyph, kind) in crate::unicode::symbols::all_commands() {
            if matches!(kind, SymbolKind::Alphabetic) || UNWRITABLE.contains(&name.as_str()) {
                continue;
            }
            for template in TEMPLATES {
                let latex = template.replace("OP", &format!("\\{name} "));
                let Some(written) = by_path(&latex) else {
                    continue;
                };
                let wanted = structure(&latex, LaTeX);
                let wanted_kinds = kinds(&latex, LaTeX, &glyph);
                for unicode in &written {
                    let found = structure(unicode, UnicodeMath);
                    if found != wanted {
                        failures.push(format!(
                            "STRUCTURE {name} {latex}: {unicode}: {found} / {wanted}"
                        ));
                        break;
                    }
                    let found_kinds = kinds(unicode, UnicodeMath, &glyph);
                    if glyph.chars().nth(1).is_none()
                        && found_kinds != wanted_kinds
                        && !OTHER_CLASS.contains(&name.as_str())
                    {
                        failures.push(format!(
                            "CLASS {name} {latex}: {unicode}: {found_kinds:?} / {wanted_kinds:?}"
                        ));
                        break;
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{}\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Commands whose glyph UnicodeMath uses for something else.
    const UNWRITABLE: &[&str] = &[
        // Structure: `&`, `\\`, `^`, `¦` and unpaired brackets.
        "And",
        "backslash",
        "textbackslash",
        "textasciicircum",
        "textbrokenbar",
        "lbrace",
        "lbrack",
        "rbrace",
        "rbrack",
        // ASCII characters the reader keeps as text.
        "ldotp",
        "textasciitilde",
        "textdollar",
        "textquotesingle",
        // Numeric characters, likewise (UTN #28: scripts and fractions).
        "bbone",
        "bbzero",
        "textonehalf",
        "textonequarter",
        "textonesuperior",
        "textthreequarters",
        "textthreesuperior",
        "texttwosuperior",
        // Letters, which read back as identifiers.
        "Finv",
        "circledS",
        "textangstrom",
        "textflorin",
        "textmicro",
        "textmu",
        "textohm",
        "textordfeminine",
        "textordmasculine",
    ];

    /// Commands that share a glyph with another of a different class.
    const OTHER_CLASS: &[&str] = &[
        "bigtriangleup",
        "vartriangle",
        "trianglelefteq",
        "trianglerighteq",
        "vartriangleleft",
        "vartriangleright",
        "dag",
        "ddag",
        "bot",
        "textdagger",
        "textdaggerdbl",
        "triangledown",
        "cdotp",
        "colon",
        "textgreater",
        "textless",
        // `|` and `‖` are fences, whichever side.
        "Vert",
        "vert",
        "textbar",
        "lVert",
        "lvert",
        "rVert",
        "rvert",
    ];

    fn write(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        UnicodeMathWriter::write(&expr, &config).unwrap()
    }

    fn choice_latex(source: &str, display: DisplayStyle) -> (String, String) {
        let config = Config::default().with_display(display);
        let parsed = crate::from_latex_with(source, &config).unwrap();
        let compact = parsed.to_unicode_math().unwrap();
        let heap = UnicodeMathWriter::write(&parsed.expr(), &config).unwrap();
        (compact, heap)
    }

    #[test]
    fn number() {
        assert_eq!(write(MathNode::Number("42".into())), "42");
    }

    #[test]
    fn identifier() {
        assert_eq!(write(MathNode::Identifier("x".into())), "x");
    }

    #[test]
    fn superscript() {
        let output = write(MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        });
        assert_eq!(output, "x^2");
    }

    #[test]
    fn subscript() {
        let output = write(MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Identifier("i".into())),
        });
        assert_eq!(output, "x_i");
    }

    #[test]
    fn fraction() {
        let output = write(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        });
        assert_eq!(output, "1/2");
    }

    #[test]
    fn sqrt() {
        let output = write(MathNode::Sqrt(Box::new(MathNode::Identifier("x".into()))));
        assert_eq!(output, "\u{221A}x");
    }

    #[test]
    fn sum_nary() {
        let output = write(MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::AboveBelow,
        });
        assert!(output.contains('\u{2211}'));
        assert!(output.contains("_i"));
        assert!(output.contains("^n"));
    }

    #[test]
    fn matrix() {
        let output = write(MathNode::Array {
            columns: vec![ColumnAlign::Center; 2],
            rows: vec![
                vec![
                    vec![MathNode::Number("1".into())],
                    vec![MathNode::Number("2".into())],
                ],
                vec![
                    vec![MathNode::Number("3".into())],
                    vec![MathNode::Number("4".into())],
                ],
            ],
            arraystretch: None,
            add_jot: false,
        });
        assert!(output.starts_with('\u{25A0}'));
        assert!(output.contains('&'));
        assert!(output.contains('@'));
    }

    #[test]
    fn mathchoice_uses_the_active_display_style() {
        let source = r"\mathchoice{D}{T}{S}{Q}";
        assert_eq!(
            choice_latex(source, DisplayStyle::Block),
            ("D".into(), "D".into())
        );
        assert_eq!(
            choice_latex(source, DisplayStyle::Inline),
            ("T".into(), "T".into())
        );
    }

    #[test]
    fn mathchoice_uses_script_transitions_and_forced_fraction_styles() {
        let choice = r"\mathchoice{D}{T}{S}{Q}";
        for (source, expected) in [
            (format!(r"x^{{{choice}}}"), "x^S"),
            (format!(r"x_{{y^{{{choice}}}}}"), "x_(y^Q)"),
            (format!(r"\frac{{{choice}}}{{{choice}}}"), "T/T"),
            (format!(r"\dfrac{{{choice}}}{{{choice}}}"), "T/T"),
            (format!(r"\tfrac{{{choice}}}{{{choice}}}"), "S/S"),
            (format!(r"\sqrt[{choice}]{{{choice}}}"), "√(Q&D)"),
        ] {
            assert_eq!(
                choice_latex(&source, DisplayStyle::Block),
                (expected.into(), expected.into()),
                "{source}"
            );
        }
    }

    #[test]
    fn mathchoice_keeps_scoped_declarations_and_table_cell_styles() {
        let choice = r"\mathchoice{D}{T}{S}{Q}";
        for (source, expected) in [
            (format!(r"{{\scriptstyle {choice}}}{choice}"), "SD"),
            (
                format!(r"\begin{{matrix}}{choice}&{choice}\\{choice}&{choice}\end{{matrix}}"),
                "■(T&T@T&T)",
            ),
            (
                format!(r"\begin{{smallmatrix}}{choice}\end{{smallmatrix}}"),
                "■(S)",
            ),
            (
                format!(r"\begin{{aligned}}{choice}&={choice}\end{{aligned}}"),
                "■(D&=D)",
            ),
        ] {
            let (compact, heap) = choice_latex(&source, DisplayStyle::Block);
            assert_eq!(compact, expected, "{source}");
            assert_eq!(heap, expected, "{source}");
        }
    }

    #[test]
    fn mathchoice_compound_branches_are_grouped_in_scripts() {
        let source = r"x^\mathchoice{0}{0}{a+b}{0}";
        let (compact, heap) = choice_latex(source, DisplayStyle::Inline);
        assert_eq!(compact, "x^(a+b)");
        assert_eq!(heap, compact);
        let reparsed = crate::from_unicode_math(&compact).unwrap();
        assert_eq!(
            reparsed.to_latex().unwrap(),
            crate::from_unicode_math("x^(a+b)")
                .unwrap()
                .to_latex()
                .unwrap()
        );

        let source = r"\left(\scriptstyle\mathchoice{271}{272}{273}{274}\middle|\mathchoice{275}{276}{277}{278}\right)";
        let (compact, heap) = choice_latex(source, DisplayStyle::Block);
        assert_eq!(compact, "(273|275)");
        assert_eq!(heap, compact);
    }

    #[test]
    fn matrix_cells_start_in_text_style_and_honor_explicit_switches() {
        let source = r"\begin{matrix}{\scriptstyle\mathchoice{D}{T}{S}{Q}}&\mathchoice{D}{T}{S}{Q}\end{matrix}";
        let (compact, heap) = choice_latex(source, DisplayStyle::Block);
        assert_eq!(compact, "■(S&T)");
        assert_eq!(heap, compact);

        let choice = || MathNode::MathChoice {
            display: Box::new(MathNode::Identifier("D".into())),
            text: Box::new(MathNode::Identifier("T".into())),
            script: Box::new(MathNode::Identifier("S".into())),
            script_script: Box::new(MathNode::Identifier("Q".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Center],
            rows: vec![vec![vec![choice()]]],
            arraystretch: Some(0.5),
            add_jot: true,
        }]);
        let config = Config::default();
        let heap = UnicodeMathWriter::write(&expr, &config).unwrap();
        let compact =
            UnicodeMathWriter::write_compact(&CompactExpr::from_math_expr(&expr).unwrap(), &config)
                .unwrap();
        assert_eq!(heap, "■(T)");
        assert_eq!(compact, heap);
    }

    #[test]
    fn spacing_inspects_the_selected_mathchoice_operator() {
        let operator = |name: &str| {
            MathNode::Operator(OperatorAttr {
                name: name.into(),
                limit_style: LimitStyle::Default,
            })
        };
        let expr = MathExpr::inline(vec![
            MathNode::Identifier("x".into()),
            MathNode::MathChoice {
                display: Box::new(operator("D")),
                text: Box::new(operator("+")),
                script: Box::new(operator("S")),
                script_script: Box::new(operator("Q")),
            },
        ]);
        assert_eq!(
            UnicodeMathWriter::write(&expr, &Config::default()).unwrap(),
            "x +"
        );
    }

    #[test]
    fn operator_terminator_uses_the_active_mathchoice_branch() {
        let operator = |name: &str| {
            MathNode::Operator(OperatorAttr {
                name: name.into(),
                limit_style: LimitStyle::Default,
            })
        };
        let expr = MathExpr::block(vec![
            MathNode::MathChoice {
                display: Box::new(operator("lim\u{00a0}sup")),
                text: Box::new(operator("T")),
                script: Box::new(operator("S")),
                script_script: Box::new(operator("Q")),
            },
            MathNode::Identifier("x".into()),
        ]);
        let config = Config::default();
        let heap = UnicodeMathWriter::write(&expr, &config).unwrap();
        let compact =
            UnicodeMathWriter::write_compact(&CompactExpr::from_math_expr(&expr).unwrap(), &config)
                .unwrap();
        assert_eq!(heap, "lim\u{00a0}sup x");
        assert_eq!(compact, heap);

        let inline =
            UnicodeMathWriter::write(&MathExpr::inline(expr.nodes.clone()), &config).unwrap();
        assert_eq!(inline, "Tx");
    }

    #[test]
    fn mathchoice_diagnostic_describes_style_dependent_selection() {
        let config = Config::default().with_display(DisplayStyle::Inline);
        let parsed = crate::from_latex_with(r"\mathchoice{D}{T}{S}{Q}", &config).unwrap();
        let (output, diagnostics) =
            UnicodeMathWriter::write_compact_with_diagnostics(parsed.compact_expr(), &config)
                .unwrap();
        assert_eq!(output, "T");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("active at this style") })
        );
    }

    #[test]
    fn text() {
        let output = write(MathNode::Text(TextStyle::Normal, "hello".into()));
        assert_eq!(output, "\"hello\"");
    }

    #[test]
    fn fenced() {
        let output = write(MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        });
        assert_eq!(output, "(x)");
    }

    #[test]
    fn textcolor_drops_color_but_keeps_content() {
        use crate::reader::Reader;
        let config = Config::default();
        // UnicodeMath has no portable color syntax, so `\textcolor{red}{x}`
        // must still emit the inner content `x` (color is dropped, with a
        // loss diagnostic).
        let result =
            crate::reader::latex::LatexReader::parse_compact("\\textcolor{red}{x}", &config)
                .unwrap();
        let (output, diags) =
            UnicodeMathWriter::write_compact_with_diagnostics(&result.expr, &config).unwrap();
        assert_eq!(output, "x", "{output}");
        assert!(
            diags.iter().any(|d| d.message.contains("color")),
            "expected a color-loss diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn actuarial_uses_masked_box_syntax() {
        let output = write(MathNode::Actuarial {
            content: Box::new(MathNode::Identifier("x".into())),
            text_mode: false,
        });
        assert_eq!(output, "▭(6&x)");
    }

    #[test]
    fn normalized_operator_decorations_keep_the_whole_name() {
        use crate::{Format, convert};
        for (src, label, mark) in [
            (r"\operatorname{\overbrace{rate}}_i\sin x", "rate", "⏞"),
            (r"\operatorname*{\underbracket{end}}\limits_i x", "end", "⎵"),
        ] {
            let linear = convert(src, Format::LaTeX, Format::UnicodeMath).unwrap();
            assert!(linear.contains(&format!("\"{label}\"")), "{linear}");
            assert_eq!(
                linear,
                convert(&linear, Format::UnicodeMath, Format::UnicodeMath).unwrap()
            );
            let xml = convert(&linear, Format::UnicodeMath, Format::MathML).unwrap();
            assert!(xml.contains(&format!("<mtext>{label}</mtext>")), "{xml}");
            assert!(xml.contains(mark), "{xml}");
            assert!(
                xml.contains("<mi>i</mi>") && xml.contains("<mi>x</mi>"),
                "{xml}"
            );
        }
    }

    #[test]
    fn structured_operator_unicode_transport_is_stable_and_preserves_bounds() {
        for source in [
            r"\operatorname{\overline{lim}}_i x",
            r"\operatorname{\underleftarrow{lim}}_i x",
            r"\operatorname*{\overline{lim}}\limits_i x",
            r"\operatorname{A\overline{lim}B}_i x",
            r"\operatorname*{\underbracket{end}}_i x",
            r"\operatorname*{\overbrace{rate}}\limits_i x",
            r"\operatorname*{\mathbf{Re}\overline{x}}_i x",
            r"\operatorname{A\underline{a\,b}B}_i x",
            r"\operatorname{\overline{\frac{a}{b}}}_i x",
            r"\operatorname{\overline{x}\,\underline{y}}_i x",
            r"\operatorname{rate}_i x",
        ] {
            let parsed = crate::from_latex(source).unwrap();
            let unicode = parsed.to_unicode_math().unwrap();
            let imported = crate::from_unicode_math(&unicode).unwrap();
            let again = imported.to_unicode_math().unwrap();
            assert_eq!(
                unicode, again,
                "UnicodeMath was not idempotent for {source}"
            );
            assert!(
                unicode.contains('_') || unicode.contains('┬') || unicode.contains('┴'),
                "bound placement was lost: {unicode}"
            );
            let imported_mathml = imported.to_mathml().unwrap();
            assert!(
                !imported_mathml.is_empty(),
                "operator body was lost: {source}"
            );
        }
    }
}
