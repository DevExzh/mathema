//! OMML (Office Math Markup Language) reader.
//!
//! Parses the math markup of ECMA-376 Part 1, §22.1 (`m:oMathPara` /
//! `m:oMath` and the math objects they contain) into the shared AST. Element
//! and property names, defaults and child order follow the schema in
//! ECMA-376 Part 1, Annex A (`shared-math.xsd`).
//!
//! In [`Strictness::Strict`] mode the reader refuses malformed XML, unknown
//! math elements, invalid property values and duplicated arguments. In
//! [`Strictness::Lenient`] mode it skips what it cannot use and reports a
//! [`Diagnostic`] for each recovery. Properties that are valid OMML but have
//! no home in the AST (for example `m:grow` or `m:eqArrPr` spacing) are
//! dropped in both modes with an [`DiagnosticKind::Info`] diagnostic.

use std::borrow::Cow;
use std::collections::HashSet;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::*;
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader as MathReader;

/// Transitional and Strict (ISO/IEC 29500) namespace URIs of the math markup.
const MATH_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "http://purl.oclc.org/ooxml/officeDocument/math",
];
/// Namespace URIs of WordprocessingML, the host markup of Word documents.
const WML_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];
/// Namespace URIs of DrawingML, the host markup of PowerPoint text.
const DML_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/drawingml/2006/main",
    "http://purl.oclc.org/ooxml/drawingml/main",
];

/// Maximum element nesting accepted by the reader. Every math object uses two
/// XML levels (the object and its argument), so this admits roughly
/// [`MAX_AST_DEPTH`] nested objects while bounding recursion.
const MAX_XML_DEPTH: u32 = 2 * MAX_AST_DEPTH as u32 + 16;

/// Padding cells always allowed, however small the input; above this the
/// budget is a quarter of the input length, so padding cannot outgrow the
/// cells actually read by more than a constant factor.
const MIN_PADDING_BUDGET: usize = 65_536;

/// U+2061 FUNCTION APPLICATION. `m:func` is represented as
/// `Row[name, U+2061, argument]`, the same invisible operator MathML uses.
pub(crate) const FUNCTION_APPLICATION: &str = "\u{2061}";

/// Horizontal justification of a display-math paragraph (`m:jc`, the
/// `ST_Jc` simple type of ECMA-376 Part 1, §22.1.3.4).
///
/// ```
/// use mathema::OmmlJustification;
/// assert_eq!(OmmlJustification::from_omml("centerGroup"), Some(OmmlJustification::CenterGroup));
/// assert_eq!(OmmlJustification::Left.as_omml(), "left");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OmmlJustification {
    /// Every equation is left-aligned.
    Left,
    /// Every equation is right-aligned.
    Right,
    /// Every equation is centred on its own.
    Center,
    /// The equations are left-aligned to each other and centred as a group
    /// (the default when `m:jc` is absent).
    CenterGroup,
}

impl OmmlJustification {
    /// Parse an `ST_Jc` attribute value.
    #[must_use]
    pub fn from_omml(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "center" => Some(Self::Center),
            "centerGroup" => Some(Self::CenterGroup),
            _ => None,
        }
    }

    /// The `ST_Jc` attribute value for this justification.
    #[must_use]
    pub fn as_omml(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
            Self::CenterGroup => "centerGroup",
        }
    }
}

/// A display-math paragraph (`m:oMathPara`) read equation by equation.
///
/// A math paragraph may hold several `m:oMath` equations that share one
/// justification. [`OmmlReader::parse`] can only return one expression and
/// stacks such equations into a single-column array; this type keeps each
/// equation separate so a document can be read, edited and written back with
/// [`OmmlWriter::write_paragraph`](crate::writer::omml::OmmlWriter::write_paragraph).
///
/// ```
/// use mathema::{Config, OmmlJustification};
/// use mathema::reader::omml::OmmlReader;
///
/// let omml = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
///   <m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr>
///   <m:oMath><m:r><m:t>a</m:t></m:r></m:oMath>
///   <m:oMath><m:r><m:t>b</m:t></m:r></m:oMath>
/// </m:oMathPara>"#;
/// let para = OmmlReader::parse_paragraph(omml, &Config::default()).unwrap();
/// assert_eq!(para.equations.len(), 2);
/// assert_eq!(para.justification, Some(OmmlJustification::Left));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OmmlParagraph {
    /// The paragraph justification from `m:oMathParaPr/m:jc`, if present.
    pub justification: Option<OmmlJustification>,
    /// The equations of the paragraph, in document order.
    pub equations: Vec<CompactExpr>,
    /// Non-fatal diagnostics collected while reading.
    pub diagnostics: Vec<Diagnostic>,
}

impl OmmlParagraph {
    /// Create a paragraph holding `equations` with no justification.
    #[must_use]
    pub fn new(equations: Vec<CompactExpr>) -> Self {
        Self {
            justification: None,
            equations,
            diagnostics: Vec::new(),
        }
    }

    /// Builder: set the paragraph justification.
    #[must_use]
    pub fn with_justification(mut self, justification: Option<OmmlJustification>) -> Self {
        self.justification = justification;
        self
    }
}

/// Reader for OMML input.
pub struct OmmlReader;

impl MathReader for OmmlReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        config.check_input_size(input)?;
        let mut parser = Parser::new(strip_bom(input), config, false);
        match parser.parse_document() {
            Ok(doc) => {
                let display = if doc.paragraph {
                    DisplayStyle::Block
                } else {
                    config.display
                };
                let roots = parser.single_expression(doc)?;
                let Parser { arena, diags, .. } = parser;
                arena.check_limits()?;
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(arena, roots, display),
                    diags,
                ))
            },
            // Resource limits apply in every mode.
            Err(e) if config.strictness == Strictness::Lenient && !is_limit(&e) => {
                let mut diags = parser.diags;
                diags.push(warning(format!("OMML parse error (recovered): {e}")));
                Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, config.display),
                    diags,
                ))
            },
            Err(e) => Err(e),
        }
    }
}

impl OmmlReader {
    /// Parse OMML into a paragraph of separate equations, keeping the
    /// `m:oMathPara` justification. A bare `m:oMath` (or an OMML fragment)
    /// yields a paragraph with one equation and no justification.
    ///
    /// # Errors
    ///
    /// Same as [`OmmlReader::parse`]: in Strict mode malformed XML, unknown
    /// math elements and invalid values are errors. In Lenient mode a fatal
    /// XML error yields an empty paragraph with a warning.
    pub fn parse_paragraph(input: &str, config: &Config) -> Result<OmmlParagraph> {
        config.check_input_size(input)?;
        let mut parser = Parser::new(strip_bom(input), config, true);
        match parser.parse_document() {
            Ok(doc) => {
                let display = if doc.paragraph {
                    DisplayStyle::Block
                } else {
                    config.display
                };
                let mut equations = Vec::with_capacity(doc.equations.len());
                for eq in doc.equations {
                    let mut arena = eq.arena.unwrap_or_default();
                    let roots = checked_span(&mut arena, &eq.ids)?;
                    arena.check_limits()?;
                    equations.push(CompactExpr::new(arena, roots, display));
                }
                Ok(OmmlParagraph {
                    justification: doc.justification,
                    equations,
                    diagnostics: parser.diags,
                })
            },
            // Resource limits apply in every mode.
            Err(e) if config.strictness == Strictness::Lenient && !is_limit(&e) => {
                let mut diagnostics = parser.diags;
                diagnostics.push(warning(format!("OMML parse error (recovered): {e}")));
                Ok(OmmlParagraph {
                    justification: None,
                    equations: Vec::new(),
                    diagnostics,
                })
            },
            Err(e) => Err(e),
        }
    }
}

// ── Parser state ────────────────────────────────────────────────────────

/// Namespace class of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ns {
    Math,
    Wml,
    Dml,
    Other,
}

/// A start tag together with its namespace class.
struct Elem<'i> {
    ns: Ns,
    start: BytesStart<'i>,
}

impl Elem<'_> {
    fn local(&self) -> &str {
        self.start.local_name().into_inner()
    }

    fn qname(&self) -> &str {
        self.start.name().into_inner()
    }

    fn is_math(&self, local: &str) -> bool {
        self.ns == Ns::Math && self.local() == local
    }
}

/// One step of the XML event stream, after comments and processing
/// instructions are dropped and entity references are resolved.
enum Item<'i> {
    Start(Elem<'i>),
    End,
    Text(Cow<'i, str>),
    Eof,
}

/// One equation being collected. In paragraph mode every equation owns its
/// arena; otherwise all equations share the parser's arena.
struct Equation {
    arena: Option<AstArena>,
    ids: SmallVec<[NodeId; 8]>,
    fragment: bool,
}

#[derive(Default)]
struct Document {
    equations: Vec<Equation>,
    justification: Option<OmmlJustification>,
    paragraph: bool,
}

/// Math run properties (`m:rPr`, `CT_RPR`).
#[derive(Default)]
struct MathRunProps {
    lit: bool,
    nor: bool,
    scr: Option<&'static str>,
    sty: Option<&'static str>,
}

impl MathRunProps {
    /// The explicit style of the run, or `None` when neither `m:sty` nor a
    /// non-roman `m:scr` is given (the math-italic default then applies).
    fn style(&self) -> Option<TextStyle> {
        let scr = self.scr.filter(|&s| s != "roman");
        if self.sty.is_none() && scr.is_none() {
            return None;
        }
        Some(combine_omml_style(
            self.sty.unwrap_or("p"),
            scr.unwrap_or("roman"),
        ))
    }
}

/// Host run properties (`w:rPr` or `a:rPr`) that the AST can hold.
#[derive(Default)]
struct HostRunProps {
    bold: bool,
    italic: bool,
    color: Option<[u8; 3]>,
}

impl HostRunProps {
    fn text_style(&self) -> TextStyle {
        match (self.bold, self.italic) {
            (true, true) => TextStyle::BoldItalic,
            (true, false) => TextStyle::Bold,
            (false, true) => TextStyle::Italic,
            (false, false) => TextStyle::Normal,
        }
    }
}

type Ids = SmallVec<[NodeId; 8]>;

struct Parser<'i> {
    reader: NsReader<&'i [u8]>,
    arena: AstArena,
    strict: bool,
    split_arenas: bool,
    depth: u32,
    /// Remaining number of padding cells that short matrix rows may add,
    /// summed over the whole input (see `build_array`).
    padding_budget: usize,
    diags: Vec<Diagnostic>,
    seen: HashSet<String>,
}

fn warning(message: String) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Warning,
        message,
        span: None,
    }
}

