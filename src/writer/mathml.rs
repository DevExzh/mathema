use crate::ast::style::MathStyle;
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::borrow::Cow;
use std::io::Cursor;

use crate::ast::arena::{AstArena, RuleDashes};
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::*;
use crate::error::{Config, Diagnostic, Result};
use crate::unicode::operators::{OperatorForm, lookup_operator_any, symbol_kind_to_form};
use crate::writer::Writer as MathWriter;

fn boundary_attr(boundaries: &[SpanBoundary]) -> Option<String> {
    if boundaries.is_empty() {
        return None;
    }
    let mut value = String::new();
    for (index, boundary) in boundaries.iter().enumerate() {
        if index > 0 {
            value.push('|');
        }
        value.push_str(match boundary {
            SpanBoundary::SuppressGlue => "padding",
            SpanBoundary::Rule => "rule",
        });
    }
    Some(value)
}

fn packed_boundary_attr(boundaries: &str) -> Option<String> {
    if boundaries.is_empty() {
        return None;
    }
    let mut rest = boundaries;
    let mut value = String::new();
    loop {
        let (name, next) = if let Some(next) = rest.strip_prefix("@{}") {
            ("padding", next)
        } else if let Some(next) = rest.strip_prefix('|') {
            ("rule", next)
        } else {
            break;
        };
        if !value.is_empty() {
            value.push('|');
        }
        value.push_str(name);
        rest = next;
    }
    (!value.is_empty()).then_some(value)
}

/// Classifies a compact node that the MathML writer cannot fully represent.
/// MathML preserves `Color`/`Boxed`/`Phantom`/`Cancel`, so only the genuinely
/// lossy kinds are reported.
fn mathml_loss(arena: &AstArena, node: &CompactNode) -> Option<String> {
    match *node {
        // `\big`-family delimiters keep their size and class, as MathJax writes them.
        CompactNode::Scaled { factor, content } if big_delim_fast(arena, factor, content).is_some() => {
            None
        },
        // `\bigl`/`\bigr`/`\bigm` round-trip through MathJax's texclass.
        CompactNode::MathAtom { atom_type, content }
            if big_delim_class(atom_type).is_some()
                && matches!(
                    *arena.node(content),
                    CompactNode::Scaled { factor, content }
                        if big_delim_fast(arena, factor, content).is_some()
                ) =>
        {
            None
        },
        CompactNode::MathChoice { .. } => {
            Some("MathML output keeps only the applicable branch of \\mathchoice".into())
        },
        CompactNode::MathAtom { atom_type: MathAtomType::Op, .. } => {
            Some("MathML records the compound operator class as an annotation. Native operator spacing can differ.".into())
        },
        CompactNode::MathAtom { atom_type: MathAtomType::Open | MathAtomType::Close, .. } => {
            Some("MathML annotates the TeX fence spacing class. Native operator spacing can differ.".into())
        },
        CompactNode::MathAtom { .. } => {
            Some("MathML output does not preserve the math atom spacing class".into())
        },
        CompactNode::Scaled { .. } => {
            Some("MathML output does not preserve the \\scalebox factor".into())
        },
        CompactNode::RaiseBox { .. } => {
            Some("MathML output does not preserve the \\raisebox shift".into())
        },
        _ => None,
    }
}

pub struct MathmlWriter;

impl MathWriter for MathmlWriter {
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut writer = MathOutput::new(Writer::new(Cursor::new(Vec::with_capacity(512))));

        let mut math = BytesStart::new("math");
        math.push_attribute(("xmlns", "http://www.w3.org/1998/Math/MathML"));
        math.push_attribute((
            "display",
            if expr.display == DisplayStyle::Block {
                "block"
            } else {
                "inline"
            },
        ));
        writer.write_event(Event::Start(math)).map_err(to_err)?;

        write_nodes(&mut writer, &expr.nodes, expr.display.into(), false)?;

        writer
            .write_event(Event::End(BytesEnd::new("math")))
            .map_err(to_err)?;

        let buf = writer.target.into_inner().into_inner();
        String::from_utf8(buf).map_err(|e| {
            crate::error::Error::Unsupported(format!("MathML output contained invalid UTF-8: {e}"))
        })
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let out = Self::write_compact(expr, config)?;
        let diags = super::collect_loss_diagnostics(expr, mathml_loss);
        Ok((out, diags))
    }

    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let arena = &expr.arena;
        let mut buf = MathOutput::new(String::with_capacity(512));

        if expr.display == DisplayStyle::Block {
            buf.push_str("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">");
        } else {
            buf.push_str("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"inline\">");
        }

        write_nodes_fast(
            &mut buf,
            arena,
            arena.children(expr.roots),
            expr.display.into(),
            false,
        );

        buf.push_str("</math>");
        Ok(buf.target)
    }
}

/// MathML Core needs the upright variant on individual identifiers. Keep
/// the operator-name font in scope through fractions, accents and scripts.
struct MathOutput<T> {
    target: T,
    operator_font: Option<TextStyle>,
}

impl<T> MathOutput<T> {
    fn new(target: T) -> Self {
        Self {
            target,
            operator_font: None,
        }
    }
    fn with_operator_font<R>(
        &mut self,
        font: Option<TextStyle>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.operator_font, font);
        let result = f(self);
        self.operator_font = previous;
        result
    }
}

impl<T> std::ops::Deref for MathOutput<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.target
    }
}

impl<T> std::ops::DerefMut for MathOutput<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.target
    }
}

type XmlWriter = MathOutput<Writer<Cursor<Vec<u8>>>>;
type XmlBuffer = MathOutput<String>;

fn to_err(e: std::io::Error) -> crate::error::Error {
    crate::error::Error::Io(e)
}

fn math_style_attrs(style: MathStyle) -> (&'static str, &'static str) {
    let depth = match style.size_class() {
        0 => "0",
        1 => "1",
        _ => "2",
    };
    (if style.is_display() { "true" } else { "false" }, depth)
}

fn start_math_style(w: &mut XmlWriter, style: MathStyle) -> Result<()> {
    open_math_style(w, math_style_attrs(style))
}

fn open_math_style(w: &mut XmlWriter, (display, depth): (&str, &str)) -> Result<()> {
    let mut tag = BytesStart::new("mstyle");
    tag.push_attribute(("displaystyle", display));
    tag.push_attribute(("scriptlevel", depth));
    w.write_event(Event::Start(tag)).map_err(to_err)?;
    Ok(())
}

fn start_math_style_fast(buf: &mut XmlBuffer, style: MathStyle) {
    open_math_style_fast(buf, math_style_attrs(style));
}

fn open_math_style_fast(buf: &mut XmlBuffer, (display, depth): (&str, &str)) {
    buf.push_str("<mstyle displaystyle=\"");
    buf.push_str(display);
    buf.push_str("\" scriptlevel=\"");
    buf.push_str(depth);
    buf.push_str("\">");
}

fn marker_attrs(marker: u8) -> (&'static str, &'static str) {
    let (display, level) = math_style_attrs(MathStyle::from_switch(marker));
    (
        if MathStyle::marker_is_display(marker) {
            "true"
        } else {
            display
        },
        level,
    )
}

fn start_math_style_marker(w: &mut XmlWriter, marker: u8) -> Result<MathStyle> {
    open_math_style(w, marker_attrs(marker))?;
    Ok(MathStyle::branch_from_switch(marker))
}

fn start_math_style_marker_fast(buf: &mut XmlBuffer, marker: u8) -> MathStyle {
    open_math_style_fast(buf, marker_attrs(marker));
    MathStyle::branch_from_switch(marker)
}

/// If every cell starts in the same style, a nested table can keep its
/// inherited size. Aligned environments retain display layout at that size.
fn inherited_table_marker(
    style: MathStyle,
    add_jot: bool,
    all_start_with: impl Fn(u8) -> bool,
) -> Option<u8> {
    let depth = style.size_class();
    if depth == 0 {
        return None;
    }
    if add_jot && all_start_with(0) {
        return Some(0);
    }
    let display_marker = if depth == 1 {
        MathStyle::DISPLAY_AT_SCRIPT
    } else {
        MathStyle::DISPLAY_AT_SCRIPTSCRIPT
    };
    if all_start_with(display_marker) {
        return Some(display_marker);
    }
    let marker = (depth + 1) as u8;
    all_start_with(marker).then_some(marker)
}

/// Consecutive declarations replace one another and a trailing declaration
/// contributes no content. Keep the runs borrowed from their original row.
fn style_runs<T>(
    mut nodes: &[T],
    switch: impl Fn(&T) -> Option<u8>,
) -> impl Iterator<Item = (Option<u8>, &[T])> {
    let mut style = None;
    std::iter::from_fn(move || {
        while let Some((node, rest)) = nodes.split_first()
            && let Some(value) = switch(node)
        {
            style = Some(value);
            nodes = rest;
        }
        let (_, tail) = nodes.split_first()?;
        let n = 1 + tail
            .iter()
            .position(|node| switch(node).is_some())
            .unwrap_or(tail.len());
        let (run, rest) = nodes.split_at(n);
        nodes = rest;
        Some((style, run))
    })
}

/// A single styled run supplies its own grouping via mstyle. All other
/// explicit rows still need mrow, especially in fixed-arity elements.
fn needs_mrow<T>(nodes: &[T], switch: impl Fn(&T) -> Option<u8>) -> bool {
    let mut iter = nodes.iter();
    if !iter.next().is_some_and(|node| switch(node).is_some()) {
        return true;
    }
    if !iter.any(|node| switch(node).is_none()) {
        return true;
    }
    iter.any(|node| switch(node).is_some())
}

fn write_sequence<T>(
    w: &mut XmlWriter,
    nodes: &[T],
    inherited: MathStyle,
    explicit_row: bool,
    switch: impl Fn(&T) -> Option<u8>,
    emit: impl Fn(&mut XmlWriter, &T, MathStyle) -> Result<()>,
) -> Result<()> {
    let grouped = explicit_row && needs_mrow(nodes, &switch);
    if grouped {
        w.write_event(Event::Start(BytesStart::new("mrow")))
            .map_err(to_err)?;
    }
    for (style, run) in style_runs(nodes, switch) {
        let active_style = if let Some(marker) = style {
            start_math_style_marker(w, marker)?
        } else {
            inherited
        };
        for node in run {
            emit(w, node, active_style)?;
        }
        if style.is_some() {
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
        }
    }
    if grouped {
        w.write_event(Event::End(BytesEnd::new("mrow")))
            .map_err(to_err)?;
    }
    Ok(())
}

fn write_nodes(
    w: &mut XmlWriter,
    nodes: &[MathNode],
    inherited: MathStyle,
    explicit_row: bool,
) -> Result<()> {
    write_sequence(
        w,
        nodes,
        inherited,
        explicit_row,
        |node| {
            if let MathNode::StyleSwitch(value) = node {
                Some(*value)
            } else {
                None
            }
        },
        write_node,
    )
}

fn write_nodes_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
) {
    write_style_sequence_fast(buf, arena, nodes, inherited, explicit_row, write_node_fast);
}

fn write_style_sequence_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
    emit: impl Fn(&mut XmlBuffer, &AstArena, NodeId, MathStyle),
) {
    let switch = |&id: &NodeId| {
        if let CompactNode::StyleSwitch(value) = *arena.node(id) {
            Some(value)
        } else {
            None
        }
    };
    let grouped = explicit_row && needs_mrow(nodes, switch);
    if grouped {
        open_tag(buf, "mrow");
    }
    for (style, run) in style_runs(nodes, switch) {
        let active_style = if let Some(marker) = style {
            start_math_style_marker_fast(buf, marker)
        } else {
            inherited
        };
        for &id in run {
            emit(buf, arena, id, active_style);
        }
        if style.is_some() {
            close_tag(buf, "mstyle");
        }
    }
    if grouped {
        close_tag(buf, "mrow");
    }
}

fn write_node(w: &mut XmlWriter, node: &MathNode, display_style: MathStyle) -> Result<()> {
    write_node_in(w, node, false, display_style)
}

/// A positional MathML child must occupy exactly one element, even when it
/// contains an operator followed by its operand.
fn write_argument(w: &mut XmlWriter, node: &MathNode, display_style: MathStyle) -> Result<()> {
    write_node_in(w, node, true, display_style)
}

