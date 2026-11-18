use quick_xml::Reader;
use quick_xml::events::{BytesText, Event};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
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
        let parser_result = MathmlCompactParser::new(trimmed, config.display);
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
type MathmlTableRow = Vec<SmallVec<[NodeId; 8]>>;
type MathmlTableRows = Vec<MathmlTableRow>;

#[derive(Clone, Copy)]
enum ScriptChildMode {
    Script,
    Accent,
}

struct MathmlCompactParser<'a> {
    reader: Reader<&'a [u8]>,
    arena: AstArena,
    display: Option<DisplayStyle>,
    depth: u32,
    movable_limit_operators: HashMap<NodeId, bool>,
    /// Standard MathML `stretchy` state for operator tokens used as accent scripts.
    wide_accents: HashSet<NodeId>,
    style_display: bool,
    /// The authored MathML scriptlevel can exceed TeX's three finite size
    /// levels. Keep the full non-negative level while reading nested
    /// `mstyle` attributes and clamp only when lowering to a StyleSwitch.
    style_level: i64,
    /// The active font variant inherited from an `mstyle`, when one is
    /// present. A multi-letter normal identifier is already upright under
    /// MathML's default rules, so it needs an AST style wrapper only when it
    /// overrides a different inherited variant.
    mathvariant_style: Option<TextStyle>,
    /// Only direct, explicitly fenced operators of a potential fenced row
    /// need to survive until its closing tag. Nested rows use their own slice.
    fence_depth: Option<u32>,
    fence_marks: SmallVec<[(NodeId, FenceRole); 4]>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum FenceRole {
    #[default]
    Inferred,
    Prefix,
    Infix,
    Postfix,
    FixedLiteral,
    Constrained,
}

