use crate::ast::*;
use crate::error::{Config, Result};
use crate::unicode::fonts::style_to_latex_font;
use crate::unicode::symbols::{is_operator_command, nary_op_to_latex, unicode_to_latex};
use crate::writer::Writer;

pub struct LatexWriter;

impl Writer for LatexWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut buf = String::new();
        write_nodes(&mut buf, &expr.nodes);
        Ok(buf)
    }
}

fn write_nodes(buf: &mut String, nodes: &[MathNode]) {
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 && needs_space(nodes.get(i - 1), node) {
            buf.push(' ');
        }
        write_node(buf, node);
    }
}

fn needs_space(prev: Option<&MathNode>, _curr: &MathNode) -> bool {
    matches!(prev, Some(MathNode::Operator(_)) | Some(MathNode::Symbol(SymbolKind::BinaryOp, _)) | Some(MathNode::Symbol(SymbolKind::Relation, _)))
}

fn write_node(buf: &mut String, node: &MathNode) {
    match node {
        MathNode::Number(s) => buf.push_str(s),

        MathNode::Identifier(s) => write_identifier(buf, s),

        MathNode::Operator(attr) => {
            if is_operator_command(&attr.name) {
                buf.push('\\');
                buf.push_str(&attr.name);
            } else {
                buf.push_str("\\operatorname{");
                buf.push_str(&attr.name);
                buf.push('}');
            }
        },

        MathNode::Symbol(kind, s) => write_symbol(buf, *kind, s),

        MathNode::Text(style, s) => {
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
            buf.push_str(s);
            buf.push('}');
        },

        MathNode::Space(r) => write_space(buf, *r),

        MathNode::Row(children) => {
            buf.push('{');
            write_nodes(buf, children);
            buf.push('}');
        },

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            // Detect binom pattern: \left( \frac[NoBar]{n}{k} \right)
            if open == "(" && close == ")" && content.len() == 1
                && let Some(FencedItem::Element(MathNode::Fraction {
                    kind: FractionKind::NoBar,
                    num,
                    den,
                })) = content.first()
            {
                buf.push_str("\\binom{");
                write_node(buf, num);
                buf.push_str("}{");
                write_node(buf, den);
                buf.push('}');
                return;
            }
            buf.push_str("\\left");
            write_delim(buf, open);
            buf.push(' ');
            for item in content {
                match item {
                    FencedItem::Element(node) => write_node(buf, node),
                    FencedItem::Middle(delim) => {
                        buf.push_str(" \\middle");
                        write_delim(buf, delim);
                        buf.push(' ');
                    },
                }
            }
            buf.push_str(" \\right");
            write_delim(buf, close);
        },

        MathNode::Subscript { base, sub } => {
            write_node(buf, base);
            buf.push('_');
            write_braced(buf, sub);
        },

        MathNode::Superscript { base, sup } => {
            write_node(buf, base);
            buf.push('^');
            write_braced(buf, sup);
        },

        MathNode::SubSup { base, sub, sup } => {
            write_node(buf, base);
            buf.push('_');
            write_braced(buf, sub);
            buf.push('^');
            write_braced(buf, sup);
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            buf.push_str("{}");
            if let Some(s) = pre_sub {
                buf.push('_');
                write_braced(buf, s);
            }
            if let Some(s) = pre_sup {
                buf.push('^');
                write_braced(buf, s);
            }
            write_node(buf, base);
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits: _,
        } => {
            // Check for overline
            if is_line_symbol(over) {
                buf.push_str("\\overline{");
                write_node(buf, base);
                buf.push('}');
            } else if is_overbrace(over) {
                buf.push_str("\\overbrace{");
                write_node(buf, base);
                buf.push('}');
            } else {
                buf.push_str("\\overset{");
                write_node(buf, over);
                buf.push_str("}{");
                write_node(buf, base);
                buf.push('}');
            }
        },

        MathNode::Underscript {
            base,
            under,
            movable_limits: _,
        } => {
            if is_line_symbol(under) {
                buf.push_str("\\underline{");
                write_node(buf, base);
                buf.push('}');
            } else if is_underbrace(under) {
                buf.push_str("\\underbrace{");
                write_node(buf, base);
                buf.push('}');
            } else {
                buf.push_str("\\underset{");
                write_node(buf, under);
                buf.push_str("}{");
                write_node(buf, base);
                buf.push('}');
            }
        },

        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits: _,
        } => {
            buf.push_str("\\underset{");
            write_node(buf, under);
            buf.push_str("}{\\overset{");
            write_node(buf, over);
            buf.push_str("}{");
            write_node(buf, base);
            buf.push_str("}}");
        },

        MathNode::Fraction { kind, num, den } => {
            let cmd = match kind {
                FractionKind::Normal => "frac",
                FractionKind::Display => "dfrac",
                FractionKind::Inline => "tfrac",
                FractionKind::NoBar => "frac", // could use \atop but frac is safer
                FractionKind::Skewed => "frac",
            };
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_node(buf, num);
            buf.push_str("}{");
            write_node(buf, den);
            buf.push('}');
        },

        MathNode::Sqrt(inner) => {
            buf.push_str("\\sqrt{");
            write_node(buf, inner);
            buf.push('}');
        },

        MathNode::Root { index, radicand } => {
            buf.push_str("\\sqrt[");
            write_node(buf, index);
            buf.push_str("]{");
            write_node(buf, radicand);
            buf.push('}');
        },

        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            if let Some(cmd) = nary_op_to_latex(*op) {
                buf.push('\\');
                buf.push_str(cmd);
            } else {
                buf.push(*op);
            }
            match limits {
                LimitStyle::AboveBelow => buf.push_str("\\limits"),
                LimitStyle::SubSup => buf.push_str("\\nolimits"),
                LimitStyle::Default => {},
            }
            if let Some(s) = sub {
                buf.push('_');
                write_braced(buf, s);
            }
            if let Some(s) = sup {
                buf.push('^');
                write_braced(buf, s);
            }
            buf.push(' ');
            write_node(buf, body);
        },

        MathNode::Accent {
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
            write_node(buf, base);
            buf.push('}');
        },

        MathNode::Array { columns, rows } => {
            buf.push_str("\\begin{array}{");
            for col in columns {
                buf.push(match col {
                    ColumnAlign::Left => 'l',
                    ColumnAlign::Center => 'c',
                    ColumnAlign::Right => 'r',
                });
            }
            buf.push_str("} ");
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    buf.push_str(" \\\\ ");
                }
                for (j, cell) in row.iter().enumerate() {
                    if j > 0 {
                        buf.push_str(" & ");
                    }
                    write_nodes(buf, cell);
                }
            }
            buf.push_str(" \\end{array}");
        },

        MathNode::Phantom(inner) => {
            buf.push_str("\\phantom{");
            write_node(buf, inner);
            buf.push('}');
        },

        MathNode::Boxed(inner) => {
            buf.push_str("\\boxed{");
            write_node(buf, inner);
            buf.push('}');
        },

        MathNode::Cancel { stroke, content } => {
            let cmd = match stroke {
                StrokeKind::Forward => "cancel",
                StrokeKind::Backward => "bcancel",
                StrokeKind::Cross => "xcancel",
                StrokeKind::Horizontal | StrokeKind::Vertical => "cancel",
            };
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_node(buf, content);
            buf.push('}');
        },

        MathNode::Scaled { content, .. } => {
            // LaTeX doesn't have a direct scale command in math;
            // just emit content.
            write_node(buf, content);
        },

        MathNode::Styled(style, children) => {
            let cmd = style_to_latex_font(*style);
            buf.push('\\');
            buf.push_str(cmd);
            buf.push('{');
            write_nodes(buf, children);
            buf.push('}');
        },
    }
}

