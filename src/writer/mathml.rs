use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

use crate::ast::*;
use crate::error::{Config, Result};
use crate::unicode::operators::{OperatorForm, lookup_operator_any, symbol_kind_to_form};
use crate::writer::Writer as MathWriter;

pub struct MathmlWriter;

impl MathWriter for MathmlWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));

        let mut math = BytesStart::new("math");
        math.push_attribute(("xmlns", "http://www.w3.org/1998/Math/MathML"));
        if expr.display == DisplayStyle::Block {
            math.push_attribute(("display", "block"));
        }
        writer.write_event(Event::Start(math)).map_err(to_err)?;

        for node in &expr.nodes {
            write_node(&mut writer, node)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("math")))
            .map_err(to_err)?;

        let buf = writer.into_inner().into_inner();
        Ok(String::from_utf8(buf).unwrap_or_default())
    }
}

type XmlWriter = Writer<Cursor<Vec<u8>>>;

fn to_err(e: std::io::Error) -> crate::error::Error {
    crate::error::Error::Io(e)
}

fn write_node(w: &mut XmlWriter, node: &MathNode) -> Result<()> {
    match node {
        MathNode::Number(s) => write_simple_element(w, "mn", s),
        MathNode::Identifier(s) => write_mi(w, s),
        MathNode::Operator(attr) => {
            let mut elem = BytesStart::new("mo");
            if attr.limit_style == LimitStyle::AboveBelow {
                elem.push_attribute(("movablelimits", "true"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            w.write_event(Event::Text(BytesText::new(&attr.name)))
                .map_err(to_err)?;
            w.write_event(Event::End(BytesEnd::new("mo")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Symbol(kind, s) => {
            let mut elem = BytesStart::new("mo");
            match kind {
                SymbolKind::LargeOp => {
                    elem.push_attribute(("largeop", "true"));
                },
                SymbolKind::Fence => {
                    elem.push_attribute(("fence", "true"));
                },
                _ => {},
            }
            // Enrich from operator dictionary
            if let Some(ch) = s.chars().next() {
                let form = symbol_kind_to_form(*kind);
                if let Some(entry) = lookup_operator_any(ch, form) {
                    if entry.stretchy {
                        elem.push_attribute(("stretchy", "true"));
                    }
                    if entry.form != OperatorForm::Infix {
                        elem.push_attribute(("form", crate::unicode::operators::form_str(entry.form)));
                    }
                }
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            w.write_event(Event::Text(BytesText::new(s)))
                .map_err(to_err)?;
            w.write_event(Event::End(BytesEnd::new("mo")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Text(_, s) => write_simple_element(w, "mtext", s),
        MathNode::Space(r) => {
            let mut elem = BytesStart::new("mspace");
            let width = format!("{}em", r.as_f64());
            elem.push_attribute(("width", width.as_str()));
            w.write_event(Event::Empty(elem)).map_err(to_err)?;
            Ok(())
        },

        MathNode::Row(children) => {
            w.write_event(Event::Start(BytesStart::new("mrow")))
                .map_err(to_err)?;
            for child in children {
                write_node(w, child)?;
            }
            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let elem = BytesStart::new("mrow");
            w.write_event(Event::Start(elem)).map_err(to_err)?;

            // Opening delimiter
            if !open.is_empty() {
                write_mo(w, open, Some("true"), None)?;
            }

            // Content
            for item in content {
                match item {
                    FencedItem::Element(node) => write_node(w, node)?,
                    FencedItem::Middle(delim) => {
                        write_mo(w, delim, Some("true"), None)?;
                    },
                }
            }

            // Closing delimiter
            if !close.is_empty() {
                write_mo(w, close, Some("true"), None)?;
            }

            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Subscript { base, sub } => {
            w.write_event(Event::Start(BytesStart::new("msub")))
                .map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, sub)?;
            w.write_event(Event::End(BytesEnd::new("msub")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Superscript { base, sup } => {
            w.write_event(Event::Start(BytesStart::new("msup")))
                .map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, sup)?;
            w.write_event(Event::End(BytesEnd::new("msup")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::SubSup { base, sub, sup } => {
            w.write_event(Event::Start(BytesStart::new("msubsup")))
                .map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, sub)?;
            write_node(w, sup)?;
            w.write_event(Event::End(BytesEnd::new("msubsup")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            w.write_event(Event::Start(BytesStart::new("mmultiscripts")))
                .map_err(to_err)?;
            write_node(w, base)?;
            w.write_event(Event::Empty(BytesStart::new("mprescripts")))
                .map_err(to_err)?;
            match pre_sub {
                Some(s) => write_node(w, s)?,
                None => {
                    w.write_event(Event::Empty(BytesStart::new("none")))
                        .map_err(to_err)?;
                },
            }
            match pre_sup {
                Some(s) => write_node(w, s)?,
                None => {
                    w.write_event(Event::Empty(BytesStart::new("none")))
                        .map_err(to_err)?;
                },
            }
            w.write_event(Event::End(BytesEnd::new("mmultiscripts")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            let mut elem = BytesStart::new("mover");
            if !movable_limits {
                elem.push_attribute(("accent", "true"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, over)?;
            w.write_event(Event::End(BytesEnd::new("mover")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            let mut elem = BytesStart::new("munder");
            if !movable_limits {
                elem.push_attribute(("accentunder", "true"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, under)?;
            w.write_event(Event::End(BytesEnd::new("munder")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => {
            let mut elem = BytesStart::new("munderover");
            if !movable_limits {
                elem.push_attribute(("accent", "true"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, base)?;
            write_node(w, under)?;
            write_node(w, over)?;
            w.write_event(Event::End(BytesEnd::new("munderover")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Fraction { kind, num, den } => {
            let mut elem = BytesStart::new("mfrac");
            if *kind == FractionKind::NoBar {
                elem.push_attribute(("linethickness", "0"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, num)?;
            write_node(w, den)?;
            w.write_event(Event::End(BytesEnd::new("mfrac")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Sqrt(inner) => {
            w.write_event(Event::Start(BytesStart::new("msqrt")))
                .map_err(to_err)?;
            write_node(w, inner)?;
            w.write_event(Event::End(BytesEnd::new("msqrt")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Root { index, radicand } => {
            w.write_event(Event::Start(BytesStart::new("mroot")))
                .map_err(to_err)?;
            write_node(w, radicand)?;
            write_node(w, index)?;
            w.write_event(Event::End(BytesEnd::new("mroot")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            // Decide between munderover and msubsup based on limits
            match limits {
                LimitStyle::AboveBelow => {
                    let tag = match (sub, sup) {
                        (Some(_), Some(_)) => "munderover",
                        (Some(_), None) => "munder",
                        (None, Some(_)) => "mover",
                        (None, None) => {
                            write_mo_char(w, *op)?;
                            write_node(w, body)?;
                            return Ok(());
                        },
                    };
                    w.write_event(Event::Start(BytesStart::new(tag)))
                        .map_err(to_err)?;
                    write_mo_char(w, *op)?;
                    if let Some(s) = sub {
                        write_node(w, s)?;
                    }
                    if let Some(s) = sup {
                        write_node(w, s)?;
                    }
                    w.write_event(Event::End(BytesEnd::new(tag)))
                        .map_err(to_err)?;
                },
                _ => {
                    let tag = match (sub, sup) {
                        (Some(_), Some(_)) => "msubsup",
                        (Some(_), None) => "msub",
                        (None, Some(_)) => "msup",
                        (None, None) => {
                            write_mo_char(w, *op)?;
                            write_node(w, body)?;
                            return Ok(());
                        },
                    };
                    w.write_event(Event::Start(BytesStart::new(tag)))
                        .map_err(to_err)?;
                    let mut op_elem = BytesStart::new("mo");
                    op_elem.push_attribute(("movablelimits", "true"));
                    w.write_event(Event::Start(op_elem)).map_err(to_err)?;
                    w.write_event(Event::Text(BytesText::new(
                        &op.to_string(),
                    )))
                    .map_err(to_err)?;
                    w.write_event(Event::End(BytesEnd::new("mo")))
                        .map_err(to_err)?;
                    if let Some(s) = sub {
                        write_node(w, s)?;
                    }
                    if let Some(s) = sup {
                        write_node(w, s)?;
                    }
                    w.write_event(Event::End(BytesEnd::new(tag)))
                        .map_err(to_err)?;
                },
            }
            write_node(w, body)?;
            Ok(())
        },

        MathNode::Accent {
            base,
            accent,
            position,
        } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            let mut elem = BytesStart::new(tag);
            elem.push_attribute(("accent", "true"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, base)?;
            write_mo_char(w, *accent)?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Array { columns, rows } => {
            w.write_event(Event::Start(BytesStart::new("mtable")))
                .map_err(to_err)?;
            for row in rows {
                w.write_event(Event::Start(BytesStart::new("mtr")))
                    .map_err(to_err)?;
                for (i, cell) in row.iter().enumerate() {
                    let mut td = BytesStart::new("mtd");
                    if let Some(align) = columns.get(i) {
                        let val = match align {
                            ColumnAlign::Left => "left",
                            ColumnAlign::Center => "center",
                            ColumnAlign::Right => "right",
                        };
                        td.push_attribute(("columnalign", val));
                    }
                    w.write_event(Event::Start(td)).map_err(to_err)?;
                    for node in cell {
                        write_node(w, node)?;
                    }
                    w.write_event(Event::End(BytesEnd::new("mtd")))
                        .map_err(to_err)?;
                }
                w.write_event(Event::End(BytesEnd::new("mtr")))
                    .map_err(to_err)?;
            }
            w.write_event(Event::End(BytesEnd::new("mtable")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Phantom(inner) => {
            w.write_event(Event::Start(BytesStart::new("mphantom")))
                .map_err(to_err)?;
            write_node(w, inner)?;
            w.write_event(Event::End(BytesEnd::new("mphantom")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Boxed(inner) => {
            let mut elem = BytesStart::new("menclose");
            elem.push_attribute(("notation", "box"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, inner)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Cancel { stroke, content } => {
            let mut elem = BytesStart::new("menclose");
            let notation = match stroke {
                StrokeKind::Forward => "updiagonalstrike",
                StrokeKind::Backward => "downdiagonalstrike",
                StrokeKind::Cross => "updiagonalstrike downdiagonalstrike",
                StrokeKind::Horizontal => "horizontalstrike",
                StrokeKind::Vertical => "verticalstrike",
            };
            elem.push_attribute(("notation", notation));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, content)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Scaled { content, .. } => write_node(w, content),

        MathNode::Styled(style, children) => {
            let variant = style_to_mathvariant(*style);
            let mut elem = BytesStart::new("mstyle");
            elem.push_attribute(("mathvariant", variant));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            for child in children {
                write_node(w, child)?;
            }
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
            Ok(())
        },
    }
}

fn write_simple_element(w: &mut XmlWriter, tag: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(to_err)?;
    Ok(())
}

fn write_mi(w: &mut XmlWriter, s: &str) -> Result<()> {
    let mut elem = BytesStart::new("mi");
    // Single-letter identifiers are italic by default in MathML;
    // multi-letter identifiers should be normal
    if s.chars().count() > 1
        && s.chars()
            .all(|c| c.is_ascii_alphabetic())
    {
        elem.push_attribute(("mathvariant", "normal"));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(s)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mi")))
        .map_err(to_err)?;
    Ok(())
}

fn write_mo(
    w: &mut XmlWriter,
    text: &str,
    fence: Option<&str>,
    stretchy: Option<&str>,
) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    if let Some(f) = fence {
        elem.push_attribute(("fence", f));
    }
    if let Some(s) = stretchy {
        elem.push_attribute(("stretchy", s));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn write_mo_char(w: &mut XmlWriter, ch: char) -> Result<()> {
    let s = ch.to_string();
    w.write_event(Event::Start(BytesStart::new("mo")))
        .map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(&s)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn style_to_mathvariant(style: TextStyle) -> &'static str {
    match style {
        TextStyle::Normal => "normal",
        TextStyle::Bold => "bold",
        TextStyle::Italic => "italic",
        TextStyle::BoldItalic => "bold-italic",
        TextStyle::Monospace => "monospace",
        TextStyle::SansSerif => "sans-serif",
        TextStyle::SansSerifBold => "bold-sans-serif",
        TextStyle::SansSerifItalic => "sans-serif-italic",
        TextStyle::SansSerifBoldItalic => "sans-serif-bold-italic",
        TextStyle::DoubleStruck => "double-struck",
        TextStyle::Script => "script",
        TextStyle::BoldScript => "bold-script",
        TextStyle::Fraktur => "fraktur",
        TextStyle::BoldFraktur => "bold-fraktur",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        MathmlWriter::write(&expr, &config).unwrap()
    }

    #[test]
    fn number() {
        let output = write(MathNode::Number("42".into()));
        assert!(output.contains("<mn>42</mn>"));
    }

    #[test]
    fn identifier() {
        let output = write(MathNode::Identifier("x".into()));
        assert!(output.contains("<mi>x</mi>"));
    }

    #[test]
    fn fraction() {
        let output = write(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<mfrac>"));
        assert!(output.contains("<mn>1</mn>"));
        assert!(output.contains("<mn>2</mn>"));
    }

    #[test]
    fn subscript() {
        let output = write(MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<msub>"));
    }

    #[test]
    fn sqrt() {
        let output = write(MathNode::Sqrt(Box::new(MathNode::Number("2".into()))));
        assert!(output.contains("<msqrt>"));
    }

    #[test]
    fn display_block() {
        let expr = MathExpr::block(vec![MathNode::Number("1".into())]);
        let config = Config::default();
        let output = MathmlWriter::write(&expr, &config).unwrap();
        assert!(output.contains("display=\"block\""));
    }

    #[test]
    fn phantom() {
        let output = write(MathNode::Phantom(Box::new(MathNode::Identifier("x".into()))));
        assert!(output.contains("<mphantom>"));
    }

    #[test]
    fn boxed() {
        let output = write(MathNode::Boxed(Box::new(MathNode::Number("1".into()))));
        assert!(output.contains("<menclose"));
        assert!(output.contains("notation=\"box\""));
    }
}
