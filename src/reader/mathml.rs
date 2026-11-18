use quick_xml::Reader;
use quick_xml::events::{BytesText, Event};
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader as MathReader;

mod content;

pub struct MathmlReader;

impl MathReader for MathmlReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        config.check_input_size(input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                config.display,
            )));
        }
        let parser_result = MathmlCompactParser::new(trimmed);
        let mut parser = match parser_result {
            Ok(p) => p,
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    return Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, config.display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MathML initialization failed (recovered): {e}"),
                            span: None,
                        }],
                    ));
                }
                return Err(e);
            },
        };
        match parser.parse_root() {
            Ok(ids) => {
                let display = parser.display.unwrap_or(config.display);
                parser.arena.check_limits()?;
                let roots = parser.arena.push_children(&ids)?;
                Ok(CompactParseResult::new(CompactExpr {
                    arena: parser.arena,
                    roots,
                    display,
                }))
            },
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let display = parser.display.unwrap_or(config.display);
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr {
                            arena: parser.arena,
                            roots: NodeSpan::EMPTY,
                            display,
                        },
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MathML parse error (recovered): {e}"),
                            span: None,
                        }],
                    ))
                } else {
                    Err(e)
                }
            },
        }
    }
}

const MAX_PARSE_DEPTH: u32 = 128;

struct MathmlCompactParser<'a> {
    reader: Reader<&'a [u8]>,
    arena: AstArena,
    display: Option<DisplayStyle>,
    depth: u32,
    movable_limit_operators: HashMap<NodeId, bool>,
}

impl<'a> MathmlCompactParser<'a> {
    fn new(input: &'a str) -> Result<Self> {
        let mut reader = Reader::from_str(input);
        // Keep token text bytes intact so token-level MathML whitespace can be
        // normalized by `read_text_content`. Formatting whitespace between
        // token elements is still ignored by `parse_children` and
        // `collect_remaining`.
        reader.config_mut().trim_text(false);
        // Heuristic: MathML is verbose, roughly 1 node per 15 input bytes
        let est_nodes = input.len() / 15 + 4;
        Ok(Self {
            reader,
            arena: AstArena::with_capacity(est_nodes, est_nodes, input.len() / 4),
            display: None,
            depth: 0,
            movable_limit_operators: HashMap::new(),
        })
    }

    #[inline]
    fn local_name(start: &quick_xml::events::BytesStart<'_>) -> String {
        let qname = start.name();
        qname.local_name().as_ref().to_owned()
    }

    #[inline]
    fn local_name_end(end: &quick_xml::events::BytesEnd<'_>) -> String {
        let qname = end.name();
        qname.local_name().as_ref().to_owned()
    }

