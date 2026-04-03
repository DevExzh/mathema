use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

use crate::ast::*;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, StrId};
use crate::error::{Config, Result};
use crate::writer::Writer as MathWriter;

const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

pub struct OmmlWriter;

impl MathWriter for OmmlWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut w = Writer::new(Cursor::new(Vec::with_capacity(512)));
        let mut math = BytesStart::new("m:oMath");
        math.push_attribute(("xmlns:m", NS));
        w.write_event(Event::Start(math)).map_err(io_err)?;
        for node in &expr.nodes {
            write_node(&mut w, node)?;
        }
        w.write_event(Event::End(BytesEnd::new("m:oMath")))
            .map_err(io_err)?;
        let buf = w.into_inner().into_inner();
        String::from_utf8(buf).map_err(|e| {
            crate::error::Error::Unsupported(format!("OMML output contained invalid UTF-8: {e}"))
        })
    }

    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let mut w = Writer::new(Cursor::new(Vec::with_capacity(512)));
        let mut math = BytesStart::new("m:oMath");
        math.push_attribute(("xmlns:m", NS));
        w.write_event(Event::Start(math)).map_err(io_err)?;
        let arena = &expr.arena;
        for &root_id in arena.children(expr.roots) {
            write_node_c(&mut w, arena, root_id)?;
        }
        w.write_event(Event::End(BytesEnd::new("m:oMath")))
            .map_err(io_err)?;
        let buf = w.into_inner().into_inner();
        String::from_utf8(buf).map_err(|e| {
            crate::error::Error::Unsupported(format!("OMML output contained invalid UTF-8: {e}"))
        })
    }
}

type XmlW = Writer<Cursor<Vec<u8>>>;

fn io_err(e: std::io::Error) -> crate::error::Error {
    crate::error::Error::Io(e)
}

