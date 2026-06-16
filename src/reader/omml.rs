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

pub struct OmmlReader;

impl MathReader for OmmlReader {
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
        let parser_result = OmmlCompactParser::new(trimmed);
        let mut parser = match parser_result {
            Ok(p) => p,
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    return Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, config.display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("OMML initialization failed (recovered): {e}"),
                            span: None,
                        }],
                    ));
                }
                return Err(e);
            },
        };
        match parser.parse_root() {
            Ok(ids) => {
                let roots = parser.arena.push_children(&ids);
                Ok(CompactParseResult::new(CompactExpr {
                    arena: parser.arena,
                    roots,
                    display: config.display,
                }))
            },
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr {
                            arena: parser.arena,
                            roots: NodeSpan::EMPTY,
                            display: config.display,
                        },
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("OMML parse error (recovered): {e}"),
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

struct OmmlCompactParser<'a> {
    reader: Reader<&'a [u8]>,
    arena: AstArena,
    depth: u32,
}

impl<'a> OmmlCompactParser<'a> {
    fn new(input: &'a str) -> Result<Self> {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        Ok(Self {
            reader,
            arena: AstArena::new(),
            depth: 0,
        })
    }

    fn empty_row(&mut self) -> NodeId {
        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
    }

    fn flatten_ids(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    fn is_empty_row_id(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    fn parse_root(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "oMathPara" || name == "oMath" {
                        return self.parse_children(&name);
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
                    return Err(Error::OmmlParse {
                        message: format!("OMML XML error: {e}"),
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
                    let name = local_name(&e);
                    nodes.push(self.parse_element_by_name(&name, &e)?);
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
            return Err(Error::OmmlParse {
                message: format!(
                    "maximum nesting depth ({MAX_PARSE_DEPTH}) exceeded in <{parent_tag}>"
                ),
            });
        }
        let mut nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    let id = self.parse_element_by_name(&name, &e)?;
                    // Filter out empty rows from property elements
                    if !self.is_empty_row_id(id) {
                        nodes.push(id);
                    }
                },
                Ok(Event::Empty(_)) => {},
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        nodes.push(self.classify_run_text(trimmed));
                    }
                },
                Ok(Event::End(e)) => {
                    let name = local_name_end(&e);
                    if name == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::OmmlParse {
                        message: format!("OMML XML error in <{parent_tag}>: {e}"),
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

    fn parse_element_by_name(
        &mut self,
        name: &str,
        _start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<NodeId> {
        match name {
            "r" => self.parse_run(),
            "f" => self.parse_fraction(),
            "rad" => self.parse_radical(),
            "sSub" => self.parse_ssub(),
            "sSup" => self.parse_ssup(),
            "sSubSup" => self.parse_ssubsup(),
            "sPre" => self.parse_spre(),
            "nary" => self.parse_nary(),
            "acc" => self.parse_accent(),
            "bar" => self.parse_bar(),
            "d" => self.parse_delimiter(),
            "m" => self.parse_matrix(),
            "eqArr" => self.parse_eq_array(),
            "func" => self.parse_func(),
            "limLow" => self.parse_lim_low(),
            "limUpp" => self.parse_lim_upp(),
            "groupChr" => self.parse_group_chr(),

            "box" | "boxedFormula" => self.parse_child_id(name),

            "borderBox" => self.parse_border_box(),

            "phant" => {
                let id = self.parse_child_id(name)?;
                Ok(self
                    .arena
                    .push_node(CompactNode::Phantom(id, PhantomKind::Full)))
            },

            "oMath" | "oMathPara" => {
                let children = self.parse_children(name)?;
                Ok(self.flatten_ids(&children))
            },

            // Property elements — skip contents, return empty
            "fPr" | "radPr" | "sSubPr" | "sSupPr" | "sSubSupPr" | "sPrePr" | "naryPr" | "accPr"
            | "barPr" | "dPr" | "mPr" | "eqArrPr" | "funcPr" | "limLowPr" | "limUppPr"
            | "groupChrPr" | "boxPr" | "borderBoxPr" | "phantPr" | "rPr" | "ctrlPr" | "mathPr" => {
                self.skip_element(name)?;
                Ok(self.empty_row())
            },

            // Content containers
            "e" | "num" | "den" | "deg" | "sub" | "sup" | "lim" | "fName" => {
                let children = self.parse_children(name)?;
                Ok(self.flatten_ids(&children))
            },

            "mr" => {
                let children = self.parse_children(name)?;
                Ok(self.flatten_ids(&children))
            },

            _ => {
                let children = self.parse_children(name)?;
                Ok(self.flatten_ids(&children))
            },
        }
    }

    // --- Specific element parsers ---

    fn parse_run(&mut self) -> Result<NodeId> {
        let mut text = String::new();
        let mut style = TextStyle::Normal;
        let tag = "r";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "t" => {
                            text = self.read_text_content("t")?;
                        },
                        "rPr" => {
                            style = self.parse_run_properties()?;
                        },
                        _ => {
                            self.skip_element(&name)?;
                        },
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(self.classify_run_text_styled(text.trim(), style))
    }

    fn parse_run_properties(&mut self) -> Result<TextStyle> {
        let mut sty = "p";
        let mut scr = "roman";
        let tag = "rPr";
        loop {
            match self.reader.read_event() {
                Ok(ev @ Event::Start(_)) | Ok(ev @ Event::Empty(_)) => {
                    let (e, needs_skip) = match ev {
                        Event::Start(e) => (e, true),
                        Event::Empty(e) => (e, false),
                        _ => unreachable!(),
                    };
                    let name = local_name(&e);
                    if name == "sty"
                        && let Some(val) = get_val_attr(&e)
                    {
                        sty = match val.as_str() {
                            "p" => "p",
                            "i" => "i",
                            "b" => "b",
                            "bi" => "bi",
                            _ => "p",
                        };
                    } else if name == "scr"
                        && let Some(val) = get_val_attr(&e)
                    {
                        scr = match val.as_str() {
                            "roman" => "roman",
                            "script" => "script",
                            "fraktur" => "fraktur",
                            "double-struck" => "double-struck",
                            "sans-serif" => "sans-serif",
                            "monospace" => "monospace",
                            _ => "roman",
                        };
                    }
                    if needs_skip {
                        self.skip_element(&name)?;
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(combine_omml_style(sty, scr))
    }

    fn parse_fraction(&mut self) -> Result<NodeId> {
        let mut kind = FractionKind::Normal;
        let mut num = NodeId::NONE;
        let mut den = NodeId::NONE;
        let tag = "f";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "fPr" => kind = self.parse_fraction_props()?,
                        "num" => num = self.parse_child_id("num")?,
                        "den" => den = self.parse_child_id("den")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if num == NodeId::NONE {
            num = self.empty_row();
        }
        if den == NodeId::NONE {
            den = self.empty_row();
        }
        Ok(self
            .arena
            .push_node(CompactNode::Fraction { kind, num, den }))
    }

    fn parse_fraction_props(&mut self) -> Result<FractionKind> {
        let mut kind = FractionKind::Normal;
        let tag = "fPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "type"
                        && let Some(val) = get_val_attr(&e)
                    {
                        kind = match val.as_str() {
                            "bar" => FractionKind::Normal,
                            "noBar" => FractionKind::NoBar,
                            "skw" => FractionKind::Skewed,
                            "lin" => FractionKind::Inline,
                            _ => FractionKind::Normal,
                        };
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(kind)
    }

    fn parse_radical(&mut self) -> Result<NodeId> {
        let mut deg_hide = false;
        let mut deg = NodeId::NONE;
        let mut content = NodeId::NONE;
        let tag = "rad";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "radPr" => deg_hide = self.parse_radical_props()?,
                        "deg" => deg = self.parse_child_id("deg")?,
                        "e" => content = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if content == NodeId::NONE {
            content = self.empty_row();
        }
        if deg_hide || deg == NodeId::NONE || self.is_empty_row_id(deg) {
            Ok(self.arena.push_node(CompactNode::Sqrt(content)))
        } else {
            Ok(self.arena.push_node(CompactNode::Root {
                index: deg,
                radicand: content,
            }))
        }
    }

    fn parse_radical_props(&mut self) -> Result<bool> {
        let mut deg_hide = false;
        let tag = "radPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "degHide"
                        && let Some(val) = get_val_attr(&e)
                    {
                        deg_hide = val == "1" || val == "on" || val == "true";
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(deg_hide)
    }

    fn parse_ssub(&mut self) -> Result<NodeId> {
        let (base, sub, _) = self.parse_script_element("sSub")?;
        Ok(self.arena.push_node(CompactNode::Subscript { base, sub }))
    }

    fn parse_ssup(&mut self) -> Result<NodeId> {
        let (base, _, sup) = self.parse_script_element("sSup")?;
        Ok(self.arena.push_node(CompactNode::Superscript { base, sup }))
    }

    fn parse_ssubsup(&mut self) -> Result<NodeId> {
        let (base, sub, sup) = self.parse_script_element("sSubSup")?;
        Ok(self.arena.push_node(CompactNode::SubSup { base, sub, sup }))
    }

    fn parse_spre(&mut self) -> Result<NodeId> {
        let (base, pre_sub, pre_sup) = self.parse_script_element("sPre")?;
        let pre_sub = if self.is_empty_row_id(pre_sub) {
            NodeId::NONE
        } else {
            pre_sub
        };
        let pre_sup = if self.is_empty_row_id(pre_sup) {
            NodeId::NONE
        } else {
            pre_sup
        };
        Ok(self.arena.push_node(CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        }))
    }

    fn parse_script_element(&mut self, tag: &str) -> Result<(NodeId, NodeId, NodeId)> {
        let mut base = NodeId::NONE;
        let mut sub = NodeId::NONE;
        let mut sup = NodeId::NONE;
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_id("e")?,
                        "sub" => sub = self.parse_child_id("sub")?,
                        "sup" => sup = self.parse_child_id("sup")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        if sub == NodeId::NONE {
            sub = self.empty_row();
        }
        if sup == NodeId::NONE {
            sup = self.empty_row();
        }
        Ok((base, sub, sup))
    }

    fn parse_nary(&mut self) -> Result<NodeId> {
        let mut op = '\u{222B}';
        let mut limits = LimitStyle::Default;
        let mut sub_hide = false;
        let mut sup_hide = false;
        let mut sub = NodeId::NONE;
        let mut sup = NodeId::NONE;
        let mut body = NodeId::NONE;
        let tag = "nary";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "naryPr" => {
                            let props = self.parse_nary_props()?;
                            op = props.0;
                            limits = props.1;
                            sub_hide = props.2;
                            sup_hide = props.3;
                        },
                        "sub" => sub = self.parse_child_id("sub")?,
                        "sup" => sup = self.parse_child_id("sup")?,
                        "e" => body = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if body == NodeId::NONE {
            body = self.empty_row();
        }
        let sub_id = if sub_hide || sub == NodeId::NONE || self.is_empty_row_id(sub) {
            NodeId::NONE
        } else {
            sub
        };
        let sup_id = if sup_hide || sup == NodeId::NONE || self.is_empty_row_id(sup) {
            NodeId::NONE
        } else {
            sup
        };
        Ok(self.arena.push_node(CompactNode::Nary {
            op,
            sub: sub_id,
            sup: sup_id,
            body,
            limits,
        }))
    }

    fn parse_nary_props(&mut self) -> Result<(char, LimitStyle, bool, bool)> {
        let mut chr = '\u{222B}';
        let mut limits = LimitStyle::Default;
        let mut sub_hide = false;
        let mut sup_hide = false;
        let tag = "naryPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "chr" => {
                            if let Some(val) = get_val_attr(&e)
                                && let Some(c) = val.chars().next()
                            {
                                chr = c;
                            }
                        },
                        "limLoc" => {
                            if let Some(val) = get_val_attr(&e) {
                                limits = match val.as_str() {
                                    "undOvr" => LimitStyle::AboveBelow,
                                    "subSup" => LimitStyle::SubSup,
                                    _ => LimitStyle::Default,
                                };
                            }
                        },
                        "subHide" => {
                            if let Some(val) = get_val_attr(&e) {
                                sub_hide = val == "1" || val == "on" || val == "true";
                            }
                        },
                        "supHide" => {
                            if let Some(val) = get_val_attr(&e) {
                                sup_hide = val == "1" || val == "on" || val == "true";
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((chr, limits, sub_hide, sup_hide))
    }

    fn parse_accent(&mut self) -> Result<NodeId> {
        let mut accent_char = '\u{0302}';
        let mut base = NodeId::NONE;
        let tag = "acc";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "accPr" => accent_char = self.parse_accent_props()?,
                        "e" => base = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        Ok(self.arena.push_node(CompactNode::Accent {
            base,
            accent: accent_char,
            position: AccentPosition::Above,
            wide: false,
        }))
    }

    fn parse_accent_props(&mut self) -> Result<char> {
        let mut chr = '\u{0302}';
        let tag = "accPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "chr"
                        && let Some(val) = get_val_attr(&e)
                        && let Some(c) = val.chars().next()
                    {
                        chr = c;
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(chr)
    }

    fn parse_bar(&mut self) -> Result<NodeId> {
        let mut pos_top = true;
        let mut base = NodeId::NONE;
        let tag = "bar";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "barPr" => pos_top = self.parse_bar_props()?,
                        "e" => base = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        let line_sid = self.arena.push_str("\u{203E}");
        let line = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, line_sid));
        if pos_top {
            Ok(self.arena.push_node(CompactNode::Overscript {
                base,
                over: line,
                movable_limits: false,
            }))
        } else {
            Ok(self.arena.push_node(CompactNode::Underscript {
                base,
                under: line,
                movable_limits: false,
            }))
        }
    }

    fn parse_bar_props(&mut self) -> Result<bool> {
        let mut top = true;
        let tag = "barPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "pos"
                        && let Some(val) = get_val_attr(&e)
                    {
                        top = val != "bot";
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(top)
    }

    fn parse_border_box(&mut self) -> Result<NodeId> {
        let mut stroke = None;
        let mut content = NodeId::NONE;
        let tag = "borderBox";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "borderBoxPr" => stroke = self.parse_border_box_props()?,
                        "e" => content = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if content == NodeId::NONE {
            content = self.empty_row();
        }
        if let Some(stroke) = stroke {
            Ok(self
                .arena
                .push_node(CompactNode::Cancel { stroke, content }))
        } else {
            let empty = self.arena.push_str("");
            Ok(self.arena.push_node(CompactNode::Boxed {
                content,
                bg_color: empty,
                border_color: empty,
            }))
        }
    }

    fn parse_border_box_props(&mut self) -> Result<Option<StrokeKind>> {
        let mut bltr = false;
        let mut tlbr = false;
        let mut horizontal = false;
        let mut vertical = false;
        let tag = "borderBoxPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    let enabled = omml_bool_attr(&e);
                    match name.as_str() {
                        "strikeBLTR" => bltr |= enabled,
                        "strikeTLBR" => tlbr |= enabled,
                        "strikeH" => horizontal |= enabled,
                        "strikeV" => vertical |= enabled,
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let stroke = if bltr && tlbr {
            Some(StrokeKind::Cross)
        } else if bltr {
            Some(StrokeKind::Forward)
        } else if tlbr {
            Some(StrokeKind::Backward)
        } else if horizontal {
            Some(StrokeKind::Horizontal)
        } else if vertical {
            Some(StrokeKind::Vertical)
        } else {
            None
        };
        Ok(stroke)
    }

    fn parse_delimiter(&mut self) -> Result<NodeId> {
        let mut open = "(".to_string();
        let mut close = ")".to_string();
        let mut separator = None;
        let mut elements: SmallVec<[NodeId; 8]> = SmallVec::new();
        let tag = "d";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "dPr" => {
                            let (o, c, sep) = self.parse_delimiter_props()?;
                            open = o;
                            close = c;
                            separator = sep;
                        },
                        "e" => {
                            let id = self.parse_child_id("e")?;
                            elements.push(id);
                        },
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }

        let mut content_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        let sep_sid = separator.as_deref().map(|sep| self.arena.push_str(sep));
        for (idx, &id) in elements.iter().enumerate() {
            if idx > 0
                && let Some(sid) = sep_sid
            {
                content_ids.push(self.arena.push_node(CompactNode::Middle(sid)));
            }
            content_ids.push(id);
        }
        let content = self.arena.push_children(&content_ids);
        let open_sid = self.arena.push_str(&open);
        let close_sid = self.arena.push_str(&close);
        Ok(self.arena.push_node(CompactNode::Fenced {
            open: open_sid,
            close: close_sid,
            content,
        }))
    }

    fn parse_delimiter_props(&mut self) -> Result<(String, String, Option<String>)> {
        let mut open = "(".to_string();
        let mut close = ")".to_string();
        let mut separator = None;
        let tag = "dPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "begChr" => {
                            if let Some(val) = get_val_attr(&e) {
                                open = val;
                            }
                        },
                        "endChr" => {
                            if let Some(val) = get_val_attr(&e) {
                                close = val;
                            }
                        },
                        "sepChr" => {
                            if let Some(val) = get_val_attr(&e) {
                                separator = Some(val);
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((open, close, separator))
    }

    fn parse_matrix(&mut self) -> Result<NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::new();
        let mut column_aligns = Vec::new();
        let tag = "m";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "mr" => rows.push(self.parse_matrix_row()?),
                        "mPr" => column_aligns = self.parse_matrix_props()?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let ncols = rows
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(0)
            .max(column_aligns.len());
        let num_cols = ncols.max(1) as u16;

        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for idx in 0..ncols {
            let align = column_aligns
                .get(idx)
                .copied()
                .unwrap_or(ColumnAlign::Center);
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(align)));
        }
        let col_aligns = self.arena.push_children(&align_ids);

        let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for row in &rows {
            for cell_children in row {
                let span = self.arena.push_children(cell_children);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
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
            col_seps: 0,
        }))
    }

    fn parse_matrix_props(&mut self) -> Result<Vec<ColumnAlign>> {
        let mut columns = Vec::new();
        let tag = "mPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "mcs" => columns = self.parse_matrix_columns()?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(columns)
    }

    fn parse_matrix_columns(&mut self) -> Result<Vec<ColumnAlign>> {
        let mut columns = Vec::new();
        let tag = "mcs";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "mc" => {
                            let (count, align) = self.parse_matrix_column()?;
                            columns.extend(std::iter::repeat_n(align, count.max(1)));
                        },
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(columns)
    }

    fn parse_matrix_column(&mut self) -> Result<(usize, ColumnAlign)> {
        let mut count = 1;
        let mut align = ColumnAlign::Center;
        let tag = "mc";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "mcPr" => {
                            let props = self.parse_matrix_column_props()?;
                            count = props.0;
                            align = props.1;
                        },
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((count, align))
    }

    fn parse_matrix_column_props(&mut self) -> Result<(usize, ColumnAlign)> {
        let mut count = 1;
        let mut align = ColumnAlign::Center;
        let tag = "mcPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "count" => {
                            if let Some(val) = get_val_attr(&e)
                                && let Ok(parsed) = val.parse::<usize>()
                            {
                                count = parsed;
                            }
                        },
                        "mcJc" => {
                            if let Some(val) = get_val_attr(&e) {
                                align = parse_column_align(&val);
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((count, align))
    }

    fn parse_matrix_row(&mut self) -> Result<Vec<SmallVec<[NodeId; 8]>>> {
        let mut cells = Vec::new();
        let tag = "mr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "e" {
                        cells.push(self.parse_children("e")?);
                    } else {
                        self.skip_element(&name)?;
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(cells)
    }

    fn parse_eq_array(&mut self) -> Result<NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 8]>>> = Vec::new();
        let tag = "eqArr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => {
                            let children = self.parse_children("e")?;
                            rows.push(vec![children]);
                        },
                        "eqArrPr" => self.skip_element("eqArrPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }

        let num_cols: u16 = 1;
        let align_id = self
            .arena
            .push_node(CompactNode::ColAlign(ColumnAlign::Left));
        let col_aligns = self.arena.push_children(&[align_id]);

        let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for row in &rows {
            for cell_children in row {
                let span = self.arena.push_children(cell_children);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
        }
        let cells = self.arena.push_children(&cell_ids);

        Ok(self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct: 100,
            add_jot: false,
            col_seps: 0,
        }))
    }

    fn parse_func(&mut self) -> Result<NodeId> {
        let mut fname = NodeId::NONE;
        let mut arg = NodeId::NONE;
        let tag = "func";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "fName" => fname = self.parse_child_id("fName")?,
                        "e" => arg = self.parse_child_id("e")?,
                        "funcPr" => self.skip_element("funcPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if fname == NodeId::NONE {
            fname = self.empty_row();
        }
        if arg == NodeId::NONE {
            arg = self.empty_row();
        }

        // Check if function name is an operator
        let op_name = self.extract_operator_name(fname);
        if let Some(name) = op_name {
            let sid = self.arena.push_str(&name);
            let op = self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits: LimitStyle::SubSup,
            });
            let span = self.arena.push_children(&[op, arg]);
            Ok(self.arena.push_node(CompactNode::Row(span)))
        } else {
            let span = self.arena.push_children(&[fname, arg]);
            Ok(self.arena.push_node(CompactNode::Row(span)))
        }
    }

    fn parse_lim_low(&mut self) -> Result<NodeId> {
        let mut base = NodeId::NONE;
        let mut lim = NodeId::NONE;
        let tag = "limLow";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_id("e")?,
                        "lim" => lim = self.parse_child_id("lim")?,
                        "limLowPr" => self.skip_element("limLowPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        if lim == NodeId::NONE {
            lim = self.empty_row();
        }
        Ok(self.arena.push_node(CompactNode::Underscript {
            base,
            under: lim,
            movable_limits: true,
        }))
    }

    fn parse_lim_upp(&mut self) -> Result<NodeId> {
        let mut base = NodeId::NONE;
        let mut lim = NodeId::NONE;
        let tag = "limUpp";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_id("e")?,
                        "lim" => lim = self.parse_child_id("lim")?,
                        "limUppPr" => self.skip_element("limUppPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        if lim == NodeId::NONE {
            lim = self.empty_row();
        }
        Ok(self.arena.push_node(CompactNode::Overscript {
            base,
            over: lim,
            movable_limits: true,
        }))
    }

    fn parse_group_chr(&mut self) -> Result<NodeId> {
        let mut chr = '\u{23DF}';
        let mut pos_top = false;
        let mut base = NodeId::NONE;
        let tag = "groupChr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "groupChrPr" => {
                            let (c, t) = self.parse_group_chr_props()?;
                            chr = c;
                            pos_top = t;
                        },
                        "e" => base = self.parse_child_id("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if base == NodeId::NONE {
            base = self.empty_row();
        }
        let mut buf = [0u8; 4];
        let sym_str = chr.encode_utf8(&mut buf);
        let sid = self.arena.push_str(sym_str);
        let sym = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        if pos_top {
            Ok(self.arena.push_node(CompactNode::Overscript {
                base,
                over: sym,
                movable_limits: false,
            }))
        } else {
            Ok(self.arena.push_node(CompactNode::Underscript {
                base,
                under: sym,
                movable_limits: false,
            }))
        }
    }

    fn parse_group_chr_props(&mut self) -> Result<(char, bool)> {
        let mut chr = '\u{23DF}';
        let mut top = false;
        let tag = "groupChrPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "chr" => {
                            if let Some(val) = get_val_attr(&e)
                                && let Some(c) = val.chars().next()
                            {
                                chr = c;
                            }
                        },
                        "pos" => {
                            if let Some(val) = get_val_attr(&e) {
                                top = val == "top";
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((chr, top))
    }

    fn read_text_content(&mut self, tag_name: &str) -> Result<String> {
        let mut text = String::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.unescape().unwrap_or_default());
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag_name {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(text)
    }

    fn skip_element(&mut self, _tag_name: &str) -> Result<()> {
        let mut depth = 1u32;
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(_)) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(())
    }

    // --- Text classification ---

    fn classify_run_text(&mut self, text: &str) -> NodeId {
        self.classify_run_text_styled(text, TextStyle::Normal)
    }

    fn classify_run_text_styled(&mut self, text: &str, style: TextStyle) -> NodeId {
        if text.is_empty() {
            return self.empty_row();
        }
        // Check if it's a number
        if text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
            && text.chars().any(|c| c.is_ascii_digit())
        {
            let sid = self.arena.push_str(text);
            return self.arena.push_node(CompactNode::Number(sid));
        }
        // Single character
        if text.chars().count() == 1 {
            let ch = text.chars().next().expect("count is 1");
            return match classify_omml_char(ch) {
                CharClass::Letter => {
                    let sid = self.arena.push_str(text);
                    if style != TextStyle::Normal && style != TextStyle::Italic {
                        let inner = self.arena.push_node(CompactNode::Identifier(sid));
                        let span = self.arena.push_children(&[inner]);
                        self.arena.push_node(CompactNode::Styled(style, span))
                    } else {
                        self.arena.push_node(CompactNode::Identifier(sid))
                    }
                },
                CharClass::Operator => {
                    let sid = self.arena.push_str(text);
                    self.arena
                        .push_node(CompactNode::Symbol(classify_op_char(ch), sid))
                },
                CharClass::Digit => {
                    let sid = self.arena.push_str(text);
                    self.arena.push_node(CompactNode::Number(sid))
                },
            };
        }
        // Multi-character: likely an operator name or text
        if crate::unicode::symbols::is_operator_command(text) {
            let sid = self.arena.push_str(text);
            return self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits: if crate::unicode::symbols::operator_has_limits(text) {
                    LimitStyle::Default
                } else {
                    LimitStyle::SubSup
                },
            });
        }
        let sid = self.arena.push_str(text);
        if text.chars().all(char::is_alphabetic) && style != TextStyle::Normal {
            let inner = self.arena.push_node(CompactNode::Identifier(sid));
            if style == TextStyle::Italic {
                inner
            } else {
                let span = self.arena.push_children(&[inner]);
                self.arena.push_node(CompactNode::Styled(style, span))
            }
        } else if style != TextStyle::Normal {
            let inner = self
                .arena
                .push_node(CompactNode::Text(TextStyle::Normal, sid));
            let span = self.arena.push_children(&[inner]);
            self.arena.push_node(CompactNode::Styled(style, span))
        } else {
            self.arena
                .push_node(CompactNode::Text(TextStyle::Normal, sid))
        }
    }

    fn extract_operator_name(&self, id: NodeId) -> Option<String> {
        match *self.arena.node(id) {
            CompactNode::Operator { name, .. } => Some(self.arena.str_val(name).to_owned()),
            CompactNode::Text(_, sid) => {
                let s = self.arena.str_val(sid);
                let trimmed = s.trim();
                if crate::unicode::symbols::is_operator_command(trimmed) {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            },
            CompactNode::Row(span) => {
                let children = self.arena.children(span);
                if children.len() == 1 {
                    self.extract_operator_name(children[0])
                } else {
                    None
                }
            },
            _ => None,
        }
    }
}

// --- Helpers ---

fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    let qname = e.name();
    let local = qname.local_name();
    std::str::from_utf8(local.as_ref()).unwrap_or("").to_owned()
}

