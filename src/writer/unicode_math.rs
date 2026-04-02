use crate::ast::*;
use crate::error::{Config, Result};
use crate::writer::Writer as MathWriter;

pub struct UnicodeMathWriter;

impl MathWriter for UnicodeMathWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut out = String::new();
        for (i, node) in expr.nodes.iter().enumerate() {
            if i > 0 && needs_space_before(node) {
                out.push(' ');
            }
            write_node(&mut out, node);
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

        MathNode::Row(children) => {
            for (i, child) in children.iter().enumerate() {
                if i > 0 && needs_space_before(child) {
                    out.push(' ');
                }
                write_node(out, child);
            }
        },

        MathNode::Fenced { open, close, content } => {
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

        MathNode::PreScripts { base, pre_sub, pre_sup } => {
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
            let _ = over;
        },

        MathNode::Underscript { base, under, .. } => {
            write_node(out, base);
            out.push('\u{0332}'); // combining underline as approximation
            let _ = under;
        },

        MathNode::UnderOver { base, under, over, .. } => {
            write_node(out, base);
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

        MathNode::Nary { op, sub, sup, body, .. } => {
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

        MathNode::Phantom(inner) => {
            out.push('\u{27E1}'); // phantom operator
            write_paren_if_complex(out, inner);
        },

        MathNode::Boxed(inner) => {
            out.push('\u{25AD}'); // rectangle
            write_paren_if_complex(out, inner);
        },

        MathNode::Cancel { content, .. } => {
            write_node(out, content);
        },

        MathNode::Scaled { content, .. } => {
            write_node(out, content);
        },

        MathNode::Styled(style, children) => {
            // Apply Unicode math alphabet mapping
            for child in children {
                match child {
                    MathNode::Identifier(s) | MathNode::Number(s) | MathNode::Text(_, s) => {
                        for ch in s.chars() {
                            let styled = crate::unicode::fonts::styled_char(ch, *style)
                                .unwrap_or(ch);
                            out.push(styled);
                        }
                    },
                    _ => write_node(out, child),
                }
            }
        },
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
    matches!(
        node,
        MathNode::Operator(_) | MathNode::Text(_, _)
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
}
