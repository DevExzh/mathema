//! Record-level encoding of the compact AST as MTEF 3 or MTEF 5.
//!
//! The encoder makes one recursive pass over the AST and writes each node as
//! the records the reader turns back into that node: characters for leaves,
//! templates for structures, piles and matrices for arrays. It follows the
//! layout MathType and Equation Editor themselves write: typesize records
//! are emitted only where the size changes (before the next record that
//! needs it), fence, operator and arrow templates carry their characters
//! after the slots, and absent slots are null lines.
//!
//! Bounds: recursion is limited by [`Config::max_depth`] and the number of
//! nodes by [`Config::max_nodes`]; the output grows linearly with the input
//! (spaces are split into at most [`MAX_SPACE_RECORDS`] records).

use smallvec::SmallVec;

use super::chars::{
    self, DIRAC_BAR, DIRAC_LEFT, DIRAC_RIGHT, FN_EXPAND, FN_FUNCTION, FN_NUMBER, FN_SPACE, FN_TEXT,
    FN_UCGREEK, FN_VARIABLE, FN_VECTOR, Fence, Glyph,
};
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::normalize::{can_take_prescripts, takes_limits};
use crate::ast::style::MathStyle;
use crate::ast::{
    AccentPosition, ColumnAlign, DisplayStyle, FontSize, FractionKind, LimitStyle, PhantomKind,
    Rational, SpanBoundary, StrokeKind, TextStyle, TypeSize,
};
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, LimitKind, Result, Strictness};
use crate::reader::mtef::chars::{is_bold_symbol, negated};

// Record types (MTEFv3.md "Tag byte structure", MTEFv5.md "Record types").
const END: u8 = 0;
const LINE: u8 = 1;
const CHAR: u8 = 2;
const TMPL: u8 = 3;
const PILE: u8 = 4;
const MATRIX: u8 = 5;
const EMBELL: u8 = 6;
/// FONT in MTEF 3, FONT_STYLE_DEF in MTEF 5.
const FONT: u8 = 8;
const SIZE: u8 = 9;
const COLOR: u8 = 15;
const COLOR_DEF: u8 = 16;
/// Distinct colours written before further ones are dropped.
const MAX_COLORS: usize = 4096;
const SYMBOL_SIZE_LOSS: &str = "symbol sizes of characters were dropped";
const V3_COLOR_LOSS: &str = "MTEF 3 has no colours; the content was written without colour";
/// The typesize records FULL (10) to SUBSYM (14) are 10 + the typesize.
const TYPESIZE_BASE: u8 = 10;

const OPT_LINE_NULL: u8 = 0x01;
// CHAR options, MTEF 5.
const V5_CHAR_EMBELL: u8 = 0x01;
const V5_CHAR_FUNC_START: u8 = 0x02;
const V5_CHAR_ENC_CHAR_8: u8 = 0x04;
// CHAR options, MTEF 3 (`xfAUTO`, `xfEMBELL`).
const V3_CHAR_AUTO: u8 = 0x01;
const V3_CHAR_EMBELL: u8 = 0x02;

/// MTEF 5 limit variation bits as MathType writes them (MTEFv5.md "Limit
/// variations" gives 0x01 and 0x02; real files use 0x10 and 0x20).
const TV_LOWER: u16 = 0x10;
const TV_UPPER: u16 = 0x20;
const TV_SUM: u16 = 0x40;

/// Font definition MathType's default preferences use for text (font 1,
/// Times New Roman); explicit upright and bold-italic letters use it.
const TEXT_FONT_DEF: u8 = 1;
// The typeface styles whose definition the writer chooses.
const DEFINED_TEXT: usize = 0;
const DEFINED_FUNCTION: usize = 1;
const DEFINED_GREEK: usize = 2;
/// The font name of explicit fonts in MTEF 3 FONT records.
const TEXT_FONT_NAME: &[u8] = b"Times New Roman";

/// Upper bound on the space records written for one space node.
pub(super) const MAX_SPACE_RECORDS: usize = 32;

/// MTEF typesizes (MTEFv5.md "Typesize values").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Full = 0,
    Sub = 1,
    Sub2 = 2,
    Sym = 3,
    SubSym = 4,
    User1 = 5,
    User2 = 6,
}

/// A size in effect, as the reader keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeRecord {
    Level(Size),
    /// A typesize and what is added to it, in 32nds of a point.
    Delta(Size, i16),
    /// 32nds of a point.
    Points(i16),
}

/// What the AST calls a typesize.
fn type_size(size: Size) -> TypeSize {
    match size {
        Size::Full => TypeSize::Full,
        Size::Sub => TypeSize::Sub,
        Size::Sub2 => TypeSize::Sub2,
        Size::Sym => TypeSize::Sym,
        Size::SubSym => TypeSize::SubSym,
        Size::User1 => TypeSize::User1,
        Size::User2 => TypeSize::User2,
    }
}

/// The character that the reader takes for an accent when it is alone above a
/// character in the limit template, where it can have a size of its own.
fn limit_mark(accent: char) -> Option<char> {
    Some(match accent {
        '\u{0303}' => '\u{223C}',
        '\u{0302}' => '\u{02C6}',
        '\u{20D7}' => '\u{2192}',
        '\u{0307}' => '\u{02D9}',
        '\u{0304}' => '\u{00AF}',
        _ => return None,
    })
}

/// One character under embellishments.
struct Embellished {
    leaf: NodeId,
    /// The innermost node around or at the character with a size, below the
    /// outermost embellishment.
    sized: Option<NodeId>,
    /// The innermost math alphabet inside an accent.
    style: Option<TextStyle>,
    /// The embellishment types, innermost first.
    kinds: SmallVec<[u8; 4]>,
}

/// The font size of the node being written.
#[derive(Debug, Clone, Copy)]
struct OwnSize {
    size: FontSize,
    /// Whether it is an attribute, not the base size of the equation.
    attribute: bool,
    /// The typesize of the node's own text, which has the font size; other
    /// typesizes inside it keep their proportion.
    place: Size,
}

impl Size {
    /// The size of scripts and limits attached to content of this size.
    fn script(self) -> Size {
        match self {
            Size::Full | Size::User1 | Size::User2 => Size::Sub,
            Size::Sym => Size::SubSym,
            _ => Size::Sub2,
        }
    }

    /// The size of a big operator's sign in content of this size.
    fn symbol(self) -> Size {
        match self {
            Size::Full | Size::Sym | Size::User1 | Size::User2 => Size::Sym,
            _ => Size::SubSym,
        }
    }
}

/// Characters the reader joins into one node when they follow each other:
/// two nodes whose glyphs share a key need a separator between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Number(i32),
    /// A point that is its own node after a number: the reader would take it
    /// and the digits after it into the number.
    Point,
    Text,
    Function,
    Styled(i32),
}

/// How the glyphs of a leaf are chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Number,
    Identifier,
    Operator,
    Symbol,
    Text,
}

pub(super) struct Encoder<'a> {
    arena: &'a AstArena,
    v5: bool,
    display: DisplayStyle,
    pub(super) out: Vec<u8>,
    /// The size in effect at the end of `out` (`None` before the first).
    size: Option<SizeRecord>,
    /// The font size of the node being written, if it is not a typesize.
    own_size: Option<OwnSize>,
    /// Whether what is written is in a root's index.
    in_index: bool,
    /// Whether the character being written is a separator.
    separator: bool,
    /// Whether a full size in a smaller slot has a record of its own.
    says_full: bool,
    /// Whether typesizes are not the sizes they stand for.
    absolute: bool,
    /// Whether any node has attributes.
    sized: bool,
    /// The size of the last glyph.
    glyph_size: Option<SizeRecord>,
    /// The size of ordinary text in points.
    base_pt: f64,
    strict: bool,
    max_depth: usize,
    max_nodes: Option<usize>,
    depth: usize,
    /// The deepest nesting written, of AST nodes or of object lists.
    deepest: usize,
    nodes: usize,
    pub(super) diagnostics: Vec<Diagnostic>,
    /// Typeface values of the explicit fonts defined so far, by style bits
    /// (bit 0 bold, bit 1 italic).
    explicit: [Option<i32>; 4],
    explicit_count: i32,
    /// The bold (1) and italic (2) bits of the Text and Function styles and of
    /// upper-case Greek, in that order: `Some(None)` where they differ.
    defined: [Option<Option<u8>>; 3],
    /// Whether punctuation was written in the Function style.
    has_function_punctuation: bool,
    /// Whether punctuation is written in an explicit font instead.
    pub(super) punctuation_font: bool,
    /// The style of the text being written.
    text_style: TextStyle,
    /// The run the last written glyph belongs to.
    last_run: Option<Run>,
    /// The colour it was drawn in.
    last_color: Option<u16>,
    /// Whether the number run last written has a decimal point.
    decimal: bool,
    /// Records that produce an item in the reader's line (CHAR, TMPL, PILE,
    /// MATRIX), counted to find bases that write nothing.
    items: usize,
    /// The records whose object lists are open, innermost last: whether
    /// each is a LINE, and the item count when it was opened.
    open: Vec<(bool, usize)>,
    /// The item count and number of open lists right after the last zero
    /// space: both unchanged while it is the last item of its list.
    zero_space: Option<(usize, usize)>,
    /// The place after an apostrophe, and after the `s` that follows one.
    apostrophe: Option<(usize, usize)>,
    possessive_s: Option<(usize, usize)>,
    /// Set when the object lists nest deeper than the reader accepts.
    too_deep: bool,
    /// Set when Far East text was written (MTEF 5 then defines its font).
    pub(super) far_east: bool,
    /// Set when a Fraktur or script letter was written (MTEF 5 then defines
    /// the Euclid Fraktur and Euclid Math One fonts).
    pub(super) alphabets: bool,
    /// Typeface values of the explicit Euclid Fraktur and Euclid Math One
    /// fonts, plain and bold, once defined.
    alphabet_typefaces: [[Option<i32>; 2]; 2],
    /// The item count when the innermost `Styled` node began: its own
    /// letters continue one run and need no separator.
    styled_start: Option<usize>,
    /// Whether a number is all that the innermost style is around.
    lone_styled: bool,
    /// The style of an enclosing `Styled` node.
    style: Option<TextStyle>,
    /// MTEF 5 COLOR_DEF records written so far, as RGB components.
    colors: Vec<[u8; 3]>,
    /// The COLOR_DEF index in effect (`None` until a colour is written).
    color: Option<u16>,
    /// Effective TeX math style used to select lossy style-choice branches.
    /// MTEF has no mathchoice record, so the encoder must resolve each choice
    /// using the style of its position before writing the selected branch.
    math_style: MathStyle,
}

impl<'a> Encoder<'a> {
    pub(super) fn new(
        arena: &'a AstArena,
        config: &Config,
        v5: bool,
        display: DisplayStyle,
        base_pt: f64,
    ) -> Self {
        Self {
            arena,
            v5,
            display,
            out: Vec::with_capacity(256),
            size: None,
            own_size: None,
            sized: arena.has_attrs(),
            absolute: false,
            in_index: false,
            says_full: false,
            separator: false,
            glyph_size: None,
            base_pt,
            strict: config.strictness == Strictness::Strict,
            max_depth: config.effective_max_depth(),
            max_nodes: config.max_nodes,
            depth: 0,
            deepest: 0,
            nodes: 0,
            diagnostics: Vec::new(),
            explicit: [None; 4],
            explicit_count: 0,
            defined: [None; 3],
            has_function_punctuation: false,
            punctuation_font: false,
            text_style: TextStyle::Normal,
            last_run: None,
            last_color: None,
            decimal: false,
            items: 0,
            open: Vec::new(),
            zero_space: None,
            apostrophe: None,
            possessive_s: None,
            too_deep: false,
            far_east: false,
            alphabets: false,
            alphabet_typefaces: [[None; 2]; 2],
            styled_start: None,
            lone_styled: false,
            style: None,
            colors: Vec::new(),
            color: None,
            math_style: display.into(),
        }
    }

    fn with_math_style<T>(
        &mut self,
        math_style: MathStyle,
        write: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = self.math_style;
        self.math_style = math_style;
        let result = write(self);
        self.math_style = previous;
        result
    }

    // ── Diagnostics and limits ─────────────────────────────────────────

    fn lose(&mut self, message: &str) {
        self.report(DiagnosticKind::Warning, message);
    }

    /// What is lost does not show by itself.
    fn inform(&mut self, message: &str) {
        self.report(DiagnosticKind::Info, message);
    }

    fn report(&mut self, kind: DiagnosticKind, message: &str) {
        if !self.diagnostics.iter().any(|d| d.message == message) {
            self.diagnostics.push(Diagnostic {
                kind,
                message: message.to_owned(),
                span: None,
            });
        }
    }