    fn parse_root(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "math" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "display" {
                                self.display = Some(if attr.value.as_ref() == "block" {
                                    DisplayStyle::Block
                                } else {
                                    DisplayStyle::Inline
                                });
                            }
                        }
                        return self.parse_children("math");
                    } else {
                        let id = self.parse_element_by_name(&name, &e)?;
                        let mut nodes: SmallVec<[NodeId; 8]> = smallvec::smallvec![id];
                        nodes.extend(self.collect_remaining()?);
                        return Ok(nodes);
                    }
                },
                Ok(Event::Eof) => return Ok(SmallVec::new()),
                Ok(_) => continue,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error: {e}"),
                    });
                },
            }
        }
    }

    fn collect_remaining(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        let mut nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = unescape_text(&e);
                    if !text.trim().is_empty() {
                        nodes.push(self.classify_text_compact(&text));
                    }
                },
                Ok(Event::GeneralRef(r)) => {
                    let text = resolve_general_ref(&r);
                    if !text.trim().is_empty() {
                        nodes.push(self.classify_text_compact(&text));
                    }
                },
                Ok(Event::Eof) | Ok(Event::End(_)) => break,
                _ => continue,
            }
        }
        Ok(nodes)
    }

    fn parse_children(&mut self, parent_tag: &str) -> Result<SmallVec<[NodeId; 8]>> {
        self.read_children(parent_tag, false)
            .map(|(children, _)| children)
    }

    /// A sole mstyle child can set an enclosure's body to text size. Record its
    /// actual presentation attributes independently of the child's font variant.
    fn read_children(
        &mut self,
        parent_tag: &str,
        inspect_style: bool,
    ) -> Result<(SmallVec<[NodeId; 8]>, bool)> {
        let mut body_in_text_style = false;
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            return Err(Error::MathmlParse {
                message: format!(
                    "maximum nesting depth ({MAX_PARSE_DEPTH}) exceeded in <{parent_tag}>"
                ),
            });
        }
        let mut nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if inspect_style && nodes.is_empty() && name == "mstyle" {
                        body_in_text_style = has_explicit_text_size(&e);
                    }
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    if inspect_style && nodes.is_empty() && name == "mstyle" {
                        body_in_text_style = has_explicit_text_size(&e);
                    }
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = unescape_text(&e);
                    if !text.trim().is_empty() {
                        nodes.push(self.classify_text_compact(&text));
                    }
                },
                Ok(Event::GeneralRef(r)) => {
                    let text = resolve_general_ref(&r);
                    if !text.trim().is_empty() {
                        nodes.push(self.classify_text_compact(&text));
                    }
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error in <{parent_tag}>: {e}"),
                    });
                },
                _ => continue,
            }
        }
        self.depth -= 1;
        let sized = body_in_text_style && nodes.len() == 1;
        Ok((nodes, sized))
    }

    fn parse_child_id(&mut self, parent_tag: &str) -> Result<NodeId> {
        let children = self.parse_children(parent_tag)?;
        Ok(self.flatten_ids(&children))
    }

    /// Read the next XML event, mapping reader errors to our error type.
    /// Shared by the Content MathML lowering in the `content` submodule.
    fn reader_read_event(&mut self) -> Result<Event<'static>> {
        match self.reader.read_event() {
            Ok(ev) => Ok(ev.into_owned()),
            Err(e) => Err(Error::MathmlParse {
                message: format!("XML error: {e}"),
            }),
        }
    }

    /// Increment the nesting-depth guard, erroring if it is exceeded.
    fn bump_depth(&mut self, tag: &str) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            return Err(Error::MathmlParse {
                message: format!("maximum nesting depth ({MAX_PARSE_DEPTH}) exceeded in <{tag}>"),
            });
        }
        Ok(())
    }

    fn unbump_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Consume events until the matching end tag of `tag` (for elements whose
    /// content we don't need, e.g. operator tokens that unexpectedly have a body).
    fn skip_element(&mut self, tag: &str) -> Result<()> {
        let mut depth = 1usize;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) if Self::local_name(&e) == tag => depth += 1,
                Event::End(e) if Self::local_name_end(&e) == tag => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                },
                Event::Eof => break,
                _ => {},
            }
        }
        Ok(())
    }

    fn flatten_ids(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children_deferred(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    fn empty_row(&mut self) -> NodeId {
        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
    }

    fn two_child_ids(&mut self, mut children: SmallVec<[NodeId; 8]>) -> (NodeId, NodeId) {
        while children.len() < 2 {
            children.push(self.empty_row());
        }
        (children[0], children[1])
    }

    fn three_child_ids(&mut self, mut children: SmallVec<[NodeId; 8]>) -> (NodeId, NodeId, NodeId) {
        while children.len() < 3 {
            children.push(self.empty_row());
        }
        (children[0], children[1], children[2])
    }

    fn classify_text_compact(&mut self, text: &str) -> NodeId {
        let trimmed = text.trim();
        let sid = self.arena.push_str(trimmed);
        if trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') && !trimmed.is_empty() {
            self.arena.push_node(CompactNode::Number(sid))
        } else if trimmed.len() == 1 && trimmed.starts_with(|c: char| c.is_alphabetic()) {
            self.arena.push_node(CompactNode::Identifier(sid))
        } else {
            self.arena
                .push_node(CompactNode::Text(TextStyle::Normal, sid))
        }
    }

    fn parse_element_by_name(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<NodeId> {
        match name {
            "mn" => {
                let text = self.read_text_content(name)?;
                let sid = self.arena.push_str(&text);
                Ok(self.arena.push_node(CompactNode::Number(sid)))
            },
            "mi" => {
                let mathvariant = get_attr(start, "mathvariant");
                let text = self.read_text_content(name)?;
                let sid = self.arena.push_str(&text);
                if let Some(variant) = mathvariant
                    && let Some(style) = mathvariant_to_style(&variant)
                    && style != TextStyle::Italic
                {
                    let inner = self.arena.push_node(CompactNode::Identifier(sid));
                    let span = self.arena.push_children_deferred(&[inner]);
                    return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
                }
                Ok(self.arena.push_node(CompactNode::Identifier(sid)))
            },
            "mo" => {
                let text = self.read_text_content(name)?;
                let kind = classify_mo(&text);
                let name = text.trim();
                // Multi-letter operator tokens and explicitly named operators
                // are distinct from ordinary identifiers in <mi>.
                let movable = match get_attr(start, "movablelimits").as_deref() {
                    Some("true") => Some(true),
                    Some("false") => Some(false),
                    _ => None,
                };
                let mut chars = name.chars();
                let named = get_attr(start, "data-mathema-operator").as_deref() == Some("name")
                    || kind == SymbolKind::Ordinary
                        && chars.next().is_some_and(|first| {
                            first.is_alphabetic()
                                && (movable.is_some()
                                    || !chars.as_str().is_empty() && chars.all(char::is_alphabetic))
                                || get_attr(start, "data-mjx-texclass").as_deref() == Some("OP")
                        });
                let sid = self.arena.push_str(if named { name } else { &text });
                let id = if named {
                    let limits = match movable {
                        Some(true) => LimitStyle::Default,
                        Some(false) => LimitStyle::AboveBelow,
                        None if crate::unicode::symbols::operator_has_limits(name) => {
                            LimitStyle::Default
                        },
                        None => LimitStyle::SubSup,
                    };
                    self.arena
                        .push_node(CompactNode::Operator { name: sid, limits })
                } else {
                    self.arena.push_node(CompactNode::Symbol(kind, sid))
                };
                if (named || kind == SymbolKind::LargeOp)
                    && let Some(value) = movable
                {
                    self.movable_limit_operators.insert(id, value);
                }
                Ok(id)
            },
            "mtext" => {
                let mathvariant = get_attr(start, "mathvariant")
                    .and_then(|variant| mathvariant_to_style(&variant))
                    .unwrap_or(TextStyle::Normal);
                let text = self.read_text_content(name)?;
                let sid = self.arena.push_str(&text);
                Ok(self.arena.push_node(CompactNode::Text(mathvariant, sid)))
            },
            "ms" => {
                // String literal: render content wrapped in quotes.
                let lquote = get_attr(start, "lquote").unwrap_or_else(|| "\"".into());
                let rquote = get_attr(start, "rquote").unwrap_or_else(|| "\"".into());
                let text = self.read_text_content(name)?;
                let quoted = format!("{lquote}{text}{rquote}");
                let sid = self.arena.push_str(&quoted);
                Ok(self
                    .arena
                    .push_node(CompactNode::Text(TextStyle::Normal, sid)))
            },
            "mglyph" => {
                // Glyph by font reference; we cannot load arbitrary fonts, so
                // fall back to the `alt` text (or the referenced character).
                let alt = get_attr(start, "alt").unwrap_or_default();
                self.skip_element(name)?;
                let sid = self.arena.push_str(&alt);
                Ok(self
                    .arena
                    .push_node(CompactNode::Text(TextStyle::Normal, sid)))
            },
            "mspace" => {
                let width = get_attr(start, "width").unwrap_or_default();
                let r = parse_em_width(&width);
                Ok(self.arena.push_node(CompactNode::Space(r)))
            },
            "mrow" => {
                let mode = get_attr(start, "data-mathema-reflect");
                let reflected = match get_attr(start, "style") {
                    Some(style) => has_horizontal_reflection(&style),
                    None => matches!(mode.as_deref(), Some("text" | "math")),
                };
                let (children, sized) = self.read_children(name, reflected)?;
                if reflected {
                    let content = self.flatten_ids(&children);
                    let text_mode =
                        mode.as_deref() == Some("text") || sized || has_explicit_text_size(start);
                    Ok(self
                        .arena
                        .push_node(CompactNode::Reflect { content, text_mode }))
                } else if get_attr(start, "data-mjx-texclass").as_deref() == Some("OP") {
                    let content = self.flatten_ids(&children);
                    let op = self.arena.push_node(CompactNode::MathAtom {
                        atom_type: MathAtomType::Op,
                        content,
                    });
                    if get_attr(start, "data-mathema-limits").as_deref() == Some("movable") {
                        self.movable_limit_operators.insert(op, true);
                    }
                    Ok(op)
                } else {
                    let span = self.arena.push_children_deferred(&children);
                    Ok(self.arena.push_node(CompactNode::Row(span)))
                }
            },
            "mfrac" => {
                let children = self.parse_children(name)?;
                let (num, den) = self.two_child_ids(children);
                let kind = if get_attr(start, "linethickness").as_deref() == Some("0") {
                    FractionKind::NoBar
                } else {
                    FractionKind::Normal
                };
                Ok(self
                    .arena
                    .push_node(CompactNode::Fraction { kind, num, den }))
            },
            "msqrt" => {
                let id = self.parse_child_id(name)?;
                Ok(self.arena.push_node(CompactNode::Sqrt(id)))
            },
            "mroot" => {
                let children = self.parse_children(name)?;
                let (radicand, index) = self.two_child_ids(children);
                Ok(self.arena.push_node(CompactNode::Root { index, radicand }))
            },
            "msub" => {
                let children = self.parse_children(name)?;
                let (base, sub) = self.two_child_ids(children);
                if let Some(node) = self.scripted_operator(base, sub, NodeId::NONE, false) {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::Subscript { base, sub }))
            },
            "msup" => {
                let children = self.parse_children(name)?;
                let (base, sup) = self.two_child_ids(children);
                if let Some(node) = self.scripted_operator(base, NodeId::NONE, sup, false) {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::Superscript { base, sup }))
            },
            "msubsup" => {
                let children = self.parse_children(name)?;
                let (base, sub, sup) = self.three_child_ids(children);
                if let Some(node) = self.scripted_operator(base, sub, sup, false) {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::SubSup { base, sub, sup }))
            },
            "mover" => {
                let movable = get_attr(start, "accent").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, over) = self.two_child_ids(children);
                // An explicitly accented mover may decorate a named or
                // compound operator just like an ordinary base. Keep a
                // genuine one-character accent typed so the MathML writer
                // emits `accent="true"` instead of treating it as a fixed
                // operator limit.
                if let CompactNode::Symbol(_, sid) = *self.arena.node(over)
                    && let Some(ch) = single_accent_char(self.arena.str_val(sid))
                    && (get_attr(start, "accent").as_deref() == Some("true") || is_accent_glyph(ch))
                {
                    return Ok(self.arena.push_node(CompactNode::Accent {
                        base,
                        accent: ch,
                        position: AccentPosition::Above,
                        wide: false,
                    }));
                }
                if movable
                    && let Some(node) = self.scripted_operator(base, NodeId::NONE, over, true)
                {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::Overscript {
                    base,
                    over,
                    movable_limits: movable,
                }))
            },
            "munder" => {
                let is_accent = get_attr(start, "accentunder").as_deref() == Some("true");
                let children = self.parse_children(name)?;
                let (base, under) = self.two_child_ids(children);
                if is_accent
                    && let CompactNode::Symbol(_, sid) = *self.arena.node(under)
                    && let Some(ch) = single_accent_char(self.arena.str_val(sid))
                {
                    return Ok(self.arena.push_node(CompactNode::Accent {
                        base,
                        accent: ch,
                        position: AccentPosition::Below,
                        wide: false,
                    }));
                }
                if !is_accent
                    && let CompactNode::Symbol(_, sid) = *self.arena.node(under)
                    && let Some(ch) = single_accent_char(self.arena.str_val(sid))
                    && is_accent_glyph(ch)
                {
                    return Ok(self.arena.push_node(CompactNode::Accent {
                        base,
                        accent: ch,
                        position: AccentPosition::Below,
                        wide: false,
                    }));
                }
                if !is_accent
                    && let Some(node) = self.scripted_operator(base, under, NodeId::NONE, true)
                {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::Underscript {
                    base,
                    under,
                    movable_limits: !is_accent,
                }))
            },
            "munderover" => {
                let accent = get_attr(start, "accent").as_deref() == Some("true");
                let accent_under = get_attr(start, "accentunder").as_deref() == Some("true");
                let children = self.parse_children(name)?;
                let (base, under, over) = self.three_child_ids(children);
                // `munderover` has independent `accent` and `accentunder`
                // flags. The public AST stores one accent flag per wrapper,
                // so preserve both flags as nested wrappers when either is
                // present. This also prevents an explicitly movable integral
                // from being collapsed into a default TeX n-ary node.
                if accent || accent_under {
                    let inner = self.arena.push_node(CompactNode::Underscript {
                        base,
                        under,
                        movable_limits: !accent_under,
                    });
                    return Ok(self.arena.push_node(CompactNode::Overscript {
                        base: inner,
                        over,
                        movable_limits: !accent,
                    }));
                }
                if let Some(node) = self.scripted_operator(base, under, over, true) {
                    return Ok(node);
                }
                Ok(self.arena.push_node(CompactNode::UnderOver {
                    base,
                    under,
                    over,
                    movable_limits: true,
                }))
            },
            "mfenced" => {
                let open = get_attr(start, "open").unwrap_or_else(|| "(".into());
                let close = get_attr(start, "close").unwrap_or_else(|| ")".into());
                let children = self.parse_children(name)?;
                let content = self.arena.push_children_deferred(&children);
                let open_sid = self.arena.push_str(&open);
                let close_sid = self.arena.push_str(&close);
                Ok(self.arena.push_node(CompactNode::Fenced {
                    open: open_sid,
                    close: close_sid,
                    content,
                }))
            },
            "mtable" => {
                let rows = self.parse_table(name)?;
                let ncols = rows.first().map(|r| r.len()).unwrap_or(0);
                let num_cols = ncols.max(1) as u16;

                let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
                for _ in 0..ncols {
                    align_ids.push(
                        self.arena
                            .push_node(CompactNode::ColAlign(ColumnAlign::Center)),
                    );
                }
                let col_aligns = self.arena.push_children_deferred(&align_ids);

                let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
                for row in &rows {
                    for cell_children in row {
                        let span = self.arena.push_children_deferred(cell_children);
                        cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
                    }
                    // Pad short rows
                    for _ in row.len()..(num_cols as usize) {
                        cell_ids.push(self.empty_row());
                    }
                }
                let cells = self.arena.push_children_deferred(&cell_ids);

                Ok(self.arena.push_node(CompactNode::Array {
                    col_aligns,
                    cells,
                    num_cols,
                    arraystretch_pct: 100,
                    add_jot: false,
                    array_style: false,
                    col_seps: 0,
                    row_seps: 0,
                }))
            },
            "mphantom" => {
                let id = self.parse_child_id(name)?;
                Ok(self
                    .arena
                    .push_node(CompactNode::Phantom(id, PhantomKind::Full)))
            },
            "menclose" => {
                let notation = get_attr(start, "notation").unwrap_or_default();
                let (children, sized) = self.read_children(name, true)?;
                let mut id = self.flatten_ids(&children);
                let text_mode = sized || has_explicit_text_size(start);
                let has = |mark: &str| notation.split_ascii_whitespace().any(|n| n == mark);
                let diagonal = match (has("updiagonalstrike"), has("downdiagonalstrike")) {
                    (true, true) => Some(StrokeKind::Cross),
                    (true, false) => Some(StrokeKind::Forward),
                    (false, true) => Some(StrokeKind::Backward),
                    (false, false) => None,
                };
                let strokes = [
                    diagonal,
                    has("horizontalstrike").then_some(StrokeKind::Horizontal),
                    has("verticalstrike").then_some(StrokeKind::Vertical),
                ];
                let has_stroke = strokes.iter().any(Option::is_some);
                for stroke in strokes.into_iter().flatten() {
                    id = self.arena.push_node(CompactNode::Cancel {
                        stroke,
                        content: id,
                    });
                }
                let top_right = has("actuarial") || (has("top") && has("right"));
                let full_box =
                    has("box") || has("roundedbox") || (top_right && has("bottom") && has("left"));
                if top_right && !full_box {
                    Ok(self.arena.push_node(CompactNode::Actuarial {
                        content: id,
                        text_mode,
                    }))
                } else if full_box || !has_stroke {
                    let empty = self.arena.push_str("");
                    Ok(self.arena.push_node(CompactNode::Boxed {
                        content: id,
                        bg_color: empty,
                        border_color: empty,
                    }))
                } else {
                    Ok(id)
                }
            },
            "mstyle" => {
                let mathvariant = get_attr(start, "mathvariant");
                let children = self.parse_children(name)?;
                if let Some(variant) = mathvariant
                    && let Some(style) = mathvariant_to_style(&variant)
                {
                    let span = self.arena.push_children_deferred(&children);
                    return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
                }
                Ok(self.flatten_ids(&children))
            },
            "mpadded" => self.parse_child_id(name),
            "mmultiscripts" => self.parse_multiscripts_element(name),
            "semantics" => {
                let children = self.parse_children(name)?;
                if let Some(&first) = children.first() {
                    Ok(first)
                } else {
                    Ok(self.empty_row())
                }
            },
            "annotation" | "annotation-xml" => {
                let _text = self.read_text_content(name)?;
                Ok(self.empty_row())
            },
            _ => {
                if content::is_content_element(name) {
                    return self.parse_content_element(name, start, false);
                }
                let children = self.parse_children(name)?;
                Ok(self.flatten_ids(&children))
            },
        }
    }

    fn parse_empty_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<NodeId> {
        match name {
            "mspace" => {
                let width = get_attr(start, "width").unwrap_or_default();
                let r = parse_em_width(&width);
                Ok(self.arena.push_node(CompactNode::Space(r)))
            },
            "mprescripts" => Ok(self.empty_row()),
            "none" => Ok(self.empty_row()),
            "mglyph" => {
                let alt = get_attr(start, "alt").unwrap_or_default();
                let sid = self.arena.push_str(&alt);
                Ok(self
                    .arena
                    .push_node(CompactNode::Text(TextStyle::Normal, sid)))
            },
            _ => {
                if content::is_content_element(name) {
                    return self.parse_content_element(name, start, true);
                }
                Ok(self.empty_row())
            },
        }
    }

    fn read_text_content(&mut self, tag_name: &str) -> Result<String> {
        self.read_text_content_with_space(tag_name)
    }

    fn read_text_content_with_space(&mut self, tag_name: &str) -> Result<String> {
        let mut text = String::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Text(e)) => {
                    text.push_str(&unescape_text(&e));
                },
                Ok(Event::GeneralRef(r)) => {
                    text.push_str(&resolve_general_ref(&r));
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == tag_name {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) => {
                    // A nested `<mglyph/>` contributes its `alt` text.
                    if Self::local_name(&e) == "mglyph"
                        && let Some(alt) = get_attr(&e, "alt")
                    {
                        text.push_str(&alt);
                    }
                },
                Ok(Event::Start(e)) => {
                    // Nested element inside text content — capture mglyph alt,
                    // otherwise skip its body.
                    let n = Self::local_name(&e);
                    if n == "mglyph"
                        && let Some(alt) = get_attr(&e, "alt")
                    {
                        text.push_str(&alt);
                    }
                    let _ = self.skip_element(&n);
                },
                _ => continue,
            }
        }
        Ok(collapse_mathml_token_text(&text))
    }

    fn parse_table(&mut self, parent_tag: &str) -> Result<Vec<Vec<SmallVec<[NodeId; 8]>>>> {
        let mut rows = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mtr" || name == "mlabeledtr" {
                        rows.push(self.parse_table_row(&name)?);
                    }
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(rows)
    }

    fn parse_table_row(&mut self, parent_tag: &str) -> Result<Vec<SmallVec<[NodeId; 8]>>> {
        let mut cells = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mtd" {
                        cells.push(self.parse_children("mtd")?);
                    } else {
                        // Inferred cell: a non-<mtd> child is its own cell.
                        let id = self.parse_element_by_name(&name, &e)?;
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    let id = self.parse_empty_element(&name, &e)?;
                    cells.push(smallvec::smallvec![id]);
                },
                Ok(Event::Text(e)) => {
                    let text = unescape_text(&e);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::GeneralRef(r)) => {
                    let text = resolve_general_ref(&r);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(cells)
    }

    fn is_empty_row_id(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    /// Script elements place their bounds at the side. Under/over elements
    /// use the core <mo>'s movablelimits value, including through a singleton
    /// row. Named <mo> tokens default to fixed stacking in native MathML.
    fn scripted_operator(
        &mut self,
        mut base: NodeId,
        sub: NodeId,
        sup: NodeId,
        stacked: bool,
    ) -> Option<NodeId> {
        while let CompactNode::Row(span) = *self.arena.node(base) {
            let &[core] = self.arena.children(span) else {
                return None;
            };
            base = core;
        }
        match *self.arena.node(base) {
            CompactNode::Operator { name, .. } => {
                let limits = if !stacked {
                    LimitStyle::SubSup
                } else if self.movable_limit_operators.get(&base) == Some(&true) {
                    LimitStyle::Default
                } else {
                    LimitStyle::AboveBelow
                };
                *self.arena.node_mut(base) = CompactNode::Operator { name, limits };
                Some(self.make_script_pair(base, sub, sup))
            },
            CompactNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => {
                let movable = self.movable_limit_operators.get(&base) == Some(&true);
                if !stacked && !movable {
                    return Some(self.make_script_pair(base, sub, sup));
                }
                // Native MathML needs separate tags in compact and display
                // styles when a compound base has no core <mo>.
                let node = match (sub.is_some(), sup.is_some()) {
                    (true, true) => CompactNode::UnderOver {
                        base,
                        under: sub,
                        over: sup,
                        movable_limits: movable,
                    },
                    (true, false) => CompactNode::Underscript {
                        base,
                        under: sub,
                        movable_limits: movable,
                    },
                    (false, true) => CompactNode::Overscript {
                        base,
                        over: sup,
                        movable_limits: movable,
                    },
                    (false, false) => return Some(base),
                };
                Some(self.arena.push_node(node))
            },
            CompactNode::Symbol(SymbolKind::LargeOp, sid) => {
                let text = self.arena.str_val(sid).trim();
                let op = text.chars().next()?;
                if text.len() != op.len_utf8() {
                    return None;
                }
                let integral = crate::unicode::symbols::is_integral_operator(op);
                let movable = self
                    .movable_limit_operators
                    .get(&base)
                    .copied()
                    .unwrap_or(!integral);
                let limits = if !stacked {
                    LimitStyle::SubSup
                } else if !movable {
                    LimitStyle::AboveBelow
                } else if integral {
                    // The generic MathML under/over node can express an
                    // explicitly movable integral, unlike a default TeX n-ary.
                    return None;
                } else {
                    LimitStyle::Default
                };
                Some(self.make_nary(op, sub, sup, limits))
            },
            _ => None,
        }
    }

    fn make_nary(&mut self, op: char, sub: NodeId, sup: NodeId, limits: LimitStyle) -> NodeId {
        let body = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
        self.arena.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        })
    }

    fn optional_script_id(&self, id: Option<&NodeId>) -> NodeId {
        id.copied()
            .filter(|&id| !self.is_empty_row_id(id))
            .unwrap_or(NodeId::NONE)
    }

    fn apply_script_pair(&mut self, base: NodeId, pair: &[NodeId]) -> NodeId {
        let sub = self.optional_script_id(pair.first());
        let sup = self.optional_script_id(pair.get(1));
        self.make_script_pair(base, sub, sup)
    }

    fn make_script_pair(&mut self, base: NodeId, sub: NodeId, sup: NodeId) -> NodeId {
        match (sub == NodeId::NONE, sup == NodeId::NONE) {
            (false, false) => self.arena.push_node(CompactNode::SubSup { base, sub, sup }),
            (false, true) => self.arena.push_node(CompactNode::Subscript { base, sub }),
            (true, false) => self.arena.push_node(CompactNode::Superscript { base, sup }),
            (true, true) => base,
        }
    }

    fn finish_multiscripts(
        &mut self,
        base: NodeId,
        post: &[NodeId],
        has_pre: bool,
        pre: &[NodeId],
    ) -> NodeId {
        let base = self.apply_script_pair(base, post);
        if has_pre {
            let pre_sub = self.optional_script_id(pre.first());
            let pre_sup = self.optional_script_id(pre.get(1));
            if pre_sub != NodeId::NONE || pre_sup != NodeId::NONE {
                return self.arena.push_node(CompactNode::PreScripts {
                    base,
                    pre_sub,
                    pre_sup,
                });
            }
        }
        base
    }

    fn skip_element_body(&mut self, tag_name: &str) -> Result<()> {
        let mut nested = 0usize;
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    if Self::local_name(&e) == tag_name {
                        nested += 1;
                    }
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == tag_name {
                        if nested == 0 {
                            break;
                        }
                        nested -= 1;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error in <{tag_name}>: {e}"),
                    });
                },
                _ => continue,
            }
        }
        Ok(())
    }

    fn parse_multiscripts_element(&mut self, parent_tag: &str) -> Result<NodeId> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            return Err(Error::MathmlParse {
                message: format!(
                    "maximum nesting depth ({MAX_PARSE_DEPTH}) exceeded in <{parent_tag}>"
                ),
            });
        }

        let mut base = NodeId::NONE;
        let mut seen_base = false;
        let mut post: SmallVec<[NodeId; 4]> = SmallVec::new();
        let mut pre: SmallVec<[NodeId; 4]> = SmallVec::new();
        let mut in_pre = false;

        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mprescripts" {
                        in_pre = true;
                        self.skip_element_body("mprescripts")?;
                    } else {
                        let id = self.parse_element_by_name(&name, &e)?;
                        if !seen_base {
                            base = id;
                            seen_base = true;
                        } else if in_pre {
                            pre.push(id);
                        } else {
                            post.push(id);
                        }
                    }
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mprescripts" {
                        in_pre = true;
                    } else {
                        let id = self.parse_empty_element(&name, &e)?;
                        if !seen_base {
                            base = id;
                            seen_base = true;
                        } else if in_pre {
                            pre.push(id);
                        } else {
                            post.push(id);
                        }
                    }
                },
                Ok(Event::Text(e)) => {
                    let text = unescape_text(&e);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        if !seen_base {
                            base = id;
                            seen_base = true;
                        } else if in_pre {
                            pre.push(id);
                        } else {
                            post.push(id);
                        }
                    }
                },
                Ok(Event::GeneralRef(r)) => {
                    let text = resolve_general_ref(&r);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        if !seen_base {
                            base = id;
                            seen_base = true;
                        } else if in_pre {
                            pre.push(id);
                        } else {
                            post.push(id);
                        }
                    }
                },
                Ok(Event::End(e)) => {
                    if Self::local_name_end(&e) == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error in <{parent_tag}>: {e}"),
                    });
                },
                _ => continue,
            }
        }

        self.depth -= 1;
        if !seen_base {
            base = self.empty_row();
        }
        Ok(self.finish_multiscripts(base, &post, in_pre, &pre))
    }
}