fn write_node(w: &mut XmlW, node: &MathNode) -> Result<()> {
    match node {
        MathNode::Number(s) => write_run(w, s, None),
        MathNode::Identifier(s) => write_run(w, s, Some("i")),
        MathNode::Operator(attr) => write_func_or_operator(w, attr),
        MathNode::Symbol(kind, s) => {
            if *kind == SymbolKind::LargeOp {
                // Large operators should ideally be in nary, but as a fallback
                write_run(w, s, None)
            } else {
                write_run(w, s, None)
            }
        },
        MathNode::Text(_, s) => write_run(w, s, Some("p")),
        MathNode::Space(_) => {
            // OMML doesn't have explicit space elements; use a text run
            write_run(w, "\u{00A0}", None)
        },

        MathNode::Row(children) => {
            for child in children {
                write_node(w, child)?;
            }
            Ok(())
        },

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            start(w, "m:d")?;
            // Properties
            start(w, "m:dPr")?;
            write_val_elem(w, "m:begChr", open)?;
            write_val_elem(w, "m:endChr", close)?;
            end(w, "m:dPr")?;
            // Content elements
            for item in content {
                match item {
                    FencedItem::Element(node) => {
                        start(w, "m:e")?;
                        write_node(w, node)?;
                        end(w, "m:e")?;
                    },
                    FencedItem::Middle(_) => {
                        // OMML uses separator in dPr, not inline. Skip for now.
                    },
                }
            }
            end(w, "m:d")
        },

        MathNode::Subscript { base, sub } => {
            start(w, "m:sSub")?;
            write_e(w, base)?;
            start(w, "m:sub")?;
            write_node(w, sub)?;
            end(w, "m:sub")?;
            end(w, "m:sSub")
        },

        MathNode::Superscript { base, sup } => {
            start(w, "m:sSup")?;
            write_e(w, base)?;
            start(w, "m:sup")?;
            write_node(w, sup)?;
            end(w, "m:sup")?;
            end(w, "m:sSup")
        },

        MathNode::SubSup { base, sub, sup } => {
            start(w, "m:sSubSup")?;
            write_e(w, base)?;
            start(w, "m:sub")?;
            write_node(w, sub)?;
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            write_node(w, sup)?;
            end(w, "m:sup")?;
            end(w, "m:sSubSup")
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            start(w, "m:sPre")?;
            start(w, "m:sub")?;
            if let Some(s) = pre_sub {
                write_node(w, s)?;
            }
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            if let Some(s) = pre_sup {
                write_node(w, s)?;
            }
            end(w, "m:sup")?;
            write_e(w, base)?;
            end(w, "m:sPre")
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if *movable_limits {
                start(w, "m:limUpp")?;
                write_e(w, base)?;
                start(w, "m:lim")?;
                write_node(w, over)?;
                end(w, "m:lim")?;
                end(w, "m:limUpp")
            } else {
                // Use bar for overline, groupChr for braces
                if is_line_symbol(over) {
                    start(w, "m:bar")?;
                    start(w, "m:barPr")?;
                    write_val_elem(w, "m:pos", "top")?;
                    end(w, "m:barPr")?;
                    write_e(w, base)?;
                    end(w, "m:bar")
                } else {
                    start(w, "m:groupChr")?;
                    start(w, "m:groupChrPr")?;
                    let chr_str = extract_symbol_text(over);
                    write_val_elem(w, "m:chr", &chr_str)?;
                    write_val_elem(w, "m:pos", "top")?;
                    end(w, "m:groupChrPr")?;
                    write_e(w, base)?;
                    end(w, "m:groupChr")
                }
            }
        },

        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if *movable_limits {
                start(w, "m:limLow")?;
                write_e(w, base)?;
                start(w, "m:lim")?;
                write_node(w, under)?;
                end(w, "m:lim")?;
                end(w, "m:limLow")
            } else {
                if is_line_symbol(under) {
                    start(w, "m:bar")?;
                    start(w, "m:barPr")?;
                    write_val_elem(w, "m:pos", "bot")?;
                    end(w, "m:barPr")?;
                    write_e(w, base)?;
                    end(w, "m:bar")
                } else {
                    start(w, "m:groupChr")?;
                    start(w, "m:groupChrPr")?;
                    let chr_str = extract_symbol_text(under);
                    write_val_elem(w, "m:chr", &chr_str)?;
                    write_val_elem(w, "m:pos", "bot")?;
                    end(w, "m:groupChrPr")?;
                    write_e(w, base)?;
                    end(w, "m:groupChr")
                }
            }
        },

        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits: _,
        } => {
            // Nest limLow inside limUpp
            start(w, "m:limUpp")?;
            start(w, "m:e")?;
            start(w, "m:limLow")?;
            write_e(w, base)?;
            start(w, "m:lim")?;
            write_node(w, under)?;
            end(w, "m:lim")?;
            end(w, "m:limLow")?;
            end(w, "m:e")?;
            start(w, "m:lim")?;
            write_node(w, over)?;
            end(w, "m:lim")?;
            end(w, "m:limUpp")
        },

        MathNode::Fraction { kind, num, den } => {
            start(w, "m:f")?;
            let type_val = match kind {
                FractionKind::Normal | FractionKind::Display => "bar",
                FractionKind::NoBar => "noBar",
                FractionKind::Skewed => "skw",
                FractionKind::Inline => "lin",
            };
            start(w, "m:fPr")?;
            write_val_elem(w, "m:type", type_val)?;
            end(w, "m:fPr")?;
            start(w, "m:num")?;
            write_node(w, num)?;
            end(w, "m:num")?;
            start(w, "m:den")?;
            write_node(w, den)?;
            end(w, "m:den")?;
            end(w, "m:f")
        },

        MathNode::Sqrt(inner) => {
            start(w, "m:rad")?;
            start(w, "m:radPr")?;
            write_val_elem(w, "m:degHide", "1")?;
            end(w, "m:radPr")?;
            start(w, "m:deg")?;
            end(w, "m:deg")?;
            write_e(w, inner)?;
            end(w, "m:rad")
        },

        MathNode::Root { index, radicand } => {
            start(w, "m:rad")?;
            start(w, "m:deg")?;
            write_node(w, index)?;
            end(w, "m:deg")?;
            write_e(w, radicand)?;
            end(w, "m:rad")
        },

        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            start(w, "m:nary")?;
            start(w, "m:naryPr")?;
            write_val_elem(w, "m:chr", &op.to_string())?;
            let lim_loc = match limits {
                LimitStyle::AboveBelow => "undOvr",
                LimitStyle::SubSup => "subSup",
                LimitStyle::Default => "undOvr",
            };
            write_val_elem(w, "m:limLoc", lim_loc)?;
            if sub.is_none() {
                write_val_elem(w, "m:subHide", "1")?;
            }
            if sup.is_none() {
                write_val_elem(w, "m:supHide", "1")?;
            }
            end(w, "m:naryPr")?;
            start(w, "m:sub")?;
            if let Some(s) = sub {
                write_node(w, s)?;
            }
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            if let Some(s) = sup {
                write_node(w, s)?;
            }
            end(w, "m:sup")?;
            write_e(w, body)?;
            end(w, "m:nary")
        },

        MathNode::Accent {
            base,
            accent,
            position: _,
        } => {
            start(w, "m:acc")?;
            start(w, "m:accPr")?;
            write_val_elem(w, "m:chr", &accent.to_string())?;
            end(w, "m:accPr")?;
            write_e(w, base)?;
            end(w, "m:acc")
        },

        MathNode::Array { rows, .. } => {
            start(w, "m:m")?;
            for row in rows {
                start(w, "m:mr")?;
                for cell in row {
                    start(w, "m:e")?;
                    for node in cell {
                        write_node(w, node)?;
                    }
                    end(w, "m:e")?;
                }
                end(w, "m:mr")?;
            }
            end(w, "m:m")
        },

        MathNode::Phantom(inner) => {
            start(w, "m:phant")?;
            write_e(w, inner)?;
            end(w, "m:phant")
        },

        MathNode::Boxed(inner) => {
            start(w, "m:borderBox")?;
            write_e(w, inner)?;
            end(w, "m:borderBox")
        },

        MathNode::Cancel { content, .. } => {
            // OMML doesn't have a direct cancel element; use borderBox with strikethrough
            start(w, "m:borderBox")?;
            start(w, "m:borderBoxPr")?;
            write_val_elem(w, "m:strikeBLTR", "1")?;
            end(w, "m:borderBoxPr")?;
            write_e(w, content)?;
            end(w, "m:borderBox")
        },

        MathNode::Scaled { content, .. } => write_node(w, content),

        MathNode::Styled(style, children) => {
            let (sty, scr) = decompose_style(*style);
            for child in children {
                match child {
                    MathNode::Identifier(s) | MathNode::Number(s) => {
                        write_styled_run(w, s, sty, scr)?;
                    },
                    MathNode::Text(_, s) => {
                        write_styled_run(w, s, sty, scr)?;
                    },
                    _ => write_node(w, child)?,
                }
            }
            Ok(())
        },
    }
}