    /// Whether the reading of what was written could exceed a limit that
    /// this tree does not.
    pub(super) fn is_near_limits(&self) -> bool {
        self.deepest > self.max_depth / 2 || self.max_nodes.is_some_and(|max| self.nodes > max / 2)
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        self.deepest = self.deepest.max(self.depth).max(self.open.len());
        if self.depth > self.max_depth || self.too_deep {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: self.max_depth,
            });
        }
        self.nodes += 1;
        if let Some(limit) = self.max_nodes
            && self.nodes > limit
        {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Nodes,
                limit,
            });
        }
        Ok(())
    }

    // ── Primitive writes ───────────────────────────────────────────────

    fn u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a typesize record when `size` is not already in effect.
    #[inline]
    fn need(&mut self, size: Size) {
        if self.sized {
            return self.write_size(size);
        }
        if !matches!(self.size, Some(SizeRecord::Level(current)) if current == size) {
            self.out.push(TYPESIZE_BASE + size as u8);
            self.size = Some(SizeRecord::Level(size));
        }
    }

    #[cold]
    #[inline(never)]
    fn write_size(&mut self, size: Size) {
        let record = self.record(size);
        if self.size == Some(record) {
            return;
        }
        match record {
            SizeRecord::Level(size) if (size as u8) <= Size::SubSym as u8 => {
                self.out.push(TYPESIZE_BASE + size as u8);
            },
            SizeRecord::Level(size) => self.out.extend([SIZE, size as u8, 128]),
            SizeRecord::Delta(size, delta) => match u8::try_from(i32::from(delta) + 128) {
                Ok(delta) if delta != 0 => self.out.extend([SIZE, size as u8, delta]),
                _ => {
                    self.out.extend([SIZE, 100, size as u8]);
                    self.out.extend_from_slice(&delta.to_le_bytes());
                },
            },
            SizeRecord::Points(points) => {
                // The size itself, as the applications write it: see the reader.
                self.out.extend([SIZE, 101]);
                self.out.extend_from_slice(&points.to_le_bytes());
            },
        }
        self.size = Some(record);
    }

    /// Write a node's sign in the size it has of its own.
    #[inline(always)]
    fn with_sign_size(
        &mut self,
        id: NodeId,
        size: Size,
        write: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        if !self.sized {
            return write(self);
        }
        let outer = self.own_size;
        if let Some(own) = self.arena.attrs(id).and_then(|attrs| attrs.glyph_size) {
            self.own_size = Some(OwnSize {
                size: own,
                attribute: true,
                place: size,
            });
        }
        let result = write(self);
        self.own_size = outer;
        result
    }

    fn points(&self, points: f64) -> FontSize {
        FontSize::Points(
            Rational::from_scaled_decimal(&format!("{points:.4}"), Rational::one())
                .unwrap_or_else(Rational::one),
        )
    }

    /// The record for `size` where it is used now.
    fn record(&self, size: Size) -> SizeRecord {
        match self.own_size {
            None => SizeRecord::Level(size),
            Some(own) => self.own_record(own, size),
        }
    }

    /// The size of what has typesize `size` in a node with a font size.
    #[cold]
    fn own_record(&self, own: OwnSize, size: Size) -> SizeRecord {
        let per_mille = |size: Size| f64::from(type_size(size).per_mille());
        let units = |points: f64| (points * 32.0).round().clamp(1.0, f64::from(i16::MAX)) as i16;
        let proportion = per_mille(size) / per_mille(own.place);
        match own.size {
            FontSize::Points(points) => SizeRecord::Points(units(points.as_f64() * proportion)),
            FontSize::Logical { delta_pt, .. } if size == own.place => {
                let delta = (delta_pt.as_f64() * 32.0)
                    .round()
                    .clamp(-f64::from(i16::MAX), f64::from(i16::MAX));
                SizeRecord::Delta(size, delta as i16)
            },
            size => SizeRecord::Points(units(size.scale(self.base_pt) * self.base_pt * proportion)),
        }
    }

    fn is_character(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Number(_)
            | CompactNode::Identifier(_)
            | CompactNode::Text(..)
            | CompactNode::Symbol(..)
            | CompactNode::Operator { .. } => true,
            CompactNode::Styled(_, span) => self
                .arena
                .children(span)
                .iter()
                .all(|&id| self.is_character(id)),
            CompactNode::Color { content, .. } => self.is_character(content),
            _ => false,
        }
    }

    /// Whether a node is set in a symbol size, which the reader does not
    /// keep.
    fn has_symbol_size(&self, id: NodeId) -> bool {
        matches!(
            self.arena.attrs(id).and_then(|attrs| attrs.size),
            Some(FontSize::Logical { level: TypeSize::Sym | TypeSize::SubSym, delta_pt }) if delta_pt.num == 0
        )
    }

    /// The size and typesize to write a node in, where `size` is the typesize
    /// of its place.
    #[cold]
    #[inline(never)]
    fn own_size_of(&self, id: NodeId, size: Size) -> (Option<OwnSize>, Size) {
        let Some(font_size) = self.arena.attrs(id).and_then(|attrs| attrs.size) else {
            return (self.own_size, size);
        };
        if self.has_symbol_size(id) {
            return (self.own_size, size);
        }
        match font_size {
            FontSize::Logical { level, delta_pt } => {
                let level = match level {
                    TypeSize::Full => Size::Full,
                    TypeSize::Sub => Size::Sub,
                    TypeSize::Sub2 => Size::Sub2,
                    TypeSize::Sym => Size::Sym,
                    TypeSize::SubSym => Size::SubSym,
                    TypeSize::User1 => Size::User1,
                    TypeSize::User2 => Size::User2,
                };
                // Equation Editor has no user sizes.
                let known =
                    !self.absolute && (self.v5 || !matches!(level, Size::User1 | Size::User2));
                // The scripts of a structure shrink from its size, those of a
                // typesize are the next one; a root's index has no typesize
                // in the AST, which a difference from one does not need.
                let in_index = self.in_index && level == Size::Sub && delta_pt.num == 0;
                let whole = self.is_character(id) && !in_index;
                if !(known && whole) {
                    let size_pt = self.points(font_size.scale(self.base_pt) * self.base_pt);
                    let own = OwnSize {
                        size: size_pt,
                        attribute: true,
                        place: size,
                    };
                    (Some(own), size)
                } else if delta_pt.num == 0 {
                    (None, level)
                } else {
                    let own = OwnSize {
                        size: font_size,
                        attribute: true,
                        place: level,
                    };
                    (Some(own), level)
                }
            },
            font_size => (
                Some(OwnSize {
                    size: font_size,
                    attribute: true,
                    place: size,
                }),
                size,
            ),
        }
    }

    /// A record tag. MTEF 5 writes an options byte after the record types
    /// that have one; MTEF 3 packs the options in the tag's high nibble.
    fn tag(&mut self, rtype: u8, options: u8) {
        if self.v5 {
            self.out.push(rtype);
            if matches!(rtype, LINE | CHAR | TMPL | PILE | MATRIX | EMBELL) {
                self.out.push(options);
            }
        } else {
            self.out.push(rtype | (options << 4));
        }
        self.last_run = None;
        if matches!(rtype, CHAR | TMPL | PILE | MATRIX) {
            self.items += 1;
        }
        match rtype {
            LINE if options & OPT_LINE_NULL == 0 => self.open.push((true, self.items)),
            TMPL | PILE | MATRIX => self.open.push((false, self.items)),
            _ => {},
        }
        // The reader reads one object list per open record plus the
        // equation's own list, and refuses more than `max_depth` of them;
        // a template takes two lists (its own and a slot's) per AST level.
        if self.open.len() >= self.max_depth {
            self.too_deep = true;
        }
    }

    /// END of the innermost open object list.
    fn end(&mut self) {
        self.out.push(END);
        self.open.pop();
        self.apostrophe = None;
        self.possessive_s = None;
        self.last_run = None;
    }

    /// Whether the innermost open line already holds an item.
    fn line_has_items(&self) -> bool {
        self.open
            .iter()
            .rev()
            .find(|&&(line, _)| line)
            .is_some_and(|&(_, start)| self.items > start)
    }

    fn template(&mut self, size: Size, selector: u8, variation: u16) {
        self.need(size);
        self.tag(TMPL, 0);
        self.out.push(selector);
        if self.v5 && variation >= 0x80 {
            // MTEFv5.md "TMPL record": two bytes when the high bit is set.
            self.out.push((variation & 0x7F) as u8 | 0x80);
            self.out.push((variation >> 8) as u8);
        } else {
            self.out.push(variation as u8);
        }
        // Template-specific options: fixed-size integrals and the default
        // fence alignment, as the applications write for new templates.
        self.out.push(0);
    }

    fn null_line(&mut self, size: Size) {
        self.need(size);
        self.tag(LINE, OPT_LINE_NULL);
    }

    /// A template slot: a null line for an absent slot, else a line.
    fn slot(&mut self, size: Size, id: Option<NodeId>) -> Result<()> {
        match id {
            None => self.null_line(size),
            // A slot holding only a one-column array is the PILE itself, as
            // Equation Editor and MathType write it.
            Some(id) if self.is_pile(self.unwrap_row(id)) => {
                self.enter()?;
                self.sized_pile(self.unwrap_row(id), size)?;
                self.depth -= 1;
            },
            Some(id) => {
                self.need(size);
                self.tag(LINE, 0);
                self.content(id, size)?;
                self.end();
            },
        }
        Ok(())
    }

    /// A slot whose content is empty is written as a null line, as MathType
    /// does for an empty integrand.
    fn slot_or_null(&mut self, size: Size, id: NodeId) -> Result<()> {
        if self.is_empty(id) {
            self.null_line(size);
            Ok(())
        } else {
            self.slot(size, Some(id))
        }
    }

    /// The contents of a line holding `id`.
    fn content(&mut self, id: NodeId, size: Size) -> Result<()> {
        match *self.arena.node(id) {
            CompactNode::Row(span) => self.list(span, size),
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                self.node(content, size)
            },
            _ => self.node(id, size),
        }
    }

    fn code(&mut self, ch: char) -> Result<u16> {
        // The reader refuses control characters other than white space.
        if ch.is_control() && !ch.is_whitespace() {
            if self.strict {
                return Err(Error::Unsupported(format!(
                    "the control character U+{:04X} cannot be written in MTEF",
                    u32::from(ch)
                )));
            }
            self.lose("control characters cannot be written in MTEF; U+FFFD was written instead");
            return Ok(0xFFFD);
        }
        match u16::try_from(u32::from(ch)) {
            Ok(code) => Ok(code),
            Err(_) if self.strict => Err(Error::Unsupported(format!(
                "character U+{:04X} cannot be written in MTEF, whose character values have 16 bits",
                u32::from(ch)
            ))),
            Err(_) => {
                self.lose(
                    "characters outside the Basic Multilingual Plane cannot be written in MTEF; U+FFFD was written instead",
                );
                Ok(0xFFFD)
            },
        }
    }

    #[cold]
    #[inline(never)]
    fn glyph_need(&mut self, size: Size) {
        // A separator has no size to see.
        if self.size.is_none() || !self.separator {
            self.need(size);
            self.glyph_size = self.size;
        }
    }

    /// One CHAR record with its embellishment list.
    fn glyph(
        &mut self,
        size: Size,
        mut glyph: Glyph,
        func_start: bool,
        embells: &[u8],
    ) -> Result<()> {
        if matches!(
            glyph.typeface,
            FN_TEXT | FN_FUNCTION | FN_UCGREEK | chars::FN_TEXT_FE
        ) {
            glyph = self.note_drawn(glyph);
        }
        // `'s` before a letter would read back as a primed variable: a zero
        // space ends the word.
        if self.possessive_s.is_some()
            && self.possessive_s == Some((self.items, self.open.len()))
            && glyph.ch.is_alphabetic()
        {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        let ends_apostrophe =
            self.apostrophe.is_some() && self.apostrophe == Some((self.items, self.open.len()));
        let code = self.code(glyph.ch)?;
        if self.sized {
            self.glyph_need(size);
        } else {
            self.need(size);
        }
        if self.v5 {
            let mut options = 0;
            if !embells.is_empty() {
                options |= V5_CHAR_EMBELL;
            }
            if func_start {
                options |= V5_CHAR_FUNC_START;
            }
            if glyph.font_position.is_some() {
                options |= V5_CHAR_ENC_CHAR_8;
            }
            self.tag(CHAR, options);
            // Signed integer, biased by 128 (MTEFv5.md "Signed integer
            // values"); every typeface the writer uses fits one byte.
            self.out.push((glyph.typeface + 128) as u8);
            self.u16(code);
            if let Some(position) = glyph.font_position {
                self.out.push(position);
            }
        } else {
            let mut options = 0;
            if !embells.is_empty() {
                options |= V3_CHAR_EMBELL;
            }
            // Equation Editor marks letters of the Variable and Function
            // styles as candidates for function recognition.
            if matches!(glyph.typeface, FN_VARIABLE | FN_FUNCTION) && glyph.ch.is_ascii_alphabetic()
            {
                options |= V3_CHAR_AUTO;
            }
            self.tag(CHAR, options);
            self.out.push((glyph.typeface + 128) as u8);
            self.u16(code);
        }
        if !embells.is_empty() {
            self.open.push((false, self.items));
            if self.open.len() >= self.max_depth {
                self.too_deep = true;
            }
            for &kind in embells {
                self.tag(EMBELL, 0);
                self.out.push(kind);
                if !self.v5 {
                    // Equation Editor writes a zero byte after the type.
                    self.out.push(0);
                }
            }
            self.end();
        }
        if embells.is_empty() && matches!(glyph.ch, 's' | '\'') {
            let place = Some((self.items, self.open.len()));
            if glyph.ch == '\'' && glyph.typeface != FN_TEXT {
                self.apostrophe = place;
            } else if glyph.ch == 's' && ends_apostrophe && glyph.typeface != FN_FUNCTION {
                self.possessive_s = place;
            }
        }
        Ok(())
    }

    /// A glyph that starts a leaf: separate it from the previous leaf when
    /// the reader would otherwise join the two.
    #[inline(always)]
    fn first_glyph(
        &mut self,
        size: Size,
        glyph: Glyph,
        run: Option<Run>,
        func_start: bool,
    ) -> Result<()> {
        // Adjacent numbers and texts are one node after normalization too, so
        // only function names and styled runs need separating here; a number
        // or text that is a script base is separated by `scripts`.
        let inside_styled = matches!(run, Some(Run::Styled(_)))
            && self.styled_start.is_some_and(|start| self.items > start);
        let joins = match run {
            Some(Run::Function | Run::Styled(_)) => run == self.run_before() && !inside_styled,
            // Two sizes are two nodes.
            Some(Run::Number(_) | Run::Text) => {
                self.sized && self.glyph_size != Some(self.record(size)) && run == self.run_before()
            },
            Some(Run::Point) | None => false,
        };
        if joins
            || matches!(run, Some(Run::Number(_)))
                && glyph.ch.is_ascii_digit()
                && self.run_before() == Some(Run::Point)
        {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        let run = match (run, self.run_before()) {
            (None, Some(Run::Number(_))) if glyph.ch == '.' && !self.decimal => Some(Run::Point),
            _ => run,
        };
        self.glyph(size, glyph, func_start, &[])?;
        self.set_run(run);
        Ok(())
    }

    fn space_code(&mut self, size: Size, low: u16) -> Result<()> {
        let base: u32 = if self.v5 { 0xEF00 } else { 0xEB00 };
        let ch = char::from_u32(base + u32::from(low)).unwrap_or('\u{FFFD}');
        self.separator = low == chars::SPACE_ZERO;
        let result = self.glyph(size, Glyph::new(FN_SPACE, ch), false, &[]);
        self.separator = false;
        result?;
        if low == chars::SPACE_ZERO {
            self.zero_space = Some((self.items, self.open.len()));
        }
        Ok(())
    }

    // ── Equation ───────────────────────────────────────────────────────

    /// The equation body: an initial size, the equation's LINE (or PILE) and
    /// the final END.
    pub(super) fn equation(&mut self, roots: NodeSpan) -> Result<()> {
        // MTEF 3 has no size preferences: what is not at their 12 points is
        // given its size in points.
        if !self.v5
            && (self.base_pt - f64::from(crate::ast::attrs::DEFAULT_BASE_SIZE_PT)).abs() > 1e-9
        {
            self.sized = true;
            self.absolute = true;
            self.own_size = Some(OwnSize {
                size: self.points(self.base_pt),
                attribute: false,
                place: Size::Full,
            });
        }
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(roots).into();
        match ids.as_slice() {
            [only] if self.is_pile(self.drawn_root(*only)) => {
                let pile = self.drawn_root(*only);
                if pile != *only {
                    self.lose(V3_COLOR_LOSS);
                }
                let mut size = Size::Full;
                if self.sized {
                    // The colours left out on the way.
                    let mut around = *only;
                    while around != pile
                        && let CompactNode::Color { content, .. } = *self.arena.node(around)
                    {
                        (self.own_size, size) = self.own_size_of(around, size);
                        around = self.unwrap_row(content);
                    }
                }
                self.enter()?;
                self.sized_pile(pile, size)?;
                self.depth -= 1;
            },
            _ => {
                self.need(Size::Full);
                self.tag(LINE, 0);
                self.list(roots, Size::Full)?;
                self.end();
            },
        }
        self.end();
        if self.too_deep {
            return Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: self.max_depth,
            });
        }
        Ok(())
    }

    /// The node the equation's only root is written as: MTEF 3 drops the
    /// colours around it.
    fn drawn_root(&self, mut id: NodeId) -> NodeId {
        while !self.v5
            && let CompactNode::Color { content, .. } = *self.arena.node(id)
        {
            id = self.unwrap_row(content);
        }
        id
    }

    fn list(&mut self, span: NodeSpan, size: Size) -> Result<()> {
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        self.list_ids(&ids, size)
    }

    fn list_ids(&mut self, ids: &[NodeId], size: Size) -> Result<()> {
        let previous_style = self.math_style;
        let result = self.list_ids_inner(ids, size);
        self.math_style = previous_style;
        result
    }

    fn list_ids_inner(&mut self, ids: &[NodeId], size: Size) -> Result<()> {
        let mut i = 0;
        while i < ids.len() {
            // A long division without a quotient reads back as the bracket
            // followed by the barred dividend.
            if let (Some(&next), true) = (ids.get(i + 1), self.is_str(ids[i], chars::LONG_DIVISION))
                && let Some(dividend) = self.bar_base(next, true)
            {
                self.enter()?;
                self.long_division(size, dividend, None)?;
                self.depth -= 1;
                i += 2;
                continue;
            }
            // An empty-base script means what normalization makes of it: a
            // limit or script of a big operator or function name before it,
            // or a leading script of the node after it (written as MathType
            // does, with tvSU_PRECEDES directly before that node). Otherwise
            // it keeps its own empty base (`x{}_1`), as in TeX.
            if let Some(&script) = ids.get(i + 1)
                && let Some((sub, sup)) = self.empty_base_scripts(script)
                && self.attaches_to_previous(ids, i + 1)
            {
                self.enter()?;
                self.scripts(size, ids[i], sub, sup)?;
                self.depth -= 1;
                i += 2;
                continue;
            }
            if let Some((sub, sup)) = self.empty_base_scripts(ids[i])
                && let Some(next) = self.prescript_target(ids, i)
            {
                self.enter()?;
                self.leading_script(size, sub, sup, next)?;
                self.depth -= 1;
                self.node(next, size)?;
                i += 2;
                continue;
            }
            self.node(ids[i], size)?;
            i += 1;
        }
        Ok(())
    }

    /// The node after the empty-base script at `i` that takes it as a
    /// leading script, as normalization decides.
    fn prescript_target(&self, ids: &[NodeId], i: usize) -> Option<NodeId> {
        ids.get(i + 1).copied().filter(|&next| {
            self.empty_base_scripts(next).is_none() && can_take_prescripts(*self.arena.node(next))
        })
    }

    /// Whether normalization attaches the empty-base script at `i` to the
    /// node before it: as a limit of a big operator or function name. As in
    /// `normalize_compact_span` this departs from TeX for `\lim{}_x` and
    /// `\operatorname{f}{}_1` (TeX sets the script on an empty atom); every
    /// other empty-base script keeps its empty base.
    fn attaches_to_previous(&self, ids: &[NodeId], i: usize) -> bool {
        i.checked_sub(1)
            .and_then(|p| ids.get(p))
            .is_some_and(|&prev| takes_limits(self.arena, *self.arena.node(prev)))
    }

    // ── Nodes ──────────────────────────────────────────────────────────

    fn node(&mut self, id: NodeId, mut size: Size) -> Result<()> {
        self.enter()?;
        if self.sized {
            let outer = self.own_size;
            if self.has_symbol_size(id) {
                self.lose(SYMBOL_SIZE_LOSS);
            }
            let place = size;
            (self.own_size, size) = self.own_size_of(id, size);
            // A full size in a smaller slot is said to be one.
            if size == Size::Full
                && place != Size::Full
                && self.size == Some(SizeRecord::Level(Size::Full))
                && !std::mem::replace(&mut self.says_full, true)
            {
                self.size = None;
            }
            self.sized_node(id, size)?;
            self.own_size = outer;
        } else {
            self.node_inner(id, size)?;
        }
        self.depth -= 1;
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn sized_node(&mut self, id: NodeId, size: Size) -> Result<()> {
        self.node_inner(id, size)
    }

    #[inline(always)]
    fn node_inner(&mut self, id: NodeId, size: Size) -> Result<()> {
        if let Some(embellished) = self.embellishments(id) {
            let fits = !self.sized || self.marks_have_size(id, &embellished);
            if fits {
                return self.embellished(size, &embellished);
            }
            if let CompactNode::Accent {
                base,
                accent,
                position: AccentPosition::Above,
                ..
            } = *self.arena.node(id)
            {
                // The reader takes it for an accent of a bare character only.
                if self.unwrap_row(base) == embellished.leaf
                    && self.reads_as_character(embellished.leaf)
                    && let Some(mark) = limit_mark(accent)
                {
                    return self.limit_glyph(id, size, base, mark, true);
                }
            }
            self.lose("MTEF draws a mark over one character at the character's size");
            return self.embellished(size, &embellished);
        }
        match *self.arena.node(id) {
            CompactNode::Number(s) => self.leaf(size, Leaf::Number, s),
            CompactNode::Identifier(s) => {
                // The AST does not say whether such an identifier is upright
                // (MathML `<mi>rate</mi>`) or italic (an OMML italic run), so
                // its letters keep the variable style; a style around it
                // writes them as one run instead.
                if matches!(self.style, None | Some(TextStyle::Italic))
                    && self.arena.str_val(s).chars().nth(1).is_some()
                {
                    self.lose(
                        "MTEF has no identifiers of several characters; each was written as separate variables",
                    );
                }
                self.leaf(size, Leaf::Identifier, s)
            },
            CompactNode::Operator { name, .. } => self.leaf(size, Leaf::Operator, name),
            CompactNode::StructuredOperator { content, .. } => {
                self.lose(
                    "MTEF has no source-level StructuredOperator marker; native function and template records were used",
                );
                self.node(content, size)
            },
            CompactNode::Symbol(_, s) => self.leaf(size, Leaf::Symbol, s),
            CompactNode::Text(style, s) => {
                if !matches!(
                    style,
                    TextStyle::Normal | TextStyle::Bold | TextStyle::Italic | TextStyle::BoldItalic
                ) {
                    self.lose("MTEF text has no font family; it was written in the text font");
                }
                self.text_style = style;
                self.leaf(size, Leaf::Text, s)
            },
            CompactNode::Middle(s) => {
                self.lose(
                    "a middle delimiter outside a Dirac bracket was written as a plain character",
                );
                self.leaf(size, Leaf::Symbol, s)
            },
            CompactNode::Space(width) => {
                if width.den == 0 {
                    return self.space(size, width);
                }
                let basis = self.arena.space_basis(id);
                let width = crate::ast::style::local_space_width(width, basis, self.math_style);
                self.space(size, width)
            },
            CompactNode::Newline => {
                self.lose("a line break inside a line cannot be written in MTEF; it was dropped");
                Ok(())
            },
            CompactNode::Row(span) => self.list(span, size),
            CompactNode::UndefinedCommand { name, arguments } => {
                self.lose("an undefined LaTeX command was written as plain text");
                self.text_style = TextStyle::Normal;
                self.leaf(size, Leaf::Text, name)?;
                let groups: SmallVec<[NodeId; 4]> = self.arena.children(arguments).into();
                for group in groups {
                    self.node(group, size)?;
                }
                Ok(())
            },
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                self.node(content, size)
            },
            CompactNode::Fenced {
                open,
                close,
                content,
            } => self.fenced(size, open, close, content),
            CompactNode::Subscript { base, sub } => self.scripts(size, base, Some(sub), None),
            CompactNode::Superscript { base, sup } => self.scripts(size, base, None, Some(sup)),
            CompactNode::SubSup { base, sub, sup } => {
                self.scripts(size, base, Some(sub), Some(sup))
            },
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => {
                let (sub, sup) = (
                    pre_sub.is_some().then_some(pre_sub),
                    pre_sup.is_some().then_some(pre_sup),
                );
                if sub.is_some() || sup.is_some() {
                    self.leading_script(size, sub, sup, base)?;
                }
                self.node(base, size)
            },
            CompactNode::Overscript {
                base,
                over,
                movable_limits,
            } => {
                if self.compound_operator_limits(size, base, None, Some(over), movable_limits)? {
                    return Ok(());
                }
                self.note_movable(movable_limits);
                self.over_under(size, base, Some(over), None)
            },
            CompactNode::Underscript {
                base,
                under,
                movable_limits,
            } => {
                if self.compound_operator_limits(size, base, Some(under), None, movable_limits)? {
                    return Ok(());
                }
                self.note_movable(movable_limits);
                self.over_under(size, base, None, Some(under))
            },
            CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits,
            } => {
                if self.compound_operator_limits(
                    size,
                    base,
                    Some(under),
                    Some(over),
                    movable_limits,
                )? {
                    return Ok(());
                }
                self.note_movable(movable_limits);
                self.over_under(size, base, Some(over), Some(under))
            },
            CompactNode::Fraction { kind, num, den } => self.fraction(size, kind, num, den),
            CompactNode::Sqrt(radicand) => self.root(size, radicand, None),
            CompactNode::Root { index, radicand } => self.root(size, radicand, Some(index)),
            CompactNode::Nary { .. } => self.nary(id, size),
            CompactNode::Accent {
                base,
                accent,
                position,
                wide: true,
            } if self.v5 && is_bracket_accent(accent, position) => self.horizontal_brace(
                size,
                base,
                None,
                position == AccentPosition::Above,
                true,
                accent,
            ),
            CompactNode::Accent {
                base,
                accent,
                position,
                wide,
            } => self.accent(id, size, base, accent, position, wide),
            CompactNode::Array { .. } => {
                // Piles are written for a whole equation and for matrix cells,
                // as the applications do; elsewhere an array is a matrix.
                self.matrix(id, size)
            },
            CompactNode::Boxed {
                content,
                bg_color,
                border_color,
            } => {
                if !self.arena.str_val(bg_color).is_empty()
                    || !self.arena.str_val(border_color).is_empty()
                {
                    self.lose("MTEF cannot represent box colours; the box was kept without them");
                }
                if self.v5 {
                    // tmBOX with all four sides (tvBX_LEFT | RIGHT | TOP | BOTTOM).
                    self.template(size, 37, 0x1E);
                    self.slot(size, Some(content))?;
                    self.end();
                    Ok(())
                } else {
                    self.lose(
                        "MTEF 3 has no box template; the boxed content was written without the box",
                    );
                    self.node(content, size)
                }
            },
            CompactNode::Actuarial { content, .. } => {
                if self.v5 {
                    // tvBX_RIGHT | tvBX_TOP in the MTEF v5 tmBOX variation.
                    self.template(size, 37, 0x0C);
                    self.slot(size, Some(content))?;
                    self.end();
                    Ok(())
                } else {
                    self.lose("MTEF 3 has no top/right enclosure template; the content was kept");
                    self.node(content, size)
                }
            },
            CompactNode::Cancel { stroke, content } => self.cancel(size, stroke, content),
            CompactNode::Styled(style, span) => self.styled(size, style, span),
            CompactNode::Phantom(
                content,
                PhantomKind::SmashBoth | PhantomKind::SmashTop | PhantomKind::SmashBottom,
            ) => {
                self.inform("MTEF cannot represent smashed content; it was written with its own height and depth");
                self.node(content, size)
            },
            CompactNode::Phantom(_, PhantomKind::Vertical) => {
                self.inform("MTEF cannot represent a vertical phantom; it was dropped");
                Ok(())
            },
            CompactNode::Phantom(_, PhantomKind::Full | PhantomKind::Horizontal) => {
                self.lose("MTEF cannot represent a phantom; it was dropped");
                Ok(())
            },
            CompactNode::Scaled { content, .. } => {
                self.lose("MTEF cannot represent scaling; the content was written at normal size");
                self.node(content, size)
            },
            CompactNode::RaiseBox { content, .. } => {
                self.lose("MTEF cannot represent a raised or lowered box; the content was written in place");
                self.node(content, size)
            },
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => {
                self.lose(
                    "MTEF cannot represent a style choice; the branch for the current math style was written",
                );
                let branch = self.math_style.choose(display, text, script, script_script);
                self.node(branch, size)
            },
            CompactNode::Color { color, content } => {
                let rgb = crate::css_colors::resolve_named_or_hex(self.arena.str_val(color));
                match rgb {
                    // Leave room for black in the 16-bit definition indices.
                    Some(rgb)
                        if self.v5
                            && (self.colors.len() < MAX_COLORS || self.colors.contains(&rgb)) =>
                    {
                        let outer = self.color;
                        let index = self.color_index(rgb);
                        self.set_color(Some(index));
                        self.node(content, size)?;
                        // Black is the colour outside every colour node.
                        let outer = match outer {
                            Some(index) => index,
                            None => self.color_index([0, 0, 0]),
                        };
                        self.set_color(Some(outer));
                        Ok(())
                    },
                    Some(_) => {
                        self.lose(if self.v5 {
                            "too many distinct colours; the content was written without colour"
                        } else {
                            V3_COLOR_LOSS
                        });
                        self.node(content, size)
                    },
                    None => {
                        self.lose(
                            "the colour value could not be resolved; the content was written without colour",
                        );
                        self.node(content, size)
                    },
                }
            },
            CompactNode::MathAtom { content, .. } => {
                self.lose("MTEF does not keep math atom classes; the content was written as is");
                self.node(content, size)
            },
            CompactNode::Rule { .. } => {
                self.lose("MTEF cannot represent a rule; it was dropped");
                Ok(())
            },
            CompactNode::StyleSwitch(style) => {
                self.lose("MTEF style switches are not written; their math style was applied to following content");
                self.math_style = MathStyle::branch_from_switch(style);
                Ok(())
            },
            CompactNode::SizeScale(_) => {
                self.lose("MTEF style and size switches are not written; they were dropped");
                Ok(())
            },
            CompactNode::ColAlign(_) => Ok(()),
            CompactNode::Reflect { content, .. } => {
                self.lose("MTEF cannot represent horizontal reflection; the content was written");
                self.node(content, size)
            },
        }
    }

    fn note_movable(&mut self, movable: bool) {
        if movable {
            self.lose("MTEF does not keep movable limits; the scripts were placed over and under");
        }
    }

    // ── Leaves ─────────────────────────────────────────────────────────

    fn leaf(&mut self, size: Size, kind: Leaf, sid: StrId) -> Result<()> {
        let arena = self.arena;
        let text = arena.str_val(sid);
        if kind == Leaf::Number && self.style == Some(TextStyle::Bold) && self.lone_styled {
            return self.bold_number(size, text);
        }
        if kind == Leaf::Number {
            let joins = self.run_before() == Some(Run::Number(FN_NUMBER));
            self.decimal = text.contains('.') || joins && self.decimal;
        }
        let mut first = true;
        for ch in text.chars() {
            // Invisible operators (function application, invisible times,
            // separator and plus) have no MTEF character.
            if matches!(ch, '\u{2061}'..='\u{2064}') {
                continue;
            }
            let (glyph, run) = self.leaf_glyph(kind, ch)?;
            if first {
                let func_start = self.v5 && kind == Leaf::Operator;
                self.first_glyph(size, glyph, run, func_start)?;
                first = false;
            } else {
                self.glyph(size, glyph, false, &[])?;
                self.set_run(run);
            }
        }
        Ok(())
    }

    /// A bold number of several digits, which is read back as one number in
    /// an explicit font. The Vector-Matrix style is for letters.
    #[cold]
    #[inline(never)]
    fn bold_number(&mut self, size: Size, text: &str) -> Result<()> {
        let typeface = self.explicit_font(1);
        let run = Run::Number(typeface);
        if self.run_before() == Some(run) {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        self.decimal = text.contains('.');
        let mut first = true;
        for ch in text.chars() {
            if std::mem::take(&mut first) {
                self.first_glyph(size, Glyph::new(typeface, ch), Some(run), false)?;
            } else {
                self.glyph(size, Glyph::new(typeface, ch), false, &[])?;
                self.set_run(Some(run));
            }
        }
        Ok(())
    }

    /// The glyph for one character of a leaf, and the run it continues.
    fn leaf_glyph(&mut self, kind: Leaf, ch: char) -> Result<(Glyph, Option<Run>)> {
        Ok(match kind {
            // Equation Editor and MathType put CJK text in the Far East text
            // style, whose font has the glyphs.
            Leaf::Text if chars::is_far_east(ch) => {
                self.far_east = true;
                let glyph = Glyph::new(chars::FN_TEXT_FE, ch);
                (glyph.drawn(font_bits(self.text_style)), Some(Run::Text))
            },
            Leaf::Text => {
                let glyph = Glyph::new(FN_TEXT, ch);
                (glyph.drawn(font_bits(self.text_style)), Some(Run::Text))
            },
            Leaf::Operator => (
                Glyph::new(FN_FUNCTION, ch).drawn(self.function_bits()),
                // MTEF 5 marks the first letter of each function name; MTEF 3
                // has no such flag, so adjacent names must be separated.
                (!self.v5 && (ch.is_alphabetic() || ch == '\u{2009}')).then_some(Run::Function),
            ),
            // As `\boldsymbol` sets it: bold, not italic.
            Leaf::Symbol if self.style == Some(TextStyle::BoldItalic) && is_bold_symbol(ch) => {
                let ch = if ch == '-' { '\u{2212}' } else { ch };
                (Glyph::new(self.explicit_font(1), ch), None)
            },
            Leaf::Symbol => (chars::symbol_glyph(ch), None),
            Leaf::Number if ch.is_ascii_digit() || ch == '.' => match self.style {
                Some(TextStyle::Bold) if ch != '.' => {
                    (Glyph::new(FN_VECTOR, ch), Some(Run::Styled(FN_VECTOR)))
                },
                style => {
                    if style == Some(TextStyle::Italic) {
                        self.lose("MTEF has no italic numbers; they were written upright");
                    }
                    (Glyph::new(FN_NUMBER, ch), Some(Run::Number(FN_NUMBER)))
                },
            },
            Leaf::Number => (chars::symbol_glyph(ch), None),
            // CJK letters need the Far East font, as in text.
            Leaf::Identifier if chars::is_far_east(ch) => {
                self.far_east = true;
                if self.style.is_some_and(|style| font_bits(style) != 0) {
                    self.lose(
                        "MTEF has no font style for Far East letters; they were written plain",
                    );
                }
                (Glyph::new(chars::FN_TEXT_FE, ch), Some(Run::Text))
            },
            Leaf::Identifier if ch.is_alphabetic() => self.styled_letter(ch)?,
            Leaf::Identifier => (chars::identifier_glyph(ch), None),
        })
    }

    /// A letter of a variable in the current style.
    fn styled_letter(&mut self, ch: char) -> Result<(Glyph, Option<Run>)> {
        let style_bits = match self.style {
            None => return Ok((chars::identifier_glyph(ch), None)),
            Some(TextStyle::Italic) => {
                let glyph = chars::identifier_glyph(ch);
                if glyph.typeface == FN_UCGREEK {
                    return Ok((glyph.drawn(2), Some(Run::Styled(FN_UCGREEK))));
                }
                self.lose(
                    "MTEF has no italic math alphabet; italic letters were written as variables",
                );
                return Ok((glyph, None));
            },
            // The Vector-Matrix style is bold upright.
            Some(TextStyle::Bold) => {
                return Ok((Glyph::new(FN_VECTOR, ch), Some(Run::Styled(FN_VECTOR))));
            },
            Some(TextStyle::Normal) => 0,
            Some(TextStyle::BoldItalic) => 3,
            Some(
                style @ (TextStyle::Fraktur
                | TextStyle::BoldFraktur
                | TextStyle::Script
                | TextStyle::BoldScript),
            ) if self.v5
                && (ch.is_ascii_uppercase()
                    || ch.is_ascii_lowercase()
                        && matches!(style, TextStyle::Fraktur | TextStyle::BoldFraktur)) =>
            {
                return Ok(self.alphabet_letter(style, ch));
            },
            Some(_) => {
                self.lose("MTEF has no equivalent of this math alphabet; the letters were written in the variable style");
                return Ok((chars::identifier_glyph(ch), None));
            },
        };
        let typeface = self.explicit_font(style_bits);
        Ok((Glyph::new(typeface, ch), Some(Run::Styled(typeface))))
    }

    /// A Fraktur letter or script capital as MathType writes it: in the explicit
    /// Euclid Fraktur or Euclid Math One font, with the letter's MTCode
    /// private-use value (U+F000 or U+F100 plus its index in A–Z, a–z) and
    /// the plain letter as its font position. The bold style follows the
    /// specification; no audited file has a bold letter of these fonts.
    fn alphabet_letter(&mut self, style: TextStyle, ch: char) -> (Glyph, Option<Run>) {
        let (family, bold, block) = match style {
            TextStyle::BoldFraktur => (0, 1, 0xF000),
            TextStyle::Script => (1, 0, 0xF100),
            TextStyle::BoldScript => (1, 1, 0xF100),
            _ => (0, 0, 0xF000),
        };
        let typeface = match self.alphabet_typefaces[family][bold] {
            Some(typeface) => typeface,
            None => {
                self.alphabets = true;
                self.explicit_count += 1;
                // FONT_STYLE_DEF: the font definition and style (bit 0 bold).
                self.out.push(FONT);
                self.out.push(super::ALPHABET_FONT_DEFS[family]);
                self.out.push(bold as u8);
                let typeface = -self.explicit_count;
                self.alphabet_typefaces[family][bold] = Some(typeface);
                typeface
            },
        };
        let letter = ch as u8;
        let index = if letter.is_ascii_uppercase() {
            letter - b'A'
        } else {
            letter - b'a' + 26
        };
        let code = char::from_u32(block + u32::from(index)).unwrap_or(ch);
        (
            Glyph::at(typeface, code, letter),
            Some(Run::Styled(typeface)),
        )
    }

    /// The COLOR_DEF index of `rgb`, writing its definition before its first
    /// use (MTEFv5.md "COLOR_DEF records": components from 0 to 1000).
    fn color_index(&mut self, rgb: [u8; 3]) -> u16 {
        if let Some(i) = self.colors.iter().position(|&c| c == rgb) {
            return i as u16 + 1;
        }
        self.out.push(COLOR_DEF);
        // Options: an unnamed RGB process colour.
        self.out.push(0);
        for component in rgb {
            self.u16(((u32::from(component) * 1000 + 127) / 255) as u16);
        }
        self.colors.push(rgb);
        self.colors.len() as u16
    }

    /// The colour in effect as the reader sees it: `None` for black.
    fn drawn_color(&self) -> Option<u16> {
        self.color
            .filter(|&c| self.colors.get(usize::from(c).wrapping_sub(1)) != Some(&[0, 0, 0]))
    }

    fn set_run(&mut self, run: Option<Run>) {
        self.last_run = run;
        self.last_color = self.drawn_color();
    }

    /// The run the next glyph would continue: the reader joins only glyphs
    /// of one colour.
    fn run_before(&self) -> Option<Run> {
        self.last_run
            .filter(|_| self.last_color == self.drawn_color())
    }

    /// A COLOR record selecting the colour `index` when it is not in effect.
    fn set_color(&mut self, index: Option<u16>) {
        if self.color == index {
            return;
        }
        if let Some(index) = index {
            // Not a `tag`: the record does not end a run, the colour does.
            self.out.push(COLOR);
            if index < 255 {
                self.out.push(index as u8);
            } else {
                self.out.push(255);
                self.u16(index);
            }
        }
        self.color = index;
    }

    /// A script that leads `base`. Before anything but a letter, a digit or
    /// text, the reader may give it to the item before it, as authors mean
    /// it there; a zero space, which carries no script, keeps it leading.
    fn leading_script(
        &mut self,
        size: Size,
        sub: Option<NodeId>,
        sup: Option<NodeId>,
        base: NodeId,
    ) -> Result<()> {
        let mut first = base;
        let leads = loop {
            first = match *self.arena.node(first) {
                CompactNode::Identifier(s) => {
                    break self
                        .arena
                        .str_val(s)
                        .chars()
                        .next()
                        .is_some_and(char::is_alphabetic);
                },
                CompactNode::Number(_) | CompactNode::Text(..) => break true,
                CompactNode::Space(width) => break width == Rational::zero(),
                CompactNode::Subscript { base, .. }
                | CompactNode::Superscript { base, .. }
                | CompactNode::SubSup { base, .. } => base,
                CompactNode::Row(span) | CompactNode::Styled(_, span) => {
                    match self.arena.children(span).first() {
                        Some(&id) => id,
                        None => break false,
                    }
                },
                _ => break false,
            };
        };
        // One zero space is enough: trees too deep for normalization keep the
        // spaces read back, which must not grow with every write.
        if !leads && self.line_has_items() && self.zero_space != Some((self.items, self.open.len()))
        {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        self.script_template(size, sub, sup, true)
    }

    /// The typeface value of an explicit font with the given style bits,
    /// writing its definition before the first character that uses it.
    fn explicit_font(&mut self, style_bits: usize) -> i32 {
        if let Some(typeface) = self.explicit[style_bits] {
            return typeface;
        }
        self.explicit_count += 1;
        let index = self.explicit_count;
        if self.v5 {
            // FONT_STYLE_DEF: font definition index and character style
            // (bit 0 bold, bit 1 italic); it has no options byte.
            self.out.push(FONT);
            self.out.push(TEXT_FONT_DEF);
            self.out.push(style_bits as u8);
        } else {
            // FONT: typeface number, style (as in MTEF 5 and against the
            // description of MTEF 3: docs/mtef-legacy.md), font name.
            self.out.push(FONT);
            self.out.push(index as u8);
            self.out.push(style_bits as u8);
            self.out.extend_from_slice(TEXT_FONT_NAME);
            self.out.push(0);
        }
        self.explicit[style_bits] = Some(-index);
        -index
    }

    /// Note how a character of a typeface style is drawn, where the equation
    /// can define the style for all its characters. Every character written
    /// passes here.
    #[inline(never)]
    fn note_drawn(&mut self, mut glyph: Glyph) -> Glyph {
        if self.punctuation_font
            && glyph.typeface == FN_FUNCTION
            && glyph.drawn.is_none()
            && !glyph.ch.is_whitespace()
        {
            glyph.typeface = self.explicit_font(0);
        }
        let which = match glyph.typeface {
            FN_TEXT => DEFINED_TEXT,
            FN_FUNCTION => DEFINED_FUNCTION,
            FN_UCGREEK => DEFINED_GREEK,
            // The Far East style is not defined with the Text style.
            chars::FN_TEXT_FE => {
                if glyph.drawn.is_some_and(|bits| bits != 0) {
                    self.defined[DEFINED_TEXT] = Some(None);
                }
                return glyph;
            },
            _ => return glyph,
        };
        let bits = match glyph.drawn {
            Some(bits) => bits,
            None if glyph.ch.is_whitespace() => return glyph,
            None if glyph.typeface == FN_FUNCTION => {
                self.has_function_punctuation = true;
                return glyph;
            },
            None => 0,
        };
        let defined = &mut self.defined[which];
        *defined = match *defined {
            None => Some(Some(bits)),
            Some(Some(before)) if before == bits => return glyph,
            Some(_) => Some(None),
        };
        glyph
    }

    /// Whether punctuation must be written again, in a font of its own: the
    /// Function style, in which it is written, is to be bold or italic.
    pub(super) fn punctuation_needs_font(&self) -> bool {
        self.v5
            && self.has_function_punctuation
            && matches!(self.defined[DEFINED_FUNCTION], Some(Some(bits)) if bits != 0)
    }

    /// The bits of a function name. Names are not italic in `\boldsymbol`.
    fn function_bits(&self) -> u8 {
        match self.style {
            Some(TextStyle::Bold | TextStyle::BoldItalic) => 1,
            Some(TextStyle::Italic) => 2,
            _ => 0,
        }
    }

    /// The bold and italic bits EQN_PREFS defines for the Text and Function
    /// styles and upper-case Greek.
    pub(super) fn defined_styles(&mut self) -> [u8; 3] {
        let losses = [
            "MTEF text has no font style of its own; styled text was written as plain text",
            "MTEF function names have no font style of their own; styled names were written plain",
            "MTEF upper-case Greek has no font style of its own; italic letters were written upright",
        ];
        let mut styles = [0; 3];
        for (which, loss) in losses.into_iter().enumerate() {
            match self.defined[which] {
                None | Some(Some(0)) => {},
                Some(Some(bits)) if self.v5 => styles[which] = bits,
                Some(_) => self.lose(loss),
            }
        }
        styles
    }

    fn styled(&mut self, size: Size, style: TextStyle, span: NodeSpan) -> Result<()> {
        let outer = self.style.replace(style);
        let outer_start = self.styled_start.replace(self.items);
        // A number the reader finds bold as a whole.
        let lone = match *self.arena.children(span) {
            [only] => match *self.arena.node(only) {
                CompactNode::Number(s) => is_bold_number(self.arena.str_val(s)),
                _ => false,
            },
            _ => false,
        };
        let outer_lone = std::mem::replace(&mut self.lone_styled, lone);
        let result = self.list(span, size);
        self.lone_styled = outer_lone;
        self.style = outer;
        self.styled_start = outer_start;
        result
    }

    /// Space as MathType space characters: the widths MathType offers, from
    /// widest to narrowest, in 36ths of an em.
    fn space(&mut self, size: Size, width: Rational) -> Result<()> {
        if width.den == 0 {
            if self.strict {
                return Err(Error::Unsupported(
                    "a space whose width has a zero denominator cannot be written".into(),
                ));
            }
            self.lose("a space with a zero-denominator width was dropped");
            return Ok(());
        }
        if width.num < 0 {
            self.lose("MTEF has no negative space; it was dropped");
            return Ok(());
        }
        if width.num == 0 {
            return self.space_code(size, chars::SPACE_ZERO);
        }
        let units = (i64::from(width.num) * 36 + i64::from(width.den) / 2) / i64::from(width.den);
        let units = u32::try_from(units).unwrap_or(u32::MAX);
        let (plan, exact) = chars::space_plan(units, MAX_SPACE_RECORDS);
        let exact = exact && Rational::new(i32::try_from(units).unwrap_or(i32::MAX), 36) == width;
        for space in plan {
            match space {
                chars::Space::Block(low) => self.space_code(size, low)?,
                chars::Space::NoBreak => {
                    self.glyph(size, Glyph::new(FN_FUNCTION, '\u{00A0}'), false, &[])?
                },
            }
        }
        if !exact {
            self.lose("a space width MathType cannot express exactly was rounded to its spaces");
        }
        Ok(())
    }

    // ── Embellishments ─────────────────────────────────────────────────

    /// A node that is one character under embellishments (accents, primes,
    /// strikes): the character's node, the innermost math alphabet inside an
    /// accent, and the embellishment types, innermost first.
    #[inline(always)]
    fn embellishments(&self, id: NodeId) -> Option<Embellished> {
        let mut kinds: SmallVec<[u8; 4]> = SmallVec::new();
        let mut accented = false;
        let mut style = None;
        let mut sized = None;
        let mut current = id;
        loop {
            if self.sized && current != id && self.own_font_size(current).is_some() {
                sized = Some(current);
            }
            match *self.arena.node(current) {
                CompactNode::Accent {
                    base,
                    accent,
                    position,
                    wide: false,
                } => {
                    let Some(kind) = chars::embellishment(accent, position) else {
                        break;
                    };
                    kinds.push(kind);
                    accented = true;
                    current = base;
                },
                CompactNode::Superscript { base, sup } => {
                    if self.sized && !self.marks_fit(base, sup) {
                        break;
                    }
                    let Some(kind) = self.primes(sup) else { break };
                    kinds.push(kind);
                    current = base;
                },
                CompactNode::Cancel {
                    stroke: StrokeKind::Horizontal,
                    content,
                } if !self.v5 => {
                    kinds.push(16);
                    current = content;
                },
                CompactNode::Cancel {
                    stroke: StrokeKind::Forward,
                    content,
                } if !self.v5
                    && self
                        .single_char(content)
                        .is_some_and(|ch| negated(ch).is_none()) =>
                {
                    kinds.push(10);
                    current = content;
                },
                // MTEF 5 strikes one character with the embellishments of
                // these names where a strike template cannot stand: under an
                // accent.
                CompactNode::Cancel { stroke, content }
                    if self.v5 && accented && self.single_char(content).is_some() =>
                {
                    kinds.push(match stroke {
                        StrokeKind::Horizontal => 16,
                        StrokeKind::Cross => 21,
                        StrokeKind::Forward => 22,
                        StrokeKind::Backward => 23,
                        StrokeKind::Vertical => break,
                    });
                    current = content;
                },
                // An accent around a math alphabet around an accent: all are
                // the one character's.
                CompactNode::Styled(inner, span)
                    if accented && self.arena.children(span).len() == 1 =>
                {
                    style = Some(inner);
                    current = self.unwrap_row(self.arena.children(span)[0]);
                },
                CompactNode::Row(_) if accented && self.unwrap_row(current) != current => {
                    current = self.unwrap_row(current);
                },
                _ => break,
            }
        }
        if kinds.is_empty() {
            return None;
        }
        // Primes and strikes alone embellish only a bare character: around a
        // styled one or a row they have forms of their own.
        self.single_char(current)?;
        if !self.v5 && kinds.iter().any(|&k| k > chars::V3_MAX_EMBELLISHMENT) {
            return None;
        }
        kinds.reverse();
        Some(Embellished {
            leaf: current,
            sized,
            style,
            kinds,
        })
    }

    /// Whether a character is written as something other than text.
    #[cold]
    fn reads_as_character(&self, leaf: NodeId) -> bool {
        matches!(
            *self.arena.node(leaf),
            CompactNode::Identifier(_) | CompactNode::Symbol(..)
        ) && self
            .single_char(leaf)
            .is_some_and(|ch| !chars::is_far_east(ch))
    }

    /// The font size a node has of its own.
    fn own_font_size(&self, id: NodeId) -> Option<FontSize> {
        self.arena.attrs(id).and_then(|attrs| attrs.size)
    }

    /// Whether the accents over a character have its size: both none of their
    /// own, or the same.
    #[cold]
    fn marks_have_size(&self, id: NodeId, embellished: &Embellished) -> bool {
        let size = embellished.sized.and_then(|id| self.own_font_size(id));
        let mut current = id;
        loop {
            current = match *self.arena.node(current) {
                CompactNode::Accent { base, .. } => {
                    let mark = self.arena.attrs(current).and_then(|attrs| attrs.glyph_size);
                    if mark != size {
                        return false;
                    }
                    base
                },
                CompactNode::Superscript { base, .. } => base,
                CompactNode::Cancel { content, .. } => content,
                CompactNode::Styled(_, span) if self.arena.children(span).len() == 1 => {
                    self.arena.children(span)[0]
                },
                CompactNode::Row(_) if self.unwrap_row(current) != current => {
                    self.unwrap_row(current)
                },
                _ => return true,
            };
        }
    }

    /// Whether marks in `sup` have the size of scripts of the character under
    /// `base`, as those of an embellishment do.
    #[cold]
    fn marks_fit(&self, base: NodeId, sup: NodeId) -> bool {
        let mut base = base;
        let size = loop {
            if let Some(size) = self.arena.attrs(base).and_then(|attrs| attrs.size) {
                break Some(size.script(self.base_pt));
            }
            base = match *self.arena.node(base) {
                CompactNode::Accent { base, .. } | CompactNode::Superscript { base, .. } => base,
                CompactNode::Cancel { content, .. } => content,
                CompactNode::Styled(_, span) if self.arena.children(span).len() == 1 => {
                    self.arena.children(span)[0]
                },
                _ => break None,
            };
        };
        let fits = |id: NodeId| self.arena.attrs(id).and_then(|attrs| attrs.size) == size;
        match *self.arena.node(sup) {
            CompactNode::Row(span) => self.arena.children(span).iter().all(|&id| fits(id)),
            _ => fits(sup),
        }
    }

    /// The prime embellishment a superscript of one, two or three primes is.
    #[inline(always)]
    fn primes(&self, sup: NodeId) -> Option<u8> {
        let is_prime = |id: NodeId| self.is_str(id, "\u{2032}");
        let count = match *self.arena.node(sup) {
            CompactNode::Row(span) => {
                let children = self.arena.children(span);
                if children.iter().all(|&c| is_prime(c)) {
                    children.len()
                } else {
                    0
                }
            },
            _ if is_prime(sup) => 1,
            _ => 0,
        };
        match count {
            1 => Some(5),
            2 => Some(6),
            3 => Some(18),
            _ => None,
        }
    }

    fn embellished(&mut self, size: Size, embellished: &Embellished) -> Result<()> {
        let outer = (self.style, self.own_size);
        if let Some(style) = embellished.style {
            self.style = Some(style);
        }
        let mut size = size;
        if let Some(sized) = embellished.sized {
            if self.has_symbol_size(sized) {
                self.lose(SYMBOL_SIZE_LOSS);
            }
            (self.own_size, size) = self.own_size_of(sized, size);
        }
        let result = self.embellished_leaf(size, embellished.leaf, &embellished.kinds);
        (self.style, self.own_size) = outer;
        result
    }

    fn embellished_leaf(&mut self, size: Size, leaf: NodeId, kinds: &[u8]) -> Result<()> {
        let (kind, ch) = match *self.arena.node(leaf) {
            CompactNode::Number(s) => (Leaf::Number, s),
            CompactNode::Identifier(s) => (Leaf::Identifier, s),
            CompactNode::Operator { name, .. } => (Leaf::Operator, name),
            CompactNode::Symbol(_, s) => (Leaf::Symbol, s),
            CompactNode::Text(style, s) => {
                self.text_style = style;
                (Leaf::Text, s)
            },
            _ => return self.node(leaf, size),
        };
        let Some(ch) = self.arena.str_val(ch).chars().next() else {
            return self.node(leaf, size);
        };
        let (glyph, _) = self.leaf_glyph(kind, ch)?;
        let func_start = self.v5 && kind == Leaf::Operator;
        self.glyph(size, glyph, func_start, kinds)
    }

    // ── Scripts ────────────────────────────────────────────────────────

    fn scripts(
        &mut self,
        size: Size,
        base: NodeId,
        sub: Option<NodeId>,
        sup: Option<NodeId>,
    ) -> Result<()> {
        // A function such as lim takes its scripts as limits. The reader
        // gives a script the single item before it as base, so a row holding
        // one node is that node.
        let base = self.unwrap_row(base);
        if let CompactNode::MathAtom {
            atom_type: crate::ast::MathAtomType::Op,
            content,
        } = *self.arena.node(base)
        {
            return self.compound_operator_template(size, content, sub, sup, false, false);
        }
        if let CompactNode::Operator { limits, .. } = *self.arena.node(base) {
            match limits {
                LimitStyle::Default => return self.limit(size, base, sub, sup),
                LimitStyle::AboveBelow => {
                    return self.compound_operator_template(size, base, sub, sup, true, false);
                },
                LimitStyle::SubSup => {},
            }
        }
        if let CompactNode::StructuredOperator { content, form } = *self.arena.node(base) {
            self.lose(
                "MTEF has no source-level StructuredOperator marker; native function and template records were used",
            );
            let limits = form.limit_style();
            match limits {
                LimitStyle::Default => {
                    return self.compound_operator_template(
                        size,
                        content,
                        sub,
                        sup,
                        self.movable_compound_is_stacked(size),
                        true,
                    );
                },
                LimitStyle::AboveBelow => {
                    return self.compound_operator_template(size, content, sub, sup, true, false);
                },
                LimitStyle::SubSup => {},
            }
        }
        // A number or text base must not join the number or text before it,
        // or the script would take both as its base.
        let (leaf, color) = self.leaf_and_color(base);
        let before = self.last_run.filter(|_| self.rgb(self.last_color) == color);
        let joins = match *self.arena.node(leaf) {
            CompactNode::Number(_) => matches!(before, Some(Run::Number(_))),
            CompactNode::Text(..) => before == Some(Run::Text),
            _ => false,
        };
        if joins {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        let before = self.items;
        if !self.is_empty(base) {
            self.node(base, size)?;
        }
        // The reader attaches a script to the item before it, or to an empty
        // base at the start of a line. A script whose base wrote nothing
        // after other items is written as one that precedes a zero-width
        // space instead, so that it keeps its empty base.
        if self.items != before || !self.line_has_items() {
            return self.script_template(size, sub, sup, false);
        }
        self.script_template(size, sub, sup, true)?;
        self.space_code(size, chars::SPACE_ZERO)
    }

    fn compound_operator_limits(
        &mut self,
        size: Size,
        base: NodeId,
        lower: Option<NodeId>,
        upper: Option<NodeId>,
        movable: bool,
    ) -> Result<bool> {
        let CompactNode::MathAtom {
            atom_type: crate::ast::MathAtomType::Op,
            content,
        } = *self.arena.node(base)
        else {
            return Ok(false);
        };
        let above_below = !movable || self.movable_compound_is_stacked(size);
        self.compound_operator_template(size, content, lower, upper, above_below, movable)?;
        Ok(true)
    }

    #[inline]
    fn movable_compound_is_stacked(&self, size: Size) -> bool {
        self.display == DisplayStyle::Block && matches!(size, Size::Full | Size::Sym)
    }

    /// General MTEF `tmINTOP`/`tmSUMOP` templates: the main slot is empty, the lower and
    /// upper limit slots carry scripts, and the fourth slot carries the
    /// compound operator itself. The selector and variation retain fixed
    /// side versus fixed above/below placement without encoding the operator
    /// as a decorative bracket.
    fn compound_operator_template(
        &mut self,
        size: Size,
        content: NodeId,
        lower: Option<NodeId>,
        upper: Option<NodeId>,
        above_below: bool,
        movable: bool,
    ) -> Result<()> {
        if movable {
            self.note_movable(true);
        }
        let (selector, variation) = if self.v5 {
            (
                if above_below { 22 } else { 21 },
                lower.map_or(0, |_| TV_LOWER)
                    | upper.map_or(0, |_| TV_UPPER)
                    | if above_below { TV_SUM } else { 0 },
            )
        } else {
            let variation = match (lower.is_some(), upper.is_some()) {
                (false, true) => 0,
                (true, false) => 1,
                _ => 2,
            };
            (if above_below { 43 } else { 42 }, variation)
        };
        self.template(size, selector, variation);
        self.null_line(size);
        let script = size.script();
        self.slot(script, lower)?;
        self.slot(script, upper)?;
        self.slot(size, Some(content))?;
        self.end();
        Ok(())
    }

    /// `tmSUB` / `tmSUP` / `tmSUBSUP` (MTEF 5) or `tmSCRIPT` / `tmLSCRIPT`
    /// (MTEF 3): subscript slot, then superscript slot.
    fn script_template(
        &mut self,
        size: Size,
        sub: Option<NodeId>,
        sup: Option<NodeId>,
        pre: bool,
    ) -> Result<()> {
        let (selector, variation) = if self.v5 {
            let selector = match (sub, sup) {
                (Some(_), None) => 27,
                (None, Some(_)) => 28,
                _ => 29,
            };
            (selector, u16::from(pre))
        } else {
            let variation = match (sub, sup) {
                (Some(_), None) => 1,
                (None, Some(_)) => 0,
                _ => 2,
            };
            (if pre { 44 } else { 15 }, variation)
        };
        self.template(size, selector, variation);
        let script = size.script();
        let script_style = self.math_style.sub();
        self.with_math_style(script_style, |this| {
            this.slot(script, sub)?;
            this.slot(script, sup)
        })?;
        self.end();
        Ok(())
    }

    /// `tmLIM`: main slot, lower slot, upper slot.
    fn limit(
        &mut self,
        size: Size,
        base: NodeId,
        lower: Option<NodeId>,
        upper: Option<NodeId>,
    ) -> Result<()> {
        // The reader drops empty limit lines (MathType draws nothing there).
        let filled = |id: &NodeId| !self.draws_nothing(*id);
        let (lower, upper) = match (lower.filter(filled), upper.filter(filled)) {
            kept if kept == (lower, upper) => kept,
            kept => {
                self.lose("MTEF does not keep empty limits; they were dropped");
                if kept == (None, None) {
                    return self.node(base, size);
                }
                kept
            },
        };
        let variation = if self.v5 {
            lower.map_or(0, |_| TV_LOWER) | upper.map_or(0, |_| TV_UPPER)
        } else {
            match (lower, upper) {
                (None, Some(_)) => 0,
                (Some(_), None) => 1,
                _ => 2,
            }
        };
        self.template(size, if self.v5 { 23 } else { 39 }, variation);
        self.slot(size, Some(base))?;
        let script = size.script();
        let script_style = self.math_style.sub();
        self.with_math_style(script_style, |this| {
            this.slot(script, lower)?;
            this.slot(script, upper)
        })?;
        self.end();
        Ok(())
    }

    // ── Over and under ─────────────────────────────────────────────────

    fn over_under(
        &mut self,
        size: Size,
        base: NodeId,
        over: Option<NodeId>,
        under: Option<NodeId>,
    ) -> Result<()> {
        match (over, under) {
            (Some(mark), None) | (None, Some(mark)) => {
                let top = over.is_some();
                if self.is_str(mark, chars::LINE_MARK) {
                    return self.bar(size, base, !top);
                }
                if let Some((brace_top, bracket, glyph)) = self.brace_of(mark)
                    && brace_top == top
                    && (self.v5 || !bracket)
                {
                    return self.horizontal_brace(size, base, None, top, bracket, glyph);
                }
                if let Some((inner, brace_top, bracket, glyph)) = self.brace_parts(base)
                    && brace_top == top
                {
                    return self.horizontal_brace(size, inner, Some(mark), top, bracket, glyph);
                }
                if top && let Some((dividend, quotient)) = self.long_division_parts(base, mark) {
                    return self.long_division(size, dividend, Some(quotient));
                }
            },
            _ => {},
        }
        if let Some(arrow) = self.single_char(base)
            && self.arrow(size, arrow, over, under)?
        {
            return Ok(());
        }
        self.limit(size, base, under, over)
    }

    /// `tmOBAR` / `tmUBAR`, doubled when the base is itself barred.
    fn bar(&mut self, size: Size, base: NodeId, under: bool) -> Result<()> {
        let (content, double) = match self.bar_base(base, !under) {
            Some(inner) => (inner, true),
            None => (base, false),
        };
        let selector = match (self.v5, under) {
            (true, false) => 13,
            (true, true) => 12,
            (false, false) => 17,
            (false, true) => 16,
        };
        self.template(size, selector, u16::from(double));
        self.slot(size, Some(content))?;
        self.end();
        Ok(())
    }

    /// The base of a node that is a bar over (or under) it.
    fn bar_base(&self, id: NodeId, over: bool) -> Option<NodeId> {
        match *self.arena.node(id) {
            CompactNode::Overscript {
                base, over: mark, ..
            } if over && self.is_str(mark, chars::LINE_MARK) => Some(base),
            CompactNode::Underscript {
                base, under: mark, ..
            } if !over && self.is_str(mark, chars::LINE_MARK) => Some(base),
            _ => None,
        }
    }

    fn brace_of(&self, id: NodeId) -> Option<(bool, bool, char)> {
        match *self.arena.node(id) {
            CompactNode::Symbol(_, s) => chars::horizontal_brace(self.arena.str_val(s)),
            _ => None,
        }
    }

    /// The content of a horizontal brace and the brace, which takes a label.
    fn brace_parts(&self, id: NodeId) -> Option<(NodeId, bool, bool, char)> {
        let (inner, mark_top, brace) = match *self.arena.node(id) {
            CompactNode::Overscript { base, over, .. } => (base, true, self.brace_of(over)),
            CompactNode::Underscript { base, under, .. } => (base, false, self.brace_of(under)),
            CompactNode::Accent {
                base,
                accent,
                position,
                wide: true,
            } if is_bracket_accent(accent, position) => {
                let top = position == AccentPosition::Above;
                (base, top, Some((top, true, accent)))
            },
            _ => return None,
        };
        let (top, bracket, glyph) = brace?;
        (top == mark_top && (self.v5 || !bracket)).then_some((inner, top, bracket, glyph))
    }

    /// HFenceBoxClass / HBrBoxClass: main slot, label slot, brace character.
    fn horizontal_brace(
        &mut self,
        size: Size,
        main: NodeId,
        label: Option<NodeId>,
        top: bool,
        bracket: bool,
        glyph: char,
    ) -> Result<()> {
        let (selector, variation) = if self.v5 {
            (if bracket { 25 } else { 24 }, u16::from(top))
        } else {
            (if top { 27 } else { 28 }, 0)
        };
        self.template(size, selector, variation);
        self.slot(size, Some(main))?;
        self.slot(size.script(), label)?;
        self.glyph(size, Glyph::new(FN_EXPAND, glyph), false, &[])?;
        self.end();
        Ok(())
    }

    /// The dividend and quotient of the reader's long-division shape: the
    /// bracket and the barred dividend under the quotient.
    fn long_division_parts(&self, base: NodeId, quotient: NodeId) -> Option<(NodeId, NodeId)> {
        let CompactNode::Row(span) = *self.arena.node(base) else {
            return None;
        };
        match *self.arena.children(span) {
            [bracket, barred] if self.is_str(bracket, chars::LONG_DIVISION) => self
                .bar_base(barred, true)
                .map(|dividend| (dividend, quotient)),
            _ => None,
        }
    }

    /// `tmLDIV`: dividend slot, quotient slot. MathType 5 and later write two
    /// further null lines, which the class does not name.
    fn long_division(
        &mut self,
        size: Size,
        dividend: NodeId,
        quotient: Option<NodeId>,
    ) -> Result<()> {
        if self.v5 {
            self.template(size, 26, u16::from(quotient.is_some()));
            self.slot(size, Some(dividend))?;
            self.slot(size, quotient)?;
            self.null_line(size);
            self.null_line(size);
        } else {
            // tvLDIVW (0) has the upper slot, tvLDIVWO (1) does not.
            self.template(size, 40, u16::from(quotient.is_none()));
            self.slot(size, Some(dividend))?;
            self.slot(size, quotient)?;
        }
        self.end();
        Ok(())
    }

    /// `tmARROW` (MTEF 5) or `tmLARROW` / `tmRARROW` / `tmBARROW` (MTEF 3)
    /// for an arrow with labels. Returns `false` when no template fits.
    fn arrow(
        &mut self,
        size: Size,
        arrow: char,
        top: Option<NodeId>,
        bottom: Option<NodeId>,
    ) -> Result<bool> {
        if self.v5 {
            let Some((variation, glyphs)) = chars::v5_arrow(arrow) else {
                return Ok(false);
            };
            let variation = variation | top.map_or(0, |_| 0x4) | bottom.map_or(0, |_| 0x8);
            self.template(size, 14, variation);
            let script = size.script();
            self.slot(script, top)?;
            self.slot(script, bottom)?;
            for &ch in glyphs {
                self.glyph(size, Glyph::new(FN_EXPAND, ch), false, &[])?;
            }
            self.end();
            return Ok(true);
        }
        let Some(selector) = chars::v3_arrow(arrow) else {
            return Ok(false);
        };
        // Equation Editor's arrows have a single label slot, above or below.
        let (label, variation) = match (top, bottom) {
            (Some(label), None) => (label, 0),
            (None, Some(label)) => (label, 1),
            _ => return Ok(false),
        };
        self.template(size, selector, variation);
        self.slot(size.script(), Some(label))?;
        self.glyph(size, Glyph::new(FN_EXPAND, arrow), false, &[])?;
        self.end();
        Ok(true)
    }

    // ── Accents and strokes ────────────────────────────────────────────

    fn accent(
        &mut self,
        id: NodeId,
        size: Size,
        base: NodeId,
        accent: char,
        position: AccentPosition,
        wide: bool,
    ) -> Result<()> {
        // The templates and bars stretch over their slot, as the accents do
        // that the MathML operator dictionary makes stretchy or Word
        // stretches: arrows, arcs and bars.
        let stretches = matches!(
            accent,
            '\u{2190}'
                ..='\u{21FF}'
                    | '\u{23DC}'
                    | '\u{23DD}'
                    | '\u{20E7}'
                    | '\u{203E}'
                    | '\u{00AF}'
                    | '\u{0305}'
                    | '\u{0332}'
                    | '_'
        );
        let narrow_loss = (!wide && !stretches).then_some(
            "MTEF keeps a narrow accent only on one character; it was written as a wide one",
        );
        if self.v5 {
            if let Some((selector, variation, glyph)) = chars::v5_accent_template(accent, position)
            {
                narrow_loss.into_iter().for_each(|m| self.lose(m));
                self.template(size, selector, variation);
                self.slot(size, Some(base))?;
                self.with_sign_size(id, size, |this| {
                    this.glyph(size, Glyph::new(FN_EXPAND, glyph), false, &[])
                })?;
                self.end();
                return Ok(());
            }
        } else if let Some((selector, variation)) = chars::v3_accent_template(accent, position) {
            narrow_loss.into_iter().for_each(|m| self.lose(m));
            self.template(size, selector, u16::from(variation));
            self.slot(size, Some(base))?;
            self.end();
            return Ok(());
        }
        if chars::is_bar_accent(accent, position) {
            narrow_loss.into_iter().for_each(|m| self.lose(m));
            return self.bar(size, base, position == AccentPosition::Below);
        }
        // No embellishment or template draws this accent over the base: put
        // the accent character over (or under) it with the limit template.
        self.lose(
            "MTEF has no template for this accent over its base; the accent was written as a limit",
        );
        self.limit_glyph(id, size, base, accent, position == AccentPosition::Above)
    }

    fn limit_glyph(
        &mut self,
        id: NodeId,
        size: Size,
        base: NodeId,
        ch: char,
        above: bool,
    ) -> Result<()> {
        let script = size.script();
        let variation = if self.v5 {
            if above { TV_UPPER } else { TV_LOWER }
        } else if above {
            0
        } else {
            1
        };
        self.template(size, if self.v5 { 23 } else { 39 }, variation);
        self.slot(size, Some(base))?;
        let glyph = chars::symbol_glyph(ch);
        if above {
            self.null_line(script);
        }
        self.need(script);
        self.tag(LINE, 0);
        self.with_sign_size(id, size, |this| this.glyph(script, glyph, false, &[]))?;
        self.end();
        if !above {
            self.null_line(script);
        }
        self.end();
        Ok(())
    }

    fn cancel(&mut self, size: Size, stroke: StrokeKind, content: NodeId) -> Result<()> {
        if self.v5 {
            // tmSTRIKE: tvST_HORIZ, tvST_UP, tvST_DOWN.
            let variation = match stroke {
                StrokeKind::Horizontal => 0x1,
                StrokeKind::Forward => 0x2,
                StrokeKind::Backward => 0x4,
                StrokeKind::Cross => 0x6,
                StrokeKind::Vertical => {
                    self.lose("MTEF has no vertical strike; the content was written without it");
                    return self.node(content, size);
                },
            };
            self.template(size, 36, variation);
            self.slot(size, Some(content))?;
            self.end();
            return Ok(());
        }
        self.lose("MTEF 3 can strike out only a single character; the content was written without the stroke");
        self.node(content, size)
    }

    // ── Fractions, roots, operators ────────────────────────────────────

    /// The size of a text at `size`, in points.
    fn size_points(&self, size: Size) -> f64 {
        let level = |size| self.base_pt * f64::from(type_size(size).per_mille()) / 1000.0;
        match self.record(size) {
            SizeRecord::Points(points) => f64::from(points) / 32.0,
            SizeRecord::Delta(size, delta) => level(size) + f64::from(delta) / 32.0,
            SizeRecord::Level(size) => level(size),
        }
    }

    /// The size of the parts of a fraction in `style`, which are smaller than
    /// what is around it in the AST, where that is sized.
    fn fraction_size(&self, size: Size, slot: Size, style: MathStyle) -> Option<OwnSize> {
        let own = self.own_size?;
        if !own.attribute {
            return Some(own);
        }
        let scale = style.fracnum().size_multiplier() / self.math_style.size_multiplier();
        if (scale - 1.0).abs() < 1e-9 && slot == size {
            return Some(own);
        }
        Some(OwnSize {
            size: self.points(self.size_points(size) * scale),
            place: slot,
            attribute: true,
        })
    }

    fn fraction(&mut self, size: Size, kind: FractionKind, num: NodeId, den: NodeId) -> Result<()> {
        let (selector, variation, slot_size) = match (self.v5, kind) {
            (_, FractionKind::NoBar) => return self.stack(size, num, den),
            // tvFR_SLASH / tmSLFRACT.
            (true, FractionKind::Skewed) => (11, 0x2, size),
            (false, FractionKind::Skewed) => (41, 0, size),
            // tvFR_SMALL / tvPFRACT: subscript-size slots.
            (true, FractionKind::Inline) => (11, 0x1, size.script()),
            (false, FractionKind::Inline) => (14, 1, size.script()),
            (true, _) => (11, 0, size),
            (false, _) => (14, 0, size),
        };
        self.template(size, selector, variation);
        let fraction_style = kind.forced_style().unwrap_or(self.math_style);
        let outer = self.own_size;
        self.own_size = self.fraction_size(size, slot_size, fraction_style);
        self.with_math_style(fraction_style.fracnum(), |this| {
            this.slot(slot_size, Some(num))
        })?;
        self.with_math_style(fraction_style.fracden(), |this| {
            this.slot(slot_size, Some(den))
        })?;
        self.own_size = outer;
        self.end();
        Ok(())
    }

    /// A fraction without a bar, as a centred pile of its two parts.
    fn stack(&mut self, size: Size, num: NodeId, den: NodeId) -> Result<()> {
        self.lose("MTEF has no fraction without a bar; it was written as a pile");
        self.need(size);
        self.tag(PILE, 0);
        self.out.push(2);
        self.out.push(1);
        let outer = self.own_size;
        self.own_size = self.fraction_size(size, size, self.math_style);
        self.with_math_style(self.math_style.fracnum(), |this| this.slot(size, Some(num)))?;
        self.with_math_style(self.math_style.fracden(), |this| this.slot(size, Some(den)))?;
        self.own_size = outer;
        self.end();
        Ok(())
    }

    /// `tmROOT`: radicand slot, then the index slot (null for a square root).
    fn root(&mut self, size: Size, radicand: NodeId, index: Option<NodeId>) -> Result<()> {
        self.template(
            size,
            if self.v5 { 10 } else { 13 },
            u16::from(index.is_some()),
        );
        self.slot(size, Some(radicand))?;
        let index_style = self.math_style.sup().sup();
        let script = size.script();
        let outer = self.own_size;
        if outer.is_some() {
            // The AST's index is at the second script size.
            let text = self.size_points(size);
            let scale = 500.0 / f64::from(type_size(size).per_mille().min(1000));
            self.own_size = Some(OwnSize {
                size: self.points(text * scale),
                attribute: outer.is_some_and(|own| own.attribute),
                place: script,
            });
        }
        let nested = std::mem::replace(&mut self.in_index, true);
        self.with_math_style(index_style, |this| this.slot(script, index))?;
        self.in_index = nested;
        self.own_size = outer;
        self.end();
        Ok(())
    }

    /// BigOpBoxClass: main slot, lower limit, upper limit, then the operator
    /// characters (or, for `tmINTOP` / `tmSUMOP` in MTEF 5, a line holding
    /// the operator).
    fn nary(&mut self, id: NodeId, size: Size) -> Result<()> {
        let CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } = *self.arena.node(id)
        else {
            return Ok(());
        };
        let sub = sub.is_some().then_some(sub);
        let sup = sup.is_some().then_some(sup);
        let above_below = match limits {
            LimitStyle::AboveBelow => true,
            LimitStyle::SubSup => false,
            LimitStyle::Default => !chars::is_integral(op),
        };
        let script = size.script();
        let symbol = size.symbol();
        if self.v5 {
            let limit_bits = sub.map_or(0, |_| TV_LOWER)
                | sup.map_or(0, |_| TV_UPPER)
                | if above_below { TV_SUM } else { 0 };
            let known = chars::v5_big_operator(op);
            let selector = match known {
                Some((selector, ..)) => selector,
                None if above_below => 22,
                None => 21,
            };
            let variation = known.map_or(0, |(_, bits, _)| bits) | limit_bits;
            self.template(size, selector, variation);
            self.slot_or_null(size, body)?;
            self.slot(script, sub)?;
            self.slot(script, sup)?;
            match known {
                Some((_, _, glyphs)) => self.with_sign_size(id, size, |this| {
                    glyphs
                        .iter()
                        .try_for_each(|&glyph| this.glyph(symbol, glyph, false, &[]))
                })?,
                None => {
                    self.with_sign_size(id, size, |this| {
                        this.need(symbol);
                        this.tag(LINE, 0);
                        this.glyph(symbol, chars::symbol_glyph(op), false, &[])
                    })?;
                    self.end();
                },
            }
            self.end();
            return Ok(());
        }
        let (selector, variation) =
            chars::v3_big_operator(op, above_below, sub.is_some(), sup.is_some()).unwrap_or_else(
                || {
                    // tmINTOP / tmSUMOP: tvU*, tvL*, tvB* (upper, lower, both).
                    let variation = match (sub.is_some(), sup.is_some()) {
                        (false, true) => 0,
                        (true, false) => 1,
                        _ => 2,
                    };
                    (if above_below { 43 } else { 42 }, variation)
                },
            );
        self.template(size, selector, u16::from(variation));
        self.slot(size, Some(body))?;
        let script_style = self.math_style.sub();
        self.with_math_style(script_style, |this| {
            this.slot(script, sub)?;
            this.slot(script, sup)
        })?;
        self.with_sign_size(id, size, |this| {
            this.glyph(symbol, chars::symbol_glyph(op), false, &[])
        })?;
        self.end();
        Ok(())
    }

    // ── Fences ─────────────────────────────────────────────────────────

    fn fenced(&mut self, size: Size, open: StrId, close: StrId, content: NodeSpan) -> Result<()> {
        let arena = self.arena;
        let (open, close) = (arena.str_val(open), arena.str_val(close));
        if self.dirac(size, open, close, content)? {
            return Ok(());
        }
        let open_family = Fence::of_open(open);
        let close_family = Fence::of_close(close);
        let usable = |f: Option<Fence>| f.filter(|f| self.v5 || *f != Fence::WhiteBracket);
        let (open_family, close_family) = (usable(open_family), usable(close_family));
        // One template for both sides, or one for a single side.
        let single = match (open.is_empty(), close.is_empty(), open_family, close_family) {
            (false, false, Some(a), Some(b)) if a == b => Some((a, true, true)),
            (false, true, Some(a), _) => Some((a, true, false)),
            (true, false, _, Some(b)) => Some((b, false, true)),
            _ => None,
        };
        if let Some((family, left, right)) = single {
            return self.fence(size, family, left, right, content);
        }
        if open.is_empty() && close.is_empty() {
            return self.fence_content(size, content);
        }
        // Mismatched parentheses and brackets.
        if self.v5
            && let (Some(left), Some(right)) =
                (chars::interval_index(open), chars::interval_index(close))
        {
            self.template(size, 9, left | (right << 4));
            self.fence_slot(size, content)?;
            for s in [open, close] {
                for ch in s.chars() {
                    self.glyph(size, Glyph::new(FN_EXPAND, ch), false, &[])?;
                }
            }
            self.end();
            return Ok(());
        }
        if !self.v5
            && let Some(selector) = chars::v3_mixed_fence(open, close)
        {
            self.template(size, selector, 0);
            self.fence_slot(size, content)?;
            for s in [open, close] {
                for ch in s.chars() {
                    self.glyph(size, Glyph::new(FN_EXPAND, ch), false, &[])?;
                }
            }
            self.end();
            return Ok(());
        }
        // Any other pair: a left-only template holding a right-only one;
        // a side with no template becomes a plain character.
        match open_family {
            Some(family) => {
                self.template(size, family.selector(), self.fence_variation(true, false));
                self.need(size);
                self.tag(LINE, 0);
                self.close_side(size, close, close_family, content)?;
                self.end();
                self.glyph(size, Glyph::new(FN_EXPAND, family.chars().0), false, &[])?;
                self.end();
                Ok(())
            },
            None => {
                self.lose("MTEF has no fence template for this delimiter; it was written as a plain character");
                for ch in open.chars() {
                    self.glyph(size, chars::symbol_glyph(ch), false, &[])?;
                }
                self.close_side(size, close, close_family, content)
            },
        }
    }

    fn close_side(
        &mut self,
        size: Size,
        close: &str,
        family: Option<Fence>,
        content: NodeSpan,
    ) -> Result<()> {
        match family {
            Some(family) if !close.is_empty() => self.fence(size, family, false, true, content),
            _ => {
                self.fence_content(size, content)?;
                if !close.is_empty() {
                    self.lose("MTEF has no fence template for this delimiter; it was written as a plain character");
                    for ch in close.chars() {
                        self.glyph(size, chars::symbol_glyph(ch), false, &[])?;
                    }
                }
                Ok(())
            },
        }
    }

    fn fence_variation(&self, left: bool, right: bool) -> u16 {
        if self.v5 {
            u16::from(left) | (u16::from(right) << 1)
        } else {
            // tvB*, tvL*, tvR*: both, left only, right only.
            match (left, right) {
                (true, false) => 1,
                (false, true) => 2,
                _ => 0,
            }
        }
    }

    /// ParBoxClass: main slot, then the fence characters that are present.
    fn fence(
        &mut self,
        size: Size,
        family: Fence,
        left: bool,
        right: bool,
        content: NodeSpan,
    ) -> Result<()> {
        self.template(size, family.selector(), self.fence_variation(left, right));
        self.fence_slot(size, content)?;
        let (l, r) = family.chars();
        if left {
            self.glyph(size, Glyph::new(FN_EXPAND, l), false, &[])?;
        }
        if right {
            self.glyph(size, Glyph::new(FN_EXPAND, r), false, &[])?;
        }
        self.end();
        Ok(())
    }

    fn fence_slot(&mut self, size: Size, content: NodeSpan) -> Result<()> {
        self.need(size);
        self.tag(LINE, 0);
        self.fence_content(size, content)?;
        self.end();
        Ok(())
    }

    fn fence_content(&mut self, size: Size, content: NodeSpan) -> Result<()> {
        // A middle delimiter ends the local style run in a left/right fence.
        let arena = self.arena;
        for segment in arena
            .children(content)
            .split_inclusive(|&id| matches!(*arena.node(id), CompactNode::Middle(_)))
        {
            self.list_ids(segment, size)?;
        }
        Ok(())
    }

    /// DiracBoxClass (`tmDIRAC`): left slot, right slot, then the angle
    /// brackets and bar that are present. Covers `⟨a|b⟩`, `⟨a|` and `|b⟩`.
    fn dirac(&mut self, size: Size, open: &str, close: &str, content: NodeSpan) -> Result<bool> {
        let children: SmallVec<[NodeId; 8]> = self.arena.children(content).into();
        let middles: SmallVec<[usize; 2]> = children
            .iter()
            .enumerate()
            .filter(|&(_, &id)| matches!(self.arena.node(id), CompactNode::Middle(_)))
            .map(|(i, _)| i)
            .collect();
        let (left, right) = match (open, close, middles.as_slice()) {
            ("\u{27E8}", "\u{27E9}", [m]) if matches!(*self.arena.node(children[*m]), CompactNode::Middle(s) if self.arena.str_val(s) == "|") => {
                (true, true)
            },
            ("\u{27E8}", "|", []) => (true, false),
            ("|", "\u{27E9}", []) => (false, true),
            _ => return Ok(false),
        };
        let (variation, selector) = if self.v5 {
            (u16::from(left) | (u16::from(right) << 1), 30)
        } else {
            // tvBDIRAC, tvLDIRAC, tvRDIRAC.
            (
                match (left, right) {
                    (true, false) => 1,
                    (false, true) => 2,
                    _ => 0,
                },
                45,
            )
        };
        self.template(size, selector, variation);
        let split = middles.first().copied();
        let (before, after) = match split {
            Some(m) => (&children[..m], &children[m + 1..]),
            None if left => (&children[..], &children[..0]),
            None => (&children[..0], &children[..]),
        };
        let current_style = self.math_style;
        self.with_math_style(current_style, |this| {
            for (present, part) in [(left, before), (right, after)] {
                if present {
                    this.need(size);
                    this.tag(LINE, 0);
                    this.list_ids_inner(part, size)?;
                    this.end();
                } else {
                    this.null_line(size);
                }
            }
            Ok(())
        })?;
        let mut glyphs: SmallVec<[char; 3]> = SmallVec::new();
        if left {
            glyphs.push(DIRAC_LEFT);
        }
        glyphs.push(DIRAC_BAR);
        if right {
            glyphs.push(DIRAC_RIGHT);
        }
        for ch in glyphs {
            self.glyph(size, Glyph::new(FN_EXPAND, ch), false, &[])?;
        }
        self.end();
        Ok(true)
    }

    // ── Arrays ─────────────────────────────────────────────────────────

    /// A one-column array without rules is a pile.
    fn is_pile(&self, id: NodeId) -> bool {
        let CompactNode::Array {
            num_cols: 1,
            col_seps: 0,
            row_seps: 0,
            cells,
            ..
        } = *self.arena.node(id)
        else {
            return false;
        };
        !cells.is_empty() && !self.arena.children(cells).iter().any(|&cell| {
            matches!(*self.arena.node(cell), CompactNode::SpannedCell {left_boundaries, right_boundaries,..}
                if self.arena.str_val(left_boundaries).contains('|') || self.arena.str_val(right_boundaries).contains('|'))
        })
    }

    fn column_align(
        &mut self,
        col_aligns: NodeSpan,
        cells: &[NodeId],
        stride: usize,
    ) -> (ColumnAlign, u8, bool) {
        let arena = self.arena;
        let columns = arena.children(col_aligns);
        let get_align = |id: NodeId| match *arena.node(id) {
            CompactNode::ColAlign(value) => value,
            _ => ColumnAlign::Center,
        };
        let first = columns
            .first()
            .copied()
            .map(get_align)
            .unwrap_or(ColumnAlign::Center);
        let has_overrides = cells.iter().any(|&id| {
            matches!(
                arena.node(id),
                CompactNode::AlignedCell { .. } | CompactNode::SpannedCell { .. }
            )
        });
        let unanimous = if has_overrides {
            let mut alignments = cells.iter().enumerate().filter_map(|(i, &cell)| {
                let declared = columns
                    .get(i % stride)
                    .copied()
                    .map(get_align)
                    .unwrap_or(ColumnAlign::Center);
                crate::writer::effective_cell_alignment(arena, cell, declared)
            });
            alignments
                .next()
                .filter(|&candidate| alignments.all(|a| a == candidate))
        } else {
            None
        };
        let alignment = unanimous.unwrap_or(first);
        if unanimous.is_none() && columns.iter().copied().map(get_align).any(|a| a != first) {
            self.lose(
                "An MTEF matrix uses the first column's alignment when column alignments differ.",
            );
        }
        let code = match alignment {
            ColumnAlign::Left => 1,
            ColumnAlign::Center => 2,
            ColumnAlign::Right => 3,
        };
        (alignment, code, has_overrides)
    }

    /// A pile that is written without the line around it, as a node is.
    fn sized_pile(&mut self, id: NodeId, mut size: Size) -> Result<()> {
        if !self.sized {
            return self.pile(id, size);
        }
        let outer = self.own_size;
        if self.has_symbol_size(id) {
            self.lose(SYMBOL_SIZE_LOSS);
        }
        (self.own_size, size) = self.own_size_of(id, size);
        let result = self.pile(id, size);
        self.own_size = outer;
        result
    }

    /// PILE: halign, valign (baseline of the centre line), one line per row.
    fn pile(&mut self, id: NodeId, size: Size) -> Result<()> {
        let CompactNode::Array {
            col_aligns,
            cells,
            array_style,
            ..
        } = *self.arena.node(id)
        else {
            return Ok(());
        };
        let rows = self.arena.children(cells);
        let (exported, halign, _) = self.column_align(col_aligns, rows, 1);
        self.note_array_padding(array_style, false);
        self.need(size);
        self.tag(PILE, 0);
        self.out.push(halign);
        self.out.push(1);
        for &row in rows {
            self.note_cell_alignment(std::slice::from_ref(&row), 0, exported, false);
            self.with_math_style(MathStyle::Text, |this| this.slot(size, Some(row)))?;
        }
        self.end();
        Ok(())
    }

    /// MATRIX: valign, h_just, v_just, rows, columns, partition lines, then
    /// one line per cell, row by row. A cell that is only a pile is written
    /// as the PILE itself, as MathType does.
    fn matrix(&mut self, id: NodeId, size: Size) -> Result<()> {
        let CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            col_seps,
            row_seps,
            array_style,
            ..
        } = *self.arena.node(id)
        else {
            return Ok(());
        };
        let cells = self.arena.children(cells);
        if cells.is_empty() {
            self.lose("an empty array cannot be written as an MTEF matrix; it was dropped");
            return Ok(());
        }
        let cols = usize::from(num_cols.max(1));
        let rows = cells.len().div_ceil(cols);
        let (Ok(rows_u8), Ok(cols_u8)) = (u8::try_from(rows), u8::try_from(cols)) else {
            return Err(Error::Unsupported(format!(
                "an MTEF matrix has at most 255 rows and 255 columns; this array has {rows} rows and {cols} columns"
            )));
        };
        let (exported, h_just, has_overrides) = self.column_align(col_aligns, cells, cols);
        let column_rules = if has_overrides {
            self.column_partition_rules(col_seps, cells, cols)
        } else {
            crate::reader::mtef::PartitionMask::from_low(col_seps)
        };
        let ruled = (0..=cols).any(|i| column_rules.get(i))
            || (0..=rows.min(31)).any(|i| row_seps & (1u32 << i) != 0);
        self.note_array_padding(array_style, ruled);
        self.need(size);
        self.tag(MATRIX, 0);
        self.out.push(1);
        self.out.push(h_just);
        self.out.push(1);
        self.out.push(rows_u8);
        self.out.push(cols_u8);
        let dashes = self.arena.rule_dashes(id);
        self.partition_lines(
            crate::reader::mtef::PartitionMask::from_low(row_seps),
            dashes.rows,
            rows + 1,
        );
        self.partition_lines(column_rules, dashes.cols, cols + 1);
        for row in cells.chunks(cols) {
            for (column, &cell) in row.iter().enumerate() {
                self.note_cell_alignment(row, column, exported, true);
                self.with_math_style(MathStyle::Text, |this| this.cell(cell, size))?;
            }
            for _ in row.len()..cols {
                self.need(size);
                self.tag(LINE, 0);
                self.end();
            }
        }
        self.end();
        Ok(())
    }

    fn note_array_padding(&mut self, padded: bool, ruled: bool) {
        match (padded, ruled) {
            (false, true) => self.lose("Mathema restores outer padding when it reads this ruled MTEF matrix, so its trimmed edges will change"),
            (true, false) => self.lose("MTEF cannot retain TeX outer padding on an unruled table."),
            _ => {},
        }
    }

    fn note_cell_alignment(
        &mut self,
        row: &[NodeId],
        index: usize,
        exported: ColumnAlign,
        matrix_rules: bool,
    ) {
        let cell = row[index];
        match *self.arena.node(cell) {
            CompactNode::AlignedCell { alignment, .. } => {
                if alignment != exported {
                    self.lose(&format!(
                        "MTEF exports one matrix alignment ({exported:?}). This cell requested {alignment:?}"
                    ));
                }
            },
            CompactNode::SpannedCell {
                alignment,
                span,
                left_boundaries,
                right_boundaries,
                ..
            } => {
                let span = self.arena.effective_cell_span_in_row(row, index, span);
                if span > 1 {
                    self.lose(&format!(
                        "MTEF writes the cell-span construct across {span} separate matrix entries."
                    ));
                }
                if !matrix_rules
                    && (!self.arena.str_val(left_boundaries).is_empty()
                        || !self.arena.str_val(right_boundaries).is_empty())
                {
                    self.lose("MTEF drops per-cell boundary material in a pile.");
                }
                if alignment != exported {
                    self.lose(&format!(
                        "MTEF exports one matrix alignment ({exported:?}). This spanned cell requested {alignment:?}"
                    ));
                }
            },
            _ => {},
        }
    }

    fn column_partition_rules(
        &mut self,
        col_seps: u32,
        cells: &[NodeId],
        cols: usize,
    ) -> crate::reader::mtef::PartitionMask {
        use crate::reader::mtef::PartitionMask;
        let base = PartitionMask::from_low(col_seps);
        let mut common = None;
        let mut any = PartitionMask::default();
        for row in cells.chunks(cols) {
            let mut rules = base;
            let mut column = 0;
            while let Some(&cell) = row.get(column) {
                let (span, left_rules, right_rules, overridden) = match *self.arena.node(cell) {
                    CompactNode::AlignedCell { .. } => (1, 0, 0, true),
                    CompactNode::SpannedCell {
                        span,
                        left_boundaries,
                        right_boundaries,
                        ..
                    } => {
                        let (left, left_suppressed) = self.boundary_rules(left_boundaries);
                        let (right, right_suppressed) = self.boundary_rules(right_boundaries);
                        if left_suppressed || right_suppressed {
                            self.lose("The MTEF matrix drops per-cell glue suppression.");
                        }
                        (
                            self.arena.effective_cell_span_in_row(row, column, span),
                            left,
                            right,
                            true,
                        )
                    },
                    _ => (1, 0, 0, false),
                };
                let width = span.min(cols - column).max(1);
                if overridden {
                    let end = column + width;
                    let first = if column == 0 { 0 } else { column + 1 };
                    for boundary in first..=end {
                        rules.set(boundary, false);
                    }
                    if left_rules > 0 {
                        rules.set(column, true);
                    }
                    if right_rules > 0 {
                        rules.set(end, true);
                    }
                }
                column += width;
            }
            common = Some(common.map_or(rules, |prior: PartitionMask| prior.intersection(rules)));
            any = any.union(rules);
        }
        let common = common.unwrap_or(base);
        let varying = any.differing(common);
        if varying.any() {
            self.lose("MTEF uses the table's declared rule for row-varying cell rules.");
        }
        common.union(base.intersection(varying))
    }

    fn boundary_rules(&mut self, id: StrId) -> (usize, bool) {
        let mut rules = 0;
        let mut suppressed = false;
        for boundary in crate::ast::span_boundaries(self.arena.str_val(id)) {
            match boundary {
                SpanBoundary::Rule => rules += 1,
                SpanBoundary::SuppressGlue => suppressed = true,
            }
        }
        if rules > 1 {
            self.lose("MTEF represents repeated cell rules with one line per boundary.");
        }
        (rules, suppressed)
    }

    fn cell(&mut self, cell: NodeId, size: Size) -> Result<()> {
        let only = match *self.arena.node(cell) {
            CompactNode::Row(span) => match *self.arena.children(span) {
                [only] => Some(only),
                _ => None,
            },
            _ => Some(cell),
        };
        if let Some(only) = only
            && self.is_pile(only)
        {
            self.enter()?;
            self.sized_pile(only, size)?;
            self.depth -= 1;
            return Ok(());
        }
        self.slot(size, Some(cell))
    }

    /// Two bits per partition line, low bits first, padded to whole bytes
    /// (MTEFv5.md "MATRIX record"): 1 for a solid line, 2 for a dashed one
    /// (`dashed` covers the first 32 boundaries).
    fn partition_lines(
        &mut self,
        mask: crate::reader::mtef::PartitionMask,
        dashed: u32,
        lines: usize,
    ) {
        for start in (0..lines).step_by(4) {
            let mut byte = 0u8;
            for k in 0..(lines - start).min(4) {
                let line = start + k;
                let style = match (mask.get(line), line < 32 && dashed & (1 << line) != 0) {
                    (false, _) => 0,
                    (true, false) => 1,
                    (true, true) => 2,
                };
                byte |= style << (2 * k);
            }
            self.out.push(byte);
        }
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// The scripts of a script node whose base is empty.
    fn empty_base_scripts(&self, id: NodeId) -> Option<(Option<NodeId>, Option<NodeId>)> {
        let (base, sub, sup) = match *self.arena.node(id) {
            CompactNode::Subscript { base, sub } => (base, Some(sub), None),
            CompactNode::Superscript { base, sup } => (base, None, Some(sup)),
            CompactNode::SubSup { base, sub, sup } => (base, Some(sub), Some(sup)),
            _ => return None,
        };
        self.is_empty(self.unwrap_row(base)).then_some((sub, sup))
    }

    /// The colour an index stands for, black when there is none.
    fn rgb(&self, color: Option<u16>) -> [u8; 3] {
        color
            .and_then(|c| self.colors.get(usize::from(c).wrapping_sub(1)))
            .copied()
            .unwrap_or_default()
    }

    /// The node a chain of colours ends in, and the colour it is drawn in.
    fn leaf_and_color(&self, id: NodeId) -> (NodeId, [u8; 3]) {
        let mut color = self.rgb(self.color);
        let mut leaf = self.unwrap_row(id);
        while self.v5
            && let CompactNode::Color {
                color: name,
                content,
            } = *self.arena.node(leaf)
        {
            color =
                crate::css_colors::resolve_named_or_hex(self.arena.str_val(name)).unwrap_or(color);
            leaf = self.unwrap_row(content);
        }
        (leaf, color)
    }

    /// The node a row of exactly one node holds, through nested rows.
    fn unwrap_row(&self, mut id: NodeId) -> NodeId {
        while let CompactNode::Row(span) = *self.arena.node(id)
            && let [only] = *self.arena.children(span)
        {
            id = only;
        }
        id
    }

    /// Whether a node draws nothing, as the reader judges it.
    fn draws_nothing(&self, id: NodeId) -> bool {
        if id.is_none() {
            return true;
        }
        match *self.arena.node(id) {
            CompactNode::Row(span) => self
                .arena
                .children(span)
                .iter()
                .all(|&child| self.draws_nothing(child)),
            CompactNode::Array { cells, .. } => cells.is_empty(),
            _ => false,
        }
    }

    fn is_empty(&self, id: NodeId) -> bool {
        id.is_none() || matches!(*self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    /// Whether `id` is a symbol, identifier or operator spelled `s`.
    fn is_str(&self, id: NodeId, s: &str) -> bool {
        match *self.arena.node(id) {
            CompactNode::Symbol(_, sid) | CompactNode::Identifier(sid) => {
                self.arena.str_val(sid) == s
            },
            CompactNode::Operator { name, .. } => self.arena.str_val(name) == s,
            _ => false,
        }
    }

    /// The character of a leaf spelled with exactly one character.
    fn single_char(&self, id: NodeId) -> Option<char> {
        let sid = match *self.arena.node(id) {
            CompactNode::Number(s)
            | CompactNode::Identifier(s)
            | CompactNode::Symbol(_, s)
            | CompactNode::Text(_, s) => s,
            CompactNode::Operator { name, .. } => name,
            _ => return None,
        };
        let mut it = self.arena.str_val(sid).chars();
        match (it.next(), it.next()) {
            (Some(ch), None) if !matches!(ch, '\u{2061}'..='\u{2064}') => Some(ch),
            _ => None,
        }
    }
}

/// MTEF 5 has one horizontal bracket template with a top/bottom variation.
fn is_bracket_accent(glyph: char, position: AccentPosition) -> bool {
    matches!(
        (glyph, position),
        ('⎴', AccentPosition::Above) | ('⎵', AccentPosition::Below)
    )
}

/// Whether a number is of several digits, with a point only between digits:
/// one the reader joins.
fn is_bold_number(text: &str) -> bool {
    text.len() > 1
        && text.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && text.bytes().filter(|&b| b == b'.').count() <= 1
        && !text.starts_with('.')
        && !text.ends_with('.')
}

/// Bold (1) and italic (2) of a style of fonts.
fn font_bits(style: TextStyle) -> u8 {
    match style {
        TextStyle::Bold => 1,
        TextStyle::Italic => 2,
        TextStyle::BoldItalic => 3,
        _ => 0,
    }
}
