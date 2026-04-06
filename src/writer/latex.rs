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
    for (i, &id) in children.iter().enumerate() {
        if i > 0 {
            let space = needs_space(arena, children.get(i - 1).copied(), id)
                || (buf_ends_with_letter_command(buf) && node_starts_with_ascii_letter(arena, id));
            if space {
                buf.push(' ');
            }
        }
        write_node(buf, arena, id);
    }
}

fn needs_space(arena: &AstArena, prev: Option<NodeId>, curr: NodeId) -> bool {
    let Some(prev_id) = prev else { return false };
    match *arena.node(prev_id) {
        CompactNode::Operator { .. }
        | CompactNode::Symbol(SymbolKind::BinaryOp, _)
        | CompactNode::Symbol(SymbolKind::Relation, _) => true,
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
        _ => false,
    }
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
            buf.push_str(arena.str_val(sid));
            buf.push('}');
        },

        CompactNode::Space(r) => write_space(buf, r),

        CompactNode::Row(span) => {
            buf.push('{');
            write_nodes(buf, arena, span);
            buf.push('}');
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
                    _ => write_node(buf, arena, child_id),
                }
            }
            buf.push_str(" \\right");
            write_delim(buf, close_str);
        },

        CompactNode::Subscript { base, sub } => {
            write_braced(buf, arena, base);
            buf.push('_');
            write_braced(buf, arena, sub);
        },

        CompactNode::Superscript { base, sup } => {
            write_braced(buf, arena, base);
            buf.push('^');
            write_braced(buf, arena, sup);
        },

        CompactNode::SubSup { base, sub, sup } => {
            write_braced(buf, arena, base);
            buf.push('_');
            write_braced(buf, arena, sub);
            buf.push('^');
            write_braced(buf, arena, sup);
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
                write_node(buf, arena, base);
                buf.push('}');
            } else if is_overbrace(arena, over) {
                buf.push_str("\\overbrace{");
                write_node(buf, arena, base);
                buf.push('}');
            } else {
                buf.push_str("\\overset{");
                write_node(buf, arena, over);
                buf.push_str("}{");
                write_node(buf, arena, base);
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
                write_node(buf, arena, base);
                buf.push('}');
            } else if is_underbrace(arena, under) {
                buf.push_str("\\underbrace{");
                write_node(buf, arena, base);
                buf.push('}');
            } else {
                buf.push_str("\\underset{");
                write_node(buf, arena, under);
                buf.push_str("}{");
                write_node(buf, arena, base);
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
            write_node(buf, arena, under);
            buf.push_str("}{\\overset{");
            write_node(buf, arena, over);
            buf.push_str("}{");
            write_node(buf, arena, base);
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
            write_node(buf, arena, num);
            buf.push_str("}{");
            write_node(buf, arena, den);
            buf.push('}');
        },

        CompactNode::Sqrt(inner) => {
            buf.push_str("\\sqrt{");
            write_node(buf, arena, inner);
            buf.push('}');
        },

        CompactNode::Root { index, radicand } => {
            buf.push_str("\\sqrt[");
            write_node(buf, arena, index);
            buf.push_str("]{");
            write_node(buf, arena, radicand);
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
            if sub.is_some() {
                buf.push('_');
                write_braced(buf, arena, sub);
            }
            if sup.is_some() {
                buf.push('^');
                write_braced(buf, arena, sup);
            }
            buf.push(' ');
            write_braced(buf, arena, body);
        },

        CompactNode::Accent {
            base,
            accent,
            position,
        } => {
            let cmd = match (accent, position) {
                ('\u{0302}', AccentPosition::Above) => "hat",
                ('\u{0304}', AccentPosition::Above) => "bar",
                ('\u{20D7}', AccentPosition::Above) => "vec",
                ('\u{0307}', AccentPosition::Above) => "dot",
                ('\u{0308}', AccentPosition::Above) => "ddot",
                ('\u{0303}', AccentPosition::Above) => "tilde",
                ('\u{0306}', AccentPosition::Above) => "breve",
                ('\u{030C}', AccentPosition::Above) => "check",
                ('\u{0301}', AccentPosition::Above) => "acute",
                ('\u{0300}', AccentPosition::Above) => "grave",
                _ => "hat",
            };
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_node(buf, arena, base);
            buf.push('}');
        },

        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
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

        CompactNode::Phantom(inner) => {
            buf.push_str("\\phantom{");
            write_node(buf, arena, inner);
            buf.push('}');
        },

        CompactNode::Boxed(inner) => {
            buf.push_str("\\boxed{");
            write_node(buf, arena, inner);
            buf.push('}');
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
            write_node(buf, arena, content);
            buf.push('}');
        },

        CompactNode::Scaled { factor, content } => {
            if factor == Rational::new(6, 5) {
                buf.push_str("\\big");
            } else if factor == Rational::new(9, 5) {
                buf.push_str("\\Big");
            } else if factor == Rational::new(12, 5) {
                buf.push_str("\\bigg");
            } else if factor == Rational::new(3, 1) {
                buf.push_str("\\Bigg");
            }
            write_node(buf, arena, content);
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
            buf.push_str("\\textcolor{");
            buf.push_str(arena.str_val(color));
            buf.push_str("}{");
            write_node(buf, arena, content);
            buf.push('}');
        },

        // Helper nodes — should not appear as standalone roots, but handle gracefully
        CompactNode::Middle(sid) => {
            buf.push_str(arena.str_val(sid));
        },
        CompactNode::ColAlign(_) => {},
        CompactNode::Rule { width, height } => {
            use std::fmt::Write as _;
            buf.push_str("\\rule{");
            let _ = write!(buf, "{}em", width.as_f64());
            buf.push_str("}{");
            let _ = write!(buf, "{}em", height.as_f64());
            buf.push('}');
        },
        CompactNode::StyleSwitch(s) => {
            match s {
                0 => buf.push_str("\\displaystyle "),
                1 => buf.push_str("\\textstyle "),
                2 => buf.push_str("\\scriptstyle "),
                _ => buf.push_str("\\scriptscriptstyle "),
            }
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
fn write_symbol(buf: &mut String, arena: &AstArena, _kind: SymbolKind, sid: compact::StrId) {
    let s = arena.str_val(sid);
    let ch = s.chars().next().unwrap_or('?');
    // ASCII symbols are written directly
    if ch.is_ascii() {
        match ch {
            '{' => buf.push_str("\\{"),
            '}' => buf.push_str("\\}"),
            _ => buf.push(ch),
        }
        return;
    }
    // Try unicode -> latex command
    if let Some(cmd) = unicode_to_latex(ch) {
        buf.push('\\');
        buf.push_str(cmd);
    } else {
        buf.push_str(s);
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
        // LaTeX `\\` (line break / row separator) is stored as Space(0/1) in the AST.
        buf.push_str("\\\\ ");
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
/// skip the braces for cleaner output.
#[inline]
fn write_braced(buf: &mut String, arena: &AstArena, id: NodeId) {
    if is_simple_token(arena, id) {
        write_node(buf, arena, id);
    } else if matches!(*arena.node(id), CompactNode::Row(_)) {
        // Row already emits its own braces — don't double-wrap
        write_node(buf, arena, id);
    } else {
        buf.push('{');
        write_node(buf, arena, id);
        buf.push('}');
    }
}

fn is_simple_token(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Number(sid) => arena.str_val(sid).len() == 1,
        CompactNode::Identifier(sid) => arena.str_val(sid).chars().count() == 1,
        CompactNode::Symbol(_, sid) => {
            let s = arena.str_val(sid);
            s.chars().count() == 1 && s.chars().next().is_some_and(|c| c.is_ascii())
        },
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
        assert_eq!(roundtrip("x^2"), "x^2");
    }

    #[test]
    fn subscript() {
        assert_eq!(roundtrip("x_i"), "x_i");
    }

    #[test]
    fn subsup() {
        let output = roundtrip("x_i^2");
        assert_eq!(output, "x_i^2");
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
    fn boxed() {
        assert_eq!(roundtrip("\\boxed{x}"), "\\boxed{x}");
    }
}