// --- XML helpers ---

fn start(w: &mut XmlW, tag: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(io_err)
}

fn end(w: &mut XmlW, tag: &str) -> Result<()> {
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(io_err)
}

fn write_val_elem(w: &mut XmlW, tag: &str, val: &str) -> Result<()> {
    let mut elem = BytesStart::new(tag);
    elem.push_attribute(("m:val", val));
    w.write_event(Event::Empty(elem)).map_err(io_err)
}

fn write_run(w: &mut XmlW, text: &str, style: Option<&str>) -> Result<()> {
    start(w, "m:r")?;
    if let Some(sty) = style {
        start(w, "m:rPr")?;
        write_val_elem(w, "m:sty", sty)?;
        end(w, "m:rPr")?;
    }
    start(w, "m:t")?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(io_err)?;
    end(w, "m:t")?;
    end(w, "m:r")
}

/// Write a run with both sty and optional scr attributes.
fn write_styled_run(w: &mut XmlW, text: &str, sty: &str, scr: Option<&str>) -> Result<()> {
    start(w, "m:r")?;
    start(w, "m:rPr")?;
    write_val_elem(w, "m:sty", sty)?;
    if let Some(s) = scr {
        write_val_elem(w, "m:scr", s)?;
    }
    end(w, "m:rPr")?;
    start(w, "m:t")?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(io_err)?;
    end(w, "m:t")?;
    end(w, "m:r")
}