fn local_name_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    let qname = e.name();
    let local = qname.local_name();
    std::str::from_utf8(local.as_ref()).unwrap_or("").to_owned()
}

fn get_val_attr(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref() == b"val" {
            a.unescape_value().ok().map(|value| value.into_owned())
        } else {
            None
        }
    })
}

fn omml_bool_attr(e: &quick_xml::events::BytesStart<'_>) -> bool {
    match get_val_attr(e).as_deref() {
        Some("0" | "off" | "false") => false,
        _ => true,
    }
}

fn parse_column_align(value: &str) -> ColumnAlign {
    match value {
        "left" | "l" => ColumnAlign::Left,
        "right" | "r" => ColumnAlign::Right,
        _ => ColumnAlign::Center,
    }
}

enum CharClass {
    Letter,
    Operator,
    Digit,
}

fn classify_omml_char(ch: char) -> CharClass {
    if ch.is_ascii_digit() {
        CharClass::Digit
    } else if ch.is_alphabetic() {
        CharClass::Letter
    } else {
        CharClass::Operator
    }
}

fn classify_op_char(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '*' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        '|' | '\u{2016}' => SymbolKind::Fence,
        '\u{2211}' | '\u{220F}' | '\u{222B}' => SymbolKind::LargeOp,
        _ => SymbolKind::Ordinary,
    }
}