// ---------------------------------------------------------------------------
// Helpers (no arena access needed)
// ---------------------------------------------------------------------------

fn has_explicit_text_size(start: &quick_xml::events::BytesStart<'_>) -> bool {
    get_attr(start, "displaystyle").is_some_and(|v| v.trim().eq_ignore_ascii_case("false"))
        && get_attr(start, "scriptlevel").is_some_and(|v| {
            let n = v.trim();
            !n.is_empty() && n.bytes().all(|b| b == b'0')
        })
}

/// Read the supported, single horizontal CSS transform. Other declarations
/// (such as CSS variables) are not transforms on this element.
fn has_horizontal_reflection(style: &str) -> bool {
    let mut reflected = false;
    let mut important = false;
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        if !property.trim().eq_ignore_ascii_case("transform") {
            continue;
        }
        let (value, priority) = match value.rsplit_once('!') {
            Some((value, priority)) if priority.trim().eq_ignore_ascii_case("important") => {
                (value, true)
            },
            _ => (value, false),
        };
        if important && !priority {
            continue;
        }
        important = priority;
        reflected = value
            .trim()
            .strip_suffix(')')
            .and_then(|v| v.split_once('('))
            .is_some_and(|(name, arg)| {
                name.eq_ignore_ascii_case("scaleX") && arg.trim().parse::<f64>() == Ok(-1.0)
            });
    }
    reflected
}

