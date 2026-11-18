//! Record-level decoding of MTEF 3 and MTEF 5 streams into the compact AST.
//!
//! The decoder makes one recursive pass over the byte stream. Every object list
//! (the contents of a LINE, TMPL, PILE, MATRIX or a character's embellishment
//! list) is read into a list of [`Item`]s, which the enclosing record then turns
//! into AST nodes. Characters stay undecided until their line is assembled, so
//! that runs of function letters (`sin`), digits and text become one node each.
//!
//! Hostile-input bounds: every read is bounds-checked; recursion is bounded by
//! [`Config::max_depth`]; every allocation is driven by a record that consumed
//! input, and the number of records and AST nodes is bounded by
//! [`Config::max_nodes`].

use smallvec::SmallVec;

use super::chars::{self, Encoding};
use super::info::{MtefInfo, MtefPlatform, MtefProduct, MtefVersion, OleHeader};
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId, NodeSpan};
use crate::ast::{
    AccentPosition, ColumnAlign, LimitStyle, Rational, StrokeKind, SymbolKind, TextStyle,
};
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, LimitKind, Result, Strictness};

// Record types (MTEFv3.md "Tag byte structure", MTEFv5.md "Record types").
pub(super) const END: u8 = 0;
pub(super) const LINE: u8 = 1;
pub(super) const CHAR: u8 = 2;
pub(super) const TMPL: u8 = 3;
pub(super) const PILE: u8 = 4;
pub(super) const MATRIX: u8 = 5;
pub(super) const EMBELL: u8 = 6;
pub(super) const RULER: u8 = 7;
/// FONT in MTEF 3, FONT_STYLE_DEF in MTEF 5.
pub(super) const FONT: u8 = 8;
pub(super) const SIZE: u8 = 9;
pub(super) const FULL: u8 = 10;
pub(super) const SUBSYM: u8 = 14;
pub(super) const COLOR: u8 = 15;
pub(super) const COLOR_DEF: u8 = 16;
pub(super) const FONT_DEF: u8 = 17;
pub(super) const EQN_PREFS: u8 = 18;
pub(super) const ENCODING_DEF: u8 = 19;
/// Record types from this value up carry their own length (MTEFv5.md).
pub(super) const FUTURE: u8 = 100;

// Option flags shared by both versions.
const OPT_NUDGE: u8 = 0x08;
const OPT_LINE_NULL: u8 = 0x01;
const OPT_LP_RULER: u8 = 0x02;
const OPT_LINE_LSPACE: u8 = 0x04;
// CHAR option flags, MTEF 3.
const V3_CHAR_EMBELL: u8 = 0x02;
// CHAR option flags, MTEF 5.
const V5_CHAR_EMBELL: u8 = 0x01;
const V5_CHAR_FUNC_START: u8 = 0x02;
const V5_CHAR_ENC_CHAR_8: u8 = 0x04;
const V5_CHAR_ENC_CHAR_16: u8 = 0x10;
const V5_CHAR_ENC_NO_MTCODE: u8 = 0x20;
// COLOR_DEF option flags, MTEF 5.
const COLOR_CMYK: u8 = 0x01;
const COLOR_NAME: u8 = 0x04;

/// Length of the `EQNOLEFILEHDR` that starts an OLE `Equation Native` stream.
pub(super) const OLE_HEADER_LEN: usize = 28;
/// The only `EQNOLEFILEHDR` version known to writers (hiword 2, loword 0).
const OLE_HEADER_VERSION: u32 = 0x0002_0000;

/// A decoded CHAR record whose node is built when its line is assembled.
#[derive(Debug, Clone)]
pub(super) struct Glyph {
    pub(super) typeface: i32,
    pub(super) ch: char,
    /// MTEF 5 `mtefOPT_CHAR_FUNC_START`: the character starts a function name.
    pub(super) func_start: bool,
    /// Bold/italic of the explicit font, for a negative typeface.
    pub(super) explicit: Option<FontStyle>,
    pub(super) embells: SmallVec<[(u8, usize); 1]>,
    /// Offset of the CHAR record, for diagnostics.
    pub(super) offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FontStyle {
    pub(super) bold: bool,
    pub(super) italic: bool,
}

/// The sub- and superscript slots of a script template.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScriptSlots {
    pub(super) sub: Option<NodeId>,
    pub(super) sup: Option<NodeId>,
}

/// One entry of a decoded object list.
pub(super) enum Item {
    /// A character, not yet turned into a node.
    Glyph(Glyph),
    /// A finished structure (template, pile, matrix).
    Node(NodeId),
    /// A LINE record: its row, or `None` for a null (placeholder) line.
    Line(Option<NodeId>),
    /// A script template; attaches to the item before it.
    Script(ScriptSlots),
    /// A leading-script template; attaches to the item after it.
    PreScript(ScriptSlots),
    /// An EMBELL record (type, offset).
    Embell(u8, usize),
}

impl Item {
    fn describe(&self) -> &'static str {
        match self {
            Item::Glyph(_) => "a character",
            Item::Node(_) => "a template, pile or matrix",
            Item::Line(_) => "a line",
            Item::Script(_) | Item::PreScript(_) => "a script template",
            Item::Embell(..) => "an embellishment",
        }
    }
}

/// How a character takes part in its line.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Text,
    Function,
    Number,
    Ident,
    Styled(TextStyle),
    Symbol(SymbolKind),
    Space(Rational),
    Marker,
}

pub(super) struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    /// End of the MTEF data (the OLE header can declare a shorter length).
    end: usize,
    pub(super) v5: bool,
    mac: bool,
    strict: bool,
    max_depth: usize,
    max_nodes: Option<usize>,
    depth: usize,
    /// Height of every node in the arena, indexed by node id.
    heights: Vec<u8>,
    pub(super) arena: AstArena,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) info: Option<MtefInfo>,
    /// MTEF 3 FONT records by typeface number.
    v3_fonts: Box<[Option<(Encoding, FontStyle)>; 256]>,
    /// MTEF 5 ENCODING_DEF records (index 5 onwards).
    encodings: Vec<Encoding>,
    /// MTEF 5 FONT_DEF records: encoding index of each font.
    font_defs: Vec<u16>,
    /// MTEF 5 FONT_STYLE_DEF records: (font_def index, style).
    font_styles: Vec<(u16, FontStyle)>,
    /// MTEF 5 EQN_PREFS style definitions: font_def index per typeface style.
    style_fonts: Vec<u16>,
    /// MTEF 5 COLOR_DEF records: whether each colour is black.
    color_is_black: Vec<bool>,
    color_noted: bool,
}

