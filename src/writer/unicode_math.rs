use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::*;
use crate::error::{Config, Diagnostic, Result};
use crate::writer::Writer as MathWriter;

/// Classifies a compact node that the UnicodeMath writer cannot fully or
/// round-trippably represent. UnicodeMath is a lossy linear notation.
fn unicode_math_loss(arena: &AstArena, node: &CompactNode) -> Option<String> {
    match node {
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
            Some("UnicodeMath keeps only the display-style branch of \\mathchoice".into())
        },
        CompactNode::MathAtom { .. } => {
            Some("UnicodeMath does not preserve the math atom spacing class".into())
        },
        // Over/under content is now emitted (approximated as limit-scripts), but
        // the over/under *positioning* is not faithfully round-trippable.
        CompactNode::Overscript { over, .. } if !is_overunder_line(arena, *over) => Some(
            "UnicodeMath approximates \\overset as a limit-script; positioning is lossy".into(),
        ),
        CompactNode::Underscript { under, .. } if !is_overunder_line(arena, *under) => Some(
            "UnicodeMath approximates \\underset as a limit-script; positioning is lossy".into(),
        ),
        _ => None,
    }
}

pub struct UnicodeMathWriter;

impl MathWriter for UnicodeMathWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut out = String::with_capacity(256);
        for (i, node) in expr.nodes.iter().enumerate() {
            if i > 0 && needs_space_before(node) {
                out.push(' ');
            }
            write_node(&mut out, node);
        }
        Ok(out)
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let out = Self::write_compact(expr, config)?;
        let diags = super::collect_loss_diagnostics(expr, unicode_math_loss);
        Ok((out, diags))
    }

    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let mut out = String::with_capacity(256);
        let arena = &expr.arena;
        for (i, &id) in arena.children(expr.roots).iter().enumerate() {
            if i > 0 && needs_space_before_c(arena, id) {
                out.push(' ');
            }
            write_node_c(&mut out, arena, id);
        }
        Ok(out)
    }
}