fn get_attr(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| unescape_attr(&a.value))
}

/// Unescape text content, resolving the full MathML/HTML named-entity set
/// (e.g. `&alpha;`, `&InvisibleTimes;`, `&ApplyFunction;`) plus numeric
/// references. Falls back to a manual pass if quick-xml reports an error so
/// that a single unknown entity never discards the surrounding text.
fn unescape_text(e: &BytesText<'_>) -> String {
    let raw = e.as_ref();
    match quick_xml::escape::unescape_with(raw, |name| {
        crate::unicode::MATHML_ENTITIES.get(name).copied()
    }) {
        Ok(cow) => cow.into_owned(),
        Err(_) => resolve_entities_lossy(raw),
    }
}

/// Unescape an attribute value the same way text content is unescaped.
fn unescape_attr(raw: &str) -> String {
    resolve_entities_lossy(raw)
}

/// Resolve a `&name;` or `&#...;` general entity reference into its replacement.
fn resolve_general_ref(r: &quick_xml::events::BytesRef<'_>) -> String {
    let name = r.as_ref();
    crate::unicode::resolve_entity(name)
        .map(|s| s.into_owned())
        .unwrap_or_else(|| format!("&{name};"))
}

/// Manual entity resolver used as a robust fallback: replaces every
/// `&name;` / `&#nnn;` / `&#xhhh;` it recognizes and leaves the rest intact.
fn resolve_entities_lossy(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let name = &after[..semi];
            if let Some(rep) = crate::unicode::resolve_entity(name) {
                out.push_str(&rep);
            } else {
                out.push('&');
                out.push_str(name);
                out.push(';');
            }
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn classify_mo(text: &str) -> SymbolKind {
    let text = text.trim();
    let ch = text.chars().next().unwrap_or(' ');
    match ch {
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2217}'
        | '\u{2218}' | '*' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}'
        | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2208}' | '\u{2192}'
        | '\u{2190}' | '\u{21A4}' | '\u{21A6}' | '\u{21D2}' | '\u{21D0}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' | '\u{230A}' | '\u{2308}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' | '\u{230B}' | '\u{2309}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        _ if ch.len_utf8() == text.len()
            && crate::unicode::symbols::nary_op_to_latex(ch).is_some() =>
        {
            SymbolKind::LargeOp
        },
        '|' | '\u{2016}' => SymbolKind::Fence,
        _ => SymbolKind::Ordinary,
    }
}

