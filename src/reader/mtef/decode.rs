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

mod legacy;

use std::collections::{HashMap, HashSet, VecDeque};

use smallvec::SmallVec;

use super::chars::{self, Encoding};
use super::info::{MtefInfo, MtefPlatform, MtefProduct, MtefVersion, OleHeader};
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId, NodeSpan};
use crate::ast::normalize::can_take_prescripts;
use crate::ast::{
    AccentPosition, ColumnAlign, FontSize, LimitStyle, NodeAttrs, Rational, StrokeKind, SymbolKind,
    TextStyle, TypeSize,
};
use crate::error::Span;
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

/// The scripts on scripts an item may have to take another prime.
const MAX_PRIMED_CHAIN: usize = 16;

/// The primes an item may have to take another run of as many: the list of
/// both has room.
const MAX_PRIMES: usize = 30_000;

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
    /// Bold/italic of the explicit font, for a negative typeface; of a
    /// typeface style, where the equation defines it otherwise than it is
    /// (MTEF 5).
    pub(super) font_style: Option<FontStyle>,
    /// The math alphabet of a letter MathType wrote as an MTCode
    /// private-use code point (`ch` then holds the plain letter).
    pub(super) alphabet: Option<TextStyle>,
    pub(super) embells: SmallVec<[(u8, usize); 1]>,
    /// Offset of the CHAR record, for diagnostics.
    pub(super) offset: u32,
    /// Exclusive offset after the CHAR record, for optional provenance.
    pub(super) end: u32,
    /// The COLOR_DEF index the character is drawn in; `None` for black.
    pub(super) color: Option<u16>,
    /// The size the character is drawn in.
    pub(super) size: SizeState,
}

/// The current size (SIZE and typesize records), in one word: it is compared
/// for every character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SizeState(u32);

/// What a [`SizeState`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SizeRecord {
    /// A typesize value and what is added to it, in 32nds of a point.
    Logical { level: u8, delta: i16 },
    /// An explicit size, in 32nds of a point.
    Points(i16),
}

impl SizeState {
    const POINTS: u32 = 1 << 31;

    /// The typesize itself is its own word.
    #[inline(always)]
    pub(super) const fn level(level: u8) -> Self {
        Self(level as u32)
    }

    fn logical(level: u8, delta: i16) -> Self {
        Self(u32::from(level) | u32::from(delta as u16) << 8)
    }

    fn points(points: i16) -> Self {
        Self(u32::from(points as u16) | Self::POINTS)
    }

    pub(super) fn record(self) -> SizeRecord {
        if self.0 & Self::POINTS != 0 {
            SizeRecord::Points(self.0 as u16 as i16)
        } else {
            SizeRecord::Logical {
                level: self.0 as u8,
                delta: (self.0 >> 8) as u16 as i16,
            }
        }
    }
}

/// Which slots of a template are smaller than the template by their place:
/// the bit of each slot, the lowest that of the first and the sign that of all
/// the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub(super) enum SlotSizes {
    Same = 0,
    Smaller = -1,
    /// All but the first.
    FirstSame = -2,
    /// Slots 1 and 2 of four: the limits of a general big operator, whose
    /// sign is in the fourth.
    Limits = 0b0110,
}

/// The sizes of MTEF 5 equation preferences.
#[derive(Debug, Default)]
struct Preferences {
    /// The full size, in points.
    full: Option<Rational>,
    /// The symbol, sub-symbol and user sizes that are not the factory
    /// settings, in points.
    others: [Option<Rational>; 4],
}

/// Sizes that are not kept, each reported once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SizeLoss {
    SymbolSize,
    FunctionName,
    Delimiter,
}

impl SizeLoss {
    const COUNT: usize = 3;

    fn message(self) -> &'static str {
        match self {
            Self::SymbolSize => "symbol sizes of characters were dropped",
            Self::FunctionName => {
                "the sizes of the letters of a function name differ; they were dropped"
            },
            Self::Delimiter => {
                "the sizes of delimiters were dropped; they fit what is between them"
            },
        }
    }
}

/// No operator command is longer: a longer name need not be looked up.
const MAX_OPERATOR_COMMAND: usize = 32;

/// The sizes that need a second look when the equation is read.
#[derive(Debug, Default)]
pub(super) struct RareSizes {
    /// Characters whose symbol size was dropped.
    pub(super) dropped_symbols: Vec<NodeId>,
    /// The same for characters that are no node of their own.
    dropped_in_runs: bool,
    /// The losses reported, by [`SizeLoss`].
    losses: [bool; SizeLoss::COUNT],
}

/// A template whose lines are being read.
#[derive(Debug, Clone, Copy)]
#[repr(align(4))]
pub(super) struct OpenTemplate {
    /// The bits of the [`SlotSizes`] from the next slot on.
    small: i8,
    /// The typesize its text has without attributes of its own.
    level: u8,
    /// Whether there is one.
    open: bool,
}

impl OpenTemplate {
    const NONE: Self = Self {
        small: 0,
        level: 0,
        open: false,
    };
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
    /// The colour the slots are drawn in, and relative to which runs inside
    /// them are already wrapped.
    pub(super) color: Option<u16>,
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
    Text(TextStyle),
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
    /// MTEF 2 or 4: laid out like MTEF 3, with 8-bit character values (see
    /// the `legacy` module).
    legacy: bool,
    mac: bool,
    strict: bool,
    max_depth: usize,
    max_nodes: Option<usize>,
    source_ranges: bool,
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
    /// The math alphabet of each FONT_DEF, by its font name.
    font_alphabets: Vec<Option<TextStyle>>,
    /// MTEF 5 FONT_STYLE_DEF records: (font_def index, style).
    font_styles: Vec<(u16, FontStyle)>,
    /// MTEF 5 EQN_PREFS style definitions: font_def index per typeface style.
    style_fonts: Vec<u16>,
    /// Bold and italic of the typeface styles 1 to 9, where EQN_PREFS
    /// defines them otherwise than MathType does.
    defined_styles: [Option<FontStyle>; chars::FN_USER1 as usize],
    /// MTEF 5 COLOR_DEF records: each colour as `#rrggbb`, `None` for black.
    colors: Vec<Option<String>>,
    /// The COLOR_DEF index of the current colour (MTEF 5 COLOR record);
    /// `None` while it is black.
    color: Option<u16>,
    /// The colour of the structure whose lines are being assembled: a run
    /// in another colour is wrapped in a colour node.
    color_context: Option<u16>,
    /// The colour of each coloured node a line is assembled from.
    node_colors: HashMap<NodeId, u16>,
    /// What INTOP and SUMOP templates that are no big operator became.
    pub(super) template_runs: HashSet<NodeId>,
    /// The current size. Equations start at full size.
    size: SizeState,
    /// The size the lines being read have by their place in the equation:
    /// that of their text in the AST without attributes of its own.
    slot_size: u8,
    /// What only equations with sizes have.
    pub(super) rare_sizes: Option<Box<RareSizes>>,
    /// The template whose own list is being read.
    template: OpenTemplate,
    /// Whether a typesize record names a script size.
    script_sizes: bool,
    /// Whether text at full size in a smaller slot has that size.
    keeps_full: bool,
    /// Whether there was such text while it had not.
    met_full: std::cell::Cell<bool>,
    /// Where the sizes of MTEF 5 equation preferences are.
    sizes_at: Option<usize>,
    /// The sizes there, read when first asked for.
    preferences: std::cell::OnceCell<Box<Preferences>>,
    /// The leaves drawn bold or italic, each with a spare node for the leaf
    /// and the style around it.
    styled_leaves: Vec<(NodeId, NodeId, TextStyle)>,
    /// The colour each colour node was painted with, in the order of nodes.
    painted: Vec<(NodeId, Option<u16>)>,
    /// The marks of the prime rows of the line being assembled, which get
    /// them at its end: joining primes one by one must not copy them.
    open_primes: HashMap<NodeId, Vec<NodeId>>,
    /// The nodes in colours of template runs that joined the lines around.
    spliced: usize,
    /// Whether there are zero spaces, and leading scripts.
    has_zero_spaces: bool,
    has_leading_scripts: bool,
}