/// Decompose a TextStyle into OMML (sty, scr) pair.
fn decompose_style(style: TextStyle) -> (&'static str, Option<&'static str>) {
    match style {
        TextStyle::Normal => ("p", None),
        TextStyle::Bold => ("b", None),
        TextStyle::Italic => ("i", None),
        TextStyle::BoldItalic => ("bi", None),
        TextStyle::Script => ("p", Some("script")),
        TextStyle::BoldScript => ("b", Some("script")),
        TextStyle::Fraktur => ("p", Some("fraktur")),
        TextStyle::BoldFraktur => ("b", Some("fraktur")),
        TextStyle::DoubleStruck => ("p", Some("double-struck")),
        TextStyle::SansSerif => ("p", Some("sans-serif")),
        TextStyle::SansSerifBold => ("b", Some("sans-serif")),
        TextStyle::SansSerifItalic => ("i", Some("sans-serif")),
        TextStyle::SansSerifBoldItalic => ("bi", Some("sans-serif")),
        TextStyle::Monospace => ("p", Some("monospace")),
    }
}

fn write_e(w: &mut XmlW, node: &MathNode) -> Result<()> {
    start(w, "m:e")?;
    write_node(w, node)?;
    end(w, "m:e")
}

fn write_func_or_operator(w: &mut XmlW, attr: &OperatorAttr) -> Result<()> {
    start(w, "m:func")?;
    start(w, "m:fName")?;
    write_run(w, &attr.name, Some("p"))?;
    end(w, "m:fName")?;
    start(w, "m:e")?;
    end(w, "m:e")?;
    end(w, "m:func")
}

fn is_line_symbol(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(_, s) if s == "\u{203E}")
}

fn extract_symbol_text(node: &MathNode) -> String {
    match node {
        MathNode::Symbol(_, s) => s.to_owned(),
        _ => "\u{23DF}".to_owned(),
    }
}

// --- Compact arena helpers ---

