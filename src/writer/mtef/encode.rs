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
    FN_VARIABLE, FN_VECTOR, Fence, Glyph,
};
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::normalize::{can_carry_script, can_take_prescripts, is_blank, takes_limits};
use crate::ast::style::MathStyle;
use crate::ast::{
    AccentPosition, ColumnAlign, DisplayStyle, FractionKind, LimitStyle, Rational, SpanBoundary,
    StrokeKind, TextStyle,
};
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, LimitKind, Result, Strictness};
use crate::reader::mtef::chars::negated;

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
}

impl Size {
    /// The size of scripts and limits attached to content of this size.
    fn script(self) -> Size {
        match self {
            Size::Full | Size::Sym => Size::Sub,
            _ => Size::Sub2,
        }
    }

    /// The size of a big operator's sign in content of this size.
    fn symbol(self) -> Size {
        match self {
            Size::Full | Size::Sym => Size::Sym,
            _ => Size::SubSym,
        }
    }
}

/// Characters the reader joins into one node when they follow each other:
/// two nodes whose glyphs share a key need a separator between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Number(i32),
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
    /// The typesize in effect at the end of `out` (`None` before the first).
    size: Option<Size>,
    strict: bool,
    max_depth: usize,
    max_nodes: Option<usize>,
    depth: usize,
    nodes: usize,
    pub(super) diagnostics: Vec<Diagnostic>,
    /// Typeface values of the explicit fonts defined so far, by style bits
    /// (bit 0 bold, bit 1 italic).
    explicit: [Option<i32>; 4],
    explicit_count: i32,
    /// The run the last written glyph belongs to.
    last_run: Option<Run>,
    /// Records that produce an item in the reader's line (CHAR, TMPL, PILE,
    /// MATRIX), counted to find bases that write nothing.
    items: usize,
    /// The records whose object lists are open, innermost last: whether
    /// each is a LINE, and the item count when it was opened.
    open: Vec<(bool, usize)>,
    /// Set when the object lists nest deeper than the reader accepts.
    too_deep: bool,
    /// Set when Far East text was written (MTEF 5 then defines its font).
    pub(super) far_east: bool,
    /// The item count when the innermost `Styled` node began: its own
    /// letters continue one run and need no separator.
    styled_start: Option<usize>,
    /// The style of an enclosing `Styled` node.
    style: Option<TextStyle>,
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
    ) -> Self {
        Self {
            arena,
            v5,
            display,
            out: Vec::with_capacity(256),
            size: None,
            strict: config.strictness == Strictness::Strict,
            max_depth: config.effective_max_depth(),
            max_nodes: config.max_nodes,
            depth: 0,
            nodes: 0,
            diagnostics: Vec::new(),
            explicit: [None; 4],
            explicit_count: 0,
            last_run: None,
            items: 0,
            open: Vec::new(),
            too_deep: false,
            far_east: false,
            styled_start: None,
            style: None,
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
        if !self.diagnostics.iter().any(|d| d.message == message) {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Warning,
                message: message.to_owned(),
                span: None,
            });
        }
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
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
    fn need(&mut self, size: Size) {
        if self.size != Some(size) {
            self.out.push(TYPESIZE_BASE + size as u8);
            self.size = Some(size);
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
                self.pile(self.unwrap_row(id), size)?;
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

    /// One CHAR record with its embellishment list.
    fn glyph(&mut self, size: Size, glyph: Glyph, func_start: bool, embells: &[u8]) -> Result<()> {
        let code = self.code(glyph.ch)?;
        self.need(size);
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
        Ok(())
    }

    /// A glyph that starts a leaf: separate it from the previous leaf when
    /// the reader would otherwise join the two.
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
        if matches!(run, Some(Run::Function | Run::Styled(_)))
            && run == self.last_run
            && !inside_styled
        {
            self.space_code(size, chars::SPACE_ZERO)?;
        }
        self.glyph(size, glyph, func_start, &[])?;
        self.last_run = run;
        Ok(())
    }

    fn space_code(&mut self, size: Size, low: u16) -> Result<()> {
        let base: u32 = if self.v5 { 0xEF00 } else { 0xEB00 };
        let ch = char::from_u32(base + u32::from(low)).unwrap_or('\u{FFFD}');
        self.glyph(size, Glyph::new(FN_SPACE, ch), false, &[])
    }

    // ── Equation ───────────────────────────────────────────────────────

    /// The equation body: an initial size, the equation's LINE (or PILE) and
    /// the final END.
    pub(super) fn equation(&mut self, roots: NodeSpan) -> Result<()> {
        let ids: SmallVec<[NodeId; 8]> = self.arena.children(roots).into();
        match ids.as_slice() {
            [only] if self.is_pile(*only) => {
                self.enter()?;
                self.pile(*only, Size::Full)?;
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
            // a leading script of the node after it (written as MathType
            // does, with tvSU_PRECEDES directly before that node), or with
            // nothing after it a script of the node before it.
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
                self.script_template(size, sub, sup, true)?;
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
    /// node before it.
    fn attaches_to_previous(&self, ids: &[NodeId], i: usize) -> bool {
        let Some(&prev) = i.checked_sub(1).and_then(|p| ids.get(p)) else {
            return false;
        };
        let prev = *self.arena.node(prev);
        let trailing = ids[i + 1..]
            .iter()
            .all(|&id| is_blank(*self.arena.node(id)));
        takes_limits(self.arena, prev) || (trailing && can_carry_script(prev))
    }

    // ── Nodes ──────────────────────────────────────────────────────────

    fn node(&mut self, id: NodeId, size: Size) -> Result<()> {
        self.enter()?;
        self.node_inner(id, size)?;
        self.depth -= 1;
        Ok(())
    }

    fn node_inner(&mut self, id: NodeId, size: Size) -> Result<()> {
        if let Some((leaf, kinds)) = self.embellishments(id) {
            return self.embellished(size, leaf, &kinds);
        }
        match *self.arena.node(id) {
            CompactNode::Number(s) => self.leaf(size, Leaf::Number, s),
            CompactNode::Identifier(s) => self.leaf(size, Leaf::Identifier, s),
            CompactNode::Operator { name, .. } => self.leaf(size, Leaf::Operator, name),
            CompactNode::StructuredOperator { content, .. } => {
                self.lose(
                    "MTEF has no source-level StructuredOperator marker; native function and template records were used",
                );
                self.node(content, size)
            },
            CompactNode::Symbol(_, s) => self.leaf(size, Leaf::Symbol, s),
            CompactNode::Text(style, s) => {
                if style != TextStyle::Normal {
                    self.lose("MTEF text has no font style; styled text was written as plain text");
                }
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
                    self.script_template(size, sub, sup, true)?;
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
            CompactNode::Nary {
                op,
                sub,
                sup,
                body,
                limits,
            } => self.nary(
                size,
                op,
                sub.is_some().then_some(sub),
                sup.is_some().then_some(sup),
                body,
                limits,
            ),
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
                ..
            } => self.accent(size, base, accent, position),
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
            CompactNode::Phantom(..) => {
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
            CompactNode::Color { content, .. } => {
                self.lose(
                    "colours are not written to MTEF; the content was written without colour",
                );
                self.node(content, size)
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
                self.last_run = run;
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
                (Glyph::new(chars::FN_TEXT_FE, ch), Some(Run::Text))
            },
            Leaf::Text => (Glyph::new(FN_TEXT, ch), Some(Run::Text)),
            Leaf::Operator => (
                Glyph::new(FN_FUNCTION, ch),
                // MTEF 5 marks the first letter of each function name; MTEF 3
                // has no such flag, so adjacent names must be separated.
                (!self.v5 && (ch.is_alphabetic() || ch == '\u{2009}')).then_some(Run::Function),
            ),
            Leaf::Symbol => (chars::symbol_glyph(ch), None),
            Leaf::Number if ch.is_ascii_digit() || ch == '.' => match self.style {
                Some(TextStyle::Bold) if ch != '.' => {
                    (Glyph::new(FN_VECTOR, ch), Some(Run::Styled(FN_VECTOR)))
                },
                _ => (Glyph::new(FN_NUMBER, ch), Some(Run::Number(FN_NUMBER))),
            },
            Leaf::Number => (chars::symbol_glyph(ch), None),
            // CJK letters need the Far East font, as in text.
            Leaf::Identifier if chars::is_far_east(ch) => {
                self.far_east = true;
                (Glyph::new(chars::FN_TEXT_FE, ch), Some(Run::Text))
            },
            Leaf::Identifier if ch.is_alphabetic() => self.styled_letter(ch)?,
            Leaf::Identifier => (chars::identifier_glyph(ch), None),
        })
    }

    /// A letter of a variable in the current style.
    fn styled_letter(&mut self, ch: char) -> Result<(Glyph, Option<Run>)> {
        let style_bits = match self.style {
            None | Some(TextStyle::Italic) => return Ok((chars::identifier_glyph(ch), None)),
            // The Vector-Matrix style is bold upright.
            Some(TextStyle::Bold) => {
                return Ok((Glyph::new(FN_VECTOR, ch), Some(Run::Styled(FN_VECTOR))));
            },
            Some(TextStyle::Normal) => 0,
            Some(TextStyle::BoldItalic) => 3,
            Some(_) => {
                self.lose("MTEF has no equivalent of this math alphabet; the letters were written in the variable style");
                return Ok((chars::identifier_glyph(ch), None));
            },
        };
        let typeface = self.explicit_font(style_bits);
        Ok((Glyph::new(typeface, ch), Some(Run::Styled(typeface))))
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
            // FONT: typeface number, style (1 italic, 2 bold), font name.
            let style = ((style_bits & 1) << 1) | ((style_bits >> 1) & 1);
            self.out.push(FONT);
            self.out.push(index as u8);
            self.out.push(style as u8);
            self.out.extend_from_slice(TEXT_FONT_NAME);
            self.out.push(0);
        }
        self.explicit[style_bits] = Some(-index);
        -index
    }

    fn styled(&mut self, size: Size, style: TextStyle, span: NodeSpan) -> Result<()> {
        if style == TextStyle::Italic {
            self.lose("MTEF has no italic math alphabet; italic letters were written as variables");
        }
        let outer = self.style.replace(style);
        let outer_start = self.styled_start.replace(self.items);
        let result = self.list(span, size);
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
    /// MTEF 3 strokes): the character's node and the embellishment types,
    /// innermost first.
    fn embellishments(&self, id: NodeId) -> Option<(NodeId, SmallVec<[u8; 4]>)> {
        let mut kinds: SmallVec<[u8; 4]> = SmallVec::new();
        let mut current = id;
        loop {
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
                    current = base;
                },
                CompactNode::Superscript { base, sup } => {
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
                _ => break,
            }
        }
        if kinds.is_empty() || self.single_char(current).is_none() {
            return None;
        }
        if !self.v5 && kinds.iter().any(|&k| k > chars::V3_MAX_EMBELLISHMENT) {
            return None;
        }
        kinds.reverse();
        Some((current, kinds))
    }

    /// The prime embellishment a superscript of one, two or three primes is.
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

    fn embellished(&mut self, size: Size, leaf: NodeId, kinds: &[u8]) -> Result<()> {
        let (kind, ch) = match *self.arena.node(leaf) {
            CompactNode::Number(s) => (Leaf::Number, s),
            CompactNode::Identifier(s) => (Leaf::Identifier, s),
            CompactNode::Operator { name, .. } => (Leaf::Operator, name),
            CompactNode::Symbol(_, s) => (Leaf::Symbol, s),
            CompactNode::Text(_, s) => (Leaf::Text, s),
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
        if self.v5
            && let CompactNode::Accent {
                base: inner,
                accent,
                position,
                wide: true,
            } = *self.arena.node(base)
            && is_bracket_accent(accent, position)
        {
            let top = position == AccentPosition::Above;
            let (label, other) = if top { (sup, sub) } else { (sub, sup) };
            if other.is_none() {
                return self.horizontal_brace(size, inner, label, top, true, accent);
            }
        }
        // The label of a horizontal brace.
        match (*self.arena.node(base), sub, sup) {
            (
                CompactNode::Overscript {
                    base: inner, over, ..
                },
                None,
                Some(label),
            ) => {
                if let Some((true, bracket, glyph)) = self.brace_of(over)
                    && (self.v5 || !bracket)
                {
                    return self.horizontal_brace(size, inner, Some(label), true, bracket, glyph);
                }
            },
            (
                CompactNode::Underscript {
                    base: inner, under, ..
                },
                Some(label),
                None,
            ) => {
                if let Some((false, bracket, glyph)) = self.brace_of(under)
                    && (self.v5 || !bracket)
                {
                    return self.horizontal_brace(size, inner, Some(label), false, bracket, glyph);
                }
            },
            _ => {},
        }
        // A number or text base must not join the number or text before it,
        // or the script would take both as its base.
        let joins = match *self.arena.node(base) {
            CompactNode::Number(_) => matches!(self.last_run, Some(Run::Number(_))),
            CompactNode::Text(..) => self.last_run == Some(Run::Text),
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
        size: Size,
        base: NodeId,
        accent: char,
        position: AccentPosition,
    ) -> Result<()> {
        if self.v5 {
            if let Some((selector, variation, glyph)) = chars::v5_accent_template(accent, position)
            {
                self.template(size, selector, variation);
                self.slot(size, Some(base))?;
                self.glyph(size, Glyph::new(FN_EXPAND, glyph), false, &[])?;
                self.end();
                return Ok(());
            }
        } else if let Some((selector, variation)) = chars::v3_accent_template(accent, position) {
            self.template(size, selector, u16::from(variation));
            self.slot(size, Some(base))?;
            self.end();
            return Ok(());
        }
        if chars::is_bar_accent(accent, position) {
            return self.bar(size, base, position == AccentPosition::Below);
        }
        // No embellishment or template draws this accent over the base: put
        // the accent character over (or under) it with the limit template.
        self.lose(
            "MTEF has no template for this accent over its base; the accent was written as a limit",
        );
        self.limit_glyph(size, base, accent, position == AccentPosition::Above)
    }

    fn limit_glyph(&mut self, size: Size, base: NodeId, ch: char, above: bool) -> Result<()> {
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
        self.glyph(script, glyph, false, &[])?;
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
        self.with_math_style(fraction_style.fracnum(), |this| {
            this.slot(slot_size, Some(num))
        })?;
        self.with_math_style(fraction_style.fracden(), |this| {
            this.slot(slot_size, Some(den))
        })?;
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
        self.with_math_style(self.math_style.fracnum(), |this| this.slot(size, Some(num)))?;
        self.with_math_style(self.math_style.fracden(), |this| this.slot(size, Some(den)))?;
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
        self.with_math_style(index_style, |this| this.slot(size.script(), index))?;
        self.end();
        Ok(())
    }

    /// BigOpBoxClass: main slot, lower limit, upper limit, then the operator
    /// characters (or, for `tmINTOP` / `tmSUMOP` in MTEF 5, a line holding
    /// the operator).
    fn nary(
        &mut self,
        size: Size,
        op: char,
        sub: Option<NodeId>,
        sup: Option<NodeId>,
        body: NodeId,
        limits: LimitStyle,
    ) -> Result<()> {
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
                Some((_, _, glyphs)) => {
                    for &glyph in glyphs {
                        self.glyph(symbol, glyph, false, &[])?;
                    }
                },
                None => {
                    self.need(symbol);
                    self.tag(LINE, 0);
                    self.glyph(symbol, chars::symbol_glyph(op), false, &[])?;
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
        self.glyph(symbol, chars::symbol_glyph(op), false, &[])?;
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

    /// PILE: halign, valign (baseline of the centre line), one line per row.
    fn pile(&mut self, id: NodeId, size: Size) -> Result<()> {
        let CompactNode::Array {
            col_aligns, cells, ..
        } = *self.arena.node(id)
        else {
            return Ok(());
        };
        let rows = self.arena.children(cells);
        let (exported, halign, _) = self.column_align(col_aligns, rows, 1);
        self.need(size);
        self.tag(PILE, 0);
        self.out.push(halign);
        self.out.push(1);
        for &row in rows {
            self.note_cell_alignment(row, exported, false);
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
        if (column_rules.any() || row_seps != 0) && !array_style {
            self.lose("Mathema restores outer padding when it reads this ruled MTEF matrix, so its trimmed edges will change");
        }
        self.need(size);
        self.tag(MATRIX, 0);
        self.out.push(1);
        self.out.push(h_just);
        self.out.push(1);
        self.out.push(rows_u8);
        self.out.push(cols_u8);
        self.partition_lines(
            crate::reader::mtef::PartitionMask::from_low(row_seps),
            rows + 1,
        );
        self.partition_lines(column_rules, cols + 1);
        for index in 0..rows * cols {
            match cells.get(index) {
                Some(&cell) => {
                    self.note_cell_alignment(cell, exported, true);
                    self.with_math_style(MathStyle::Text, |this| this.cell(cell, size))?;
                },
                None => {
                    self.need(size);
                    self.tag(LINE, 0);
                    self.end();
                },
            }
        }
        self.end();
        Ok(())
    }

    fn note_cell_alignment(&mut self, cell: NodeId, exported: ColumnAlign, matrix_rules: bool) {
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
                        (usize::from(span.max(1)), left, right, true)
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
            self.pile(only, size)?;
            self.depth -= 1;
            return Ok(());
        }
        self.slot(size, Some(cell))
    }

    /// Two bits per partition line (solid when drawn), low bits first,
    /// padded to whole bytes (MTEFv5.md "MATRIX record").
    fn partition_lines(&mut self, mask: crate::reader::mtef::PartitionMask, lines: usize) {
        for start in (0..lines).step_by(4) {
            let mut byte = 0u8;
            for k in 0..(lines - start).min(4) {
                byte |= u8::from(mask.get(start + k)) << (2 * k);
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

    /// The node a row of exactly one node holds, through nested rows.
    fn unwrap_row(&self, mut id: NodeId) -> NodeId {
        while let CompactNode::Row(span) = *self.arena.node(id)
            && let [only] = *self.arena.children(span)
        {
            id = only;
        }
        id
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