fn omml_err(message: impl Into<String>) -> Error {
    Error::OmmlParse {
        message: message.into(),
    }
}

/// Whether `e` reports a resource limit, which Lenient mode must not hide.
fn is_limit(e: &Error) -> bool {
    matches!(e, Error::LimitExceeded { .. } | Error::InputTooLarge { .. })
}

/// Push `ids` as a child span, refusing spans the arena cannot index.
fn checked_span(arena: &mut AstArena, ids: &[NodeId]) -> Result<NodeSpan> {
    arena.push_children(ids)
}

fn is_xml_whitespace(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// XML 1.0 `Char` production (XML 1.0 §2.2).
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}')
        || c >= '\u{10000}'
}

fn classify_ns(res: &ResolveResult<'_>, prefix: Option<&str>) -> Ns {
    let by_uri = |uri: &str| {
        if MATH_NAMESPACES.contains(&uri) {
            Ns::Math
        } else if WML_NAMESPACES.contains(&uri) {
            Ns::Wml
        } else if DML_NAMESPACES.contains(&uri) {
            Ns::Dml
        } else {
            Ns::Other
        }
    };
    match res {
        ResolveResult::Bound(ns) => by_uri(ns.into_inner()),
        // Fragments copied out of a document often lose their namespace
        // declarations; fall back to the conventional prefixes.
        ResolveResult::Unknown(_) => match prefix {
            Some("m") => Ns::Math,
            Some("w") => Ns::Wml,
            Some("a") => Ns::Dml,
            _ => Ns::Other,
        },
        ResolveResult::Unbound => Ns::Math,
    }
}

impl<'i> Parser<'i> {
    fn new(input: &'i str, config: &Config, split_arenas: bool) -> Self {
        let mut reader = NsReader::from_str(input);
        let cfg = reader.config_mut();
        cfg.trim_text(false);
        cfg.expand_empty_elements = true;
        cfg.check_end_names = true;
        cfg.check_comments = true;
        Self {
            reader,
            arena: AstArena::new(),
            strict: config.strictness == Strictness::Strict,
            split_arenas,
            depth: 0,
            padding_budget: (input.len() / 4).max(MIN_PADDING_BUDGET),
            diags: Vec::new(),
            seen: HashSet::new(),
        }
    }

    // ── Diagnostics ─────────────────────────────────────────────────────

    fn note(&mut self, kind: DiagnosticKind, message: String) {
        if self.seen.insert(message.clone()) {
            self.diags.push(Diagnostic {
                kind,
                message,
                span: None,
            });
        }
    }

    fn info(&mut self, message: String) {
        self.note(DiagnosticKind::Info, message);
    }

    /// Strict: fail with `message`. Lenient: record it as a warning.
    fn recover(&mut self, message: String) -> Result<()> {
        if self.strict {
            Err(omml_err(message))
        } else {
            self.note(DiagnosticKind::Warning, format!("{message} (recovered)"));
            Ok(())
        }
    }

    // ── Event stream ────────────────────────────────────────────────────

    fn xml_err(&self, e: impl std::fmt::Display) -> Error {
        omml_err(format!(
            "malformed XML near byte {}: {e}",
            self.reader.buffer_position()
        ))
    }