impl<'a> Decoder<'a> {
    pub(super) fn new(data: &'a [u8], config: &Config) -> Self {
        Self {
            data,
            pos: 0,
            end: data.len(),
            v5: false,
            legacy: false,
            mac: false,
            strict: config.strictness == Strictness::Strict,
            max_depth: config.effective_max_depth(),
            max_nodes: config.max_nodes,
            source_ranges: config.source_ranges,
            depth: 0,
            heights: Vec::new(),
            arena: AstArena::with_capacity(data.len().min(1 << 16) / 2, 0, 0),
            diagnostics: Vec::new(),
            info: None,
            v3_fonts: Box::new([None; 256]),
            encodings: Vec::new(),
            font_defs: Vec::new(),
            font_alphabets: Vec::new(),
            font_styles: Vec::new(),
            style_fonts: Vec::new(),
            defined_styles: [None; chars::FN_USER1 as usize],
            colors: Vec::new(),
            color: None,
            color_context: None,
            node_colors: HashMap::new(),
            template_runs: HashSet::new(),
            size: SizeState::level(0),
            slot_size: 0,
            rare_sizes: None,
            template: OpenTemplate::NONE,
            script_sizes: false,
            keeps_full: false,
            met_full: std::cell::Cell::new(false),
            sizes_at: None,
            preferences: std::cell::OnceCell::new(),
            styled_leaves: Vec::new(),
            painted: Vec::new(),
            open_primes: HashMap::new(),
            spliced: 0,
            has_zero_spaces: false,
            has_leading_scripts: false,
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

    #[inline]
    fn set_source_range(&mut self, id: NodeId, start: u32, end: u32) {
        if self.source_ranges && start < end {
            self.arena
                .set_source_range(id, Span::new(start as usize, end as usize));
        }
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
        if let Some(open) = self.open_primes.get(&id) {
            return open.iter().copied().collect();
        }
        match *self.arena.node(id) {
            CompactNode::Row(span) => self.arena.children(span).into(),
            _ => smallvec::smallvec![id],
        }
    }

    pub(super) fn is_empty_row(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    /// Whether a node draws nothing: rows of nothing, however nested, and
    /// piles or matrices without cells.
    pub(super) fn draws_nothing(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Row(span) => self
                .arena
                .children(span)
                .iter()
                .all(|&child| self.draws_nothing(child)),
            CompactNode::Array { cells, .. } => cells.is_empty(),
            // Normalization drops zero spaces.
            CompactNode::Space(width) => width == Rational::zero(),
            _ => false,
        }
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
        let line = self.assemble_line(items)?;
        if let Some(rare) = self.rare_sizes.take() {
            self.settle_rare_sizes(*rare);
        }
        self.style_leaves()?;
        Ok(line)
    }

    /// Note the style a number, symbol or function name is drawn in. It is put around the
    /// leaf once the equation is complete, so that the leaf is found as one
    /// until then.
    fn set_leaf_style(&mut self, id: NodeId, glyph: &Glyph) -> Result<()> {
        let Some(style) = leaf_style(glyph) else {
            return Ok(());
        };
        let spare = self.node(*self.arena.node(id))?;
        // Checked where a node is built around it.
        self.heights[id.index()] += 1;
        self.styled_leaves.push((id, spare, style));
        Ok(())
    }

    fn style_leaves(&mut self) -> Result<()> {
        for (id, spare, style) in std::mem::take(&mut self.styled_leaves) {
            let leaf = *self.arena.node(id);
            // A name that takes limits has to be found as one.
            let styled = match leaf {
                CompactNode::Operator { limits, .. } => limits == LimitStyle::SubSup,
                CompactNode::Number(_) | CompactNode::Symbol(..) => true,
                _ => false,
            };
            if styled {
                *self.arena.node_mut(spare) = leaf;
                let span = self.children(&[spare])?;
                *self.arena.node_mut_in_place(id) = CompactNode::Styled(style, span);
            }
        }
        Ok(())
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
            2 => MtefVersion::V2,
            3 => MtefVersion::V3,
            4 => MtefVersion::V4,
            5 => MtefVersion::V5,
            1 | 101 => {
                return Err(self.error_at(
                    start,
                    format!("MTEF version {version} is not supported (only versions 2 to 5 are)"),
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
        self.legacy = matches!(version, MtefVersion::V2 | MtefVersion::V4);
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

    /// Bytes after the final END. Zero padding is common and harmless. The
    /// size in an `Equation Native` header can count the buffer the equation
    /// was written into rather than the equation: in every real stream with
    /// such bytes it is a multiple of 4 that exceeds the equation by less
    /// than 8 bytes, or a fixed 400. The slack holds unrelated leftovers
    /// (heap fill, string fragments, part of an earlier equation cut off at
    /// the buffer's end). Any other tail is unexplained, for instance a
    /// damaged equation that ends too early, and is not accepted; nor is a
    /// tail that decodes as equation content.
    fn check_trailing(&mut self) -> Result<()> {
        let rest = &self.data[self.pos..self.end];
        if rest.iter().all(|&b| b == 0) {
            return Ok(());
        }
        let (pos, len) = (self.pos, rest.len());
        let is_slack = !self.tail_holds_equation_data()
            && self
                .info
                .as_ref()
                .and_then(|info| info.ole_header)
                .is_some_and(|header| {
                    header.object_size.is_multiple_of(4)
                        && (len < 8 || header.object_size == 400 || self.legacy_slack(len))
                });
        if is_slack {
            self.note(format!(
                "ignored {len} bytes of buffer slack after the end of the equation (at byte {pos})"
            ));
            Ok(())
        } else {
            self.recover(
                pos,
                format!("{len} bytes of data follow the end of the equation"),
            )
        }
    }

    /// Whether the bytes after the final END hold equation content: past
    /// further ENDs they decode as a list of objects that draws something, or
    /// as a complete CHAR record, possibly after typesize, font and colour
    /// records.
    ///
    /// A list of two or more LINEs and nothing else is the shape of a
    /// template's slots, which a piece of an earlier equation in the slack
    /// has. Such a piece is cut off by the end of the buffer, so it counts as
    /// content only when everything up to the end (or zero padding) decodes.
    ///
    /// Bytes after the final END are not part of the equation by definition,
    /// and slack can hold anything, so no such rule is airtight: this one
    /// refuses tails that a damaged or second equation would leave.
    fn tail_holds_equation_data(&self) -> bool {
        let rest = &self.data[self.pos..self.end];
        let Some(skip) = rest.iter().position(|&b| b != 0) else {
            return false;
        };
        let config = Config::default().with_strictness(Strictness::Strict);
        let trial = || {
            let mut trial = Decoder::new(self.data, &config);
            trial.pos = self.pos + skip;
            trial.end = self.end;
            trial.v5 = self.v5;
            trial.legacy = self.legacy;
            trial.mac = self.mac;
            trial.v3_fonts = self.v3_fonts.clone();
            trial.encodings = self.encodings.clone();
            trial.font_defs = self.font_defs.clone();
            trial.font_alphabets = self.font_alphabets.clone();
            trial.font_styles = self.font_styles.clone();
            trial.style_fonts = self.style_fonts.clone();
            trial
        };
        let mut list = trial();
        // Past lists that draw nothing.
        while list.skip_zeros()
            && let Ok(items) = list.parse_object_list(true)
        {
            let draws = items.iter().any(|item| match *item {
                Item::Glyph(_) | Item::Script(_) | Item::PreScript(_) => true,
                Item::Node(id) | Item::Line(Some(id)) => !list.draws_nothing(id),
                Item::Line(None) | Item::Embell(..) => false,
            });
            if draws {
                let slots =
                    items.len() > 1 && items.iter().all(|item| matches!(item, Item::Line(_)));
                return !slots || list.decodes_to_the_end();
            }
        }
        let mut first = trial();
        loop {
            let start = first.pos;
            match first.read_tag() {
                Ok((CHAR, options)) => return first.parse_char(start, options).is_ok(),
                Ok((
                    rtype @ (FONT
                    | SIZE
                    | FULL..=SUBSYM
                    | COLOR
                    | COLOR_DEF
                    | FONT_DEF
                    | ENCODING_DEF),
                    options,
                )) => {
                    if first
                        .parse_record(start, rtype, options, &mut Vec::new())
                        .is_err()
                    {
                        return false;
                    }
                },
                _ => return false,
            }
        }
    }

    /// Whether only complete object lists and zero padding follow.
    fn decodes_to_the_end(&mut self) -> bool {
        while self.skip_zeros() {
            if self.parse_object_list(false).is_err() {
                return false;
            }
        }
        true
    }

    /// Skip zero bytes; whether anything follows them.
    fn skip_zeros(&mut self) -> bool {
        let rest = &self.data[self.pos..self.end];
        let skip = rest.iter().position(|&b| b != 0).unwrap_or(rest.len());
        self.pos += skip;
        skip < rest.len()
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
        let mut items = Vec::with_capacity(8);
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
                let color = self.color;
                let outer = std::mem::replace(&mut self.color_context, color);
                let id = self.in_slot(|decoder| decoder.parse_pile(options))?;
                self.color_context = outer;
                self.set_color(id, color);
                items.push(Item::Node(id));
            },
            MATRIX => {
                let color = self.color;
                let outer = std::mem::replace(&mut self.color_context, color);
                let id = self.in_slot(|decoder| decoder.parse_matrix(start, options))?;
                self.color_context = outer;
                self.set_color(id, color);
                items.push(Item::Node(id));
            },
            EMBELL => {
                if options & OPT_NUDGE != 0 {
                    self.nudge()?;
                }
                let kind = self.u8()?;
                if !self.v5 && !self.legacy {
                    // Equation Editor 3 writes a zero byte after the type; the
                    // MTEF 3 description omits it, but every reader of real
                    // files has to consume it to stay in step. MTEF 2 and 4
                    // files do not have it.
                    self.u8()?;
                }
                items.push(Item::Embell(kind, start));
            },
            RULER => self.ruler_body()?,
            FONT if self.v5 => self.font_style_def()?,
            FONT => self.v3_font()?,
            SIZE => self.size_record()?,
            FULL..=SUBSYM => {
                self.set_typesize(SizeState::level(rtype - FULL));
            },
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
                let version = self.info.as_ref().map_or(3, |info| info.version.number());
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
        self.in_slot(|decoder| decoder.parse_line_body(options))
    }

    fn parse_line_body(&mut self, options: u8) -> Result<Option<NodeId>> {
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
        let ids = self.assemble_line(items)?;
        let span = self.children(&ids)?;
        Ok(Some(self.node(CompactNode::Row(span))?))
    }

    /// The RULER of a LINE or PILE with the ruler option. Both specifications
    /// call it a RULER record. The 171 MathType 4 to 6 files in the audited
    /// corpus that have one (all on a LINE or PILE of MTEF 5) write it
    /// without the record's tag; no MTEF 3 file there has one, so MTEF 3 is
    /// read as specified.
    fn ruler_record(&mut self) -> Result<()> {
        if !self.v5 {
            let start = self.pos;
            let rtype = self.u8()? & 0x0F;
            if rtype != RULER {
                return Err(self.error_at(
                    start,
                    format!("expected a RULER record, found record type {rtype}"),
                ));
            }
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
        let (typeface, ch, has_embells, func_start, alphabet) = if self.v5 {
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
            let mut ch = self.v5_char(start, typeface, mtcode, font_position)?;
            // A math-alphabet letter in MTCode's private-use blocks, when the
            // font and the font position both say so.
            let mut alphabet = None;
            if let Some(style) = self.alphabet_of_font(typeface)
                && let Some(letter) = chars::mtcode_alphabet(ch, style)
                && font_position == Some(u16::from(letter as u8))
            {
                ch = letter;
                alphabet = Some(style);
            }
            (
                typeface,
                ch,
                options & V5_CHAR_EMBELL != 0,
                options & V5_CHAR_FUNC_START != 0,
                alphabet,
            )
        } else {
            let typeface = i32::from(self.u8()?) - 128;
            let (typeface, ch) = if self.legacy {
                let code = self.u8()?;
                self.legacy_char(start, typeface, code)?
            } else {
                let code = self.u16()?;
                (typeface, self.v3_char(start, typeface, code)?)
            };
            (typeface, ch, options & V3_CHAR_EMBELL != 0, false, None)
        };
        let explicit = if typeface < 0 {
            Some(self.explicit_font_style(start, typeface)?)
        } else {
            if !matches!(typeface, 1..=12 | 22..=24) {
                self.recover(start, format!("unknown MTEF typeface style {typeface}"))?;
            }
            None
        };
        // Bold is read as the specification defines the style byte; no
        // audited file has a bold letter of these fonts.
        let alphabet = alphabet.map(|style| match (style, explicit) {
            (TextStyle::Fraktur, Some(FontStyle { bold: true, .. })) => TextStyle::BoldFraktur,
            (TextStyle::Script, Some(FontStyle { bold: true, .. })) => TextStyle::BoldScript,
            _ => style,
        });
        let mut ch = chars::normalize_code_point(ch);
        if !matches!(typeface, chars::FN_TEXT | chars::FN_TEXT_FE) {
            ch = chars::math_ascii(ch);
        }
        // MathType 5.0 writes the Spacing style's characters as their bare
        // codes (0x02 for a thin space), not in the 0xEF00 block.
        let space_code = typeface == chars::FN_SPACE && chars::space_width(u32::from(ch)).is_some();
        if ch.is_control() && !ch.is_whitespace() && !space_code {
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
            font_style: explicit.or_else(|| self.defined_style(typeface)),
            alphabet,
            embells,
            offset: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(self.pos).unwrap_or(u32::MAX),
            color: self.color,
            size: self.size,
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

    /// The math alphabet of an MTEF 5 explicit font.
    fn alphabet_of_font(&self, typeface: i32) -> Option<TextStyle> {
        let style = usize::try_from(typeface.checked_neg()?)
            .ok()?
            .checked_sub(1)?;
        let &(font, _) = self.font_styles.get(style)?;
        *self.font_alphabets.get(usize::from(font).checked_sub(1)?)?
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
        self.check_v2_selector(start, selector)?;
        let first = self.u8()?;
        // MTEF 5 writes a two-byte variation when the high bit is set.
        let variation = if self.v5 && first & 0x80 != 0 {
            u16::from(first & 0x7F) | (u16::from(self.u8()?) << 8)
        } else {
            u16::from(first)
        };
        let _template_options = self.u8()?;
        // The lines of a template are assembled relative to its colour.
        let color = self.color;
        let outer = std::mem::replace(&mut self.color_context, color);
        // Only characters have sizes: what a template draws of its own has the
        // size of its contents or of its sign.
        let open = OpenTemplate {
            small: self.slot_sizes(selector, variation) as i8,
            level: self.slot_size,
            open: true,
        };
        let outer_template = std::mem::replace(&mut self.template, open);
        let list = self.parse_list();
        self.template = outer_template;
        self.color_context = outer;
        let list = list?;
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
        let first = items.len();
        self.build_template(start, selector, variation, slots, glyphs, items)?;
        for item in &mut items[first..] {
            match item {
                Item::Node(id) => {
                    self.set_color(*id, color);
                },
                Item::Script(slots) | Item::PreScript(slots) => slots.color = color,
                _ => {},
            }
        }
        Ok(())
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
        let (row_rules, row_dashes) = self.partition_lines(rows + 1)?;
        let row_seps = row_rules.low();
        if row_rules.extended_count() > 0 {
            self.warn(
                start,
                format!(
                    "{} horizontal matrix partition lines exceed the native row mask",
                    row_rules.extended_count()
                ),
            );
        }
        let (column_rules, col_dashes) = self.partition_lines(cols + 1)?;
        let col_seps = column_rules.low();
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
        let align = column_align(h_just);
        let mut right_rule = None;
        // The native mask covers the first 32 boundaries. Retain later
        // column rules on the preceding cell, in the public tree as well
        // as the compact one. These wrappers also survive table edits.
        for (index, cell) in cells.iter_mut().enumerate() {
            let row = self.as_row(*cell)?;
            let edge = index % num_cols + 1;
            *cell = if edge >= u32::BITS as usize && column_rules.get(edge) {
                let rule = *right_rule.get_or_insert_with(|| {
                    self.arena
                        .push_span_boundaries(&[crate::ast::SpanBoundary::Rule])
                });
                let content = match *self.arena.node(row) {
                    CompactNode::Row(span) if self.arena.children(span).len() == 1 => {
                        self.arena.children(span)[0]
                    },
                    _ => row,
                };
                self.node(CompactNode::SpannedCell {
                    alignment: align,
                    span: 1,
                    left_boundaries: crate::ast::compact::StrId::EMPTY,
                    right_boundaries: rule,
                    content,
                })?
            } else {
                row
            };
        }
        let mut aligns: SmallVec<[NodeId; 8]> = SmallVec::new();
        for _ in 0..num_cols {
            aligns.push(self.node(CompactNode::ColAlign(align))?);
        }
        let col_aligns = self.children(&aligns)?;
        let cells = self.children(&cells)?;
        let array_style = column_rules.any() || row_seps != 0;
        let id = self.node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: u16::try_from(num_cols).unwrap_or(u16::MAX),
            arraystretch_pct: 100,
            add_jot: false,
            // MATRIX partition lines record the rules. Import a ruled matrix
            // with standard array padding on its outer edges.
            array_style,
            col_seps,
            row_seps,
        })?;
        // Dashed and dotted partition lines are drawn dashed (the AST has
        // no dotted rule).
        let dashes = crate::ast::arena::RuleDashes {
            cols: col_dashes & col_seps,
            rows: row_dashes & row_seps,
        };
        if dashes != crate::ast::arena::RuleDashes::default() {
            self.arena.mark_native_array(id, dashes);
        }
        Ok(id)
    }

    /// Each boundary occupies two bits, least significant first: 0 none,
    /// 1 solid, 2 dashed, 3 dotted. Returns the boundaries with a line, and
    /// the first 32 of them drawn dashed or dotted.
    fn partition_lines(&mut self, lines: usize) -> Result<(super::PartitionMask, u32)> {
        let mut mask = super::PartitionMask::default();
        let mut dashed = 0u32;
        for offset in (0..lines).step_by(4) {
            let byte = self.u8()?;
            if byte == 0 {
                continue;
            }
            for k in 0..(lines - offset).min(4) {
                let style = (byte >> (2 * k)) & 3;
                if style != 0 {
                    mask.set(offset + k, true);
                }
                if style >= 2 && offset + k < 32 {
                    dashed |= 1 << (offset + k);
                }
            }
        }
        Ok((mask, dashed))
    }

    // ── Non-structural records ─────────────────────────────────────────

    fn v3_font(&mut self) -> Result<()> {
        let typeface = self.u8()?;
        let style = self.u8()?;
        let name = self.cstr()?;
        // 1 is bold and 2 italic. The descriptions of MTEF 3 and 4 say the
        // opposite; the pictures stored with MTEF 2 and 3 equations do not
        // (docs/mtef-legacy.md). No picture of an MTEF 4 equation has either.
        let is_v4 = self
            .info
            .as_ref()
            .is_some_and(|info| info.version == MtefVersion::V4);
        let style = FontStyle {
            bold: style & if is_v4 { 0x02 } else { 0x01 } != 0,
            italic: style & if is_v4 { 0x01 } else { 0x02 } != 0,
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
        let name = self.cstr()?;
        self.font_defs.push(encoding);
        self.font_alphabets.push(chars::alphabet_font(name));
        Ok(())
    }

    /// SIZE: an explicit point size (101), a large delta (100) or a typesize
    /// with a one-byte delta.
    fn size_record(&mut self) -> Result<()> {
        let size = match self.u8()? {
            // MTEFv5.md gives "-point size" here, but every record seen in
            // documents holds the positive size. Both are read.
            101 => SizeState::points((self.u16()? as i16).saturating_abs()),
            100 => {
                let level = self.u8()?;
                let delta = self.u16()? as i16;
                SizeState::logical(level, delta)
            },
            level => SizeState::logical(level, i16::from(self.u8()?) - 128),
        };
        self.set_typesize(size);
        Ok(())
    }

    /// A SIZE record, or a typesize record: the short way of specifying a
    /// typesize with nothing added (MTEFv5.md "Typesize records").
    fn set_typesize(&mut self, size: SizeState) {
        // A full size that is said to be one in a smaller slot is not the lack
        // of records.
        self.script_sizes |=
            matches!(size.0, 1 | 2) || size == SizeState::level(0) && self.slot_size != 0;
        self.size = size;
    }

    /// Whether `size` is, without a closer look, what text has by its place.
    #[inline(always)]
    fn is_size_by_place(&self, size: SizeState) -> bool {
        size == self.size_by_place()
    }

    #[cold]
    fn settle_rare_sizes(&mut self, rare: RareSizes) {
        if rare.dropped_in_runs || !rare.dropped_symbols.is_empty() {
            self.lose_size(SizeLoss::SymbolSize);
        }
    }

    /// Whether `size` is a symbol size. Big operators have their signs at
    /// it; what it makes of other characters is the document's setting, which
    /// MTEF 3 does not record. It is not kept.
    #[inline]
    fn is_dropped_symbol(size: SizeState) -> bool {
        // The typesizes 3 and 4, with nothing added.
        matches!(size.0, 3 | 4)
    }

    /// The size of something drawn in `size` that does not have it by its
    /// place in the equation.
    fn own_size(&self, size: SizeState) -> Option<FontSize> {
        match size.record() {
            SizeRecord::Points(points) => {
                (points > 0).then(|| FontSize::Points(Rational::new(points.into(), 32)))
            },
            SizeRecord::Logical { level, delta } => {
                if level == self.slot_size && delta == 0 || Self::is_dropped_symbol(size) {
                    return None;
                }
                // Some writers leave the typesize records out: without any
                // script size in the equation, text at full size in a smaller
                // slot is what it is by its place. That is known at the end.
                if size == SizeState::level(0) && !self.keeps_full {
                    self.met_full.set(true);
                    return None;
                }
                let level = match level {
                    0 => TypeSize::Full,
                    1 => TypeSize::Sub,
                    2 => TypeSize::Sub2,
                    3 => TypeSize::Sym,
                    4 => TypeSize::SubSym,
                    5 => TypeSize::User1,
                    6 => TypeSize::User2,
                    _ => return None,
                };
                // A size that the preferences set: scripts and what matches
                // them are the layout's matter, the others are as they say.
                if !matches!(level, TypeSize::Full | TypeSize::Sub | TypeSize::Sub2)
                    && let Some(points) = self.preferred_size(level)
                {
                    let den = i64::from(points.den);
                    let num = i64::from(points.num) * 32 + i64::from(delta) * den;
                    return Some(FontSize::Points(Rational::new(
                        i32::try_from(num).ok()?,
                        u32::try_from(den * 32).ok()?,
                    )));
                }
                Some(FontSize::Logical {
                    level,
                    delta_pt: Rational::new(delta.into(), 32),
                })
            },
        }
    }

    /// The size of a sign that is not its by place, as that of text there.
    /// Typesizes are where equations have them: only sizes in points and
    /// amounts added to a typesize count.
    #[inline(always)]
    pub(super) fn sign_size(&self, sign: &Glyph, big_operator: bool) -> Option<FontSize> {
        match sign.size.record() {
            SizeRecord::Logical { delta: 0, .. } => None,
            _ => self.own_sign_size(sign, big_operator),
        }
    }

    #[cold]
    fn own_sign_size(&self, sign: &Glyph, big_operator: bool) -> Option<FontSize> {
        let text = [1000, 700, 500][usize::from(self.slot_size.min(2))];
        let normal = match (big_operator, text) {
            (false, _) => text,
            (true, 1000) => 1500,
            (true, _) => 1000,
        };
        let points = match sign.size.record() {
            SizeRecord::Points(points) => i64::from(points),
            SizeRecord::Logical { delta: 0, .. } => return None,
            SizeRecord::Logical { level, delta } => {
                let per_mille = [1000, 700, 500, 1500, 1000, 750, 1500]
                    .get(usize::from(level))
                    .copied()?;
                let full = self.full_size().unwrap_or(Rational::new(12, 1));
                i64::from(full.num) * 32 * per_mille / (i64::from(full.den) * 1000)
                    + i64::from(delta)
            },
        };
        if points <= 0 {
            return None;
        }
        let num = i32::try_from(points.checked_mul(text)?).ok()?;
        Some(FontSize::Points(Rational::new(
            num,
            u32::try_from(32 * normal).ok()?,
        )))
    }

    /// Report a size that the AST has no place for.
    #[cold]
    pub(super) fn lose_size(&mut self, loss: SizeLoss) {
        let rare = self.rare_sizes.get_or_insert_default();
        if !std::mem::replace(&mut rare.losses[loss as usize], true) {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Info,
                message: loss.message().to_owned(),
                span: None,
            });
        }
    }

    /// Record the size of a node drawn in `size`.
    #[inline(always)]
    fn set_size(&mut self, id: NodeId, size: SizeState) {
        if !self.is_size_by_place(size) {
            self.set_own_size(id, size);
        }
    }

    /// Give primes that were read as characters of a line the sizes of the
    /// scripts they become.
    fn size_as_script(&mut self, id: NodeId) {
        if !self.arena.has_attrs() {
            return;
        }
        for mark in self.children_of(id) {
            if let Some(attrs) = self.arena.attrs(mark).cloned()
                && let Some(size) = attrs.size
            {
                let script = NodeAttrs::with_size(self.script_of(size));
                self.arena.remove_attrs(mark, &attrs);
                self.arena.add_attrs(mark, &script);
            }
        }
    }

    /// The size of primes drawn in `size`: raised characters there, a script
    /// in the AST.
    fn set_prime_size(&mut self, marks: NodeId, size: SizeState) {
        if self.is_size_by_place(size) {
            return;
        }
        if Self::is_dropped_symbol(size) {
            self.rare_sizes.get_or_insert_default().dropped_in_runs = true;
        }
        if let Some(own) = self.own_size(size) {
            self.arena
                .add_attrs(marks, &NodeAttrs::with_size(self.script_of(own)));
        }
    }

    #[cold]
    fn set_own_size(&mut self, id: NodeId, size: SizeState) {
        // What has no glyph has no size to see.
        let drawn = match *self.arena.node(id) {
            CompactNode::Space(_) => false,
            CompactNode::Text(_, text) => !self.arena.str_val(text).trim().is_empty(),
            _ => true,
        };
        if drawn && Self::is_dropped_symbol(size) {
            self.rare_sizes
                .get_or_insert_default()
                .dropped_symbols
                .push(id);
        } else if drawn && let Some(size) = self.own_size(size) {
            self.add_size(id, size);
        }
    }

    fn add_size(&mut self, id: NodeId, size: FontSize) {
        self.arena.add_attrs(id, &NodeAttrs::with_size(size));
    }

    /// The size of an embellished character and its marks, which have the
    /// character's size.
    fn set_embellished_size(&mut self, id: NodeId, size: SizeState) {
        if self.is_size_by_place(size) {
            return;
        }
        let Some(own) = self.own_size(size) else {
            if Self::is_dropped_symbol(size) {
                self.rare_sizes
                    .get_or_insert_default()
                    .dropped_symbols
                    .push(id);
            }
            return;
        };
        let mut current = id;
        loop {
            match *self.arena.node(current) {
                CompactNode::Accent { base, .. } => {
                    self.arena
                        .add_attrs(current, &NodeAttrs::with_glyph_size(own));
                    current = base;
                },
                CompactNode::Superscript { base, sup } => {
                    self.arena
                        .add_attrs(sup, &NodeAttrs::with_size(self.script_of(own)));
                    current = base;
                },
                CompactNode::Underscript { base, under, .. } => {
                    self.add_size(under, own);
                    current = base;
                },
                CompactNode::PreScripts { base, pre_sup, .. } => {
                    self.arena
                        .add_attrs(pre_sup, &NodeAttrs::with_size(self.script_of(own)));
                    current = base;
                },
                CompactNode::Cancel { content, .. } => current = content,
                _ => return self.set_own_size(current, size),
            }
        }
    }

    /// The size of a script of text in `size`, in 32nds of a point.
    fn script_of(&self, size: FontSize) -> FontSize {
        let full = self.full_size().unwrap_or(Rational::new(12, 1));
        size.script(full.as_f64())
    }

    /// Read a slot of the open template, or a line of a pile or matrix.
    #[inline]
    fn in_slot<T>(&mut self, read: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let mut open = std::mem::replace(&mut self.template, OpenTemplate::NONE);
        let outer = self.slot_size;
        if open.open {
            self.slot_size = if open.small & 1 != 0 {
                (open.level + 1).min(2)
            } else {
                open.level
            };
            open.small >>= 1;
        }
        let result = read(self);
        self.slot_size = outer;
        self.template = open;
        result
    }

    /// COLOR: the following records are drawn in the given colour.
    fn color(&mut self) -> Result<()> {
        let index = self.uint()?;
        let defined = usize::from(index)
            .checked_sub(1)
            .and_then(|i| self.colors.get(i))
            .is_some_and(Option::is_some);
        self.color = defined.then_some(index);
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
        // Components range from 0 to 1000.
        let unit = |v: u16| f64::from(v.min(1000)) / 1000.0;
        let rgb = if components == 4 {
            let k = 1.0 - unit(values[3]);
            [0, 1, 2].map(|i| (1.0 - unit(values[i])) * k)
        } else {
            [0, 1, 2].map(|i| unit(values[i]))
        };
        let rgb = rgb.map(|v| (v * 255.0).round() as u8);
        self.colors.push(
            (rgb != [0, 0, 0]).then(|| format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])),
        );
        Ok(())
    }

    /// EQN_PREFS: options, the sizes and spacing dimension arrays, then the
    /// style definitions (MTEFv5.md "EQN_PREFS records", "Dimension arrays").
    fn eqn_prefs(&mut self, start: usize) -> Result<()> {
        self.sizes_at = Some(self.pos);
        self.preferences = std::cell::OnceCell::new();
        self.dimension_array(start)?;
        self.dimension_array(start)?;
        let count = self.u8()?;
        self.style_fonts.clear();
        self.defined_styles = [None; chars::FN_USER1 as usize];
        for index in 0..usize::from(count) {
            let font = self.uint()?;
            if font != 0 {
                // Bit 0 bold, bit 1 italic.
                let bits = self.u8()? & 0x03;
                if let (Some(defined), Some(&own)) =
                    (self.defined_styles.get_mut(index), OWN_STYLES.get(index))
                    && bits != own
                {
                    *defined = Some(FontStyle {
                        bold: bits & 0x01 != 0,
                        italic: bits & 0x02 != 0,
                    });
                }
            }
            self.style_fonts.push(font);
        }
        Ok(())
    }

    fn defined_style(&self, typeface: i32) -> Option<FontStyle> {
        let index = usize::try_from(typeface).ok()?.checked_sub(1)?;
        *self.defined_styles.get(index)?
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

    /// The full size of MTEF 5 equation preferences, in points.
    pub(super) fn full_size(&self) -> Option<Rational> {
        self.preferences().full
    }

    /// The size that the preferences of MTEF 5 give a typesize, in points, if
    /// it is not the factory setting.
    #[cold]
    fn preferred_size(&self, level: TypeSize) -> Option<Rational> {
        let preferences = self.preferences();
        match level {
            TypeSize::Sym => preferences.others[0],
            TypeSize::SubSym => preferences.others[1],
            TypeSize::User1 => preferences.others[2],
            TypeSize::User2 => preferences.others[3],
            TypeSize::Full | TypeSize::Sub | TypeSize::Sub2 => None,
        }
    }

    fn preferences(&self) -> &Preferences {
        self.preferences
            .get_or_init(|| Box::new(self.read_preferences().unwrap_or_default()))
    }

    #[cold]
    fn read_preferences(&self) -> Option<Preferences> {
        let (&count, bytes) = self.data.get(self.sizes_at?..self.end)?.split_first()?;
        let mut nibbles = bytes.iter().flat_map(|b| [b >> 4, b & 0x0F]);
        // Full, Sub, Sub2, Sym, SubSym and the two user sizes.
        let mut sizes: [Option<(u8, String)>; 7] = Default::default();
        for size in sizes.iter_mut().take(usize::from(count)) {
            let unit = nibbles.next()?;
            let mut digits = String::new();
            for nibble in nibbles.by_ref().take_while(|&nibble| nibble != 0x0F) {
                if digits.len() < 16
                    && let Some(&digit) = b"0123456789.-".get(usize::from(nibble))
                {
                    digits.push(char::from(digit));
                }
            }
            *size = Some((unit, digits));
        }
        let points = |size: &Option<(u8, String)>| {
            let (unit, digits) = size.as_ref()?;
            dimension_points(*unit, digits)
        };
        let full = points(&sizes[0]);
        let other = |size: &Option<(u8, String)>, level: TypeSize| {
            let (unit, digits) = size.as_ref()?;
            if *unit != 4 {
                return points(size);
            }
            let percent = Rational::from_scaled_decimal(digits, Rational::one())?;
            if percent == Rational::new(level.per_mille(), 10) || percent.num <= 0 {
                return None;
            }
            let full = full.unwrap_or(Rational::new(12, 1));
            Rational::from_scaled_decimal(
                digits,
                Rational::new(full.num, full.den.checked_mul(100)?),
            )
        };
        Some(Preferences {
            full,
            others: [
                other(&sizes[3], TypeSize::Sym),
                other(&sizes[4], TypeSize::SubSym),
                other(&sizes[5], TypeSize::User1),
                other(&sizes[6], TypeSize::User2),
            ],
        })
    }

    /// Whether the equation has to be decoded again, keeping the full size of
    /// text in smaller slots: it has script sizes, which such text has not.
    pub(super) fn decodes_again(&self) -> bool {
        self.script_sizes && self.met_full.get()
    }

    pub(super) fn keep_full_sizes(&mut self) {
        self.keeps_full = true;
    }

    // ── Line assembly ──────────────────────────────────────────────────

    pub(super) fn slot_size_level(&self) -> u8 {
        self.slot_size
    }

    /// The size that text has by its place.
    #[inline(always)]
    fn size_by_place(&self) -> SizeState {
        SizeState::level(self.slot_size)
    }

    /// Note the size of a character that joins a run, where it is dropped.
    #[inline(always)]
    fn note_run_size(&mut self, size: SizeState, ch: char) {
        if Self::is_dropped_symbol(size) && !ch.is_whitespace() {
            self.rare_sizes.get_or_insert_default().dropped_in_runs = true;
        }
    }

    /// Whether a glyph is drawn in the size of a run in `size`.
    fn same_size(&self, glyph: &Glyph, size: SizeState) -> bool {
        glyph.size == size || self.own_size(glyph.size) == self.own_size(size)
    }

    /// Turn the items of a line into nodes: group characters into function
    /// names, numbers and text, attach scripts to their bases and apply
    /// embellishments.
    pub(super) fn assemble_line(&mut self, items: Vec<Item>) -> Result<SmallVec<[NodeId; 8]>> {
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut pending: Option<ScriptSlots> = None;
        let mut items: VecDeque<Item> = items.into();
        self.mark_primes(items.make_contiguous());
        while let Some(item) = items.pop_front() {
            match item {
                Item::Glyph(glyph) => {
                    let class = classify(&glyph);
                    if chars::is_private_use(glyph.ch)
                        && !matches!(class, Class::Space(_) | Class::Marker)
                    {
                        self.warn(
                            glyph.offset as usize,
                            format!(
                                "MathType private-use character U+{:04X} has no Unicode equivalent; it was kept as is",
                                u32::from(glyph.ch)
                            ),
                        );
                    }
                    if !glyph.embells.is_empty() {
                        // A prime with primes is that many primes.
                        if chars::is_prime(glyph.ch)
                            && pending.is_none()
                            && let Some(count) = primes_of(&glyph.embells)
                            && let Some(base) = self.take_prime_base(&mut out, glyph.color)?
                        {
                            let mut marks: SmallVec<[NodeId; 4]> = SmallVec::new();
                            for at in 0..=count {
                                let mark = if at == 0 {
                                    self.glyph_node(&glyph, class)?
                                } else {
                                    self.char_node('\u{2032}', |s| {
                                        CompactNode::Symbol(SymbolKind::Ordinary, s)
                                    })?
                                };
                                self.set_prime_size(mark, glyph.size);
                                marks.push(mark);
                            }
                            let id = self.prime_in(base, &marks, glyph.color)?;
                            out.push(id);
                            continue;
                        }
                        let mut id = self.glyph_node(&glyph, class)?;
                        // A zero space or a marker draws nothing to embellish.
                        if self.draws_nothing(id) {
                            id = self.empty_row()?;
                        }
                        for &(kind, offset) in &glyph.embells {
                            id = self.embellish(id, kind, offset)?;
                        }
                        self.set_color(id, glyph.color);
                        self.set_embellished_size(id, glyph.size);
                        self.emit(&mut out, &mut pending, id)?;
                        continue;
                    }
                    if chars::is_prime(glyph.ch)
                        && pending.is_none()
                        && let Some(base) = self.take_prime_base(&mut out, glyph.color)?
                    {
                        let mut marks: SmallVec<[NodeId; 3]> = SmallVec::new();
                        let mark = self.glyph_node(&glyph, class)?;
                        self.set_prime_size(mark, glyph.size);
                        marks.push(mark);
                        while marks.len() < MAX_PRIMES
                            && let Some(Item::Glyph(next)) = items.front()
                            && next.embells.is_empty()
                            && next.color == glyph.color
                            && chars::is_prime(next.ch)
                        {
                            let next = next.clone();
                            items.pop_front();
                            let mark = self.glyph_node(&next, classify(&next))?;
                            self.set_prime_size(mark, next.size);
                            marks.push(mark);
                        }
                        let id = self.prime_in(base, &marks, glyph.color)?;
                        out.push(id);
                        continue;
                    }
                    let mut size = glyph.size;
                    let id = match class {
                        Class::Marker => continue,
                        Class::Text(style) => {
                            let mut text = String::new();
                            text.push(glyph.ch);
                            let mut blank = glyph.ch.is_whitespace();
                            while let Some(Item::Glyph(next)) = items.front()
                                && next.embells.is_empty()
                                && next.color == glyph.color
                                && classify(next) == class
                                && (blank || self.same_size(next, size) || next.ch.is_whitespace())
                            {
                                // Blanks have no size that shows.
                                if blank {
                                    size = next.size;
                                    blank = next.ch.is_whitespace();
                                }
                                text.push(next.ch);
                                self.note_run_size(next.size, next.ch);
                                items.pop_front();
                            }
                            self.str_node(&text, |s| CompactNode::Text(style, s))?
                        },
                        Class::Function => {
                            let mut mixed = false;
                            let mut name = String::new();
                            name.push(glyph.ch);
                            while let Some(Item::Glyph(next)) = items.front()
                                && next.embells.is_empty()
                                && next.color == glyph.color
                                && classify(next) == Class::Function
                                && !next.func_start
                                // A name is one whatever its sizes.
                                && (self.same_size(next, size)
                                    || name.len() > MAX_OPERATOR_COMMAND
                                    || !crate::unicode::symbols::is_operator_command(&name))
                            {
                                mixed |= !self.same_size(next, size);
                                name.push(next.ch);
                                self.note_run_size(next.size, next.ch);
                                items.pop_front();
                            }
                            if mixed {
                                self.lose_size(SizeLoss::FunctionName);
                                size = self.size_by_place();
                            }
                            let id = self.str_node(&name, |s| CompactNode::Operator {
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
                            })?;
                            if glyph.font_style.is_some() {
                                self.set_leaf_style(id, &glyph)?;
                            }
                            id
                        },
                        Class::Number => {
                            let mut digits = String::new();
                            digits.push(glyph.ch);
                            loop {
                                if let Some(Item::Glyph(next)) = items.front()
                                    && next.embells.is_empty()
                                    && next.color == glyph.color
                                    && next.typeface == glyph.typeface
                                    && classify(next) == Class::Number
                                    && self.same_size(next, size)
                                {
                                    digits.push(next.ch);
                                    self.note_run_size(next.size, next.ch);
                                    items.pop_front();
                                } else if !digits.contains('.')
                                    && let Some(Item::Glyph(point)) = items.front()
                                    && is_decimal_point(point)
                                    && point.color == glyph.color
                                    && self.same_size(point, size)
                                    && let Some(Item::Glyph(next)) = items.get(1)
                                    && next.color == glyph.color
                                    && next.embells.is_empty()
                                    && next.ch.is_ascii_digit()
                                    && classify(next) == Class::Number
                                    && leaf_style(next) == leaf_style(&glyph)
                                    && self.same_size(next, size)
                                {
                                    // Equation Editor sets the point of a
                                    // decimal in the function style and the
                                    // digits in the number style.
                                    digits.push('.');
                                    self.note_run_size(point.size, point.ch);
                                    items.pop_front();
                                } else {
                                    break;
                                }
                            }
                            let id = self.str_node(&digits, CompactNode::Number)?;
                            if glyph.font_style.is_some() {
                                self.set_leaf_style(id, &glyph)?;
                            }
                            id
                        },
                        Class::Styled(style) => {
                            let mut run: SmallVec<[NodeId; 8]> = SmallVec::new();
                            run.push(self.plain_char_node(glyph.ch)?);
                            // Letters of a style are variables of their own:
                            // scripts and primes are the nearest one's.
                            let is_variable = glyph.typeface > 0 && glyph.font_style.is_some();
                            while let Some(Item::Glyph(next)) = items.front()
                                && next.embells.is_empty()
                                && next.color == glyph.color
                                && next.typeface == glyph.typeface
                                && classify(next) == class
                                && self.same_size(next, size)
                                && !(is_variable
                                    && (pending.is_some() || takes_scripts(items.get(1))))
                            {
                                let ch = next.ch;
                                self.note_run_size(next.size, next.ch);
                                items.pop_front();
                                run.push(self.plain_char_node(ch)?);
                            }
                            let span = self.children(&run)?;
                            self.node(CompactNode::Styled(style, span))?
                        },
                        _ => self.glyph_node(&glyph, class)?,
                    };
                    self.set_source_range(id, glyph.offset, glyph.end);
                    self.set_color(id, glyph.color);
                    self.set_size(id, size);
                    self.emit(&mut out, &mut pending, id)?;
                },
                Item::Node(id) if self.template_runs.contains(&id) => {
                    self.emit_run(&mut out, &mut pending, id)?;
                },
                Item::Node(id) => self.emit_assembled(&mut out, &mut pending, id)?,
                // A line inside a line (or the equation's own line) only
                // groups its contents.
                Item::Line(Some(id)) => {
                    for child in self.children_of(id) {
                        self.emit_assembled(&mut out, &mut pending, child)?;
                    }
                },
                Item::Line(None) => {},
                Item::Script(slots) => self.emit_script(&mut out, slots)?,
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
            // A leading script at the end of a line has nothing to lead: it
            // follows the item before it, as it is drawn.
            let id = match out.pop() {
                Some(previous) if self.carries_script(previous) => {
                    self.script_node(previous, slots)?
                },
                previous => {
                    out.extend(previous);
                    let base = self.empty_row()?;
                    self.prescript_node(base, slots)?
                },
            };
            out.push(id);
        }
        self.wrap_colors(out)
    }

    /// A script template: a script of the item before it.
    fn emit_script(&mut self, out: &mut SmallVec<[NodeId; 8]>, slots: ScriptSlots) -> Result<()> {
        let mut base = match out.pop() {
            Some(base) => base,
            None => self.empty_row()?,
        };
        // Leading scripts of nothing are how the writer spells scripts on
        // nothing (`x{}_1`); these then have the scripts.
        if let CompactNode::PreScripts {
            base: led,
            pre_sub,
            pre_sup,
        } = *self.arena.node(base)
            && self.is_empty_row(led)
        {
            let slot = |id: NodeId| (!id.is_none()).then_some(id);
            let color = self.node_colors.get(&base).copied();
            let (sub, sup) = (slot(pre_sub), slot(pre_sup));
            let nothing = self.empty_row()?;
            base = self.script_node(nothing, ScriptSlots { sub, sup, color })?;
        }
        // A leading script belongs to the whole scripted node
        // after it (`{}_a b_c` is b with both scripts), as
        // normalization builds it for every other reader: keep
        // the leading scripts outermost.
        let id = match *self.arena.node(base) {
            CompactNode::PreScripts {
                base: inner,
                pre_sub,
                pre_sup,
            } => {
                // The leading scripts and the base have one colour; where
                // the trailing scripts have another, each part keeps its own.
                let color = self.node_colors.get(&base).copied();
                self.set_color(inner, color);
                let inner = if slots.color != color && !self.script_parts(inner).is_empty() {
                    self.paint_script_tree(inner, color)?
                } else {
                    inner
                };
                let mut scripted = self.script_node(inner, slots)?;
                let mut outer = self.node_colors.get(&scripted).copied();
                let (mut pre_sub, mut pre_sup) = (pre_sub, pre_sup);
                if outer != color {
                    if outer != self.color_context {
                        scripted = if self.is_scripted_name(scripted) {
                            self.paint(scripted, outer)?
                        } else {
                            self.paint_script_tree(scripted, outer)?
                        };
                        outer = self.color_context;
                    }
                    for slot in [&mut pre_sub, &mut pre_sup] {
                        if !slot.is_none() {
                            *slot = self.paint(*slot, color)?;
                        }
                    }
                }
                let id = self.node(CompactNode::PreScripts {
                    base: scripted,
                    pre_sub,
                    pre_sup,
                })?;
                self.set_color(id, outer);
                id
            },
            _ => self.script_node(base, slots)?,
        };
        out.push(id);
        Ok(())
    }

    /// Emit a finished structure, or an item of a line already assembled:
    /// its primes and scripts have their bases.
    fn emit_assembled(
        &mut self,
        out: &mut SmallVec<[NodeId; 8]>,
        pending: &mut Option<ScriptSlots>,
        id: NodeId,
    ) -> Result<()> {
        // A leading script is the first item's of a row, where it also reads
        // back from.
        if pending.is_some()
            && matches!(*self.arena.node(id), CompactNode::Row(span) if !span.is_empty())
        {
            for child in self.children_of(id) {
                self.emit(out, pending, child)?;
            }
            return Ok(());
        }
        self.emit(out, pending, id)
    }

    /// Emit an INTOP or SUMOP template that is no big operator: its operator
    /// and its body are a run of items of the line, as they are drawn and
    /// written. A prime or script after it is its last item's, and a prime
    /// that starts it is the item's before it.
    fn emit_run(
        &mut self,
        out: &mut SmallVec<[NodeId; 8]>,
        pending: &mut Option<ScriptSlots>,
        id: NodeId,
    ) -> Result<()> {
        let color = self.node_colors.get(&id).copied();
        // One that draws nothing is no item.
        if self.is_empty_row(id) {
            return Ok(());
        }
        // A coloured part joins the line in its colour, not as one node.
        if let CompactNode::Color { content, .. } = *self.arena.node(id)
            && let Some(color) = self.painted_in(id)
            && let children = self.children_of(content)
            && self.spliced + children.len() <= (self.data.len() / 4).max(1_000)
        {
            // Each template around splices again what it holds: no more than
            // some times all of the equation.
            self.spliced += children.len();
            for child in children {
                match color {
                    Some(color) => self.node_colors.insert(child, color),
                    None => self.node_colors.remove(&child),
                };
                self.emit_run(out, pending, child)?;
            }
            return Ok(());
        }
        if matches!(*self.arena.node(id), CompactNode::Row(span) if !span.is_empty()) {
            for child in self.children_of(id) {
                if !self.node_colors.contains_key(&child) {
                    self.set_color(child, color);
                }
                self.emit_run(out, pending, child)?;
            }
            return Ok(());
        }
        if pending.is_none()
            && self.all_primes(id)
            && let Some(base) = self.take_prime_base(out, color)?
        {
            self.size_as_script(id);
            let primed = self.prime_in(base, &[id], color)?;
            out.push(primed);
            return Ok(());
        }
        // A prime with primes is that many primes.
        if pending.is_none()
            && let Some(marks) = self.prime_marks(id)
            && let Some(base) = self.take_prime_base(out, color)?
        {
            for &mark in &marks {
                self.size_as_script(mark);
            }
            let primed = self.prime_in(base, &marks, color)?;
            out.push(primed);
            return Ok(());
        }
        // The same for a prime the template drew with scripts of its own.
        if let Some((mark, sub, sup)) = self.trailing_scripts(id)
            && pending.is_none()
            && self.all_primes(mark)
            && let Some(base) = self.take_prime_base(out, color)?
        {
            self.size_as_script(mark);
            let primed = self.prime_in(base, &[mark], color)?;
            out.push(primed);
            return self.emit_script(out, ScriptSlots { sub, sup, color });
        }
        self.emit(out, pending, id)
    }

    fn emit(
        &mut self,
        out: &mut SmallVec<[NodeId; 8]>,
        pending: &mut Option<ScriptSlots>,
        id: NodeId,
    ) -> Result<()> {
        let id = match pending.take() {
            // MathType draws a leading script (`tvSU_PRECEDES`) between the
            // items around it. Before an operator, a relation, punctuation or
            // a fence it cannot lead that item; it was typed for the item
            // before it (`E {}_k +`), so it becomes that item's script.
            Some(slots) if !self.takes_leading_script(id) => {
                match out.pop() {
                    Some(previous) if self.carries_script(previous) => {
                        let scripted = self.script_node(previous, slots)?;
                        out.push(scripted);
                    },
                    previous => {
                        out.extend(previous);
                        let base = self.empty_row()?;
                        let script = self.prescript_node(base, slots)?;
                        out.push(script);
                    },
                }
                id
            },
            Some(slots) => self.prescript_node(id, slots)?,
            None => id,
        };
        out.push(id);
        Ok(())
    }

    /// Whether a leading script may lead `id`: not an operator, relation,
    /// punctuation, fence or space. A zero space keeps it as a script on
    /// nothing (how the MTEF writer spells `x{}_1`).
    fn takes_leading_script(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            // A `/` is an operator here, though ordinary in TeX.
            CompactNode::Symbol(_, s) if self.arena.str_val(s) == "/" => false,
            CompactNode::Symbol(kind, _) => !matches!(
                kind,
                SymbolKind::BinaryOp
                    | SymbolKind::Relation
                    | SymbolKind::Punctuation
                    | SymbolKind::Open
                    | SymbolKind::Close
                    | SymbolKind::Fence
            ),
            CompactNode::Space(width) => width == Rational::zero(),
            // In every real file a script before a fence template or a
            // function name is the script of the item before it.
            CompactNode::Fenced { .. } | CompactNode::Operator { .. } => false,
            // Scripts on nothing are nothing to lead.
            CompactNode::Subscript { .. }
            | CompactNode::Superscript { .. }
            | CompactNode::SubSup { .. } => self
                .trailing_scripts(id)
                .is_none_or(|(base, ..)| !self.is_empty_row(base)),
            // Leading scripts of its own come between.
            CompactNode::PreScripts { .. } => false,
            _ => true,
        }
    }

    /// Whether `id` can be the base of a script that follows it.
    fn carries_script(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            // A `/` is an operator here, though ordinary in TeX.
            CompactNode::Symbol(_, s) if self.arena.str_val(s) == "/" => false,
            CompactNode::Symbol(kind, _) => !matches!(
                kind,
                SymbolKind::BinaryOp
                    | SymbolKind::Relation
                    | SymbolKind::Punctuation
                    | SymbolKind::Open
            ),
            CompactNode::Space(_) | CompactNode::Text(..) | CompactNode::PreScripts { .. } => false,
            CompactNode::Row(span) => !span.is_empty(),
            // A labelled arrow is a relation: `\xrightarrow{…} {}^{188}Re`.
            CompactNode::Overscript { base, .. }
            | CompactNode::Underscript { base, .. }
            | CompactNode::UnderOver { base, .. } => self.carries_script(base),
            _ => true,
        }
    }

    /// Take the item a prime drawn in `color` is the superscript of. Zero
    /// spaces before the prime draw nothing and normalization drops them.
    fn take_prime_base(
        &mut self,
        out: &mut SmallVec<[NodeId; 8]>,
        color: Option<u16>,
    ) -> Result<Option<NodeId>> {
        let Some(at) = out.iter().rposition(|&id| !self.draws_nothing(id)) else {
            return Ok(None);
        };
        let base = out[at];
        let own = self.node_colors.get(&base).copied();
        // Normalization gives scripts on nothing after a function name to it.
        let name = at
            .checked_sub(1)
            .map(|before| out[before])
            .filter(|&before| {
                self.scripts_on_nothing(base).is_some()
                    && crate::ast::normalize::takes_limits(&self.arena, *self.arena.node(before))
                    && self.node_colors.get(&before).copied() == own
            });
        let carrier = name.unwrap_or(base);
        let same_color = own == color || self.nucleus_color(carrier) == Some(color);
        // Each prime may nest the base deeper: a flat line of scripts and
        // primes must not reach the depth limit.
        if !same_color
            || name.is_none() && !self.takes_prime(base)
            || self.is_long_chain(carrier)
            || self.has_many_primes(carrier)
        {
            return Ok(None);
        }
        let Some((name, (sub, sup))) = name.zip(self.scripts_on_nothing(base)) else {
            out.truncate(at);
            return Ok(Some(base));
        };
        let slots = ScriptSlots {
            sub,
            sup,
            color: own,
        };
        let base = self.script_node(name, slots)?;
        out.truncate(at - 1);
        Ok(Some(base))
    }

    /// Whether `id` has [`MAX_PRIMES`].
    fn has_many_primes(&self, mut id: NodeId) -> bool {
        while let CompactNode::PreScripts { base, .. } = *self.arena.node(id) {
            id = base;
        }
        let CompactNode::Superscript { mut sup, .. } = *self.arena.node(id) else {
            return false;
        };
        if let CompactNode::Color { content, .. } = *self.arena.node(sup) {
            sup = content;
        }
        let count = match (self.open_primes.get(&sup), *self.arena.node(sup)) {
            (Some(open), _) => open.len(),
            (None, CompactNode::Row(span)) => self.arena.children(span).len(),
            _ => 1,
        };
        count >= MAX_PRIMES
    }

    /// Whether `id` has more scripts on scripts than are read as one item's
    /// primes.
    fn is_long_chain(&self, mut id: NodeId) -> bool {
        for _ in 0..MAX_PRIMED_CHAIN {
            id = match *self.arena.node(id) {
                CompactNode::PreScripts { base, .. } => base,
                CompactNode::Color { content, .. } => content,
                _ => match self.trailing_scripts(id) {
                    Some((base, ..)) => base,
                    None => return false,
                },
            };
        }
        true
    }

    /// The colour the nucleus of a script node was painted in, when the node
    /// has another: a script template draws nothing in its own colour.
    fn nucleus_color(&self, mut id: NodeId) -> Option<Option<u16>> {
        loop {
            id = *self.script_parts(id).first()?;
            if let Some(color) = self.painted_in(id) {
                return Some(color);
            }
        }
    }

    /// The colour a colour node was painted with.
    fn painted_in(&self, id: NodeId) -> Option<Option<u16>> {
        let at = self
            .painted
            .binary_search_by_key(&id.index(), |&(node, _)| node.index())
            .ok()?;
        Some(self.painted[at].1)
    }

    /// Paint the base of scripts in another colour: scripts on nothing part
    /// by part.
    fn paint_base(&mut self, base: NodeId, color: Option<u16>) -> Result<NodeId> {
        if self.scripts_on_nothing(base).is_some() {
            self.paint_script_tree(base, color)
        } else {
            self.paint(base, color)
        }
    }

    /// Whether `id` is primes in a colour, through rows of one node.
    fn holds_colored_primes(&self, mut id: NodeId) -> bool {
        let mut is_colored = false;
        loop {
            id = match *self.arena.node(id) {
                CompactNode::Color { content, .. } => {
                    is_colored = true;
                    content
                },
                CompactNode::Row(span) if span.len() == 1 => self.arena.children(span)[0],
                _ => return is_colored && self.all_primes(id),
            };
        }
    }

    /// The prime marks of `sup` and `marks`: `sup` itself when it is open.
    fn join_primes(&mut self, sup: NodeId, marks: &[NodeId]) -> Result<NodeId> {
        if let Some(open) = self.open_primes.get_mut(&sup) {
            open.extend_from_slice(marks);
            return Ok(sup);
        }
        let mut all: Vec<NodeId> = self.children_of(sup).to_vec();
        all.extend_from_slice(marks);
        let row = self.wrap(&all)?;
        if matches!(*self.arena.node(row), CompactNode::Row(_)) {
            self.open_primes.insert(row, all);
        }
        Ok(row)
    }

    /// Give the open prime rows their marks.
    fn close_primes(&mut self) -> Result<()> {
        if self.open_primes.is_empty() {
            return Ok(());
        }
        for (row, marks) in std::mem::take(&mut self.open_primes) {
            if let CompactNode::Row(span) = *self.arena.node(row)
                && self.arena.children(span).len() < marks.len()
            {
                let span = self.children(&marks)?;
                *self.arena.node_mut(row) = CompactNode::Row(span);
            }
        }
        Ok(())
    }

    /// `base`, which `take_prime_base` gave for primes in `color`, with them.
    fn prime_in(&mut self, base: NodeId, marks: &[NodeId], color: Option<u16>) -> Result<NodeId> {
        if self.node_colors.get(&base).copied() == color {
            let id = self.prime(base, marks)?;
            self.set_color(id, color);
            return Ok(id);
        }
        // The nucleus and the primes have a colour of their own, inside the
        // node's.
        let own = self.node_colors.get(&base).copied();
        let around = std::mem::replace(&mut self.color_context, own);
        let id = self.prime_painted(base, marks, color);
        self.color_context = around;
        let id = id?;
        self.set_color(id, own);
        Ok(id)
    }

    /// `base`, whose nucleus was painted in `color`, with primes painted in it.
    fn prime_painted(
        &mut self,
        base: NodeId,
        marks: &[NodeId],
        color: Option<u16>,
    ) -> Result<NodeId> {
        match *self.arena.node(base) {
            CompactNode::PreScripts {
                base: led,
                pre_sub,
                pre_sup,
            } => {
                let base = self.prime_painted(led, marks, color)?;
                self.node(CompactNode::PreScripts {
                    base,
                    pre_sub,
                    pre_sup,
                })
            },
            CompactNode::Superscript { base: inner, sup }
                if matches!(*self.arena.node(sup), CompactNode::Color { content, .. }
                    if self.painted_in(sup) == Some(color) && self.all_primes(content)) =>
            {
                let content = match *self.arena.node(sup) {
                    CompactNode::Color { content, .. } => content,
                    _ => sup,
                };
                let joined = self.join_primes(content, marks)?;
                if joined == content {
                    return Ok(base);
                }
                let sup = self.paint(joined, color)?;
                self.node(CompactNode::Superscript { base: inner, sup })
            },
            _ => {
                let sup = self.wrap(marks)?;
                let sup = self.paint(sup, color)?;
                self.node(CompactNode::Superscript { base, sup })
            },
        }
    }

    /// Read the apostrophes of a line as primes, except the possessive or plural
    /// one of `ν_e's`: it follows an item and precedes a lone `s` that is not the
    /// start of a function name.
    fn mark_primes(&self, items: &mut [Item]) {
        let letter = |item: Option<&Item>| match item {
            Some(Item::Glyph(g)) if g.ch.is_alphabetic() => Some((g.typeface, g.ch)),
            Some(&Item::Node(id)) if self.template_runs.contains(&id) => {
                self.leading_letter(id).map(|ch| (chars::FN_FUNCTION, ch))
            },
            _ => None,
        };
        // Zero spaces do not come between (the writer adds them, and drops them).
        let after = |items: &[Item], mut i: usize| {
            while let Some(Item::Glyph(g)) = items.get(i + 1)
                && matches!(classify(g), Class::Space(width) if width == Rational::zero())
            {
                i += 1;
            }
            i + 1
        };
        for i in 0..items.len() {
            if !matches!(&items[i], Item::Glyph(g) if g.ch == '\'') {
                continue;
            }
            let first = after(items, i);
            let second = first + 1;
            let possessive = i > 0
                && letter(items.get(first))
                    .is_some_and(|(typeface, ch)| ch == 's' && typeface != chars::FN_FUNCTION)
                && letter(items.get(second)).is_none()
                // A variable: `l_1 s_1 l_1's_1'`.
                && !matches!(items.get(second), Some(Item::Script(_)))
                && !matches!(items.get(second), Some(Item::Glyph(g))
                    if (g.ch == '\'' || chars::is_prime(g.ch))
                        && !matches!(g.typeface, chars::FN_TEXT | chars::FN_TEXT_FE));
            if let Item::Glyph(glyph) = &mut items[i]
                && glyph.ch == '\''
                && !possessive
                && !matches!(glyph.typeface, chars::FN_TEXT | chars::FN_TEXT_FE)
            {
                glyph.ch = '\u{2032}';
            }
        }
    }

    /// The letter a template's run starts with.
    fn leading_letter(&self, mut id: NodeId) -> Option<char> {
        loop {
            id = match *self.arena.node(id) {
                CompactNode::Row(span) | CompactNode::Styled(_, span) => {
                    *self.arena.children(span).first()?
                },
                CompactNode::Color { content, .. } => content,
                CompactNode::Identifier(s) | CompactNode::Symbol(_, s) => {
                    let ch = self.arena.str_val(s).chars().next()?;
                    return ch.is_alphabetic().then_some(ch);
                },
                CompactNode::Operator { name, .. } => {
                    let ch = self.arena.str_val(name).chars().next()?;
                    return ch.is_alphabetic().then_some(ch);
                },
                _ => self.trailing_scripts(id)?.0,
            }
        }
    }

    /// Make leading scripts of nothing scripts on nothing and drop zero spaces
    /// at the ends of lists, as normalization does where it reaches.
    pub(super) fn settle_leading_scripts(&mut self) -> Result<()> {
        if !self.has_zero_spaces && !self.has_leading_scripts {
            return Ok(());
        }
        for raw in 0..self.arena.nodes().len() as u32 {
            let id = NodeId::new(raw);
            // The zero spaces at the ends of a list have done their part.
            if let CompactNode::Row(span) = *self.arena.node(id) {
                let is_zero = |d: &Self, id: NodeId| matches!(*d.arena.node(id), CompactNode::Space(width) if width == Rational::zero());
                let ends = match self.arena.children(span) {
                    [] => continue,
                    [first, .., last] => [*first, *last],
                    [only] => [*only; 2],
                };
                if ends.into_iter().any(|id| is_zero(self, id)) {
                    let children = self.children_of(id);
                    let mut rest = &children[..];
                    while let [first, middle @ ..] = rest
                        && is_zero(self, *first)
                    {
                        rest = middle;
                    }
                    while let [middle @ .., last] = rest
                        && is_zero(self, *last)
                    {
                        rest = middle;
                    }
                    let span = self.children(rest)?;
                    *self.arena.node_mut(id) = CompactNode::Row(span);
                }
                continue;
            }
            let CompactNode::PreScripts {
                base,
                pre_sub: sub,
                pre_sup: sup,
            } = *self.arena.node(id)
            else {
                continue;
            };
            if !self.draws_nothing(base) {
                continue;
            }
            let base = self.empty_row()?;
            *self.arena.node_mut(id) = if sup.is_none() {
                CompactNode::Subscript { base, sub }
            } else if sub.is_none() {
                CompactNode::Superscript { base, sup }
            } else {
                CompactNode::SubSup { base, sub, sup }
            };
        }
        Ok(())
    }

    /// The primes of a prime with prime embellishments.
    fn prime_marks(&self, id: NodeId) -> Option<SmallVec<[NodeId; 4]>> {
        let mut rest = id;
        let mut over: SmallVec<[NodeId; 2]> = SmallVec::new();
        while let CompactNode::Superscript { base, sup } = *self.arena.node(rest)
            && self.all_primes(sup)
        {
            over.push(sup);
            rest = base;
        }
        if over.is_empty() || !self.all_primes(rest) {
            return None;
        }
        let mut marks: SmallVec<[NodeId; 4]> = self.children_of(rest).into_iter().collect();
        for sup in over.into_iter().rev() {
            marks.extend(self.children_of(sup));
        }
        Some(marks)
    }

    /// Whether a prime typed after `id` is its superscript.
    fn takes_prime(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Symbol(kind, s) => {
                let text = self.arena.str_val(s);
                matches!(kind, SymbolKind::Ordinary | SymbolKind::Close)
                    && text != "/"
                    && !text.chars().all(chars::is_prime)
            },
            // A big operator's node includes its body.
            CompactNode::Nary { .. } => false,
            // The prime is the led item's.
            CompactNode::PreScripts { base, .. } => match *self.arena.node(base) {
                CompactNode::Color { content, .. } => self.takes_prime(content),
                _ => self.takes_prime(base),
            },
            CompactNode::Space(_) | CompactNode::Text(..) | CompactNode::Row(_) => false,
            // A coloured run of an assembled line: its last item has its
            // primes already.
            CompactNode::Color { .. } => false,
            _ => true,
        }
    }

    /// `base` with the prime marks `marks` as its superscript; primes after a
    /// primed base join its primes.
    fn prime(&mut self, base: NodeId, marks: &[NodeId]) -> Result<NodeId> {
        if let CompactNode::PreScripts {
            base: inner,
            pre_sub,
            pre_sup,
        } = *self.arena.node(base)
        {
            // `{}_a^b z′`: leading scripts stay outermost, as they do around
            // a script template.
            let base = self.prime(inner, marks)?;
            return self.node(CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            });
        }
        if let CompactNode::Superscript { base: inner, sup } = *self.arena.node(base)
            && self.all_primes(sup)
        {
            let joined = self.join_primes(sup, marks)?;
            if joined == sup {
                return Ok(base);
            }
            return self.node(CompactNode::Superscript {
                base: inner,
                sup: joined,
            });
        }
        let sup = self.wrap(marks)?;
        self.node(CompactNode::Superscript { base, sup })
    }

    /// Whether `id` is a prime mark or a row of them.
    fn all_primes(&self, id: NodeId) -> bool {
        if self.open_primes.contains_key(&id) {
            return true;
        }
        let ids = self.children_of(id);
        !ids.is_empty()
            && ids.iter().all(|&id| {
                matches!(*self.arena.node(id), CompactNode::Symbol(_, s)
                    if self.arena.str_val(s).chars().all(chars::is_prime))
            })
    }

    /// A native script or limit template determines the placement of its
    /// function-face operand, independently of the spelling of that operand.
    pub(super) fn named_with_limits(
        &mut self,
        mut id: NodeId,
        placement: LimitStyle,
    ) -> Option<NodeId> {
        while let CompactNode::Row(span) = *self.arena.node(id) {
            let &[only] = self.arena.children(span) else {
                return None;
            };
            id = only;
        }
        if let CompactNode::Operator { limits, .. } = self.arena.node_mut_in_place(id) {
            *limits = placement;
            Some(id)
        } else {
            None
        }
    }

    fn script_node(&mut self, base: NodeId, slots: ScriptSlots) -> Result<NodeId> {
        // `y′` and a subscript in another colour: one node, as in one colour
        // (`script_node_inner`).
        if let (Some(sub), None) = (slots.sub, slots.sup)
            && let CompactNode::Superscript { base: inner, sup } = *self.arena.node(base)
        {
            let color = self.node_colors.get(&base).copied();
            let is_painted = self.holds_colored_primes(sup);
            if is_painted || color != slots.color && self.all_primes(sup) {
                // A function name is coloured as one (`paint_scripts`).
                if !is_painted && matches!(*self.arena.node(inner), CompactNode::Operator { .. }) {
                    let around = std::mem::replace(&mut self.color_context, color);
                    let sub = self.paint(sub, slots.color)?;
                    self.color_context = around;
                    let id = self.node(CompactNode::SubSup {
                        base: inner,
                        sub,
                        sup,
                    })?;
                    self.set_color(id, color);
                    return Ok(id);
                }
                let base = self.paint_base(inner, color)?;
                let sup = self.paint(sup, color)?;
                let sub = self.paint(sub, slots.color)?;
                let id = self.node(CompactNode::SubSup { base, sub, sup })?;
                self.set_color(id, self.color_context);
                return Ok(id);
            }
        }
        let (base, slots, color) = self.paint_scripts(base, slots)?;
        let id = self.script_node_inner(base, slots)?;
        self.set_color(id, color);
        Ok(id)
    }

    fn script_node_inner(&mut self, base: NodeId, slots: ScriptSlots) -> Result<NodeId> {
        let base = if slots.sub.is_some() || slots.sup.is_some() {
            self.named_with_limits(base, LimitStyle::SubSup)
                .unwrap_or(base)
        } else {
            base
        };
        if let (Some(sub), None) = (slots.sub, slots.sup) {
            // `y′` then a subscript: the subscript is the primed letter's,
            // with leading scripts or without.
            match *self.arena.node(base) {
                CompactNode::Superscript { base, sup } if self.all_primes(sup) => {
                    return self.node(CompactNode::SubSup { base, sub, sup });
                },
                CompactNode::PreScripts {
                    base: led,
                    pre_sub,
                    pre_sup,
                } => {
                    if let CompactNode::Superscript { base, sup } = *self.arena.node(led)
                        && self.all_primes(sup)
                    {
                        let base = self.node(CompactNode::PreScripts {
                            base,
                            pre_sub,
                            pre_sup,
                        })?;
                        return self.node(CompactNode::SubSup { base, sub, sup });
                    }
                },
                _ => {},
            }
        }
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
        self.has_leading_scripts = true;
        let (base, slots, color) = self.paint_scripts(base, slots)?;
        let id = self.node(CompactNode::PreScripts {
            base,
            pre_sub: slots.sub.unwrap_or(NodeId::NONE),
            pre_sup: slots.sup.unwrap_or(NodeId::NONE),
        })?;
        self.set_color(id, color);
        Ok(id)
    }

    // ── Colour ─────────────────────────────────────────────────────────

    fn set_color(&mut self, id: NodeId, color: Option<u16>) {
        if let Some(color) = color {
            self.node_colors.insert(id, color);
        }
    }

    fn color_name(&self, color: Option<u16>) -> &str {
        color
            .and_then(|c| self.colors.get(usize::from(c).checked_sub(1)?))
            .and_then(Option::as_deref)
            .unwrap_or("#000000")
    }

    /// `id`, drawn in `color`, wrapped in a colour node where that differs
    /// from the colour around the line.
    fn paint(&mut self, id: NodeId, color: Option<u16>) -> Result<NodeId> {
        if color == self.color_context {
            return Ok(id);
        }
        // Content that has its own colour shows none of the colour around
        // it: only the runs between such content are painted.
        let around = self.color_name(self.color_context).to_owned();
        let name = self.color_name(color).to_owned();
        let children = self.children_of(id);
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut i = 0;
        while i < children.len() {
            if let CompactNode::Color {
                color: inner,
                content,
            } = *self.arena.node(children[i])
            {
                if self.arena.str_val(inner) == around {
                    out.extend(self.children_of(content));
                } else {
                    out.push(children[i]);
                }
                i += 1;
                continue;
            }
            if self.is_mixed_script(children[i]) {
                out.push(self.paint_script_parts(children[i], color, false)?);
                i += 1;
                continue;
            }
            let start = i;
            while i < children.len()
                && !matches!(*self.arena.node(children[i]), CompactNode::Color { .. })
                && !self.is_mixed_script(children[i])
            {
                i += 1;
            }
            let content = if start == 0 && i == children.len() {
                id
            } else {
                self.wrap(&children[start..i])?
            };
            let node = self.str_node(&name, |color| CompactNode::Color { color, content })?;
            self.painted.push((node, color));
            out.push(node);
        }
        let out = self.join_colors(out)?;
        let painted = if out.is_empty() { id } else { self.wrap(&out)? };
        self.set_color(painted, self.color_context);
        Ok(painted)
    }

    /// The base and the trailing scripts of a script node.
    fn trailing_scripts(&self, id: NodeId) -> Option<(NodeId, Option<NodeId>, Option<NodeId>)> {
        match *self.arena.node(id) {
            CompactNode::Subscript { base, sub } => Some((base, Some(sub), None)),
            CompactNode::Superscript { base, sup } => Some((base, None, Some(sup))),
            CompactNode::SubSup { base, sub, sup } => Some((base, Some(sub), Some(sup))),
            _ => None,
        }
    }

    /// The scripts of a node that has nothing else: normalization gives them
    /// to a neighbour.
    fn scripts_on_nothing(&self, id: NodeId) -> Option<(Option<NodeId>, Option<NodeId>)> {
        let (base, sub, sup) = match *self.arena.node(id) {
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => {
                let slot = |id: NodeId| (!id.is_none()).then_some(id);
                (base, slot(pre_sub), slot(pre_sup))
            },
            _ => self.trailing_scripts(id)?,
        };
        self.draws_nothing(base).then_some((sub, sup))
    }

    /// The base and scripts of a script node.
    fn script_parts(&self, id: NodeId) -> SmallVec<[NodeId; 3]> {
        match *self.arena.node(id) {
            CompactNode::Subscript { base, sub } => smallvec::smallvec![base, sub],
            CompactNode::Superscript { base, sup } => smallvec::smallvec![base, sup],
            CompactNode::SubSup { base, sub, sup } => smallvec::smallvec![base, sub, sup],
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => [base, pre_sub, pre_sup]
                .into_iter()
                .filter(|id| !id.is_none())
                .collect(),
            _ => SmallVec::new(),
        }
    }

    /// Whether `id` is a script node whose parts have colours of their own:
    /// `paint` then colours the others, as they are read when written again.
    /// (A function name must stay the direct base.)
    fn is_mixed_script(&self, id: NodeId) -> bool {
        let parts = self.script_parts(id);
        parts.first().is_some_and(|&base| {
            !matches!(*self.arena.node(base), CompactNode::Operator { .. })
                && parts.iter().any(|&part| {
                    matches!(*self.arena.node(part), CompactNode::Color { .. })
                        || self.is_mixed_script(part)
                })
        })
    }

    /// `id` with the colour of its base, when that base has scripts, painted
    /// part by part.
    fn open_colored_base(&mut self, id: NodeId) -> Result<NodeId> {
        let Some((base, ..)) = self.trailing_scripts(id) else {
            return Ok(id);
        };
        let opened = match (*self.arena.node(base), self.painted_in(base)) {
            (CompactNode::Color { content, .. }, Some(color))
                if !self.script_parts(content).is_empty() && !self.is_scripted_name(content) =>
            {
                self.paint_script_tree(content, color)?
            },
            (CompactNode::Color { .. }, _) => base,
            _ => self.open_colored_base(base)?,
        };
        if opened == base {
            return Ok(id);
        }
        let node = self.with_base(id, opened)?;
        let color = self.node_colors.get(&id).copied();
        self.set_color(node, color);
        Ok(node)
    }

    /// A copy of a node with trailing scripts that has another base.
    fn with_base(&mut self, id: NodeId, base: NodeId) -> Result<NodeId> {
        let node = match *self.arena.node(id) {
            CompactNode::Subscript { sub, .. } => CompactNode::Subscript { base, sub },
            CompactNode::Superscript { sup, .. } => CompactNode::Superscript { base, sup },
            CompactNode::SubSup { sub, sup, .. } => CompactNode::SubSup { base, sub, sup },
            other => other,
        };
        self.node(node)
    }

    /// Whether `id` is a function name with scripts.
    fn is_scripted_name(&self, id: NodeId) -> bool {
        self.trailing_scripts(id).is_some_and(|(base, ..)| {
            matches!(*self.arena.node(base), CompactNode::Operator { .. })
        })
    }

    /// Paint the parts of a script node and of the script nodes that are its
    /// base; the node itself then has the colour around it.
    fn paint_script_tree(&mut self, id: NodeId, color: Option<u16>) -> Result<NodeId> {
        let mut has_painted_base = false;
        let id = match self.trailing_scripts(id) {
            // (A function name with scripts is coloured as one.)
            Some((base, ..))
                if !self.script_parts(base).is_empty() && !self.is_scripted_name(base) =>
            {
                let base = self.paint_script_tree(base, color)?;
                has_painted_base = true;
                self.with_base(id, base)?
            },
            _ => id,
        };
        let painted = self.paint_script_parts(id, color, has_painted_base)?;
        self.node_colors.remove(&painted);
        self.set_color(painted, self.color_context);
        Ok(painted)
    }

    fn paint_script_parts(
        &mut self,
        id: NodeId,
        color: Option<u16>,
        has_painted_base: bool,
    ) -> Result<NodeId> {
        let mut painted: SmallVec<[NodeId; 3]> = SmallVec::new();
        let is_led = matches!(*self.arena.node(id), CompactNode::PreScripts { .. });
        for (at, part) in self.script_parts(id).into_iter().enumerate() {
            let part = if has_painted_base && at == 0 || self.draws_nothing(part) {
                part
            } else if is_led
                && at == 0
                && !self.script_parts(part).is_empty()
                && !self.is_scripted_name(part)
            {
                // Scripts of what leading scripts lead are painted one by one,
                // as they are where they are read (`emit_script`).
                self.paint_script_tree(part, color)?
            } else {
                self.paint(part, color)?
            };
            painted.push(part);
        }
        let base = painted[0];
        let node = match *self.arena.node(id) {
            CompactNode::Subscript { .. } => CompactNode::Subscript {
                base,
                sub: painted[1],
            },
            CompactNode::Superscript { .. } => CompactNode::Superscript {
                base,
                sup: painted[1],
            },
            CompactNode::SubSup { .. } => CompactNode::SubSup {
                base,
                sub: painted[1],
                sup: painted[2],
            },
            CompactNode::PreScripts {
                pre_sub, pre_sup, ..
            } => {
                let mut rest = painted[1..].iter().copied();
                let mut next = |slot: NodeId| {
                    if slot.is_none() {
                        slot
                    } else {
                        rest.next().unwrap_or(NodeId::NONE)
                    }
                };
                CompactNode::PreScripts {
                    base,
                    pre_sub: next(pre_sub),
                    pre_sup: next(pre_sup),
                }
            },
            _ => return Ok(id),
        };
        self.node(node)
    }

    /// The parts of a script node and the node's colour: the colour its base
    /// and scripts share, else each part is painted on its own (a black
    /// script of a red letter stays black).
    fn paint_scripts(
        &mut self,
        base: NodeId,
        mut slots: ScriptSlots,
    ) -> Result<(NodeId, ScriptSlots, Option<u16>)> {
        let color = self.node_colors.get(&base).copied();
        // An empty base draws nothing, in any colour.
        if color == slots.color || self.draws_nothing(base) {
            return Ok((base, slots, slots.color));
        }
        // A function name keeps its limits only as the direct base: colour
        // the whole node, and each script back to its own colour.
        if matches!(*self.arena.node(base), CompactNode::Operator { .. }) {
            let around = std::mem::replace(&mut self.color_context, color);
            for slot in [&mut slots.sub, &mut slots.sup] {
                if let Some(id) = *slot {
                    *slot = Some(self.paint(id, slots.color)?);
                }
            }
            self.color_context = around;
            return Ok((base, slots, color));
        }
        let base = self.paint_base(base, color)?;
        for slot in [&mut slots.sub, &mut slots.sup] {
            if let Some(id) = *slot {
                *slot = Some(self.paint(id, slots.color)?);
            }
        }
        Ok((base, slots, self.color_context))
    }

    /// Wrap each run of a line's nodes drawn in another colour than the
    /// structure around the line in a colour node.
    fn wrap_colors(&mut self, out: SmallVec<[NodeId; 8]>) -> Result<SmallVec<[NodeId; 8]>> {
        self.close_primes()?;
        let context = self.color_context;
        let color_of = |d: &Self, id: NodeId| d.node_colors.get(&id).copied();
        // Zero spaces show no colour and have done their part in the line
        // (they keep scripts and runs apart); normalization drops them, where
        // it reaches, and must not find them between or as coloured runs.
        let is_zero_space = |d: &Self, id: NodeId| matches!(*d.arena.node(id), CompactNode::Space(width) if width == Rational::zero());
        let mut out = out;
        if self.has_zero_spaces && out.iter().any(|&id| is_zero_space(self, id)) {
            let mut kept: SmallVec<[NodeId; 8]> = SmallVec::new();
            let mut space = None;
            for id in out {
                if is_zero_space(self, id) {
                    space = Some(id);
                    continue;
                }
                // (At the ends of a line they keep what the line is spliced
                // between apart.)
                if let Some(space) = space.take()
                    && kept.is_empty()
                {
                    kept.push(space);
                }
                kept.push(id);
            }
            kept.extend(space);
            out = kept;
        }
        // A node normalization gives leading scripts is painted as one that
        // has them (`emit_script`).
        let painted = !self.painted.is_empty();
        for i in 1..if painted { out.len() } else { 0 } {
            if self.trailing_scripts(out[i]).is_some()
                && self.scripts_on_nothing(out[i - 1]).is_some()
            {
                let own = color_of(self, out[i]);
                let around = std::mem::replace(&mut self.color_context, own);
                let opened = self.open_colored_base(out[i]);
                self.color_context = around;
                out[i] = opened?;
            }
        }
        if out
            .iter()
            .all(|&id| color_of(self, id) == context || is_zero_space(self, id))
        {
            return Ok(out);
        }
        // Normalization gives scripts on nothing after a function name to it.
        let mut items: SmallVec<[NodeId; 8]> = SmallVec::new();
        for id in out {
            if is_zero_space(self, id) {
                continue;
            }
            if let Some(&name) = items.last()
                && matches!(*self.arena.node(name), CompactNode::Operator { .. })
                && let Some((sub, sup)) = self.scripts_on_nothing(id)
                && color_of(self, name) == color_of(self, id)
            {
                let color = color_of(self, id);
                items.pop();
                items.push(self.script_node(name, ScriptSlots { sub, sup, color })?);
            } else {
                items.push(id);
            }
        }
        let out = items;
        // Normalization makes scripts on nothing the leading scripts of the
        // node after them, which would be a whole run of one colour: here
        // they lead its first item, as they do when written again.
        let mut led: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut items = out.into_iter().peekable();
        while let Some(id) = items.next() {
            let Some((sub, sup)) = self.scripts_on_nothing(id) else {
                led.push(id);
                continue;
            };
            let Some(&next) = items.peek() else {
                led.push(id);
                continue;
            };
            let color = color_of(self, next);
            if color == context || !can_take_prescripts(*self.arena.node(next)) {
                led.push(id);
                continue;
            }
            items.next();
            // Scripts of its own are painted one by one, as `emit_script`
            // paints them.
            let primed = items.peek().is_some_and(|&mark| {
                self.all_primes(mark) && color_of(self, mark) == color_of(self, id)
            });
            let next = if !self.script_parts(next).is_empty() && color_of(self, id) != color {
                if primed && self.is_scripted_name(next) {
                    self.paint(next, color)?
                } else {
                    self.paint_script_tree(next, color)?
                }
            } else {
                next
            };
            if !self.takes_leading_script(next) {
                // Normalization makes the two one node.
                led.push(id);
                led.push(next);
                continue;
            }
            let slots = ScriptSlots {
                sub,
                sup,
                color: color_of(self, id),
            };
            let mut id = self.prescript_node(next, slots)?;
            // A prime after them is now the led item's.
            while let Some(&mark) = items.peek()
                && self.all_primes(mark)
                && color_of(self, mark) == color_of(self, id)
                && self.takes_prime(id)
            {
                items.next();
                let primed = self.prime(id, &[mark])?;
                self.set_color(primed, color_of(self, id));
                id = primed;
            }
            led.push(id);
        }
        let out = led;
        let mut wrapped: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut i = 0;
        while i < out.len() {
            let color = color_of(self, out[i]);
            let mut end = i + 1;
            // A zero space shows no colour, and normalization drops it.
            while end < out.len()
                && (color_of(self, out[end]) == color || self.draws_nothing(out[end]))
            {
                end += 1;
            }
            if color == context {
                wrapped.extend_from_slice(&out[i..end]);
            } else {
                let content = self.wrap(&out[i..end])?;
                wrapped.push(self.paint(content, color)?);
            }
            i = end;
        }
        self.join_colors(wrapped)
    }

    /// A part painted on its own (the base of a script in another colour)
    /// joins its neighbours of the same colour.
    fn join_colors(&mut self, wrapped: SmallVec<[NodeId; 8]>) -> Result<SmallVec<[NodeId; 8]>> {
        let is_color = |id: NodeId| matches!(*self.arena.node(id), CompactNode::Color { .. });
        if !wrapped
            .windows(2)
            .any(|pair| is_color(pair[0]) && is_color(pair[1]))
        {
            return Ok(wrapped);
        }
        let context = self.color_context;
        let mut merged: SmallVec<[NodeId; 8]> = SmallVec::new();
        let mut i = 0;
        while i < wrapped.len() {
            let color_at = |d: &Self, i: usize| match wrapped.get(i).map(|&id| *d.arena.node(id)) {
                Some(CompactNode::Color { color, .. }) => Some(d.arena.str_val(color).to_owned()),
                _ => None,
            };
            let name = color_at(self, i);
            let mut end = i + 1;
            while name.is_some() && color_at(self, end) == name {
                end += 1;
            }
            if end - i == 1 {
                merged.push(wrapped[i]);
            } else {
                let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
                for &id in &wrapped[i..end] {
                    if let CompactNode::Color { content, .. } = *self.arena.node(id) {
                        ids.extend(self.children_of(content));
                    }
                }
                let content = self.wrap(&ids)?;
                let name = name.unwrap_or_default();
                let node = self.str_node(&name, |color| CompactNode::Color { color, content })?;
                self.set_color(node, context);
                merged.push(node);
            }
            i = end;
        }
        Ok(merged)
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
        let id = match class {
            Class::Text(style) => self.char_node(ch, |s| CompactNode::Text(style, s)),
            Class::Function => {
                let id = self.char_node(ch, |s| CompactNode::Operator {
                    name: s,
                    // This fallback is a single function glyph, not a complete
                    // native command name; it cannot denote one of the known
                    // limit functions.
                    limits: LimitStyle::SubSup,
                })?;
                if glyph.font_style.is_some() {
                    self.set_leaf_style(id, glyph)?;
                }
                Ok(id)
            },
            Class::Number => {
                let id = self.char_node(ch, CompactNode::Number)?;
                if glyph.font_style.is_some() {
                    self.set_leaf_style(id, glyph)?;
                }
                Ok(id)
            },
            Class::Ident => self.char_node(ch, CompactNode::Identifier),
            Class::Styled(style) => {
                let inner = self.plain_char_node(ch)?;
                let span = self.children(&[inner])?;
                self.node(CompactNode::Styled(style, span))
            },
            Class::Symbol(kind) => {
                let id = self.char_node(ch, |s| CompactNode::Symbol(kind, s))?;
                if glyph.font_style.is_some() {
                    self.set_leaf_style(id, glyph)?;
                }
                Ok(id)
            },
            Class::Space(width) => {
                self.has_zero_spaces |= width == Rational::zero();
                self.node(CompactNode::Space(width))
            },
            Class::Marker => self.empty_row(),
        }?;
        self.set_source_range(id, glyph.offset, glyph.end);
        Ok(id)
    }

    /// Apply one embellishment (MTEFv5.md "EMBELL record"; MTEF 3 defines
    /// types 2 to 20 with the same meaning).
    fn embellish(&mut self, base: NodeId, kind: u8, offset: usize) -> Result<NodeId> {
        self.check_v2_embellishment(offset, kind)?;
        if !self.v5 && kind > 20 {
            self.recover(
                offset,
                format!("unknown MTEF 3 embellishment type {kind}; it was dropped"),
            )?;
            return Ok(base);
        }
        if let Some(primes) = primes_of(&[(kind, offset)]) {
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

/// A dimension of the equation preferences in points: units are inches,
/// centimetres, points and picas (a percentage is no length).
fn dimension_points(unit: u8, digits: &str) -> Option<Rational> {
    let (num, den) = match unit {
        0 => (72, 1),
        1 => (3600, 127),
        2 => (1, 1),
        3 => (12, 1),
        _ => return None,
    };
    let size = Rational::from_scaled_decimal(digits, Rational::new(num, den))?;
    // No font size is larger, and the digits written hold up to it.
    if size.num <= 0 || size.as_f64() > 10_000.0 {
        return None;
    }
    if unit == 2 && size.den <= 1000 {
        return Some(size);
    }
    // One that has no short decimal in points, which is what is written:
    // in 32nds of a point, which have.
    let units = (size.as_f64() * 32.0).round();
    (units >= 1.0 && units <= f64::from(i32::MAX)).then(|| Rational::new(units as i32, 32))
}

/// Bold (1) and italic (2) of the typeface styles 1 to 9 as MathType defines
/// them and as they are read without a definition; USER1 is read as variables.
const OWN_STYLES: [u8; chars::FN_USER1 as usize] = [0, 0, 2, 2, 0, 0, 1, 0, 2];

/// Whether a glyph is the point of a decimal number: a full stop outside the
/// text styles.
fn is_decimal_point(glyph: &Glyph) -> bool {
    glyph.ch == '.'
        && glyph.embells.is_empty()
        && !matches!(glyph.typeface, chars::FN_TEXT | chars::FN_TEXT_FE)
}

/// How a character takes part in its line, from its typeface and value.
fn classify(glyph: &Glyph) -> Class {
    if let Some(style) = glyph.alphabet {
        return Class::Styled(style);
    }
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
        chars::FN_TEXT | chars::FN_TEXT_FE => {
            return Class::Text(glyph.font_style.map_or(TextStyle::Normal, letter_style));
        },
        _ => {},
    }
    if chars::is_marker(code) {
        return Class::Marker;
    }
    if let Some(width) = (code >= 0xE000).then(|| chars::space_width(code)).flatten() {
        return Class::Space(width);
    }
    // Within a function-face run, U+2009 is part of the operator name.
    // Standalone mathematical spacing is encoded by FN_SPACE above.
    if ch == '\u{2009}' && glyph.typeface == chars::FN_FUNCTION {
        return Class::Function;
    }
    // A symbol-face U+2009 is a literal glyph, while FN_SPACE records are
    // actual MathType glue. Preserve the ordinary atom class for authored
    // character-code spaces emitted by the MTEF writer.
    if glyph.typeface == chars::FN_SYMBOL && crate::unicode::math_space_width(ch).is_some() {
        return Class::Symbol(chars::symbol_kind(ch));
    }
    if ch.is_whitespace() {
        return Class::Space(Rational::new(1, 4));
    }
    if glyph.typeface < 0
        && let Some(style) = glyph.font_style
    {
        if ch.is_ascii_digit() {
            return Class::Number;
        }
        if ch.is_alphabetic() {
            return match letter_style(style) {
                TextStyle::Italic => Class::Ident,
                style => Class::Styled(style),
            };
        }
        return Class::Symbol(chars::symbol_kind(ch));
    }
    match glyph.typeface {
        chars::FN_FUNCTION if ch.is_alphabetic() => Class::Function,
        chars::FN_VARIABLE | chars::FN_LCGREEK | chars::FN_UCGREEK | chars::FN_USER1
            if ch.is_alphabetic() =>
        {
            match glyph.font_style {
                Some(style) => Class::Styled(letter_style(style)),
                None => Class::Ident,
            }
        },
        chars::FN_VECTOR if ch.is_alphanumeric() => Class::Styled(TextStyle::Bold),
        chars::FN_NUMBER if ch.is_ascii_digit() || ch == '.' => Class::Number,
        _ if ch.is_ascii_digit() => Class::Number,
        _ if ch.is_alphabetic() => Class::Ident,
        _ => Class::Symbol(chars::symbol_kind(ch)),
    }
}

/// The style of a number, symbol or function name drawn bold or italic.
///
/// Symbols are as `\boldsymbol` sets them: `\mathbf` is for letters and digits.
fn leaf_style(glyph: &Glyph) -> Option<TextStyle> {
    let style = glyph.font_style?;
    if glyph.typeface == chars::FN_FUNCTION {
        return if style.bold {
            Some(TextStyle::Bold)
        } else {
            (style.italic).then_some(TextStyle::Italic)
        }
        .filter(|_| glyph.ch.is_alphabetic());
    }
    if !style.bold {
        return None;
    }
    let explicit = glyph.typeface < 0;
    if glyph.ch.is_ascii_digit() {
        (explicit || matches!(glyph.typeface, chars::FN_NUMBER | chars::FN_USER1))
            .then_some(TextStyle::Bold)
    } else {
        ((explicit || glyph.typeface == chars::FN_SYMBOL) && chars::is_bold_symbol(glyph.ch))
            .then_some(TextStyle::BoldItalic)
    }
}

fn letter_style(style: FontStyle) -> TextStyle {
    match (style.bold, style.italic) {
        (false, false) => TextStyle::Normal,
        (true, false) => TextStyle::Bold,
        (false, true) => TextStyle::Italic,
        (true, true) => TextStyle::BoldItalic,
    }
}

/// Whether what precedes `item` on its line is its base.
fn takes_scripts(item: Option<&Item>) -> bool {
    match item {
        Some(Item::Script(_)) => true,
        Some(Item::Glyph(glyph)) => chars::is_prime(glyph.ch),
        _ => false,
    }
}

/// The number of primes embellishments draw, when they are all primes.
fn primes_of(embells: &[(u8, usize)]) -> Option<usize> {
    embells
        .iter()
        .map(|&(kind, _)| match kind {
            5 => Some(1),
            6 => Some(2),
            18 => Some(3),
            _ => None,
        })
        .sum()
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