fn write_identifier(buf: &mut String, s: &str) {
    let ch = s.chars().next().unwrap_or('?');
    if let Some(cmd) = unicode_to_latex(ch) {
        buf.push('\\');
        buf.push_str(cmd);
    } else {
        buf.push_str(s);
    }
}

fn write_symbol(buf: &mut String, _kind: SymbolKind, s: &str) {
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
    let approx = r.as_f64();
    if approx <= -0.1 {
        buf.push_str("\\!");
    } else if approx < 0.2 {
        buf.push_str("\\,");
    } else if approx < 0.3 {
        buf.push_str("\\:");
    } else if approx < 0.4 {
        buf.push_str("\\;");
    } else if (approx - 1.0).abs() < 0.1 {
        buf.push_str("\\quad ");
    } else if (approx - 2.0).abs() < 0.1 {
        buf.push_str("\\qquad ");
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
fn write_braced(buf: &mut String, node: &MathNode) {
    if is_simple_token(node) {
        write_node(buf, node);
    } else {
        buf.push('{');
        write_node(buf, node);
        buf.push('}');
    }
}

fn is_simple_token(node: &MathNode) -> bool {
    match node {
        MathNode::Number(s) => s.len() == 1,
        MathNode::Identifier(s) => s.chars().count() == 1,
        MathNode::Symbol(_, s) => s.chars().count() == 1 && s.chars().next().is_some_and(|c| c.is_ascii()),
        _ => false,
    }
}

fn is_line_symbol(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if s == "\u{203E}")
}

fn is_overbrace(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if s == "\u{23DE}")
}

fn is_underbrace(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if s == "\u{23DF}")
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