fn write_node_c(w: &mut XmlW, arena: &AstArena, id: NodeId) -> Result<()> {
    match *arena.node(id) {
        CompactNode::Number(sid) => write_run_c(w, arena, sid, None),
        CompactNode::Identifier(sid) => write_run_c(w, arena, sid, Some("i")),
        CompactNode::Operator { name, .. } => write_func_or_operator_c(w, arena, name),
        CompactNode::Symbol(_kind, sid) => {
            write_run_c(w, arena, sid, None)
        },
        CompactNode::Text(_, sid) => write_run_c(w, arena, sid, Some("p")),
        CompactNode::Space(_) => {
            write_run(w, "\u{00A0}", None)
        },

        CompactNode::Row(span) => {
            for &child_id in arena.children(span) {
                write_node_c(w, arena, child_id)?;
            }
            Ok(())
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            start(w, "m:d")?;
            start(w, "m:dPr")?;
            write_val_elem(w, "m:begChr", arena.str_val(open))?;
            write_val_elem(w, "m:endChr", arena.str_val(close))?;
            end(w, "m:dPr")?;
            for &child_id in arena.children(content) {
                match *arena.node(child_id) {
                    CompactNode::Middle(_) => {
                        // OMML uses separator in dPr, not inline. Skip for now.
                    },
                    _ => {
                        start(w, "m:e")?;
                        write_node_c(w, arena, child_id)?;
                        end(w, "m:e")?;
                    },
                }
            }
            end(w, "m:d")
        },

        CompactNode::Subscript { base, sub } => {
            start(w, "m:sSub")?;
            write_e_c(w, arena, base)?;
            start(w, "m:sub")?;
            write_node_c(w, arena, sub)?;
            end(w, "m:sub")?;
            end(w, "m:sSub")
        },

        CompactNode::Superscript { base, sup } => {
            start(w, "m:sSup")?;
            write_e_c(w, arena, base)?;
            start(w, "m:sup")?;
            write_node_c(w, arena, sup)?;
            end(w, "m:sup")?;
            end(w, "m:sSup")
        },

        CompactNode::SubSup { base, sub, sup } => {
            start(w, "m:sSubSup")?;
            write_e_c(w, arena, base)?;
            start(w, "m:sub")?;
            write_node_c(w, arena, sub)?;
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            write_node_c(w, arena, sup)?;
            end(w, "m:sup")?;
            end(w, "m:sSubSup")
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            start(w, "m:sPre")?;
            start(w, "m:sub")?;
            if pre_sub.is_some() {
                write_node_c(w, arena, pre_sub)?;
            }
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            if pre_sup.is_some() {
                write_node_c(w, arena, pre_sup)?;
            }
            end(w, "m:sup")?;
            write_e_c(w, arena, base)?;
            end(w, "m:sPre")
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if movable_limits {
                start(w, "m:limUpp")?;
                write_e_c(w, arena, base)?;
                start(w, "m:lim")?;
                write_node_c(w, arena, over)?;
                end(w, "m:lim")?;
                end(w, "m:limUpp")
            } else {
                if is_line_symbol_c(arena, over) {
                    start(w, "m:bar")?;
                    start(w, "m:barPr")?;
                    write_val_elem(w, "m:pos", "top")?;
                    end(w, "m:barPr")?;
                    write_e_c(w, arena, base)?;
                    end(w, "m:bar")
                } else {
                    start(w, "m:groupChr")?;
                    start(w, "m:groupChrPr")?;
                    let chr_str = extract_symbol_text_c(arena, over);
                    write_val_elem(w, "m:chr", &chr_str)?;
                    write_val_elem(w, "m:pos", "top")?;
                    end(w, "m:groupChrPr")?;
                    write_e_c(w, arena, base)?;
                    end(w, "m:groupChr")
                }
            }
        },

        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if movable_limits {
                start(w, "m:limLow")?;
                write_e_c(w, arena, base)?;
                start(w, "m:lim")?;
                write_node_c(w, arena, under)?;
                end(w, "m:lim")?;
                end(w, "m:limLow")
            } else {
                if is_line_symbol_c(arena, under) {
                    start(w, "m:bar")?;
                    start(w, "m:barPr")?;
                    write_val_elem(w, "m:pos", "bot")?;
                    end(w, "m:barPr")?;
                    write_e_c(w, arena, base)?;
                    end(w, "m:bar")
                } else {
                    start(w, "m:groupChr")?;
                    start(w, "m:groupChrPr")?;
                    let chr_str = extract_symbol_text_c(arena, under);
                    write_val_elem(w, "m:chr", &chr_str)?;
                    write_val_elem(w, "m:pos", "bot")?;
                    end(w, "m:groupChrPr")?;
                    write_e_c(w, arena, base)?;
                    end(w, "m:groupChr")
                }
            }
        },

        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits: _,
        } => {
            start(w, "m:limUpp")?;
            start(w, "m:e")?;
            start(w, "m:limLow")?;
            write_e_c(w, arena, base)?;
            start(w, "m:lim")?;
            write_node_c(w, arena, under)?;
            end(w, "m:lim")?;
            end(w, "m:limLow")?;
            end(w, "m:e")?;
            start(w, "m:lim")?;
            write_node_c(w, arena, over)?;
            end(w, "m:lim")?;
            end(w, "m:limUpp")
        },

        CompactNode::Fraction { kind, num, den } => {
            start(w, "m:f")?;
            let type_val = match kind {
                FractionKind::Normal | FractionKind::Display => "bar",
                FractionKind::NoBar => "noBar",
                FractionKind::Skewed => "skw",
                FractionKind::Inline => "lin",
            };
            start(w, "m:fPr")?;
            write_val_elem(w, "m:type", type_val)?;
            end(w, "m:fPr")?;
            start(w, "m:num")?;
            write_node_c(w, arena, num)?;
            end(w, "m:num")?;
            start(w, "m:den")?;
            write_node_c(w, arena, den)?;
            end(w, "m:den")?;
            end(w, "m:f")
        },

        CompactNode::Sqrt(inner) => {
            start(w, "m:rad")?;
            start(w, "m:radPr")?;
            write_val_elem(w, "m:degHide", "1")?;
            end(w, "m:radPr")?;
            start(w, "m:deg")?;
            end(w, "m:deg")?;
            write_e_c(w, arena, inner)?;
            end(w, "m:rad")
        },

        CompactNode::Root { index, radicand } => {
            start(w, "m:rad")?;
            start(w, "m:deg")?;
            write_node_c(w, arena, index)?;
            end(w, "m:deg")?;
            write_e_c(w, arena, radicand)?;
            end(w, "m:rad")
        },

        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            start(w, "m:nary")?;
            start(w, "m:naryPr")?;
            write_val_elem(w, "m:chr", &op.to_string())?;
            let lim_loc = match limits {
                LimitStyle::AboveBelow => "undOvr",
                LimitStyle::SubSup => "subSup",
                LimitStyle::Default => "undOvr",
            };
            write_val_elem(w, "m:limLoc", lim_loc)?;
            if sub.is_none() {
                write_val_elem(w, "m:subHide", "1")?;
            }
            if sup.is_none() {
                write_val_elem(w, "m:supHide", "1")?;
            }
            end(w, "m:naryPr")?;
            start(w, "m:sub")?;
            if sub.is_some() {
                write_node_c(w, arena, sub)?;
            }
            end(w, "m:sub")?;
            start(w, "m:sup")?;
            if sup.is_some() {
                write_node_c(w, arena, sup)?;
            }
            end(w, "m:sup")?;
            write_e_c(w, arena, body)?;
            end(w, "m:nary")
        },

        CompactNode::Accent {
            base,
            accent,
            position: _,
        } => {
            start(w, "m:acc")?;
            start(w, "m:accPr")?;
            write_val_elem(w, "m:chr", &accent.to_string())?;
            end(w, "m:accPr")?;
            write_e_c(w, arena, base)?;
            end(w, "m:acc")
        },

        CompactNode::Array {
            cells,
            num_cols,
            ..
        } => {
            start(w, "m:m")?;
            let cell_ids = arena.children(cells);
            let nc = num_cols as usize;
            for chunk in cell_ids.chunks(nc.max(1)) {
                start(w, "m:mr")?;
                for &cell_id in chunk {
                    start(w, "m:e")?;
                    // Each cell is a Row node wrapping the cell contents
                    match *arena.node(cell_id) {
                        CompactNode::Row(span) => {
                            for &child_id in arena.children(span) {
                                write_node_c(w, arena, child_id)?;
                            }
                        },
                        _ => {
                            write_node_c(w, arena, cell_id)?;
                        },
                    }
                    end(w, "m:e")?;
                }
                end(w, "m:mr")?;
            }
            end(w, "m:m")
        },

        CompactNode::Phantom(inner) => {
            start(w, "m:phant")?;
            write_e_c(w, arena, inner)?;
            end(w, "m:phant")
        },

        CompactNode::Boxed(inner) => {
            start(w, "m:borderBox")?;
            write_e_c(w, arena, inner)?;
            end(w, "m:borderBox")
        },

        CompactNode::Cancel { content, .. } => {
            start(w, "m:borderBox")?;
            start(w, "m:borderBoxPr")?;
            write_val_elem(w, "m:strikeBLTR", "1")?;
            end(w, "m:borderBoxPr")?;
            write_e_c(w, arena, content)?;
            end(w, "m:borderBox")
        },

        CompactNode::Scaled { content, .. } => write_node_c(w, arena, content),

        CompactNode::Styled(style, span) => {
            let (sty, scr) = decompose_style(style);
            for &child_id in arena.children(span) {
                match *arena.node(child_id) {
                    CompactNode::Identifier(sid) | CompactNode::Number(sid) => {
                        write_styled_run_c(w, arena, sid, sty, scr)?;
                    },
                    CompactNode::Text(_, sid) => {
                        write_styled_run_c(w, arena, sid, sty, scr)?;
                    },
                    _ => write_node_c(w, arena, child_id)?,
                }
            }
            Ok(())
        },

        // Helper nodes that should not normally appear at the top level
        CompactNode::Middle(_) | CompactNode::ColAlign(_) => Ok(()),
    }
}