fn write_node(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Number(s) => out.push_str(s),
        MathNode::Identifier(s) => out.push_str(s),
        MathNode::Operator(attr) => out.push_str(&attr.name),
        MathNode::Symbol(_, s) => out.push_str(s),
        MathNode::Text(_, s) => {
            out.push('"');
            out.push_str(s);
            out.push('"');
        },
        MathNode::Space(r) => {
            let em = r.as_f64();
            if em >= 1.0 {
                out.push('\u{2003}'); // em space
            } else if em >= 0.5 {
                out.push('\u{2002}'); // en space
            } else {
                out.push('\u{2009}'); // thin space
            }
        },
        MathNode::Rule { .. } => {},

        MathNode::Row(children) => {
            for (i, child) in children.iter().enumerate() {
                if i > 0 && needs_space_before(child) {
                    out.push(' ');
                }
                write_node(out, child);
            }
        },

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            out.push_str(open);
            for (i, item) in content.iter().enumerate() {
                if i > 0 {
                    // separator
                }
                match item {
                    FencedItem::Element(node) => write_node(out, node),
                    FencedItem::Middle(delim) => out.push_str(delim),
                }
            }
            out.push_str(close);
        },

        MathNode::Subscript { base, sub } => {
            write_base(out, base);
            out.push('_');
            write_script_arg(out, sub);
        },

        MathNode::Superscript { base, sup } => {
            write_base(out, base);
            out.push('^');
            write_script_arg(out, sup);
        },

        MathNode::SubSup { base, sub, sup } => {
            write_base(out, base);
            out.push('_');
            write_script_arg(out, sub);
            out.push('^');
            write_script_arg(out, sup);
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // UnicodeMath doesn't have direct prescripts; approximate
            if let Some(sub) = pre_sub {
                out.push('_');
                write_script_arg(out, sub);
            }
            if let Some(sup) = pre_sup {
                out.push('^');
                write_script_arg(out, sup);
            }
            write_node(out, base);
        },

        MathNode::Overscript { base, over, .. } => {
            write_node(out, base);
            out.push('\u{0305}'); // combining overline as approximation
            // UnicodeMath limitation: arbitrary overscript content not representable;
            // only a combining overline is emitted for the base.
            let _ = over;
        },

        MathNode::Underscript { base, under, .. } => {
            write_node(out, base);
            out.push('\u{0332}'); // combining underline as approximation
            // UnicodeMath limitation: arbitrary underscript content not representable;
            // only a combining underline is emitted for the base.
            let _ = under;
        },

        MathNode::UnderOver {
            base, under, over, ..
        } => {
            write_node(out, base);
            // UnicodeMath limitation: under/over content not representable;
            // only the base is emitted.
            let _ = (under, over);
        },

        MathNode::Fraction { num, den, .. } => {
            write_fraction_arg(out, num);
            out.push('/');
            write_fraction_arg(out, den);
        },

        MathNode::Sqrt(inner) => {
            out.push('\u{221A}');
            write_paren_if_complex(out, inner);
        },

        MathNode::Root { index, radicand } => {
            // Use cube root symbol for index=3, fourth root for index=4
            match index.as_ref() {
                MathNode::Number(s) if s == "3" => {
                    out.push('\u{221B}');
                    write_paren_if_complex(out, radicand);
                },
                MathNode::Number(s) if s == "4" => {
                    out.push('\u{221C}');
                    write_paren_if_complex(out, radicand);
                },
                _ => {
                    // General nth root: √(n&radicand) notation
                    out.push('\u{221A}');
                    out.push('(');
                    write_node(out, index);
                    out.push('&');
                    write_node(out, radicand);
                    out.push(')');
                },
            }
        },

        MathNode::Nary {
            op, sub, sup, body, ..
        } => {
            out.push(*op);
            if let Some(s) = sub {
                out.push('_');
                write_script_arg(out, s);
            }
            if let Some(s) = sup {
                out.push('^');
                write_script_arg(out, s);
            }
            out.push(' ');
            write_node(out, body);
        },

        MathNode::Accent { base, accent, .. } => {
            write_node(out, base);
            out.push(*accent);
        },

        MathNode::Array { rows, .. } => {
            out.push('\u{25A0}'); // ■
            out.push('(');
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    out.push('@');
                }
                for (j, cell) in row.iter().enumerate() {
                    if j > 0 {
                        out.push('&');
                    }
                    for node in cell {
                        write_node(out, node);
                    }
                }
            }
            out.push(')');
        },

        MathNode::Phantom(inner, _) => {
            out.push('\u{27E1}'); // phantom operator
            write_paren_if_complex(out, inner);
        },

        MathNode::Boxed { content: inner, .. } => {
            out.push('\u{25AD}'); // rectangle
            write_paren_if_complex(out, inner);
        },

        MathNode::Cancel { content, .. } => {
            write_node(out, content);
        },

        MathNode::Scaled { content, .. } | MathNode::RaiseBox { content, .. } => {
            write_node(out, content);
        },

        MathNode::Styled(style, children) => {
            // Apply Unicode math alphabet mapping
            for child in children {
                match child {
                    MathNode::Identifier(s) | MathNode::Number(s) | MathNode::Text(_, s) => {
                        for ch in s.chars() {
                            let styled =
                                crate::unicode::fonts::styled_char(ch, *style).unwrap_or(ch);
                            out.push(styled);
                        }
                    },
                    _ => write_node(out, child),
                }
            }
        },
        MathNode::MathChoice { display, .. } => {
            // UnicodeMath doesn't have mathchoice; output the display branch
            write_node(out, display);
        },
        MathNode::MathAtom { content, .. } => write_node(out, content),
    }
}

fn write_base(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Row(_) | MathNode::Fraction { .. } => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        _ => write_node(out, node),
    }
}

fn write_script_arg(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        MathNode::Fraction { .. }
        | MathNode::Subscript { .. }
        | MathNode::Superscript { .. }
        | MathNode::SubSup { .. } => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        _ => write_node(out, node),
    }
}

fn write_fraction_arg(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        MathNode::Fraction { .. } => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        _ => write_node(out, node),
    }
}

fn write_paren_if_complex(out: &mut String, node: &MathNode) {
    match node {
        MathNode::Row(children) if children.len() > 1 => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
        MathNode::Identifier(s) if s.chars().count() == 1 => {
            write_node(out, node);
        },
        MathNode::Number(s) if s.chars().count() == 1 => {
            write_node(out, node);
        },
        _ => {
            out.push('(');
            write_node(out, node);
            out.push(')');
        },
    }
}