fn is_combining_accent(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}'
    )
}

#[inline]
fn is_accent_glyph(ch: char) -> bool {
    is_combining_accent(ch)
        || matches!(
            ch,
            '\u{00AF}' // macron
                | '\u{02C9}' // modifier letter macron
                | '\u{203E}' // overline
                | '\u{23B4}' // overbracket
                | '\u{23B5}' // underbracket
                | '\u{23DC}' // top brace
                | '\u{23DD}' // bottom brace
                | '\u{23DE}' // overbrace
                | '\u{23DF}' // underbrace
                | '_'
                | '^'
        )
}

fn single_accent_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}

fn parse_em_width(s: &str) -> Rational {
    let s = s.trim();
    if s.ends_with("em") {
        let num_str = s.trim_end_matches("em").trim();
        if let Ok(f) = num_str.parse::<f64>() {
            let scaled = (f * 18.0).round() as i32;
            return Rational::new(scaled, 18);
        }
    }
    Rational::zero()
}

/// Apply MathML token whitespace rules to the four XML whitespace characters.
/// Non-ASCII spaces such as NBSP remain rendered characters.
fn collapse_mathml_token_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut has_content = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r') {
            pending_space = true;
            continue;
        }
        if pending_space && has_content {
            out.push(' ');
        }
        out.push(ch);
        has_content = true;
        pending_space = false;
    }
    out
}

