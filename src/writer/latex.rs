use crate::ast::arena::AstArena;
use crate::ast::compact::{self, CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{Config, Result};
use crate::unicode::fonts::style_to_latex_font;
use crate::unicode::symbols::{is_operator_command, nary_op_to_latex, unicode_to_latex};
use crate::writer::Writer;

pub struct LatexWriter;

impl Writer for LatexWriter {
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        let compact = CompactExpr::from_math_expr(expr);
        Self::write_compact(&compact, config)
    }

    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let mut buf = String::with_capacity(256);
        write_nodes(&mut buf, &expr.arena, expr.roots);
        Ok(buf)
    }
}

fn write_nodes(buf: &mut String, arena: &AstArena, span: NodeSpan) {
    let children = arena.children(span);
    let mut i = 0;
    while i < children.len() {
        let id = children[i];

        // Stand-alone empty-base script (first child or following another
        // empty-base script). Write it with an explicit `{}` base so that it
        // stays a separate atom and remains stable across round-trips.
        if is_script_with_empty_base(arena, id) {
            let next = children.get(i + 1).copied();
            write_standalone_empty_base_script(buf, arena, id, next);
            i += 1;
            continue;
        }

        // If this node is immediately followed by a *trailing* empty-base
        // script, attach the script directly when safe. Attaching is only done
        // for the last child in the row; otherwise a following sibling could be
        // re-parsed as part of the scripted atom (e.g. `1_{\prime}^1{1}`
        // instead of separate `1` and `{}_{\prime}^1{1}` atoms).
        if i + 1 < children.len()
            && i + 1 == children.len() - 1
            && is_script_with_empty_base(arena, children[i + 1])
        {
            if i > 0 {
                let space = needs_space(arena, children.get(i - 1).copied(), id)
                    || (buf_ends_with_letter_command(buf)
                        && node_starts_with_ascii_letter(arena, id));
                if space {
                    buf.push(' ');
                }
            }
            let script_id = children[i + 1];
            let next = children.get(i + 2).copied();
            if is_simple_token(arena, id) && !would_merge_with_buffer(buf, arena, id) {
                // Safe to attach directly; brace the script argument if it would
                // otherwise merge with the following sibling.
                write_node(buf, arena, id);
                write_empty_base_script_markers(buf, arena, script_id, next);
            } else if matches!(*arena.node(id), CompactNode::Row(_)) {
                // Use the script-base writer so that a single-child Row wrapping
                // a simple token gets braced when it would otherwise merge with
                // the preceding output (e.g. `p{p}^{\prime}`).
                write_script_base(buf, arena, id);
                write_empty_base_script_markers(buf, arena, script_id, next);
            } else {
                // Complex or merge-prone base: keep the script as a separate
                // atom so the output shape is preserved.
                write_node(buf, arena, id);
                write_standalone_empty_base_script(buf, arena, script_id, next);
            }
            i += 2;
            continue;
        }

        // Collapse a flattened ∫⋯∫ sequence back into the canonical \idotsint
        // command. Writing it as separate \int\cdots\int nodes is not stable
        // because the first \int greedily absorbs the \cdots as its body.
        if i + 3 <= children.len() && is_idotsint_triple(arena, &children[i..i + 3]) {
            if i > 0 {
                let space = needs_space(arena, children.get(i - 1).copied(), children[i])
                    || (buf_ends_with_letter_command(buf)
                        && node_starts_with_ascii_letter(arena, children[i]));
                if space {
                    buf.push(' ');
                }
            }
            write_idotsint(buf);
            i += 3;
            continue;
        }

        if i > 0 {
            let space = needs_space(arena, children.get(i - 1).copied(), id)
                || (buf_ends_with_letter_command(buf)
                    && node_starts_with_ascii_letter(arena, id));
            if space {
                buf.push(' ');
            }
        }

        // Script nodes whose argument would merge with the following sibling
        // need to be written with the argument forcibly braced.
        if is_script_node(arena, id) && !is_script_with_empty_base(arena, id) {
            let next = children.get(i + 1).copied();
            write_script_node_with_next(buf, arena, id, next);
        } else {
            write_node(buf, arena, id);
            // An empty-body nary operator followed by another node needs an
            // explicit `{}` body, otherwise the next node would be re-parsed as
            // the operator's body.
            if i + 1 < children.len()
                && is_empty_body_nary(arena, id)
            {
                buf.push_str("{}");
            }
        }
        i += 1;
    }
}

