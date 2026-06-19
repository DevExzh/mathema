use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::*;
use crate::error::{Config, Diagnostic, Result};
use crate::writer::Writer as MathWriter;

const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Classifies a compact node that the OMML writer cannot fully represent.
fn omml_loss(_arena: &AstArena, node: &CompactNode) -> Option<String> {
    match node {
        CompactNode::Color { .. } => {
            Some("OMML output does not preserve color; only the content is written".into())
        },
        CompactNode::MathChoice { .. } => {
            Some("OMML output keeps only the display-style branch of \\mathchoice".into())
        },
        CompactNode::MathAtom { .. } => {
            Some("OMML output does not preserve the math atom spacing class".into())
        },
        CompactNode::Rule { .. } => {
            Some("OMML cannot represent a rule (\\rule/\\hline); written as a space".into())
        },
        CompactNode::Scaled { .. } => {
            Some("OMML output does not preserve the \\scalebox factor".into())
        },
        CompactNode::RaiseBox { .. } => {
            Some("OMML output does not preserve the \\raisebox shift".into())
        },
        _ => None,
    }
}

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

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let out = Self::write_compact(expr, config)?;
        let diags = super::collect_loss_diagnostics(expr, omml_loss);
        Ok((out, diags))
    }

    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let arena = &expr.arena;
        let mut buf = String::with_capacity(512);
        buf.push_str("<m:oMath xmlns:m=\"");
        buf.push_str(NS);
        buf.push_str("\">");
        for &root_id in arena.children(expr.roots) {
            write_node_cf(&mut buf, arena, root_id);
        }
        buf.push_str("</m:oMath>");
        Ok(buf)
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
        MathNode::Space(_) | MathNode::Rule { .. } => {
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
            if let Some(separator) = fenced_separator(content) {
                write_val_elem(w, "m:sepChr", separator)?;
            }
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

        MathNode::Accent { base, accent, .. } => {
            start(w, "m:acc")?;
            start(w, "m:accPr")?;
            write_val_elem(w, "m:chr", &accent.to_string())?;
            end(w, "m:accPr")?;
            write_e(w, base)?;
            end(w, "m:acc")
        },

        MathNode::Array { columns, rows, .. } => {
            start(w, "m:m")?;
            write_matrix_props(w, columns)?;
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

        MathNode::Phantom(inner, kind) => {
            start(w, "m:phant")?;
            write_phantom_pr(w, *kind)?;
            write_e(w, inner)?;
            end(w, "m:phant")
        },

        MathNode::Boxed { content: inner, .. } => {
            start(w, "m:borderBox")?;
            write_e(w, inner)?;
            end(w, "m:borderBox")
        },

        MathNode::Cancel { stroke, content } => {
            // OMML represents cancellation as borderBox strike properties.
            start(w, "m:borderBox")?;
            start(w, "m:borderBoxPr")?;
            write_cancel_strike_props(w, *stroke)?;
            end(w, "m:borderBoxPr")?;
            write_e(w, content)?;
            end(w, "m:borderBox")
        },

        MathNode::Scaled { content, .. } | MathNode::RaiseBox { content, .. } => {
            write_node(w, content)
        },

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
        MathNode::MathChoice { display, .. } => {
            // OMML doesn't have mathchoice; output the display branch
            write_node(w, display)
        },
        MathNode::MathAtom { content, .. } => write_node(w, content),
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

/// Map a [`PhantomKind`] to OMML `phantPr` flags
/// `(show, zero_wid, zero_asc, zero_desc)`, where `show` is whether the
/// content is visible (`\smash` family) versus invisible (`\phantom` family).
fn phantom_pr_flags(kind: PhantomKind) -> (bool, bool, bool, bool) {
    match kind {
        // Invisible, all dimensions kept.
        PhantomKind::Full => (false, false, false, false),
        // \vphantom: invisible, zero width, keep height/depth.
        PhantomKind::Vertical => (false, true, false, false),
        // \hphantom: invisible, keep width, zero height/depth.
        PhantomKind::Horizontal => (false, false, true, true),
        // \smash: visible, zero height and depth.
        PhantomKind::SmashBoth => (true, false, true, true),
        // \smash[t]: visible, zero height (ascent) only.
        PhantomKind::SmashTop => (true, false, true, false),
        // \smash[b]: visible, zero depth (descent) only.
        PhantomKind::SmashBottom => (true, false, false, true),
    }
}

/// Emit an OMML `<m:phantPr>` element describing `kind`. `show` is written
/// explicitly so the subtype round-trips regardless of reader defaults.
fn write_phantom_pr(w: &mut XmlW, kind: PhantomKind) -> Result<()> {
    let (show, zero_wid, zero_asc, zero_desc) = phantom_pr_flags(kind);
    start(w, "m:phantPr")?;
    write_val_elem(w, "m:show", if show { "1" } else { "0" })?;
    if zero_wid {
        write_val_elem(w, "m:zeroWid", "1")?;
    }
    if zero_asc {
        write_val_elem(w, "m:zeroAsc", "1")?;
    }
    if zero_desc {
        write_val_elem(w, "m:zeroDesc", "1")?;
    }
    end(w, "m:phantPr")
}

fn fenced_separator(content: &[FencedItem]) -> Option<&str> {
    content.iter().find_map(|item| match item {
        FencedItem::Middle(separator) => Some(separator.as_str()),
        FencedItem::Element(_) => None,
    })
}

fn write_matrix_props(w: &mut XmlW, columns: &[ColumnAlign]) -> Result<()> {
    if columns.is_empty() {
        return Ok(());
    }
    start(w, "m:mPr")?;
    start(w, "m:mcs")?;
    for column in columns {
        start(w, "m:mc")?;
        start(w, "m:mcPr")?;
        write_val_elem(w, "m:count", "1")?;
        write_val_elem(w, "m:mcJc", column_align_val(*column))?;
        end(w, "m:mcPr")?;
        end(w, "m:mc")?;
    }
    end(w, "m:mcs")?;
    end(w, "m:mPr")
}

fn write_cancel_strike_props(w: &mut XmlW, stroke: StrokeKind) -> Result<()> {
    match stroke {
        StrokeKind::Forward => write_val_elem(w, "m:strikeBLTR", "1"),
        StrokeKind::Backward => write_val_elem(w, "m:strikeTLBR", "1"),
        StrokeKind::Cross => {
            write_val_elem(w, "m:strikeBLTR", "1")?;
            write_val_elem(w, "m:strikeTLBR", "1")
        },
        StrokeKind::Horizontal => write_val_elem(w, "m:strikeH", "1"),
        StrokeKind::Vertical => write_val_elem(w, "m:strikeV", "1"),
    }
}

fn column_align_val(align: ColumnAlign) -> &'static str {
    match align {
        ColumnAlign::Left => "left",
        ColumnAlign::Center => "center",
        ColumnAlign::Right => "right",
    }
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

// --- Compact arena helpers (legacy quick_xml path, kept for reference) ---

#[allow(dead_code)]
fn write_node_c(w: &mut XmlW, arena: &AstArena, id: NodeId) -> Result<()> {
    match *arena.node(id) {
        CompactNode::Number(sid) => write_run_c(w, arena, sid, None),
        CompactNode::Identifier(sid) => write_run_c(w, arena, sid, Some("i")),
        CompactNode::Operator { name, .. } => write_func_or_operator_c(w, arena, name),
        CompactNode::Symbol(_kind, sid) => write_run_c(w, arena, sid, None),
        CompactNode::Text(_, sid) => write_run_c(w, arena, sid, Some("p")),
        CompactNode::Space(_) => write_run(w, "\u{00A0}", None),

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
            if let Some(separator) = compact_fenced_separator(arena, content) {
                write_val_elem(w, "m:sepChr", separator)?;
            }
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

        CompactNode::Accent { base, accent, .. } => {
            start(w, "m:acc")?;
            start(w, "m:accPr")?;
            write_val_elem(w, "m:chr", &accent.to_string())?;
            end(w, "m:accPr")?;
            write_e_c(w, arena, base)?;
            end(w, "m:acc")
        },

        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            ..
        } => {
            start(w, "m:m")?;
            write_matrix_props_c(w, arena, col_aligns)?;
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

        CompactNode::Phantom(inner, kind) => {
            start(w, "m:phant")?;
            write_phantom_pr(w, kind)?;
            write_e_c(w, arena, inner)?;
            end(w, "m:phant")
        },

        CompactNode::Boxed { content: inner, .. } => {
            start(w, "m:borderBox")?;
            write_e_c(w, arena, inner)?;
            end(w, "m:borderBox")
        },

        CompactNode::Cancel { stroke, content } => {
            start(w, "m:borderBox")?;
            start(w, "m:borderBoxPr")?;
            write_cancel_strike_props(w, stroke)?;
            end(w, "m:borderBoxPr")?;
            write_e_c(w, arena, content)?;
            end(w, "m:borderBox")
        },

        CompactNode::Scaled { content, .. } | CompactNode::RaiseBox { content, .. } => {
            write_node_c(w, arena, content)
        },

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

        // Color: OMML has no direct color support, just write the content
        CompactNode::Color { content, .. } => write_node_c(w, arena, content),

        CompactNode::MathChoice { display, .. } => write_node_c(w, arena, display),

        CompactNode::MathAtom { content, .. } => write_node_c(w, arena, content),

        CompactNode::Rule { .. } => write_run(w, "\u{00A0}", None),

        // Helper nodes that should not normally appear at the top level
        CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_)
        | CompactNode::Newline => Ok(()),
    }
}

#[allow(dead_code)]
fn write_run_c(w: &mut XmlW, arena: &AstArena, sid: StrId, style: Option<&str>) -> Result<()> {
    write_run(w, arena.str_val(sid), style)
}

#[allow(dead_code)]
fn write_styled_run_c(
    w: &mut XmlW,
    arena: &AstArena,
    sid: StrId,
    sty: &str,
    scr: Option<&str>,
) -> Result<()> {
    write_styled_run(w, arena.str_val(sid), sty, scr)
}

#[allow(dead_code)]
fn write_e_c(w: &mut XmlW, arena: &AstArena, id: NodeId) -> Result<()> {
    start(w, "m:e")?;
    write_node_c(w, arena, id)?;
    end(w, "m:e")
}

#[allow(dead_code)]
fn write_matrix_props_c(w: &mut XmlW, arena: &AstArena, col_aligns: NodeSpan) -> Result<()> {
    let ids = arena.children(col_aligns);
    if ids.is_empty() {
        return Ok(());
    }
    start(w, "m:mPr")?;
    start(w, "m:mcs")?;
    for &id in ids {
        if let CompactNode::ColAlign(column) = *arena.node(id) {
            start(w, "m:mc")?;
            start(w, "m:mcPr")?;
            write_val_elem(w, "m:count", "1")?;
            write_val_elem(w, "m:mcJc", column_align_val(column))?;
            end(w, "m:mcPr")?;
            end(w, "m:mc")?;
        }
    }
    end(w, "m:mcs")?;
    end(w, "m:mPr")
}

#[allow(dead_code)]
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

// ── Fast direct-string compact OMML writer (bypasses quick_xml) ─────

/// Escape XML special characters in text content.
#[inline]
fn xml_esc(buf: &mut String, s: &str) {
    if !s
        .bytes()
        .any(|b| matches!(b, b'<' | b'>' | b'&' | b'"' | b'\''))
    {
        buf.push_str(s);
        return;
    }
    for ch in s.chars() {
        match ch {
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '&' => buf.push_str("&amp;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            _ => buf.push(ch),
        }
    }
}

#[inline]
fn f_start(buf: &mut String, tag: &str) {
    buf.push('<');
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn f_end(buf: &mut String, tag: &str) {
    buf.push_str("</");
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn f_val_elem(buf: &mut String, tag: &str, val: &str) {
    buf.push('<');
    buf.push_str(tag);
    buf.push_str(" m:val=\"");
    xml_esc(buf, val);
    buf.push_str("\"/>");
}

/// Buffered-writer equivalent of [`write_phantom_pr`].
fn f_write_phantom_pr(buf: &mut String, kind: PhantomKind) {
    let (show, zero_wid, zero_asc, zero_desc) = phantom_pr_flags(kind);
    f_start(buf, "m:phantPr");
    f_val_elem(buf, "m:show", if show { "1" } else { "0" });
    if zero_wid {
        f_val_elem(buf, "m:zeroWid", "1");
    }
    if zero_asc {
        f_val_elem(buf, "m:zeroAsc", "1");
    }
    if zero_desc {
        f_val_elem(buf, "m:zeroDesc", "1");
    }
    f_end(buf, "m:phantPr");
}

fn compact_fenced_separator(arena: &AstArena, content: NodeSpan) -> Option<&str> {
    arena
        .children(content)
        .iter()
        .find_map(|&child_id| match *arena.node(child_id) {
            CompactNode::Middle(sid) => Some(arena.str_val(sid)),
            _ => None,
        })
}

fn f_matrix_props(buf: &mut String, arena: &AstArena, col_aligns: NodeSpan) {
    let ids = arena.children(col_aligns);
    if ids.is_empty() {
        return;
    }
    f_start(buf, "m:mPr");
    f_start(buf, "m:mcs");
    for &id in ids {
        if let CompactNode::ColAlign(column) = *arena.node(id) {
            f_start(buf, "m:mc");
            f_start(buf, "m:mcPr");
            f_val_elem(buf, "m:count", "1");
            f_val_elem(buf, "m:mcJc", column_align_val(column));
            f_end(buf, "m:mcPr");
            f_end(buf, "m:mc");
        }
    }
    f_end(buf, "m:mcs");
    f_end(buf, "m:mPr");
}

fn f_cancel_strike_props(buf: &mut String, stroke: StrokeKind) {
    match stroke {
        StrokeKind::Forward => f_val_elem(buf, "m:strikeBLTR", "1"),
        StrokeKind::Backward => f_val_elem(buf, "m:strikeTLBR", "1"),
        StrokeKind::Cross => {
            f_val_elem(buf, "m:strikeBLTR", "1");
            f_val_elem(buf, "m:strikeTLBR", "1");
        },
        StrokeKind::Horizontal => f_val_elem(buf, "m:strikeH", "1"),
        StrokeKind::Vertical => f_val_elem(buf, "m:strikeV", "1"),
    }
}

#[inline]
fn f_run(buf: &mut String, text: &str, style: Option<&str>) {
    f_start(buf, "m:r");
    if let Some(sty) = style {
        f_start(buf, "m:rPr");
        f_val_elem(buf, "m:sty", sty);
        f_end(buf, "m:rPr");
    }
    f_start(buf, "m:t");
    xml_esc(buf, text);
    f_end(buf, "m:t");
    f_end(buf, "m:r");
}

#[inline]
fn f_styled_run(buf: &mut String, text: &str, sty: &str, scr: Option<&str>) {
    f_start(buf, "m:r");
    f_start(buf, "m:rPr");
    f_val_elem(buf, "m:sty", sty);
    if let Some(s) = scr {
        f_val_elem(buf, "m:scr", s);
    }
    f_end(buf, "m:rPr");
    f_start(buf, "m:t");
    xml_esc(buf, text);
    f_end(buf, "m:t");
    f_end(buf, "m:r");
}

#[inline]
fn f_e(buf: &mut String, arena: &AstArena, id: NodeId) {
    f_start(buf, "m:e");
    write_node_cf(buf, arena, id);
    f_end(buf, "m:e");
}

fn write_node_cf(buf: &mut String, arena: &AstArena, id: NodeId) {
    match *arena.node(id) {
        CompactNode::Number(sid) => f_run(buf, arena.str_val(sid), None),
        CompactNode::Identifier(sid) => f_run(buf, arena.str_val(sid), Some("i")),
        CompactNode::Operator { name, .. } => {
            f_start(buf, "m:func");
            f_start(buf, "m:fName");
            f_run(buf, arena.str_val(name), Some("p"));
            f_end(buf, "m:fName");
            f_start(buf, "m:e");
            f_end(buf, "m:e");
            f_end(buf, "m:func");
        },
        CompactNode::Symbol(_kind, sid) => f_run(buf, arena.str_val(sid), None),
        CompactNode::Text(_, sid) => f_run(buf, arena.str_val(sid), Some("p")),
        CompactNode::Space(_) => f_run(buf, "\u{00A0}", None),

        CompactNode::Row(span) => {
            for &child_id in arena.children(span) {
                write_node_cf(buf, arena, child_id);
            }
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            f_start(buf, "m:d");
            f_start(buf, "m:dPr");
            f_val_elem(buf, "m:begChr", arena.str_val(open));
            f_val_elem(buf, "m:endChr", arena.str_val(close));
            if let Some(separator) = compact_fenced_separator(arena, content) {
                f_val_elem(buf, "m:sepChr", separator);
            }
            f_end(buf, "m:dPr");
            for &child_id in arena.children(content) {
                if matches!(*arena.node(child_id), CompactNode::Middle(_)) {
                    continue;
                }
                f_start(buf, "m:e");
                write_node_cf(buf, arena, child_id);
                f_end(buf, "m:e");
            }
            f_end(buf, "m:d");
        },

        CompactNode::Subscript { base, sub } => {
            f_start(buf, "m:sSub");
            f_e(buf, arena, base);
            f_start(buf, "m:sub");
            write_node_cf(buf, arena, sub);
            f_end(buf, "m:sub");
            f_end(buf, "m:sSub");
        },

        CompactNode::Superscript { base, sup } => {
            f_start(buf, "m:sSup");
            f_e(buf, arena, base);
            f_start(buf, "m:sup");
            write_node_cf(buf, arena, sup);
            f_end(buf, "m:sup");
            f_end(buf, "m:sSup");
        },

        CompactNode::SubSup { base, sub, sup } => {
            f_start(buf, "m:sSubSup");
            f_e(buf, arena, base);
            f_start(buf, "m:sub");
            write_node_cf(buf, arena, sub);
            f_end(buf, "m:sub");
            f_start(buf, "m:sup");
            write_node_cf(buf, arena, sup);
            f_end(buf, "m:sup");
            f_end(buf, "m:sSubSup");
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            f_start(buf, "m:sPre");
            f_start(buf, "m:sub");
            if pre_sub.is_some() {
                write_node_cf(buf, arena, pre_sub);
            }
            f_end(buf, "m:sub");
            f_start(buf, "m:sup");
            if pre_sup.is_some() {
                write_node_cf(buf, arena, pre_sup);
            }
            f_end(buf, "m:sup");
            f_e(buf, arena, base);
            f_end(buf, "m:sPre");
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if movable_limits {
                f_start(buf, "m:limUpp");
                f_e(buf, arena, base);
                f_start(buf, "m:lim");
                write_node_cf(buf, arena, over);
                f_end(buf, "m:lim");
                f_end(buf, "m:limUpp");
            } else if is_line_symbol_c(arena, over) {
                f_start(buf, "m:bar");
                f_start(buf, "m:barPr");
                f_val_elem(buf, "m:pos", "top");
                f_end(buf, "m:barPr");
                f_e(buf, arena, base);
                f_end(buf, "m:bar");
            } else {
                f_start(buf, "m:groupChr");
                f_start(buf, "m:groupChrPr");
                let chr_str = extract_symbol_text_c(arena, over);
                f_val_elem(buf, "m:chr", &chr_str);
                f_val_elem(buf, "m:pos", "top");
                f_end(buf, "m:groupChrPr");
                f_e(buf, arena, base);
                f_end(buf, "m:groupChr");
            }
        },

        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if movable_limits {
                f_start(buf, "m:limLow");
                f_e(buf, arena, base);
                f_start(buf, "m:lim");
                write_node_cf(buf, arena, under);
                f_end(buf, "m:lim");
                f_end(buf, "m:limLow");
            } else if is_line_symbol_c(arena, under) {
                f_start(buf, "m:bar");
                f_start(buf, "m:barPr");
                f_val_elem(buf, "m:pos", "bot");
                f_end(buf, "m:barPr");
                f_e(buf, arena, base);
                f_end(buf, "m:bar");
            } else {
                f_start(buf, "m:groupChr");
                f_start(buf, "m:groupChrPr");
                let chr_str = extract_symbol_text_c(arena, under);
                f_val_elem(buf, "m:chr", &chr_str);
                f_val_elem(buf, "m:pos", "bot");
                f_end(buf, "m:groupChrPr");
                f_e(buf, arena, base);
                f_end(buf, "m:groupChr");
            }
        },

        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits: _,
        } => {
            f_start(buf, "m:limUpp");
            f_start(buf, "m:e");
            f_start(buf, "m:limLow");
            f_e(buf, arena, base);
            f_start(buf, "m:lim");
            write_node_cf(buf, arena, under);
            f_end(buf, "m:lim");
            f_end(buf, "m:limLow");
            f_end(buf, "m:e");
            f_start(buf, "m:lim");
            write_node_cf(buf, arena, over);
            f_end(buf, "m:lim");
            f_end(buf, "m:limUpp");
        },

        CompactNode::Fraction { kind, num, den } => {
            let type_val = match kind {
                FractionKind::Normal | FractionKind::Display => "bar",
                FractionKind::NoBar => "noBar",
                FractionKind::Skewed => "skw",
                FractionKind::Inline => "lin",
            };
            f_start(buf, "m:f");
            f_start(buf, "m:fPr");
            f_val_elem(buf, "m:type", type_val);
            f_end(buf, "m:fPr");
            f_start(buf, "m:num");
            write_node_cf(buf, arena, num);
            f_end(buf, "m:num");
            f_start(buf, "m:den");
            write_node_cf(buf, arena, den);
            f_end(buf, "m:den");
            f_end(buf, "m:f");
        },

        CompactNode::Sqrt(inner) => {
            f_start(buf, "m:rad");
            f_start(buf, "m:radPr");
            f_val_elem(buf, "m:degHide", "1");
            f_end(buf, "m:radPr");
            f_start(buf, "m:deg");
            f_end(buf, "m:deg");
            f_e(buf, arena, inner);
            f_end(buf, "m:rad");
        },

        CompactNode::Root { index, radicand } => {
            f_start(buf, "m:rad");
            f_start(buf, "m:deg");
            write_node_cf(buf, arena, index);
            f_end(buf, "m:deg");
            f_e(buf, arena, radicand);
            f_end(buf, "m:rad");
        },

        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let lim_loc = match limits {
                LimitStyle::AboveBelow => "undOvr",
                LimitStyle::SubSup => "subSup",
                LimitStyle::Default => "undOvr",
            };
            f_start(buf, "m:nary");
            f_start(buf, "m:naryPr");
            // Write op char
            let mut char_buf = [0u8; 4];
            let op_str = op.encode_utf8(&mut char_buf);
            f_val_elem(buf, "m:chr", op_str);
            f_val_elem(buf, "m:limLoc", lim_loc);
            if sub.is_none() {
                f_val_elem(buf, "m:subHide", "1");
            }
            if sup.is_none() {
                f_val_elem(buf, "m:supHide", "1");
            }
            f_end(buf, "m:naryPr");
            f_start(buf, "m:sub");
            if sub.is_some() {
                write_node_cf(buf, arena, sub);
            }
            f_end(buf, "m:sub");
            f_start(buf, "m:sup");
            if sup.is_some() {
                write_node_cf(buf, arena, sup);
            }
            f_end(buf, "m:sup");
            f_e(buf, arena, body);
            f_end(buf, "m:nary");
        },

        CompactNode::Accent { base, accent, .. } => {
            f_start(buf, "m:acc");
            f_start(buf, "m:accPr");
            let mut char_buf = [0u8; 4];
            let acc_str = accent.encode_utf8(&mut char_buf);
            f_val_elem(buf, "m:chr", acc_str);
            f_end(buf, "m:accPr");
            f_e(buf, arena, base);
            f_end(buf, "m:acc");
        },

        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            ..
        } => {
            let cell_ids = arena.children(cells);
            let nc = num_cols as usize;
            f_start(buf, "m:m");
            f_matrix_props(buf, arena, col_aligns);
            for chunk in cell_ids.chunks(nc.max(1)) {
                f_start(buf, "m:mr");
                for &cell_id in chunk {
                    f_start(buf, "m:e");
                    match *arena.node(cell_id) {
                        CompactNode::Row(span) => {
                            for &child_id in arena.children(span) {
                                write_node_cf(buf, arena, child_id);
                            }
                        },
                        _ => write_node_cf(buf, arena, cell_id),
                    }
                    f_end(buf, "m:e");
                }
                f_end(buf, "m:mr");
            }
            f_end(buf, "m:m");
        },

        CompactNode::Phantom(inner, kind) => {
            f_start(buf, "m:phant");
            f_write_phantom_pr(buf, kind);
            f_e(buf, arena, inner);
            f_end(buf, "m:phant");
        },

        CompactNode::Boxed { content: inner, .. } => {
            f_start(buf, "m:borderBox");
            f_e(buf, arena, inner);
            f_end(buf, "m:borderBox");
        },

        CompactNode::Cancel { stroke, content } => {
            f_start(buf, "m:borderBox");
            f_start(buf, "m:borderBoxPr");
            f_cancel_strike_props(buf, stroke);
            f_end(buf, "m:borderBoxPr");
            f_e(buf, arena, content);
            f_end(buf, "m:borderBox");
        },

        CompactNode::Scaled { content, .. } | CompactNode::RaiseBox { content, .. } => {
            write_node_cf(buf, arena, content)
        },

        CompactNode::Styled(style, span) => {
            let (sty, scr) = decompose_style(style);
            for &child_id in arena.children(span) {
                match *arena.node(child_id) {
                    CompactNode::Identifier(sid) | CompactNode::Number(sid) => {
                        f_styled_run(buf, arena.str_val(sid), sty, scr);
                    },
                    CompactNode::Text(_, sid) => {
                        f_styled_run(buf, arena.str_val(sid), sty, scr);
                    },
                    _ => write_node_cf(buf, arena, child_id),
                }
            }
        },

        CompactNode::Color { content, .. } => write_node_cf(buf, arena, content),

        CompactNode::MathChoice { display, .. } => write_node_cf(buf, arena, display),

        CompactNode::MathAtom { content, .. } => write_node_cf(buf, arena, content),

        CompactNode::Rule { .. } => f_run(buf, "\u{00A0}", None),

        CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_)
        | CompactNode::Newline => {},
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

    fn write_compact(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let compact = CompactExpr::from_math_expr(&expr);
        let config = Config::default();
        OmmlWriter::write_compact(&compact, &config).unwrap()
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
    fn fenced_middle_writes_separator_property() {
        let output = write_compact(MathNode::Fenced {
            open: "[".into(),
            close: "]".into(),
            content: vec![
                FencedItem::Element(MathNode::Identifier("a".into())),
                FencedItem::Middle(";".into()),
                FencedItem::Element(MathNode::Identifier("b".into())),
            ],
        });
        assert!(output.contains("<m:sepChr m:val=\";\"/>"), "{output}");
        assert_eq!(output.matches("<m:e>").count(), 2, "{output}");
    }

    #[test]
    fn special_delimiter_attributes_are_escaped_in_both_writers() {
        let node = MathNode::Fenced {
            open: "<&".into(),
            close: "\"&".into(),
            content: vec![
                FencedItem::Element(MathNode::Identifier("a".into())),
                FencedItem::Middle("&\"<".into()),
                FencedItem::Element(MathNode::Identifier("b".into())),
            ],
        };
        for output in [write(node.clone()), write_compact(node)] {
            assert!(
                output.contains("<m:begChr m:val=\"&lt;&amp;\"/>"),
                "{output}"
            );
            assert!(
                output.contains("<m:endChr m:val=\"&quot;&amp;\"/>"),
                "{output}"
            );
            assert!(
                output.contains("<m:sepChr m:val=\"&amp;&quot;&lt;\"/>"),
                "{output}"
            );
        }
    }

    #[test]
    fn compact_rule_matches_noncompact_placeholder_output() {
        let node = MathNode::Rule {
            width: Rational::new(1, 1),
            height: Rational::new(1, 1),
            lift: Rational::zero(),
        };
        let noncompact = write(node.clone());
        let compact = write_compact(node);
        assert!(noncompact.contains("<m:t>\u{00A0}</m:t>"), "{noncompact}");
        assert_eq!(compact, noncompact);
    }

    #[test]
    fn array_columns_write_mcs_alignment() {
        let output = write_compact(MathNode::Array {
            columns: vec![ColumnAlign::Left, ColumnAlign::Right],
            rows: vec![vec![
                vec![MathNode::Identifier("a".into())],
                vec![MathNode::Identifier("b".into())],
            ]],
            arraystretch: None,
            add_jot: false,
        });
        assert!(output.contains("<m:mPr>"), "{output}");
        assert!(output.contains("<m:mcs>"), "{output}");
        assert!(output.contains("<m:mcJc m:val=\"left\"/>"), "{output}");
        assert!(output.contains("<m:mcJc m:val=\"right\"/>"), "{output}");
    }

    #[test]
    fn cancel_stroke_variants_write_matching_strikes() {
        let cases = [
            (StrokeKind::Forward, vec!["m:strikeBLTR"]),
            (StrokeKind::Backward, vec!["m:strikeTLBR"]),
            (StrokeKind::Cross, vec!["m:strikeBLTR", "m:strikeTLBR"]),
            (StrokeKind::Horizontal, vec!["m:strikeH"]),
            (StrokeKind::Vertical, vec!["m:strikeV"]),
        ];
        for (stroke, expected_props) in cases {
            let output = write_compact(MathNode::Cancel {
                stroke,
                content: Box::new(MathNode::Identifier("x".into())),
            });
            for prop in expected_props {
                assert!(output.contains(prop), "{stroke:?} missing {prop}: {output}");
            }
        }
    }

    #[test]
    fn accent() {
        let output = write(MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '\u{0302}',
            position: AccentPosition::Above,
            wide: false,
        });
        assert!(output.contains("<m:acc>"));
    }

    #[test]
    fn phantom() {
        let output = write(MathNode::Phantom(
            Box::new(MathNode::Identifier("x".into())),
            PhantomKind::Full,
        ));
        assert!(output.contains("<m:phant>"));
        // Full phantom: invisible (show off), no dimensions zeroed.
        assert!(output.contains("<m:phantPr>"), "{output}");
        assert!(output.contains(r#"<m:show m:val="0"/>"#), "{output}");
        assert!(!output.contains("zeroWid"), "{output}");
    }

    #[test]
    fn phantom_subtypes_emit_phant_pr() {
        let cases = [
            (PhantomKind::Vertical, r#"<m:show m:val="0"/>"#, "zeroWid"),
            (PhantomKind::Horizontal, r#"<m:show m:val="0"/>"#, "zeroAsc"),
            (PhantomKind::SmashBoth, r#"<m:show m:val="1"/>"#, "zeroDesc"),
            (PhantomKind::SmashTop, r#"<m:show m:val="1"/>"#, "zeroAsc"),
            (
                PhantomKind::SmashBottom,
                r#"<m:show m:val="1"/>"#,
                "zeroDesc",
            ),
        ];
        for (kind, show, flag) in cases {
            let output = write(MathNode::Phantom(
                Box::new(MathNode::Identifier("x".into())),
                kind,
            ));
            assert!(output.contains(show), "{kind:?} missing {show}: {output}");
            assert!(output.contains(flag), "{kind:?} missing {flag}: {output}");
        }
    }

    #[test]
    fn boxed() {
        let output = write(MathNode::Boxed {
            content: Box::new(MathNode::Number("1".into())),
            bg_color: None,
            border_color: None,
        });
        assert!(output.contains("<m:borderBox>"));
    }
}
