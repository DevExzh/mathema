use quick_xml::Reader;
use quick_xml::events::Event;
use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader as MathReader;

pub struct MathmlReader;

impl MathReader for MathmlReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
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
                let roots = parser.arena.push_children(&ids);
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
}

impl<'a> MathmlCompactParser<'a> {
    fn new(input: &'a str) -> Result<Self> {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        // Heuristic: MathML is verbose, roughly 1 node per 15 input bytes
        let est_nodes = input.len() / 15 + 4;
        Ok(Self {
            reader,
            arena: AstArena::with_capacity(est_nodes, est_nodes, input.len() / 4),
            display: None,
            depth: 0,
        })
    }

    /// Convert a quick-xml QName to `&str` without heap allocation.
    /// MathML tag names are always ASCII, so this is safe.
    #[inline]
    fn name_str(raw: &[u8]) -> &str {
        // MathML tags are ASCII; fast path avoids UTF-8 validation.
        std::str::from_utf8(raw).unwrap_or("")
    }

    fn parse_root(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref());
                    if name == "math" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"display" {
                                self.display = Some(if attr.value.as_ref() == b"block" {
                                    DisplayStyle::Block
                                } else {
                                    DisplayStyle::Inline
                                });
                            }
                        }
                        return self.parse_children("math");
                    } else {
                        let name_owned = name.to_owned();
                        let id = self.parse_element_by_name(&name_owned, &e)?;
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
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref()).to_owned();
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref()).to_owned();
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default();
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
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref()).to_owned();
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref()).to_owned();
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default();
                    if !text.trim().is_empty() {
                        nodes.push(self.classify_text_compact(&text));
                    }
                },
                Ok(Event::End(e)) => {
                    let qname = e.name();
                    if Self::name_str(qname.as_ref()) == parent_tag {
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
        Ok(nodes)
    }

    fn parse_child_id(&mut self, parent_tag: &str) -> Result<NodeId> {
        let children = self.parse_children(parent_tag)?;
        Ok(self.flatten_ids(&children))
    }

    fn flatten_ids(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
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
                    let span = self.arena.push_children(&[inner]);
                    return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
                }
                Ok(self.arena.push_node(CompactNode::Identifier(sid)))
            },
            "mo" => {
                let text = self.read_text_content(name)?;
                let kind = classify_mo(&text);
                let sid = self.arena.push_str(&text);
                Ok(self.arena.push_node(CompactNode::Symbol(kind, sid)))
            },
            "mtext" => {
                let text = self.read_text_content(name)?;
                let sid = self.arena.push_str(&text);
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
                let children = self.parse_children(name)?;
                let span = self.arena.push_children(&children);
                Ok(self.arena.push_node(CompactNode::Row(span)))
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
                Ok(self.arena.push_node(CompactNode::Subscript { base, sub }))
            },
            "msup" => {
                let children = self.parse_children(name)?;
                let (base, sup) = self.two_child_ids(children);
                Ok(self.arena.push_node(CompactNode::Superscript { base, sup }))
            },
            "msubsup" => {
                let children = self.parse_children(name)?;
                let (base, sub, sup) = self.three_child_ids(children);
                Ok(self.arena.push_node(CompactNode::SubSup { base, sub, sup }))
            },
            "mover" => {
                let movable = get_attr(start, "accent").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, over) = self.two_child_ids(children);
                // Check if this is an accent
                if let CompactNode::Symbol(_, sid) = *self.arena.node(over) {
                    let s = self.arena.str_val(sid).to_owned();
                    if let Some(ch) = s.chars().next()
                        && is_combining_accent(ch)
                    {
                        return Ok(self.arena.push_node(CompactNode::Accent {
                            base,
                            accent: ch,
                            position: AccentPosition::Above,
                            wide: false,
                        }));
                    }
                }
                Ok(self.arena.push_node(CompactNode::Overscript {
                    base,
                    over,
                    movable_limits: movable,
                }))
            },
            "munder" => {
                let movable = get_attr(start, "accentunder").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, under) = self.two_child_ids(children);
                Ok(self.arena.push_node(CompactNode::Underscript {
                    base,
                    under,
                    movable_limits: movable,
                }))
            },
            "munderover" => {
                let movable = get_attr(start, "accent").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, under, over) = self.three_child_ids(children);
                Ok(self.arena.push_node(CompactNode::UnderOver {
                    base,
                    under,
                    over,
                    movable_limits: movable,
                }))
            },
            "mfenced" => {
                let open = get_attr(start, "open").unwrap_or_else(|| "(".into());
                let close = get_attr(start, "close").unwrap_or_else(|| ")".into());
                let children = self.parse_children(name)?;
                let content = self.arena.push_children(&children);
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
                let col_aligns = self.arena.push_children(&align_ids);

                let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
                for row in &rows {
                    for cell_children in row {
                        let span = self.arena.push_children(cell_children);
                        cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
                    }
                    // Pad short rows
                    for _ in row.len()..(num_cols as usize) {
                        cell_ids.push(self.empty_row());
                    }
                }
                let cells = self.arena.push_children(&cell_ids);

                Ok(self.arena.push_node(CompactNode::Array {
                    col_aligns,
                    cells,
                    num_cols,
                    arraystretch_pct: 100,
                    add_jot: false,
                }))
            },
            "mphantom" => {
                let id = self.parse_child_id(name)?;
                Ok(self.arena.push_node(CompactNode::Phantom(id)))
            },
            "menclose" => {
                let notation = get_attr(start, "notation").unwrap_or_default();
                let id = self.parse_child_id(name)?;
                if notation.contains("box") || notation.contains("roundedbox") {
                    let empty = self.arena.push_str("");
                    Ok(self.arena.push_node(CompactNode::Boxed {
                        content: id,
                        bg_color: empty,
                        border_color: empty,
                    }))
                } else if notation.contains("updiagonalstrike") {
                    Ok(self.arena.push_node(CompactNode::Cancel {
                        stroke: StrokeKind::Forward,
                        content: id,
                    }))
                } else if notation.contains("downdiagonalstrike") {
                    Ok(self.arena.push_node(CompactNode::Cancel {
                        stroke: StrokeKind::Backward,
                        content: id,
                    }))
                } else if notation.contains("horizontalstrike") {
                    Ok(self.arena.push_node(CompactNode::Cancel {
                        stroke: StrokeKind::Horizontal,
                        content: id,
                    }))
                } else {
                    let empty = self.arena.push_str("");
                    Ok(self.arena.push_node(CompactNode::Boxed {
                        content: id,
                        bg_color: empty,
                        border_color: empty,
                    }))
                }
            },
            "mstyle" => {
                let mathvariant = get_attr(start, "mathvariant");
                let children = self.parse_children(name)?;
                if let Some(variant) = mathvariant
                    && let Some(style) = mathvariant_to_style(&variant)
                {
                    let span = self.arena.push_children(&children);
                    return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
                }
                Ok(self.flatten_ids(&children))
            },
            "mpadded" => self.parse_child_id(name),
            "mmultiscripts" => {
                let children = self.parse_children(name)?;
                self.parse_multiscripts(&children)
            },
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
            "mprescripts" => {
                // Marker for mmultiscripts — sentinel Space(0/1)
                Ok(self
                    .arena
                    .push_node(CompactNode::Space(Rational::new(0, 1))))
            },
            "none" => Ok(self.empty_row()),
            _ => Ok(self.empty_row()),
        }
    }

    fn read_text_content(&mut self, tag_name: &str) -> Result<String> {
        let mut text = String::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.unescape().unwrap_or_default());
                },
                Ok(Event::End(e)) => {
                    let qname = e.name();
                    if Self::name_str(qname.as_ref()) == tag_name {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Ok(Event::Start(_)) => {
                    // Nested element inside text content — skip it
                    continue;
                },
                _ => continue,
            }
        }
        Ok(text.trim().to_string())
    }

    fn parse_table(&mut self, parent_tag: &str) -> Result<Vec<Vec<SmallVec<[NodeId; 8]>>>> {
        let mut rows = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref());
                    if name == "mtr" || name == "mlabeledtr" {
                        let name_owned = name.to_owned();
                        rows.push(self.parse_table_row(&name_owned)?);
                    }
                },
                Ok(Event::End(e)) => {
                    let qname = e.name();
                    if Self::name_str(qname.as_ref()) == parent_tag {
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
                    let qname = e.name();
                    let name = Self::name_str(qname.as_ref());
                    if name == "mtd" {
                        cells.push(self.parse_children("mtd")?);
                    }
                },
                Ok(Event::End(e)) => {
                    let qname = e.name();
                    if Self::name_str(qname.as_ref()) == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(cells)
    }

    fn parse_multiscripts(&mut self, children: &[NodeId]) -> Result<NodeId> {
        let base = children
            .first()
            .copied()
            .unwrap_or_else(|| self.empty_row());

        // Find prescripts marker (Space(0/1) sentinel)
        let mut found_pre = false;
        let mut remaining: SmallVec<[NodeId; 4]> = SmallVec::new();
        for &id in children.iter().skip(1) {
            if let CompactNode::Space(r) = *self.arena.node(id)
                && r.num == 0
                && r.den == 1
            {
                found_pre = true;
                continue;
            }
            remaining.push(id);
        }

        if found_pre && remaining.len() >= 2 {
            let is_empty = |id: NodeId| matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty());
            let pre_sub = if is_empty(remaining[0]) {
                NodeId::NONE
            } else {
                remaining[0]
            };
            let pre_sup = if is_empty(remaining[1]) {
                NodeId::NONE
            } else {
                remaining[1]
            };
            Ok(self.arena.push_node(CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            }))
        } else {
            Ok(base)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers (no arena access needed)
// ---------------------------------------------------------------------------

fn get_attr(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .map(|a| {
            // Avoid from_utf8_lossy + to_string; attribute values are valid UTF-8
            String::from_utf8(a.value.to_vec()).unwrap_or_default()
        })
}

fn classify_mo(text: &str) -> SymbolKind {
    let ch = text.trim().chars().next().unwrap_or(' ');
    match ch {
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2217}'
        | '\u{2218}' | '*' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}'
        | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2208}' | '\u{2192}'
        | '\u{2190}' | '\u{21D2}' | '\u{21D0}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' | '\u{230A}' | '\u{2308}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' | '\u{230B}' | '\u{2309}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        '\u{2211}' | '\u{220F}' | '\u{2210}' | '\u{222B}' | '\u{222C}' | '\u{222D}'
        | '\u{222E}' | '\u{22C2}' | '\u{22C3}' => SymbolKind::LargeOp,
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
    fn mfenced() {
        let node = parse_one("<math><mfenced open=\"(\" close=\")\"><mi>x</mi></mfenced></math>");
        assert!(matches!(node, MathNode::Fenced { .. }));
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
        assert!(matches!(node, MathNode::Phantom(_)));
    }

    #[test]
    fn mover_accent() {
        let node =
            parse_one("<math><mover accent=\"true\"><mi>x</mi><mo>\u{0302}</mo></mover></math>");
        assert!(matches!(node, MathNode::Accent { .. }));
    }
}