fn needs_space_before(node: &MathNode) -> bool {
    matches!(node, MathNode::Operator(_) | MathNode::Text(_, _))
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

fn write_node_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Number(sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Identifier(sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Operator { name, .. } => out.push_str(arena.str_val(name)),
        CompactNode::Symbol(_, sid) => out.push_str(arena.str_val(sid)),
        CompactNode::Text(_, sid) => {
            out.push('"');
            out.push_str(arena.str_val(sid));
            out.push('"');
        },
        CompactNode::Space(r) => {
            let em = r.as_f64();
            if em >= 1.0 {
                out.push('\u{2003}');
            } else if em >= 0.5 {
                out.push('\u{2002}');
            } else {
                out.push('\u{2009}');
            }
        },

        CompactNode::Row(span) => {
            for (i, &child_id) in arena.children(span).iter().enumerate() {
                if i > 0 && needs_space_before_c(arena, child_id) {
                    out.push(' ');
                }
                write_node_c(out, arena, child_id);
            }
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            out.push_str(arena.str_val(open));
            for &cid in arena.children(content) {
                match *arena.node(cid) {
                    CompactNode::Middle(sid) => out.push_str(arena.str_val(sid)),
                    _ => write_node_c(out, arena, cid),
                }
            }
            out.push_str(arena.str_val(close));
        },

        CompactNode::Subscript { base, sub } => {
            write_base_c(out, arena, base);
            out.push('_');
            write_script_arg_c(out, arena, sub);
        },

        CompactNode::Superscript { base, sup } => {
            write_base_c(out, arena, base);
            out.push('^');
            write_script_arg_c(out, arena, sup);
        },

        CompactNode::SubSup { base, sub, sup } => {
            write_base_c(out, arena, base);
            out.push('_');
            write_script_arg_c(out, arena, sub);
            out.push('^');
            write_script_arg_c(out, arena, sup);
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            if pre_sub.is_some() {
                out.push('_');
                write_script_arg_c(out, arena, pre_sub);
            }
            if pre_sup.is_some() {
                out.push('^');
                write_script_arg_c(out, arena, pre_sup);
            }
            write_node_c(out, arena, base);
        },

        CompactNode::Overscript { base, over, .. } => {
            write_node_c(out, arena, base);
            if is_overunder_line(arena, over) {
                out.push('\u{0305}'); // combining overline for \overline / \bar
            } else {
                // Preserve overset / brace / arrow content instead of dropping it.
                out.push('^');
                write_script_arg_c(out, arena, over);
            }
        },

        CompactNode::Underscript { base, under, .. } => {
            write_node_c(out, arena, base);
            if is_overunder_line(arena, under) {
                out.push('\u{0332}'); // combining underline for \underline
            } else {
                out.push('_');
                write_script_arg_c(out, arena, under);
            }
        },

        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            write_node_c(out, arena, base);
            if is_overunder_line(arena, under) {
                out.push('\u{0332}');
            } else {
                out.push('_');
                write_script_arg_c(out, arena, under);
            }
            if is_overunder_line(arena, over) {
                out.push('\u{0305}');
            } else {
                out.push('^');
                write_script_arg_c(out, arena, over);
            }
        },

        CompactNode::Fraction { num, den, .. } => {
            write_fraction_arg_c(out, arena, num);
            out.push('/');
            write_fraction_arg_c(out, arena, den);
        },

        CompactNode::Sqrt(inner) => {
            out.push('\u{221A}');
            write_paren_if_complex_c(out, arena, inner);
        },

        CompactNode::Root { index, radicand } => {
            let idx_node = *arena.node(index);
            match idx_node {
                CompactNode::Number(sid) if arena.str_val(sid) == "3" => {
                    out.push('\u{221B}');
                    write_paren_if_complex_c(out, arena, radicand);
                },
                CompactNode::Number(sid) if arena.str_val(sid) == "4" => {
                    out.push('\u{221C}');
                    write_paren_if_complex_c(out, arena, radicand);
                },
                _ => {
                    out.push('\u{221A}');
                    out.push('(');
                    write_node_c(out, arena, index);
                    out.push('&');
                    write_node_c(out, arena, radicand);
                    out.push(')');
                },
            }
        },

        CompactNode::Nary {
            op, sub, sup, body, ..
        } => {
            out.push(op);
            if sub.is_some() {
                out.push('_');
                write_script_arg_c(out, arena, sub);
            }
            if sup.is_some() {
                out.push('^');
                write_script_arg_c(out, arena, sup);
            }
            out.push(' ');
            write_node_c(out, arena, body);
        },

        CompactNode::Accent { base, accent, .. } => {
            write_node_c(out, arena, base);
            out.push(accent);
        },

        CompactNode::Array {
            cells, num_cols, ..
        } => {
            out.push('\u{25A0}');
            out.push('(');
            let cell_ids = arena.children(cells);
            let nc = num_cols as usize;
            let num_rows = cell_ids.len().checked_div(nc).unwrap_or(0);
            for i in 0..num_rows {
                if i > 0 {
                    out.push('@');
                }
                for j in 0..nc {
                    if j > 0 {
                        out.push('&');
                    }
                    let cell_id = cell_ids[i * nc + j];
                    if let CompactNode::Row(cell_span) = *arena.node(cell_id) {
                        for &nid in arena.children(cell_span) {
                            write_node_c(out, arena, nid);
                        }
                    } else {
                        write_node_c(out, arena, cell_id);
                    }
                }
            }
            out.push(')');
        },

        CompactNode::Phantom(inner, _) => {
            out.push('\u{27E1}');
            write_paren_if_complex_c(out, arena, inner);
        },

        CompactNode::Boxed { content: inner, .. } => {
            out.push('\u{25AD}');
            write_paren_if_complex_c(out, arena, inner);
        },

        CompactNode::Cancel { content, .. } => {
            write_node_c(out, arena, content);
        },

        CompactNode::Scaled { content, .. } => {
            write_node_c(out, arena, content);
        },
        CompactNode::RaiseBox { content, .. } => {
            write_node_c(out, arena, content);
        },

        CompactNode::Styled(style, span) => {
            for &child_id in arena.children(span) {
                let child = *arena.node(child_id);
                match child {
                    CompactNode::Identifier(sid)
                    | CompactNode::Number(sid)
                    | CompactNode::Text(_, sid) => {
                        for ch in arena.str_val(sid).chars() {
                            let styled =
                                crate::unicode::fonts::styled_char(ch, style).unwrap_or(ch);
                            out.push(styled);
                        }
                    },
                    _ => write_node_c(out, arena, child_id),
                }
            }
        },

        CompactNode::Color { content, .. } => {
            // Plain-text UnicodeMath has no portable color syntax, so the color
            // is dropped (see `unicode_math_loss`); the inner content is always
            // emitted so nothing is lost from the rendered expression.
            write_node_c(out, arena, content);
        },

        CompactNode::MathChoice { display, .. } => {
            write_node_c(out, arena, display);
        },

        CompactNode::MathAtom { content, .. } => {
            write_node_c(out, arena, content);
        },

        CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_)
        | CompactNode::Newline => {},
    }
}

fn write_base_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Row(_) | CompactNode::Fraction { .. } => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        _ => write_node_c(out, arena, id),
    }
}

fn write_script_arg_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        CompactNode::Fraction { .. }
        | CompactNode::Subscript { .. }
        | CompactNode::Superscript { .. }
        | CompactNode::SubSup { .. } => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        _ => write_node_c(out, arena, id),
    }
}

fn write_fraction_arg_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        CompactNode::Fraction { .. } => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        _ => write_node_c(out, arena, id),
    }
}

fn write_paren_if_complex_c(out: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Row(span) if arena.children(span).len() > 1 => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
        CompactNode::Identifier(sid) if arena.str_val(sid).chars().count() == 1 => {
            write_node_c(out, arena, id);
        },
        CompactNode::Number(sid) if arena.str_val(sid).chars().count() == 1 => {
            write_node_c(out, arena, id);
        },
        _ => {
            out.push('(');
            write_node_c(out, arena, id);
            out.push(')');
        },
    }
}

fn needs_space_before_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(
        *arena.node(id),
        CompactNode::Operator { .. } | CompactNode::Text(_, _)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        UnicodeMathWriter::write(&expr, &config).unwrap()
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
}