fn write_node_in(
    w: &mut XmlWriter,
    node: &MathNode,
    single_argument: bool,
    display_style: MathStyle,
) -> Result<()> {
    match node {
        MathNode::Number(s) => write_simple_element(w, "mn", s),
        MathNode::Identifier(s) => write_mi(w, s),
        MathNode::Operator(attr) => write_named_operator(w, &attr.name, attr.limit_style, false),
        MathNode::Symbol(kind, s) => write_symbol_node(w, *kind, s),
        MathNode::Text(style, s) => write_text(w, *style, s),
        MathNode::Space(r) => write_mspace(w, *r),
        MathNode::ExplicitSpace { width, basis } => write_mspace(
            w,
            crate::ast::style::local_space_width(*width, Some(*basis), display_style),
        ),
        MathNode::Rule {
            width,
            height,
            lift,
        } => {
            let mut elem = BytesStart::new("mspace");
            let width_attr = format!("{}em", width.as_f64());
            let height_attr = format!("{}em", (lift.as_f64() + height.as_f64()).max(0.0));
            let depth_attr = format!("{}em", (-lift.as_f64()).max(0.0));
            elem.push_attribute(("width", width_attr.as_str()));
            elem.push_attribute(("height", height_attr.as_str()));
            elem.push_attribute(("depth", depth_attr.as_str()));
            w.write_event(Event::Empty(elem)).map_err(to_err)?;
            Ok(())
        },

        MathNode::Row(children) => write_nodes(w, children, display_style, true),

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let elem = BytesStart::new("mrow");
            w.write_event(Event::Start(elem)).map_err(to_err)?;

            // Keep the endpoint role explicit when the glyph pair is
            // ambiguous or one side is null. Ordinary matching delimiters
            // retain their compact historical MathML spelling.
            let endpoint_roles = needs_explicit_endpoint_roles(open, close);
            let stretch_endpoints = endpoint_roles
                || needs_stretchy_endpoints(open, close)
                || fenced_content_needs_stretch(content);
            write_mo_fence(
                w,
                open,
                endpoint_roles.then_some("prefix"),
                stretch_endpoints,
            )?;

            // Content
            for segment in content.split_inclusive(|item| matches!(item, FencedItem::Middle(_))) {
                let (elements, middle) = match segment.split_last() {
                    Some((FencedItem::Middle(delim), prefix)) => (prefix, Some(delim)),
                    _ => (segment, None),
                };
                write_sequence(
                    w,
                    elements,
                    display_style,
                    false,
                    |item| match item {
                        FencedItem::Element(MathNode::StyleSwitch(value)) => Some(*value),
                        _ => None,
                    },
                    |w, item, style| match item {
                        FencedItem::Element(MathNode::Symbol(SymbolKind::Fence, text))
                            if is_literal_fence_glyph(text) =>
                        {
                            write_literal_fence(w, text)
                        },
                        FencedItem::Element(node) => write_node(w, node, style),
                        FencedItem::Middle(_) => Ok(()),
                    },
                )?;
                if let Some(delim) = middle {
                    write_mo_middle(w, delim)?;
                }
            }

            // Closing delimiter
            write_mo_fence(
                w,
                close,
                endpoint_roles.then_some("postfix"),
                stretch_endpoints,
            )?;

            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Subscript { base, sub } => {
            write_scripts(w, base, Some(sub), None, None, display_style)
        },
        MathNode::Superscript { base, sup } => {
            write_scripts(w, base, None, Some(sup), None, display_style)
        },
        MathNode::SubSup { base, sub, sup } => {
            write_scripts(w, base, Some(sub), Some(sup), None, display_style)
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            w.write_event(Event::Start(BytesStart::new("mmultiscripts")))
                .map_err(to_err)?;
            write_argument(w, base, display_style)?;
            w.write_event(Event::Empty(BytesStart::new("mprescripts")))
                .map_err(to_err)?;
            match pre_sub {
                Some(s) => write_argument(w, s, display_style.sup())?,
                None => {
                    w.write_event(Event::Empty(BytesStart::new("none")))
                        .map_err(to_err)?;
                },
            }
            match pre_sup {
                Some(s) => write_argument(w, s, display_style.sup())?,
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
        } => write_scripts(
            w,
            base,
            None,
            Some(over),
            Some(*movable_limits),
            display_style,
        ),
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => write_scripts(
            w,
            base,
            Some(under),
            None,
            Some(*movable_limits),
            display_style,
        ),
        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => write_scripts(
            w,
            base,
            Some(under),
            Some(over),
            Some(*movable_limits),
            display_style,
        ),

        MathNode::Fraction { kind, num, den } => {
            let forced = kind.forced_style();
            if let Some(style) = forced {
                start_math_style(w, style)?;
            }
            let style = forced.unwrap_or(display_style);
            let mut elem = BytesStart::new("mfrac");
            if *kind == FractionKind::NoBar {
                elem.push_attribute(("linethickness", "0"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_argument(w, num, style.fracnum())?;
            write_argument(w, den, style.fracden())?;
            w.write_event(Event::End(BytesEnd::new("mfrac")))
                .map_err(to_err)?;
            if forced.is_some() {
                w.write_event(Event::End(BytesEnd::new("mstyle")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::Sqrt(inner) => {
            w.write_event(Event::Start(BytesStart::new("msqrt")))
                .map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("msqrt")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Root { index, radicand } => {
            w.write_event(Event::Start(BytesStart::new("mroot")))
                .map_err(to_err)?;
            write_argument(w, radicand, display_style)?;
            write_argument(w, index, MathStyle::ScriptScript)?;
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
            let has_body = !is_empty_math_node(body);
            let group = single_argument && has_body;
            if group {
                w.write_event(Event::Start(BytesStart::new("mrow")))
                    .map_err(to_err)?;
            }
            if let Some((tag, movable)) =
                nary_script_tag(*op, *limits, sub.is_some(), sup.is_some())
            {
                w.write_event(Event::Start(BytesStart::new(tag)))
                    .map_err(to_err)?;
                write_mo_char(w, *op, Some(movable))?;
                if let Some(lower) = sub {
                    write_argument(w, lower, display_style.sub())?;
                }
                if let Some(upper) = sup {
                    write_argument(w, upper, display_style.sup())?;
                }
                w.write_event(Event::End(BytesEnd::new(tag)))
                    .map_err(to_err)?;
            } else {
                write_mo_char(w, *op, None)?;
            }
            if has_body {
                write_node(w, body, display_style)?;
            }
            if group {
                w.write_event(Event::End(BytesEnd::new("mrow")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::Accent {
            base,
            accent,
            position,
            wide,
        } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            let mut elem = BytesStart::new(tag);
            match position {
                AccentPosition::Above => elem.push_attribute(("accent", "true")),
                AccentPosition::Below => elem.push_attribute(("accentunder", "true")),
            }
            if *accent == '\u{0304}' {
                elem.push_attribute(("data-mathema-accent-decoration", "combining-macron"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_argument(w, base, display_style)?;
            write_accent_char(w, mathml_accent_glyph(*accent), *wide)?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Array { .. } | MathNode::Matrix { .. } | MathNode::NativeArray { .. } => {
            let (columns, rows, arraystretch, add_jot, array_style, col_seps, row_seps, kind) =
                match node {
                    MathNode::Array {
                        columns,
                        rows,
                        arraystretch,
                        add_jot,
                    } => (
                        Cow::Borrowed(columns.as_slice()),
                        rows,
                        *arraystretch,
                        *add_jot,
                        false,
                        0,
                        0,
                        None,
                    ),
                    MathNode::Matrix {
                        kind,
                        alignment,
                        rows,
                    } => (
                        Cow::Owned(vec![*alignment; table_width(rows, 1)?]),
                        rows,
                        None,
                        false,
                        false,
                        0,
                        0,
                        Some(*kind),
                    ),
                    MathNode::NativeArray {
                        columns,
                        rows,
                        arraystretch,
                        add_jot,
                        array_style,
                        col_seps,
                        row_seps,
                        ..
                    } => (
                        Cow::Borrowed(columns.as_slice()),
                        rows,
                        *arraystretch,
                        *add_jot,
                        *array_style,
                        *col_seps,
                        *row_seps,
                        None,
                    ),
                    _ => unreachable!(),
                };
            let promoted = inherited_table_marker(display_style, add_jot, |marker| {
                rows.iter().flatten().all(|cell| {
                    table_cell_parts(cell).1.first().is_none_or(
                        |node| matches!(node, MathNode::StyleSwitch(value) if *value == marker),
                    )
                })
            });
            let reset_to_text = display_style.size_class() > 0 && promoted.is_none();
            let cell_style = promoted.map_or(MathStyle::Text, MathStyle::branch_from_switch);
            if reset_to_text {
                start_math_style(w, MathStyle::Text)?;
            }
            let mut table = BytesStart::new("mtable");
            for &(name, value) in matrix_attributes(kind) {
                table.push_attribute((name, value));
            }
            let pct = arraystretch
                .map(|value| (value * 100.0).round() as u8)
                .unwrap_or(if kind == Some(MatrixKind::SmallMatrix) {
                    50
                } else {
                    100
                });
            if pct != 100 {
                table.push_attribute(("data-mathema-arraystretch", format!("{pct}%").as_str()));
            }
            if promoted.is_some_and(|marker| MathStyle::branch_from_switch(marker).is_display()) {
                table.push_attribute(("displaystyle", "true"));
            }
            let num_cols = if kind.is_some() {
                columns.len()
            } else {
                table_width(rows, columns.len())?
            };
            let dashes = match node {
                MathNode::NativeArray {
                    dashed_col_seps,
                    dashed_row_seps,
                    ..
                } => RuleDashes {
                    cols: dashed_col_seps & col_seps,
                    rows: dashed_row_seps & row_seps,
                },
                _ => RuleDashes::default(),
            };
            table_rule_attributes(
                col_seps,
                row_seps,
                dashes,
                num_cols,
                rows.len(),
                (array_style
                    || (matches!(node, MathNode::NativeArray { .. })
                        && col_seps == 0
                        && row_seps == 0))
                    .then_some(array_style),
                |key, value| {
                    table.push_attribute((key, value));
                },
            );
            w.write_event(Event::Start(table)).map_err(to_err)?;
            for row in rows {
                w.write_event(Event::Start(BytesStart::new("mtr")))
                    .map_err(to_err)?;
                let mut authored = row.iter();
                let mut i = 0;
                while i < num_cols {
                    let cell = authored.next().map(Vec::as_slice).unwrap_or_default();
                    let cell_span = table_cell_span(cell);
                    let (left_boundaries, right_boundaries) = match cell {
                        [
                            MathNode::SpannedCell {
                                left_boundaries,
                                right_boundaries,
                                ..
                            },
                        ] => (left_boundaries.as_slice(), right_boundaries.as_slice()),
                        _ => (&[][..], &[][..]),
                    };
                    let (cell_override, cell) = table_cell_parts(cell);
                    let mut td = BytesStart::new("mtd");
                    if let Some(align) = cell_override.or_else(|| columns.get(i).copied()) {
                        let val = match align {
                            ColumnAlign::Left => "left",
                            ColumnAlign::Center => "center",
                            ColumnAlign::Right => "right",
                        };
                        td.push_attribute(("columnalign", val));
                    }
                    if cell_span > 1 {
                        td.push_attribute(("columnspan", cell_span.to_string().as_str()));
                    }
                    if let Some(value) = boundary_attr(left_boundaries) {
                        td.push_attribute(("data-mathema-left-boundaries", value.as_str()));
                    }
                    if let Some(value) = boundary_attr(right_boundaries) {
                        td.push_attribute(("data-mathema-right-boundaries", value.as_str()));
                    }
                    if left_boundaries.contains(&SpanBoundary::Rule) {
                        td.push_attribute(("data-mathema-left-rule", "true"));
                    }
                    if right_boundaries.contains(&SpanBoundary::Rule) {
                        td.push_attribute(("data-mathema-right-rule", "true"));
                    }
                    if left_boundaries.contains(&SpanBoundary::SuppressGlue) {
                        td.push_attribute(("data-mathema-left-padding", "true"));
                    }
                    if right_boundaries.contains(&SpanBoundary::SuppressGlue) {
                        td.push_attribute(("data-mathema-right-padding", "true"));
                    }
                    w.write_event(Event::Start(td)).map_err(to_err)?;
                    let cell = if promoted.is_some() && !cell.is_empty() {
                        &cell[1..]
                    } else {
                        cell
                    };
                    let small = promoted.is_none()
                        && kind == Some(MatrixKind::SmallMatrix)
                        && cell
                            .first()
                            .is_some_and(|n| !matches!(n, MathNode::StyleSwitch(_)));
                    if small {
                        start_math_style(w, MathStyle::Script)?;
                        write_nodes(w, cell, MathStyle::Script, false)?;
                        w.write_event(Event::End(BytesEnd::new("mstyle")))
                            .map_err(to_err)?;
                    } else {
                        write_nodes(w, cell, cell_style, false)?;
                    }
                    w.write_event(Event::End(BytesEnd::new("mtd")))
                        .map_err(to_err)?;
                    i = i.saturating_add(cell_span);
                }
                w.write_event(Event::End(BytesEnd::new("mtr")))
                    .map_err(to_err)?;
            }
            w.write_event(Event::End(BytesEnd::new("mtable")))
                .map_err(to_err)?;
            if reset_to_text {
                w.write_event(Event::End(BytesEnd::new("mstyle")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            write_node_in(w, content, single_argument, display_style)
        },
        MathNode::Phantom(inner, _) => {
            w.write_event(Event::Start(BytesStart::new("mphantom")))
                .map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mphantom")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            let mut elem = BytesStart::new("menclose");
            elem.push_attribute(("notation", "box"));
            if let Some(bg) = bg_color {
                let mut style = String::new();
                if let Some(bc) = border_color {
                    style.push_str("border-color: ");
                    style.push_str(bc);
                    style.push_str("; ");
                }
                style.push_str("background-color: ");
                style.push_str(bg);
                elem.push_attribute(("style", style.as_str()));
            } else if let Some(bc) = border_color {
                let style = format!("border-color: {bc}");
                elem.push_attribute(("style", style.as_str()));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Actuarial {
            content: inner,
            text_mode,
        } => {
            let mut elem = BytesStart::new("menclose");
            elem.push_attribute(("notation", "actuarial"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_box_child(w, inner, *text_mode, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Reflect { content, text_mode } => {
            let mut elem = BytesStart::new("mrow");
            elem.push_attribute((
                "data-mathema-reflect",
                if *text_mode { "text" } else { "math" },
            ));
            elem.push_attribute(("style", "display:inline-block; transform:scaleX(-1)"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_box_child(w, content, *text_mode, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mrow")))
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
            write_node(w, content, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Scaled { factor, content } => {
            if let Some(delim) = big_delim(*factor, content) {
                write_big_delim(w, "ORD", delim)
            } else {
                write_node_in(w, content, single_argument, display_style)
            }
        },
        MathNode::RaiseBox { content, .. } => {
            write_node_in(w, content, single_argument, display_style)
        },

        MathNode::Styled(style, children) => {
            let variant = style_to_mathvariant(*style);
            let mut elem = BytesStart::new("mstyle");
            elem.push_attribute(("mathvariant", variant));
            if w.operator_font.is_some()
                && let Some(css) = scoped_font_css(*style)
            {
                elem.push_attribute(("style", css));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            let font = w.operator_font.map(|_| *style);
            w.with_operator_font(font, |w| write_nodes(w, children, display_style, false))?;
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Color { color, content } => {
            let mut elem = BytesStart::new("mstyle");
            elem.push_attribute(("mathcolor", color.as_str()));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node_in(w, content, single_argument, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let branch = display_style.choose(display, text, script, script_script);
            write_node_in(w, branch, single_argument, display_style)
        },
        MathNode::StyleSwitch(_) => {
            if single_argument {
                w.write_event(Event::Empty(BytesStart::new("mrow")))
                    .map_err(to_err)?;
            }
            Ok(())
        },
        MathNode::MathAtom { atom_type, content } => {
            if let Some(class) = big_delim_class(*atom_type)
                && let MathNode::Scaled {
                    factor,
                    content: inner,
                } = content.as_ref()
                && let Some(delim) = big_delim(*factor, inner)
            {
                write_big_delim(w, class, delim)
            } else if let Some(class) = annotated_atom_class(*atom_type) {
                write_atom_group(w, content, class, false, display_style)
            } else if *atom_type == MathAtomType::Ord
                && let MathNode::Symbol(_, s) = content.as_ref()
                && marks_ordinary_operator(SymbolKind::Ordinary, s)
            {
                write_symbol_node(w, SymbolKind::Ordinary, s)
            } else if let Some(kind) = spacing_atom_kind(*atom_type)
                && let MathNode::Symbol(SymbolKind::Ordinary, s) = content.as_ref()
                && marks_ordinary_operator(SymbolKind::Ordinary, s)
            {
                write_symbol_node(w, kind, s)
            } else {
                write_node_in(w, content, single_argument, display_style)
            }
        },
        MathNode::StructuredOperator { content, form } => {
            write_structured_operator_group(w, content, *form, false, display_style)
        },
    }
}

/// Whether an ordinary `<mo>` needs `data-mjx-texclass="ORD"` so the reader
/// does not give it the glyph's default operator class (`\mathord{-}`,
/// `\mathord{\lneqq}`, `\mathord{\llbracket}`).
fn marks_ordinary_operator(kind: SymbolKind, text: &str) -> bool {
    if kind != SymbolKind::Ordinary {
        return false;
    }
    text == "'" || crate::reader::mathml::ordinary_mo_needs_marker(text)
}

/// `\mathbin`/`\mathrel` around an ordinary operator glyph (`\mathbin{\dag}`)
/// is written as a plain `<mo>` rather than the ordinary marker.
fn spacing_atom_kind(atom: MathAtomType) -> Option<SymbolKind> {
    match atom {
        MathAtomType::Bin => Some(SymbolKind::BinaryOp),
        MathAtomType::Rel => Some(SymbolKind::Relation),
        _ => None,
    }
}

fn write_symbol_node(w: &mut XmlWriter, kind: SymbolKind, text: &str) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    let ordinary_literal_bar = kind == SymbolKind::Ordinary && matches!(text, "|" | "‖");
    match kind {
        SymbolKind::LargeOp => elem.push_attribute(("largeop", "true")),
        SymbolKind::Fence => elem.push_attribute(("fence", "true")),
        _ => {},
    }
    if ordinary_literal_bar {
        elem.push_attribute(("fence", "false"));
        elem.push_attribute(("stretchy", "false"));
    } else if let Some(ch) = text.chars().next() {
        let form = symbol_kind_to_form(kind);
        if let Some(entry) = lookup_operator_any(ch, form) {
            if entry.stretchy {
                elem.push_attribute(("stretchy", "true"));
            }
            if entry.form != OperatorForm::Infix {
                elem.push_attribute(("form", crate::unicode::operators::form_str(entry.form)));
            }
        }
    }
    if marks_ordinary_operator(kind, text) {
        elem.push_attribute(("data-mjx-texclass", "ORD"));
        elem.push_attribute(("lspace", "0em"));
        elem.push_attribute(("rspace", "0em"));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

fn write_box_child(
    w: &mut XmlWriter,
    child: &MathNode,
    text_size: bool,
    display_style: MathStyle,
) -> Result<()> {
    if text_size {
        let mut style = BytesStart::new("mstyle");
        style.push_attribute(("displaystyle", "false"));
        style.push_attribute(("scriptlevel", "0"));
        w.write_event(Event::Start(style)).map_err(to_err)?;
    }
    write_node(
        w,
        child,
        if text_size {
            MathStyle::Text
        } else {
            display_style
        },
    )?;
    if text_size {
        w.write_event(Event::End(BytesEnd::new("mstyle")))
            .map_err(to_err)?;
    }
    Ok(())
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

fn write_text(w: &mut XmlWriter, style: TextStyle, text: &str) -> Result<()> {
    let text = canonical_mathml_text(text);
    let mut elem = BytesStart::new("mtext");
    if style != TextStyle::Normal {
        elem.push_attribute(("mathvariant", style_to_mathvariant(style)));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text.as_ref())))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mtext")))
        .map_err(to_err)?;
    Ok(())
}

/// CSS width is nonnegative. A logical start margin gives a negative TeX
/// space its signed advance in a MathML layout.
fn write_mspace(w: &mut XmlWriter, width: Rational) -> Result<()> {
    let mut tag = BytesStart::new("mspace");
    let mut value = String::new();
    if width.num < 0 {
        tag.push_attribute(("width", "0em"));
        value.push_str("margin-inline-start:");
    }
    width.write_scaled_decimal(&mut value, 1);
    value.push_str("em");
    tag.push_attribute((
        if width.num < 0 { "style" } else { "width" },
        value.as_str(),
    ));
    w.write_event(Event::Empty(tag)).map_err(to_err)
}

fn identifier_variant(s: &str, font: Option<TextStyle>) -> Option<&'static str> {
    font.map(style_to_mathvariant)
        .or_else(|| (s.len() > 1 && s.bytes().all(|b| b.is_ascii_alphabetic())).then_some("normal"))
}

fn write_mi(w: &mut XmlWriter, s: &str) -> Result<()> {
    let mut elem = BytesStart::new("mi");
    if let Some(variant) = identifier_variant(s, w.operator_font) {
        elem.push_attribute(("mathvariant", variant));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(s)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mi")))
        .map_err(to_err)?;
    Ok(())
}

/// Write an endpoint of a `Fenced` node with its standard MathML operator
/// form. Empty endpoints are null delimiters, represented by an empty fenced
/// operator so one-sided `\left.` / `\right.` structures survive reimport.
fn write_mo_fence(w: &mut XmlWriter, text: &str, form: Option<&str>, stretchy: bool) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("fence", "true"));
    if let Some(form) = form {
        elem.push_attribute(("form", form));
    }
    if stretchy {
        elem.push_attribute(("stretchy", "true"));
        elem.push_attribute(("symmetric", "true"));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

/// Neutral delimiters already have a middle position here. Other glyphs,
/// including bracket-shaped middles, need their infix role to survive import.
fn middle_form(text: &str) -> Option<&'static str> {
    (!matches!(text, "" | "|" | "‖" | "∣" | "∥" | "/")).then_some("infix")
}

fn write_mo_middle(w: &mut XmlWriter, text: &str) -> Result<()> {
    write_mo_fence(w, text, middle_form(text), true)
}

fn write_literal_fence(w: &mut XmlWriter, text: &str) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("stretchy", "false"));
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn needs_stretchy_endpoints(open: &str, close: &str) -> bool {
    matches!((open, close), ("\\", "/") | ("/", "\\"))
}

fn fenced_content_needs_stretch(content: &[FencedItem]) -> bool {
    content.iter().any(|item| {
        matches!(
            item,
            FencedItem::Element(MathNode::Symbol(SymbolKind::Fence, text))
                if is_literal_fence_glyph(text)
        )
    })
}

fn write_mo_char(w: &mut XmlWriter, ch: char, movable: Option<bool>) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    if let Some(movable) = movable {
        elem.push_attribute(("movablelimits", if movable { "true" } else { "false" }));
    }
    let mut glyph = [0; 4];
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(ch.encode_utf8(&mut glyph))))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn write_accent_char(w: &mut XmlWriter, ch: char, wide: bool) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("stretchy", if wide { "true" } else { "false" }));
    let mut glyph = [0; 4];
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(ch.encode_utf8(&mut glyph))))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

#[inline]
fn mathml_accent_glyph(ch: char) -> char {
    match ch {
        '\u{0304}' => '\u{203E}',
        _ => ch,
    }
}

/// An explicitly movable integral cannot be represented by the default TeX
/// n-ary placement. Its generic under/over node carries the MathML attribute.
fn write_limit_base(
    w: &mut XmlWriter,
    node: &MathNode,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    match node {
        MathNode::Operator(attr) => write_named_operator(
            w,
            &attr.name,
            if movable {
                LimitStyle::Default
            } else {
                LimitStyle::AboveBelow
            },
            true,
        ),
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => write_atom_group(w, content, "OP", movable, display_style),
        MathNode::StructuredOperator { content, form } => {
            write_structured_operator_group(w, content, *form, movable, display_style)
        },
        MathNode::Symbol(SymbolKind::LargeOp, text) if movable => {
            if let Some(ch) = single_operator_char(text) {
                write_mo_char(w, ch, Some(true))
            } else {
                write_argument(w, node, display_style)
            }
        },
        MathNode::Row(children) if children.len() == 1 => {
            w.write_event(Event::Start(BytesStart::new("mrow")))
                .map_err(to_err)?;
            write_limit_base(w, &children[0], movable, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)
        },
        _ => write_argument(w, node, display_style),
    }
}

/// A `\big`-family delimiter: its MathJax size and its glyph.
struct BigDelim<'a> {
    size: &'static str,
    glyph: &'a str,
}

/// The `minsize`/`maxsize` MathJax writes for a `\big`-family scale factor.
fn big_delim_size(factor: Rational) -> Option<&'static str> {
    BIG_DELIM_SIZES
        .iter()
        .find(|big| big.scale == factor)
        .map(|big| big.mathjax)
}

fn big_delim(factor: Rational, content: &MathNode) -> Option<BigDelim<'_>> {
    let MathNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, glyph) = content
    else {
        return None;
    };
    Some(BigDelim {
        size: big_delim_size(factor)?,
        glyph,
    })
}

fn big_delim_fast(arena: &AstArena, factor: Rational, content: NodeId) -> Option<BigDelim<'_>> {
    let CompactNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, sid) =
        *arena.node(content)
    else {
        return None;
    };
    Some(BigDelim {
        size: big_delim_size(factor)?,
        glyph: arena.str_val(sid),
    })
}

/// MathJax's TeX class for `\bigl`, `\bigr` and `\bigm`.
fn big_delim_class(atom: MathAtomType) -> Option<&'static str> {
    match atom {
        MathAtomType::Open => Some("OPEN"),
        MathAtomType::Close => Some("CLOSE"),
        MathAtomType::Rel => Some("REL"),
        _ => None,
    }
}

/// Write a sized delimiter as MathJax does: a fixed-size stretchy `<mo>` in
/// an `<mrow>` that records the TeX class of `\big`/`\bigl`/`\bigr`/`\bigm`.
fn write_big_delim(w: &mut XmlWriter, class: &str, delim: BigDelim<'_>) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", class));
    w.write_event(Event::Start(group)).map_err(to_err)?;
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("minsize", delim.size));
    elem.push_attribute(("maxsize", delim.size));
    elem.push_attribute(("fence", "true"));
    elem.push_attribute(("stretchy", "true"));
    elem.push_attribute(("symmetric", "true"));
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(delim.glyph)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn write_big_delim_fast(buf: &mut XmlBuffer, class: &str, delim: BigDelim<'_>) {
    buf.push_str(r#"<mrow data-mjx-texclass=""#);
    buf.push_str(class);
    buf.push_str(r#""><mo minsize=""#);
    buf.push_str(delim.size);
    buf.push_str(r#"" maxsize=""#);
    buf.push_str(delim.size);
    buf.push_str(r#"" fence="true" stretchy="true" symmetric="true">"#);
    xml_escape_into(buf, delim.glyph);
    buf.push_str("</mo>");
    close_tag(buf, "mrow");
}

fn annotated_atom_class(atom: MathAtomType) -> Option<&'static str> {
    match atom {
        MathAtomType::Op => Some("OP"),
        MathAtomType::Open => Some("OPEN"),
        MathAtomType::Close => Some("CLOSE"),
        _ => None,
    }
}

fn write_atom_group(
    w: &mut XmlWriter,
    content: &MathNode,
    class: &str,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", class));
    if movable {
        group.push_attribute(("data-mathema-limits", "movable"));
    }
    w.write_event(Event::Start(group)).map_err(to_err)?;
    if let MathNode::Row(children) = content {
        write_nodes(w, children, display_style, false)?;
    } else {
        write_node(w, content, display_style)?;
    }
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn structured_operator_marker(form: OperatorNameForm) -> &'static str {
    match form {
        OperatorNameForm::Unstarred => "unstarred",
        OperatorNameForm::Starred(LimitStyle::Default) => "starred-default",
        OperatorNameForm::Starred(LimitStyle::AboveBelow) => "starred-limits",
        OperatorNameForm::Starred(LimitStyle::SubSup) => "starred-nolimits",
    }
}

fn write_structured_operator_group(
    w: &mut XmlWriter,
    content: &MathNode,
    form: OperatorNameForm,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", "OP"));
    group.push_attribute((
        "data-mathema-structured-operator",
        structured_operator_marker(form),
    ));
    if movable {
        group.push_attribute(("data-mathema-limits", "movable"));
    }
    w.write_event(Event::Start(group)).map_err(to_err)?;
    w.with_operator_font(Some(TextStyle::Normal), |w| match content {
        MathNode::Row(children) => write_nodes(w, children, display_style, false),
        _ => write_node(w, content, display_style),
    })?;
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn write_named_operator(
    w: &mut XmlWriter,
    name: &str,
    limits: LimitStyle,
    scripted: bool,
) -> Result<()> {
    let mut token = BytesStart::new("mo");
    if let Some(value) = named_movable_attr(limits) {
        token.push_attribute(("movablelimits", value));
    }
    if needs_operator_marker(name, limits, scripted) {
        token.push_attribute(("data-mathema-operator", "name"));
    }
    w.write_event(Event::Start(token)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(name)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

fn single_operator_char(text: &str) -> Option<char> {
    let text = text.trim();
    let ch = text.chars().next()?;
    (text.len() == ch.len_utf8()).then_some(ch)
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

// ── Fast direct-string compact writer (bypasses quick_xml) ───────────

/// Escape XML special characters in text content.
/// Math content rarely contains these, so we fast-path the common case.
#[inline]
fn xml_escape_into(buf: &mut XmlBuffer, s: &str) {
    // Fast path: if no special chars, push the whole string at once
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
fn open_tag(buf: &mut XmlBuffer, tag: &str) {
    buf.push('<');
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn close_tag(buf: &mut XmlBuffer, tag: &str) {
    buf.push_str("</");
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn simple_element(buf: &mut XmlBuffer, tag: &str, text: &str) {
    open_tag(buf, tag);
    xml_escape_into(buf, text);
    close_tag(buf, tag);
}

#[inline]
fn write_text_fast(buf: &mut XmlBuffer, style: TextStyle, text: &str) {
    let text = canonical_mathml_text(text);
    buf.push_str("<mtext");
    if style != TextStyle::Normal {
        buf.push_str(" mathvariant=\"");
        buf.push_str(style_to_mathvariant(style));
        buf.push('"');
    }
    buf.push('>');
    xml_escape_into(buf, text.as_ref());
    buf.push_str("</mtext>");
}

/// Encode AST text in MathML's token whitespace model. XML whitespace is
/// collapsed and trimmed in token content, so leading/trailing spaces and
/// repeated interior spaces use NBSP after the first rendered space. A single
/// ordinary interior space remains ordinary text.
fn canonical_mathml_text(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
        return Cow::Borrowed(text);
    }

    let normalized: Vec<char> = text
        .chars()
        .map(|c| {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < normalized.len() {
        if normalized[index] != ' ' {
            output.push(normalized[index]);
            index += 1;
            continue;
        }

        let start = index;
        while index < normalized.len() && normalized[index] == ' ' {
            index += 1;
        }
        let count = index - start;
        if start == 0 || index == normalized.len() {
            output.extend(std::iter::repeat_n('\u{00A0}', count));
        } else {
            output.push(' ');
            output.extend(std::iter::repeat_n('\u{00A0}', count.saturating_sub(1)));
        }
    }
    Cow::Owned(output)
}

#[inline]
fn write_node_fast(buf: &mut XmlBuffer, arena: &AstArena, id: NodeId, display_style: MathStyle) {
    write_node_fast_in(buf, arena, id, false, display_style);
}

fn write_argument_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    display_style: MathStyle,
) {
    write_node_fast_in(buf, arena, id, true, display_style);
}

fn write_node_fast_in(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    single_argument: bool,
    display_style: MathStyle,
) {
    match *arena.node(id) {
        CompactNode::Number(sid) => simple_element(buf, "mn", arena.str_val(sid)),
        CompactNode::Identifier(sid) => write_mi_fast(buf, arena.str_val(sid)),
        CompactNode::Operator { name, limits } => {
            write_named_operator_fast(buf, arena.str_val(name), limits, false)
        },
        CompactNode::Symbol(kind, sid) => {
            let s = arena.str_val(sid);
            write_mo_symbol_fast(buf, kind, s);
        },
        CompactNode::Text(style, sid) => write_text_fast(buf, style, arena.str_val(sid)),
        CompactNode::Space(r) => {
            let width =
                crate::ast::style::local_space_width(r, arena.space_basis(id), display_style);
            if width.num < 0 {
                buf.push_str("<mspace width=\"0em\" style=\"margin-inline-start:");
            } else {
                buf.push_str("<mspace width=\"");
            }
            width.write_scaled_decimal(buf, 1);
            buf.push_str("em\"/>");
        },

        CompactNode::Row(span) => {
            write_nodes_fast(buf, arena, arena.children(span), display_style, true)
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            buf.push_str("<mrow>");

            let open_s = arena.str_val(open);
            let close_s = arena.str_val(close);
            let endpoint_roles = needs_explicit_endpoint_roles(open_s, close_s);
            let stretch_endpoints = endpoint_roles
                || needs_stretchy_endpoints(open_s, close_s)
                || compact_fenced_content_needs_stretch(arena, content);
            write_mo_fence_fast(
                buf,
                open_s,
                endpoint_roles.then_some("prefix"),
                stretch_endpoints,
            );

            for segment in arena
                .children(content)
                .split_inclusive(|&id| matches!(arena.node(id), CompactNode::Middle(_)))
            {
                if let Some((&last, prefix)) = segment.split_last()
                    && let CompactNode::Middle(sid) = *arena.node(last)
                {
                    write_nodes_fenced_fast(buf, arena, prefix, display_style);
                    write_mo_middle_fast(buf, arena.str_val(sid));
                } else {
                    write_nodes_fenced_fast(buf, arena, segment, display_style);
                }
            }

            write_mo_fence_fast(
                buf,
                close_s,
                endpoint_roles.then_some("postfix"),
                stretch_endpoints,
            );

            buf.push_str("</mrow>");
        },

        CompactNode::Subscript { base, sub } => {
            write_scripts_fast(buf, arena, base, sub, NodeId::NONE, None, display_style)
        },
        CompactNode::Superscript { base, sup } => {
            write_scripts_fast(buf, arena, base, NodeId::NONE, sup, None, display_style)
        },
        CompactNode::SubSup { base, sub, sup } => {
            write_scripts_fast(buf, arena, base, sub, sup, None, display_style)
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            buf.push_str("<mmultiscripts>");
            write_argument_fast(buf, arena, base, display_style);
            buf.push_str("<mprescripts/>");
            if pre_sub.is_some() {
                write_argument_fast(buf, arena, pre_sub, display_style.sub());
            } else {
                buf.push_str("<none/>");
            }
            if pre_sup.is_some() {
                write_argument_fast(buf, arena, pre_sup, display_style.sup());
            } else {
                buf.push_str("<none/>");
            }
            buf.push_str("</mmultiscripts>");
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            NodeId::NONE,
            over,
            Some(movable_limits),
            display_style,
        ),
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            under,
            NodeId::NONE,
            Some(movable_limits),
            display_style,
        ),
        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            under,
            over,
            Some(movable_limits),
            display_style,
        ),

        CompactNode::Fraction { kind, num, den } => {
            let forced = kind.forced_style();
            if let Some(style) = forced {
                start_math_style_fast(buf, style);
            }
            let style = forced.unwrap_or(display_style);
            if kind == FractionKind::NoBar {
                buf.push_str("<mfrac linethickness=\"0\">");
            } else {
                buf.push_str("<mfrac>");
            }
            write_argument_fast(buf, arena, num, style.fracnum());
            write_argument_fast(buf, arena, den, style.fracden());
            buf.push_str("</mfrac>");
            if forced.is_some() {
                close_tag(buf, "mstyle");
            }
        },

        CompactNode::Sqrt(inner) => {
            buf.push_str("<msqrt>");
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</msqrt>");
        },

        CompactNode::Root { index, radicand } => {
            buf.push_str("<mroot>");
            write_argument_fast(buf, arena, radicand, display_style);
            write_argument_fast(buf, arena, index, MathStyle::ScriptScript);
            buf.push_str("</mroot>");
        },

        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let has_body = !is_empty_compact_node(arena, body);
            let group = single_argument && has_body;
            if group {
                open_tag(buf, "mrow");
            }
            if let Some((tag, movable)) = nary_script_tag(op, limits, sub.is_some(), sup.is_some())
            {
                open_tag(buf, tag);
                write_mo_char_fast(buf, op, Some(movable));
                if sub.is_some() {
                    write_argument_fast(buf, arena, sub, display_style.sub());
                }
                if sup.is_some() {
                    write_argument_fast(buf, arena, sup, display_style.sup());
                }
                close_tag(buf, tag);
            } else {
                write_mo_char_fast(buf, op, None);
            }
            if has_body {
                write_node_fast(buf, arena, body, display_style);
            }
            if group {
                close_tag(buf, "mrow");
            }
        },

        CompactNode::Accent {
            base,
            accent,
            position,
            wide,
        } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            buf.push('<');
            buf.push_str(tag);
            match position {
                AccentPosition::Above => buf.push_str(" accent=\"true\""),
                AccentPosition::Below => buf.push_str(" accentunder=\"true\""),
            }
            if accent == '\u{0304}' {
                buf.push_str(r#" data-mathema-accent-decoration="combining-macron""#);
            }
            buf.push('>');
            write_argument_fast(buf, arena, base, display_style);
            let glyph = mathml_accent_glyph(accent);
            buf.push_str("<mo stretchy=\"");
            buf.push_str(if wide { "true" } else { "false" });
            buf.push_str("\">");
            let mut encoded = [0; 4];
            xml_escape_into(buf, glyph.encode_utf8(&mut encoded));
            buf.push_str("</mo>");
            close_tag(buf, tag);
        },

        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            write_node_fast(buf, arena, content, display_style)
        },
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            array_style,
            col_seps,
            row_seps,
        } => {
            let nc = usize::from(num_cols.max(1));
            let environment = arena
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
                .map(|(kind, _)| kind);
            let cell_ids = arena.children(cells);
            let promoted = inherited_table_marker(display_style, add_jot, |marker| {
                cell_ids
                    .iter()
                    .all(|cell| super::cell_after_style(arena, cell, marker).is_some())
            });
            let reset_to_text = display_style.size_class() > 0 && promoted.is_none();
            let cell_style = promoted.map_or(MathStyle::Text, MathStyle::branch_from_switch);

            if reset_to_text {
                start_math_style_fast(buf, MathStyle::Text);
            }

            buf.push_str("<mtable");
            for &(name, value) in matrix_attributes(environment) {
                buf.push(' ');
                buf.push_str(name);
                buf.push_str("=\"");
                buf.push_str(value);
                buf.push('"');
            }
            if arraystretch_pct != 100 {
                buf.push_str(" data-mathema-arraystretch=\"");
                buf.push_str(&arraystretch_pct.to_string());
                buf.push_str("%\"");
            }
            if promoted.is_some_and(|marker| MathStyle::branch_from_switch(marker).is_display()) {
                buf.push_str(" displaystyle=\"true\"");
            }
            table_rule_attributes(
                col_seps,
                row_seps,
                arena.rule_dashes(id),
                nc,
                cell_ids.len().div_ceil(nc.max(1)),
                (array_style || (arena.is_native_array(id) && col_seps == 0 && row_seps == 0))
                    .then_some(array_style),
                |key, value| {
                    buf.push(' ');
                    buf.push_str(key);
                    buf.push_str("=\"");
                    buf.push_str(value);
                    buf.push('"');
                },
            );
            buf.push('>');
            for row_chunk in cell_ids.chunks(nc.max(1)) {
                buf.push_str("<mtr>");
                let mut i = 0;
                while i < nc {
                    let slot = row_chunk.get(i);
                    let align_nodes = arena.children(col_aligns);
                    let (cell_override, mut nodes) = slot
                        .map(|id| arena.table_cell_parts(id))
                        .unwrap_or((None, &[]));
                    let (cell_span, left_boundaries, right_boundaries) =
                        match slot.map(|&id| *arena.node(id)) {
                            Some(CompactNode::SpannedCell {
                                span,
                                left_boundaries,
                                right_boundaries,
                                ..
                            }) => (
                                arena.effective_cell_span_in_row(row_chunk, i, span),
                                left_boundaries,
                                right_boundaries,
                            ),
                            _ => (1, StrId::EMPTY, StrId::EMPTY),
                        };
                    buf.push_str("<mtd");
                    if let Some(a) = cell_override.or_else(|| {
                        align_nodes
                            .get(i)
                            .and_then(|&align_id| match *arena.node(align_id) {
                                CompactNode::ColAlign(a) => Some(a),
                                _ => None,
                            })
                    }) {
                        let val = match a {
                            ColumnAlign::Left => "left",
                            ColumnAlign::Center => "center",
                            ColumnAlign::Right => "right",
                        };
                        buf.push_str(" columnalign=\"");
                        buf.push_str(val);
                        buf.push('"');
                    }
                    if cell_span > 1 {
                        buf.push_str(" columnspan=\"");
                        buf.push_str(&cell_span.to_string());
                        buf.push('"');
                    }
                    if let Some(value) = packed_boundary_attr(arena.str_val(left_boundaries)) {
                        buf.push_str(" data-mathema-left-boundaries=\"");
                        buf.push_str(&value);
                        buf.push('"');
                    }
                    if let Some(value) = packed_boundary_attr(arena.str_val(right_boundaries)) {
                        buf.push_str(" data-mathema-right-boundaries=\"");
                        buf.push_str(&value);
                        buf.push('"');
                    }
                    if span_boundary_has(arena.str_val(left_boundaries), SpanBoundary::Rule) {
                        buf.push_str(" data-mathema-left-rule=\"true\"");
                    }
                    if span_boundary_has(arena.str_val(right_boundaries), SpanBoundary::Rule) {
                        buf.push_str(" data-mathema-right-rule=\"true\"");
                    }
                    if span_boundary_has(arena.str_val(left_boundaries), SpanBoundary::SuppressGlue)
                    {
                        buf.push_str(" data-mathema-left-padding=\"true\"");
                    }
                    if span_boundary_has(
                        arena.str_val(right_boundaries),
                        SpanBoundary::SuppressGlue,
                    ) {
                        buf.push_str(" data-mathema-right-padding=\"true\"");
                    }
                    buf.push('>');
                    if promoted.is_some() && !nodes.is_empty() {
                        nodes = &nodes[1..];
                    }
                    if promoted.is_none()
                        && environment == Some(MatrixKind::SmallMatrix)
                        && nodes
                            .first()
                            .is_some_and(|&n| !matches!(arena.node(n), CompactNode::StyleSwitch(_)))
                    {
                        start_math_style_fast(buf, MathStyle::Script);
                        write_nodes_fast(buf, arena, nodes, MathStyle::Script, false);
                        close_tag(buf, "mstyle");
                    } else {
                        write_nodes_fast(buf, arena, nodes, cell_style, false);
                    }
                    buf.push_str("</mtd>");
                    i = i.saturating_add(cell_span);
                }
                buf.push_str("</mtr>");
            }
            buf.push_str("</mtable>");
            if reset_to_text {
                close_tag(buf, "mstyle");
            }
        },

        CompactNode::Phantom(inner, _) => {
            buf.push_str("<mphantom>");
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</mphantom>");
        },

        CompactNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            let bg = arena.str_val(bg_color);
            let border = arena.str_val(border_color);
            if !bg.is_empty() || !border.is_empty() {
                buf.push_str("<menclose notation=\"box\" style=\"");
                if !border.is_empty() {
                    buf.push_str("border-color: ");
                    xml_escape_into(buf, border);
                    buf.push_str("; ");
                }
                if !bg.is_empty() {
                    buf.push_str("background-color: ");
                    xml_escape_into(buf, bg);
                }
                buf.push_str("\">");
            } else {
                buf.push_str("<menclose notation=\"box\">");
            }
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</menclose>");
        },
        CompactNode::Actuarial {
            content: inner,
            text_mode,
        } => {
            buf.push_str("<menclose notation=\"actuarial\">");
            write_box_child_fast(buf, arena, inner, text_mode, display_style);
            buf.push_str("</menclose>");
        },

        CompactNode::Reflect { content, text_mode } => {
            buf.push_str("<mrow data-mathema-reflect=\"");
            buf.push_str(if text_mode { "text" } else { "math" });
            buf.push_str("\" style=\"display:inline-block; transform:scaleX(-1)\">");
            write_box_child_fast(buf, arena, content, text_mode, display_style);
            buf.push_str("</mrow>");
        },

        CompactNode::Cancel { stroke, content } => {
            let notation = match stroke {
                StrokeKind::Forward => "updiagonalstrike",
                StrokeKind::Backward => "downdiagonalstrike",
                StrokeKind::Cross => "updiagonalstrike downdiagonalstrike",
                StrokeKind::Horizontal => "horizontalstrike",
                StrokeKind::Vertical => "verticalstrike",
            };
            buf.push_str("<menclose notation=\"");
            buf.push_str(notation);
            buf.push_str("\">");
            write_node_fast(buf, arena, content, display_style);
            buf.push_str("</menclose>");
        },

        CompactNode::Scaled { factor, content } => {
            if let Some(delim) = big_delim_fast(arena, factor, content) {
                write_big_delim_fast(buf, "ORD", delim);
            } else {
                write_node_fast_in(buf, arena, content, single_argument, display_style)
            }
        },
        CompactNode::RaiseBox { content, .. } => {
            write_node_fast_in(buf, arena, content, single_argument, display_style)
        },

        CompactNode::Styled(style, span) => {
            let variant = style_to_mathvariant(style);
            buf.push_str("<mstyle mathvariant=\"");
            buf.push_str(variant);
            if buf.operator_font.is_some()
                && let Some(css) = scoped_font_css(style)
            {
                buf.push_str("\" style=\"");
                buf.push_str(css);
            }
            buf.push_str("\">");
            let font = buf.operator_font.map(|_| style);
            buf.with_operator_font(font, |buf| {
                write_nodes_fast(buf, arena, arena.children(span), display_style, false)
            });
            buf.push_str("</mstyle>");
        },

        CompactNode::Color { color, content } => {
            buf.push_str("<mstyle mathcolor=\"");
            xml_escape_into(buf, arena.str_val(color));
            buf.push_str("\">");
            write_node_fast(buf, arena, content, display_style);
            buf.push_str("</mstyle>");
        },

        // Helper nodes
        CompactNode::Middle(sid) => write_mo_middle_fast(buf, arena.str_val(sid)),
        CompactNode::ColAlign(_) => {},
        CompactNode::Rule {
            width,
            height,
            lift,
        } => {
            use std::fmt::Write as _;
            buf.push_str("<mspace");
            let _ = write!(buf, " width=\"{}em\"", width.as_f64());
            let _ = write!(
                buf,
                " height=\"{}em\"",
                (lift.as_f64() + height.as_f64()).max(0.0)
            );
            let _ = write!(buf, " depth=\"{}em\"", (-lift.as_f64()).max(0.0));
            buf.push_str("/>");
        },
        CompactNode::StyleSwitch(_) => {
            if single_argument {
                buf.push_str("<mrow/>");
            }
        },
        CompactNode::SizeScale(_) => {},
        CompactNode::Newline => buf.push_str(r#"<mspace linebreak="newline"/>"#),
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let branch = display_style.choose(display, text, script, script_script);
            write_node_fast_in(buf, arena, branch, single_argument, display_style);
        },
        CompactNode::MathAtom { atom_type, content } => {
            if let Some(class) = big_delim_class(atom_type)
                && let CompactNode::Scaled {
                    factor,
                    content: inner,
                } = *arena.node(content)
                && let Some(delim) = big_delim_fast(arena, factor, inner)
            {
                write_big_delim_fast(buf, class, delim);
            } else if let Some(class) = annotated_atom_class(atom_type) {
                write_atom_group_fast(buf, arena, content, class, false, display_style);
            } else if atom_type == MathAtomType::Ord
                && let CompactNode::Symbol(_, sid) = *arena.node(content)
                && marks_ordinary_operator(SymbolKind::Ordinary, arena.str_val(sid))
            {
                write_mo_symbol_fast(buf, SymbolKind::Ordinary, arena.str_val(sid));
            } else if let Some(kind) = spacing_atom_kind(atom_type)
                && let CompactNode::Symbol(SymbolKind::Ordinary, sid) = *arena.node(content)
                && marks_ordinary_operator(SymbolKind::Ordinary, arena.str_val(sid))
            {
                write_mo_symbol_fast(buf, kind, arena.str_val(sid));
            } else {
                write_node_fast_in(buf, arena, content, single_argument, display_style);
            }
        },
        CompactNode::StructuredOperator { content, form } => {
            write_structured_operator_group_fast(buf, arena, content, form, false, display_style)
        },
    }
}

/// Chromium treats MathML's `mathvariant` as a glyph-selection hint and may
/// report it as a normal computed CSS weight. The structured operator scope
/// also carries the equivalent standard CSS declaration, which makes the
/// browser's computed style and its selected glyph agree. Ordinary math
/// output does not use this helper.
fn scoped_font_css(style: TextStyle) -> Option<&'static str> {
    match style {
        TextStyle::Normal => Some("font-style:normal;font-weight:400"),
        TextStyle::Bold => Some("font-weight:700"),
        TextStyle::Italic => Some("font-style:italic"),
        TextStyle::BoldItalic => Some("font-style:italic;font-weight:700"),
        TextStyle::SansSerif => Some("font-family:sans-serif"),
        TextStyle::SansSerifBold => Some("font-family:sans-serif;font-weight:700"),
        TextStyle::SansSerifItalic => Some("font-family:sans-serif;font-style:italic"),
        TextStyle::SansSerifBoldItalic => {
            Some("font-family:sans-serif;font-style:italic;font-weight:700")
        },
        TextStyle::Monospace => Some("font-family:monospace"),
        _ => None,
    }
}

/// MathML has cell spacing but no element for a TeX matrix environment.
fn matrix_attributes(kind: Option<MatrixKind>) -> &'static [(&'static str, &'static str)] {
    match kind {
        Some(MatrixKind::SmallMatrix) => &[
            ("data-mathema-array-environment", "smallmatrix"),
            ("scriptlevel", "1"),
            ("rowspacing", "0.1em"),
            ("columnspacing", "0.2778em"),
        ],
        Some(MatrixKind::Matrix) => &[("data-mathema-array-environment", "matrix")],
        None => &[],
    }
}

#[inline]
fn table_rule_bit(mask: u32, boundary: usize) -> bool {
    boundary < u32::BITS as usize && mask & (1u32 << boundary) != 0
}

/// The MathML line style of a native rule boundary.
fn table_rule_style(mask: u32, dashed: u32, boundary: usize) -> &'static str {
    if !table_rule_bit(mask, boundary) {
        "none"
    } else if table_rule_bit(dashed, boundary) {
        "dashed"
    } else {
        "solid"
    }
}

/// Encode an interior rule mask using MathML's repeat-last line-list rule.
/// Trailing `none` is retained when it is needed to prevent a preceding
/// `solid` from repeating across the rest of the table.
fn table_line_attr(mask: u32, dashed: u32, cells: usize) -> Option<String> {
    let limit = cells.saturating_sub(1).min(u32::BITS as usize);
    if mask & !1 == 0 || !(1..=limit).any(|i| table_rule_bit(mask, i)) {
        return None;
    }
    let style = |i| table_rule_style(mask, dashed, i);
    // Boundary 32 can contribute the final "none" after a solid bit 31.
    let end = (2..=limit)
        .rev()
        .find(|&i| style(i) != style(i - 1))
        .unwrap_or(1);
    let mut value = String::new();
    for i in 1..=end {
        if i > 1 {
            value.push(' ');
        }
        value.push_str(style(i));
    }
    Some(value)
}

fn table_rule_attributes(
    col_seps: u32,
    row_seps: u32,
    dashes: RuleDashes,
    columns: usize,
    rows: usize,
    array_style: Option<bool>,
    mut attr: impl FnMut(&'static str, &str),
) {
    if let Some(value) = table_line_attr(col_seps, dashes.cols, columns) {
        attr("columnlines", &value);
    }
    if let Some(value) = table_line_attr(row_seps, dashes.rows, rows) {
        attr("rowlines", &value);
    }
    let edges = [
        (col_seps, dashes.cols, columns),
        (row_seps, dashes.rows, rows),
    ]
    .map(|(mask, dashed, size)| [0, size].map(|boundary| table_rule_style(mask, dashed, boundary)));
    let frame = match edges.as_flattened() {
        [first, rest @ ..] if *first != "none" && rest.iter().all(|style| style == first) => {
            Some(*first)
        },
        _ => None,
    };
    if let Some(frame) = frame {
        attr("frame", frame);
    } else {
        // A MathML frame covers all four sides in one style. The extension
        // encodes just the partial or mixed exterior edges so standard
        // interior lists remain editable.
        for (key, dashed_key, mask, dashed, size) in [
            (
                "data-mathema-col-seps",
                "data-mathema-dashed-col-seps",
                col_seps,
                dashes.cols,
                columns,
            ),
            (
                "data-mathema-row-seps",
                "data-mathema-dashed-row-seps",
                row_seps,
                dashes.rows,
                rows,
            ),
        ] {
            let exterior = 1 | u32::try_from(size)
                .ok()
                .and_then(|n| 1u32.checked_shl(n))
                .unwrap_or(0);
            let edges = mask & exterior;
            if edges != 0 {
                attr(key, &format!("0x{edges:x}"));
            }
            let dashed_edges = edges & dashed;
            if dashed_edges != 0 {
                attr(dashed_key, &format!("0x{dashed_edges:x}"));
            }
        }
    }
    if let Some(padded) = array_style {
        attr(
            "data-mathema-array-style",
            if padded { "true" } else { "false" },
        );
    }
}

// A named <mo> is a MathML core operator and can carry movablelimits.
// A TeX operator made of multiple elements is not a core <mo>, so its
// enclosing script element must reflect the current math style.
enum ScriptBase {
    Named(LimitStyle),
    Structured(LimitStyle),
    OperatorGroup,
    Other,
}

struct ScriptPlacement {
    stacked: bool,
    base_movable: Option<bool>,
}

impl ScriptBase {
    fn for_node(mut node: &MathNode) -> Self {
        while let MathNode::Row(nodes) = node {
            let [only] = nodes.as_slice() else { break };
            node = only;
        }
        match node {
            MathNode::Operator(attr) => Self::Named(attr.limit_style),
            MathNode::StructuredOperator { form, .. } => Self::Structured(form.limit_style()),
            MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => Self::OperatorGroup,
            _ => Self::Other,
        }
    }

    fn for_compact(arena: &AstArena, mut id: NodeId) -> Self {
        while let CompactNode::Row(span) = *arena.node(id) {
            let &[only] = arena.children(span) else { break };
            id = only;
        }
        match *arena.node(id) {
            CompactNode::Operator { limits, .. } => Self::Named(limits),
            CompactNode::StructuredOperator { form, .. } => Self::Structured(form.limit_style()),
            CompactNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => Self::OperatorGroup,
            _ => Self::Other,
        }
    }

    fn placement(&self, under_over: Option<bool>, display: MathStyle) -> ScriptPlacement {
        match self {
            Self::Named(style) => {
                let movable = under_over.or(match style {
                    LimitStyle::Default => Some(true),
                    LimitStyle::AboveBelow => Some(false),
                    LimitStyle::SubSup => None,
                });
                ScriptPlacement {
                    stacked: movable.is_some(),
                    base_movable: movable,
                }
            },
            Self::Structured(style) => {
                let movable = under_over.or(match style {
                    LimitStyle::Default => Some(true),
                    LimitStyle::AboveBelow => Some(false),
                    LimitStyle::SubSup => None,
                });
                ScriptPlacement {
                    stacked: movable.is_some_and(|moves| !moves || display.is_display()),
                    base_movable: movable,
                }
            },
            Self::OperatorGroup => ScriptPlacement {
                stacked: under_over.is_some_and(|movable| !movable || display.is_display()),
                base_movable: under_over,
            },
            Self::Other => ScriptPlacement {
                stacked: under_over.is_some(),
                base_movable: under_over,
            },
        }
    }

    fn allows_glyph_accent(&self) -> bool {
        match self {
            Self::Named(style) => *style != LimitStyle::AboveBelow,
            Self::Structured(style) => *style != LimitStyle::AboveBelow,
            Self::Other => true,
            // An explicit `\limits` on a compound operator has no style field
            // in MathAtom. Keep its fixed under/over wrapper unaccented.
            Self::OperatorGroup => false,
        }
    }
}

fn named_movable_attr(style: LimitStyle) -> Option<&'static str> {
    match style {
        LimitStyle::Default => Some("true"),
        LimitStyle::AboveBelow => Some("false"),
        LimitStyle::SubSup => None,
    }
}

/// A single letter or punctuation might otherwise be read as a symbol.
fn needs_operator_marker(name: &str, limits: LimitStyle, scripted: bool) -> bool {
    // Without a marker, a bare built-in name such as lim defaults to movable
    // limits on import. Retain an explicit side-script choice as well.
    if !scripted
        && limits == LimitStyle::SubSup
        && crate::unicode::symbols::operator_has_limits(name)
    {
        return true;
    }
    let mut chars = name.chars();
    !chars.next().is_some_and(|first| {
        first.is_alphabetic() && !chars.as_str().is_empty() && chars.all(char::is_alphabetic)
    })
}

#[inline]
fn is_mathml_accent_glyph(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(_, text) => {
            let mut chars = text.chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && crate::unicode::symbols::is_accent_glyph(ch)
        },
        MathNode::Row(nodes) if nodes.len() == 1 => is_mathml_accent_glyph(&nodes[0]),
        _ => false,
    }
}

#[inline]
fn is_mathml_accent_glyph_fast(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Symbol(_, sid) => {
            let mut chars = arena.str_val(sid).chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && crate::unicode::symbols::is_accent_glyph(ch)
        },
        CompactNode::Row(span) if arena.children(span).len() == 1 => {
            is_mathml_accent_glyph_fast(arena, arena.children(span)[0])
        },
        _ => false,
    }
}

fn mathml_script_decoration(node: &MathNode, position: AccentPosition) -> Option<&'static str> {
    let MathNode::Symbol(_, text) = node else {
        return None;
    };
    crate::unicode::symbols::fixed_script_decoration(text, position)
}

fn mathml_script_decoration_fast(
    arena: &AstArena,
    id: NodeId,
    position: AccentPosition,
) -> Option<&'static str> {
    let CompactNode::Symbol(_, sid) = *arena.node(id) else {
        return None;
    };
    crate::unicode::symbols::fixed_script_decoration(arena.str_val(sid), position)
}

fn write_scripts(
    w: &mut XmlWriter,
    base: &MathNode,
    lower: Option<&MathNode>,
    upper: Option<&MathNode>,
    under_over: Option<bool>,
    display: MathStyle,
) -> Result<()> {
    let script_base = ScriptBase::for_node(base);
    let place = script_base.placement(under_over, display);
    let accent_value = if under_over == Some(false) && script_base.allows_glyph_accent() {
        "true"
    } else {
        "false"
    };
    let Some(tag) = script_tag(place.stacked, lower.is_some(), upper.is_some()) else {
        return write_argument(w, base, display);
    };
    let mut elem = BytesStart::new(tag);
    if under_over == Some(false) && accent_value == "true" && tag != "munderover" {
        if let Some(name) =
            upper.and_then(|node| mathml_script_decoration(node, AccentPosition::Above))
        {
            elem.push_attribute(("data-mathema-accent-decoration", name));
        } else if let Some(name) =
            lower.and_then(|node| mathml_script_decoration(node, AccentPosition::Below))
        {
            elem.push_attribute(("data-mathema-accent-decoration", name));
        }
    }
    if place.stacked {
        if lower.is_some_and(is_mathml_accent_glyph) {
            elem.push_attribute(("accentunder", accent_value));
        }
        if upper.is_some_and(is_mathml_accent_glyph) {
            elem.push_attribute(("accent", accent_value));
        }
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    if let Some(movable) = place.base_movable {
        write_limit_base(w, base, movable, display)?;
    } else if let MathNode::Operator(attr) = base {
        write_named_operator(w, &attr.name, attr.limit_style, true)?;
    } else {
        write_argument(w, base, display)?;
    }
    if let Some(sub) = lower {
        write_argument(w, sub, display.sub())?;
    }
    if let Some(sup) = upper {
        write_argument(w, sup, display.sup())?;
    }
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(to_err)
}

fn write_scripts_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    base: NodeId,
    lower: NodeId,
    upper: NodeId,
    under_over: Option<bool>,
    display: MathStyle,
) {
    let script_base = ScriptBase::for_compact(arena, base);
    let place = script_base.placement(under_over, display);
    let accent_value = if under_over == Some(false) && script_base.allows_glyph_accent() {
        "true"
    } else {
        "false"
    };
    let Some(tag) = script_tag(place.stacked, lower.is_some(), upper.is_some()) else {
        return write_argument_fast(buf, arena, base, display);
    };
    buf.push('<');
    buf.push_str(tag);
    if under_over == Some(false) && accent_value == "true" && tag != "munderover" {
        if let Some(name) = upper
            .is_some()
            .then(|| mathml_script_decoration_fast(arena, upper, AccentPosition::Above))
            .flatten()
        {
            buf.push_str(" data-mathema-accent-decoration=\"");
            buf.push_str(name);
            buf.push('"');
        } else if let Some(name) = lower
            .is_some()
            .then(|| mathml_script_decoration_fast(arena, lower, AccentPosition::Below))
            .flatten()
        {
            buf.push_str(" data-mathema-accent-decoration=\"");
            buf.push_str(name);
            buf.push('"');
        }
    }
    if place.stacked {
        for (attr, id) in [("accentunder", lower), ("accent", upper)] {
            if id.is_some() && is_mathml_accent_glyph_fast(arena, id) {
                buf.push(' ');
                buf.push_str(attr);
                buf.push_str("=\"");
                buf.push_str(accent_value);
                buf.push('"');
            }
        }
    }
    buf.push('>');
    if let Some(movable) = place.base_movable {
        write_limit_base_fast(buf, arena, base, movable, display);
    } else if let CompactNode::Operator { name, limits } = *arena.node(base) {
        write_named_operator_fast(buf, arena.str_val(name), limits, true);
    } else {
        write_argument_fast(buf, arena, base, display);
    }
    if lower.is_some() {
        write_argument_fast(buf, arena, lower, display.sub());
    }
    if upper.is_some() {
        write_argument_fast(buf, arena, upper, display.sup());
    }
    close_tag(buf, tag);
}

/// Select the MathML script element and whether its limits move to the side
/// in compact math style. Fixed side scripts use the sub/sup family directly.
fn nary_script_tag(
    op: char,
    limits: LimitStyle,
    sub: bool,
    sup: bool,
) -> Option<(&'static str, bool)> {
    let movable =
        limits == LimitStyle::Default && !crate::unicode::symbols::is_integral_operator(op);
    let stacked = movable || limits == LimitStyle::AboveBelow;
    script_tag(stacked, sub, sup).map(|tag| (tag, movable))
}

fn script_tag(stacked: bool, sub: bool, sup: bool) -> Option<&'static str> {
    Some(match (stacked, sub, sup) {
        (_, false, false) => return None,
        (true, true, true) => "munderover",
        (true, true, false) => "munder",
        (true, false, true) => "mover",
        (false, true, true) => "msubsup",
        (false, true, false) => "msub",
        (false, false, true) => "msup",
    })
}

#[inline]
fn is_empty_math_node(node: &MathNode) -> bool {
    matches!(node, MathNode::Row(children) if children.is_empty())
}

#[inline]
fn is_empty_compact_node(arena: &AstArena, id: NodeId) -> bool {
    id == NodeId::NONE
        || matches!(*arena.node(id), CompactNode::Row(span) if arena.children(span).is_empty())
}

#[inline]
fn write_mi_fast(buf: &mut XmlBuffer, s: &str) {
    if let Some(variant) = identifier_variant(s, buf.operator_font) {
        buf.push_str(r#"<mi mathvariant=""#);
        buf.push_str(variant);
        buf.push_str(r#"">"#);
    } else {
        buf.push_str("<mi>");
    }
    xml_escape_into(buf, s);
    buf.push_str("</mi>");
}

#[inline]
fn write_mo_char_fast(buf: &mut XmlBuffer, ch: char, movable: Option<bool>) {
    buf.push_str(match movable {
        Some(true) => "<mo movablelimits=\"true\">",
        Some(false) => "<mo movablelimits=\"false\">",
        None => "<mo>",
    });
    let mut glyph = [0; 4];
    xml_escape_into(buf, ch.encode_utf8(&mut glyph));
    buf.push_str("</mo>");
}

fn write_limit_base_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    movable: bool,
    display_style: MathStyle,
) {
    match *arena.node(id) {
        CompactNode::Operator { name, .. } => write_named_operator_fast(
            buf,
            arena.str_val(name),
            if movable {
                LimitStyle::Default
            } else {
                LimitStyle::AboveBelow
            },
            true,
        ),
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => write_atom_group_fast(buf, arena, content, "OP", movable, display_style),
        CompactNode::StructuredOperator { content, form } => {
            write_structured_operator_group_fast(buf, arena, content, form, movable, display_style)
        },
        CompactNode::Symbol(SymbolKind::LargeOp, sid) if movable => {
            if let Some(ch) = single_operator_char(arena.str_val(sid)) {
                write_mo_char_fast(buf, ch, Some(true));
            } else {
                write_argument_fast(buf, arena, id, display_style);
            }
        },
        CompactNode::Row(span) if arena.children(span).len() == 1 => {
            open_tag(buf, "mrow");
            write_limit_base_fast(buf, arena, arena.children(span)[0], movable, display_style);
            close_tag(buf, "mrow");
        },
        _ => write_argument_fast(buf, arena, id, display_style),
    }
}

fn write_atom_group_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    content: NodeId,
    class: &str,
    movable: bool,
    display_style: MathStyle,
) {
    buf.push_str(r#"<mrow data-mjx-texclass=""#);
    buf.push_str(class);
    if movable {
        buf.push_str(r#"" data-mathema-limits="movable">"#);
    } else {
        buf.push_str(r#"">"#);
    }
    if let CompactNode::Row(span) = *arena.node(content) {
        write_nodes_fast(buf, arena, arena.children(span), display_style, false);
    } else {
        write_node_fast(buf, arena, content, display_style);
    }
    close_tag(buf, "mrow");
}

fn write_structured_operator_group_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    content: NodeId,
    form: OperatorNameForm,
    movable: bool,
    display_style: MathStyle,
) {
    buf.push_str(r#"<mrow data-mjx-texclass="OP""#);
    buf.push_str(r#" data-mathema-structured-operator=""#);
    buf.push_str(structured_operator_marker(form));
    buf.push('"');
    if movable {
        buf.push_str(r#" data-mathema-limits="movable""#);
    }
    buf.push('>');
    buf.with_operator_font(Some(TextStyle::Normal), |buf| match *arena.node(content) {
        CompactNode::Row(span) => {
            write_nodes_fast(buf, arena, arena.children(span), display_style, false)
        },
        _ => write_node_fast(buf, arena, content, display_style),
    });
    close_tag(buf, "mrow");
}

fn write_named_operator_fast(buf: &mut XmlBuffer, name: &str, limits: LimitStyle, scripted: bool) {
    buf.push_str("<mo");
    if let Some(value) = named_movable_attr(limits) {
        buf.push_str(" movablelimits=\"");
        buf.push_str(value);
        buf.push('"');
    }
    if needs_operator_marker(name, limits, scripted) {
        buf.push_str(r#" data-mathema-operator="name""#);
    }
    buf.push('>');
    xml_escape_into(buf, name);
    close_tag(buf, "mo");
}

#[inline]
fn write_box_child_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    child: NodeId,
    text_size: bool,
    display_style: MathStyle,
) {
    if text_size {
        buf.push_str(r#"<mstyle displaystyle="false" scriptlevel="0">"#);
    }
    write_node_fast(
        buf,
        arena,
        child,
        if text_size {
            MathStyle::Text
        } else {
            display_style
        },
    );
    if text_size {
        buf.push_str("</mstyle>");
    }
}

fn write_mo_fence_fast(buf: &mut XmlBuffer, text: &str, form: Option<&str>, stretchy: bool) {
    buf.push_str("<mo fence=\"true\"");
    if let Some(form) = form {
        buf.push_str(" form=\"");
        buf.push_str(form);
        buf.push('"');
    }
    if stretchy {
        buf.push_str(" stretchy=\"true\" symmetric=\"true\"");
    }
    buf.push('>');
    xml_escape_into(buf, text);
    buf.push_str("</mo>");
}

fn write_mo_middle_fast(buf: &mut XmlBuffer, text: &str) {
    write_mo_fence_fast(buf, text, middle_form(text), true);
}

fn write_literal_fence_fast(buf: &mut XmlBuffer, text: &str) {
    buf.push_str("<mo stretchy=\"false\">");
    xml_escape_into(buf, text);
    buf.push_str("</mo>");
}

fn is_literal_fence_glyph(text: &str) -> bool {
    matches!(text, "|" | "‖" | "∣" | "∥")
}

fn write_nodes_fenced_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    display_style: MathStyle,
) {
    write_style_sequence_fast(
        buf,
        arena,
        nodes,
        display_style,
        false,
        |buf, arena, id, style| {
            if let CompactNode::Symbol(SymbolKind::Fence, sid) = *arena.node(id) {
                let text = arena.str_val(sid);
                if is_literal_fence_glyph(text) {
                    write_literal_fence_fast(buf, text);
                    return;
                }
            }
            write_node_fast(buf, arena, id, style);
        },
    );
}

fn compact_fenced_content_needs_stretch(arena: &AstArena, content: NodeSpan) -> bool {
    arena.children(content).iter().any(|&id| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::Fence, sid)
                if is_literal_fence_glyph(arena.str_val(sid))
        )
    })
}

fn needs_explicit_endpoint_roles(open: &str, close: &str) -> bool {
    crate::unicode::operators::closing_fence(open) != Some(close)
}

fn write_mo_symbol_fast(buf: &mut XmlBuffer, kind: SymbolKind, s: &str) {
    buf.push_str("<mo");
    let ordinary_literal_bar = kind == SymbolKind::Ordinary && matches!(s, "|" | "‖");
    match kind {
        SymbolKind::LargeOp => buf.push_str(" largeop=\"true\""),
        SymbolKind::Fence => buf.push_str(" fence=\"true\""),
        _ => {},
    }
    if ordinary_literal_bar {
        buf.push_str(" fence=\"false\" stretchy=\"false\"");
    } else if let Some(ch) = s.chars().next() {
        let form = symbol_kind_to_form(kind);
        if let Some(entry) = lookup_operator_any(ch, form) {
            if entry.stretchy {
                buf.push_str(" stretchy=\"true\"");
            }
            if entry.form != OperatorForm::Infix {
                buf.push_str(" form=\"");
                buf.push_str(crate::unicode::operators::form_str(entry.form));
                buf.push('"');
            }
        }
    }
    if marks_ordinary_operator(kind, s) {
        buf.push_str(r#" data-mjx-texclass="ORD" lspace="0em" rspace="0em""#);
    }
    buf.push('>');
    xml_escape_into(buf, s);
    buf.push_str("</mo>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::normalize::{normalize_compact_expr, normalize_expr};
    use crate::reader::Reader;
    use crate::reader::latex::LatexReader;

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
    fn a_spanning_cell_keeps_the_following_authored_cell() {
        let xml = r#"<math><mtable><mtr><mtd columnspan="2"><mn>8</mn></mtd><mtd columnalign="right"><mn>9</mn></mtd></mtr><mtr><mn>1</mn><mn>2</mn><mn>3</mn></mtr></mtable></math>"#;
        let parsed = crate::from_mathml(xml).unwrap();
        let heap = parsed.expr();
        let compact = CompactExpr::from_math_expr(&heap).unwrap();
        let cfg = Config::default();
        let result = MathmlWriter::write(&heap, &cfg).unwrap();
        assert!(
            result.contains(r#"<mtd columnalign="right"><mn>9</mn></mtd>"#),
            "{result}"
        );
        assert_eq!(result, MathmlWriter::write_compact(&compact, &cfg).unwrap());
        assert_eq!(result, parsed.to_mathml().unwrap());
    }

    #[test]
    fn mixed_span_wrappers_do_not_change_table_shape() {
        let span = MathNode::SpannedCell {
            span: 6,
            alignment: ColumnAlign::Right,
            left_boundaries: vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
            right_boundaries: vec![SpanBoundary::Rule, SpanBoundary::SuppressGlue],
            content: Box::new(MathNode::Number("1".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::Matrix,
            alignment: ColumnAlign::Left,
            rows: vec![vec![
                vec![span, MathNode::Number("2".into())],
                vec![MathNode::Number("3".into())],
            ]],
        }]);
        let config = Config::default();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let xml = MathmlWriter::write(&expr, &config).unwrap();
        assert_eq!(xml.matches("<mtd ").count(), 2);
        assert!(!xml.contains("columnspan"));
        assert!(!xml.contains("data-mathema-left-rule"));
        assert_eq!(xml, MathmlWriter::write_compact(&compact, &config).unwrap());
        #[cfg(feature = "render")]
        {
            let opts = crate::render::RenderOptions::default();
            assert_eq!(
                crate::render::render_to_svg(&expr, &opts).unwrap(),
                crate::render::render_to_svg_compact(&compact, &opts).unwrap()
            );
        }
    }

    #[test]
    fn ordered_span_boundaries_survive_mathml_roundtrip() {
        let cell = MathNode::SpannedCell {
            span: 1,
            alignment: ColumnAlign::Left,
            left_boundaries: vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
            right_boundaries: vec![SpanBoundary::Rule, SpanBoundary::Rule],
            content: Box::new(MathNode::Identifier("x".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Left],
            rows: vec![vec![vec![cell]]],
            arraystretch: None,
            add_jot: false,
        }]);
        let config = Config::default();
        let xml = MathmlWriter::write(&expr, &config).unwrap();
        assert!(
            xml.contains(r#"data-mathema-left-boundaries="padding|rule""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"data-mathema-right-boundaries="rule|rule""#),
            "{xml}"
        );
        let reparsed = crate::from_mathml(&xml).unwrap();
        assert_eq!(reparsed.expr(), expr);
    }

    #[test]
    fn expansive_heap_spans_respect_the_compact_grid_limit() {
        let cell = MathNode::SpannedCell {
            span: 32768,
            alignment: ColumnAlign::Left,
            left_boundaries: Vec::new(),
            right_boundaries: Vec::new(),
            content: Box::new(MathNode::Number("1".into())),
        };
        let rows = vec![vec![vec![cell]]; 2];
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::Matrix,
            alignment: ColumnAlign::Left,
            rows,
        }]);
        let is_grid_error = |err: crate::Error| {
            matches!(
                err,
                crate::Error::LimitExceeded {
                    kind: crate::error::LimitKind::ListLength,
                    limit: AstArena::MAX_CHILDREN
                }
            )
        };
        assert!(is_grid_error(
            CompactExpr::from_math_expr(&expr).unwrap_err()
        ));
        assert!(is_grid_error(
            MathmlWriter::write(&expr, &Config::default()).unwrap_err()
        ));
        #[cfg(feature = "render")]
        assert!(is_grid_error(
            crate::render::render_to_svg(&expr, &crate::render::RenderOptions::default())
                .unwrap_err()
        ));
    }

    #[test]
    fn minus_symbol_class_survives_mathml_round_trip() {
        for (source, expected_class) in [
            (r"x-y", "binary"),
            (r"-x", "binary"),
            (r"\mathord{-}", "ordinary"),
            (r"\mathord{−}", "ordinary"),
        ] {
            let parsed = crate::from_latex(source).unwrap();
            let mathml = parsed.to_mathml().unwrap();
            assert!(
                mathml.contains(if expected_class == "ordinary" {
                    r#"data-mjx-texclass="ORD" lspace="0em" rspace="0em""#
                } else {
                    "<mo>−</mo>"
                }),
                "{source}: {mathml}"
            );
            let roundtrip = crate::from_mathml(&mathml).unwrap();
            assert_eq!(mathml, roundtrip.to_mathml().unwrap(), "{source}");
            #[cfg(feature = "render")]
            assert_eq!(
                parsed.to_svg().unwrap(),
                roundtrip.to_svg().unwrap(),
                "{source}"
            );
        }

        let unmarked =
            crate::from_mathml(r#"<math><mi>x</mi><mo>−</mo><mi>y</mi></math>"#).unwrap();
        let marked = unmarked.to_mathml().unwrap();
        assert!(marked.contains("<mo>−</mo>"));
        assert_eq!(
            marked,
            crate::from_latex("x-y").unwrap().to_mathml().unwrap()
        );
    }

    #[test]
    fn text_style_and_whitespace_round_trip_on_heap_and_compact_paths() {
        let styles = [
            TextStyle::Normal,
            TextStyle::Bold,
            TextStyle::Italic,
            TextStyle::BoldItalic,
            TextStyle::Monospace,
            TextStyle::SansSerif,
            TextStyle::SansSerifBold,
            TextStyle::SansSerifItalic,
            TextStyle::SansSerifBoldItalic,
            TextStyle::DoubleStruck,
            TextStyle::Script,
            TextStyle::BoldScript,
            TextStyle::Fraktur,
            TextStyle::BoldFraktur,
        ];
        let config = Config::default();
        let texts = [
            (
                "  a  < b  ",
                "\u{00a0}\u{00a0}a \u{00a0}< b\u{00a0}\u{00a0}",
            ),
            ("\ta\nb\r", "\u{00a0}a b\u{00a0}"),
            ("\u{00a0} a\u{00a0}", "\u{00a0} a\u{00a0}"),
        ];
        for style in styles {
            for &(source, canonical) in &texts {
                let expr = MathExpr::inline(vec![MathNode::Text(style, source.into())]);
                let heap = MathmlWriter::write(&expr, &config).unwrap();
                let compact_expr = CompactExpr::from_math_expr(&expr).unwrap();
                let compact = MathmlWriter::write_compact(&compact_expr, &config).unwrap();
                assert_eq!(heap, compact, "style: {style:?}, source: {source:?}");
                if style == TextStyle::Normal {
                    assert!(heap.contains("<mtext>"), "{heap}");
                } else {
                    assert!(
                        heap.contains(&format!("mathvariant=\"{}\"", style_to_mathvariant(style))),
                        "style: {style:?}, output: {heap}"
                    );
                }

                let parsed = crate::reader::mathml::MathmlReader::parse(
                    &heap,
                    &config.with_display(DisplayStyle::Inline),
                )
                .unwrap()
                .expr;
                assert_eq!(
                    parsed.nodes,
                    vec![MathNode::Text(style, canonical.into())],
                    "style: {style:?}, source: {source:?}"
                );
                let rewritten = MathmlWriter::write(&parsed, &config).unwrap();
                assert_eq!(rewritten, heap, "style: {style:?}, source: {source:?}");
            }
        }
    }

    #[test]
    fn ordinary_vertical_bars_keep_their_mathml_class() {
        let config = Config::default();
        for source in [
            r"\vert",
            r"\Vert",
            r"\left(\vert x\vert\right)",
            r"\Set{\vert x\vert|P}",
        ] {
            let original = crate::from_latex(source).unwrap();
            let mathml = original.to_mathml().unwrap();
            assert!(mathml.contains("fence=\"false\""), "{source}: {mathml}");
            assert!(mathml.contains("stretchy=\"false\""), "{source}: {mathml}");
            let imported = crate::from_mathml(&mathml).unwrap();
            assert_eq!(imported.to_mathml().unwrap(), mathml, "{source}");
            #[cfg(feature = "render")]
            assert_eq!(
                imported.to_svg_text().unwrap(),
                original.to_svg_text().unwrap()
            );

            let raw_heap = LatexReader::parse(source, &config).unwrap().expr;
            let raw_compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            assert_eq!(
                MathmlWriter::write(&raw_heap, &config).unwrap(),
                MathmlWriter::write_compact(&raw_compact, &config).unwrap(),
                "raw heap/compact: {source}"
            );

            let mut normalized_heap = raw_heap.clone();
            normalize_expr(&mut normalized_heap);
            let mut normalized_compact = raw_compact.clone();
            normalize_compact_expr(&mut normalized_compact).unwrap();
            assert_eq!(
                MathmlWriter::write(&normalized_heap, &config).unwrap(),
                MathmlWriter::write_compact(&normalized_compact, &config).unwrap(),
                "normalized heap/compact: {source}"
            );
        }
    }

    #[test]
    fn text_without_whitespace_keeps_style() {
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Bold, "hello".into())]);
        let output = MathmlWriter::write(&expr, &Config::default()).unwrap();
        assert!(output.contains("<mtext mathvariant=\"bold\">hello</mtext>"));
        assert!(!output.contains("xml:space"));
    }

    #[test]
    fn middle_and_slash_delimiters_request_symmetric_stretching() {
        for (source, operator) in [
            (
                r"\left(\frac{1}{x}\middle|y\right)",
                r#"stretchy="true" symmetric="true">|</mo>"#,
            ),
            (
                r"\left\backslash\frac{1}{x}\right/",
                r#"fence="true" stretchy="true" symmetric="true">\</mo>"#,
            ),
        ] {
            let result = crate::from_latex(source).unwrap();
            let xml = result.to_mathml().unwrap();
            assert_eq!(
                MathmlWriter::write(&result.expr(), &Config::default()).unwrap(),
                xml
            );
            assert!(xml.contains(operator), "{source}: {xml}");
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn negative_mspace_has_a_signed_browser_advance_and_round_trips() {
        let formula = crate::from_latex(r"a\hskip-.25em b").unwrap();
        let xml = formula.to_mathml().unwrap();
        assert!(
            xml.contains(r#"<mspace width="0em" style="margin-inline-start:-0.25em"/>"#),
            "{xml}"
        );
        assert_eq!(
            xml,
            MathmlWriter::write(&formula.expr(), &Config::default()).unwrap()
        );
        let again = crate::from_mathml(&xml).unwrap();
        assert_eq!(again.to_mathml().unwrap(), xml);
        assert_eq!(
            crate::from_mathml(r#"<math><mspace width="-.25em"/></math>"#)
                .unwrap()
                .expr()
                .nodes,
            vec![MathNode::Space(Rational::new(-1, 4))]
        );
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
    fn operator_accents_keep_true_accents_separate_from_limit_labels() {
        for (latex, accent) in [
            (r"\overset{¯}{\lim}", true),
            (r"\overset{¯}{\operatorname{rate}}", true),
            (r"\overset{¯}{\mathop{lim}}", true),
            (r"\overline{\lim}", true),
            (r"\overset{!}{\lim}", false),
            (r"\lim\limits^¯", false),
            (r"\overset{!}{=}", false),
        ] {
            let result = crate::from_latex(latex).unwrap();
            let heap = MathmlWriter::write(&result.expr(), &Config::default()).unwrap();
            let compact =
                MathmlWriter::write_compact(result.compact_expr(), &Config::default()).unwrap();
            assert_eq!(heap, compact, "heap/compact mismatch for {latex}");
            assert_eq!(heap.contains("accent=\"true\""), accent, "{latex}: {heap}");
        }

        let authored = r#"<math><mover accent="true"><mo>lim</mo><mo>¯</mo></mover></math>"#;
        let output =
            crate::convert(authored, crate::Format::MathML, crate::Format::MathML).unwrap();
        assert!(output.contains(r#"<mover accent="true"><mo movablelimits="false">lim</mo>"#));
        let movable = authored.replace("<mo>lim", r#"<mo movablelimits="true">lim"#);
        let output = crate::from_mathml(&movable).unwrap().to_mathml().unwrap();
        assert!(output.contains(r#"<mover accent="true"><mo movablelimits="true">lim</mo>"#));
        for source in [r"\lim\limits^¯", r"\mathop{x}\limits_¯"] {
            let xml = crate::from_latex(source).unwrap().to_mathml().unwrap();
            assert!(
                xml.contains(r#"accent="false""#) || xml.contains(r#"accentunder="false""#),
                "{xml}"
            );
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn text_sizing_is_separate_from_an_upright_font() {
        let xml = r#"<math><menclose notation="actuarial"><mi mathvariant="normal">x</mi></menclose></math>"#;
        let parsed = crate::from_mathml(xml).unwrap();
        let normalized = parsed.to_mathml().unwrap();
        assert!(
            normalized.contains(r#"mathvariant="normal""#),
            "{normalized}"
        );
        assert!(!normalized.contains(r#"scriptlevel="0""#), "{normalized}");
        for mode in [true, false] {
            for node in [
                MathNode::Actuarial {
                    content: Box::new(MathNode::Identifier("x".into())),
                    text_mode: mode,
                },
                MathNode::Reflect {
                    content: Box::new(MathNode::Identifier("x".into())),
                    text_mode: mode,
                },
            ] {
                let expr = MathExpr::block(vec![node]);
                let compact = CompactExpr::from_math_expr(&expr).unwrap();
                for output in [
                    MathmlWriter::write(&expr, &Config::default()).unwrap(),
                    MathmlWriter::write_compact(&compact, &Config::default()).unwrap(),
                ] {
                    assert_eq!(output.matches("<mi>x</mi>").count(), 1, "{output}");
                    assert_eq!(output.contains(r#"scriptlevel="0""#), mode);
                    let round = crate::from_mathml(&output).unwrap().to_mathml().unwrap();
                    assert_eq!(round, output);
                }
            }
        }
    }

    #[test]
    fn actuarial_text_mode_emits_standard_size_hints() {
        let text = write(MathNode::Actuarial {
            content: Box::new(MathNode::Identifier("x".into())),
            text_mode: true,
        });
        assert!(text.contains("notation=\"actuarial\""));
        assert!(text.contains("displaystyle=\"false\""));
        assert!(text.contains("scriptlevel=\"0\""));

        let inherited = write(MathNode::Actuarial {
            content: Box::new(MathNode::Identifier("x".into())),
            text_mode: false,
        });
        assert!(inherited.contains("notation=\"actuarial\""));
        assert!(!inherited.contains("scriptlevel=\"0\""));
    }

    #[test]
    fn native_actuarial_mathml_keeps_inherited_style() {
        let config = Config::default();
        let parsed = crate::reader::mathml::MathmlReader::parse(
            r#"<math><msub><mi>x</mi><menclose notation="actuarial"><mi>y</mi></menclose></msub></math>"#,
            &config,
        )
        .unwrap()
        .expr;
        let MathNode::Subscript { sub, .. } = &parsed.nodes[0] else {
            panic!("expected subscript: {:?}", parsed.nodes);
        };
        assert!(matches!(
            &**sub,
            MathNode::Actuarial {
                text_mode: false,
                ..
            }
        ));
    }

    #[test]
    fn latex_hbox_actuarial_style_hints_survive_mathml_round_trip() {
        let mathml =
            crate::convert(r"\angl{$x_i$}", crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert!(mathml.contains("displaystyle=\"false\""));
        assert!(mathml.contains("scriptlevel=\"0\""));
        let latex = crate::convert(&mathml, crate::Format::MathML, crate::Format::LaTeX).unwrap();
        assert!(latex.starts_with(r"\angl{"), "{latex:?}");
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
        let output = write(MathNode::Phantom(
            Box::new(MathNode::Identifier("x".into())),
            PhantomKind::Full,
        ));
        assert!(output.contains("<mphantom>"));
    }

    #[test]
    fn boxed() {
        let output = write(MathNode::Boxed {
            content: Box::new(MathNode::Number("1".into())),
            bg_color: None,
            border_color: None,
        });
        assert!(output.contains("<menclose"));
        assert!(output.contains("notation=\"box\""));
    }

    #[test]
    fn below_accent_uses_accentunder_attribute() {
        let node = MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '_',
            position: AccentPosition::Below,
            wide: false,
        };
        let output = write(node.clone());
        assert!(output.contains("<munder accentunder=\"true\">"));
        assert!(!output.contains("<munder accent=\"true\">"));

        let expr = MathExpr::inline(vec![node]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(output.contains("<munder accentunder=\"true\">"));
        assert!(!output.contains("<munder accent=\"true\">"));
    }

    #[test]
    #[cfg(feature = "render")]
    fn builtin_accent_identity_survives_mathml_roundtrip_on_both_paths() {
        use crate::reader::Reader;
        use crate::reader::latex::LatexReader;
        use crate::render::{RenderOptions, render_to_svg_compact};

        let config = Config::default();
        for source in [
            r"\underline{x}",
            r"\overline{lim}",
            r"\underbracket{end}",
            r"\overbracket{rate}",
            r"\overbrace{rate}",
            r"\underbrace{rate}",
            r"\underleftarrow{lim}",
            r"\overlinesegment{ab}",
            r"\underlinesegment{ab}",
            r"\widehat{AB}",
            r"\widetilde{AB}",
            r"\hat{x}",
            r"\bar{x}",
        ] {
            let heap = LatexReader::parse(source, &config).unwrap().expr;
            let compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            let heap_xml = MathmlWriter::write(&heap, &config).unwrap();
            let compact_xml = MathmlWriter::write_compact(&compact, &config).unwrap();
            assert_eq!(
                heap_xml, compact_xml,
                "heap/compact MathML mismatch: {source}"
            );
            let imported = crate::from_mathml(&heap_xml).unwrap();
            assert_eq!(
                heap_xml,
                imported.to_mathml().unwrap(),
                "MathML instability: {source}"
            );
            assert_eq!(
                svg_dimensions(
                    &render_to_svg_compact(&compact, &RenderOptions::default()).unwrap()
                ),
                svg_dimensions(&imported.to_svg().unwrap()),
                "compact geometry changed on reimport: {source}"
            );
        }
    }

    #[cfg(feature = "render")]
    fn svg_dimensions(svg: &str) -> (&str, &str, &str) {
        let root = svg.split('>').next().expect("SVG root");
        let attr = |name: &str| {
            root.split_once(&format!(" {name}=\""))
                .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
                .unwrap_or("")
        };
        (attr("width"), attr("height"), attr("viewBox"))
    }

    fn assert_rule_outputs_match(
        width: Rational,
        height: Rational,
        lift: Rational,
        expected: &str,
    ) {
        let node = MathNode::Rule {
            width,
            height,
            lift,
        };
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        let output = MathmlWriter::write(&expr, &config).unwrap();
        assert!(output.contains(expected), "non-compact output: {output}");

        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(output.contains(expected), "compact output: {output}");
    }

    #[test]
    fn rule_positive_lift_writes_height_and_depth_on_both_paths() {
        assert_rule_outputs_match(
            Rational::new(2, 1),
            Rational::new(3, 1),
            Rational::new(1, 1),
            "<mspace width=\"2em\" height=\"4em\" depth=\"0em\"/>",
        );
    }

    #[test]
    fn rule_negative_lift_writes_height_and_depth_on_both_paths() {
        assert_rule_outputs_match(
            Rational::new(2, 1),
            Rational::new(3, 1),
            Rational::new(-1, 1),
            "<mspace width=\"2em\" height=\"2em\" depth=\"1em\"/>",
        );
    }

    #[test]
    fn compact_accent_escapes_xml_special_operator_chars() {
        let expr = MathExpr::inline(vec![MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '<',
            position: AccentPosition::Below,
            wide: false,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains(r#"<mo stretchy="false">&lt;</mo>"#),
            "compact output: {output}"
        );
        assert!(!output.contains("<mo><</mo>"), "compact output: {output}");
    }

    #[test]
    fn compact_nary_escapes_xml_special_operator_chars() {
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '<',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: None,
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("<mo movablelimits=\"false\">&lt;</mo>"),
            "compact output: {output}"
        );
        assert!(!output.contains("><<"), "compact output: {output}");
    }

    #[test]
    fn mathchoice_uses_its_local_style_in_both_writers() {
        fn xml(latex: &str, display: DisplayStyle) -> String {
            let config = Config::default().with_display(display);
            let parsed = crate::from_latex_with(latex, &config).unwrap();
            let xml = parsed.to_mathml().unwrap();
            assert_eq!(
                xml,
                MathmlWriter::write(&parsed.expr(), &config).unwrap(),
                "{latex}"
            );
            xml
        }
        let choice = r"\mathchoice{D}{T}{S}{Q}";
        for (source, mode, expected) in [
            (choice, DisplayStyle::Block, "<mi>D</mi>"),
            (choice, DisplayStyle::Inline, "<mi>T</mi>"),
            (
                r"x^{\mathchoice{D}{T}{S}{Q}}",
                DisplayStyle::Block,
                "<msup><mi>x</mi><mi>S</mi></msup>",
            ),
            (
                r"x_{y^{\mathchoice{D}{T}{S}{Q}}}",
                DisplayStyle::Inline,
                "<msup><mi>y</mi><mi>Q</mi></msup>",
            ),
            (
                r"\frac{\mathchoice{D}{T}{S}{Q}}{y}",
                DisplayStyle::Block,
                "<mfrac><mi>T</mi><mi>y</mi>",
            ),
            (
                r"\sqrt[\mathchoice{D}{T}{S}{Q}]{z}",
                DisplayStyle::Block,
                "<mroot><mi>z</mi><mi>Q</mi></mroot>",
            ),
            (
                r"{\scriptstyle\mathchoice{D}{T}{S}{Q}}\mathchoice{D}{T}{S}{Q}",
                DisplayStyle::Block,
                r#"<mstyle displaystyle="false" scriptlevel="1"><mi>S</mi></mstyle><mi>D</mi>"#,
            ),
        ] {
            let written = xml(source, mode);
            assert!(written.contains(expected), "{source}: {written}");
        }
        let inline = xml(
            r"\mathchoice{D}{T}{S}{Q}\displaystyle\mathchoice{D}{T}{S}{Q}",
            DisplayStyle::Inline,
        );
        assert!(inline.contains(r#"display="inline"><mi>T</mi><mstyle displaystyle="true" scriptlevel="0"><mi>D</mi></mstyle>"#), "{inline}");
    }

    #[test]
    fn a_single_styled_cell_does_not_override_the_rest_of_a_table() {
        let parsed =
            crate::from_latex(r"x^{\begin{matrix}\scriptscriptstyle a & b\end{matrix}}").unwrap();
        let xml = parsed.to_mathml().unwrap();
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="0"><mtable data-mathema-array-environment="matrix">"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="2"><mi>a</mi></mstyle>"#),
            "{xml}"
        );
        assert_eq!(
            xml,
            MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap()
        );

        let xml = r#"<math><mtable scriptlevel="2"><mtr scriptlevel="-1"><mtd scriptlevel="+1"><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr></mtable><mi>z</mi></math>"#;
        let canonical = crate::from_mathml(xml).unwrap().to_mathml().unwrap();
        for (level, text) in [(2, 'a'), (1, 'b')] {
            assert!(
                canonical.contains(&format!(
                    r#"<mstyle displaystyle="false" scriptlevel="{level}"><mi>{text}</mi></mstyle>"#
                )),
                "{canonical}"
            );
        }
        assert!(
            canonical.ends_with("</mtable><mi>z</mi></math>"),
            "{canonical}"
        );
    }

    #[test]
    fn tables_reset_matrix_cells_to_text_size_inside_scripts() {
        let config = Config::default();
        for source in [
            r"x^{\begin{matrix}a&b\\c&d\end{matrix}}",
            r"x^{\begin{array}{c}a\end{array}}",
        ] {
            let result = crate::from_latex(source).unwrap();
            let heap = MathmlWriter::write(&result.expr(), &config).unwrap();
            let compact = MathmlWriter::write_compact(result.compact_expr(), &config).unwrap();
            assert_eq!(heap, compact, "heap/compact mismatch for {source}");
            assert!(
                heap.contains("<mstyle displaystyle=\"false\" scriptlevel=\"0\"><mtable"),
                "nested table did not reset to text size for {source}: {heap}"
            );
        }

        let aligned = crate::from_latex(
            r"x^{\begin{aligned}\mathchoice{D}{T}{S}{Q}&=\mathchoice{D}{T}{S}{Q}\end{aligned}}",
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            aligned.contains("<mtable displaystyle=\"true\">")
                && aligned.contains("<mi>D</mi>")
                && !aligned.contains("scriptlevel=\"0\""),
            "aligned table did not retain inherited script size: {aligned}"
        );
        let aligned_fraction =
            crate::from_latex(r"x^{\begin{aligned}\frac{a}{b}\end{aligned}}").unwrap();
        let aligned_fraction_xml = aligned_fraction.to_mathml().unwrap();
        let aligned_fraction_again = crate::from_mathml(&aligned_fraction_xml)
            .unwrap()
            .to_mathml()
            .unwrap();
        assert_eq!(aligned_fraction_xml, aligned_fraction_again);

        let top = crate::from_latex(r"\begin{matrix}a&b\end{matrix}")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(
            !top.contains("scriptlevel=\"0\""),
            "top-level matrix gained an unnecessary style wrapper: {top}"
        );

        let authored = crate::from_mathml(
            r#"<math><msup><mi>x</mi><mtable><mtr><mtd><mi>a</mi></mtd></mtr></mtable></msup></math>"#,
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            authored.contains("<msup><mi>x</mi><mtable>")
                && !authored.contains("scriptlevel=\"0\""),
            "an unstyled MathML table inherits its surrounding script level: {authored}"
        );

        let explicit = crate::from_mathml(
            r#"<math><msup><mi>x</mi><mstyle scriptlevel="0"><mtable><mtr><mtd><mi>a</mi></mtd></mtr></mtable></mstyle></msup></math>"#,
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            explicit.contains("<mstyle displaystyle=\"false\" scriptlevel=\"0\"><mtable>"),
            "explicit table scriptlevel was not preserved: {explicit}"
        );
    }

    #[test]
    fn forced_fraction_styles_match_explicit_switches_in_scripts() {
        for (command, declaration, child) in
            [("dfrac", "displaystyle", "T"), ("tfrac", "textstyle", "S")]
        {
            let choice = r"\mathchoice{D}{T}{S}{Q}";
            let fraction = format!(r"x_{{\{command}{{{choice}}}{{{choice}}}}}");
            let declared = format!(r"x_{{\{declaration}\frac{{{choice}}}{{{choice}}}}}");
            let expected = crate::from_latex(&fraction).unwrap().to_mathml().unwrap();
            let actual = crate::from_latex(&declared).unwrap().to_mathml().unwrap();
            assert_eq!(expected, actual);
            assert!(actual.contains(&format!("<mfrac><mi>{child}</mi><mi>{child}</mi></mfrac>")));
            assert_eq!(
                crate::from_mathml(&actual).unwrap().to_mathml().unwrap(),
                actual
            );
        }
    }

    #[test]
    fn named_operators_employ_the_requested_mathml_limit_style() {
        for (style, start) in [
            (
                LimitStyle::Default,
                r#"<munder><mo movablelimits="true">lim</mo>"#,
            ),
            (
                LimitStyle::AboveBelow,
                r#"<munder><mo movablelimits="false">lim</mo>"#,
            ),
            (LimitStyle::SubSup, "<msub><mo>lim</mo>"),
        ] {
            let expr = MathExpr::block(vec![MathNode::Subscript {
                base: Box::new(MathNode::Operator(OperatorAttr {
                    name: "lim".into(),
                    limit_style: style,
                })),
                sub: Box::new(MathNode::Identifier("i".into())),
            }]);
            let compact = CompactExpr::from_math_expr(&expr).unwrap();
            let xml = MathmlWriter::write(&expr, &Config::default()).unwrap();
            assert!(xml.contains(start), "{xml}");
            assert_eq!(
                MathmlWriter::write_compact(&compact, &Config::default()).unwrap(),
                xml
            );
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn compound_operator_limits_follow_their_local_math_style() {
        let operator = || MathNode::Underscript {
            base: Box::new(MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                content: Box::new(MathNode::Row(vec![
                    MathNode::Identifier("a".into()),
                    MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                    MathNode::Identifier("b".into()),
                ])),
            }),
            under: Box::new(MathNode::Identifier("i".into())),
            movable_limits: true,
        };
        let nested = MathNode::Superscript {
            base: Box::new(MathNode::Identifier("z".into())),
            sup: Box::new(operator()),
        };
        for (expr, tag) in [
            (MathExpr::block(vec![operator()]), "munder"),
            (MathExpr::inline(vec![operator()]), "msub"),
            (MathExpr::block(vec![nested]), "msub"),
        ] {
            let config = Config::default();
            let xml = MathmlWriter::write(&expr, &config).unwrap();
            let fragment = format!(
                r#"<{tag}><mrow data-mjx-texclass="OP" data-mathema-limits="movable"><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mi>i</mi></{tag}>"#
            );
            assert!(xml.contains(&fragment), "{xml}");
            let packed = CompactExpr::from_math_expr(&expr).unwrap();
            assert_eq!(MathmlWriter::write_compact(&packed, &config).unwrap(), xml);
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn mathml_retains_open_and_close_spacing_classes_around_a_fence() {
        use crate::reader::{Reader, latex::LatexReader};

        let source = r"x+\mathopen{}\mathclose{\left(a+b\right)}_i y";
        let cfg = Config::default();
        let heap = LatexReader::parse(source, &cfg).unwrap().expr;
        let compact = LatexReader::parse_compact(source, &cfg).unwrap().expr;
        let xml = MathmlWriter::write_compact(&compact, &cfg).unwrap();
        assert_eq!(MathmlWriter::write(&heap, &cfg).unwrap(), xml);
        assert!(xml.contains(r#"<mrow data-mjx-texclass="OPEN"></mrow>"#));
        assert!(xml.contains(r#"<mrow data-mjx-texclass="CLOSE">"#));
        let short = xml.replace(
            r#"<mrow data-mjx-texclass="OPEN"></mrow>"#,
            r#"<mrow data-mjx-texclass="OPEN"/>"#,
        );
        for input in [&xml, &short] {
            let again = crate::from_mathml(input).unwrap();
            assert_eq!(again.to_mathml().unwrap(), xml);
            assert_eq!(
                again.to_latex().unwrap(),
                crate::from_latex(source).unwrap().to_latex().unwrap()
            );
            #[cfg(feature = "render")]
            assert_eq!(
                again.to_svg().unwrap(),
                crate::from_latex(source).unwrap().to_svg().unwrap()
            );
        }
    }

    #[test]
    fn constructed_smallmatrix_selects_scriptstyle_without_an_explicit_switch() {
        let leaf = |name: &str| Box::new(MathNode::Identifier(name.into()));
        let cell = MathNode::MathChoice {
            display: leaf("D"),
            text: leaf("T"),
            script: leaf("S"),
            script_script: leaf("Q"),
        };
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::SmallMatrix,
            alignment: ColumnAlign::Right,
            rows: vec![vec![vec![cell]]],
        }]);
        let cfg = Config::default();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let xml = MathmlWriter::write(&expr, &cfg).unwrap();
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="1"><mi>S</mi></mstyle>"#),
            "{xml}"
        );
        assert_eq!(MathmlWriter::write_compact(&compact, &cfg).unwrap(), xml);
        assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
    }

    #[test]
    fn nary_operands_stay_inside_fixed_arity_mathml_arguments() {
        for (source, before, after) in [
            (
                r"\frac{\sum_i^n x}{2}",
                "<mfrac><mrow><munderover>",
                "</munderover><mi>x</mi></mrow><mn>2</mn></mfrac>",
            ),
            (
                r"y^{\prod_k^n k}",
                "<msup><mi>y</mi><mrow><munderover>",
                "</munderover><mi>k</mi></mrow></msup>",
            ),
            (
                r"\sqrt[\sum_i i]{z}",
                "<mroot><mi>z</mi><mrow><munder>",
                "</munder><mi>i</mi></mrow></mroot>",
            ),
            (
                r"\overset{2}{\sum_i x}",
                "<mrow><munder>",
                "</munder><mi>x</mi></mrow><mn>2</mn></mover>",
            ),
            (
                r"\frac{a}{\int_0^1 x}",
                "<mfrac><mi>a</mi><mrow><msubsup>",
                "</msubsup><mi>x</mi></mrow></mfrac>",
            ),
        ] {
            let parsed = crate::from_latex(source).unwrap();
            for xml in [
                parsed.to_mathml().unwrap(),
                MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap(),
            ] {
                assert!(
                    xml.contains(before) && xml.contains(after),
                    "{source}: {xml}"
                );
                assert_eq!(
                    crate::from_mathml(&xml).unwrap().to_mathml().unwrap(),
                    xml,
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn default_nary_limits_use_movable_under_over_mathml() {
        let config = Config::default();
        let default_sum = MathExpr::block(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let compact = CompactExpr::from_math_expr(&default_sum).unwrap();
        for output in [
            MathmlWriter::write(&default_sum, &config).unwrap(),
            MathmlWriter::write_compact(&compact, &config).unwrap(),
        ] {
            assert!(
                output.contains("<munderover><mo movablelimits=\"true\">∑</mo>"),
                "default sum limits were not encoded as movable under/over: {output}"
            );
            assert!(
                !output.contains("<msubsup>"),
                "default sum used fixed scripts: {output}"
            );
            assert_eq!(
                crate::convert(&output, crate::Format::MathML, crate::Format::MathML).unwrap(),
                output,
                "movable default limits lost on MathML import"
            );
        }

        let default_integral = MathExpr::block(vec![MathNode::Nary {
            op: '∫',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: Some(Box::new(MathNode::Number("1".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let output = MathmlWriter::write(&default_integral, &config).unwrap();
        assert!(output.contains("<msubsup><mo movablelimits=\"false\">∫</mo>"));

        let forced = MathExpr::inline(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let forced_output = MathmlWriter::write(&forced, &config).unwrap();
        assert!(forced_output.contains("<msubsup><mo movablelimits=\"false\">∑</mo>"));

        let above_below = MathExpr::block(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::AboveBelow,
        }]);
        let above_below_output = MathmlWriter::write(&above_below, &config).unwrap();
        assert!(above_below_output.contains("<munderover><mo movablelimits=\"false\">∑</mo>"));
        assert_eq!(
            crate::convert(
                &above_below_output,
                crate::Format::MathML,
                crate::Format::MathML
            )
            .unwrap(),
            above_below_output,
            "forced limits lost on MathML import"
        );
    }

    #[test]
    fn compact_boxed_style_escapes_attribute_values() {
        let expr = MathExpr::inline(vec![MathNode::Boxed {
            content: Box::new(MathNode::Identifier("x".into())),
            bg_color: Some("red&blue\"".into()),
            border_color: Some("black&white\"".into()),
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("black&amp;white&quot;"),
            "compact output: {output}"
        );
        assert!(
            output.contains("red&amp;blue&quot;"),
            "compact output: {output}"
        );
        assert!(
            !output.contains("black&white\""),
            "compact output: {output}"
        );
    }

    #[test]
    fn compact_mathcolor_escapes_attribute_values() {
        let mut arena = AstArena::new();
        let color = arena.push_str("red&blue\"");
        let x = arena.push_str("x");
        let content = arena.push_node(CompactNode::Identifier(x));
        let root = arena.push_node(CompactNode::Color { color, content });
        let roots = arena.push_children(&[root]).unwrap();
        let compact = CompactExpr::new(arena, roots, DisplayStyle::Inline);
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("mathcolor=\"red&amp;blue&quot;\""),
            "compact output: {output}"
        );
        assert!(
            !output.contains("mathcolor=\"red&blue\"\""),
            "compact output: {output}"
        );
    }

    #[test]
    fn big_delimiters_are_written_as_mathjax_sized_operators() {
        let mathml = crate::convert(
            r"a\bigl( b\bigr) \Bigm| \bigg\| \Biggl.",
            crate::Format::LaTeX,
            crate::Format::MathML,
        )
        .unwrap();
        for expected in [
            r#"<mrow data-mjx-texclass="OPEN"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">(</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="CLOSE"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">)</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="REL"><mo minsize="1.623em" maxsize="1.623em" fence="true" stretchy="true" symmetric="true">|</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="ORD"><mo minsize="2.047em" maxsize="2.047em" fence="true" stretchy="true" symmetric="true">‖</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="OPEN"><mo minsize="2.470em" maxsize="2.470em" fence="true" stretchy="true" symmetric="true"></mo></mrow>"#,
        ] {
            assert!(mathml.contains(expected), "missing {expected} in {mathml}");
        }
        // The heap writer agrees with the compact writer.
        let parsed = crate::from_latex(r"a\bigl( b\bigr) \Bigm| \bigg\| \Biggl.").unwrap();
        let heap = MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap();
        assert_eq!(heap, mathml);
        // Sized delimiters and their classes round-trip without a loss diagnostic.
        let (_, diagnostics) = parsed.to_mathml_with_diagnostics().unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