    fn next_item(&mut self) -> Result<Item<'i>> {
        loop {
            let read = self.reader.read_resolved_event();
            let (ns, event) = match read {
                Ok((res, event)) => {
                    let ns = match &event {
                        Event::Start(s) => {
                            classify_ns(&res, s.name().prefix().map(|p| p.into_inner()))
                        },
                        _ => Ns::Other,
                    };
                    (ns, event)
                },
                Err(e) => return Err(self.xml_err(e)),
            };
            match event {
                Event::Start(start) => {
                    self.depth += 1;
                    if self.depth > MAX_XML_DEPTH {
                        return Err(omml_err(format!(
                            "maximum nesting depth ({MAX_XML_DEPTH}) exceeded"
                        )));
                    }
                    // Validate every attribute (syntax, duplicates, entity
                    // references) so malformed tags are not silently accepted.
                    let mut bad_char = false;
                    for attr in start.attributes() {
                        let attr = attr.map_err(|e| self.xml_err(e))?;
                        let value = attr
                            .normalized_value(XmlVersion::Implicit1_0)
                            .map_err(|e| self.xml_err(e))?;
                        bad_char |= !value.chars().all(is_xml_char);
                    }
                    if bad_char {
                        self.recover(format!(
                            "an attribute of <{}> holds a character that XML does not allow",
                            start.name().into_inner()
                        ))?;
                    }
                    return Ok(Item::Start(Elem { ns, start }));
                },
                Event::End(_) => {
                    self.depth = self.depth.saturating_sub(1);
                    return Ok(Item::End);
                },
                Event::Empty(_) => {
                    // `expand_empty_elements` reports `<a/>` as start + end.
                    return Err(omml_err("internal error: unexpanded empty element"));
                },
                Event::Text(t) => return self.checked_text(t.xml10_content()),
                Event::CData(c) => return self.checked_text(c.xml10_content()),
                Event::GeneralRef(r) => {
                    let name = r.into_inner();
                    let text = self.resolve_reference(&name)?;
                    return Ok(Item::Text(Cow::Owned(text)));
                },
                Event::Comment(_) | Event::PI(_) | Event::Decl(_) => continue,
                Event::DocType(_) => {
                    self.recover("a document type declaration is not allowed in OMML".into())?;
                    continue;
                },
                Event::Eof => {
                    if self.depth != 0 {
                        return Err(omml_err("unexpected end of input inside an element"));
                    }
                    return Ok(Item::Eof);
                },
            }
        }
    }

    /// Refuse characters outside the XML 1.0 `Char` production (such as
    /// NUL); Lenient mode drops them.
    fn checked_text(&mut self, text: Cow<'i, str>) -> Result<Item<'i>> {
        if text.chars().all(is_xml_char) {
            return Ok(Item::Text(text));
        }
        self.recover("text holds a character that XML does not allow; it was dropped".into())?;
        Ok(Item::Text(Cow::Owned(
            text.chars().filter(|&c| is_xml_char(c)).collect(),
        )))
    }

    /// Resolve `&name;`: the five predefined XML entities and character
    /// references (XML 1.0 §4.1, §4.6). Other names are undeclared entities.
    fn resolve_reference(&mut self, name: &str) -> Result<String> {
        if let Some(num) = name.strip_prefix('#') {
            let cp = match num.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            return match cp.and_then(char::from_u32).filter(|&c| is_xml_char(c)) {
                Some(c) => Ok(c.to_string()),
                None => Err(omml_err(format!(
                    "character reference &{name}; is not a valid XML character"
                ))),
            };
        }
        let predefined = match name {
            "lt" => Some("<"),
            "gt" => Some(">"),
            "amp" => Some("&"),
            "apos" => Some("'"),
            "quot" => Some("\""),
            _ => None,
        };
        if let Some(s) = predefined {
            return Ok(s.to_owned());
        }
        if !self.strict
            && let Some(s) = crate::unicode::resolve_entity(name)
        {
            self.note(
                DiagnosticKind::Warning,
                format!("undeclared entity &{name}; resolved as a MathML entity (recovered)"),
            );
            return Ok(s.into_owned());
        }
        Err(omml_err(format!("undeclared entity reference &{name};")))
    }

    /// The next child element of the element named `parent`, or `None` at
    /// its end tag. Text between elements must be whitespace.
    fn next_child(&mut self, parent: &str) -> Result<Option<Elem<'i>>> {
        loop {
            match self.next_item()? {
                Item::Start(e) => return Ok(Some(e)),
                Item::End => return Ok(None),
                Item::Text(t) => {
                    if !is_xml_whitespace(&t) {
                        self.recover(format!("unexpected text {:?} in <{parent}>", truncate(&t)))?;
                    }
                },
                Item::Eof => {
                    return Err(omml_err(format!("unexpected end of input in <{parent}>")));
                },
            }
        }
    }

    /// Consume the rest of an element that was just opened.
    fn skip(&mut self) -> Result<()> {
        let mut depth = 1u32;
        while depth > 0 {
            match self.next_item()? {
                Item::Start(_) => depth += 1,
                Item::End => depth -= 1,
                Item::Text(_) => {},
                Item::Eof => return Err(omml_err("unexpected end of input")),
            }
        }
        Ok(())
    }

    /// Handle an element that is not allowed where it occurs.
    fn unknown(&mut self, el: &Elem<'_>, parent: &str) -> Result<()> {
        let message = format!("unsupported element <{}> in <{parent}>", el.qname());
        if self.strict {
            return Err(omml_err(message));
        }
        self.skip()?;
        self.note(DiagnosticKind::Warning, format!("skipped {message}"));
        Ok(())
    }

    /// Skip a valid property that the AST cannot hold, noting the loss.
    fn drop_property(&mut self, el: &Elem<'_>, parent: &str) -> Result<()> {
        let message = format!(
            "<{}> in <{parent}> has no equivalent in the AST and was dropped",
            el.qname()
        );
        self.skip()?;
        self.info(message);
        Ok(())
    }

    // ── Attribute and property helpers ──────────────────────────────────

    /// The value of the attribute whose local name is `local` (any prefix).
    fn attr(&self, el: &Elem<'_>, local: &str) -> Result<Option<String>> {
        for attr in el.start.attributes() {
            let attr = attr.map_err(|e| self.xml_err(e))?;
            let key = attr.key;
            if key.local_name().into_inner() == local
                && key.prefix().is_none_or(|p| p.into_inner() != "xmlns")
            {
                let value = attr
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map_err(|e| self.xml_err(e))?;
                return Ok(Some(value.into_owned()));
            }
        }
        Ok(None)
    }

    /// Read a property element that carries only `val` and no content.
    fn leaf(&mut self, el: &Elem<'_>) -> Result<Option<String>> {
        let val = self.attr(el, "val")?;
        let name = el.qname().to_owned();
        while let Some(child) = self.next_child(&name)? {
            self.unknown(&child, &name)?;
        }
        Ok(val)
    }

    /// Report an invalid attribute value; Lenient mode uses `fallback`.
    fn bad_value<T>(&mut self, el: &Elem<'_>, value: &str, fallback: T) -> Result<T> {
        self.recover(format!("invalid value {value:?} for <{}>", el.qname()))?;
        Ok(fallback)
    }

    /// `ST_OnOff`: absent `val` means on.
    fn on_off(&mut self, el: &Elem<'_>) -> Result<bool> {
        match self.leaf(el)?.as_deref() {
            None | Some("1" | "on" | "true") => Ok(true),
            Some("0" | "off" | "false") => Ok(false),
            Some(other) => self.bad_value(el, other, true),
        }
    }

    /// `CT_Char`: at most one character; the empty string means "none".
    fn char_val(&mut self, el: &Elem<'_>) -> Result<Option<String>> {
        let Some(val) = self.leaf(el)? else {
            return self
                .recover(format!("<{}> requires a val attribute", el.qname()))
                .map(|()| None);
        };
        let mut chars = val.chars();
        match (chars.next(), chars.next()) {
            (_, None) => Ok(Some(val)),
            (Some(first), Some(_)) => self.bad_value(el, &val, Some(first.to_string())),
            (None, Some(_)) => unreachable!("a second char implies a first"),
        }
    }

    /// Parse a property element (`m:fPr`, `m:naryPr`, ...). `handle` returns
    /// `true` for the children it consumed; `m:ctrlPr` (formatting of the
    /// control character) is skipped, names in `dropped` are valid but
    /// unrepresentable, and anything else is unknown.
    fn props(
        &mut self,
        name: &'static str,
        dropped: &[&str],
        mut handle: impl FnMut(&mut Self, &Elem<'i>) -> Result<bool>,
    ) -> Result<()> {
        while let Some(child) = self.next_child(name)? {
            if child.ns == Ns::Math {
                if handle(self, &child)? {
                    continue;
                }
                if child.local() == "ctrlPr" {
                    self.skip()?;
                    continue;
                }
                if dropped.contains(&child.local()) {
                    self.drop_property(&child, name)?;
                    continue;
                }
            }
            self.unknown(&child, name)?;
        }
        Ok(())
    }

    /// Store `id` into an argument slot, refusing duplicates.
    fn set_once(
        &mut self,
        slot: &mut Option<NodeId>,
        id: NodeId,
        child: &str,
        parent: &str,
    ) -> Result<()> {
        if slot.is_some() {
            return self.recover(format!(
                "duplicate <m:{child}> in <{parent}>; the later one was ignored"
            ));
        }
        *slot = Some(id);
        Ok(())
    }

    fn once_props(&mut self, seen: &mut bool, child: &str, parent: &str) -> Result<bool> {
        if *seen {
            self.recover(format!("duplicate <m:{child}> in <{parent}>"))?;
            self.skip()?;
            return Ok(false);
        }
        *seen = true;
        Ok(true)
    }

    // ── Arena helpers ───────────────────────────────────────────────────

    fn empty_row(&mut self) -> NodeId {
        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
    }

    fn group(&mut self, ids: &[NodeId]) -> Result<NodeId> {
        if ids.len() == 1 {
            Ok(ids[0])
        } else {
            let span = checked_span(&mut self.arena, ids)?;
            Ok(self.arena.push_node(CompactNode::Row(span)))
        }
    }

    fn or_empty(&mut self, id: Option<NodeId>) -> NodeId {
        match id {
            Some(id) => id,
            None => self.empty_row(),
        }
    }

    fn is_empty_row(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    fn symbol(&mut self, kind: SymbolKind, s: &str) -> NodeId {
        let sid = self.arena.push_str(s);
        self.arena.push_node(CompactNode::Symbol(kind, sid))
    }

    // ── Document level ──────────────────────────────────────────────────

    fn begin_equation(&mut self, fragment: bool, doc: &mut Document) {
        let arena = if self.split_arenas {
            Some(std::mem::take(&mut self.arena))
        } else {
            None
        };
        // The previous arena (if any) is stashed in the new equation slot and
        // swapped back in `end_equation`.
        doc.equations.push(Equation {
            arena,
            ids: SmallVec::new(),
            fragment,
        });
    }

    fn end_equation(&mut self, doc: &mut Document) {
        if self.split_arenas
            && let Some(eq) = doc.equations.last_mut()
            && let Some(outer) = eq.arena.take()
        {
            eq.arena = Some(std::mem::replace(&mut self.arena, outer));
        }
    }

    fn parse_document(&mut self) -> Result<Document> {
        let mut doc = Document::default();
        let mut open_fragment = false;
        loop {
            match self.next_item()? {
                Item::Eof => break,
                Item::Text(t) => {
                    if !is_xml_whitespace(&t) {
                        self.recover(format!(
                            "unexpected text {:?} outside an equation",
                            truncate(&t)
                        ))?;
                    }
                },
                Item::End => return Err(omml_err("unexpected end tag at top level")),
                Item::Start(el) => self.top_element(el, &mut doc, &mut open_fragment)?,
            }
        }
        if open_fragment {
            self.end_equation(&mut doc);
        }
        Ok(doc)
    }

    fn top_element(
        &mut self,
        el: Elem<'i>,
        doc: &mut Document,
        open_fragment: &mut bool,
    ) -> Result<()> {
        let is_container = el.is_math("oMathPara") || el.is_math("oMath");
        if *open_fragment && (is_container || el.ns != Ns::Math) {
            self.end_equation(doc);
            *open_fragment = false;
        }
        if el.is_math("oMathPara") {
            if doc.paragraph {
                self.recover("more than one <m:oMathPara> in the input".into())?;
            }
            doc.paragraph = true;
            return self.math_paragraph(doc);
        }
        if el.is_math("oMath") {
            self.begin_equation(false, doc);
            let mut ids = Ids::new();
            self.math_content("m:oMath", &mut ids)?;
            if let Some(eq) = doc.equations.last_mut() {
                eq.ids = ids;
            }
            self.end_equation(doc);
            return Ok(());
        }
        if el.is_math("mathPr") {
            // Document-wide math settings (`m:mathPr`, §22.1.2.62).
            return self.drop_property(&el, "the input");
        }
        if el.ns == Ns::Math {
            // A bare math object: read it as part of an equation fragment.
            if !*open_fragment {
                self.begin_equation(true, doc);
                *open_fragment = true;
            }
            let mut ids = Ids::new();
            self.math_element(el, "the input", &mut ids)?;
            if let Some(eq) = doc.equations.last_mut() {
                debug_assert!(eq.fragment);
                eq.ids.extend(ids);
            }
            return Ok(());
        }
        let message = format!(
            "unsupported root element <{}>; expected <m:oMathPara> or <m:oMath>",
            el.qname()
        );
        if self.strict {
            return Err(omml_err(message));
        }
        // Lenient: look through foreign wrappers (for example `a14:m`).
        self.note(
            DiagnosticKind::Warning,
            format!("{message} (descended into it)"),
        );
        let name = el.qname().to_owned();
        while let Some(child) = self.next_child(&name)? {
            self.top_element(child, doc, open_fragment)?;
        }
        if *open_fragment {
            self.end_equation(doc);
            *open_fragment = false;
        }
        Ok(())
    }

    fn math_paragraph(&mut self, doc: &mut Document) -> Result<()> {
        let mut seen_pr = false;
        while let Some(child) = self.next_child("m:oMathPara")? {
            if child.is_math("oMathParaPr") {
                if !self.once_props(&mut seen_pr, "oMathParaPr", "m:oMathPara")? {
                    continue;
                }
                let mut jc = None;
                self.props("m:oMathParaPr", &[], |p, c| {
                    if c.local() != "jc" {
                        return Ok(false);
                    }
                    jc = match p.leaf(c)? {
                        None => None,
                        Some(v) => match OmmlJustification::from_omml(&v) {
                            Some(j) => Some(j),
                            None => p.bad_value(c, &v, None)?,
                        },
                    };
                    Ok(true)
                })?;
                doc.justification = jc;
            } else if child.is_math("oMath") {
                self.begin_equation(false, doc);
                let mut ids = Ids::new();
                self.math_content("m:oMath", &mut ids)?;
                if let Some(eq) = doc.equations.last_mut() {
                    eq.ids = ids;
                }
                self.end_equation(doc);
            } else {
                self.unknown(&child, "m:oMathPara")?;
            }
        }
        Ok(())
    }

    /// Combine the equations of a non-paragraph parse into one root span.
    fn single_expression(&mut self, doc: Document) -> Result<NodeSpan> {
        if let Some(jc) = doc.justification
            && jc != OmmlJustification::CenterGroup
        {
            self.info(format!(
                "math paragraph justification {:?} is not represented in a single expression; \
                 use OmmlReader::parse_paragraph to keep it",
                jc.as_omml()
            ));
        }
        match doc.equations.len() {
            0 => Ok(NodeSpan::EMPTY),
            1 => {
                let ids = &doc.equations[0].ids;
                checked_span(&mut self.arena, ids)
            },
            n => {
                self.info(format!(
                    "{n} equations of a math paragraph were stacked into one array; \
                     use OmmlReader::parse_paragraph to keep them separate"
                ));
                let mut rows = Vec::with_capacity(n);
                for eq in &doc.equations {
                    let span = checked_span(&mut self.arena, &eq.ids)?;
                    rows.push(smallvec::smallvec![
                        self.arena.push_node(CompactNode::Row(span))
                    ]);
                }
                let array = self.build_array(rows, Vec::new(), true)?;
                checked_span(&mut self.arena, &[array])
            },
        }
    }

    // ── Math content ────────────────────────────────────────────────────

    /// Parse the math elements (`EG_OMathElements`) of `parent` up to its
    /// end tag.
    fn math_content(&mut self, parent: &'static str, out: &mut Ids) -> Result<()> {
        while let Some(child) = self.next_child(parent)? {
            self.math_element(child, parent, out)?;
        }
        Ok(())
    }

    /// Parse one element of `EG_OMathElements` (the math objects and the
    /// WordprocessingML paragraph content allowed among them).
    fn math_element(&mut self, el: Elem<'i>, parent: &'static str, out: &mut Ids) -> Result<()> {
        match el.ns {
            Ns::Math => {
                let id = match el.local() {
                    "r" => return self.run(out),
                    "acc" => self.accent()?,
                    "bar" => self.bar()?,
                    "box" => self.boxed()?,
                    "borderBox" => self.border_box()?,
                    "d" => self.delimiter()?,
                    "eqArr" => self.eq_array()?,
                    "f" => self.fraction()?,
                    "func" => self.func()?,
                    "groupChr" => self.group_chr()?,
                    "limLow" => self.limit(false)?,
                    "limUpp" => self.limit(true)?,
                    "m" => self.matrix()?,
                    "nary" => self.nary()?,
                    "phant" => self.phantom()?,
                    "rad" => self.radical()?,
                    "sPre" => self.pre_scripts()?,
                    "sSub" => self.scripts("m:sSub", "sSubPr", false, true)?,
                    "sSup" => self.scripts("m:sSup", "sSupPr", true, false)?,
                    "sSubSup" => self.scripts("m:sSubSup", "sSubSupPr", true, true)?,
                    _ => return self.unknown(&el, parent),
                };
                out.push(id);
                Ok(())
            },
            Ns::Wml => match el.local() {
                // Annotation anchors carry no content (ECMA-376 §17.13).
                "bookmarkStart" | "bookmarkEnd" | "proofErr" | "permStart" | "permEnd"
                | "commentRangeStart" | "commentRangeEnd" | "moveFromRangeStart"
                | "moveFromRangeEnd" | "moveToRangeStart" | "moveToRangeEnd" => self.skip(),
                // Inserted or moved-here content stays (§17.13.5).
                "ins" | "moveTo" => self.math_content("w:ins", out),
                "del" | "moveFrom" => {
                    self.skip()?;
                    self.info("deleted tracked-change content in an equation was skipped".into());
                    Ok(())
                },
                "r" => self.wml_run(out),
                _ => self.unknown(&el, parent),
            },
            _ => self.unknown(&el, parent),
        }
    }

    /// Parse an argument (`CT_OMathArg`) and return its nodes.
    fn arg(&mut self, name: &'static str) -> Result<Ids> {
        let mut out = Ids::new();
        let mut seen_pr = false;
        while let Some(child) = self.next_child(name)? {
            if child.is_math("argPr") {
                if self.once_props(&mut seen_pr, "argPr", name)? {
                    self.props("m:argPr", &["argSz"], |_, _| Ok(false))?;
                }
            } else if child.is_math("ctrlPr") {
                self.skip()?;
            } else {
                self.math_element(child, name, &mut out)?;
            }
        }
        Ok(out)
    }

    fn arg_id(&mut self, name: &'static str) -> Result<NodeId> {
        let ids = self.arg(name)?;
        self.group(&ids)
    }

    // ── Runs ────────────────────────────────────────────────────────────

    fn run(&mut self, out: &mut Ids) -> Result<()> {
        let mut props = MathRunProps::default();
        let mut host = HostRunProps::default();
        let mut text = String::new();
        let (mut seen_mpr, mut seen_hpr) = (false, false);
        while let Some(child) = self.next_child("m:r")? {
            match (child.ns, child.local()) {
                (Ns::Math, "rPr") => {
                    if self.once_props(&mut seen_mpr, "rPr", "m:r")? {
                        self.math_run_props(&mut props)?;
                    }
                },
                (Ns::Math, "t") => self.text_content(&child, &mut text)?,
                (Ns::Wml, "rPr") => {
                    if self.once_props(&mut seen_hpr, "rPr", "m:r")? {
                        self.wml_run_props(&mut host)?;
                    }
                },
                (Ns::Dml, "rPr") => {
                    if self.once_props(&mut seen_hpr, "rPr", "m:r")? {
                        self.dml_run_props(&child, &mut host)?;
                    }
                },
                _ => self.unknown(&child, "m:r")?,
            }
        }
        if props.lit {
            self.info("<m:lit> (literal operator text) has no equivalent in the AST".into());
        }
        self.emit_run(&text, &props, &host, out)
    }

    /// A WordprocessingML run inside an equation: plain (non-math) text.
    fn wml_run(&mut self, out: &mut Ids) -> Result<()> {
        let mut host = HostRunProps::default();
        let mut text = String::new();
        let mut seen_pr = false;
        while let Some(child) = self.next_child("w:r")? {
            match (child.ns, child.local()) {
                (Ns::Wml, "rPr") => {
                    if self.once_props(&mut seen_pr, "rPr", "w:r")? {
                        self.wml_run_props(&mut host)?;
                    }
                },
                (Ns::Wml, "t") => self.text_content(&child, &mut text)?,
                _ => self.unknown(&child, "w:r")?,
            }
        }
        let props = MathRunProps {
            nor: true,
            ..MathRunProps::default()
        };
        self.emit_run(&text, &props, &host, out)
    }

    fn text_content(&mut self, el: &Elem<'_>, text: &mut String) -> Result<()> {
        let name = el.qname().to_owned();
        loop {
            match self.next_item()? {
                Item::Text(t) => text.push_str(&t),
                Item::Start(child) => self.unknown(&child, &name)?,
                Item::End => return Ok(()),
                Item::Eof => return Err(omml_err(format!("unexpected end of input in <{name}>"))),
            }
        }
    }

    fn math_run_props(&mut self, props: &mut MathRunProps) -> Result<()> {
        self.props("m:rPr", &["brk"], |p, c| {
            match c.local() {
                "lit" => props.lit = p.on_off(c)?,
                "nor" => props.nor = p.on_off(c)?,
                "aln" => {
                    if p.on_off(c)? {
                        p.info("<m:aln> (alignment point) has no equivalent in the AST".into());
                    }
                },
                "sty" => {
                    props.sty = match p.leaf(c)?.as_deref() {
                        None => None,
                        Some("p") => Some("p"),
                        Some("b") => Some("b"),
                        Some("i") => Some("i"),
                        Some("bi") => Some("bi"),
                        Some(other) => p.bad_value(c, other, None)?,
                    }
                },
                "scr" => {
                    props.scr = match p.leaf(c)?.as_deref() {
                        None => None,
                        Some("roman") => Some("roman"),
                        Some("script") => Some("script"),
                        Some("fraktur") => Some("fraktur"),
                        Some("double-struck") => Some("double-struck"),
                        Some("sans-serif") => Some("sans-serif"),
                        Some("monospace") => Some("monospace"),
                        Some(other) => p.bad_value(c, other, None)?,
                    }
                },
                _ => return Ok(false),
            }
            Ok(true)
        })
    }

    /// Note a host run property the AST cannot hold.
    fn drop_host_property(&mut self, name: &str) {
        self.info(format!(
            "host run property {name} has no equivalent in the AST and was dropped"
        ));
    }

    /// `w:rPr`: keep bold, italic and color; other host formatting (fonts,
    /// sizes, highlighting, ...) is dropped with a note per property.
    fn wml_run_props(&mut self, host: &mut HostRunProps) -> Result<()> {
        while let Some(child) = self.next_child("w:rPr")? {
            if child.ns != Ns::Wml {
                let name = child.qname().to_owned();
                self.skip()?;
                self.drop_host_property(&format!("<{name}>"));
                continue;
            }
            match child.local() {
                "b" => host.bold = self.on_off(&child)?,
                "i" => host.italic = self.on_off(&child)?,
                "color" => {
                    let val = self.attr(&child, "val")?;
                    self.skip()?;
                    host.color = match val.as_deref() {
                        None | Some("auto") => None,
                        Some(hex) => match parse_hex_color(hex) {
                            Some(rgb) => Some(rgb),
                            None => self.bad_value(&child, hex, None)?,
                        },
                    };
                },
                _ => {
                    let name = child.qname().to_owned();
                    self.skip()?;
                    self.drop_host_property(&format!("<{name}>"));
                },
            }
        }
        Ok(())
    }

    /// `a:rPr` (DrawingML text run properties, ECMA-376 §21.1.2.3.9).
    fn dml_run_props(&mut self, el: &Elem<'_>, host: &mut HostRunProps) -> Result<()> {
        let xsd_bool = |v: Option<String>| matches!(v.as_deref(), Some("1" | "true"));
        host.bold = xsd_bool(self.attr(el, "b")?);
        host.italic = xsd_bool(self.attr(el, "i")?);
        let mut dropped: SmallVec<[String; 4]> = SmallVec::new();
        for attr in el.start.attributes() {
            let attr = attr.map_err(|e| self.xml_err(e))?;
            let key = attr.key.into_inner();
            if !matches!(key, "b" | "i") && !key.starts_with("xmlns") {
                dropped.push(key.to_owned());
            }
        }
        for key in dropped {
            self.drop_host_property(&format!("a:rPr@{key}"));
        }
        while let Some(child) = self.next_child("a:rPr")? {
            if child.ns == Ns::Dml && child.local() == "solidFill" {
                while let Some(color) = self.next_child("a:solidFill")? {
                    let value = match (color.ns, color.local()) {
                        (Ns::Dml, "srgbClr") => self.attr(&color, "val")?,
                        (Ns::Dml, "sysClr") => self.attr(&color, "lastClr")?,
                        _ => None,
                    };
                    self.skip()?;
                    match value.as_deref().map(|v| (v, parse_hex_color(v))) {
                        Some((_, Some(rgb))) => host.color = Some(rgb),
                        Some((v, None)) => host.color = self.bad_value(&color, v, None)?,
                        None => self.info(format!(
                            "run color <{}> cannot be resolved to RGB and was dropped",
                            color.qname()
                        )),
                    }
                }
            } else {
                let name = child.qname().to_owned();
                self.skip()?;
                self.drop_host_property(&format!("<{name}>"));
            }
        }
        Ok(())
    }

    fn emit_run(
        &mut self,
        text: &str,
        props: &MathRunProps,
        host: &HostRunProps,
        out: &mut Ids,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let mut ids = Ids::new();
        if props.nor {
            let sid = self.arena.push_str(text);
            ids.push(
                self.arena
                    .push_node(CompactNode::Text(host.text_style(), sid)),
            );
        } else {
            self.tokenize(text, props.style(), &mut ids)?;
        }
        match host.color {
            // One wrapper per token, matching the writer's one run per token.
            Some([r, g, b]) => {
                let color = self.arena.push_str(&format!("#{r:02X}{g:02X}{b:02X}"));
                for content in ids {
                    out.push(self.arena.push_node(CompactNode::Color { color, content }));
                }
            },
            None => out.extend(ids),
        }
        Ok(())
    }

    /// Split the text of a math run into numbers, letter runs, symbols and
    /// spaces. Mathematical alphanumeric characters (U+1D400 block, as
    /// written by PowerPoint) are decoded into a base letter plus a style.
    fn tokenize(&mut self, text: &str, style: Option<TextStyle>, out: &mut Ids) -> Result<()> {
        // The OMML writer keeps compound limit names such as `lim\u{2009}sup`
        // in one math run.  The thin space is otherwise tokenized as an
        // ordinary `Space`, which splits the operator on reimport and loses
        // its movable-limit metadata.  Recognize only the canonical named
        // limit spellings here so an authored arbitrary identifier remains a
        // normal run.
        if matches!(style, None | Some(TextStyle::Normal))
            && text.contains('\u{2009}')
            && crate::unicode::symbols::operator_has_limits(text)
        {
            let sid = self.arena.push_str(text);
            out.push(self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits: LimitStyle::Default,
            }));
            return Ok(());
        }
        let chars: Vec<(char, char, Option<TextStyle>)> = text
            .chars()
            .map(
                |c| match crate::unicode::unstyled_char(c).or_else(|| unstyled_greek(c)) {
                    Some((base, st)) => (c, base, Some(st)),
                    None => (c, c, None),
                },
            )
            .collect();
        let mut i = 0;
        let mut buf = String::new();
        while i < chars.len() {
            let (orig, base, cstyle) = chars[i];
            if let Some(width) = space_width(orig) {
                out.push(self.arena.push_node(CompactNode::Space(width)));
                i += 1;
            } else if orig.is_whitespace() {
                buf.clear();
                while i < chars.len()
                    && chars[i].0.is_whitespace()
                    && space_width(chars[i].0).is_none()
                {
                    buf.push(chars[i].0);
                    i += 1;
                }
                let sid = self.arena.push_str(&buf);
                out.push(
                    self.arena
                        .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                );
            } else if base.is_ascii_digit() {
                buf.clear();
                while i < chars.len() {
                    let (_, b, st) = chars[i];
                    let digit = b.is_ascii_digit() && st == cstyle;
                    let sep = matches!(b, '.' | ',')
                        && chars
                            .get(i + 1)
                            .is_some_and(|&(_, nb, nst)| nb.is_ascii_digit() && nst == cstyle);
                    if !(digit || sep) {
                        break;
                    }
                    buf.push(b);
                    i += 1;
                }
                let sid = self.arena.push_str(&buf);
                let number = self.arena.push_node(CompactNode::Number(sid));
                let id = match cstyle.or(style) {
                    None | Some(TextStyle::Normal) => number,
                    Some(st) => self.styled(st, number)?,
                };
                out.push(id);
            } else if base.is_alphabetic() {
                buf.clear();
                while i < chars.len() && chars[i].1.is_alphabetic() && chars[i].2 == cstyle {
                    buf.push(chars[i].1);
                    i += 1;
                }
                let id = self.letters(&buf, cstyle.or(style))?;
                out.push(id);
            } else {
                let mut tmp = [0u8; 4];
                let sym = self.symbol(classify_op_char(orig), orig.encode_utf8(&mut tmp));
                let id = match style {
                    None | Some(TextStyle::Normal | TextStyle::Italic) => sym,
                    Some(st) => self.styled(st, sym)?,
                };
                out.push(id);
                i += 1;
            }
        }
        Ok(())
    }

    fn styled(&mut self, style: TextStyle, id: NodeId) -> Result<NodeId> {
        let span = checked_span(&mut self.arena, &[id])?;
        Ok(self.arena.push_node(CompactNode::Styled(style, span)))
    }

    /// Classify a run of letters. Without an explicit style (math italic)
    /// or with upright style, a known function name becomes an operator.
    fn letters(&mut self, text: &str, style: Option<TextStyle>) -> Result<NodeId> {
        let sid = self.arena.push_str(text);
        let is_operator =
            text.chars().count() > 1 && crate::unicode::symbols::is_operator_command(text);
        match style {
            None | Some(TextStyle::Normal) if is_operator => {
                let limits = if crate::unicode::symbols::operator_has_limits(text) {
                    LimitStyle::Default
                } else {
                    LimitStyle::SubSup
                };
                Ok(self
                    .arena
                    .push_node(CompactNode::Operator { name: sid, limits }))
            },
            None | Some(TextStyle::Italic) => {
                Ok(self.arena.push_node(CompactNode::Identifier(sid)))
            },
            Some(st) => {
                let ident = self.arena.push_node(CompactNode::Identifier(sid));
                self.styled(st, ident)
            },
        }
    }

    // ── Math objects ────────────────────────────────────────────────────

    /// `m:acc` (§22.1.2.1): `accPr{chr}`, `e`.
    fn accent(&mut self) -> Result<NodeId> {
        let mut chr = Some("\u{0302}".to_owned());
        let mut base = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:acc")? {
            match (c.ns, c.local()) {
                (Ns::Math, "accPr") => {
                    if self.once_props(&mut seen_pr, "accPr", "m:acc")? {
                        self.props("m:accPr", &[], |p, c| {
                            if c.local() != "chr" {
                                return Ok(false);
                            }
                            chr = p.char_val(c)?;
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", "m:acc")?;
                },
                _ => self.unknown(&c, "m:acc")?,
            }
        }
        let base = self.or_empty(base);
        match chr.and_then(|s| s.chars().next()) {
            Some(accent) => Ok(self.arena.push_node(CompactNode::Accent {
                base,
                accent,
                position: AccentPosition::Above,
                wide: false,
            })),
            None => Ok(base),
        }
    }

    /// `m:bar` (§22.1.2.7): `barPr{pos}` (default `bot`), `e`.
    fn bar(&mut self) -> Result<NodeId> {
        let mut top = false;
        let mut base = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:bar")? {
            match (c.ns, c.local()) {
                (Ns::Math, "barPr") => {
                    if self.once_props(&mut seen_pr, "barPr", "m:bar")? {
                        self.props("m:barPr", &[], |p, c| {
                            if c.local() != "pos" {
                                return Ok(false);
                            }
                            top = p.top_bot(c, false)?;
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", "m:bar")?;
                },
                _ => self.unknown(&c, "m:bar")?,
            }
        }
        let base = self.or_empty(base);
        let line = self.symbol(SymbolKind::Ordinary, "\u{203E}");
        Ok(self.arena.push_node(if top {
            CompactNode::Overscript {
                base,
                over: line,
                movable_limits: false,
            }
        } else {
            CompactNode::Underscript {
                base,
                under: line,
                movable_limits: false,
            }
        }))
    }

    /// `ST_TopBot`; returns `true` for `top`.
    fn top_bot(&mut self, el: &Elem<'_>, default_top: bool) -> Result<bool> {
        match self.leaf(el)?.as_deref() {
            Some("top") => Ok(true),
            Some("bot") => Ok(false),
            None => self
                .recover(format!("<{}> requires a val attribute", el.qname()))
                .map(|()| default_top),
            Some(other) => self.bad_value(el, other, default_top),
        }
    }

    /// `m:box` (§22.1.2.13): a grouped object, read as an ordinary atom, or
    /// as an operator atom when `m:opEmu` is on.
    fn boxed(&mut self) -> Result<NodeId> {
        let mut op_emu = false;
        let mut content = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:box")? {
            match (c.ns, c.local()) {
                (Ns::Math, "boxPr") => {
                    if self.once_props(&mut seen_pr, "boxPr", "m:box")? {
                        self.props("m:boxPr", &["brk"], |p, c| {
                            match c.local() {
                                "opEmu" => op_emu = p.on_off(c)?,
                                // On by default; turning it off cannot be kept.
                                "noBreak" => {
                                    if !p.on_off(c)? {
                                        p.info("<m:noBreak m:val=\"off\"> in <m:boxPr> has no equivalent in the AST".into());
                                    }
                                },
                                "diff" | "aln" => {
                                    if p.on_off(c)? {
                                        p.info(format!("<m:{}> in <m:boxPr> has no equivalent in the AST", c.local()));
                                    }
                                },
                                _ => return Ok(false),
                            }
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut content, id, "e", "m:box")?;
                },
                _ => self.unknown(&c, "m:box")?,
            }
        }
        let content = self.or_empty(content);
        // The writer boxes an ordinary operator glyph (`\dag`, `\mathord{\lneqq}`)
        // so it does not read back with its symbol-table class.
        if !op_emu && let Some(symbol) = self.boxed_operator_glyph(content) {
            return Ok(symbol);
        }
        let atom_type = if op_emu {
            MathAtomType::Op
        } else {
            MathAtomType::Ord
        };
        Ok(self
            .arena
            .push_node(CompactNode::MathAtom { atom_type, content }))
    }

    /// The ordinary symbol a box holding only one table-classed operator glyph
    /// stands for, if `content` is such a glyph (directly or in a one-item row).
    fn boxed_operator_glyph(&mut self, content: NodeId) -> Option<NodeId> {
        let mut id = content;
        if let CompactNode::Row(span) = *self.arena.node(id) {
            let &[only] = self.arena.children(span) else {
                return None;
            };
            id = only;
        }
        let CompactNode::Symbol(_, sid) = *self.arena.node(id) else {
            return None;
        };
        let mut chars = self.arena.str_val(sid).chars();
        let ch = chars.next()?;
        (chars.as_str().is_empty()
            && ch != '\u{2016}'
            && crate::unicode::symbols::operator_glyph_kind(ch).is_some())
        .then(|| {
            self.arena
                .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid))
        })
    }

    /// `m:borderBox` (§22.1.2.11). Strikes over a box whose four sides are
    /// hidden are a cancellation; strikes over a visible box are a
    /// cancellation inside a box.
    fn border_box(&mut self) -> Result<NodeId> {
        let mut hide = [false; 4];
        let mut strikes = [false; 4]; // H, V, BLTR, TLBR
        let mut content = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:borderBox")? {
            match (c.ns, c.local()) {
                (Ns::Math, "borderBoxPr") => {
                    if self.once_props(&mut seen_pr, "borderBoxPr", "m:borderBox")? {
                        self.props("m:borderBoxPr", &[], |p, c| {
                            let slot = match c.local() {
                                "hideTop" => &mut hide[0],
                                "hideBot" => &mut hide[1],
                                "hideLeft" => &mut hide[2],
                                "hideRight" => &mut hide[3],
                                "strikeH" => &mut strikes[0],
                                "strikeV" => &mut strikes[1],
                                "strikeBLTR" => &mut strikes[2],
                                "strikeTLBR" => &mut strikes[3],
                                _ => return Ok(false),
                            };
                            *slot = p.on_off(c)?;
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut content, id, "e", "m:borderBox")?;
                },
                _ => self.unknown(&c, "m:borderBox")?,
            }
        }
        let mut node = self.or_empty(content);
        let mut kinds: SmallVec<[StrokeKind; 3]> = SmallVec::new();
        if strikes[2] && strikes[3] {
            kinds.push(StrokeKind::Cross);
        } else if strikes[2] {
            kinds.push(StrokeKind::Forward);
        } else if strikes[3] {
            kinds.push(StrokeKind::Backward);
        }
        if strikes[0] {
            kinds.push(StrokeKind::Horizontal);
        }
        if strikes[1] {
            kinds.push(StrokeKind::Vertical);
        }
        for stroke in kinds {
            node = self.arena.push_node(CompactNode::Cancel {
                stroke,
                content: node,
            });
        }
        let all_hidden = hide.iter().all(|&h| h);
        let actuarial = hide == [false, true, true, false] && strikes == [false; 4];
        if actuarial {
            node = self.arena.push_node(CompactNode::Actuarial {
                content: node,
                text_mode: false,
            });
        } else if !all_hidden {
            if hide.iter().any(|&h| h) {
                self.info(
                    "partially hidden <m:borderBox> sides are not represented; read as a full box"
                        .into(),
                );
            }
            let empty = self.arena.push_str("");
            node = self.arena.push_node(CompactNode::Boxed {
                content: node,
                bg_color: empty,
                border_color: empty,
            });
        }
        Ok(node)
    }

    /// `m:d` (§22.1.2.24): `dPr{begChr, sepChr, endChr, grow, shp}`, `e+`.
    /// Defaults: `(`, `|`, `)`; an empty value means no character.
    fn delimiter(&mut self) -> Result<NodeId> {
        let mut open = "(".to_owned();
        let mut close = ")".to_owned();
        let mut sep = "|".to_owned();
        let mut elements: SmallVec<[NodeId; 4]> = SmallVec::new();
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:d")? {
            match (c.ns, c.local()) {
                (Ns::Math, "dPr") => {
                    if self.once_props(&mut seen_pr, "dPr", "m:d")? {
                        self.props("m:dPr", &["shp"], |p, c| {
                            let slot = match c.local() {
                                "begChr" => &mut open,
                                "endChr" => &mut close,
                                "sepChr" => &mut sep,
                                "grow" => {
                                    if !p.on_off(c)? {
                                        p.info("non-growing delimiters (<m:grow m:val=\"off\">) are not represented".into());
                                    }
                                    return Ok(true);
                                },
                                _ => return Ok(false),
                            };
                            *slot = p.char_val(c)?.unwrap_or_default();
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    elements.push(id);
                },
                _ => self.unknown(&c, "m:d")?,
            }
        }
        let mut content: Ids = SmallVec::new();
        let sep_sid = self.arena.push_str(&sep);
        for (idx, &id) in elements.iter().enumerate() {
            if idx > 0 {
                content.push(self.arena.push_node(CompactNode::Middle(sep_sid)));
            }
            content.push(id);
        }
        let content = checked_span(&mut self.arena, &content)?;
        let open = self.arena.push_str(&open);
        let close = self.arena.push_str(&close);
        Ok(self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        }))
    }

    /// `m:eqArr` (§22.1.2.34): a single-column, centred stack of equations,
    /// read as an array with equation-array row spacing (`add_jot`).
    fn eq_array(&mut self) -> Result<NodeId> {
        let mut rows = Vec::new();
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:eqArr")? {
            match (c.ns, c.local()) {
                (Ns::Math, "eqArrPr") => {
                    if self.once_props(&mut seen_pr, "eqArrPr", "m:eqArr")? {
                        self.props(
                            "m:eqArrPr",
                            &["baseJc", "maxDist", "objDist", "rSpRule", "rSp"],
                            |_, _| Ok(false),
                        )?;
                    }
                },
                (Ns::Math, "e") => {
                    let ids = self.arg("m:e")?;
                    // `&` in a row's text marks an alignment point (the
                    // convention Word uses for equation arrays): split the
                    // row into cells at each one.
                    let mut cells: SmallVec<[NodeId; 4]> = SmallVec::new();
                    let mut start = 0;
                    for end in 0..=ids.len() {
                        if end == ids.len() || self.is_alignment_mark(ids[end]) {
                            let span = checked_span(&mut self.arena, &ids[start..end])?;
                            cells.push(self.arena.push_node(CompactNode::Row(span)));
                            start = end + 1;
                        }
                    }
                    rows.push(cells);
                },
                _ => self.unknown(&c, "m:eqArr")?,
            }
        }
        // Aligned columns alternate right- and left-aligned, as in amsmath's
        // `aligned`; without alignment points the rows are centred.
        let widest = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let columns = if widest > 1 {
            (0..widest)
                .map(|i| {
                    if i % 2 == 0 {
                        ColumnAlign::Right
                    } else {
                        ColumnAlign::Left
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        self.build_array(rows, columns, true)
    }

    /// A top-level `&` symbol (possibly colored) in an equation-array row.
    fn is_alignment_mark(&self, id: NodeId) -> bool {
        let id = match *self.arena.node(id) {
            CompactNode::Color { content, .. } => content,
            _ => id,
        };
        matches!(*self.arena.node(id), CompactNode::Symbol(_, sid) if self.arena.str_val(sid) == "&")
    }

    /// `m:f` (§22.1.2.36): `fPr{type}`, `num`, `den`.
    fn fraction(&mut self) -> Result<NodeId> {
        let mut kind = FractionKind::Normal;
        let (mut num, mut den) = (None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:f")? {
            match (c.ns, c.local()) {
                (Ns::Math, "fPr") => {
                    if self.once_props(&mut seen_pr, "fPr", "m:f")? {
                        self.props("m:fPr", &[], |p, c| {
                            if c.local() != "type" {
                                return Ok(false);
                            }
                            kind = match p.leaf(c)?.as_deref() {
                                Some("bar") => FractionKind::Normal,
                                Some("noBar") => FractionKind::NoBar,
                                Some("skw") => FractionKind::Skewed,
                                Some("lin") => FractionKind::Inline,
                                Some(other) => p.bad_value(c, other, FractionKind::Normal)?,
                                None => p
                                    .recover("<m:type> requires a val attribute".into())
                                    .map(|()| FractionKind::Normal)?,
                            };
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "num") => {
                    let id = self.arg_id("m:num")?;
                    self.set_once(&mut num, id, "num", "m:f")?;
                },
                (Ns::Math, "den") => {
                    let id = self.arg_id("m:den")?;
                    self.set_once(&mut den, id, "den", "m:f")?;
                },
                _ => self.unknown(&c, "m:f")?,
            }
        }
        let num = self.or_empty(num);
        let den = self.or_empty(den);
        Ok(self
            .arena
            .push_node(CompactNode::Fraction { kind, num, den }))
    }

    /// `m:func` (§22.1.2.39): `fName`, `e`, read as
    /// `Row[name, U+2061, argument]`.
    fn func(&mut self) -> Result<NodeId> {
        let (mut name, mut arg) = (None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:func")? {
            match (c.ns, c.local()) {
                (Ns::Math, "funcPr") => {
                    if self.once_props(&mut seen_pr, "funcPr", "m:func")? {
                        self.props("m:funcPr", &[], |_, _| Ok(false))?;
                    }
                },
                (Ns::Math, "fName") => {
                    let id = self.arg_id("m:fName")?;
                    self.set_once(&mut name, id, "fName", "m:func")?;
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut arg, id, "e", "m:func")?;
                },
                _ => self.unknown(&c, "m:func")?,
            }
        }
        let name = self.or_empty(name);
        let arg = self.or_empty(arg);
        if self.is_empty_row(arg) {
            return Ok(self.arena.push_node(CompactNode::StructuredOperator {
                content: name,
                form: OperatorNameForm::Starred(LimitStyle::Default),
            }));
        }
        let apply = self.symbol(SymbolKind::Ordinary, FUNCTION_APPLICATION);
        let span = checked_span(&mut self.arena, &[name, apply, arg])?;
        Ok(self.arena.push_node(CompactNode::Row(span)))
    }

    /// `m:groupChr` (§22.1.2.41): `groupChrPr{chr, pos, vertJc}`, `e`.
    /// Defaults: U+23DF BOTTOM CURLY BRACKET, below the base.
    fn group_chr(&mut self) -> Result<NodeId> {
        let mut chr = "\u{23DF}".to_owned();
        let mut top = false;
        let mut base = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:groupChr")? {
            match (c.ns, c.local()) {
                (Ns::Math, "groupChrPr") => {
                    if self.once_props(&mut seen_pr, "groupChrPr", "m:groupChr")? {
                        self.props("m:groupChrPr", &["vertJc"], |p, c| {
                            match c.local() {
                                "chr" => chr = p.char_val(c)?.unwrap_or_default(),
                                "pos" => top = p.top_bot(c, false)?,
                                _ => return Ok(false),
                            }
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", "m:groupChr")?;
                },
                _ => self.unknown(&c, "m:groupChr")?,
            }
        }
        let base = self.or_empty(base);
        let sym = self.symbol(SymbolKind::Ordinary, &chr);
        // The writer's form of a combining mark below (`m:acc` is above).
        let mark = !top
            && chr.chars().count() == 1
            && chr.starts_with(crate::unicode::symbols::is_mark_below);
        if mark || matches!(chr.as_str(), "→" | "←") {
            return Ok(self.arena.push_node(CompactNode::Accent {
                base,
                accent: chr.chars().next().unwrap_or('→'),
                position: if top {
                    AccentPosition::Above
                } else {
                    AccentPosition::Below
                },
                wide: false,
            }));
        }
        Ok(self.arena.push_node(if top {
            CompactNode::Overscript {
                base,
                over: sym,
                movable_limits: false,
            }
        } else {
            CompactNode::Underscript {
                base,
                under: sym,
                movable_limits: false,
            }
        }))
    }

    /// `m:limLow` / `m:limUpp` (§22.1.2.54, §22.1.2.56): `e`, `lim`.
    fn limit(&mut self, upper: bool) -> Result<NodeId> {
        let (name, pr) = if upper {
            ("m:limUpp", "limUppPr")
        } else {
            ("m:limLow", "limLowPr")
        };
        let (mut base, mut lim) = (None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child(name)? {
            match (c.ns, c.local()) {
                (Ns::Math, l) if l == pr => {
                    if self.once_props(&mut seen_pr, pr, name)? {
                        self.props(
                            if upper { "m:limUppPr" } else { "m:limLowPr" },
                            &[],
                            |_, _| Ok(false),
                        )?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", name)?;
                },
                (Ns::Math, "lim") => {
                    let id = self.arg_id("m:lim")?;
                    self.set_once(&mut lim, id, "lim", name)?;
                },
                _ => self.unknown(&c, name)?,
            }
        }
        let base = self.or_empty(base);
        let lim = self.or_empty(lim);
        // The native limit object fixes this bound above or below its
        // base. A function name carries that placement directly; combine
        // nested complementary limit objects into its two script fields.
        let named = |node: &CompactNode| {
            matches!(
                node,
                CompactNode::Operator {
                    limits: LimitStyle::AboveBelow,
                    ..
                } | CompactNode::StructuredOperator {
                    form: OperatorNameForm::Starred(LimitStyle::AboveBelow),
                    ..
                }
            )
        };
        match *self.arena.node(base) {
            CompactNode::Operator { name, .. } => {
                let base = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: LimitStyle::AboveBelow,
                });
                return Ok(self.arena.push_node(if upper {
                    CompactNode::Superscript { base, sup: lim }
                } else {
                    CompactNode::Subscript { base, sub: lim }
                }));
            },
            CompactNode::StructuredOperator { content, .. } => {
                let base = self.arena.push_node(CompactNode::StructuredOperator {
                    content,
                    form: OperatorNameForm::Starred(LimitStyle::AboveBelow),
                });
                return Ok(self.arena.push_node(if upper {
                    CompactNode::Superscript { base, sup: lim }
                } else {
                    CompactNode::Subscript { base, sub: lim }
                }));
            },
            CompactNode::Subscript { base, sub } if upper && named(self.arena.node(base)) => {
                return Ok(self.arena.push_node(CompactNode::SubSup {
                    base,
                    sub,
                    sup: lim,
                }));
            },
            CompactNode::Superscript { base, sup } if !upper && named(self.arena.node(base)) => {
                return Ok(self.arena.push_node(CompactNode::SubSup {
                    base,
                    sub: lim,
                    sup,
                }));
            },
            _ => {},
        }
        Ok(self.arena.push_node(if upper {
            CompactNode::Overscript {
                base,
                over: lim,
                movable_limits: true,
            }
        } else {
            CompactNode::Underscript {
                base,
                under: lim,
                movable_limits: true,
            }
        }))
    }

    /// `m:m` (§22.1.2.60): `mPr{..., mcs}`, `mr+` of `e+`.
    fn matrix(&mut self) -> Result<NodeId> {
        let mut rows: Vec<SmallVec<[NodeId; 4]>> = Vec::new();
        let mut columns: Vec<ColumnAlign> = Vec::new();
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:m")? {
            match (c.ns, c.local()) {
                (Ns::Math, "mPr") => {
                    if self.once_props(&mut seen_pr, "mPr", "m:m")? {
                        self.props(
                            "m:mPr",
                            &[
                                "baseJc", "plcHide", "rSpRule", "cGpRule", "rSp", "cSp", "cGp",
                            ],
                            |p, c| {
                                if c.local() != "mcs" {
                                    return Ok(false);
                                }
                                columns = p.matrix_columns()?;
                                Ok(true)
                            },
                        )?;
                    }
                },
                (Ns::Math, "mr") => {
                    let mut cells = SmallVec::new();
                    while let Some(e) = self.next_child("m:mr")? {
                        if e.is_math("e") {
                            let ids = self.arg("m:e")?;
                            let span = checked_span(&mut self.arena, &ids)?;
                            cells.push(self.arena.push_node(CompactNode::Row(span)));
                        } else {
                            self.unknown(&e, "m:mr")?;
                        }
                    }
                    rows.push(cells);
                },
                _ => self.unknown(&c, "m:m")?,
            }
        }
        self.build_array(rows, columns, false)
    }

    /// `m:mcs` (§22.1.2.66): column groups `mc{mcPr{count, mcJc}}`.
    fn matrix_columns(&mut self) -> Result<Vec<ColumnAlign>> {
        let mut columns = Vec::new();
        while let Some(mc) = self.next_child("m:mcs")? {
            if !mc.is_math("mc") {
                self.unknown(&mc, "m:mcs")?;
                continue;
            }
            let mut count = 1usize;
            let mut align = ColumnAlign::Center;
            while let Some(pr) = self.next_child("m:mc")? {
                if !pr.is_math("mcPr") {
                    self.unknown(&pr, "m:mc")?;
                    continue;
                }
                self.props("m:mcPr", &[], |p, c| {
                    match c.local() {
                        "count" => {
                            // ST_Integer255: 1..=255.
                            let v = p.leaf(c)?.unwrap_or_default();
                            count = match v.parse::<usize>() {
                                Ok(n @ 1..=255) => n,
                                _ => p.bad_value(c, &v, 1)?,
                            };
                        },
                        "mcJc" => {
                            let v = p.leaf(c)?.unwrap_or_default();
                            align = match v.as_str() {
                                "left" | "inside" => ColumnAlign::Left,
                                "right" | "outside" => ColumnAlign::Right,
                                "center" => ColumnAlign::Center,
                                other => p.bad_value(c, other, ColumnAlign::Center)?,
                            };
                        },
                        _ => return Ok(false),
                    }
                    Ok(true)
                })?;
            }
            if columns.len() + count > u16::MAX as usize {
                return Err(omml_err("matrix declares too many columns"));
            }
            columns.extend(std::iter::repeat_n(align, count));
        }
        Ok(columns)
    }

    /// Build an `Array` from row-major cell `Row` ids. Short rows are padded;
    /// the cell count is bounded by the arena's span limit.
    fn build_array(
        &mut self,
        rows: Vec<SmallVec<[NodeId; 4]>>,
        columns: Vec<ColumnAlign>,
        eq_array: bool,
    ) -> Result<NodeId> {
        let widest = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let ncols = widest.max(columns.len()).max(1);
        if rows.len().saturating_mul(ncols) > u16::MAX as usize {
            return Err(omml_err(format!(
                "matrix of {} rows and {ncols} columns exceeds the supported {} cells",
                rows.len(),
                u16::MAX
            )));
        }
        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for idx in 0..ncols {
            let align = columns.get(idx).copied().unwrap_or(ColumnAlign::Center);
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(align)));
        }
        let col_aligns = checked_span(&mut self.arena, &align_ids)?;
        // Nodes not backed by input: padding cells of short rows and
        // declared column alignments beyond the widest row. Both draw on one
        // budget for the whole input.
        let read: usize = rows.iter().map(|r| r.len()).sum();
        let padding = rows.len() * ncols - read;
        let extra = padding + ncols.saturating_sub(widest.max(1));
        if extra > self.padding_budget {
            return Err(omml_err(format!(
                "a matrix needs {extra} padding cells and column alignments, more than \
                 the {} the input size allows",
                self.padding_budget
            )));
        }
        self.padding_budget -= extra;
        // Each padding cell is its own node, so editing one in place cannot
        // change the others.
        let mut cells: Vec<NodeId> = Vec::with_capacity(rows.len() * ncols);
        for row in &rows {
            cells.extend_from_slice(row);
            for _ in row.len()..ncols {
                cells.push(self.empty_row());
            }
        }
        let cells = checked_span(&mut self.arena, &cells)?;
        Ok(self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: ncols as u16,
            arraystretch_pct: 100,
            add_jot: eq_array,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        }))
    }

    /// `m:nary` (§22.1.2.70): `naryPr{chr, limLoc, grow, subHide, supHide}`,
    /// `sub`, `sup`, `e`. Default operator: U+222B INTEGRAL.
    fn nary(&mut self) -> Result<NodeId> {
        let mut op = Some('\u{222B}');
        let mut limits = LimitStyle::Default;
        let (mut sub_hide, mut sup_hide) = (false, false);
        let (mut sub, mut sup, mut body) = (None, None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:nary")? {
            match (c.ns, c.local()) {
                (Ns::Math, "naryPr") => {
                    if self.once_props(&mut seen_pr, "naryPr", "m:nary")? {
                        self.props("m:naryPr", &[], |p, c| {
                            match c.local() {
                                // An empty value means no operator character.
                                "chr" => op = p.char_val(c)?.and_then(|s| s.chars().next()),
                                "limLoc" => {
                                    limits = match p.leaf(c)?.as_deref() {
                                        Some("undOvr") => LimitStyle::AboveBelow,
                                        Some("subSup") => LimitStyle::SubSup,
                                        Some(other) => p.bad_value(c, other, LimitStyle::Default)?,
                                        None => p
                                            .recover("<m:limLoc> requires a val attribute".into())
                                            .map(|()| LimitStyle::Default)?,
                                    };
                                },
                                "grow" => {
                                    if p.on_off(c)? {
                                        p.info("growing n-ary operators (<m:grow>) are not represented".into());
                                    }
                                },
                                "subHide" => sub_hide = p.on_off(c)?,
                                "supHide" => sup_hide = p.on_off(c)?,
                                _ => return Ok(false),
                            }
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "sub") => {
                    let id = self.arg_id("m:sub")?;
                    self.set_once(&mut sub, id, "sub", "m:nary")?;
                },
                (Ns::Math, "sup") => {
                    let id = self.arg_id("m:sup")?;
                    self.set_once(&mut sup, id, "sup", "m:nary")?;
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut body, id, "e", "m:nary")?;
                },
                _ => self.unknown(&c, "m:nary")?,
            }
        }
        let body = self.or_empty(body);
        let sub = self.limit_slot(sub, sub_hide, "lower");
        let sup = self.limit_slot(sup, sup_hide, "upper");
        let Some(op) = op else {
            // The AST's n-ary node needs an operator character. Lenient mode
            // keeps the limits as scripts on an empty base before the body.
            self.recover(
                "an n-ary object without an operator character cannot be represented".into(),
            )?;
            let base = self.empty_row();
            let scripts = match (sub.is_some(), sup.is_some()) {
                (true, true) => Some(CompactNode::SubSup { base, sub, sup }),
                (true, false) => Some(CompactNode::Subscript { base, sub }),
                (false, true) => Some(CompactNode::Superscript { base, sup }),
                (false, false) => None,
            };
            let mut ids = Ids::new();
            if let Some(node) = scripts {
                ids.push(self.arena.push_node(node));
            }
            ids.push(body);
            return self.group(&ids);
        };
        Ok(self.arena.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        }))
    }

    /// A limit or degree slot: hidden or empty slots become `NONE`.
    fn limit_slot(&mut self, id: Option<NodeId>, hidden: bool, what: &str) -> NodeId {
        match id {
            Some(id) if !self.is_empty_row(id) => {
                if hidden {
                    self.info(format!("hidden {what} limit content was dropped"));
                    NodeId::NONE
                } else {
                    id
                }
            },
            _ => NodeId::NONE,
        }
    }

    /// `m:phant` (§22.1.2.81): `phantPr{show, zeroWid, zeroAsc, zeroDesc,
    /// transp}`, `e`.
    fn phantom(&mut self) -> Result<NodeId> {
        // show, zeroWid, zeroAsc, zeroDesc. Content is shown unless m:show is
        // turned off: Word writes \smash as zeroAsc + zeroDesc without m:show.
        let mut flags = [true, false, false, false];
        let mut content = None;
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:phant")? {
            match (c.ns, c.local()) {
                (Ns::Math, "phantPr") => {
                    if self.once_props(&mut seen_pr, "phantPr", "m:phant")? {
                        self.props("m:phantPr", &["transp"], |p, c| {
                            let slot = match c.local() {
                                "show" => &mut flags[0],
                                "zeroWid" => &mut flags[1],
                                "zeroAsc" => &mut flags[2],
                                "zeroDesc" => &mut flags[3],
                                _ => return Ok(false),
                            };
                            *slot = p.on_off(c)?;
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut content, id, "e", "m:phant")?;
                },
                _ => self.unknown(&c, "m:phant")?,
            }
        }
        let content = self.or_empty(content);
        let [show, zero_wid, zero_asc, zero_desc] = flags;
        if show && !zero_asc && !zero_desc {
            // Visible content with its height and depth kept: only a zero
            // width could make this differ from the plain content.
            if zero_wid {
                self.info("a visible zero-width phantom was read as its plain content".into());
            }
            return Ok(content);
        }
        let kind = phantom_kind_from_flags(show, zero_wid, zero_asc, zero_desc);
        if crate::writer::omml::phantom_pr_flags(kind) != (show, zero_wid, zero_asc, zero_desc) {
            self.info(format!(
                "phantom flags (show={show}, zeroWid={zero_wid}, zeroAsc={zero_asc}, \
                 zeroDesc={zero_desc}) were read as the closest phantom kind {kind:?}"
            ));
        }
        Ok(self.arena.push_node(CompactNode::Phantom(content, kind)))
    }

    /// `m:rad` (§22.1.2.88): `radPr{degHide}`, `deg`, `e`.
    fn radical(&mut self) -> Result<NodeId> {
        let mut deg_hide = false;
        let (mut deg, mut radicand) = (None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:rad")? {
            match (c.ns, c.local()) {
                (Ns::Math, "radPr") => {
                    if self.once_props(&mut seen_pr, "radPr", "m:rad")? {
                        self.props("m:radPr", &[], |p, c| {
                            if c.local() != "degHide" {
                                return Ok(false);
                            }
                            deg_hide = p.on_off(c)?;
                            Ok(true)
                        })?;
                    }
                },
                (Ns::Math, "deg") => {
                    let id = self.arg_id("m:deg")?;
                    self.set_once(&mut deg, id, "deg", "m:rad")?;
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut radicand, id, "e", "m:rad")?;
                },
                _ => self.unknown(&c, "m:rad")?,
            }
        }
        let radicand = self.or_empty(radicand);
        let index = self.limit_slot(deg, deg_hide, "radical degree");
        Ok(self.arena.push_node(if index.is_none() {
            CompactNode::Sqrt(radicand)
        } else {
            CompactNode::Root { index, radicand }
        }))
    }

    /// `m:sPre` (§22.1.2.99): `sub`, `sup`, `e`.
    fn pre_scripts(&mut self) -> Result<NodeId> {
        let (mut sub, mut sup, mut base) = (None, None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child("m:sPre")? {
            match (c.ns, c.local()) {
                (Ns::Math, "sPrePr") => {
                    if self.once_props(&mut seen_pr, "sPrePr", "m:sPre")? {
                        self.props("m:sPrePr", &[], |_, _| Ok(false))?;
                    }
                },
                (Ns::Math, "sub") => {
                    let id = self.arg_id("m:sub")?;
                    self.set_once(&mut sub, id, "sub", "m:sPre")?;
                },
                (Ns::Math, "sup") => {
                    let id = self.arg_id("m:sup")?;
                    self.set_once(&mut sup, id, "sup", "m:sPre")?;
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", "m:sPre")?;
                },
                _ => self.unknown(&c, "m:sPre")?,
            }
        }
        let base = self.or_empty(base);
        let pre_sub = self.limit_slot(sub, false, "");
        let pre_sup = self.limit_slot(sup, false, "");
        Ok(self.arena.push_node(CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        }))
    }

    /// `m:sSub`, `m:sSup`, `m:sSubSup` (§22.1.2.101–105).
    fn scripts(
        &mut self,
        name: &'static str,
        pr: &'static str,
        has_sup: bool,
        has_sub: bool,
    ) -> Result<NodeId> {
        let (mut base, mut sub, mut sup) = (None, None, None);
        let mut seen_pr = false;
        while let Some(c) = self.next_child(name)? {
            match (c.ns, c.local()) {
                (Ns::Math, l) if l == pr => {
                    if self.once_props(&mut seen_pr, pr, name)? {
                        let pr_name = match pr {
                            "sSubPr" => "m:sSubPr",
                            "sSupPr" => "m:sSupPr",
                            _ => "m:sSubSupPr",
                        };
                        self.props(pr_name, &["alnScr"], |_, _| Ok(false))?;
                    }
                },
                (Ns::Math, "e") => {
                    let id = self.arg_id("m:e")?;
                    self.set_once(&mut base, id, "e", name)?;
                },
                (Ns::Math, "sub") if has_sub => {
                    let id = self.arg_id("m:sub")?;
                    self.set_once(&mut sub, id, "sub", name)?;
                },
                (Ns::Math, "sup") if has_sup => {
                    let id = self.arg_id("m:sup")?;
                    self.set_once(&mut sup, id, "sup", name)?;
                },
                _ => self.unknown(&c, name)?,
            }
        }
        let base = self.or_empty(base);
        // `m:sSub`/`m:sSup` explicitly request side scripts. Preserve that
        // effective placement when the base is a naturally movable named
        // operator. Stacked limits arrive through `m:limLow`/`m:limUpp` and
        // are decoded separately below.
        let base = match *self.arena.node(base) {
            CompactNode::Operator { name, .. } => self.arena.push_node(CompactNode::Operator {
                name,
                limits: LimitStyle::SubSup,
            }),
            CompactNode::StructuredOperator { content, form } => {
                let form = match form {
                    OperatorNameForm::Unstarred => OperatorNameForm::Unstarred,
                    OperatorNameForm::Starred(_) => OperatorNameForm::Starred(LimitStyle::SubSup),
                };
                self.arena
                    .push_node(CompactNode::StructuredOperator { content, form })
            },
            _ => base,
        };
        let sub = self.or_empty(sub);
        let sup = self.or_empty(sup);
        Ok(self.arena.push_node(match (has_sub, has_sup) {
            (true, true) => CompactNode::SubSup { base, sub, sup },
            (true, false) => CompactNode::Subscript { base, sub },
            _ => CompactNode::Superscript { base, sup },
        }))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Drop a leading U+FEFF byte-order mark, which XML 1.0 §4.3.3 allows
/// before the document.
fn strip_bom(input: &str) -> &str {
    input.strip_prefix('\u{FEFF}').unwrap_or(input)
}

fn truncate(s: &str) -> String {
    s.chars().take(32).collect()
}

/// `ST_HexColor` RGB value (`RRGGBB`).
fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Decode a Greek letter from the Mathematical Alphanumeric Symbols block
/// (Unicode 15, §22.2: five styled runs of 58 code points each, ordered
/// capitals Α–Ω with ϴ in the gap after Ρ, ∇, small α–ω, then ∂ ϵ ϑ ϰ ϕ ϱ ϖ).
fn unstyled_greek(c: char) -> Option<(char, TextStyle)> {
    const BLOCKS: [(u32, TextStyle); 5] = [
        (0x1D6A8, TextStyle::Bold),
        (0x1D6E2, TextStyle::Italic),
        (0x1D71C, TextStyle::BoldItalic),
        (0x1D756, TextStyle::SansSerifBold),
        (0x1D790, TextStyle::SansSerifBoldItalic),
    ];
    const TAIL: [char; 7] = [
        '\u{2202}', '\u{03F5}', '\u{03D1}', '\u{03F0}', '\u{03D5}', '\u{03F1}', '\u{03D6}',
    ];
    let cp = c as u32;
    let &(start, style) = BLOCKS
        .iter()
        .find(|&&(start, _)| (start..start + 58).contains(&cp))?;
    let i = cp - start;
    let base = match i {
        17 => '\u{03F4}',
        0..=24 => char::from_u32(0x0391 + i)?,
        25 => '\u{2207}',
        26..=50 => char::from_u32(0x03B1 + i - 26)?,
        _ => TAIL[(i - 51) as usize],
    };
    base.is_alphabetic().then_some((base, style))
}

/// Widths, in em, of the Unicode spaces used for math spacing; the same
/// table drives the writer, so explicit spaces round-trip.
pub(crate) const MATH_SPACES: [(char, i32, u32); 7] = [
    ('\u{200A}', 1, 18),  // hair space
    ('\u{2009}', 3, 18),  // thin space
    ('\u{205F}', 4, 18),  // medium mathematical space
    ('\u{2005}', 5, 18),  // four-per-em space (thick math space)
    ('\u{2004}', 6, 18),  // three-per-em space
    ('\u{2002}', 9, 18),  // en space
    ('\u{2003}', 18, 18), // em space
];

fn space_width(c: char) -> Option<Rational> {
    MATH_SPACES
        .iter()
        .find(|&&(ch, _, _)| ch == c)
        .map(|&(_, num, den)| Rational::new(num, den))
}

/// Map OMML `phantPr` flags to a [`PhantomKind`]. Inverse of the writer's
/// `phantom_pr_flags` for the six representable combinations.
fn phantom_kind_from_flags(
    show: bool,
    zero_wid: bool,
    zero_asc: bool,
    zero_desc: bool,
) -> PhantomKind {
    if show {
        // Content visible → \smash family.
        match (zero_asc, zero_desc) {
            (true, false) => PhantomKind::SmashTop,
            (false, true) => PhantomKind::SmashBottom,
            _ => PhantomKind::SmashBoth,
        }
    } else if zero_wid && !zero_asc && !zero_desc {
        PhantomKind::Vertical
    } else if !zero_wid && zero_asc && zero_desc {
        PhantomKind::Horizontal
    } else {
        PhantomKind::Full
    }
}

fn classify_op_char(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '*' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2212}' | '\u{00B1}' => {
            SymbolKind::BinaryOp
        },
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        '|' | '\u{2016}' => SymbolKind::Fence,
        '\u{2211}' | '\u{220F}' | '\u{222B}' => SymbolKind::LargeOp,
        _ => crate::unicode::symbols::operator_glyph_kind(ch).unwrap_or(SymbolKind::Ordinary),
    }
}

/// Combine `m:sty` and `m:scr` into a [`TextStyle`] (§22.1.2.90, §22.1.2.112).
pub(crate) fn combine_omml_style(sty: &str, scr: &str) -> TextStyle {
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
mod tests;