impl<'a> Decoder<'a> {
    pub(super) fn new(data: &'a [u8], config: &Config) -> Self {
        Self {
            data,
            pos: 0,
            end: data.len(),
            v5: false,
            mac: false,
            strict: config.strictness == Strictness::Strict,
            max_depth: config.effective_max_depth(),
            max_nodes: config.max_nodes,
            depth: 0,
            heights: Vec::new(),
            arena: AstArena::with_capacity(data.len().min(1 << 16) / 2, 0, 0),
            diagnostics: Vec::new(),
            info: None,
            v3_fonts: Box::new([None; 256]),
            encodings: Vec::new(),
            font_defs: Vec::new(),
            font_styles: Vec::new(),
            style_fonts: Vec::new(),
            color_is_black: Vec::new(),
            color_noted: false,
        }
    }

    // ── Diagnostics ────────────────────────────────────────────────────

    pub(super) fn error_at(&self, offset: usize, message: impl AsRef<str>) -> Error {
        Error::MtefParse {
            message: format!("{} (at byte {offset})", message.as_ref()),
        }
    }

    fn diag(&mut self, kind: DiagnosticKind, message: String) {
        self.diagnostics.push(Diagnostic {
            kind,
            message,
            span: None,
        });
    }

    pub(super) fn warn(&mut self, offset: usize, message: impl AsRef<str>) {
        let message = format!("{} (at byte {offset})", message.as_ref());
        self.diag(DiagnosticKind::Warning, message);
    }

    fn note(&mut self, message: impl Into<String>) {
        self.diag(DiagnosticKind::Info, message.into());
    }

    /// A problem Strict mode refuses and Lenient mode reports and recovers from.
    pub(super) fn recover(&mut self, offset: usize, message: impl AsRef<str>) -> Result<()> {
        if self.strict {
            Err(self.error_at(offset, message))
        } else {
            self.warn(offset, message);
            Ok(())
        }
    }

    // ── Limits ─────────────────────────────────────────────────────────