fn combine_omml_style(sty: &str, scr: &str) -> TextStyle {
    match scr {
        "script" => match sty {
            "b" | "bi" => TextStyle::BoldScript,
            _ => TextStyle::Script,
        },
        "fraktur" => match sty {
            "b" | "bi" => TextStyle::BoldFraktur,
            _ => TextStyle::Fraktur,
        },
        "double-struck" => TextStyle::DoubleStruck,
        "sans-serif" => match sty {
            "b" => TextStyle::SansSerifBold,
            "i" => TextStyle::SansSerifItalic,
            "bi" => TextStyle::SansSerifBoldItalic,
            _ => TextStyle::SansSerif,
        },
        "monospace" => TextStyle::Monospace,
        _ => match sty {
            "i" => TextStyle::Italic,
            "b" => TextStyle::Bold,
            "bi" => TextStyle::BoldItalic,
            _ => TextStyle::Normal,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        OmmlReader::parse(input, &config).unwrap().expr.nodes
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
    fn simple_run_number() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>42</m:t></m:r></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(node, MathNode::Number("42".into()));
    }

    #[test]
    fn simple_run_identifier() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(node, MathNode::Identifier("x".into()));
    }

    #[test]
    fn fraction() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        ));
    }

    #[test]
    fn subscript() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn superscript() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn sqrt() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn delimiter() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn delimiter_sep_chr_becomes_middle_item() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/><m:sepChr m:val=";"/></m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(
            node,
            MathNode::Fenced {
                open: "[".into(),
                close: "]".into(),
                content: vec![
                    FencedItem::Element(MathNode::Identifier("a".into())),
                    FencedItem::Middle(";".into()),
                    FencedItem::Element(MathNode::Identifier("b".into())),
                ],
            }
        );
    }

    #[test]
    fn delimiter_props_accept_alternate_prefix_and_unescape_values() {
        let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:d><q:dPr><q:begChr q:val="&lt;"/><q:endChr q:val="&gt;"/><q:sepChr q:val="&amp;"/></q:dPr><q:e><q:r><q:t>a</q:t></q:r></q:e><q:e><q:r><q:t>b</q:t></q:r></q:e></q:d></q:oMath>"#;
        let node = parse_one(input);
        assert_eq!(
            node,
            MathNode::Fenced {
                open: "<".into(),
                close: ">".into(),
                content: vec![
                    FencedItem::Element(MathNode::Identifier("a".into())),
                    FencedItem::Middle("&".into()),
                    FencedItem::Element(MathNode::Identifier("b".into())),
                ],
            }
        );
    }

    #[test]
    fn matrix_props_accept_alternate_value_prefix() {
        let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:m><q:mPr><q:mcs><q:mc><q:mcPr><q:count q:val="2"/><q:mcJc q:val="left"/></q:mcPr></q:mc></q:mcs></q:mPr><q:mr><q:e><q:r><q:t>a</q:t></q:r></q:e><q:e><q:r><q:t>b</q:t></q:r></q:e></q:mr></q:m></q:oMath>"#;
        let node = parse_one(input);
        match node {
            MathNode::Array { columns, .. } => {
                assert_eq!(columns, vec![ColumnAlign::Left, ColumnAlign::Left]);
            },
            other => panic!("expected matrix array, got {other:?}"),
        }
    }

    #[test]
    fn disabled_strike_with_alternate_value_prefix_stays_boxed() {
        let input = r#"<q:oMath xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/math"><q:borderBox><q:borderBoxPr><q:strikeBLTR q:val="0"/></q:borderBoxPr><q:e><q:r><q:t>x</q:t></q:r></q:e></q:borderBox></q:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Boxed { .. }), "{node:?}");
    }

    #[test]
    fn matrix_mcs_column_alignment_is_preserved() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="1"/><m:mcJc m:val="left"/></m:mcPr></m:mc><m:mc><m:mcPr><m:count m:val="1"/><m:mcJc m:val="right"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:mr></m:m></m:oMath>"#;
        let node = parse_one(input);
        match node {
            MathNode::Array { columns, .. } => {
                assert_eq!(columns, vec![ColumnAlign::Left, ColumnAlign::Right]);
            },
            other => panic!("expected matrix array, got {other:?}"),
        }
    }

    #[test]
    fn italic_multi_character_run_is_identifier() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>abc</m:t></m:r></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(node, MathNode::Identifier("abc".into()));
    }

    #[test]
    fn border_box_strikes_become_cancel_strokes() {
        let cases = [
            ("strikeBLTR", StrokeKind::Forward),
            ("strikeTLBR", StrokeKind::Backward),
            ("strikeH", StrokeKind::Horizontal),
            ("strikeV", StrokeKind::Vertical),
        ];
        for (strike, stroke) in cases {
            let input = format!(
                r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:borderBox><m:borderBoxPr><m:{strike} m:val="1"/></m:borderBoxPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:borderBox></m:oMath>"#
            );
            let node = parse_one(&input);
            assert_eq!(
                node,
                MathNode::Cancel {
                    stroke,
                    content: Box::new(MathNode::Identifier("x".into())),
                },
                "{strike} should parse as {stroke:?} cancel"
            );
        }
    }

    #[test]
    fn accent() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:acc><m:accPr><m:chr m:val="̂"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Accent { .. }));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = OmmlReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn border_box() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:borderBox><m:e><m:r><m:t>x</m:t></m:r></m:e></m:borderBox></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Boxed { .. }));
    }

    #[test]
    fn phantom() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:phant><m:e><m:r><m:t>x</m:t></m:r></m:e></m:phant></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Phantom(_, _)));
    }
}