/// True if the node is a Nary operator with an effectively empty body.
fn is_empty_body_nary(arena: &AstArena, id: NodeId) -> bool {
    matches!(
        *arena.node(id),
        CompactNode::Nary { body, .. } if is_empty_body(arena, body)
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

/// Write a script node, forcibly bracing any sub/superscript argument that
/// would merge with the following sibling when written unbraced.
fn write_script_node_with_next(
    buf: &mut String,
    arena: &AstArena,
    id: NodeId,
    next: Option<NodeId>,
) {
    match *arena.node(id) {
        CompactNode::Subscript { base, sub } => {
            write_script_base(buf, arena, base);
            if is_effectively_empty_arg(arena, sub) {
                return;
            }
            buf.push('_');
            write_script_arg(buf, arena, sub, next);
        },
        CompactNode::Superscript { base, sup } => {
            write_script_base(buf, arena, base);
            if is_effectively_empty_arg(arena, sup) {
                return;
            }
            buf.push('^');
            write_script_arg(buf, arena, sup, next);
        },
        CompactNode::SubSup { base, sub, sup } => {
            write_script_base(buf, arena, base);
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if !sub_empty {
                buf.push('_');
                // The subscript argument is always followed by the `^` marker, so
                // it cannot merge with the row-next node.
                write_script_arg(buf, arena, sub, None);
            }
            if !sup_empty {
                buf.push('^');
                write_script_arg(buf, arena, sup, next);
            }
        },
        _ => write_node(buf, arena, id),
    }
}

/// Write a script argument.  Digit sequences are always braced so that a
/// following `.` is not swallowed into a decimal literal and so that trailing
/// vs. non-trailing position cannot change the bracing decision.
fn write_script_arg(
    buf: &mut String,
    arena: &AstArena,
    arg: NodeId,
    _next: Option<NodeId>,
) {
    if is_digit_sequence_script_arg(arena, arg) {
        buf.push('{');
        write_node(buf, arena, arg);
        buf.push('}');
    } else {
        write_braced(buf, arena, arg);
    }
}

/// True for script arguments that are a single Number or a Row containing only
/// digit characters.  Such arguments must be braced when written after `_`/`^`
/// to avoid being merged with a following `.` or changing shape depending on
/// whether the script is trailing.
fn is_digit_sequence_script_arg(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(_) => true,
        CompactNode::Row(span) => row_produces_only_digits(arena, span),
        _ => false,
    }
}

/// Write a stand-alone empty-base script with an explicit `{}` base.
fn write_standalone_empty_base_script(
    buf: &mut String,
    arena: &AstArena,
    script_id: NodeId,
    _next: Option<NodeId>,
) {
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
    buf: &mut String,
    arena: &AstArena,
    script_id: NodeId,
) {
    match *arena.node(script_id) {
        CompactNode::Subscript { sub, .. }
            if !is_effectively_empty_arg(arena, sub) =>
        {
            buf.push('_');
            write_always_braced(buf, arena, sub);
        },
        CompactNode::Superscript { sup, .. }
            if !is_effectively_empty_arg(arena, sup) =>
        {
            buf.push('^');
            write_always_braced(buf, arena, sup);
        },
        CompactNode::SubSup { sub, sup, .. } => {
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if !sub_empty {
                buf.push('_');
                write_always_braced(buf, arena, sub);
            }
            if !sup_empty {
                buf.push('^');
                write_always_braced(buf, arena, sup);
            }
        },
        _ => {},
    }
}

/// Write the `_`/`^` markers and braced arguments of an empty-base script node.
/// `next` is the following sibling in the current row; it is used to brace a
/// script argument that would otherwise merge with that sibling (e.g. `{}^2 2`).
fn write_empty_base_script_markers(
    buf: &mut String,
    arena: &AstArena,
    script_id: NodeId,
    next: Option<NodeId>,
) {
    match *arena.node(script_id) {
        CompactNode::Subscript { sub, .. } => {
            if is_effectively_empty_arg(arena, sub) {
                return;
            }
            buf.push('_');
            write_script_arg(buf, arena, sub, next);
        },
        CompactNode::Superscript { sup, .. } => {
            if is_effectively_empty_arg(arena, sup) {
                return;
            }
            buf.push('^');
            write_script_arg(buf, arena, sup, next);
        },
        CompactNode::SubSup { sub, sup, .. } => {
            let sub_empty = is_effectively_empty_arg(arena, sub);
            let sup_empty = is_effectively_empty_arg(arena, sup);
            if sub_empty && sup_empty {
                return;
            }
            if !sub_empty {
                buf.push('_');
                // The subscript argument is followed by the `^` marker, so it
                // cannot merge with the row-next node.
                write_script_arg(buf, arena, sub, None);
            }
            if !sup_empty {
                buf.push('^');
                write_script_arg(buf, arena, sup, next);
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
        CompactNode::Operator { .. }
        | CompactNode::Symbol(SymbolKind::BinaryOp, _)
        | CompactNode::Symbol(SymbolKind::Relation, _) => true,
        // \mathrel / \mathbin wrappers behave like the relation/binary-op
        // symbols they contain, so they also need trailing space.
        CompactNode::MathAtom {
            atom_type: MathAtomType::Rel | MathAtomType::Bin,
            ..
        } => true,
        // After an identifier written as \command, space before plain ASCII letter
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            let ch = s.chars().next().unwrap_or('?');
            if unicode_to_latex(ch).is_some() {
                curr_starts_with_ascii_letter(arena, curr)
            } else {
                false
            }
        },
        // After a non-ASCII symbol written as \command
        CompactNode::Symbol(_, sid) => {
            let s = arena.str_val(sid);
            let ch = s.chars().next().unwrap_or('?');
            !ch.is_ascii()
                && unicode_to_latex(ch).is_some()
                && curr_starts_with_ascii_letter(arena, curr)
        },
        _ => false,
    }
}

fn curr_starts_with_ascii_letter(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            let ch = s.chars().next().unwrap_or('?');
            ch.is_ascii_alphabetic() && unicode_to_latex(ch).is_none()
        },
        _ => false,
    }
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
    is_empty_row(arena, base)
}