    /// Bound the recursion over nested object lists. Lists nest at least as
    /// deeply as the nodes built from them, so `max_depth` bounds both.
    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: self.max_depth,
            });
        }
        Ok(())
    }

    /// Push an AST node, enforcing [`Config::max_nodes`] and
    /// [`Config::max_depth`]: the node's height (0 for a leaf) is the depth of
    /// its deepest descendant when it is a top-level node, and every node is
    /// built before its parent, so checking heights here bounds the tree
    /// however it is nested (script chains, embellishments, templates).
    pub(super) fn node(&mut self, node: CompactNode) -> Result<NodeId> {
        if let Some(limit) = self.max_nodes
            && self.arena.node_count() >= limit
        {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Nodes,
                limit,
            });
        }
        let height = crate::ast::compact_children(&self.arena, &node)
            .iter()
            .map(|child| self.heights[child.index()] + 1)
            .max()
            .unwrap_or(0);
        if usize::from(height) > self.max_depth {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: self.max_depth,
            });
        }
        self.heights.push(height);
        Ok(self.arena.push_node(node))
    }

    /// Store a node's children, refusing more than a node can hold.
    pub(super) fn children(&mut self, ids: &[NodeId]) -> Result<NodeSpan> {
        self.arena.push_children(ids)
    }

    pub(super) fn str_node(
        &mut self,
        s: &str,
        make: impl FnOnce(crate::ast::compact::StrId) -> CompactNode,
    ) -> Result<NodeId> {
        let sid = self.arena.push_str(s);
        self.node(make(sid))
    }

    pub(super) fn char_node(
        &mut self,
        ch: char,
        make: impl FnOnce(crate::ast::compact::StrId) -> CompactNode,
    ) -> Result<NodeId> {
        let mut buf = [0u8; 4];
        self.str_node(ch.encode_utf8(&mut buf), make)
    }

    pub(super) fn empty_row(&mut self) -> Result<NodeId> {
        self.node(CompactNode::Row(NodeSpan::EMPTY))
    }

    /// One node for a list of nodes: the node itself, or a row.
    pub(super) fn wrap(&mut self, ids: &[NodeId]) -> Result<NodeId> {
        match ids {
            [] => self.empty_row(),
            [id] => Ok(*id),
            _ => {
                let span = self.children(ids)?;
                self.node(CompactNode::Row(span))
            },
        }
    }

    /// `id` as a Row node (array cells must be rows).
    fn as_row(&mut self, id: NodeId) -> Result<NodeId> {
        if matches!(self.arena.node(id), CompactNode::Row(_)) {
            Ok(id)
        } else {
            let span = self.children(&[id])?;
            self.node(CompactNode::Row(span))
        }
    }

    /// The children a node contributes to a surrounding list.
    pub(super) fn children_of(&self, id: NodeId) -> SmallVec<[NodeId; 8]> {
        match *self.arena.node(id) {
            CompactNode::Row(span) => self.arena.children(span).into(),
            _ => smallvec::smallvec![id],
        }
    }

    pub(super) fn is_empty_row(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    // ── Primitive reads ────────────────────────────────────────────────

    fn truncated(&self) -> Error {
        self.error_at(self.pos, "unexpected end of MTEF data")
    }

    fn u8(&mut self) -> Result<u8> {
        if self.pos >= self.end {
            return Err(self.truncated());
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16> {
        let lo = self.u8()?;
        let hi = self.u8()?;
        Ok(u16::from_le_bytes([lo, hi]))
    }

    fn u32(&mut self) -> Result<u32> {
        let lo = self.u16()?;
        let hi = self.u16()?;
        Ok(u32::from(lo) | (u32::from(hi) << 16))
    }

    /// MTEF 5 unsigned integer: one byte below 255, else 255 and 16 bits.
    fn uint(&mut self) -> Result<u16> {
        match self.u8()? {
            255 => self.u16(),
            b => Ok(u16::from(b)),
        }
    }

    /// MTEF 5 signed integer: value + 128 in one byte, else 255 and the value
    /// + 32768 in 16 bits.
    fn sint(&mut self) -> Result<i32> {
        match self.u8()? {
            255 => Ok(i32::from(self.u16()?) - 32768),
            b => Ok(i32::from(b) - 128),
        }
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        if self.end - self.pos < n {
            self.pos = self.end;
            return Err(self.truncated());
        }
        self.pos += n;
        Ok(())
    }

    /// A null-terminated string, without its terminator.
    fn cstr(&mut self) -> Result<&'a [u8]> {
        let data: &'a [u8] = self.data;
        let rest = &data[self.pos..self.end];
        let Some(len) = rest.iter().position(|&b| b == 0) else {
            self.pos = self.end;
            return Err(self.error_at(self.end, "unterminated string in MTEF data"));
        };
        self.pos += len + 1;
        Ok(&rest[..len])
    }

    /// Nudge offsets: two biased bytes, or 128, 128 and two 16-bit values.
    fn nudge(&mut self) -> Result<()> {
        let dx = self.u8()?;
        let dy = self.u8()?;
        if dx == 128 && dy == 128 {
            self.skip(4)?;
        }
        Ok(())
    }

    /// The record type and option flags. MTEF 3 packs the options in the high
    /// nibble of the tag; MTEF 5 writes an options byte after the record types
    /// that have one (MTEFv5.md "Record details").
    fn read_tag(&mut self) -> Result<(u8, u8)> {
        let b = self.u8()?;
        if !self.v5 {
            return Ok((b & 0x0F, b >> 4));
        }
        let has_options = matches!(
            b,
            LINE | CHAR | TMPL | PILE | MATRIX | EMBELL | COLOR_DEF | EQN_PREFS
        );
        let options = if has_options { self.u8()? } else { 0 };
        Ok((b, options))
    }

    // ── Headers ────────────────────────────────────────────────────────

    /// Decode the whole input: optional OLE header, MTEF header and body.
    pub(super) fn run(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        let ole_header = self.ole_header()?;
        self.mtef_header(ole_header)?;
        let items = self.parse_object_list(true)?;
        self.check_trailing()?;
        self.assemble_line(items)
    }

    /// Detect and validate an `EQNOLEFILEHDR` (cbHdr = 28 as a 16-bit value;
    /// no MTEF version byte is 28, so the two cannot be confused).
    fn ole_header(&mut self) -> Result<Option<OleHeader>> {
        if self.data.len() < 2 || self.data[0] != OLE_HEADER_LEN as u8 || self.data[1] != 0 {
            return Ok(None);
        }
        if self.data.len() < OLE_HEADER_LEN {
            return Err(self.error_at(
                self.data.len(),
                "truncated Equation Native header: it needs 28 bytes",
            ));
        }
        self.pos = 2;
        let version = self.u32()?;
        let clipboard_format = self.u16()?;
        let object_size = self.u32()?;
        self.pos = OLE_HEADER_LEN;
        if version != OLE_HEADER_VERSION {
            self.recover(
                2,
                format!(
                    "unknown Equation Native header version {version:#010x}, expected 0x00020000"
                ),
            )?;
        }
        let available = self.data.len() - OLE_HEADER_LEN;
        let declared = usize::try_from(object_size).unwrap_or(usize::MAX);
        if declared > available {
            return Err(self.error_at(
                8,
                format!(
                    "Equation Native header declares {object_size} bytes of MTEF data but only {available} follow"
                ),
            ));
        }
        if declared < available {
            self.note(format!(
                "ignored {} bytes after the {object_size} bytes of MTEF data the Equation Native header declares",
                available - declared
            ));
        }
        self.end = OLE_HEADER_LEN + declared;
        Ok(Some(OleHeader {
            version,
            clipboard_format,
            object_size,
        }))
    }

    fn mtef_header(&mut self, ole_header: Option<OleHeader>) -> Result<()> {
        let start = self.pos;
        let version = self.u8()?;
        let version = match version {
            3 => MtefVersion::V3,
            5 => MtefVersion::V5,
            1 | 101 | 2 | 4 => {
                return Err(self.error_at(
                    start,
                    format!("MTEF version {version} is not supported (only versions 3 and 5 are)"),
                ));
            },
            _ => {
                return Err(self.error_at(
                    start,
                    format!("not MTEF data: unknown MTEF version byte {version}"),
                ));
            },
        };
        let platform = self.u8()?;
        let product = self.u8()?;
        let product_version = self.u8()?;
        let product_subversion = self.u8()?;
        let (application_key, inline) = if version == MtefVersion::V5 {
            let key = String::from_utf8_lossy(self.cstr()?).into_owned();
            let options_at = self.pos;
            let options = self.u8()?;
            if options & !0x01 != 0 {
                self.warn(
                    options_at,
                    format!(
                        "MTEF equation options {options:#04x} set undefined bits; they were ignored"
                    ),
                );
            }
            (Some(key), Some(options & 0x01 != 0))
        } else {
            (None, None)
        };
        self.v5 = version == MtefVersion::V5;
        self.mac = platform == 0;
        self.info = Some(MtefInfo {
            version,
            platform: MtefPlatform::from_byte(platform),
            product: MtefProduct::from_byte(product),
            product_version,
            product_subversion,
            application_key,
            inline,
            ole_header,
        });
        Ok(())
    }

    /// Bytes after the final END: zero padding is common and harmless; anything
    /// else is not part of the equation.
    fn check_trailing(&mut self) -> Result<()> {
        let rest = &self.data[self.pos..self.end];
        if rest.iter().any(|&b| b != 0) {
            let (pos, len) = (self.pos, rest.len());
            self.recover(
                pos,
                format!("{len} bytes of data follow the end of the equation"),
            )?;
        }
        Ok(())
    }

    // ── Object lists ───────────────────────────────────────────────────

    /// Read records up to and including the END that closes the list.
    pub(super) fn parse_list(&mut self) -> Result<Vec<Item>> {
        self.parse_object_list(false)
    }

    /// Read an object list. In Lenient mode the equation's own list (`top`)
    /// may end with the data instead of an END record once its LINE or PILE
    /// has been read completely.
    fn parse_object_list(&mut self, top: bool) -> Result<Vec<Item>> {
        self.enter()?;
        let mut items = Vec::new();
        loop {
            if self.pos >= self.end {
                if top
                    && items
                        .iter()
                        .any(|item| matches!(item, Item::Line(_) | Item::Node(_)))
                {
                    // Real files always close the equation; Lenient mode
                    // accepts the omission because nothing can follow.
                    self.recover(
                        self.pos,
                        "MTEF data ends without the END record that closes the equation",
                    )?;
                    break;
                }
                return Err(self.error_at(
                    self.pos,
                    "unexpected end of MTEF data: an object list is missing its END record",
                ));
            }
            let start = self.pos;
            let (rtype, options) = self.read_tag()?;
            if rtype == END {
                break;
            }
            if items.len() >= AstArena::MAX_CHILDREN {
                return Err(Error::LimitExceeded {
                    kind: LimitKind::ListLength,
                    limit: AstArena::MAX_CHILDREN,
                });
            }
            self.parse_record(start, rtype, options, &mut items)?;
        }
        self.depth -= 1;
        Ok(items)
    }

    fn parse_record(
        &mut self,
        start: usize,
        rtype: u8,
        options: u8,
        items: &mut Vec<Item>,
    ) -> Result<()> {
        match rtype {
            LINE => {
                let line = self.parse_line(options)?;
                items.push(Item::Line(line));
            },
            CHAR => {
                let glyph = self.parse_char(start, options)?;
                items.push(Item::Glyph(glyph));
            },
            TMPL => self.parse_template(start, options, items)?,
            PILE => {
                let id = self.parse_pile(options)?;
                items.push(Item::Node(id));
            },
            MATRIX => {
                let id = self.parse_matrix(start, options)?;
                items.push(Item::Node(id));
            },
            EMBELL => {
                if options & OPT_NUDGE != 0 {
                    self.nudge()?;
                }
                let kind = self.u8()?;
                if !self.v5 {
                    // Equation Editor 3 writes a zero byte after the type; the
                    // MTEF 3 description omits it, but every reader of real
                    // files has to consume it to stay in step.
                    self.u8()?;
                }
                items.push(Item::Embell(kind, start));
            },
            RULER => self.ruler_body()?,
            FONT if self.v5 => self.font_style_def()?,
            FONT => self.v3_font()?,
            SIZE => self.size_record()?,
            FULL..=SUBSYM => {},
            COLOR if self.v5 => self.color()?,
            COLOR_DEF if self.v5 => self.color_def(options)?,
            FONT_DEF if self.v5 => self.font_def()?,
            EQN_PREFS if self.v5 => self.eqn_prefs(start)?,
            ENCODING_DEF if self.v5 => {
                let name = self.cstr()?;
                self.encodings.push(Encoding::from_encoding_name(name));
            },
            t if self.v5 && t >= FUTURE => {
                let len = self.uint()?;
                self.skip(usize::from(len))?;
                self.note(format!(
                    "skipped MTEF future-expansion record type {t} ({len} bytes) at byte {start}"
                ));
            },
            t => {
                let version = if self.v5 { 5 } else { 3 };
                return Err(self.error_at(start, format!("unknown MTEF {version} record type {t}")));
            },
        }
        Ok(())
    }

    /// Report an item that cannot appear in the given list; Lenient mode drops it.
    fn unexpected(&mut self, item: &Item, context: &str, offset: usize) -> Result<()> {
        let what = item.describe();
        self.recover(
            offset,
            format!("{what} cannot appear directly in {context}; it was dropped"),
        )
    }

    // ── Structure records ──────────────────────────────────────────────

    fn parse_line(&mut self, options: u8) -> Result<Option<NodeId>> {
        if options & OPT_NUDGE != 0 {
            self.nudge()?;
        }
        if options & OPT_LINE_LSPACE != 0 {
            self.u16()?;
        }
        if options & OPT_LP_RULER != 0 {
            self.ruler_record()?;
        }
        if options & OPT_LINE_NULL != 0 {
            return Ok(None);
        }
        let items = self.parse_list()?;
        let mut ids = self.assemble_line(items)?;
        self.coalesce_compound_limit_names(&mut ids)?;
        let span = self.children(&ids)?;
        Ok(Some(self.node(CompactNode::Row(span))?))
    }

    /// MTEF stores the thin separator between a compound limit name as a
    /// separate space record. Reassemble the four native compound names so a
    /// round trip retains one movable operator and its limit template.
    fn coalesce_compound_limit_names(&mut self, ids: &mut SmallVec<[NodeId; 8]>) -> Result<()> {
        let mut index = 0;
        while index + 2 < ids.len() {
            let (left, width, right) = match (
                *self.arena.node(ids[index]),
                *self.arena.node(ids[index + 1]),
                *self.arena.node(ids[index + 2]),
            ) {
                (
                    CompactNode::Operator { name: left, .. },
                    CompactNode::Space(width),
                    CompactNode::Operator { name: right, .. },
                ) => (left, (width.num, width.den), right),
                _ => {
                    index += 1;
                    continue;
                },
            };
            if width != (1, 4) && width != (3, 18) {
                index += 1;
                continue;
            }
            let left_text = self.arena.str_val(left);
            let right_text = self.arena.str_val(right);
            let Some(combined) = (match (left_text, right_text) {
                ("lim", "sup") => Some("lim\u{2009}sup"),
                ("lim", "inf") => Some("lim\u{2009}inf"),
                ("inj", "lim") => Some("inj\u{2009}lim"),
                ("proj", "lim") => Some("proj\u{2009}lim"),
                _ => None,
            }) else {
                index += 1;
                continue;
            };
            let sid = self.arena.push_str(combined);
            let operator = self.node(CompactNode::Operator {
                name: sid,
                limits: LimitStyle::Default,
            })?;
            ids[index] = operator;
            ids.remove(index + 2);
            ids.remove(index + 1);
        }
        Ok(())
    }

    /// A RULER record (with its tag) following a LINE or PILE.
    fn ruler_record(&mut self) -> Result<()> {
        let start = self.pos;
        let tag = self.u8()?;
        let rtype = if self.v5 { tag } else { tag & 0x0F };
        if rtype != RULER {
            return Err(self.error_at(
                start,
                format!("expected a RULER record, found record type {rtype}"),
            ));
        }
        self.ruler_body()
    }

    /// Tab stops: a count, then a type byte and a 16-bit offset for each.
    fn ruler_body(&mut self) -> Result<()> {
        let stops = self.u8()?;
        self.skip(usize::from(stops) * 3)
    }

    fn parse_char(&mut self, start: usize, options: u8) -> Result<Glyph> {
        if options & OPT_NUDGE != 0 {
            self.nudge()?;
        }
        let (typeface, ch, has_embells, func_start) = if self.v5 {
            let typeface = self.sint()?;
            let mtcode = if options & V5_CHAR_ENC_NO_MTCODE == 0 {
                Some(self.u16()?)
            } else {
                None
            };
            if options & V5_CHAR_ENC_CHAR_8 != 0 && options & V5_CHAR_ENC_CHAR_16 != 0 {
                self.recover(
                    start,
                    "CHAR record sets both the 8-bit and the 16-bit font position flags",
                )?;
            }
            let font_position = if options & V5_CHAR_ENC_CHAR_8 != 0 {
                Some(u16::from(self.u8()?))
            } else if options & V5_CHAR_ENC_CHAR_16 != 0 {
                Some(self.u16()?)
            } else {
                None
            };
            let ch = self.v5_char(start, typeface, mtcode, font_position)?;
            (
                typeface,
                ch,
                options & V5_CHAR_EMBELL != 0,
                options & V5_CHAR_FUNC_START != 0,
            )
        } else {
            let typeface = i32::from(self.u8()?) - 128;
            let code = self.u16()?;
            let ch = self.v3_char(start, typeface, code)?;
            (typeface, ch, options & V3_CHAR_EMBELL != 0, false)
        };
        let explicit = if typeface < 0 {
            Some(self.explicit_font_style(start, typeface)?)
        } else {
            if !matches!(typeface, 1..=12 | 22..=24) {
                self.recover(start, format!("unknown MTEF typeface style {typeface}"))?;
            }
            None
        };
        let mut ch = chars::normalize_code_point(ch);
        if ch.is_control() && !ch.is_whitespace() {
            self.recover(
                start,
                format!(
                    "CHAR record holds the control character U+{:04X}",
                    u32::from(ch)
                ),
            )?;
            ch = char::REPLACEMENT_CHARACTER;
        }
        if typeface == chars::FN_SPACE
            && chars::space_width(u32::from(ch)).is_none()
            && !chars::is_marker(u32::from(ch))
            && !ch.is_whitespace()
        {
            self.warn(
                start,
                format!(
                    "unknown MathType space character U+{:04X}; a thin space was used",
                    u32::from(ch)
                ),
            );
        }
        let mut embells = SmallVec::new();
        if has_embells {
            let items = self.parse_list()?;
            for item in items {
                match item {
                    Item::Embell(kind, offset) => embells.push((kind, offset)),
                    other => self.unexpected(&other, "an embellishment list", start)?,
                }
            }
        }
        Ok(Glyph {
            typeface,
            ch,
            func_start,
            explicit,
            embells,
            offset: start,
        })
    }

    /// MTEF 5 character value: the MTCode value when present, else the font
    /// position in the font's encoding (MTEFv5.md "CHAR record").
    fn v5_char(
        &mut self,
        start: usize,
        typeface: i32,
        mtcode: Option<u16>,
        font_position: Option<u16>,
    ) -> Result<char> {
        if let Some(code) = mtcode {
            return match char::from_u32(u32::from(code)) {
                Some(ch) => Ok(ch),
                None => {
                    self.recover(
                        start,
                        format!("CHAR record holds the invalid MTCode value {code:#06x}"),
                    )?;
                    Ok(char::REPLACEMENT_CHARACTER)
                },
            };
        }
        let Some(code) = font_position else {
            self.recover(
                start,
                "CHAR record has neither an MTCode value nor a font position",
            )?;
            return Ok(char::REPLACEMENT_CHARACTER);
        };
        let encoding = self.v5_encoding(typeface);
        match chars::decode_font_position(code, encoding) {
            Some(ch) => Ok(ch),
            None => {
                self.recover(
                    start,
                    format!("font position {code:#04x} has no known Unicode equivalent in its font encoding"),
                )?;
                Ok(char::REPLACEMENT_CHARACTER)
            },
        }
    }

    fn v5_encoding(&self, typeface: i32) -> Encoding {
        let font_def = if typeface < 0 {
            let index = usize::try_from(-typeface).unwrap_or(0);
            self.font_styles
                .get(index.wrapping_sub(1))
                .map(|&(font, _)| font)
        } else {
            let index = usize::try_from(typeface).unwrap_or(0);
            self.style_fonts.get(index.wrapping_sub(1)).copied()
        };
        let encoding_index = font_def
            .and_then(|font| self.font_defs.get(usize::from(font).wrapping_sub(1)))
            .copied();
        match encoding_index {
            Some(1) => Encoding::Unicode,
            Some(3) => Encoding::Symbol,
            Some(4) => Encoding::MtExtra,
            Some(n) if n >= 5 => self
                .encodings
                .get(usize::from(n) - 5)
                .copied()
                .unwrap_or(Encoding::Other),
            Some(_) => Encoding::Other,
            // No font definition: fall back on what the style implies.
            None => match typeface {
                chars::FN_SYMBOL | chars::FN_LCGREEK | chars::FN_UCGREEK => Encoding::Symbol,
                chars::FN_MTEXTRA => Encoding::MtExtra,
                _ => Encoding::Other,
            },
        }
    }

    /// MTEF 3 character value. Equation Editor writes Unicode for text fonts
    /// and a font index for symbolic fonts (MTEFv3.md "CHAR record"); the
    /// Windows version writes Unicode for the Greek and Symbol styles too, so
    /// only codes that cannot be Unicode in those styles are re-mapped.
    fn v3_char(&mut self, start: usize, typeface: i32, code: u16) -> Result<char> {
        let symbolic = match typeface {
            chars::FN_LCGREEK | chars::FN_UCGREEK | chars::FN_SYMBOL => Encoding::Symbol,
            chars::FN_MTEXTRA => Encoding::MtExtra,
            t if t < 0 => usize::try_from(-t)
                .ok()
                .and_then(|i| self.v3_fonts.get(i).copied().flatten())
                .map_or(Encoding::Other, |(encoding, _)| encoding),
            _ => Encoding::Other,
        };
        let ascii_letter = u8::try_from(code).is_ok_and(|b| b.is_ascii_alphabetic());
        let remap = match symbolic {
            Encoding::Symbol => {
                (ascii_letter && matches!(typeface, chars::FN_LCGREEK | chars::FN_UCGREEK))
                    || (self.mac && code < 0x100)
            },
            Encoding::MtExtra => self.mac && code < 0x100,
            _ => false,
        };
        if remap && let Some(ch) = chars::decode_font_position(code, symbolic) {
            return Ok(ch);
        }
        match char::from_u32(u32::from(code)) {
            Some(ch) => Ok(ch),
            None => {
                self.recover(
                    start,
                    format!("CHAR record holds the invalid character value {code:#06x}"),
                )?;
                Ok(char::REPLACEMENT_CHARACTER)
            },
        }
    }

    fn explicit_font_style(&mut self, start: usize, typeface: i32) -> Result<FontStyle> {
        let index = usize::try_from(-typeface).unwrap_or(usize::MAX);
        let style = if self.v5 {
            self.font_styles
                .get(index.wrapping_sub(1))
                .map(|&(_, style)| style)
        } else {
            self.v3_fonts
                .get(index)
                .copied()
                .flatten()
                .map(|(_, style)| style)
        };
        match style {
            Some(style) => Ok(style),
            None => {
                self.recover(
                    start,
                    format!("CHAR record refers to undefined font {index}"),
                )?;
                Ok(FontStyle {
                    bold: false,
                    italic: false,
                })
            },
        }
    }

    fn parse_template(&mut self, start: usize, options: u8, items: &mut Vec<Item>) -> Result<()> {
        if options & OPT_NUDGE != 0 {
            self.nudge()?;
        }
        let selector = self.u8()?;
        let first = self.u8()?;
        // MTEF 5 writes a two-byte variation when the high bit is set.
        let variation = if self.v5 && first & 0x80 != 0 {
            u16::from(first & 0x7F) | (u16::from(self.u8()?) << 8)
        } else {
            u16::from(first)
        };
        let _template_options = self.u8()?;
        let list = self.parse_list()?;
        let mut slots = Vec::new();
        let mut glyphs = Vec::new();
        for item in list {
            match item {
                Item::Line(line) => slots.push(line),
                Item::Node(id) => slots.push(Some(id)),
                Item::Glyph(glyph) => glyphs.push(glyph),
                other => self.unexpected(&other, "a template", start)?,
            }
        }
        self.build_template(start, selector, variation, slots, glyphs, items)
    }

    fn parse_pile(&mut self, options: u8) -> Result<NodeId> {
        let start = self.pos;
        if options & OPT_NUDGE != 0 {
            self.nudge()?;
        }
        let halign = self.u8()?;
        let _valign = self.u8()?;
        if options & OPT_LP_RULER != 0 {
            self.ruler_record()?;
        }
        let list = self.parse_list()?;
        let mut rows: SmallVec<[NodeId; 8]> = SmallVec::new();
        for item in list {
            match item {
                Item::Line(Some(id)) | Item::Node(id) => {
                    let row = self.as_row(id)?;
                    rows.push(row);
                },
                Item::Line(None) => rows.push(self.empty_row()?),
                other => self.unexpected(&other, "a pile", start)?,
            }
        }
        let align = self.node(CompactNode::ColAlign(column_align(halign)))?;
        let col_aligns = self.children(&[align])?;
        let cells = self.children(&rows)?;
        self.node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: 1,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        })
    }

    fn parse_matrix(&mut self, start: usize, options: u8) -> Result<NodeId> {
        if options & OPT_NUDGE != 0 {
            self.nudge()?;
        }
        let _valign = self.u8()?;
        let h_just = self.u8()?;
        let _v_just = self.u8()?;
        let rows = usize::from(self.u8()?);
        let cols = usize::from(self.u8()?);
        // Partition line styles: two bits per line, one more line than rows
        // (columns), low bits first, padded to whole bytes.
        let row_seps = self.partition_lines(rows + 1)?;
        let col_seps = self.partition_lines(cols + 1)?;
        let list = self.parse_list()?;
        // Cells are only allocated for LINE records actually read, so the
        // declared size cannot make the reader allocate more than the input.
        let mut cells: Vec<NodeId> = Vec::with_capacity(list.len());
        for item in list {
            match item {
                // MathType also writes a PILE directly as a matrix element.
                Item::Line(Some(id)) | Item::Node(id) => cells.push(id),
                Item::Line(None) => cells.push(self.empty_row()?),
                other => self.unexpected(&other, "a matrix", start)?,
            }
        }
        let expected = rows * cols;
        if cells.len() != expected {
            self.recover(
                start,
                format!(
                    "matrix declares {rows} x {cols} cells but holds {} lines",
                    cells.len()
                ),
            )?;
        }
        // Never more columns than cells, so that neither the column
        // alignments nor the padding below can outgrow the input.
        let num_cols = cols.min(cells.len()).max(1);
        // Lenient mode: complete the last row only.
        while !cells.len().is_multiple_of(num_cols) {
            cells.push(self.empty_row()?);
        }
        for cell in &mut cells {
            *cell = self.as_row(*cell)?;
        }
        let align = column_align(h_just);
        let mut aligns: SmallVec<[NodeId; 8]> = SmallVec::new();
        for _ in 0..num_cols {
            aligns.push(self.node(CompactNode::ColAlign(align))?);
        }
        let col_aligns = self.children(&aligns)?;
        let cells = self.children(&cells)?;
        self.node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: u16::try_from(num_cols).unwrap_or(u16::MAX),
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps,
            row_seps,
        })
    }

    /// Read `lines` two-bit partition styles; bit i of the result is set when
    /// line i is drawn (dashed and dotted lines become solid rules).
    /// The AST records rules before the first 32 rows or columns only; a
    /// drawn line beyond them is reported and dropped.
    fn partition_lines(&mut self, lines: usize) -> Result<u32> {
        let start = self.pos;
        let bytes = lines.div_ceil(4);
        let mut mask = 0u32;
        let mut dropped = 0usize;
        for byte_index in 0..bytes {
            let b = self.u8()?;
            for k in 0..4 {
                let line = byte_index * 4 + k;
                if line < lines && (b >> (2 * k)) & 0x3 != 0 {
                    if line < 32 {
                        mask |= 1 << line;
                    } else {
                        dropped += 1;
                    }
                }
            }
        }
        if dropped > 0 {
            self.warn(
                start,
                format!(
                    "{dropped} matrix partition line(s) after the 32nd row or column were dropped"
                ),
            );
        }
        Ok(mask)
    }

    // ── Non-structural records ─────────────────────────────────────────

    fn v3_font(&mut self) -> Result<()> {
        let typeface = self.u8()?;
        let style = self.u8()?;
        let name = self.cstr()?;
        // MTEF 3 font style: 1 italic, 2 bold.
        let style = FontStyle {
            italic: style & 0x01 != 0,
            bold: style & 0x02 != 0,
        };
        self.v3_fonts[usize::from(typeface)] = Some((Encoding::from_font_name(name), style));
        Ok(())
    }

    fn font_style_def(&mut self) -> Result<()> {
        let font = self.uint()?;
        let style = self.u8()?;
        // MTEF 5 character style: bit 0 bold, bit 1 italic.
        self.font_styles.push((
            font,
            FontStyle {
                bold: style & 0x01 != 0,
                italic: style & 0x02 != 0,
            },
        ));
        Ok(())
    }

    fn font_def(&mut self) -> Result<()> {
        let encoding = self.uint()?;
        self.cstr()?;
        self.font_defs.push(encoding);
        Ok(())
    }

    /// SIZE: an explicit point size (101), a large delta (100) or a typesize
    /// with a one-byte delta.
    fn size_record(&mut self) -> Result<()> {
        match self.u8()? {
            101 => self.skip(2),
            100 => self.skip(3),
            _ => self.skip(1),
        }
    }

    fn color(&mut self) -> Result<()> {
        let index = usize::from(self.uint()?);
        let black = index == 0 || self.color_is_black.get(index - 1).copied().unwrap_or(true);
        if !black && !self.color_noted {
            self.color_noted = true;
            self.note("MTEF colour records are not represented in the AST; the equation was read without colour");
        }
        Ok(())
    }

    fn color_def(&mut self, options: u8) -> Result<()> {
        let components = if options & COLOR_CMYK != 0 { 4 } else { 3 };
        let mut values = [0u16; 4];
        for value in values.iter_mut().take(components) {
            *value = self.u16()?;
        }
        if options & COLOR_NAME != 0 {
            self.cstr()?;
        }
        let black = if components == 4 {
            values[3] >= 1000 || values[..3].iter().all(|&v| v >= 1000)
        } else {
            values[..3].iter().all(|&v| v == 0)
        };
        self.color_is_black.push(black);
        Ok(())
    }

    /// EQN_PREFS: options, the sizes and spacing dimension arrays, then the
    /// style definitions (MTEFv5.md "EQN_PREFS records", "Dimension arrays").
    fn eqn_prefs(&mut self, start: usize) -> Result<()> {
        self.dimension_array(start)?;
        self.dimension_array(start)?;
        let count = self.u8()?;
        self.style_fonts.clear();
        for _ in 0..count {
            let font = self.uint()?;
            if font != 0 {
                self.u8()?;
            }
            self.style_fonts.push(font);
        }
        Ok(())
    }

    /// A count of dimensions followed by a nibble stream: per dimension a unit
    /// nibble, value nibbles and a terminating 0xF.
    fn dimension_array(&mut self, start: usize) -> Result<()> {
        let mut remaining = self.u8()?;
        let mut in_value = false;
        while remaining > 0 {
            let b = self.u8()?;
            for nibble in [b >> 4, b & 0x0F] {
                if remaining == 0 {
                    break;
                }
                if !in_value {
                    if nibble > 4 {
                        self.recover(
                            start,
                            format!("invalid unit {nibble} in an EQN_PREFS dimension"),
                        )?;
                    }
                    in_value = true;
                } else if nibble == 0x0F {
                    in_value = false;
                    remaining -= 1;
                } else if nibble > 0x0B {
                    self.recover(
                        start,
                        format!("invalid digit {nibble:#x} in an EQN_PREFS dimension"),
                    )?;
                }
            }
        }
        Ok(())
    }

    // ── Line assembly ──────────────────────────────────────────────────

    /// Turn the items of a line into nodes: group characters into function
    /// names, numbers and text, attach scripts to their bases and apply
    /// embellishments.
    pub(super) fn assemble_line(&mut self, items: Vec<Item>) -> Result<SmallVec<[NodeId; 8]>> {
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut pending: Option<ScriptSlots> = None;
        let mut items = items.into_iter().peekable();
        while let Some(item) = items.next() {
            match item {
                Item::Glyph(glyph) => {
                    let class = classify(&glyph);
                    if chars::is_private_use(glyph.ch)
                        && !matches!(class, Class::Space(_) | Class::Marker)
                    {
                        self.warn(
                            glyph.offset,
                            format!(
                                "MathType private-use character U+{:04X} has no Unicode equivalent; it was kept as is",
                                u32::from(glyph.ch)
                            ),
                        );
                    }
                    if !glyph.embells.is_empty() {
                        let mut id = self.glyph_node(&glyph, class)?;
                        for &(kind, offset) in &glyph.embells {
                            id = self.embellish(id, kind, offset)?;
                        }
                        self.emit(&mut out, &mut pending, id)?;
                        continue;
                    }
                    let id = match class {
                        Class::Marker => continue,
                        Class::Text => {
                            let mut text = String::new();
                            text.push(glyph.ch);
                            while let Some(Item::Glyph(next)) = items.peek()
                                && next.embells.is_empty()
                                && classify(next) == Class::Text
                            {
                                text.push(next.ch);
                                items.next();
                            }
                            self.str_node(&text, |s| CompactNode::Text(TextStyle::Normal, s))?
                        },
                        Class::Function => {
                            let mut name = String::new();
                            name.push(glyph.ch);
                            while let Some(Item::Glyph(next)) = items.peek()
                                && next.embells.is_empty()
                                && classify(next) == Class::Function
                                && !next.func_start
                            {
                                name.push(next.ch);
                                items.next();
                            }
                            self.str_node(&name, |s| CompactNode::Operator {
                                name: s,
                                // MTEF records the function class but not an
                                // explicit movable/side limit modifier.  Use
                                // each native command's TeX default instead
                                // of treating every function as a limit.
                                limits: if crate::unicode::symbols::operator_has_limits(&name) {
                                    LimitStyle::Default
                                } else {
                                    LimitStyle::SubSup
                                },
                            })?
                        },
                        Class::Number => {
                            let mut digits = String::new();
                            digits.push(glyph.ch);
                            while let Some(Item::Glyph(next)) = items.peek()
                                && next.embells.is_empty()
                                && next.typeface == glyph.typeface
                                && classify(next) == Class::Number
                            {
                                digits.push(next.ch);
                                items.next();
                            }
                            self.str_node(&digits, CompactNode::Number)?
                        },
                        Class::Styled(style) => {
                            let mut run: SmallVec<[NodeId; 8]> = SmallVec::new();
                            run.push(self.plain_char_node(glyph.ch)?);
                            while let Some(Item::Glyph(next)) = items.peek()
                                && next.embells.is_empty()
                                && next.typeface == glyph.typeface
                                && classify(next) == class
                            {
                                let ch = next.ch;
                                items.next();
                                run.push(self.plain_char_node(ch)?);
                            }
                            let span = self.children(&run)?;
                            self.node(CompactNode::Styled(style, span))?
                        },
                        _ => self.glyph_node(&glyph, class)?,
                    };
                    self.emit(&mut out, &mut pending, id)?;
                },
                Item::Node(id) => self.emit(&mut out, &mut pending, id)?,
                // A line inside a line (or the equation's own line) only
                // groups its contents.
                Item::Line(Some(id)) => {
                    for child in self.children_of(id) {
                        self.emit(&mut out, &mut pending, child)?;
                    }
                },
                Item::Line(None) => {},
                Item::Script(slots) => {
                    let base = match out.pop() {
                        Some(base) => base,
                        None => self.empty_row()?,
                    };
                    let id = self.script_node(base, slots)?;
                    out.push(id);
                },
                Item::PreScript(slots) => {
                    if let Some(previous) = pending.take() {
                        let base = self.empty_row()?;
                        let id = self.prescript_node(base, previous)?;
                        out.push(id);
                    }
                    pending = Some(slots);
                },
                Item::Embell(_, offset) => {
                    self.recover(
                        offset,
                        "EMBELL record outside a character's embellishment list; it was dropped",
                    )?;
                },
            }
        }
        if let Some(slots) = pending.take() {
            let base = self.empty_row()?;
            let id = self.prescript_node(base, slots)?;
            out.push(id);
        }
        Ok(out)
    }

    fn emit(
        &mut self,
        out: &mut SmallVec<[NodeId; 8]>,
        pending: &mut Option<ScriptSlots>,
        id: NodeId,
    ) -> Result<()> {
        let id = match pending.take() {
            Some(slots) => self.prescript_node(id, slots)?,
            None => id,
        };
        out.push(id);
        Ok(())
    }

    fn script_node(&mut self, base: NodeId, slots: ScriptSlots) -> Result<NodeId> {
        match (slots.sub, slots.sup) {
            (Some(sub), Some(sup)) => self.node(CompactNode::SubSup { base, sub, sup }),
            (Some(sub), None) => self.node(CompactNode::Subscript { base, sub }),
            (None, Some(sup)) => self.node(CompactNode::Superscript { base, sup }),
            (None, None) => Ok(base),
        }
    }

    fn prescript_node(&mut self, base: NodeId, slots: ScriptSlots) -> Result<NodeId> {
        if slots.sub.is_none() && slots.sup.is_none() {
            return Ok(base);
        }
        self.node(CompactNode::PreScripts {
            base,
            pre_sub: slots.sub.unwrap_or(NodeId::NONE),
            pre_sup: slots.sup.unwrap_or(NodeId::NONE),
        })
    }

    /// The node for a single character of an upright or bold run.
    fn plain_char_node(&mut self, ch: char) -> Result<NodeId> {
        if ch.is_ascii_digit() {
            self.char_node(ch, CompactNode::Number)
        } else {
            self.char_node(ch, CompactNode::Identifier)
        }
    }

    fn glyph_node(&mut self, glyph: &Glyph, class: Class) -> Result<NodeId> {
        let ch = glyph.ch;
        match class {
            Class::Text => self.char_node(ch, |s| CompactNode::Text(TextStyle::Normal, s)),
            Class::Function => self.char_node(ch, |s| CompactNode::Operator {
                name: s,
                // This fallback is a single function glyph, not a complete
                // native command name; it cannot denote one of the known
                // limit functions.
                limits: LimitStyle::SubSup,
            }),
            Class::Number => self.char_node(ch, CompactNode::Number),
            Class::Ident => self.char_node(ch, CompactNode::Identifier),
            Class::Styled(style) => {
                let inner = self.plain_char_node(ch)?;
                let span = self.children(&[inner])?;
                self.node(CompactNode::Styled(style, span))
            },
            Class::Symbol(kind) => self.char_node(ch, |s| CompactNode::Symbol(kind, s)),
            Class::Space(width) => self.node(CompactNode::Space(width)),
            Class::Marker => self.empty_row(),
        }
    }

    /// Apply one embellishment (MTEFv5.md "EMBELL record"; MTEF 3 defines
    /// types 2 to 20 with the same meaning).
    fn embellish(&mut self, base: NodeId, kind: u8, offset: usize) -> Result<NodeId> {
        if !self.v5 && kind > 20 {
            self.recover(
                offset,
                format!("unknown MTEF 3 embellishment type {kind}; it was dropped"),
            )?;
            return Ok(base);
        }
        let primes = match kind {
            5 => 1,
            6 => 2,
            18 => 3,
            _ => 0,
        };
        if primes > 0 {
            let mut marks: SmallVec<[NodeId; 3]> = SmallVec::new();
            for _ in 0..primes {
                marks.push(
                    self.char_node('\u{2032}', |s| CompactNode::Symbol(SymbolKind::Ordinary, s))?,
                );
            }
            let sup = self.wrap(&marks)?;
            return self.node(CompactNode::Superscript { base, sup });
        }
        match kind {
            // Backwards prime, left of the character.
            7 => {
                let mark =
                    self.char_node('\u{2035}', |s| CompactNode::Symbol(SymbolKind::Ordinary, s))?;
                self.node(CompactNode::PreScripts {
                    base,
                    pre_sub: NodeId::NONE,
                    pre_sup: mark,
                })
            },
            // Diagonal slash: the negated relation when one exists.
            10 => {
                if let CompactNode::Symbol(symbol_kind, sid) = *self.arena.node(base) {
                    let mut it = self.arena.str_val(sid).chars();
                    if let (Some(ch), None) = (it.next(), it.next())
                        && let Some(negated) = chars::negated(ch)
                    {
                        return self.char_node(negated, |s| CompactNode::Symbol(symbol_kind, s));
                    }
                }
                self.node(CompactNode::Cancel {
                    stroke: StrokeKind::Forward,
                    content: base,
                })
            },
            16 => self.node(CompactNode::Cancel {
                stroke: StrokeKind::Horizontal,
                content: base,
            }),
            21 => self.node(CompactNode::Cancel {
                stroke: StrokeKind::Cross,
                content: base,
            }),
            22 => self.node(CompactNode::Cancel {
                stroke: StrokeKind::Forward,
                content: base,
            }),
            23 => self.node(CompactNode::Cancel {
                stroke: StrokeKind::Backward,
                content: base,
            }),
            29 => {
                let line =
                    self.char_node('\u{203E}', |s| CompactNode::Symbol(SymbolKind::Ordinary, s))?;
                self.node(CompactNode::Underscript {
                    base,
                    under: line,
                    movable_limits: false,
                })
            },
            _ => {
                let Some((accent, position)) = embellishment_accent(kind) else {
                    self.recover(
                        offset,
                        format!("unknown MTEF embellishment type {kind}; it was dropped"),
                    )?;
                    return Ok(base);
                };
                if kind == 28 {
                    self.warn(
                        offset,
                        "four dots below have no Unicode accent; three dots below were used",
                    );
                }
                self.node(CompactNode::Accent {
                    base,
                    accent,
                    position,
                    wide: false,
                })
            },
        }
    }
}