fn write_run_c(w: &mut XmlW, arena: &AstArena, sid: StrId, style: Option<&str>) -> Result<()> {
    write_run(w, arena.str_val(sid), style)
}

fn write_styled_run_c(
    w: &mut XmlW,
    arena: &AstArena,
    sid: StrId,
    sty: &str,
    scr: Option<&str>,
) -> Result<()> {
    write_styled_run(w, arena.str_val(sid), sty, scr)
}

fn write_e_c(w: &mut XmlW, arena: &AstArena, id: NodeId) -> Result<()> {
    start(w, "m:e")?;
    write_node_c(w, arena, id)?;
    end(w, "m:e")
}

fn write_func_or_operator_c(w: &mut XmlW, arena: &AstArena, name: StrId) -> Result<()> {
    start(w, "m:func")?;
    start(w, "m:fName")?;
    write_run(w, arena.str_val(name), Some("p"))?;
    end(w, "m:fName")?;
    start(w, "m:e")?;
    end(w, "m:e")?;
    end(w, "m:func")
}

fn is_line_symbol_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == "\u{203E}")
}

fn extract_symbol_text_c(arena: &AstArena, id: NodeId) -> String {
    match *arena.node(id) {
        CompactNode::Symbol(_, sid) => arena.str_val(sid).to_owned(),
        _ => "\u{23DF}".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        OmmlWriter::write(&expr, &config).unwrap()
    }

    #[test]
    fn number() {
        let output = write(MathNode::Number("42".into()));
        assert!(output.contains("<m:t>42</m:t>"));
    }

    #[test]
    fn identifier() {
        let output = write(MathNode::Identifier("x".into()));
        assert!(output.contains("<m:t>x</m:t>"));
    }

    #[test]
    fn fraction() {
        let output = write(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<m:f>"));
        assert!(output.contains("<m:num>"));
        assert!(output.contains("<m:den>"));
    }

    #[test]
    fn subscript() {
        let output = write(MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<m:sSub>"));
    }

    #[test]
    fn sqrt() {
        let output = write(MathNode::Sqrt(Box::new(MathNode::Number("2".into()))));
        assert!(output.contains("<m:rad>"));
        assert!(output.contains("degHide"));
    }

    #[test]
    fn nary_sum() {
        let output = write(MathNode::Nary {
            op: '\u{2211}',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("i".into())),
            limits: LimitStyle::AboveBelow,
        });
        assert!(output.contains("<m:nary>"));
        assert!(output.contains("\u{2211}"));
    }

    #[test]
    fn delimiter() {
        let output = write(MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        });
        assert!(output.contains("<m:d>"));
        assert!(output.contains("begChr"));
    }

    #[test]
    fn accent() {
        let output = write(MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '\u{0302}',
            position: AccentPosition::Above,
        });
        assert!(output.contains("<m:acc>"));
    }

    #[test]
    fn phantom() {
        let output = write(MathNode::Phantom(Box::new(MathNode::Identifier(
            "x".into(),
        ))));
        assert!(output.contains("<m:phant>"));
    }

    #[test]
    fn boxed() {
        let output = write(MathNode::Boxed(Box::new(MathNode::Number("1".into()))));
        assert!(output.contains("<m:borderBox>"));
    }
}