/// Check if the buffer ends with a `\command` pattern (backslash followed by ASCII letters).
/// This detects cases where appending an ASCII letter would extend the command name.
fn buf_ends_with_letter_command(buf: &str) -> bool {
    let bytes = buf.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return false;
    }
    // Scan backwards past ASCII letters
    let mut i = len;
    while i > 0 && bytes[i - 1].is_ascii_alphabetic() {
        i -= 1;
    }
    // Must have at least one letter, preceded by '\'
    i < len && i > 0 && bytes[i - 1] == b'\\'
}

/// Check if a node's first written character would be a plain ASCII letter.
/// Recursively handles compound nodes whose base is written via `write_braced`.
fn node_starts_with_ascii_letter(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Identifier(sid) => {
            let s = arena.str_val(sid);
            let ch = s.chars().next().unwrap_or('?');
            ch.is_ascii_alphabetic() && unicode_to_latex(ch).is_none()
        },
        CompactNode::Number(sid) => arena
            .str_val(sid)
            .as_bytes()
            .first()
            .is_some_and(|&b| b.is_ascii_alphabetic()),
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
fn write_idotsint(buf: &mut String) {
    buf.push_str("\\idotsint");
}

fn write_node(buf: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Number(sid) => buf.push_str(arena.str_val(sid)),

        CompactNode::Identifier(sid) => write_identifier(buf, arena, sid),

        CompactNode::Operator { name, .. } => {
            let name_str = arena.str_val(name);
            if is_operator_command(name_str) {
                buf.push('\\');
                buf.push_str(name_str);
            } else {
                buf.push_str("\\operatorname{");
                buf.push_str(name_str);
                buf.push('}');
            }
        },

        CompactNode::Symbol(kind, sid) => write_symbol(buf, arena, kind, sid),

        CompactNode::Text(style, sid) => {
            let s = arena.str_val(sid);
            if style == TextStyle::Monospace
                && (s.contains('\\') || s.contains('{') || s.contains('}'))
            {
                // \texttt cannot faithfully round-trip content containing
                // backslashes or braces. Use \verb with a delimiter that does
                // not appear in the content.
                write_verb(buf, s);
            } else {
                let cmd = match style {
                    TextStyle::Bold => "textbf",
                    TextStyle::Italic => "textit",
                    TextStyle::Monospace => "texttt",
                    TextStyle::SansSerif => "textsf",
                    _ => "text",
                };
                buf.push('\\');
                buf.push_str(cmd);
                buf.push('{');
                // Ligature sequences such as '' and --, and the active '~'
                // shorthand for a non-breaking space, would be re-parsed
                // differently on the next read. Escape them so the text content
                // round-trips faithfully.
                buf.push_str(&escape_text_content(s));
                buf.push('}');
            }
        },

        CompactNode::Space(r) => write_space(buf, r),

        CompactNode::Row(span) => {
            if is_idotsint_row(arena, id) {
                write_idotsint(buf);
                return;
            }
            let children = arena.children(span);
            if children.len() == 1 {
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

            // Detect binom pattern
            if open_str == "(" && close_str == ")" && content_ids.len() == 1 {
                let child_id = content_ids[0];
                if let CompactNode::Fraction {
                    kind: FractionKind::NoBar,
                    num,
                    den,
                } = *arena.node(child_id)
                {
                    buf.push_str("\\binom{");
                    write_node(buf, arena, num);
                    buf.push_str("}{");
                    write_node(buf, arena, den);
                    buf.push('}');
                    return;
                }
            }

            buf.push_str("\\left");
            write_delim(buf, open_str);
            buf.push(' ');
            for &child_id in content_ids {
                match *arena.node(child_id) {
                    CompactNode::Middle(sid) => {
                        buf.push_str(" \\middle");
                        write_delim(buf, arena.str_val(sid));
                        buf.push(' ');
                    },
                    _ => {
                        // A command delimiter at the end of the previous child
                        // (e.g. an inner \right\rangle) would merge with a
                        // following plain letter. Insert a separator when needed.
                        if buf_ends_with_letter_command(buf)
                            && node_starts_with_ascii_letter(arena, child_id)
                        {
                            buf.push(' ');
                        }
                        write_node(buf, arena, child_id);
                    },
                }
            }

            buf.push_str(" \\right");
            write_delim(buf, close_str);
        },

        CompactNode::Subscript { base, sub } => {
            if is_empty_row(arena, base) {
                buf.push_str("{}");
                if !is_effectively_empty_arg(arena, sub) {
                    buf.push('_');
                    write_always_braced(buf, arena, sub);
                }
            } else {
                write_script_base(buf, arena, base);
                if !is_effectively_empty_arg(arena, sub) {
                    buf.push('_');
                    write_braced(buf, arena, sub);
                }
            }
        },

        CompactNode::Superscript { base, sup } => {
            if is_empty_row(arena, base) {
                buf.push_str("{}");
                if !is_effectively_empty_arg(arena, sup) {
                    buf.push('^');
                    write_always_braced(buf, arena, sup);
                }
            } else {
                write_script_base(buf, arena, base);
                if !is_effectively_empty_arg(arena, sup) {
                    buf.push('^');
                    write_braced(buf, arena, sup);
                }
            }
        },

        CompactNode::SubSup { base, sub, sup } => {
            if is_empty_row(arena, base) {
                buf.push_str("{}");
                let sub_empty = is_effectively_empty_arg(arena, sub);
                let sup_empty = is_effectively_empty_arg(arena, sup);
                if !sub_empty {
                    buf.push('_');
                    write_always_braced(buf, arena, sub);
                }
                if !sup_empty {
                    buf.push('^');
                    write_always_braced(buf, arena, sup);
                }
            } else {
                write_script_base(buf, arena, base);
                let sub_empty = is_effectively_empty_arg(arena, sub);
                let sup_empty = is_effectively_empty_arg(arena, sup);
                if !sub_empty {
                    buf.push('_');
                    write_braced(buf, arena, sub);
                }
                if !sup_empty {
                    buf.push('^');
                    write_braced(buf, arena, sup);
                }
            }
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            buf.push_str("{}");
            if pre_sub.is_some() {
                buf.push('_');
                write_braced(buf, arena, pre_sub);
            }
            if pre_sup.is_some() {
                buf.push('^');
                write_braced(buf, arena, pre_sup);
            }
            write_node(buf, arena, base);
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits: _,
        } => {
            if is_line_symbol(arena, over) {
                buf.push_str("\\overline{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else if is_overbrace(arena, over) {
                buf.push_str("\\overbrace{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else {
                buf.push_str("\\overset{");
                write_wrapped_content(buf, arena, over);
                buf.push_str("}{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            }
        },

        CompactNode::Underscript {
            base,
            under,
            movable_limits: _,
        } => {
            if is_line_symbol(arena, under) {
                buf.push_str("\\underline{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else if is_underbrace(arena, under) {
                buf.push_str("\\underbrace{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else {
                buf.push_str("\\underset{");
                write_wrapped_content(buf, arena, under);
                buf.push_str("}{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            }
        },

        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits: _,
        } => {
            buf.push_str("\\underset{");
            write_wrapped_content(buf, arena, under);
            buf.push_str("}{\\overset{");
            write_wrapped_content(buf, arena, over);
            buf.push_str("}{");
            write_wrapped_content(buf, arena, base);
            buf.push_str("}}");
        },

        CompactNode::Fraction { kind, num, den } => {
            let cmd = match kind {
                FractionKind::Normal => "frac",
                FractionKind::Display => "dfrac",
                FractionKind::Inline => "tfrac",
                FractionKind::NoBar => "frac",
                FractionKind::Skewed => "frac",
            };
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
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
            write_wrapped_content(buf, arena, index);
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
            if let Some(cmd) = nary_op_to_latex(op) {
                buf.push('\\');
                buf.push_str(cmd);
            } else {
                buf.push(op);
            }
            match limits {
                LimitStyle::AboveBelow => buf.push_str("\\limits"),
                LimitStyle::SubSup => buf.push_str("\\nolimits"),
                LimitStyle::Default => {},
            }
            if !is_effectively_empty_arg(arena, sub) {
                buf.push('_');
                write_braced(buf, arena, sub);
            }
            if !is_effectively_empty_arg(arena, sup) {
                buf.push('^');
                write_braced(buf, arena, sup);
            }
            // Only write the body if it's non-empty — avoids \prod vs \prod {} divergence
            if !is_empty_body(arena, body) {
                buf.push(' ');
                write_braced(buf, arena, body);
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
                    ('\u{2192}', _) => Some("overrightarrow"),
                    ('\u{2190}', _) => Some("overleftarrow"),
                    ('\u{2194}', _) => Some("overleftrightarrow"),
                    ('\u{21C0}', _) => Some("overrightharpoon"),
                    ('\u{21BC}', _) => Some("overleftharpoon"),
                    ('\u{21D2}', _) => Some("Overrightarrow"),
                    ('\u{23DC}', _) => Some("overgroup"),
                    ('\u{23B4}', _) => Some("overlinesegment"),
                    _ => None,
                },
                AccentPosition::Below => match accent {
                    '\u{2192}' => Some("underrightarrow"),
                    '\u{2190}' => Some("underleftarrow"),
                    '\u{2194}' => Some("underleftrightarrow"),
                    '\u{0330}' => Some("utilde"),
                    '\u{23DD}' => Some("undergroup"),
                    '\u{23B5}' => Some("underlinesegment"),
                    _ => None,
                },
            };
            if let Some(cmd) = cmd {
                buf.push('\\');
                buf.push_str(cmd);
                buf.push('{');
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            } else {
                // No dedicated command: preserve the accent glyph faithfully
                // via overset/underset rather than silently emitting \hat.
                buf.push_str(match position {
                    AccentPosition::Above => "\\overset{",
                    AccentPosition::Below => "\\underset{",
                });
                buf.push(accent);
                buf.push_str("}{");
                write_wrapped_content(buf, arena, base);
                buf.push('}');
            }
        },

        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            ..
        } => {
            buf.push_str("\\begin{array}{");
            for &align_id in arena.children(col_aligns) {
                if let CompactNode::ColAlign(col) = *arena.node(align_id) {
                    buf.push(match col {
                        ColumnAlign::Left => 'l',
                        ColumnAlign::Center => 'c',
                        ColumnAlign::Right => 'r',
                    });
                }
            }
            buf.push_str("} ");
            let cell_ids = arena.children(cells);
            let nc = num_cols as usize;
            if nc > 0 {
                let num_rows = cell_ids.len() / nc.max(1);
                for row_idx in 0..num_rows {
                    if row_idx > 0 {
                        buf.push_str(" \\\\ ");
                    }
                    for col_idx in 0..nc {
                        if col_idx > 0 {
                            buf.push_str(" & ");
                        }
                        let cell_id = cell_ids[row_idx * nc + col_idx];
                        // Each cell is a Row node wrapping cell contents
                        if let CompactNode::Row(span) = *arena.node(cell_id) {
                            write_nodes(buf, arena, span);
                        } else {
                            write_node(buf, arena, cell_id);
                        }
                    }
                }
            }
            buf.push_str(" \\end{array}");
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
            let bg = arena.str_val(bg_color);
            let border = arena.str_val(border_color);
            if !bg.is_empty() && !border.is_empty() {
                // \fcolorbox{border}{bg}{content}
                buf.push_str("\\fcolorbox{");
                buf.push_str(border);
                buf.push_str("}{");
                buf.push_str(bg);
                buf.push_str("}{");
                write_wrapped_content(buf, arena, inner);
                buf.push('}');
            } else if !bg.is_empty() {
                // \colorbox{bg}{content}
                buf.push_str("\\colorbox{");
                buf.push_str(bg);
                buf.push_str("}{");
                write_wrapped_content(buf, arena, inner);
                buf.push('}');
            } else {
                buf.push_str("\\boxed{");
                write_wrapped_content(buf, arena, inner);
                buf.push('}');
            }
        },

        CompactNode::Cancel { stroke, content } => {
            let cmd = match stroke {
                StrokeKind::Forward => "cancel",
                StrokeKind::Backward => "bcancel",
                StrokeKind::Cross => "xcancel",
                StrokeKind::Horizontal | StrokeKind::Vertical => "cancel",
            };
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_wrapped_content(buf, arena, content);
            buf.push('}');
        },

        CompactNode::Scaled { factor, content } => {
            let cmd = match factor {
                r if r == Rational::new(6, 5) => "\\big",
                r if r == Rational::new(9, 5) => "\\Big",
                r if r == Rational::new(12, 5) => "\\bigg",
                r if r == Rational::new(3, 1) => "\\Bigg",
                _ => "",
            };
            // Sizing commands only apply to delimiter symbols. If the parsed
            // content is not an Open/Close/Fence symbol, the original input was
            // invalid (e.g. \Big\mathrel{...}); emit the content alone so the
            // sizing command does not detach from its argument on re-parse.
            if matches!(
                *arena.node(content),
                CompactNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, ..)
            ) {
                buf.push_str(cmd);
                write_node(buf, arena, content);
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
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_nodes(buf, arena, span);
            buf.push('}');
        },

        CompactNode::Color { color, content } => {
            let color_str = arena.str_val(color);
            // A malformed color argument may contain characters that cannot be
            // written verbatim inside \textcolor{...} without breaking brace
            // balance or being re-interpreted as special LaTeX tokens. In those
            // cases the color cannot round-trip, so emit only the content.
            if color_str
                .chars()
                .any(|c| matches!(c, '{' | '}' | '\\' | '~' | '#' | '$' | '%' | '&' | '^' | '_'))
            {
                if let CompactNode::Row(span) = *arena.node(content) {
                    write_nodes(buf, arena, span);
                } else {
                    write_node(buf, arena, content);
                }
            } else {
                buf.push_str("\\textcolor{");
                buf.push_str(color_str);
                buf.push_str("}{");
                // The outer braces already group the content; if it is a Row,
                // write its children directly to avoid double-bracing (e.g.
                // \textcolor{red}{lnc}).
                if let CompactNode::Row(span) = *arena.node(content) {
                    write_nodes(buf, arena, span);
                } else {
                    write_node(buf, arena, content);
                }
                buf.push('}');
            }
        },

        // Helper nodes — should not appear as standalone roots, but handle gracefully
        CompactNode::Middle(sid) => {
            buf.push_str(arena.str_val(sid));
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
            let cmd = match atom_type {
                MathAtomType::Ord => "\\mathord",
                MathAtomType::Op => "\\mathop",
                MathAtomType::Bin => "\\mathbin",
                MathAtomType::Rel => "\\mathrel",
                MathAtomType::Open => "\\mathopen",
                MathAtomType::Close => "\\mathclose",
                MathAtomType::Punct => "\\mathpunct",
                MathAtomType::Inner => "\\mathinner",
            };
            buf.push_str(cmd);
            buf.push('{');
            write_wrapped_content(buf, arena, content);
            buf.push('}');
        },
    }
}

#[inline]
fn write_identifier(buf: &mut String, arena: &AstArena, sid: compact::StrId) {
    let s = arena.str_val(sid);
    let ch = s.chars().next().unwrap_or('?');
    if let Some(cmd) = unicode_to_latex(ch) {
        buf.push('\\');
        buf.push_str(cmd);
    } else {
        buf.push_str(s);
    }
}

#[inline]
fn write_symbol(buf: &mut String, arena: &AstArena, kind: SymbolKind, sid: compact::StrId) {
    let s = arena.str_val(sid);
    let ch = s.chars().next().unwrap_or('?');
    // Math minus sign → ASCII hyphen-minus for LaTeX output
    if ch == '\u{2212}' {
        buf.push('-');
        return;
    }
    // ASCII symbols are written directly
    if ch.is_ascii() {
        match ch {
            '{' => buf.push_str("\\{"),
            '}' => buf.push_str("\\}"),
            '#' => buf.push_str("\\#"),
            '$' => buf.push_str("\\$"),
            '%' => buf.push_str("\\%"),
            '&' => buf.push_str("\\&"),
            _ => buf.push(ch),
        }
        return;
    }
    // Try unicode -> latex command
    if let Some(cmd) = unicode_to_latex(ch) {
        buf.push('\\');
        buf.push_str(cmd);
        return;
    }
    // For relation/binary operators that have no direct LaTeX command in our
    // table, wrap the raw Unicode character so its math class is preserved
    // across round-trips. Without this, the parser would re-read the bare
    // character as Ordinary and spacing would change.
    match kind {
        SymbolKind::Relation => {
            buf.push_str("\\mathrel{");
            buf.push_str(s);
            buf.push('}');
        },
        SymbolKind::BinaryOp => {
            buf.push_str("\\mathbin{");
            buf.push_str(s);
            buf.push('}');
        },
        _ => buf.push_str(s),
    }
}

fn write_space(buf: &mut String, r: Rational) {
    // Use exact Rational matching for all known LaTeX spacing commands
    // to ensure round-trip idempotency.
    if r == Rational::new(-3, 18) {
        buf.push_str("\\!");
    } else if r == Rational::new(3, 18) {
        buf.push_str("\\,");
    } else if r == Rational::new(4, 18) {
        buf.push_str("\\:");
    } else if r == Rational::new(5, 18) {
        buf.push_str("\\;");
    } else if r == Rational::new(1, 4) {
        buf.push_str("\\ ");
    } else if r == Rational::new(1, 2) {
        buf.push_str("\\enspace ");
    } else if r == Rational::new(1, 1) {
        buf.push_str("\\quad ");
    } else if r == Rational::new(2, 1) {
        buf.push_str("\\qquad ");
    } else if r == Rational::zero() {
        // Zero-width spacing commands (e.g. a malformed/empty `\kern` or
        // `\hspace{}`) produce no visible output and must not be emitted as
        // `\\`, which would be re-parsed as a row separator and change the
        // array structure. Real line breaks are represented by `Newline` nodes.
    } else {
        buf.push_str("\\;");
    }
}

fn write_delim(buf: &mut String, delim: &str) {
    match delim {
        "" => buf.push('.'),
        "{" => buf.push_str("\\{"),
        "}" => buf.push_str("\\}"),
        "\u{2016}" => buf.push_str("\\|"),
        "\u{27E8}" => buf.push_str("\\langle"),
        "\u{27E9}" => buf.push_str("\\rangle"),
        "\u{230A}" => buf.push_str("\\lfloor"),
        "\u{230B}" => buf.push_str("\\rfloor"),
        "\u{2308}" => buf.push_str("\\lceil"),
        "\u{2309}" => buf.push_str("\\rceil"),
        s => buf.push_str(s),
    }
}

/// Write a node wrapped in braces. If the node is a single simple token,
/// skip the braces for cleaner output. A single-child Row containing a
/// non-simple node keeps the braces so the child is not unwrapped in contexts
/// where grouping matters (e.g. a script base that is a Row wrapping a Color
/// or command node).
#[inline]
fn write_braced(buf: &mut String, arena: &AstArena, id: NodeId) {
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
fn write_wrapped_content(buf: &mut String, arena: &AstArena, content: NodeId) {
    if let CompactNode::Row(span) = *arena.node(content) {
        write_nodes(buf, arena, span);
    } else {
        write_node(buf, arena, content);
    }
}

/// Write a node inside a pair of braces, always keeping the braces. Unlike
/// `write_braced`, this never unwraps a single-child Row, so it is safe for
/// stand-alone empty-base scripts whose arguments must stay isolated from the
/// surrounding text.
#[inline]
fn write_always_braced(buf: &mut String, arena: &AstArena, id: NodeId) {
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

/// Write verbatim content using \verb with a delimiter that is not present in
/// the content.
fn write_verb(buf: &mut String, content: &str) {
    const DELIMS: &[char] = &['|', '!', '@', '#', '%', '~', '`', ':', ';', '+', '='];
    let delim = DELIMS
        .iter()
        .copied()
        .find(|&d| !content.contains(d))
        .unwrap_or('¶');
    buf.push_str("\\verb");
    buf.push(delim);
    buf.push_str(content);
    buf.push(delim);
}

/// Escape characters inside a text-mode command (\text, \texttt, ...) that
/// would be interpreted specially when the output is read back in. Ligatures
/// such as '' and -- become their Unicode equivalents, active shorthand '~'
/// becomes a non-breaking space, and LaTeX special characters are backslash-
/// escaped so they survive re-parsing.
fn escape_text_content(s: &str) -> String {
    if !s.contains('\'')
        && !s.contains('`')
        && !s.contains('-')
        && !s.contains('~')
        && !s.contains('%')
        && !s.contains('#')
        && !s.contains('$')
        && !s.contains('&')
        && !s.contains('_')
        && !s.contains('^')
        && !s.contains('{')
        && !s.contains('}')
    {
        return s.to_string();
    }
    let mut out = s.to_string();
    out = out.replace("---", "\u{2014}"); // em dash
    out = out.replace("--", "\u{2013}"); // en dash
    out = out.replace("``", "\u{201C}"); // left double quote
    out = out.replace("''", "\u{201D}"); // right double quote
    out = out.replace('~', "\u{00A0}"); // non-breaking space
    out = out.replace('%', "\\%");
    out = out.replace('#', "\\#");
    out = out.replace('$', "\\$");
    out = out.replace('&', "\\&");
    out = out.replace('_', "\\_");
    out = out.replace('^', "\\^");
    out = out.replace('{', "\\{");
    out = out.replace('}', "\\}");
    out
}

/// Write the base of a sub/superscript. In addition to the usual bracing
/// rules, force braces when the base is a single-character token that would
/// merge with the immediately preceding output (e.g. `6` after another `6`).
/// Without the braces, re-parsing would combine the base with the preceding
/// token and the writer would emit a different form on the next round-trip.
#[inline]
fn write_script_base(buf: &mut String, arena: &AstArena, id: NodeId) {
    if is_idotsint_row(arena, id) {
        write_idotsint(buf);
        return;
    }
    // A single-child Row wrapping a simple token must inherit the token's
    // merge check, otherwise `pp^{\prime}` would be written as `pp^{
    // \prime}` and re-parsed as the multi-letter identifier `pp` carrying
    // the prime.
    if let CompactNode::Row(span) = *arena.node(id) {
        let children = arena.children(span);
        if children.len() == 1
            && is_simple_token(arena, children[0])
            && would_merge_with_buffer(buf, arena, children[0])
        {
            buf.push('{');
            write_node(buf, arena, id);
            buf.push('}');
            return;
        }
    }
    if is_simple_token(arena, id) && would_merge_with_buffer(buf, arena, id) {
        buf.push('{');
        write_node(buf, arena, id);
        buf.push('}');
    } else {
        write_braced(buf, arena, id);
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
        }
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

/// Check if a Nary body is effectively empty (empty Row or NONE node).
fn is_empty_body(arena: &AstArena, id: NodeId) -> bool {
    if !id.is_some() {
        return true;
    }
    matches!(*arena.node(id), CompactNode::Row(span) if arena.children(span).is_empty())
}

/// True for a missing node or an empty Row (an absent/effectively-empty script
/// argument).
fn is_effectively_empty_arg(arena: &AstArena, id: NodeId) -> bool {
    !id.is_some() || is_empty_row(arena, id)
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
        CompactNode::Symbol(_, sid) => {
            let s = arena.str_val(sid);
            s.chars().count() == 1 && s.chars().next().is_some_and(|c| c.is_ascii())
        },
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
    use crate::reader::Reader;
    use crate::reader::latex::LatexReader;

    fn roundtrip(input: &str) -> String {
        let config = Config::default();
        let parsed = LatexReader::parse(input, &config).unwrap();
        LatexWriter::write(&parsed.expr, &config).unwrap()
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
        assert_eq!(roundtrip("\\varinjlim"), "\\underrightarrow{\\lim}");
        assert_eq!(roundtrip("\\varprojlim"), "\\underleftarrow{\\lim}");
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
    fn boxed() {
        assert_eq!(roundtrip("\\boxed{x}"), "\\boxed{x}");
    }

    #[test]
    fn colorbox_roundtrip() {
        assert_eq!(roundtrip("\\colorbox{teal}{B}"), "\\colorbox{teal}{B}");
    }

    #[test]
    fn fcolorbox_roundtrip() {
        assert_eq!(
            roundtrip("\\fcolorbox{blue}{red}{C}"),
            "\\fcolorbox{blue}{red}{C}"
        );
    }
}