/// How a character takes part in its line, from its typeface and value.
fn classify(glyph: &Glyph) -> Class {
    let ch = glyph.ch;
    let code = u32::from(ch);
    match glyph.typeface {
        chars::FN_MARKER => return Class::Marker,
        chars::FN_SPACE => {
            return match chars::space_width(code) {
                Some(width) => Class::Space(width),
                None if chars::is_marker(code) => Class::Marker,
                None if ch == ' ' || ch == '\u{00A0}' => Class::Space(Rational::new(1, 4)),
                None => Class::Space(Rational::new(3, 18)),
            };
        },
        chars::FN_TEXT | chars::FN_TEXT_FE => return Class::Text,
        _ => {},
    }
    if chars::is_marker(code) {
        return Class::Marker;
    }
    if let Some(width) = (code >= 0xE000).then(|| chars::space_width(code)).flatten() {
        return Class::Space(width);
    }
    if ch.is_whitespace() {
        return Class::Space(Rational::new(1, 4));
    }
    if let Some(style) = glyph.explicit {
        if ch.is_ascii_digit() {
            return Class::Number;
        }
        if ch.is_alphabetic() {
            return match (style.bold, style.italic) {
                (false, true) => Class::Ident,
                (true, true) => Class::Styled(TextStyle::BoldItalic),
                (true, false) => Class::Styled(TextStyle::Bold),
                (false, false) => Class::Styled(TextStyle::Normal),
            };
        }
        return Class::Symbol(chars::symbol_kind(ch));
    }
    match glyph.typeface {
        chars::FN_FUNCTION if ch.is_alphabetic() => Class::Function,
        chars::FN_VECTOR if ch.is_alphanumeric() => Class::Styled(TextStyle::Bold),
        chars::FN_NUMBER if ch.is_ascii_digit() || ch == '.' => Class::Number,
        _ if ch.is_ascii_digit() => Class::Number,
        _ if ch.is_alphabetic() => Class::Ident,
        _ => Class::Symbol(chars::symbol_kind(ch)),
    }
}