impl<'a> MathmlCompactParser<'a> {
    fn new(input: &'a str, display: DisplayStyle) -> Result<Self> {
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
            wide_accents: HashSet::new(),
            style_display: display == DisplayStyle::Block,
            style_level: 0,
            mathvariant_style: None,
            fence_depth: None,
            fence_marks: SmallVec::new(),
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
                        if let Some(display) = self.display {
                            self.style_display = display == DisplayStyle::Block;
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

    /// Recover explicitly marked delimiters of a single MathML row. Interior
    /// brackets may belong to separate pairs; a prefix or postfix vertical
    /// bar inside parentheses is an ordinary child rather than a middle.
    fn recover_fenced_row(&mut self, children: &mut [NodeId], start: usize) -> Option<NodeId> {
        let [(first, open_role), interior @ .., (last, close_role)] = &self.fence_marks[start..]
        else {
            return None;
        };
        if children.first() != Some(first)
            || children.last() != Some(last)
            || !matches!(open_role, FenceRole::Inferred | FenceRole::Prefix)
            || !matches!(close_role, FenceRole::Inferred | FenceRole::Postfix)
        {
            return None;
        }
        let open = self.fence_delimiter(*first)?;
        let close = self.fence_delimiter(*last)?;
        let pair = (self.arena.str_val(open), self.arena.str_val(close));
        let explicit = matches!(
            (open_role, close_role),
            (FenceRole::Prefix, FenceRole::Postfix)
        );
        let bars = matches!(pair, ("|", "|") | ("‖", "‖"));
        let natural_close = crate::unicode::operators::closing_fence(pair.0);
        if !bars
            && natural_close != Some(pair.1)
            && !(explicit && known_fence_endpoint(pair.0) && known_fence_endpoint(pair.1))
        {
            return None;
        }

        let mut marks = interior.iter().copied().peekable();
        let mut middles = SmallVec::<[(usize, StrId); 2]>::new();
        let mut inner_pairs = SmallVec::<[&'static str; 2]>::new();
        let end = children.len() - 1;
        let content = &mut children[1..end];
        for (i, &id) in content.iter().enumerate() {
            let Some((_, role)) = marks.next_if(|(mark, _)| *mark == id) else {
                // A literal closing delimiter can finish the first of two
                // adjacent pairs even when only the outermost operators have
                // attributes. Do not join those pairs into one fence.
                if let Some(sid) = self.fence_delimiter(id) {
                    let glyph = self.arena.str_val(sid);
                    if let Some(partner) = crate::unicode::operators::closing_fence(glyph) {
                        inner_pairs.push(partner);
                    } else if inner_pairs.last() == Some(&glyph) {
                        inner_pairs.pop();
                    } else if (!glyph.is_empty() && glyph == pair.1) || natural_close == Some(glyph)
                    {
                        return None;
                    }
                }
                continue;
            };
            // This marked operator is fixed-width content, even when its
            // glyph is identical to the enclosing closing delimiter.
            if role == FenceRole::FixedLiteral {
                continue;
            }
            let sid = self.fence_delimiter(id)?;
            let glyph = self.arena.str_val(sid);
            if (role == FenceRole::Infix && writable_middle(glyph))
                || (role == FenceRole::Inferred
                    && matches!(glyph, "" | "|" | "‖" | "∣" | "∥" | "/"))
            {
                middles.push((i, sid));
            } else if bars
                || !matches!(role, FenceRole::Prefix | FenceRole::Postfix)
                || !matches!(glyph, "|" | "‖" | "∣" | "∥")
            {
                return None;
            }
        }
        if !inner_pairs.is_empty() {
            return None;
        }
        if bars && middles.is_empty() && !explicit {
            return None;
        }
        for (i, sid) in middles {
            content[i] = self.arena.push_node(CompactNode::Middle(sid));
        }
        let content = self.arena.push_children_deferred(content);
        Some(self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        }))
    }

    fn fence_delimiter(&self, id: NodeId) -> Option<StrId> {
        match *self.arena.node(id) {
            CompactNode::Symbol(_, sid) => Some(sid),
            _ => None,
        }
    }

    /// A sole mstyle child can set an enclosure's body to text size. Record its
    /// actual presentation attributes independently of the child's font variant.
    fn read_children(
        &mut self,
        parent_tag: &str,
        inspect_style: bool,
    ) -> Result<(SmallVec<[NodeId; 8]>, bool)> {
        self.read_children_with_script_modes(parent_tag, inspect_style, None)
    }

    /// Read children while optionally suppressing the automatic script-style
    /// transition for selected positional children. Accents are laid out in
    /// the base style, while ordinary under/over scripts use one smaller
    /// style. Keeping this policy at the child reader boundary also preserves
    /// nested `mstyle` relative levels.
    fn read_children_with_script_modes(
        &mut self,
        parent_tag: &str,
        inspect_style: bool,
        script_modes: Option<&[ScriptChildMode]>,
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
                    let child_style = script_modes
                        .and_then(|modes| modes.get(nodes.len()).copied())
                        .unwrap_or(ScriptChildMode::Script);
                    let previous = match child_style {
                        ScriptChildMode::Script => {
                            self.enter_script_child_style(parent_tag, nodes.len())
                        },
                        ScriptChildMode::Accent => {
                            self.enter_accent_child_style(parent_tag, nodes.len())
                        },
                    };
                    let id = if inspect_style
                        && nodes.is_empty()
                        && name == "mstyle"
                        && has_explicit_text_size(&e)
                    {
                        body_in_text_style = true;
                        self.parse_mstyle(&e, true)?
                    } else {
                        self.parse_element_by_name(&name, &e)?
                    };
                    self.restore_style(previous);
                    nodes.push(id);
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    if inspect_style && nodes.is_empty() && name == "mstyle" {
                        body_in_text_style = has_explicit_text_size(&e);
                    }
                    let child_style = script_modes
                        .and_then(|modes| modes.get(nodes.len()).copied())
                        .unwrap_or(ScriptChildMode::Script);
                    let previous = match child_style {
                        ScriptChildMode::Script => {
                            self.enter_script_child_style(parent_tag, nodes.len())
                        },
                        ScriptChildMode::Accent => {
                            self.enter_accent_child_style(parent_tag, nodes.len())
                        },
                    };
                    let id = self.parse_empty_element(&name, &e)?;
                    self.restore_style(previous);
                    nodes.push(id);
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
        // A sole text-sized body is represented by the enclosure itself. With
        // siblings the style belongs only to the first child.
        if body_in_text_style && !sized && !self.is_empty_style_subtree(nodes[0]) {
            let marker = self.arena.push_node(CompactNode::StyleSwitch(1));
            nodes[0] = self.flatten_ids(&[marker, nodes[0]]);
        }
        Ok((nodes, sized))
    }

    fn parse_child_id(&mut self, parent_tag: &str) -> Result<NodeId> {
        let children = self.parse_children(parent_tag)?;
        Ok(self.flatten_ids(&children))
    }

    fn enter_script_child_style(
        &mut self,
        parent_tag: &str,
        child_index: usize,
    ) -> Option<(bool, i64)> {
        let level_delta = match parent_tag {
            // MathML's fraction rule sets displaystyle to false. It only
            // increments scriptlevel when the incoming style is already
            // compact, so display fractions use text size and compact ones
            // move one level deeper.
            "mfrac" if child_index < 2 => Some(if self.style_display { 0 } else { 1 }),
            // A root index is two script levels below its base.
            "mroot" if child_index == 1 => Some(2),
            // Script and limit schemata compact every child after the base.
            "msub" | "msup" | "msubsup" | "munder" | "mover" | "munderover" if child_index > 0 => {
                Some(1)
            },
            // Each post- or pre-script pair in `mmultiscripts` is rendered at
            // the compact script level. The parser applies this transition
            // around every child after the base, including both sides of the
            // `mprescripts` marker.
            "mmultiscripts" if child_index > 0 => Some(1),
            _ => None,
        }?;
        let previous = (self.style_display, self.style_level);
        self.style_display = false;
        self.style_level = self.style_level.saturating_add(level_delta);
        Some(previous)
    }

    /// Accents switch to compact displaystyle but keep the inherited
    /// scriptlevel. MathML Core applies this displaystyle change even when an
    /// accent attribute suppresses the size reduction.
    fn enter_accent_child_style(
        &mut self,
        parent_tag: &str,
        child_index: usize,
    ) -> Option<(bool, i64)> {
        if !matches!(parent_tag, "munder" | "mover" | "munderover") || child_index == 0 {
            return None;
        }
        let previous = (self.style_display, self.style_level);
        self.style_display = false;
        Some(previous)
    }

    fn restore_style(&mut self, previous: Option<(bool, i64)>) {
        if let Some((display, level)) = previous {
            self.style_display = display;
            self.style_level = level;
        }
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

    /// Resolve the supported absolute and relative `mstyle` attributes.
    /// The AST has four TeX-style levels, so larger absolute levels clamp to
    /// scriptscript and mixed `displaystyle`/nonzero `scriptlevel` values use
    /// the script-level representation as the closest available style.
    fn scoped_style(&self, start: &quick_xml::events::BytesStart<'_>) -> Option<(bool, i64)> {
        let display_attr = get_attr(start, "displaystyle");
        let level_attr = get_attr(start, "scriptlevel");
        if display_attr.is_none() && level_attr.is_none() {
            return None;
        }
        let mut display = self.style_display;
        let mut changed = false;
        if let Some(value) = display_attr {
            match value.trim().to_ascii_lowercase().as_str() {
                "true" => {
                    display = true;
                    changed = true;
                },
                "false" => {
                    display = false;
                    changed = true;
                },
                _ => {},
            }
        }
        let mut level = self.style_level;
        if let Some(value) = level_attr
            && let Some(parsed) = parse_scriptlevel(&value, self.style_level)
        {
            level = parsed;
            changed = true;
        }
        changed.then_some((display, level))
    }

    fn parse_mstyle(
        &mut self,
        start: &quick_xml::events::BytesStart<'_>,
        omit_text_marker: bool,
    ) -> Result<NodeId> {
        let previous = (self.style_display, self.style_level);
        let previous_variant = self.mathvariant_style;
        if let Some(variant) =
            get_attr(start, "mathvariant").and_then(|value| mathvariant_to_style(&value))
        {
            self.mathvariant_style = Some(variant);
        }
        let scoped = self.scoped_style(start);
        if let Some((display, level)) = scoped {
            self.style_display = display;
            self.style_level = level;
        }
        let children = self.parse_children("mstyle")?;
        self.style_display = previous.0;
        self.style_level = previous.1;
        self.mathvariant_style = previous_variant;

        // An empty style element has no descendant to which its attributes
        // can apply. Emitting a leading StyleSwitch here would incorrectly
        // style the next sibling when the compact AST is serialized.
        if children.is_empty() || children.iter().all(|&id| self.is_empty_style_subtree(id)) {
            return Ok(self.flatten_ids(&children));
        }

        let mut scoped_children: SmallVec<[NodeId; 8]> = SmallVec::new();
        if let Some(scoped) = scoped
            && !omit_text_marker
        {
            let marker = self
                .arena
                .push_node(CompactNode::StyleSwitch(self.style_marker_for(scoped)));
            scoped_children.push(marker);
        }
        scoped_children.extend(children);
        let content = self.flatten_ids(&scoped_children);
        let content = if let Some(color) =
            get_attr(start, "mathcolor").filter(|color| !color.trim().is_empty())
        {
            let color = self.arena.push_str(color.trim());
            self.arena.push_node(CompactNode::Color { color, content })
        } else {
            content
        };
        if let Some(variant) = get_attr(start, "mathvariant")
            && let Some(style) = mathvariant_to_style(&variant)
        {
            let span = self.arena.push_children_deferred(&[content]);
            return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
        }
        Ok(content)
    }

    fn is_empty_style_subtree(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Row(span) | CompactNode::Styled(_, span) => self
                .arena
                .children(span)
                .iter()
                .all(|&child| self.is_empty_style_subtree(child)),
            _ => false,
        }
    }

    fn style_marker_for(&self, style: (bool, i64)) -> u8 {
        match (style.0, style.1) {
            (true, 1) => crate::ast::style::MathStyle::DISPLAY_AT_SCRIPT,
            (true, 2..) => crate::ast::style::MathStyle::DISPLAY_AT_SCRIPTSCRIPT,
            (_, 1) => 2,
            (_, 2..) => 3,
            _ if style.0 => 0,
            _ => 1,
        }
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

    fn operator_token(&mut self, text: &str, start: &quick_xml::events::BytesStart<'_>) -> NodeId {
        let mut kind = classify_mo(text);
        let name = text.trim();
        // Multi-letter operator tokens and explicitly named operators
        // are distinct from ordinary identifiers in <mi>.
        let mark_fences = self.fence_depth == Some(self.depth);
        let vertical_line = matches!(name, "|" | "‖");
        let attrs = operator_attributes(start, mark_fences || vertical_line);
        if let Some(marked_kind) = attrs
            .symbol_kind
            .filter(|_| matches!(name, "-" | "−" | "*" | "'"))
        {
            kind = marked_kind;
        }
        if vertical_line
            && attrs.non_fence
            && attrs.fixed_literal
            && attrs.form == FenceRole::Inferred
        {
            kind = SymbolKind::Ordinary;
        }
        let movable = attrs.movable_limits;
        let mut chars = name.chars();
        let named = attrs.named_operator
            || kind == SymbolKind::Ordinary
                && chars.next().is_some_and(|first| {
                    first.is_alphabetic()
                        && (movable.is_some()
                            || !chars.as_str().is_empty() && chars.all(char::is_alphabetic))
                        || attrs.texclass_op
                });
        let sid = self.arena.push_str(if named { name } else { text });
        let id = if named {
            let limits = match movable {
                Some(true) => LimitStyle::Default,
                Some(false) => LimitStyle::AboveBelow,
                None if !attrs.named_operator
                    && crate::unicode::symbols::operator_has_limits(name) =>
                {
                    LimitStyle::Default
                },
                None => LimitStyle::SubSup,
            };
            self.arena
                .push_node(CompactNode::Operator { name: sid, limits })
        } else {
            self.arena.push_node(CompactNode::Symbol(kind, sid))
        };
        if attrs.stretchy && !named {
            self.wide_accents.insert(id);
        }

        if mark_fences && attrs.fenced {
            self.fence_marks.push((id, attrs.fence_role()));
        } else if mark_fences
            && attrs.fixed_literal
            && !attrs.constrained
            && attrs.form == FenceRole::Inferred
            && is_vertical_bar(name)
        {
            self.fence_marks.push((id, FenceRole::FixedLiteral));
        }
        if kind == SymbolKind::LargeOp {
            if let Some(value) = movable {
                self.movable_limit_operators.insert(id, value);
            }
        } else if named && movable == Some(true) {
            self.movable_limit_operators.insert(id, true);
        }
        id
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
                    && self.mathvariant_style != Some(style)
                    && (style != TextStyle::Italic || self.mathvariant_style.is_some())
                    && !(style == TextStyle::Normal
                        && text.chars().count() > 1
                        && self
                            .mathvariant_style
                            .is_none_or(|inherited| inherited == TextStyle::Normal))
                {
                    let inner = self.arena.push_node(CompactNode::Identifier(sid));
                    let span = self.arena.push_children_deferred(&[inner]);
                    return Ok(self.arena.push_node(CompactNode::Styled(style, span)));
                }
                Ok(self.arena.push_node(CompactNode::Identifier(sid)))
            },
            "mo" => {
                let text = self.read_text_content(name)?;
                Ok(self.operator_token(&text, start))
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
                let r = mspace_width(start);
                Ok(self.arena.push_node(CompactNode::Space(r)))
            },
            "mrow" => {
                let attrs = mrow_attributes(start);
                let reflected = match attrs.style.as_deref() {
                    Some(style) => has_horizontal_reflection(style),
                    None => matches!(attrs.reflect.as_deref(), Some("text" | "math")),
                };
                let mark_start = self.fence_marks.len();
                let enclosing = self.fence_depth;
                self.fence_depth = start
                    .attributes()
                    .next()
                    .is_none()
                    .then_some(self.depth + 1);
                let previous_variant = self.mathvariant_style;
                if attrs.atom_type == Some(MathAtomType::Op) && attrs.structured_operator.is_some()
                {
                    self.mathvariant_style = Some(TextStyle::Normal);
                }
                let result = self.read_children(name, reflected);
                self.mathvariant_style = previous_variant;
                let (mut children, sized) = result?;
                self.fence_depth = enclosing;
                let fenced = self.recover_fenced_row(&mut children, mark_start);
                self.fence_marks.truncate(mark_start);
                if let Some(fenced) = fenced {
                    return Ok(fenced);
                }
                if reflected {
                    let content = self.flatten_ids(&children);
                    let text_mode = attrs.reflect.as_deref() == Some("text")
                        || sized
                        || attrs.explicit_text_size;
                    Ok(self
                        .arena
                        .push_node(CompactNode::Reflect { content, text_mode }))
                } else if let Some(atom_type) = attrs.atom_type {
                    let content = self.flatten_ids(&children);
                    Ok(self.marked_atom(atom_type, content, &attrs))
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
                let accent = get_attr(start, "accent");
                let is_accent = accent.as_deref().is_some_and(is_true_mathml_boolean);
                let movable = !is_accent;
                let child_styles = [
                    ScriptChildMode::Script,
                    if is_accent {
                        ScriptChildMode::Accent
                    } else {
                        ScriptChildMode::Script
                    },
                ];
                let children = self
                    .read_children_with_script_modes(name, false, Some(&child_styles))
                    .map(|(children, _)| children)?;
                let (base, over) = self.two_child_ids(children);
                if let Some(node) = self.mathml_accent(
                    base,
                    over,
                    AccentPosition::Above,
                    is_accent,
                    get_attr(start, "data-mathema-accent-decoration").as_deref(),
                ) {
                    return Ok(node);
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
                let accent = get_attr(start, "accentunder");
                let is_accent = accent.as_deref().is_some_and(is_true_mathml_boolean);
                let child_styles = [
                    ScriptChildMode::Script,
                    if is_accent {
                        ScriptChildMode::Accent
                    } else {
                        ScriptChildMode::Script
                    },
                ];
                let children = self
                    .read_children_with_script_modes(name, false, Some(&child_styles))
                    .map(|(children, _)| children)?;
                let (base, under) = self.two_child_ids(children);
                if let Some(node) = self.mathml_accent(
                    base,
                    under,
                    AccentPosition::Below,
                    is_accent,
                    get_attr(start, "data-mathema-accent-decoration").as_deref(),
                ) {
                    return Ok(node);
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
                let accent = get_attr(start, "accent")
                    .as_deref()
                    .is_some_and(is_true_mathml_boolean);
                let accent_under = get_attr(start, "accentunder")
                    .as_deref()
                    .is_some_and(is_true_mathml_boolean);
                let child_styles = [
                    ScriptChildMode::Script,
                    if accent_under {
                        ScriptChildMode::Accent
                    } else {
                        ScriptChildMode::Script
                    },
                    if accent {
                        ScriptChildMode::Accent
                    } else {
                        ScriptChildMode::Script
                    },
                ];
                let children = self
                    .read_children_with_script_modes(name, false, Some(&child_styles))
                    .map(|(children, _)| children)?;
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
                let table_columns = parse_column_aligns(get_attr(start, "columnalign"));
                let (rows, columns, row_alignments) =
                    self.with_table_style(start, true, |p| p.parse_table(name, &table_columns))?;
                self.finish_table(start, rows, columns, row_alignments)
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
            "mstyle" => self.parse_mstyle(start, false),
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

    fn marked_atom(
        &mut self,
        atom_type: MathAtomType,
        content: NodeId,
        attrs: &MrowAttributes,
    ) -> NodeId {
        if atom_type == MathAtomType::Op
            && let Some(form) = attrs.structured_operator
        {
            return self
                .arena
                .push_node(CompactNode::StructuredOperator { content, form });
        }
        let id = self
            .arena
            .push_node(CompactNode::MathAtom { atom_type, content });
        if attrs.movable_limits && atom_type == MathAtomType::Op {
            self.movable_limit_operators.insert(id, true);
        }
        id
    }

    fn parse_empty_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<NodeId> {
        match name {
            "mtable" => self.finish_table(start, Vec::new(), Vec::new(), Vec::new()),
            "mrow" => {
                let attrs = mrow_attributes(start);
                let empty = self.empty_row();
                Ok(if let Some(atom_type) = attrs.atom_type {
                    self.marked_atom(atom_type, empty, &attrs)
                } else {
                    empty
                })
            },
            "mo" => Ok(self.operator_token("", start)),
            "mspace" => {
                let r = mspace_width(start);
                Ok(self.arena.push_node(CompactNode::Space(r)))
            },
            "mprescripts" => Ok(self.empty_row()),
            "none" => Ok(self.empty_row()),
            "mstyle" => Ok(self.empty_row()),
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

    /// Tables start in compact display style and inherit the surrounding
    /// script level. Rows and cells can override either presentation hint.
    fn with_table_style<T>(
        &mut self,
        start: &quick_xml::events::BytesStart<'_>,
        table_root: bool,
        parse: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = (self.style_display, self.style_level);
        if table_root {
            self.style_display = false;
        }
        if let Some((display, level)) = self.scoped_style(start) {
            self.style_display = display;
            self.style_level = level;
        }
        let result = parse(self);
        (self.style_display, self.style_level) = previous;
        result
    }

    fn parse_table_cell(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        let mut nodes = self.parse_children("mtd")?;
        // The MathML writer represents a cell's inherited TeX style as a
        // leading mstyle. Parsing that mstyle naturally creates one wrapper
        // Row, while LaTeX table cells store the marker directly in their
        // cell Row. Flatten only this marker-bearing wrapper so compact
        // reimport can recognize and remove the environment-provided style.
        if nodes.len() == 1
            && let CompactNode::Row(span) = *self.arena.node(nodes[0])
            && let Some(&CompactNode::StyleSwitch(_)) = self
                .arena
                .children(span)
                .first()
                .map(|&id| self.arena.node(id))
        {
            nodes = self.arena.children(span).iter().copied().collect();
        }
        let marker = self.style_marker_for((self.style_display, self.style_level));
        if marker != 1
            && nodes
                .first()
                .is_some_and(|&id| !matches!(self.arena.node(id), CompactNode::StyleSwitch(_)))
        {
            nodes.insert(0, self.arena.push_node(CompactNode::StyleSwitch(marker)));
        }
        Ok(nodes)
    }

    fn finish_table(
        &mut self,
        start: &quick_xml::events::BytesStart<'_>,
        rows: MathmlTableRows,
        columns: Vec<ColumnAlign>,
        row_alignments: Vec<Vec<ColumnAlign>>,
    ) -> Result<NodeId> {
        let ncols = columns.len();
        let stride = ncols.max(1);
        if rows.len().saturating_mul(stride) > AstArena::MAX_CHILDREN {
            return Err(Error::LimitExceeded {
                kind: crate::error::LimitKind::ListLength,
                limit: AstArena::MAX_CHILDREN,
            });
        }
        let num_cols = stride as u16;

        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for &align in &columns {
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(align)));
        }
        let col_aligns = self.arena.push_children_deferred(&align_ids);

        let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (row_index, row) in rows.iter().enumerate() {
            for (cell_index, cell_children) in row.iter().enumerate() {
                let cell_id = if let Some(alignment) = row_alignments
                    .get(row_index)
                    .and_then(|row| row.get(cell_index))
                    .copied()
                    .filter(|alignment| columns.get(cell_index) != Some(alignment))
                {
                    let content = self.flatten_ids(cell_children);
                    self.arena
                        .push_node(CompactNode::AlignedCell { alignment, content })
                } else {
                    let span = self.arena.push_children_deferred(cell_children);
                    self.arena.push_node(CompactNode::Row(span))
                };
                cell_ids.push(cell_id);
            }
            // Pad short rows
            for _ in row.len()..(num_cols as usize) {
                cell_ids.push(self.empty_row());
            }
        }
        let cells = self.arena.push_children_deferred(&cell_ids);
        let arraystretch_pct = arraystretch_percent(start).unwrap_or(100);
        let array = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        if let Some(environment) = array_environment(start, arraystretch_pct) {
            self.arena.mark_array_environment(array, environment);
        }
        Ok(array)
    }

    fn parse_table(
        &mut self,
        parent_tag: &str,
        table_columns: &[ColumnAlign],
    ) -> Result<(MathmlTableRows, Vec<ColumnAlign>, Vec<Vec<ColumnAlign>>)> {
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        let mut row_alignments = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mtr" || name == "mlabeledtr" {
                        let authored_row_alignments =
                            parse_column_aligns(get_attr(&e, "columnalign"));
                        let inherited = if authored_row_alignments.is_empty() {
                            table_columns
                        } else {
                            &authored_row_alignments
                        };
                        let (row, row_columns) = self
                            .with_table_style(&e, false, |p| p.parse_table_row(&name, inherited))?;
                        // Alignment declarations do not create cells. A later
                        // row can supply columns absent from earlier ragged rows.
                        if row_columns.len() > columns.len() {
                            columns.extend_from_slice(&row_columns[columns.len()..]);
                        }
                        rows.push(row);
                        row_alignments.push(row_columns);
                    }
                },
                Ok(Event::Empty(e))
                    if matches!(Self::local_name(&e).as_str(), "mtr" | "mlabeledtr") =>
                {
                    rows.push(Vec::new());
                    row_alignments.push(Vec::new());
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
        Ok((rows, columns, row_alignments))
    }

    fn parse_table_row(
        &mut self,
        parent_tag: &str,
        inherited: &[ColumnAlign],
    ) -> Result<(MathmlTableRow, Vec<ColumnAlign>)> {
        let mut cells = Vec::new();
        let mut columns = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mtd" {
                        let index = cells.len();
                        columns.push(cell_alignment(
                            inherited,
                            index,
                            get_attr(&e, "columnalign"),
                        ));
                        cells.push(self.with_table_style(&e, false, Self::parse_table_cell)?);
                    } else {
                        // Inferred cell: a non-<mtd> child is its own cell.
                        columns.push(cell_alignment(inherited, cells.len(), None));
                        let id = self.parse_element_by_name(&name, &e)?;
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::Empty(e)) => {
                    let name = Self::local_name(&e);
                    if name == "mtd" {
                        let index = cells.len();
                        columns.push(cell_alignment(
                            inherited,
                            index,
                            get_attr(&e, "columnalign"),
                        ));
                        cells.push(SmallVec::new());
                    } else {
                        columns.push(cell_alignment(inherited, cells.len(), None));
                        let id = self.parse_empty_element(&name, &e)?;
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::Text(e)) => {
                    let text = unescape_text(&e);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        columns.push(cell_alignment(inherited, cells.len(), None));
                        cells.push(smallvec::smallvec![id]);
                    }
                },
                Ok(Event::GeneralRef(r)) => {
                    let text = resolve_general_ref(&r);
                    if !text.trim().is_empty() {
                        let id = self.classify_text_compact(&text);
                        columns.push(cell_alignment(inherited, cells.len(), None));
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
        Ok((cells, columns))
    }

    fn is_empty_row_id(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    fn mathml_accent(
        &mut self,
        base: NodeId,
        script: NodeId,
        position: AccentPosition,
        is_accent: bool,
        decoration: Option<&str>,
    ) -> Option<NodeId> {
        // A marker only disambiguates a matching accent; it cannot replace
        // either of the authored operands or change its base's limit policy.
        if !is_accent {
            return None;
        }
        let CompactNode::Symbol(_, sid) = *self.arena.node(script) else {
            return None;
        };
        let glyph = self.arena.str_val(sid);
        let mut ch = single_accent_char(glyph)?;
        if let Some(name) = crate::unicode::symbols::fixed_script_decoration(glyph, position)
            && decoration == Some(name)
        {
            return Some(self.arena.push_node(match position {
                AccentPosition::Above => CompactNode::Overscript {
                    base,
                    over: script,
                    movable_limits: false,
                },
                AccentPosition::Below => CompactNode::Underscript {
                    base,
                    under: script,
                    movable_limits: false,
                },
            }));
        }
        if ch == '‾' && decoration == Some("combining-macron") {
            ch = '\u{0304}';
        }
        let mut core = base;
        while let CompactNode::Row(span) = *self.arena.node(core) {
            let &[only] = self.arena.children(span) else {
                break;
            };
            core = only;
        }
        if self.movable_limit_operators.get(&core) != Some(&true)
            && let CompactNode::Operator { limits, .. } = self.arena.node_mut(core)
            && *limits == LimitStyle::Default
        {
            *limits = LimitStyle::AboveBelow;
        }
        Some(self.arena.push_node(CompactNode::Accent {
            base,
            accent: ch,
            position,
            wide: self.wide_accents.contains(&script),
        }))
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
            CompactNode::StructuredOperator { content, form } => {
                let expected_stacked = match form.limit_style() {
                    LimitStyle::AboveBelow => true,
                    LimitStyle::Default => self.style_display,
                    LimitStyle::SubSup => false,
                };
                // A movable operator legitimately uses msub in inline MathML.
                // Keep its original form when the visible placement agrees.
                if expected_stacked != stacked {
                    let effective = if stacked {
                        LimitStyle::AboveBelow
                    } else {
                        LimitStyle::SubSup
                    };
                    *self.arena.node_mut(base) = CompactNode::StructuredOperator {
                        content,
                        form: OperatorNameForm::Starred(effective),
                    };
                }
                Some(self.make_script_pair(base, sub, sup))
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
                        let previous = seen_base
                            .then(|| self.enter_script_child_style(parent_tag, 1))
                            .flatten();
                        let id = self.parse_element_by_name(&name, &e)?;
                        self.restore_style(previous);
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
                        let previous = seen_base
                            .then(|| self.enter_script_child_style(parent_tag, 1))
                            .flatten();
                        let id = self.parse_empty_element(&name, &e)?;
                        self.restore_style(previous);
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
                        let previous = seen_base
                            .then(|| self.enter_script_child_style(parent_tag, 1))
                            .flatten();
                        let id = self.classify_text_compact(&text);
                        self.restore_style(previous);
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
                        let previous = seen_base
                            .then(|| self.enter_script_child_style(parent_tag, 1))
                            .flatten();
                        let id = self.classify_text_compact(&text);
                        self.restore_style(previous);
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

#[inline]
fn is_true_mathml_boolean(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("true")
}

/// Parse MathML's absolute or relative scriptlevel syntax. Invalid values are
/// ignored by the caller, just as an unsupported presentation hint would be.
/// The parser keeps levels above scriptscript until the final TeX-style marker
/// is selected, so a later relative decrement still has its authored meaning.
fn parse_scriptlevel(value: &str, current: i64) -> Option<i64> {
    let value = value.trim();
    let (sign, digits) = match value.as_bytes().first().copied() {
        Some(b'+') => (1i8, &value[1..]),
        Some(b'-') => (-1i8, &value[1..]),
        _ => (0i8, value),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let amount = digits.parse::<i64>().ok()?;
    Some(match sign {
        1 => current.saturating_add(amount),
        -1 => current.saturating_sub(amount).max(0),
        _ => amount,
    })
}

/// Read the writer's narrowly scoped table multiplier marker. The value is
/// expressed as a percentage so it remains independent of the current MathML
/// font size and can be reconstructed without guessing from `scriptlevel`.
fn arraystretch_percent(start: &quick_xml::events::BytesStart<'_>) -> Option<u8> {
    let value = get_attr(start, "data-mathema-arraystretch")?;
    let value = value.trim().strip_suffix('%')?.trim();
    let value = value.parse::<u16>().ok()?;
    u8::try_from(value).ok()
}

fn array_environment(
    start: &quick_xml::events::BytesStart<'_>,
    arraystretch_pct: u8,
) -> Option<MatrixKind> {
    match get_attr(start, "data-mathema-array-environment")?.trim() {
        "matrix" if arraystretch_pct == 100 => Some(MatrixKind::Matrix),
        "smallmatrix" if arraystretch_pct == 50 => Some(MatrixKind::SmallMatrix),
        _ => None,
    }
}

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

#[derive(Default)]
struct OperatorAttributes {
    movable_limits: Option<bool>,
    symbol_kind: Option<SymbolKind>,
    named_operator: bool,
    texclass_op: bool,
    fenced: bool,
    form: FenceRole,
    constrained: bool,
    fixed_literal: bool,
    stretchy: bool,
    non_fence: bool,
}

impl OperatorAttributes {
    fn fence_role(&self) -> FenceRole {
        if self.constrained || self.fixed_literal {
            FenceRole::Constrained
        } else {
            self.form
        }
    }
}

/// The LaTeX delimiter scanner accepts a single glyph or a null delimiter.
fn writable_middle(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_none_or(|c| {
        chars.as_str().is_empty()
            && !c.is_ascii_alphanumeric()
            && !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '%' | '#' | '$' | '&' | '~' | '^' | '_' | '.')
    })
}

fn known_fence_endpoint(text: &str) -> bool {
    if matches!(text, "" | "/" | "\\" | "|" | "‖" | "∣" | "∥") {
        return true;
    }
    let mut chars = text.chars();
    chars.next().is_some_and(|ch| {
        chars.as_str().is_empty()
            && (crate::unicode::operators::is_stretchy(ch)
                || matches!(
                    classify_mo(text),
                    SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence
                ))
    })
}

#[inline]
fn is_vertical_bar(text: &str) -> bool {
    matches!(text, "|" | "‖" | "∣" | "∥")
}

fn fence_attr_value(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.contains('&') {
        unescape_attr(raw).into()
    } else {
        raw.into()
    }
}

/// Read the small attribute subset used by an operator in one pass. Attribute
/// values still use the same entity resolver as the general path.
fn operator_attributes(
    start: &quick_xml::events::BytesStart<'_>,
    inspect_fences: bool,
) -> OperatorAttributes {
    let mut result = OperatorAttributes::default();
    for attr in start.attributes().flatten() {
        match attr.key.as_ref() {
            "fence" if inspect_fences => {
                let value = fence_attr_value(&attr.value);
                let value = value.trim();
                result.non_fence = value.eq_ignore_ascii_case("false");
                result.fenced = value.eq_ignore_ascii_case("true");
            },
            "stretchy" => {
                let value = fence_attr_value(&attr.value);
                let value = value.trim();
                result.stretchy = value.eq_ignore_ascii_case("true");
                if inspect_fences {
                    result.fixed_literal = value.eq_ignore_ascii_case("false");
                    result.constrained |= !result.fixed_literal && !result.stretchy;
                }
            },
            "symmetric" if inspect_fences => {
                result.constrained |= !fence_attr_value(&attr.value)
                    .trim()
                    .eq_ignore_ascii_case("true");
            },
            "form" if inspect_fences => {
                let value = fence_attr_value(&attr.value);
                result.form = match value.trim() {
                    s if s.eq_ignore_ascii_case("prefix") => FenceRole::Prefix,
                    s if s.eq_ignore_ascii_case("infix") => FenceRole::Infix,
                    s if s.eq_ignore_ascii_case("postfix") => FenceRole::Postfix,
                    _ => FenceRole::Constrained,
                };
            },
            "minsize" | "maxsize" | "lspace" | "rspace" | "style" | "mathsize" | "mathvariant"
            | "mathcolor" | "mathbackground" | "class" | "displaystyle" | "scriptlevel"
                if inspect_fences =>
            {
                result.constrained = true;
            },
            "movablelimits" => {
                if result.movable_limits.is_none() {
                    result.movable_limits = match unescape_attr(&attr.value).as_str() {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => None,
                    };
                }
            },
            "data-mathema-operator" => {
                result.named_operator = unescape_attr(&attr.value) == "name";
            },
            "data-mjx-texclass" => {
                let class = unescape_attr(&attr.value);
                result.texclass_op = class == "OP";
                result.symbol_kind = match class.as_str() {
                    "ORD" => Some(SymbolKind::Ordinary),
                    "BIN" => Some(SymbolKind::BinaryOp),
                    _ => None,
                };
            },
            _ => {},
        }
    }
    result
}

#[derive(Default)]
struct MrowAttributes {
    reflect: Option<String>,
    style: Option<String>,
    atom_type: Option<MathAtomType>,
    structured_operator: Option<OperatorNameForm>,
    movable_limits: bool,
    explicit_text_size: bool,
}

/// Read presentation attributes needed by an mrow in one pass. Unknown
/// attributes remain ignored as before, while known values retain entity
/// decoding and the existing case-sensitive comparisons.
fn mrow_attributes(start: &quick_xml::events::BytesStart<'_>) -> MrowAttributes {
    let mut result = MrowAttributes::default();
    let mut display = None;
    let mut scriptlevel = None;
    for attr in start.attributes().flatten() {
        match attr.key.as_ref() {
            "data-mathema-reflect" if result.reflect.is_none() => {
                result.reflect = Some(unescape_attr(&attr.value));
            },
            "style" if result.style.is_none() => {
                result.style = Some(unescape_attr(&attr.value));
            },
            "data-mjx-texclass" => {
                result.atom_type = match unescape_attr(&attr.value).as_str() {
                    "OPEN" => Some(MathAtomType::Open),
                    "CLOSE" => Some(MathAtomType::Close),
                    "OP" => Some(MathAtomType::Op),
                    _ => None,
                };
            },
            "data-mathema-limits" => {
                result.movable_limits = unescape_attr(&attr.value) == "movable";
            },
            "data-mathema-structured-operator" => {
                result.structured_operator = match unescape_attr(&attr.value).as_str() {
                    "starred-default" => Some(OperatorNameForm::Starred(LimitStyle::Default)),
                    "starred-limits" => Some(OperatorNameForm::Starred(LimitStyle::AboveBelow)),
                    "starred-nolimits" => Some(OperatorNameForm::Starred(LimitStyle::SubSup)),
                    "unstarred" => Some(OperatorNameForm::Unstarred),
                    _ => None,
                };
            },
            "displaystyle" if display.is_none() => {
                display = Some(unescape_attr(&attr.value));
            },
            "scriptlevel" if scriptlevel.is_none() => {
                scriptlevel = Some(unescape_attr(&attr.value));
            },
            _ => {},
        }
    }
    result.explicit_text_size = display
        .as_deref()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("false"))
        && scriptlevel.as_deref().is_some_and(|value| {
            let value = value.trim();
            !value.is_empty() && value.bytes().all(|byte| byte == b'0')
        });
    result
}

fn get_attr(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| unescape_attr(&a.value))
}

fn column_alignment(value: &str) -> ColumnAlign {
    if value.eq_ignore_ascii_case("left") {
        ColumnAlign::Left
    } else if value.eq_ignore_ascii_case("right") {
        ColumnAlign::Right
    } else {
        ColumnAlign::Center
    }
}

fn parse_column_aligns(value: Option<String>) -> SmallVec<[ColumnAlign; 4]> {
    value
        .as_deref()
        .unwrap_or_default()
        .split_ascii_whitespace()
        .take(AstArena::MAX_CHILDREN)
        .map(column_alignment)
        .collect()
}

fn cell_alignment(inherited: &[ColumnAlign], index: usize, cell: Option<String>) -> ColumnAlign {
    if let Some(value) = cell
        .as_deref()
        .and_then(|v| v.split_ascii_whitespace().next())
    {
        return column_alignment(value);
    }
    inherited
        .get(index)
        .or_else(|| inherited.last())
        .copied()
        .unwrap_or(ColumnAlign::Center)
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
        '+' | '-' | '\u{2212}' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}'
        | '\u{2217}' | '\u{2218}' | '*' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}'
        | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2208}' | '\u{2223}'
        | '\u{2225}' | '\u{2192}' | '\u{2190}' | '\u{21A4}' | '\u{21A6}' | '\u{21D2}'
        | '\u{21D0}' => SymbolKind::Relation,
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

fn single_accent_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

fn mspace_width(start: &quick_xml::events::BytesStart<'_>) -> Rational {
    fn em_length(value: &str) -> Option<Rational> {
        Rational::from_scaled_decimal(value.trim().strip_suffix("em")?.trim(), Rational::one())
    }
    let width = get_attr(start, "width").as_deref().and_then(em_length);
    if width == Some(Rational::zero())
        && let Some(style) = get_attr(start, "style")
        && let Some((key, value)) = style.trim().trim_end_matches(';').split_once(':')
        && key.trim().eq_ignore_ascii_case("margin-inline-start")
        && let Some(margin) = em_length(value)
        && margin.num < 0
    {
        return margin;
    }
    width.unwrap_or_else(Rational::zero)
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
    use crate::writer::Writer;

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
    fn accent_markers_require_matching_glyph_and_accent_direction() {
        let invalid_bracket = r#"<math><munder data-mathema-accent-decoration="underbracket" accentunder="true"><mi>x</mi><mo>‾</mo></munder></math>"#;
        let result = crate::from_mathml(invalid_bracket).unwrap();
        assert!(result.to_latex().unwrap().contains(r"\underset"));
        assert!(!result.to_latex().unwrap().contains(r"\underbracket"));

        let contradictory = r#"<math><mover data-mathema-accent-decoration="overline" accent="false"><mi>x</mi><mo>‾</mo></mover></math>"#;
        let result = crate::from_mathml(contradictory).unwrap();
        assert!(matches!(
            result.expr().nodes.first(),
            Some(MathNode::Overscript {
                movable_limits: true,
                ..
            })
        ));
        assert!(result.to_latex().unwrap().contains('x'));

        let marked_limit = r#"<math><mover data-mathema-accent-decoration="overline" accent="true"><mo movablelimits="false">lim</mo><mo>‾</mo></mover></math>"#;
        let result = crate::from_mathml(marked_limit).unwrap();
        assert!(result.to_latex().unwrap().contains(r"\limits"));
    }

    #[test]
    fn standard_stretchy_accent_glyph_restores_wide_shape() {
        let source = r#"<math><munder accentunder="true"><mi>x</mi><mo stretchy="true">⎵</mo></munder></math>"#;
        let result = crate::from_mathml(source).unwrap();
        let output = result.to_mathml().unwrap();
        assert!(output.contains(r#"stretchy="true""#));
        assert_eq!(result.to_latex().unwrap(), r"\underbracket{x}");
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
    fn normal_multiletter_identifiers_do_not_gain_nested_style_wrappers() {
        let ordinary = r#"<math><mi mathvariant="normal">xy</mi></math>"#;
        let ordinary_output = crate::from_mathml(ordinary).unwrap().to_mathml().unwrap();
        assert!(
            !ordinary_output.contains("<mstyle mathvariant=\"normal\">"),
            "{ordinary_output}"
        );
        assert_eq!(
            crate::from_mathml(&ordinary_output)
                .unwrap()
                .to_mathml()
                .unwrap(),
            ordinary_output
        );

        let single = r#"<math><mi mathvariant="normal">x</mi></math>"#;
        let single_output = crate::from_mathml(single).unwrap().to_mathml().unwrap();
        assert!(
            single_output.contains("mathvariant=\"normal\""),
            "{single_output}"
        );
        assert_eq!(
            crate::from_mathml(&single_output)
                .unwrap()
                .to_mathml()
                .unwrap(),
            single_output
        );

        for variant in ["bold", "italic", "bold-italic"] {
            let input = format!(
                r#"<math><mstyle mathvariant="{variant}"><mi mathvariant="normal">xy</mi><mi>z</mi></mstyle></math>"#
            );
            let output = crate::from_mathml(&input).unwrap().to_mathml().unwrap();
            assert!(
                output.contains(
                    r#"<mstyle mathvariant="normal"><mi mathvariant="normal">xy</mi></mstyle>"#
                ),
                "{output}"
            );
            assert!(output.contains("<mi>z</mi>"), "{output}");
            assert_eq!(
                crate::from_mathml(&output).unwrap().to_mathml().unwrap(),
                output
            );
        }
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
    fn mstyle_preserves_scoped_display_and_script_levels() {
        assert_eq!(
            parse_one(
                r#"<math><mstyle displaystyle="false" scriptlevel="0"><mi>x</mi></mstyle></math>"#
            ),
            MathNode::Row(vec![
                MathNode::StyleSwitch(1),
                MathNode::Identifier("x".into()),
            ])
        );
        assert_eq!(
            parse_one(
                r#"<math display="inline"><mstyle scriptlevel="+1"><mi>x</mi></mstyle></math>"#
            ),
            MathNode::Row(vec![
                MathNode::StyleSwitch(2),
                MathNode::Identifier("x".into()),
            ])
        );
        assert_eq!(
            parse_one(
                r#"<math><mstyle scriptlevel="1"><mstyle scriptlevel="0"><mi>x</mi></mstyle></mstyle></math>"#
            ),
            MathNode::Row(vec![
                MathNode::StyleSwitch(4),
                MathNode::Row(vec![
                    MathNode::StyleSwitch(0),
                    MathNode::Identifier("x".into()),
                ]),
            ])
        );
        let nested = parse_one(
            r#"<math><msup><mi>x</mi><mstyle scriptlevel="+1"><mi>y</mi></mstyle></msup></math>"#,
        );
        assert!(matches!(
            nested,
            MathNode::Superscript {
                sup,
                ..
            } if *sup == MathNode::Row(vec![MathNode::StyleSwitch(3), MathNode::Identifier("y".into())])
        ));
    }

    #[test]
    fn mstyle_mathvariant_and_positional_arity_keep_their_scopes() {
        let styled = parse_one(
            r#"<math><mstyle mathvariant="bold" displaystyle="false" scriptlevel="0"><mi>x</mi></mstyle></math>"#,
        );
        assert_eq!(
            styled,
            MathNode::Styled(
                TextStyle::Bold,
                vec![MathNode::Row(vec![
                    MathNode::StyleSwitch(1),
                    MathNode::Identifier("x".into()),
                ])]
            )
        );

        let fraction = parse_one(
            r#"<math><mfrac><mstyle displaystyle="true"><mn>1</mn></mstyle><mn>2</mn></mfrac></math>"#,
        );
        assert!(matches!(fraction, MathNode::Fraction { .. }));
        let MathNode::Fraction { num, den, .. } = fraction else {
            unreachable!();
        };
        assert!(matches!(*num, MathNode::Row(_)));
        assert_eq!(*den, MathNode::Number("2".into()));
    }

    #[test]
    fn mstyle_mathcolor_materializes_in_the_heap_ast() {
        let node = parse_one(r#"<math><mstyle mathcolor="red"><mi>x</mi></mstyle></math>"#);
        assert_eq!(
            node,
            MathNode::Color {
                color: "red".into(),
                content: Box::new(MathNode::Identifier("x".into())),
            }
        );

        let input = r#"<math><mstyle mathcolor="red"><mi>x</mi><mstyle mathcolor="blue"><mi>y</mi></mstyle><mi>z</mi></mstyle><mi>w</mi></math>"#;
        let result = crate::from_mathml(input).unwrap();
        let heap = result.expr();
        let latex = crate::writer::latex::LatexWriter::write(&heap, &Config::default()).unwrap();
        assert!(latex.contains(r#"\textcolor{red}"#), "{latex}");
        assert!(latex.contains(r#"\textcolor{blue}"#), "{latex}");
        assert_eq!(latex, result.to_latex().unwrap());

        let reparsed = crate::from_latex(&latex).unwrap();
        assert_eq!(reparsed.to_latex().unwrap(), latex);
    }

    #[test]
    fn colored_empty_script_bases_match_heap_and_compact_normalization() {
        for input in [
            r#"<math><msup><mstyle mathcolor="red"><mrow/></mstyle><mn>2</mn></msup></math>"#,
            r#"<math><msup><mstyle mathcolor="red"><mi>x</mi></mstyle><mrow/></msup></math>"#,
        ] {
            let config = Config::default();
            let mut compact = MathmlReader::parse_compact(input, &config).unwrap().expr;
            crate::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
            let compact_latex =
                crate::writer::latex::LatexWriter::write_compact(&compact, &config).unwrap();

            let mut heap = MathmlReader::parse(input, &config).unwrap().expr;
            crate::ast::normalize::normalize_expr(&mut heap);
            let heap_latex = crate::writer::latex::LatexWriter::write(&heap, &config).unwrap();
            assert_eq!(compact_latex, heap_latex, "{input}");
            assert_eq!(
                crate::from_latex(&compact_latex)
                    .unwrap()
                    .to_latex()
                    .unwrap(),
                compact_latex
            );
        }
    }

    #[cfg(feature = "render")]
    #[test]
    fn mstyle_mathcolor_renders_through_heap_and_compact_paths() {
        let input = r#"<math><mstyle mathcolor="red"><mi>x</mi><mstyle mathcolor="blue"><mi>y</mi></mstyle><mi>z</mi></mstyle><mi>w</mi></math>"#;
        let result = crate::from_mathml(input).unwrap();
        let options = crate::render::RenderOptions::default();
        let compact_svg = result.to_svg_text().unwrap();
        let heap_svg = crate::render::render_to_svg(&result.expr(), &options).unwrap();
        for svg in [compact_svg, heap_svg] {
            assert!(svg.contains(r#"fill="red""#), "{svg}");
            assert!(svg.contains(r#"fill="blue""#), "{svg}");
        }
    }

    #[test]
    fn normalized_heap_and_compact_rows_agree_on_unwritable_color() {
        let xml = r#"<math><mstyle mathcolor="red&amp;blue"><mrow><mi>x</mi><mo>+</mo><mi>y</mi></mrow></mstyle><mi>z</mi></math>"#;
        let cfg = Config::default();
        let mut heap = MathmlReader::parse(xml, &cfg).unwrap().expr;
        let mut compact = MathmlReader::parse_compact(xml, &cfg).unwrap().expr;
        crate::ast::normalize::normalize_expr(&mut heap);
        crate::ast::normalize::normalize_compact_expr(&mut compact).unwrap();
        assert_eq!(heap, compact.to_math_expr());
    }

    #[test]
    fn mstyle_mathcolor_survives_compact_reimport_and_stays_scoped() {
        let input = r#"<math><mstyle mathcolor="red"><mi>x</mi><mstyle mathcolor="blue"><mi>y</mi></mstyle><mi>z</mi></mstyle><mi>w</mi></math>"#;
        let result = crate::from_mathml(input).unwrap();
        let mathml = result.to_mathml().unwrap();
        assert!(mathml.contains(r#"<mstyle mathcolor="red">"#), "{mathml}");
        assert!(mathml.contains(r#"<mstyle mathcolor="blue">"#), "{mathml}");
        assert!(mathml.ends_with("<mi>w</mi></math>"), "{mathml}");

        let reparsed = crate::from_mathml(&mathml).unwrap();
        assert_eq!(reparsed.to_mathml().unwrap(), mathml);
        assert_eq!(reparsed.to_latex().unwrap(), result.to_latex().unwrap());

        #[cfg(feature = "render")]
        {
            let svg = result.to_svg_text().unwrap();
            assert!(svg.contains(r#"fill="red""#), "{svg}");
            assert!(svg.contains(r#"fill="blue""#), "{svg}");
        }
    }

    #[test]
    fn mstyle_mathcolor_combines_with_mathvariant_without_leaking() {
        let input = r##"<math><mstyle mathcolor="#6495ed" mathvariant="bold"><mi>x</mi></mstyle><mi>y</mi></math>"##;
        let result = crate::from_mathml(input).unwrap();
        let mathml = result.to_mathml().unwrap();
        assert!(mathml.contains(r##"mathcolor="#6495ed""##), "{mathml}");
        assert!(mathml.contains(r#"mathvariant="bold""#), "{mathml}");
        assert!(mathml.ends_with("<mi>y</mi></math>"), "{mathml}");
        assert_eq!(
            crate::from_mathml(&mathml).unwrap().to_mathml().unwrap(),
            mathml
        );
    }

    #[test]
    fn mmultiscripts_apply_script_style_to_post_and_pre_pairs() {
        let node = parse_one(
            r#"<math display="inline"><mmultiscripts>
                <mi>x</mi>
                <mstyle scriptlevel="+1"><mi>i</mi></mstyle>
                <mstyle scriptlevel="+1"><mn>2</mn></mstyle>
                <mprescripts/>
                <mstyle scriptlevel="+1"><mi>j</mi></mstyle>
                <mstyle scriptlevel="+1"><mn>3</mn></mstyle>
            </mmultiscripts></math>"#,
        );
        let expected_script = |node| MathNode::Row(vec![MathNode::StyleSwitch(3), node]);
        assert_eq!(
            node,
            MathNode::PreScripts {
                base: Box::new(MathNode::SubSup {
                    base: Box::new(MathNode::Identifier("x".into())),
                    sub: Box::new(expected_script(MathNode::Identifier("i".into()))),
                    sup: Box::new(expected_script(MathNode::Number("2".into()))),
                }),
                pre_sub: Some(Box::new(expected_script(MathNode::Identifier("j".into())))),
                pre_sup: Some(Box::new(expected_script(MathNode::Number("3".into())))),
            }
        );
    }

    #[test]
    fn accent_under_over_children_keep_base_style() {
        let accent_over = parse_one(
            r#"<math display="inline"><mover accent="true"><mi>x</mi><mstyle scriptlevel="+0"><mo>^</mo></mstyle></mover></math>"#,
        );
        let ordinary_over = parse_one(
            r#"<math display="inline"><mover accent="false"><mi>x</mi><mstyle scriptlevel="+0"><mo>^</mo></mstyle></mover></math>"#,
        );
        assert!(matches!(
            accent_over,
            MathNode::Overscript { over, movable_limits: false, .. }
                if *over == MathNode::Row(vec![MathNode::StyleSwitch(1), MathNode::Symbol(SymbolKind::Ordinary, "^".into())])
        ));
        assert!(matches!(
            ordinary_over,
            MathNode::Overscript { over, movable_limits: true, .. }
                if *over == MathNode::Row(vec![MathNode::StyleSwitch(2), MathNode::Symbol(SymbolKind::Ordinary, "^".into())])
        ));

        let accent_under_over = parse_one(
            r#"<math display="inline"><munderover accent="true" accentunder="true"><mi>x</mi><mstyle scriptlevel="+0"><mo>_</mo></mstyle><mstyle scriptlevel="+0"><mo>^</mo></mstyle></munderover></math>"#,
        );
        assert!(matches!(
            accent_under_over,
            MathNode::Overscript { base, over, movable_limits: false }
                if *over == MathNode::Row(vec![MathNode::StyleSwitch(1), MathNode::Symbol(SymbolKind::Ordinary, "^".into())])
                    && matches!(&*base, MathNode::Underscript { under, movable_limits: false, .. }
                        if **under == MathNode::Row(vec![MathNode::StyleSwitch(1), MathNode::Symbol(SymbolKind::Ordinary, "_".into())]))
        ));

        let display_accent = parse_one(
            r#"<math display="block"><mover accent="true"><mi>x</mi><mstyle scriptlevel="+0"><mo>^</mo></mstyle></mover></math>"#,
        );
        assert!(matches!(
            display_accent,
            MathNode::Overscript { over, movable_limits: false, .. }
                if *over == MathNode::Row(vec![MathNode::StyleSwitch(1), MathNode::Symbol(SymbolKind::Ordinary, "^".into())])
        ));
    }

    #[test]
    fn absent_or_invalid_accent_attributes_use_script_style() {
        for attr in [
            "",
            "accent=\"false\"",
            "accent=\"maybe\"",
            "accent=\"TRUE\"",
        ] {
            let attr = if attr.is_empty() {
                String::new()
            } else {
                format!(" {attr}")
            };
            let xml = format!(
                r#"<math display="inline"><mover{attr}><mi>x</mi><mstyle scriptlevel="+0"><mo>^</mo></mstyle></mover></math>"#
            );
            let node = parse_one(&xml);
            let expected_style = if attr.eq_ignore_ascii_case(" accent=\"TRUE\"") {
                1
            } else {
                2
            };
            assert!(
                matches!(
                    node,
                MathNode::Overscript { ref over, movable_limits: movable, .. }
                        if movable == (expected_style == 2)
                        && **over == MathNode::Row(vec![
                                MathNode::StyleSwitch(expected_style),
                                MathNode::Symbol(SymbolKind::Ordinary, "^".into()),
                            ])
                ),
                "{xml}: {node:?}"
            );
        }
    }

    #[test]
    fn empty_mstyle_does_not_style_following_siblings() {
        for input in [
            r#"<math><mstyle scriptlevel="1"/><mi>x</mi></math>"#,
            r#"<math><mstyle scriptlevel="1"></mstyle><mi>x</mi></math>"#,
            r#"<math><mstyle scriptlevel="1"><mrow/></mstyle><mi>x</mi></math>"#,
        ] {
            assert_eq!(
                parse_one(input),
                MathNode::Row(vec![
                    MathNode::Row(vec![]),
                    MathNode::Identifier("x".into())
                ]),
                "empty style leaked into a sibling for {input}"
            );
        }
        assert_eq!(
            parse_one(
                r#"<math><mstyle scriptlevel="1"><mrow/><mi>x</mi></mstyle><mi>y</mi></math>"#,
            ),
            MathNode::Row(vec![
                MathNode::Row(vec![
                    MathNode::StyleSwitch(4),
                    MathNode::Row(vec![]),
                    MathNode::Identifier("x".into()),
                ]),
                MathNode::Identifier("y".into()),
            ])
        );
    }

    #[test]
    fn mfrac_children_and_mroot_index_use_their_mathml_styles() {
        let fraction = parse_one(
            r#"<math display="block"><mfrac><mstyle scriptlevel="0"><mi>a</mi></mstyle><mstyle scriptlevel="0"><mi>b</mi></mstyle></mfrac></math>"#,
        );
        let MathNode::Fraction { num, den, .. } = fraction else {
            panic!("expected fraction")
        };
        let expected = |name: &str| {
            MathNode::Row(vec![
                MathNode::StyleSwitch(1),
                MathNode::Identifier(name.into()),
            ])
        };
        assert_eq!(*num, expected("a"));
        assert_eq!(*den, expected("b"));

        let root = parse_one(
            r#"<math display="block"><mroot><mi>x</mi><mstyle scriptlevel="-1"><mi>i</mi></mstyle></mroot></math>"#,
        );
        let MathNode::Root { index, .. } = root else {
            panic!("expected root")
        };
        assert_eq!(
            *index,
            MathNode::Row(vec![
                MathNode::StyleSwitch(2),
                MathNode::Identifier("i".into()),
            ])
        );
    }

    #[test]
    fn mstyle_relative_levels_are_applied_before_tex_clamping() {
        let node = parse_one(
            r#"<math><mstyle scriptlevel="4"><mstyle scriptlevel="-3"><mi>x</mi></mstyle></mstyle></math>"#,
        );
        assert_eq!(
            node,
            MathNode::Row(vec![
                MathNode::StyleSwitch(5),
                MathNode::Row(vec![
                    MathNode::StyleSwitch(4),
                    MathNode::Identifier("x".into()),
                ]),
            ])
        );
    }

    #[test]
    fn invalid_mstyle_hints_are_ignored_without_creating_markers() {
        assert_eq!(
            parse_one(
                r#"<math><mstyle displaystyle="perhaps" scriptlevel="+"><mi>x</mi></mstyle><mi>y</mi></math>"#,
            ),
            MathNode::Row(vec![
                MathNode::Identifier("x".into()),
                MathNode::Identifier("y".into()),
            ])
        );
        assert_eq!(
            parse_one(
                r#"<math><mstyle displaystyle="perhaps" scriptlevel="+1"><mi>x</mi></mstyle></math>"#,
            ),
            MathNode::Row(vec![
                MathNode::StyleSwitch(4),
                MathNode::Identifier("x".into()),
            ])
        );
    }

    #[test]
    fn mstyle_inside_text_sized_enclosures_keeps_existing_heuristic() {
        let node = parse_one(
            r#"<math><menclose notation="actuarial"><mstyle displaystyle="false" scriptlevel="0"><mi>x</mi></mstyle></menclose></math>"#,
        );
        assert!(matches!(
            node,
            MathNode::Actuarial {
                text_mode: true,
                ..
            }
        ));
    }

    #[test]
    fn enclosure_keeps_the_text_style_of_one_child_among_siblings() {
        let xml = r#"<math><menclose notation="actuarial"><mstyle displaystyle="false" scriptlevel="0" mathvariant="bold"><mi>x</mi></mstyle><mi>y</mi></menclose></math>"#;
        let MathNode::Actuarial { content, text_mode } = parse_one(xml) else {
            panic!("expected actuarial enclosure")
        };
        assert!(!text_mode);
        let MathNode::Row(children) = *content else {
            panic!("expected two children")
        };
        assert!(
            matches!(&children[..], [MathNode::Row(group), MathNode::Identifier(y)]
            if y == "y" && matches!(&group[..], [MathNode::StyleSwitch(1), MathNode::Styled(TextStyle::Bold, _)]))
        );
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
    fn writer_fences_reimport_with_middle_delimiters() {
        for source in [
            r"\left( x \middle| y \right)",
            r"\left| x \middle| y \right|",
        ] {
            let first = crate::from_latex(source).unwrap();
            let mathml = first.to_mathml().unwrap();
            let reparsed = crate::from_mathml(&mathml).unwrap();
            assert_eq!(reparsed.to_latex().unwrap(), first.to_latex().unwrap());
            assert!(matches!(
                reparsed.expr().nodes.as_slice(),
                [MathNode::Fenced { content, .. }]
                    if content.iter().any(|item| matches!(item, FencedItem::Middle(_)))
            ));
        }
    }

    #[test]
    fn literal_and_invisible_middle_delimiters_round_trip_in_their_outer_fence() {
        for source in [
            r"\left(x|y\right)",
            r"\left[x\middle. y\right]",
            r"\left(\left[x|y\right]\middle|z\right)",
        ] {
            let original = crate::from_latex(source).unwrap();
            let mathml = original.to_mathml().unwrap();
            let imported = crate::from_mathml(&mathml).unwrap();
            assert_eq!(imported.expr(), original.expr(), "{mathml}");
            assert_eq!(imported.to_mathml().unwrap(), mathml);
            assert_eq!(imported.to_latex().unwrap(), original.to_latex().unwrap());
            #[cfg(feature = "render")]
            assert_eq!(
                imported.to_svg_text().unwrap(),
                original.to_svg_text().unwrap()
            );
        }
        let empty = r#"<math><mrow><mo fence="true">(</mo><mo fence="true"/><mi>x</mi><mo fence="true">)</mo></mrow></math>"#;
        assert!(matches!(parse_one(empty), MathNode::Fenced { content, .. }
            if matches!(&content[..], [FencedItem::Middle(s), FencedItem::Element(_)] if s.is_empty())));
    }

    #[test]
    fn operator_attributes_are_local_to_each_direct_child() {
        let plus = r#"<math><mrow><mo fence="true">(</mo><mi>a</mi><mo form="infix">+</mo><mi>b</mi><mo fence="true">)</mo></mrow></math>"#;
        assert!(
            matches!(parse_one(plus), MathNode::Fenced { content, .. } if matches!(&content[1], FencedItem::Element(MathNode::Symbol(_, s)) if s == "+"))
        );
        let bars = r#"<math><mrow><mo fence="true" form="prefix">|</mo><mi>x</mi><mo fence="true" form="postfix">|</mo></mrow></math>"#;
        assert!(matches!(parse_one(bars), MathNode::Fenced { content, .. } if content.len() == 1));
        for constraint in [
            r#"maxsize="1em""#,
            r#"minsize="3em""#,
            r#"lspace="0.4em""#,
            r#"symmetric="false""#,
            r#"style="font-size: 10px""#,
        ] {
            let xml = format!(
                r#"<math><mrow><mo fence="true" {constraint}>(</mo><mi>x</mi><mo fence="true">)</mo></mrow></math>"#
            );
            assert!(matches!(parse_one(&xml), MathNode::Row(_)), "{xml}");
        }
    }

    #[test]
    fn infix_form_without_a_fence_does_not_create_a_middle() {
        for attributes in [
            r#"form="infix""#,
            r#"fence="false" form="infix""#,
            r#"stretchy="true" symmetric="true""#,
        ] {
            let xml = format!(
                r#"<math><mrow><mo fence="true">(</mo><mi>x</mi><mo {attributes}>|</mo><mi>y</mi><mo fence="true">)</mo></mrow></math>"#
            );
            assert!(
                matches!(parse_one(&xml), MathNode::Fenced {content,..}
                if content.iter().all(|v|matches!(v,FencedItem::Element(_)))),
                "{xml}"
            );
        }
        let original = crate::from_latex(r"\left(1\middle] 2\right)").unwrap();
        let reimport = crate::from_mathml(&original.to_mathml().unwrap()).unwrap();
        assert_eq!(original.to_latex().unwrap(), reimport.to_latex().unwrap());
    }

    #[test]
    fn adjacent_fence_pairs_and_explicit_endpoint_roles_are_preserved() {
        for inputs in [
            r#"<mo fence="true">(</mo><mi>a</mi><mo>)</mo><mo>(</mo><mi>b</mi><mo fence="true">)</mo>"#,
            r#"<mo fence="true" form="prefix">|</mo><mi>a</mi><mo>|</mo><mo>|</mo><mi>b</mi><mo fence="true" form="postfix">|</mo>"#,
            r#"<mo fence="true">(</mo><mi>a</mi><mo fence="true">)</mo><mo fence="true">(</mo><mi>b</mi><mo fence="true">)</mo>"#,
            r#"<mo fence="true" form="postfix">(</mo><mi>a</mi><mo fence="true">)</mo>"#,
            r#"<mo fence="true">(</mo><mi>a</mi><mo fence="true" form="infix">)</mo>"#,
        ] {
            let xml = format!("<math><mrow>{inputs}</mrow></math>");
            assert!(matches!(parse_one(&xml), MathNode::Row(_)), "{xml}");
        }
    }

    #[test]
    fn ambiguous_or_one_sided_fence_rows_stay_rows() {
        let one_sided = r#"<math><mrow><mo fence="true">|</mo><mi>x</mi></mrow></math>"#;
        assert!(matches!(
            parse_one(one_sided),
            MathNode::Row(children)
                if matches!(children.first(), Some(MathNode::Symbol(SymbolKind::Fence, _)))
        ));

        let literal = r#"<math><mrow><mo fence="true">(</mo><mi>x</mi><mo fence="true" stretchy="true" form="prefix">|</mo><mi>y</mi><mo fence="true">)</mo></mrow></math>"#;
        assert!(
            matches!(parse_one(literal), MathNode::Fenced { content, .. }
            if content.iter().all(|item| matches!(item, FencedItem::Element(_))))
        );

        let fixed = r#"<math><mrow><mo fence="true" stretchy="false">(</mo><mi>x</mi><mo fence="true" stretchy="false">)</mo></mrow></math>"#;
        assert!(matches!(parse_one(fixed), MathNode::Row(_)));
    }

    #[test]
    #[cfg(feature = "render")]
    fn smallmatrix_arraystretch_survives_mathml_reimport() {
        for source in [
            r"\begin{smallmatrix}a\end{smallmatrix}",
            r"\begin{smallmatrix}a&b\\c&d\end{smallmatrix}",
            r"x^{\begin{smallmatrix}a&b\\c&d\end{smallmatrix}}",
        ] {
            let original = crate::from_latex(source).unwrap();
            let mathml = original.to_mathml().unwrap();
            assert!(
                mathml.contains("data-mathema-arraystretch=\"50%\""),
                "{mathml}"
            );
            assert!(
                mathml.contains("data-mathema-array-environment=\"smallmatrix\""),
                "{mathml}"
            );
            assert!(mathml.contains("rowspacing=\"0.1em\""), "{mathml}");
            assert!(mathml.contains("columnspacing=\"0.2778em\""), "{mathml}");
            let reimported = crate::from_mathml(&mathml).unwrap();
            assert_eq!(original.to_svg().unwrap(), reimported.to_svg().unwrap());
            assert_eq!(mathml, reimported.to_mathml().unwrap());
            assert!(
                reimported
                    .to_latex()
                    .unwrap()
                    .contains(r"\begin{smallmatrix}")
            );
            assert!(!reimported.to_latex().unwrap().contains(r"\scriptstyle"));
        }

        let authored = crate::from_mathml(
            r#"<math><mtable rowspacing="0em"><mtr><mtd><mi>a</mi></mtd></mtr></mtable></math>"#,
        )
        .unwrap();
        assert!(
            !authored
                .to_mathml()
                .unwrap()
                .contains("data-mathema-arraystretch")
        );

        let contradictory = crate::from_mathml(
            r#"<math><mtable data-mathema-array-environment="smallmatrix" data-mathema-arraystretch="100%"><mtr><mtd><mi>a</mi></mtd></mtr></mtable></math>"#,
        )
        .unwrap();
        assert!(
            !contradictory
                .to_latex()
                .unwrap()
                .contains(r"\begin{smallmatrix}")
        );
    }

    #[test]
    fn mathml_preserves_an_operator_names_form_across_display_modes() {
        for (src, expected) in [
            (r"\operatorname{\overline{x}}", OperatorNameForm::Unstarred),
            (
                r"\operatorname*{\overline{x}}",
                OperatorNameForm::Starred(LimitStyle::Default),
            ),
            (
                r"\operatorname*{\overline{x}}\limits",
                OperatorNameForm::Starred(LimitStyle::AboveBelow),
            ),
            (
                r"\operatorname*{\overline{x}}\nolimits",
                OperatorNameForm::Starred(LimitStyle::SubSup),
            ),
        ] {
            for mode in [DisplayStyle::Inline, DisplayStyle::Block] {
                let cfg = Config::default().with_display(mode);
                let parsed = crate::from_latex_with(&format!("{src}_i^j"), &cfg).unwrap();
                let xml = parsed.to_mathml().unwrap();
                let imported = crate::from_mathml(&xml).unwrap();
                assert!(
                    matches!(&imported.expr().nodes[..], [MathNode::SubSup { base, .. }]
                    if matches!(base.as_ref(), MathNode::StructuredOperator { form, .. } if *form == expected)),
                    "{src}: {xml}"
                );
                assert_eq!(
                    parsed.to_latex().unwrap(),
                    imported.to_latex().unwrap(),
                    "{src}"
                );
            }
        }
        let xml = r#"<math><mrow data-mjx-texclass="OP" data-mathema-structured-operator="starred-default"/></math>"#;
        assert!(matches!(
            &crate::from_mathml(xml).unwrap().expr().nodes[..],
            [MathNode::StructuredOperator {
                form: OperatorNameForm::Starred(LimitStyle::Default),
                ..
            }]
        ));
    }

    #[test]
    fn ragged_mtable_uses_columns_first_occupied_in_later_rows() {
        let xml = r#"<math><mtable><mtr/><mtr><mtd columnalign="left"><mi>a</mi></mtd></mtr><mtr><mtd><mi>b</mi></mtd><mtd columnalign="right"/></mtr><mtr><mtd/><mtd><mi>c</mi></mtd></mtr></mtable></math>"#;
        let result = crate::from_mathml(xml).unwrap();
        let MathNode::Array { columns, rows, .. } = &result.expr().nodes[0] else {
            panic!("expected an authored array");
        };
        assert_eq!(columns, &[ColumnAlign::Left, ColumnAlign::Right]);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.len() == 2));
        assert!(
            matches!(&rows[3][1][..], [MathNode::AlignedCell { content, .. }] if matches!(content.as_ref(), MathNode::Identifier(s) if s == "c"))
        );
        let wide = format!(
            "<math><mtable>{}<mtr>{}</mtr></mtable></math>",
            "<mtr/>".repeat(256),
            "<mtd/>".repeat(256)
        );
        assert!(matches!(
            crate::from_mathml(&wide),
            Err(Error::LimitExceeded {
                kind: crate::error::LimitKind::ListLength,
                ..
            })
        ));
    }

    #[test]
    fn authored_table_row_and_cell_alignment_follow_mathml_precedence() {
        let inherited = r#"<math><mtable columnalign="left"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr></mtable></math>"#;
        let inherited = crate::from_mathml(inherited).unwrap();
        let MathNode::Array { columns, .. } = &inherited.expr().nodes[0] else {
            panic!("expected an inherited table")
        };
        assert_eq!(columns, &[ColumnAlign::Left, ColumnAlign::Left]);

        let table = r#"<math><mtable columnalign="left right"><mtr><mtd><mi>a</mi></mtd><mtd columnalign="center"><mi>b</mi></mtd><mtd><mi>c</mi></mtd></mtr><mtr columnalign="right left"><mtd><mi>d</mi></mtd><mtd><mi>e</mi></mtd><mtd columnalign="left"><mi>f</mi></mtd></mtr></mtable></math>"#;
        let result = crate::from_mathml(table).unwrap();
        let MathNode::Array { columns, rows, .. } = &result.expr().nodes[0] else {
            panic!("expected an authored table")
        };
        // The table list supplies the first row, the row list overrides the
        // second row, and an mtd value wins over both. Each column takes its
        // initial alignment from the first row occupying it.
        assert_eq!(
            columns,
            &[ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right]
        );
        assert_eq!(rows.len(), 2);
        let canonical = result.to_latex().unwrap();
        assert!(
            canonical.contains(r"\begin{array}{@{}lcr@{}}"),
            "{canonical}"
        );
    }

    #[test]
    #[cfg(feature = "render")]
    fn conflicting_authored_cell_alignment_survives_heap_and_compact_routes() {
        let source = r#"<math><mtable columnalign="left left"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr columnalign="right right"><mtd><mi>LONG</mi></mtd><mtd columnalign="center"><mi>c</mi></mtd></mtr></mtable></math>"#;
        let parsed = crate::from_mathml(source).unwrap();
        let compact_xml = parsed.to_mathml().unwrap();
        let reparsed = crate::from_mathml(&compact_xml).unwrap();
        assert_eq!(compact_xml, reparsed.to_mathml().unwrap());
        assert_eq!(parsed.to_svg().unwrap(), reparsed.to_svg().unwrap());
        let latex = parsed.to_latex().unwrap();
        assert!(latex.contains(r"\multicolumn{1}{@{}r"));
        let latex_reparsed = crate::from_latex(&latex).unwrap();
        assert_eq!(parsed.to_svg().unwrap(), latex_reparsed.to_svg().unwrap());
        let MathNode::Array { rows, .. } = &parsed.expr().nodes[0] else {
            panic!("expected table array");
        };
        assert!(
            rows.iter()
                .flatten()
                .any(|cell| matches!(cell.as_slice(), [MathNode::AlignedCell { .. }]))
        );
        let compact = MathmlReader::parse_compact(source, &Config::default()).unwrap();
        assert_eq!(compact.expr.to_math_expr(), parsed.expr());
        let converted =
            crate::convert(source, crate::Format::MathML, crate::Format::MathML).unwrap();
        assert!(
            converted.contains(r#"<mtd columnalign="right"><mi mathvariant="normal">LONG</mi>"#)
        );
    }

    #[test]
    fn aligned_text_uses_the_normal_text_escapes() {
        let xml = r#"<math><mtable><mtr><mi>x</mi></mtr><mtr><mtd columnalign="right"><mtext>A#--B\c</mtext></mtd></mtr></mtable></math>"#;
        let parsed = crate::from_mathml(xml).unwrap();
        let latex = parsed.to_latex().unwrap();
        assert!(
            latex.contains(r"\text{A\#-{}-B\textbackslash{}c}"),
            "{latex}"
        );
        let recovered = crate::from_latex(&latex).unwrap().to_mathml().unwrap();
        assert!(recovered.contains(r"<mtext>A#--B\c</mtext>"), "{recovered}");
    }

    #[test]
    fn empty_table_retains_its_type_in_both_xml_forms() {
        let short = crate::from_mathml("<math><mtable/></math>").unwrap();
        let long = crate::from_mathml("<math><mtable></mtable></math>").unwrap();
        assert!(
            matches!(&short.expr().nodes[..], [MathNode::Array { rows, .. }] if rows.is_empty())
        );
        assert_eq!(short.expr(), long.expr());
        assert_eq!(short.to_mathml().unwrap(), long.to_mathml().unwrap());
    }

    #[test]
    fn alignment_lists_do_not_create_columns_or_hide_later_overrides() {
        for (xml, expected) in [
            (
                r#"<mtable columnalign="left right center"><mtr><mtd/></mtr></mtable>"#,
                vec![ColumnAlign::Left],
            ),
            (
                r#"<mtable columnalign="right left"><mtr/><mtr columnalign="left"><mtd/></mtr><mtr><mtd/><mtd columnalign="right"/></mtr></mtable>"#,
                vec![ColumnAlign::Left, ColumnAlign::Right],
            ),
            (
                r#"<mtable columnalign="left right"><mtr/></mtable>"#,
                vec![],
            ),
            (
                r#"<mtable columnalign="right"><mtr columnalign="left right"><mtd/><mi>b</mi><mtd columnalign="  ">c</mtd></mtr></mtable>"#,
                vec![ColumnAlign::Left, ColumnAlign::Right, ColumnAlign::Right],
            ),
        ] {
            let parsed = crate::from_mathml(&format!("<math>{xml}</math>")).unwrap();
            let MathNode::Array { columns, .. } = &parsed.expr().nodes[0] else {
                panic!("missing authored array for {xml}");
            };
            assert_eq!(columns, &expected, "{xml}");
        }
    }

    #[test]
    fn heap_arraystretch_uses_the_same_mathml_marker_without_environment_provenance() {
        let heap = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Center],
            rows: vec![vec![vec![MathNode::Identifier("a".into())]]],
            arraystretch: Some(0.5),
            add_jot: false,
        }]);
        let mathml = crate::writer::mathml::MathmlWriter::write(&heap, &Config::default()).unwrap();
        assert!(mathml.contains("data-mathema-arraystretch=\"50%\""));
        assert!(!mathml.contains("data-mathema-array-environment"));
        assert_eq!(
            mathml,
            crate::from_mathml(&mathml).unwrap().to_mathml().unwrap()
        );
    }

    #[test]
    fn writer_fence_roles_roundtrip_bars_mismatches_and_null_endpoints() {
        use crate::writer::Writer;

        for source in [
            r"\left\backslash x \right/",
            r"\left. x \right.",
            r"\left|x\right|",
            r"\left[x\right)",
            r"\left(x\right]",
            r"\left[ a(b) \right)",
            r"\left. f\right|",
            r"\left|f\right.",
            r"\left(x|y\right)",
            r"\left(x\middle. y\right)",
        ] {
            let first = crate::from_latex(source).unwrap();
            let mathml = first.to_mathml().unwrap();
            let heap =
                crate::writer::mathml::MathmlWriter::write(&first.expr(), &Config::default())
                    .unwrap();
            assert_eq!(heap, mathml, "heap/compact mismatch for {source}");
            let requires_endpoint_roles = source.contains(r"\left|")
                || source.contains(r"\right.")
                || source == r"\left[x\right)"
                || source == r"\left(x\right]";
            if requires_endpoint_roles {
                assert!(
                    mathml.contains("form=\"prefix\"") && mathml.contains("form=\"postfix\""),
                    "endpoint roles missing for {source}: {mathml}"
                );
            }
            let reparsed = crate::from_mathml(&mathml).unwrap();
            assert_eq!(reparsed.to_latex().unwrap(), first.to_latex().unwrap());
            assert_eq!(reparsed.to_mathml().unwrap(), mathml);
            assert!(matches!(
                reparsed.expr().nodes.as_slice(),
                [MathNode::Fenced { .. }]
            ));
        }
    }

    #[test]
    fn non_fence_attributes_preserve_divides_and_parallel_relations() {
        for glyph in ["∣", "∥"] {
            for attrs in [
                "",
                r#" fence="false""#,
                r#" fence="false" stretchy="false""#,
            ] {
                let xml = format!("<math><mo{attrs}>{glyph}</mo></math>");
                assert!(
                    matches!(parse_one(&xml), MathNode::Symbol(SymbolKind::Relation, _)),
                    "{xml}"
                );
            }
        }
        assert!(matches!(
            parse_one(r#"<math><mo fence="false" stretchy="false">|</mo></math>"#),
            MathNode::Symbol(SymbolKind::Ordinary, _)
        ));
    }

    #[test]
    fn explicit_bra_with_literal_bar_reimports_as_one_fence() {
        for source in [
            r"\bra{\phi|\psi}",
            r"\bra{\phi\|\psi}",
            r"\bra{a||b}",
            r"\left\langle\phi|x\right|",
        ] {
            let first = crate::from_latex(source).unwrap();
            let mathml = first.to_mathml().unwrap();
            let reparsed = crate::from_mathml(&mathml).unwrap();
            assert_eq!(reparsed.to_mathml().unwrap(), mathml, "{source}: {mathml}");
            assert!(matches!(
                reparsed.expr().nodes.as_slice(),
                [MathNode::Fenced { .. }]
            ));
        }

        let authored_fixed = r#"<math><mrow><mo fence="true" form="prefix">⟨</mo><mi>a</mi><mo stretchy="false">|</mo><mo stretchy="false">|</mo><mi>b</mi><mo fence="true" form="postfix" stretchy="true">|</mo></mrow></math>"#;
        assert!(matches!(parse_one(authored_fixed), MathNode::Fenced { .. }));
        for value in ["true", "unknown"] {
            let ordinary =
                authored_fixed.replace(r#"stretchy="false""#, &format!(r#"stretchy="{value}""#));
            assert!(
                matches!(parse_one(&ordinary), MathNode::Row(_)),
                "{ordinary}"
            );
        }

        let constrained_endpoint = r#"<math><mrow><mo fence="true" form="prefix" stretchy="false">⟨</mo><mi>a</mi><mo stretchy="false">|</mo><mo fence="true" form="postfix" stretchy="false">|</mo></mrow></math>"#;
        assert!(matches!(parse_one(constrained_endpoint), MathNode::Row(_)));
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