fn mathvariant_to_style(variant: &str) -> Option<TextStyle> {
    match variant {
        "normal" => Some(TextStyle::Normal),
        "bold" => Some(TextStyle::Bold),
        "italic" => Some(TextStyle::Italic),
        "bold-italic" => Some(TextStyle::BoldItalic),
        "double-struck" => Some(TextStyle::DoubleStruck),
        "script" => Some(TextStyle::Script),
        "bold-script" => Some(TextStyle::BoldScript),
        "fraktur" => Some(TextStyle::Fraktur),
        "bold-fraktur" => Some(TextStyle::BoldFraktur),
        "sans-serif" => Some(TextStyle::SansSerif),
        "bold-sans-serif" => Some(TextStyle::SansSerifBold),
        "sans-serif-italic" => Some(TextStyle::SansSerifItalic),
        "sans-serif-bold-italic" => Some(TextStyle::SansSerifBoldItalic),
        "monospace" => Some(TextStyle::Monospace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Config;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        MathmlReader::parse(input, &config).unwrap().expr.nodes
    }

    fn parse_one(input: &str) -> MathNode {
        let nodes = parse(input);
        if nodes.len() == 1 {
            nodes.into_iter().next().unwrap()
        } else {
            MathNode::Row(nodes)
        }
    }

    #[test]
    fn simple_number() {
        let node = parse_one("<math><mn>42</mn></math>");
        assert_eq!(node, MathNode::Number("42".into()));
    }

    #[test]
    fn simple_identifier() {
        let node = parse_one("<math><mi>x</mi></math>");
        assert_eq!(node, MathNode::Identifier("x".into()));
    }

    #[test]
    fn simple_operator() {
        let node = parse_one("<math><mo>+</mo></math>");
        assert_eq!(node, MathNode::Symbol(SymbolKind::BinaryOp, "+".into()));
    }

    #[test]
    fn fraction() {
        let node = parse_one("<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>");
        assert!(matches!(node, MathNode::Fraction { .. }));
    }

    #[test]
    fn sqrt() {
        let node = parse_one("<math><msqrt><mi>x</mi></msqrt></math>");
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn subscript() {
        let node = parse_one("<math><msub><mi>x</mi><mn>2</mn></msub></math>");
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn superscript() {
        let node = parse_one("<math><msup><mi>x</mi><mn>2</mn></msup></math>");
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn subsup() {
        let node = parse_one("<math><msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup></math>");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn named_operator_scripts_preserve_authored_placement_and_both_labels() {
        for (tag, token, expected) in [
            (
                "msubsup",
                r#"<mo movablelimits="true">lim</mo>"#,
                LimitStyle::SubSup,
            ),
            ("munderover", "<mo>lim</mo>", LimitStyle::AboveBelow),
            (
                "munderover",
                r#"<mo movablelimits="false">lim</mo>"#,
                LimitStyle::AboveBelow,
            ),
            (
                "munderover",
                r#"<mrow><mo movablelimits="true"> lim </mo></mrow>"#,
                LimitStyle::Default,
            ),
        ] {
            let xml = format!("<math><{tag}>{token}<mi>a</mi><mi>b</mi></{tag}></math>");
            let node = parse_one(&xml);
            let MathNode::SubSup { base, sub, sup } = node else {
                panic!("{xml}: {node:?}")
            };
            assert_eq!(
                *base,
                MathNode::Operator(OperatorAttr {
                    name: "lim".into(),
                    limit_style: expected
                })
            );
            assert_eq!(*sub, MathNode::Identifier("a".into()));
            assert_eq!(*sup, MathNode::Identifier("b".into()));
        }
        assert_eq!(
            parse_one("<math><mi>lim</mi></math>"),
            MathNode::Identifier("lim".into())
        );
    }

    #[test]
    fn mathjax_operator_rows_preserve_compound_bases() {
        let node = parse_one(
            r#"<math><mrow data-mjx-texclass="OP"><mi>a</mi><mo>+</mo><mi>b</mi></mrow></math>"#,
        );
        let MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } = node
        else {
            panic!("{node:?}")
        };
        assert_eq!(
            *content,
            MathNode::Row(vec![
                MathNode::Identifier("a".into()),
                MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                MathNode::Identifier("b".into())
            ])
        );
    }

    #[test]
    fn movable_under_over_large_operator_round_trips_as_default_nary() {
        let node = parse_one(
            "<math><munderover><mo movablelimits=\"true\">∑</mo><mi>i</mi><mi>n</mi></munderover><mi>x</mi></math>",
        );
        let MathNode::Row(nodes) = node else {
            panic!("expected operator and body row");
        };
        let MathNode::Nary {
            op,
            sub,
            sup,
            limits,
            ..
        } = &nodes[0]
        else {
            panic!("expected default n-ary operator");
        };
        assert_eq!(*op, '∑');
        assert!(sub.is_some() && sup.is_some());
        assert_eq!(*limits, LimitStyle::Default);
    }

    #[test]
    fn explicitly_movable_integral_keeps_under_over_structure() {
        for (tag, expected) in [
            (
                "munder",
                MathNode::Underscript {
                    base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into())),
                    under: Box::new(MathNode::Identifier("a".into())),
                    movable_limits: true,
                },
            ),
            (
                "mover",
                MathNode::Overscript {
                    base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into())),
                    over: Box::new(MathNode::Identifier("b".into())),
                    movable_limits: true,
                },
            ),
        ] {
            let input = format!(
                "<math><{tag}><mo movablelimits=\"true\">∫</mo><mi>{}</mi></{tag}></math>",
                if tag == "munder" { "a" } else { "b" }
            );
            assert_eq!(parse_one(&input), expected);
            let output =
                crate::convert(&input, crate::Format::MathML, crate::Format::MathML).unwrap();
            assert!(output.contains(&format!("<{tag}><mo movablelimits=\"true\">∫</mo>")));
        }

        let input = "<math><munderover><mo movablelimits=\"true\">∫</mo><mi>a</mi><mi>b</mi></munderover></math>";
        assert!(matches!(
            parse_one(input),
            MathNode::UnderOver {
                movable_limits: true,
                ..
            }
        ));
        let output = crate::convert(input, crate::Format::MathML, crate::Format::MathML).unwrap();
        assert!(output.contains("<munderover><mo movablelimits=\"true\">∫</mo>"));
    }

    #[test]
    fn operator_scripts_and_multi_character_tokens_keep_all_operands() {
        for (tag, bounds) in [
            ("msub", "<mi>a</mi>"),
            ("msup", "<mi>b</mi>"),
            ("msubsup", "<mi>a</mi><mi>b</mi>"),
        ] {
            let input =
                format!(r#"<math><{tag}><mo movablelimits="true">∫</mo>{bounds}</{tag}></math>"#);
            let output = crate::from_mathml(&input).unwrap().to_mathml().unwrap();
            assert!(
                output.contains(&format!(
                    r#"<{tag}><mo movablelimits="false">∫</mo>{bounds}</{tag}>"#
                )),
                "{output}"
            );
        }

        let multi = r#"<math><munder><mo movablelimits="true">∑x</mo><mi>a</mi></munder></math>"#;
        let output = crate::from_mathml(multi).unwrap().to_mathml().unwrap();
        assert!(output.contains(">∑x</mo><mi>a</mi></munder>"), "{output}");
    }

    #[test]
    fn munderover_preserves_both_accent_attributes() {
        let input = "<math><munderover accent=\"true\" accentunder=\"true\"><mo>∫</mo><mo>_</mo><mo>¯</mo></munderover></math>";
        let output = crate::convert(input, crate::Format::MathML, crate::Format::MathML).unwrap();
        assert!(output.contains("<mover accent=\"true\"><munder accentunder=\"true\">"));
    }

    #[test]
    fn large_operator_bounds_follow_the_element_and_dictionary_defaults() {
        for (tag, token, bounds, canonical_operator) in [
            (
                "msub",
                "<mo> ∑ </mo>",
                "<mi>i</mi>",
                r#"<mo movablelimits="false">∑</mo>"#,
            ),
            (
                "msup",
                r#"<mo movablelimits="true">∏</mo>"#,
                "<mi>n</mi>",
                r#"<mo movablelimits="false">∏</mo>"#,
            ),
            (
                "munder",
                "<mo> ∑ </mo>",
                "<mi>i</mi>",
                r#"<mo movablelimits="true">∑</mo>"#,
            ),
            (
                "mover",
                "<mrow><mo>⨆</mo></mrow>",
                "<mi>n</mi>",
                r#"<mo movablelimits="true">⨆</mo>"#,
            ),
            (
                "munderover",
                "<mo>∲</mo>",
                "<mi>a</mi><mi>b</mi>",
                r#"<mo movablelimits="false">∲</mo>"#,
            ),
            (
                "msubsup",
                "<mo>⨌</mo>",
                "<mi>a</mi><mi>b</mi>",
                r#"<mo movablelimits="false">⨌</mo>"#,
            ),
        ] {
            let authored = format!("<math><{tag}>{token}{bounds}</{tag}><mi>x</mi></math>");
            let first = crate::from_mathml(&authored).unwrap().to_mathml().unwrap();
            assert!(
                first.contains(&format!(
                    "<{tag}>{canonical_operator}{bounds}</{tag}><mi>x</mi>"
                )),
                "{first}"
            );
            assert_eq!(
                crate::from_mathml(&first).unwrap().to_mathml().unwrap(),
                first
            );
        }
    }

    #[test]
    fn fixed_under_over_large_operator_round_trips_as_forced_limits() {
        let node = parse_one(
            "<math><munderover><mo movablelimits=\"false\">∑</mo><mi>i</mi><mi>n</mi></munderover></math>",
        );
        let MathNode::Nary { limits, .. } = node else {
            panic!("expected forced n-ary operator");
        };
        assert_eq!(limits, LimitStyle::AboveBelow);
    }

    #[test]
    fn mrow() {
        let node = parse_one("<math><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow></math>");
        assert!(matches!(node, MathNode::Row(_)));
    }

    #[test]
    fn mtext() {
        let node = parse_one("<math><mtext>hello</mtext></math>");
        assert_eq!(node, MathNode::Text(TextStyle::Normal, "hello".into()));
    }

    #[test]
    fn mtext_collapses_xml_whitespace_and_preserves_rendered_spaces() {
        let variants = [
            ("bold", TextStyle::Bold),
            ("italic", TextStyle::Italic),
            ("bold-italic", TextStyle::BoldItalic),
            ("monospace", TextStyle::Monospace),
            ("sans-serif", TextStyle::SansSerif),
            ("bold-sans-serif", TextStyle::SansSerifBold),
            ("sans-serif-italic", TextStyle::SansSerifItalic),
            ("sans-serif-bold-italic", TextStyle::SansSerifBoldItalic),
            ("double-struck", TextStyle::DoubleStruck),
            ("script", TextStyle::Script),
            ("bold-script", TextStyle::BoldScript),
            ("fraktur", TextStyle::Fraktur),
            ("bold-fraktur", TextStyle::BoldFraktur),
        ];
        for (variant, style) in variants {
            let input = format!(
                "<math><mtext mathvariant=\"{variant}\" xml:space=\"preserve\"> \t a \n &lt; b \r </mtext></math>"
            );
            let expected = MathNode::Text(style, "a < b".into());
            assert_eq!(parse_one(&input), expected, "variant: {variant}");

            let compact = MathmlReader::parse_compact(&input, &Config::default())
                .unwrap()
                .to_parse_result();
            assert_eq!(compact.expr.nodes, vec![expected], "variant: {variant}");
        }

        let node = parse_one("<math><mtext>\u{00a0} a  b \u{00a0}</mtext></math>");
        assert_eq!(
            node,
            MathNode::Text(TextStyle::Normal, "\u{00a0} a b \u{00a0}".into())
        );
    }

    #[test]
    fn mfenced() {
        let node = parse_one("<math><mfenced open=\"(\" close=\")\"><mi>x</mi></mfenced></math>");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn mfenced_unescapes_attribute_entities() {
        let node =
            parse_one(r#"<math><mfenced open="&lt;" close="&gt;"><mi>x</mi></mfenced></math>"#);
        assert_eq!(
            node,
            MathNode::Fenced {
                open: "<".into(),
                close: ">".into(),
                content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
            }
        );
    }

    #[test]
    fn display_block() {
        let config = Config::default();
        let result =
            MathmlReader::parse("<math display=\"block\"><mn>1</mn></math>", &config).unwrap();
        assert_eq!(result.expr.display, DisplayStyle::Block);
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = MathmlReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn table() {
        let input = "<math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr><mtr><mtd><mn>3</mn></mtd><mtd><mn>4</mn></mtd></mtr></mtable></math>";
        let node = parse_one(input);
        if let MathNode::Array { rows, .. } = &node {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn phantom() {
        let node = parse_one("<math><mphantom><mi>x</mi></mphantom></math>");
        assert!(matches!(node, MathNode::Phantom(_, _)));
    }

    #[test]
    fn mover_accent() {
        let node =
            parse_one("<math><mover accent=\"true\"><mi>x</mi><mo>\u{0302}</mo></mover></math>");
        assert!(matches!(node, MathNode::Accent { .. }));
    }

    #[test]
    fn namespaced_presentation_mathml_uses_local_names() {
        let node = parse_one(
            r#"<m:math xmlns:m="http://www.w3.org/1998/Math/MathML"><m:mrow><m:mi>x</m:mi><m:mo>+</m:mo><m:mn>1</m:mn></m:mrow></m:math>"#,
        );
        assert_eq!(
            node,
            MathNode::Row(vec![
                MathNode::Identifier("x".into()),
                MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                MathNode::Number("1".into()),
            ])
        );
    }

    #[test]
    fn munder_accentunder_parses_as_below_accent() {
        let node =
            parse_one(r#"<math><munder accentunder="true"><mi>x</mi><mo>_</mo></munder></math>"#);
        assert_eq!(
            node,
            MathNode::Accent {
                base: Box::new(MathNode::Identifier("x".into())),
                accent: '_',
                position: AccentPosition::Below,
                wide: false,
            }
        );
    }

    #[test]
    fn munder_accentunder_preserves_multi_character_symbol_as_underscript() {
        let node =
            parse_one(r#"<math><munder accentunder="true"><mi>x</mi><mo>__</mo></munder></math>"#);
        assert_eq!(
            node,
            MathNode::Underscript {
                base: Box::new(MathNode::Identifier("x".into())),
                under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "__".into())),
                movable_limits: false,
            }
        );
    }

    #[test]
    fn mmultiscripts_post_only_preserves_post_scripts() {
        let node =
            parse_one("<math><mmultiscripts><mi>x</mi><mi>i</mi><mn>2</mn></mmultiscripts></math>");
        assert_eq!(
            node,
            MathNode::SubSup {
                base: Box::new(MathNode::Identifier("x".into())),
                sub: Box::new(MathNode::Identifier("i".into())),
                sup: Box::new(MathNode::Number("2".into())),
            }
        );
    }

    #[test]
    fn mmultiscripts_mixed_preserves_post_and_pre_scripts() {
        let node = parse_one(
            "<math><mmultiscripts><mi>x</mi><mi>i</mi><mn>2</mn><mprescripts/><mi>j</mi><mn>3</mn></mmultiscripts></math>",
        );
        assert_eq!(
            node,
            MathNode::PreScripts {
                base: Box::new(MathNode::SubSup {
                    base: Box::new(MathNode::Identifier("x".into())),
                    sub: Box::new(MathNode::Identifier("i".into())),
                    sup: Box::new(MathNode::Number("2".into())),
                }),
                pre_sub: Some(Box::new(MathNode::Identifier("j".into()))),
                pre_sup: Some(Box::new(MathNode::Number("3".into()))),
            }
        );
    }

    #[test]
    fn mmultiscripts_zero_width_mspace_does_not_act_as_mprescripts_marker() {
        let node = parse_one(
            r#"<math><mmultiscripts><mi>x</mi><mspace width="0em"/><mn>2</mn><mprescripts/><mi>j</mi><mn>3</mn></mmultiscripts></math>"#,
        );
        assert_eq!(
            node,
            MathNode::PreScripts {
                base: Box::new(MathNode::SubSup {
                    base: Box::new(MathNode::Identifier("x".into())),
                    sub: Box::new(MathNode::Space(Rational::zero())),
                    sup: Box::new(MathNode::Number("2".into())),
                }),
                pre_sub: Some(Box::new(MathNode::Identifier("j".into()))),
                pre_sup: Some(Box::new(MathNode::Number("3".into()))),
            }
        );
    }

    #[test]
    fn mmultiscripts_expanded_mprescripts_preserves_pre_scripts() {
        let node = parse_one(
            "<math><mmultiscripts><mi>x</mi><mi>i</mi><mn>2</mn><mprescripts></mprescripts><mi>j</mi><mn>3</mn></mmultiscripts></math>",
        );
        assert_eq!(
            node,
            MathNode::PreScripts {
                base: Box::new(MathNode::SubSup {
                    base: Box::new(MathNode::Identifier("x".into())),
                    sub: Box::new(MathNode::Identifier("i".into())),
                    sup: Box::new(MathNode::Number("2".into())),
                }),
                pre_sub: Some(Box::new(MathNode::Identifier("j".into()))),
                pre_sup: Some(Box::new(MathNode::Number("3".into()))),
            }
        );
    }

    #[test]
    fn menclose_diagonal_pair_parses_as_cross_cancel() {
        let node = parse_one(
            r#"<math><menclose notation="updiagonalstrike downdiagonalstrike"><mi>x</mi></menclose></math>"#,
        );
        assert_eq!(
            node,
            MathNode::Cancel {
                stroke: StrokeKind::Cross,
                content: Box::new(MathNode::Identifier("x".into())),
            }
        );
    }

    #[test]
    fn menclose_verticalstrike_parses_as_vertical_cancel() {
        let node =
            parse_one(r#"<math><menclose notation="verticalstrike"><mi>x</mi></menclose></math>"#);
        assert_eq!(
            node,
            MathNode::Cancel {
                stroke: StrokeKind::Vertical,
                content: Box::new(MathNode::Identifier("x".into())),
            }
        );
    }

    // ── Named-entity resolution ────────────────────────────────────────
    fn to_latex(input: &str) -> String {
        let cfg = Config::default();
        crate::from_mathml_with(input, &cfg)
            .unwrap()
            .to_latex()
            .unwrap()
    }

    #[test]
    fn named_entities_resolve_in_text() {
        let node = parse_one("<math><mi>&alpha;</mi></math>");
        assert_eq!(node, MathNode::Identifier("\u{03B1}".into()));
    }

    #[test]
    fn named_entities_resolve_in_operators_and_attrs() {
        // &InvisibleTimes; and a numeric reference both resolve.
        let nodes = parse("<math><mo>&InvisibleTimes;</mo><mn>&#x32;</mn></math>");
        assert!(matches!(&nodes[0], MathNode::Symbol(_, s) if s == "\u{2062}"));
        assert!(matches!(&nodes[1], MathNode::Number(s) if s == "2"));
    }

    #[test]
    fn unknown_entity_is_preserved_not_dropped() {
        // A bogus entity must not wipe out the surrounding text.
        let node = parse_one("<math><mtext>a&notreal;b</mtext></math>");
        assert!(matches!(&node, MathNode::Text(_, t) if t.contains('a') && t.contains('b')));
    }

    // ── Content MathML ─────────────────────────────────────────────────
    #[test]
    fn content_apply_factorial() {
        assert_eq!(
            to_latex("<math><apply><factorial/><cn>3</cn></apply></math>"),
            "{3!}"
        );
    }

    #[test]
    fn content_apply_plus_and_times() {
        let l = to_latex("<math><apply><plus/><ci>a</ci><ci>b</ci></apply></math>");
        assert!(l.contains('a') && l.contains('+') && l.contains('b'), "{l}");
    }

    #[test]
    fn content_function_ln() {
        let l = to_latex("<math><apply><ln/><ci>a</ci></apply></math>");
        assert!(l.contains("\\ln"), "{l}");
    }

    #[test]
    fn content_power_is_superscript() {
        let node = parse_one("<math><apply><power/><ci>x</ci><cn>2</cn></apply></math>");
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn content_divide_is_fraction() {
        let node = parse_one("<math><apply><divide/><ci>a</ci><ci>b</ci></apply></math>");
        assert!(matches!(node, MathNode::Fraction { .. }));
    }

    #[test]
    fn content_root_with_degree_is_root() {
        let node =
            parse_one("<math><apply><root/><degree><cn>3</cn></degree><ci>x</ci></apply></math>");
        assert!(matches!(node, MathNode::Root { .. }));
    }

    #[test]
    fn content_sum_builds_nary() {
        let node = parse_one(
            "<math><apply><sum/><bvar><ci>i</ci></bvar><lowlimit><cn>1</cn></lowlimit>\
             <uplimit><ci>n</ci></uplimit><ci>i</ci></apply></math>",
        );
        assert!(matches!(node, MathNode::Nary { .. }));
    }

    #[test]
    fn content_constant_pi() {
        let l = to_latex("<math><pi/></math>");
        assert!(l.contains('\u{03C0}') || l.contains("\\pi"), "{l}");
    }

    #[test]
    fn content_ci_with_embedded_presentation() {
        // <ci> wrapping <msub> must preserve the subscript structure.
        let node = parse_one("<math><ci><msub><mi>x</mi><mi>a</mi></msub></ci></math>");
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    // ── Token elements ────────────────────────────────────────────────
    #[test]
    fn ms_wraps_in_quotes() {
        let node = parse_one("<math><ms>hi</ms></math>");
        assert!(matches!(&node, MathNode::Text(_, t) if t == "\"hi\""));
    }

    #[test]
    fn mglyph_uses_alt_text() {
        // <mglyph> inside <mi> contributes its alt text to the identifier.
        let node =
            parse_one(r#"<math><mi><mglyph fontfamily="f" index="2" alt="braid"/></mi></math>"#);
        assert!(matches!(&node, MathNode::Identifier(t) if t == "braid"));
    }

    #[test]
    fn mtr_infers_cells_without_mtd() {
        // <mtr> children that are not <mtd> are each an inferred table cell.
        let node = parse_one(
            "<math><mtable><mtr><mn>1</mn><mn>2</mn></mtr>\
             <mtr><mn>3</mn><mn>4</mn></mtr></mtable></math>",
        );
        match node {
            MathNode::Array { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2, "expected 2 inferred cells per row");
            },
            other => panic!("expected Array, got {other:?}"),
        }
    }
}