/// Accent character and position for the embellishments drawn as accents.
fn embellishment_accent(kind: u8) -> Option<(char, AccentPosition)> {
    use AccentPosition::{Above, Below};
    Some(match kind {
        2 => ('\u{0307}', Above),
        3 => ('\u{0308}', Above),
        4 => ('\u{20DB}', Above),
        8 => ('\u{0303}', Above),
        9 => ('\u{0302}', Above),
        11 => ('\u{20D7}', Above),
        12 => ('\u{2190}', Above),
        13 => ('\u{2194}', Above),
        14 => ('\u{21C0}', Above),
        15 => ('\u{21BC}', Above),
        17 => ('\u{0304}', Above),
        19 => ('\u{23DC}', Above),
        20 => ('\u{0306}', Above),
        24 => ('\u{20DC}', Above),
        25 => ('\u{0323}', Below),
        26 => ('\u{0324}', Below),
        27 | 28 => ('\u{20E8}', Below),
        30 => ('\u{0330}', Below),
        31 => ('\u{032F}', Below),
        32 => ('\u{23DD}', Below),
        33 => ('\u{2192}', Below),
        34 => ('\u{2190}', Below),
        35 => ('\u{2194}', Below),
        36 => ('\u{21C1}', Below),
        37 => ('\u{21BD}', Below),
        _ => return None,
    })
}

/// Column alignment for a PILE halign or MATRIX h_just value. Operator and
/// decimal-point alignment have no AST equivalent and become left and right.
fn column_align(value: u8) -> ColumnAlign {
    match value {
        1 | 4 => ColumnAlign::Left,
        3 | 5 => ColumnAlign::Right,
        _ => ColumnAlign::Center,
    }
}
