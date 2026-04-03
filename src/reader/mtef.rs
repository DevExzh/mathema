use smallvec::SmallVec;

use crate::ast::*;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader;

pub struct MtefReader;

impl Reader for MtefReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        let bytes = match hex_decode(input) {
            Ok(b) => b,
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    return Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, DisplayStyle::Inline),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MTEF hex decode failed (recovered): {e}"),
                            span: None,
                        }],
                    ));
                }
                return Err(e);
            },
        };
        Self::parse_bytes_compact(&bytes, config)
    }
}

impl MtefReader {
    /// Parse raw MTEF binary bytes.
    pub fn parse_bytes(bytes: &[u8], config: &Config) -> Result<ParseResult> {
        Self::parse_bytes_compact(bytes, config).map(|r| r.to_parse_result())
    }

    fn parse_bytes_compact(bytes: &[u8], config: &Config) -> Result<CompactParseResult> {
        if bytes.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                DisplayStyle::Inline,
            )));
        }
        let mut parser = MtefCompactParser::new(bytes);
        if let Err(e) = parser.parse_header() {
            if config.strictness == Strictness::Lenient {
                return Ok(CompactParseResult::with_diagnostics(
                    CompactExpr::new(parser.arena, NodeSpan::EMPTY, DisplayStyle::Inline),
                    vec![Diagnostic {
                        kind: DiagnosticKind::Warning,
                        message: format!("MTEF header parse failed (recovered): {e}"),
                        span: None,
                    }],
                ));
            }
            return Err(e);
        }
        match parser.parse_object_list() {
            Ok(ids) => {
                let display = if parser.inline {
                    DisplayStyle::Inline
                } else {
                    DisplayStyle::Block
                };
                let roots = parser.arena.push_children(&ids);
                Ok(CompactParseResult::new(CompactExpr {
                    arena: parser.arena,
                    roots,
                    display,
                }))
            },
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let display = if parser.inline {
                        DisplayStyle::Inline
                    } else {
                        DisplayStyle::Block
                    };
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr {
                            arena: parser.arena,
                            roots: NodeSpan::EMPTY,
                            display,
                        },
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MTEF parse error (recovered): {e}"),
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

/// Decode hex string to bytes, skipping whitespace.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    let hex_bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !hex_bytes.len().is_multiple_of(2) {
        return Err(Error::Unsupported("MTEF hex string has odd length".into()));
    }
    let mut out = Vec::with_capacity(hex_bytes.len() / 2);
    for pair in hex_bytes.chunks_exact(2) {
        let hi = hex_val_byte(pair[0])?;
        let lo = hex_val_byte(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val_byte(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::Unsupported(format!("invalid hex char: {}", b as char))),
    }
}

// --- Record type constants ---
const END: u8 = 0;
const LINE: u8 = 1;
const CHAR: u8 = 2;
const TMPL: u8 = 3;
const PILE: u8 = 4;
const MATRIX: u8 = 5;
const EMBELL: u8 = 6;
const FONT: u8 = 8;
const SIZE: u8 = 9;
const FULL: u8 = 10;
const SUB: u8 = 11;
const SUB2: u8 = 12;
const SYM: u8 = 13;
const SUBSYM: u8 = 14;
const COLOR: u8 = 15;
const COLOR_DEF: u8 = 16;
const FONT_DEF: u8 = 17;
const EQN_PREFS: u8 = 18;
const ENCODING_DEF: u8 = 19;

// LINE option flags
const XF_NULL: u8 = 0x01;
const XF_RULER: u8 = 0x02;
const XF_LSPACE: u8 = 0x04;
const XF_LMOVE: u8 = 0x08;

// CHAR option flags (v5)
const CHAR_EMBELL: u8 = 0x01;
const CHAR_ENC_CHAR_8: u8 = 0x04;
const CHAR_NUDGE: u8 = 0x08;
const CHAR_ENC_CHAR_16: u8 = 0x10;
const CHAR_ENC_NO_MTCODE: u8 = 0x20;

// Embellishment codes
const EMB_DOT: u8 = 2;
const EMB_DDOT: u8 = 3;
const EMB_TDOT: u8 = 4;
const EMB_PRIME: u8 = 5;
const EMB_DPRIME: u8 = 6;
const EMB_TILDE: u8 = 8;
const EMB_HAT: u8 = 9;
const EMB_NOT: u8 = 10;
const EMB_RARROW: u8 = 11;
const EMB_MBAR: u8 = 16;
const EMB_OBAR: u8 = 17;
const EMB_BREVE: u8 = 20;

// Typeface slots
const TF_TEXT: u8 = 1;
const TF_FUNCTION: u8 = 2;
const TF_VARIABLE: u8 = 3;
const TF_LCGREEK: u8 = 4;
const TF_UCGREEK: u8 = 5;
const TF_SYMBOL: u8 = 6;
const TF_VECTOR: u8 = 7;
const TF_NUMBER: u8 = 8;
const TF_MTEXTRA: u8 = 11;

struct MtefCompactParser<'a> {
    data: &'a [u8],
    pos: usize,
    version: u8,
    platform: u8,
    inline: bool,
    fonts: Vec<String>,
    arena: AstArena,
}

impl<'a> MtefCompactParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            version: 5,
            platform: 1,
            inline: true,
            fonts: Vec::new(),
            arena: AstArena::new(),
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_byte(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(Error::Unsupported("unexpected end of MTEF data".into()));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        let lo = self.read_byte()? as u16;
        let hi = self.read_byte()? as u16;
        Ok(lo | (hi << 8))
    }

    fn skip_bytes(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.data.len());
    }

    fn read_null_terminated_string(&mut self) -> Result<String> {
        let start = self.pos;
        while self.pos < self.data.len() && self.data[self.pos] != 0 {
            self.pos += 1;
        }
        let s = String::from_utf8_lossy(&self.data[start..self.pos]).to_string();
        if self.pos < self.data.len() {
            self.pos += 1;
        }
        Ok(s)
    }

    fn empty_row(&mut self) -> NodeId {
        self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY))
    }

    fn wrap_ids(&mut self, ids: &[NodeId]) -> NodeId {
        match ids.len() {
            0 => self.empty_row(),
            1 => ids[0],
            _ => {
                let span = self.arena.push_children(ids);
                self.arena.push_node(CompactNode::Row(span))
            },
        }
    }

    fn parse_header(&mut self) -> Result<()> {
        if self.remaining() < 1 {
            return Ok(());
        }
        self.version = self.read_byte()?;

        match self.version {
            0 => {
                self.version = 5;
            },
            1 => {
                self.platform = 0;
            },
            101 => {
                self.version = 1;
                self.platform = 1;
            },
            2..=4 => {
                self.platform = self.read_byte()?;
                let _product = self.read_byte()?;
                let _ver = self.read_byte()?;
                let _ver_sub = self.read_byte()?;
            },
            5 => {
                self.platform = self.read_byte()?;
                let _product = self.read_byte()?;
                let _ver = self.read_byte()?;
                let _ver_sub = self.read_byte()?;
                let _app_key = self.read_null_terminated_string()?;
                let inline_byte = self.read_byte()?;
                self.inline = inline_byte != 0;
            },
            _ => {
                return Err(Error::Unsupported(format!(
                    "unsupported MTEF version: {}",
                    self.version
                )));
            },
        }
        Ok(())
    }

    fn get_tag_and_options(&mut self) -> Result<(u8, u8)> {
        let byte = self.read_byte()?;
        if byte == END {
            return Ok((END, 0));
        }
        if self.version >= 5 {
            let options = if self.pos < self.data.len() {
                self.read_byte()?
            } else {
                0
            };
            Ok((byte, options))
        } else {
            let tag = byte & 0x0F;
            let options = (byte >> 4) & 0x0F;
            Ok((tag, options))
        }
    }

    fn skip_nudge(&mut self) -> Result<()> {
        let b1 = self.read_byte()?;
        let b2 = self.read_byte()?;
        if b1 == 128 && b2 == 128 {
            self.skip_bytes(4);
        }
        Ok(())
    }

    fn parse_object_list(&mut self) -> Result<SmallVec<[NodeId; 8]>> {
        let mut nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved_pos = self.pos;
            let (tag, options) = self.get_tag_and_options()?;
            match tag {
                END => break,
                LINE => {
                    let line_ids = self.parse_line(options)?;
                    nodes.extend(line_ids);
                },
                CHAR => {
                    if let Some(id) = self.parse_char(options)? {
                        nodes.push(id);
                    }
                },
                TMPL => {
                    let id = self.parse_template(options)?;
                    nodes.push(id);
                },
                PILE => {
                    let id = self.parse_pile(options)?;
                    nodes.push(id);
                },
                MATRIX => {
                    let id = self.parse_matrix(options)?;
                    nodes.push(id);
                },
                EMBELL => {
                    self.skip_embell(options)?;
                },
                FONT => {
                    self.skip_font_record()?;
                },
                SIZE => {
                    self.skip_size_record()?;
                },
                FULL | SUB | SUB2 | SYM | SUBSYM => {},
                COLOR => {
                    self.skip_color()?;
                },
                COLOR_DEF => {
                    self.skip_color_def()?;
                },
                FONT_DEF => {
                    self.parse_font_def()?;
                },
                EQN_PREFS => {
                    self.skip_eqn_prefs()?;
                },
                ENCODING_DEF => {
                    let _name = self.read_null_terminated_string()?;
                },
                _ => {
                    if self.pos == saved_pos {
                        self.skip_bytes(1);
                    }
                },
            }
        }
        Ok(nodes)
    }

    fn parse_line(&mut self, options: u8) -> Result<SmallVec<[NodeId; 8]>> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        if options & XF_LSPACE != 0 {
            let _spacing = self.read_byte()?;
        }
        if options & XF_RULER != 0 {
            self.skip_ruler()?;
        }
        if options & XF_NULL != 0 {
            return Ok(SmallVec::new());
        }
        self.parse_object_list()
    }

    fn parse_char(&mut self, options: u8) -> Result<Option<NodeId>> {
        if options & CHAR_NUDGE != 0 {
            self.skip_nudge()?;
        }

        let typeface_raw = self.read_byte()?;
        let typeface = if typeface_raw >= 128 {
            typeface_raw - 128
        } else {
            typeface_raw
        };

        let character: u16;
        if self.version >= 5 {
            let mut mt_char: u16 = 0;
            if options & CHAR_ENC_NO_MTCODE == 0 {
                mt_char = self.read_u16_le()?;
            }
            if options & CHAR_ENC_CHAR_8 != 0 {
                character = self.read_byte()? as u16;
            } else if options & CHAR_ENC_CHAR_16 != 0 {
                character = self.read_u16_le()?;
            } else {
                character = mt_char;
            }
        } else {
            let ch = self.read_byte()? as u16;
            if self.platform == 1 && self.remaining() > 0 {
                let hi = self.read_byte()? as u16;
                character = ch | (hi << 8);
            } else {
                character = ch;
            }
        }

        let embells = if options & CHAR_EMBELL != 0 {
            self.parse_embell_list()?
        } else {
            vec![]
        };

        let unicode_char = resolve_mtef_char(typeface, character);
        let text = unicode_char.to_string();
        let sid = self.arena.push_str(&text);

        let base_id = match typeface {
            TF_NUMBER => self.arena.push_node(CompactNode::Number(sid)),
            TF_TEXT => self
                .arena
                .push_node(CompactNode::Text(TextStyle::Normal, sid)),
            TF_FUNCTION => self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits: LimitStyle::Default,
            }),
            TF_VARIABLE => self.arena.push_node(CompactNode::Identifier(sid)),
            TF_LCGREEK | TF_UCGREEK => self.arena.push_node(CompactNode::Identifier(sid)),
            TF_SYMBOL => {
                let kind = classify_symbol(unicode_char);
                self.arena.push_node(CompactNode::Symbol(kind, sid))
            },
            TF_VECTOR => self.arena.push_node(CompactNode::Identifier(sid)),
            _ => {
                if unicode_char.is_ascii_digit()
                    || (text.len() > 1 && text.chars().all(|c| c.is_ascii_digit()))
                {
                    self.arena.push_node(CompactNode::Number(sid))
                } else if is_operator_char(unicode_char) {
                    let kind = classify_symbol(unicode_char);
                    self.arena.push_node(CompactNode::Symbol(kind, sid))
                } else {
                    self.arena.push_node(CompactNode::Identifier(sid))
                }
            },
        };

        if embells.is_empty() {
            Ok(Some(base_id))
        } else {
            let mut id = base_id;
            for emb in embells {
                id = self.apply_embellishment(id, emb);
            }
            Ok(Some(id))
        }
    }

    fn parse_embell_list(&mut self) -> Result<Vec<u8>> {
        let mut embells = Vec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved = self.pos;
            let (tag, options) = self.get_tag_and_options()?;
            if tag != EMBELL {
                self.pos = saved;
                break;
            }
            if options & XF_LMOVE != 0 {
                self.skip_nudge()?;
            }
            let emb_type = self.read_byte()?;
            embells.push(emb_type);
        }
        if self.peek_byte() == Some(END) {
            self.get_tag_and_options()?;
        }
        Ok(embells)
    }

    fn skip_embell(&mut self, options: u8) -> Result<()> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let _emb_type = self.read_byte()?;
        Ok(())
    }

    fn parse_template(&mut self, options: u8) -> Result<NodeId> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let selector = self.read_byte()?;
        let var_byte = self.read_byte()?;
        let variation = if self.version >= 5 && (var_byte & 0x80) != 0 {
            let hi = self.read_byte()? as u16;
            ((var_byte & 0x7F) as u16) | (hi << 7)
        } else {
            var_byte as u16
        };
        let _tmpl_options = self.read_byte()?;

        let slots = self.parse_template_slots()?;
        self.template_to_compact(selector, variation, slots)
    }

    fn parse_template_slots(&mut self) -> Result<Vec<SmallVec<[NodeId; 8]>>> {
        let mut slots = Vec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved = self.pos;
            let (tag, options) = self.get_tag_and_options()?;
            match tag {
                END => break,
                LINE => {
                    let ids = self.parse_line(options)?;
                    slots.push(ids);
                },
                PILE => {
                    let pile_id = self.parse_pile(options)?;
                    slots.push(smallvec::smallvec![pile_id]);
                },
                _ => {
                    self.pos = saved;
                    break;
                },
            }
        }
        Ok(slots)
    }

    fn parse_pile(&mut self, options: u8) -> Result<NodeId> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let _halign = self.read_byte()?;
        if options & XF_RULER != 0 {
            self.skip_ruler()?;
        }

        let mut rows: Vec<SmallVec<[NodeId; 8]>> = Vec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved = self.pos;
            let (tag, opts) = self.get_tag_and_options()?;
            match tag {
                END => break,
                LINE => {
                    let ids = self.parse_line(opts)?;
                    rows.push(ids);
                },
                _ => {
                    self.pos = saved;
                    break;
                },
            }
        }

        // Pile → single-column Array
        let num_cols: u16 = 1;
        let align_id = self
            .arena
            .push_node(CompactNode::ColAlign(ColumnAlign::Center));
        let col_aligns = self.arena.push_children(&[align_id]);

        let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for row in &rows {
            let span = self.arena.push_children(row);
            cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
        }
        let cells = self.arena.push_children(&cell_ids);

        Ok(self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
        }))
    }

    fn parse_matrix(&mut self, options: u8) -> Result<NodeId> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let _valign = self.read_byte()?;
        let _halign = self.read_byte()?;
        let num_rows = self.read_byte()? as usize;
        let num_cols_raw = self.read_byte()? as usize;

        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for _ in 0..num_cols_raw {
            let _col_align = self.read_byte()?;
            align_ids.push(
                self.arena
                    .push_node(CompactNode::ColAlign(ColumnAlign::Center)),
            );
        }
        let col_aligns = self.arena.push_children(&align_ids);

        if options & XF_RULER != 0 {
            self.skip_ruler()?;
        }

        let num_cols = num_cols_raw.max(1) as u16;
        let mut cell_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for _ in 0..num_rows {
            for _ in 0..num_cols_raw {
                if self.remaining() == 0 {
                    cell_ids.push(self.empty_row());
                    continue;
                }
                let saved = self.pos;
                let (tag, opts) = self.get_tag_and_options()?;
                if tag == LINE {
                    let ids = self.parse_line(opts)?;
                    let span = self.arena.push_children(&ids);
                    cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
                } else {
                    self.pos = saved;
                    cell_ids.push(self.empty_row());
                }
            }
        }
        let cells = self.arena.push_children(&cell_ids);

        Ok(self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
        }))
    }

    fn skip_ruler(&mut self) -> Result<()> {
        if self.remaining() == 0 {
            return Ok(());
        }
        let count = self.read_byte()? as usize;
        for _ in 0..count {
            if self.remaining() >= 2 {
                self.skip_bytes(2);
            }
        }
        Ok(())
    }

    fn skip_font_record(&mut self) -> Result<()> {
        let _tface = self.read_byte()?;
        let _style = self.read_byte()?;
        Ok(())
    }

    fn skip_size_record(&mut self) -> Result<()> {
        let code = self.read_byte()?;
        if code == 101 {
            self.skip_bytes(2);
        }
        Ok(())
    }

    fn skip_color(&mut self) -> Result<()> {
        if self.version >= 5 {
            let _color_def_index = self.read_u16_le()?;
        } else {
            self.skip_bytes(2);
        }
        Ok(())
    }

    fn skip_color_def(&mut self) -> Result<()> {
        let _options = self.read_byte()?;
        self.skip_bytes(6);
        let _name = self.read_null_terminated_string()?;
        Ok(())
    }

    fn parse_font_def(&mut self) -> Result<()> {
        let _enc_def_index = self.read_byte()?;
        let name = self.read_null_terminated_string()?;
        self.fonts.push(name);
        Ok(())
    }

    fn skip_eqn_prefs(&mut self) -> Result<()> {
        if self.version >= 5 {
            let option = self.read_byte()?;
            if option & 0x01 != 0 {
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.skip_bytes(2);
                }
            }
            if option & 0x02 != 0 {
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.skip_bytes(2);
                }
            }
            if option & 0x04 != 0 {
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.skip_bytes(2);
                }
            }
        }
        Ok(())
    }

    // --- Template / embellishment conversion ---

    fn get_slot(&mut self, slots: &mut [SmallVec<[NodeId; 8]>], i: usize) -> NodeId {
        if i < slots.len() {
            let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[i]);
            self.wrap_ids(&ids)
        } else {
            self.empty_row()
        }
    }

    fn template_to_compact(
        &mut self,
        selector: u8,
        variation: u16,
        mut slots: Vec<SmallVec<[NodeId; 8]>>,
    ) -> Result<NodeId> {
        match selector {
            // Fences (selectors 0-12)
            0..=12 => {
                let (open, close) = fence_delimiters(selector, variation);
                let content_id = if !slots.is_empty() {
                    self.get_slot(&mut slots, 0)
                } else {
                    self.empty_row()
                };
                let content = self.arena.push_children(&[content_id]);
                let open_sid = self.arena.push_str(open);
                let close_sid = self.arena.push_str(close);
                Ok(self.arena.push_node(CompactNode::Fenced {
                    open: open_sid,
                    close: close_sid,
                    content,
                }))
            },
            // Root (selector 13)
            13 => {
                if variation == 0 {
                    let content = self.get_slot(&mut slots, 0);
                    Ok(self.arena.push_node(CompactNode::Sqrt(content)))
                } else {
                    let index = self.get_slot(&mut slots, 0);
                    let radicand = self.get_slot(&mut slots, 1);
                    Ok(self.arena.push_node(CompactNode::Root { index, radicand }))
                }
            },
            // Fraction (selector 14)
            14 => {
                let num = self.get_slot(&mut slots, 0);
                let den = self.get_slot(&mut slots, 1);
                Ok(self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::Normal,
                    num,
                    den,
                }))
            },
            // Scripts (selector 15)
            15 => match variation & 0x0F {
                0 => {
                    let base = self.get_slot(&mut slots, 0);
                    let sup = self.get_slot(&mut slots, 1);
                    Ok(self
                        .arena
                        .push_node(CompactNode::Superscript { base, sup }))
                },
                1 => {
                    let base = self.get_slot(&mut slots, 0);
                    let sub = self.get_slot(&mut slots, 1);
                    Ok(self.arena.push_node(CompactNode::Subscript { base, sub }))
                },
                2 => {
                    let base = self.get_slot(&mut slots, 0);
                    let sub = self.get_slot(&mut slots, 1);
                    let sup = self.get_slot(&mut slots, 2);
                    Ok(self
                        .arena
                        .push_node(CompactNode::SubSup { base, sub, sup }))
                },
                _ => Ok(self.get_slot(&mut slots, 0)),
            },
            // Underbar (selector 16)
            16 => {
                let base = self.get_slot(&mut slots, 0);
                let sid = self.arena.push_str("\u{0332}");
                let under = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
                Ok(self.arena.push_node(CompactNode::Underscript {
                    base,
                    under,
                    movable_limits: false,
                }))
            },
            // Overbar (selector 17)
            17 => {
                let base = self.get_slot(&mut slots, 0);
                let sid = self.arena.push_str("\u{203E}");
                let over = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
                Ok(self.arena.push_node(CompactNode::Overscript {
                    base,
                    over,
                    movable_limits: false,
                }))
            },
            // Arrows with labels (selectors 18-20)
            18..=20 => {
                let arrow_char = match selector {
                    18 => "\u{2190}",
                    19 => "\u{2192}",
                    _ => "\u{2194}",
                };
                let label = self.get_slot(&mut slots, 0);
                let sid = self.arena.push_str(arrow_char);
                let arrow = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Relation, sid));
                if variation == 0 {
                    Ok(self.arena.push_node(CompactNode::Overscript {
                        base: arrow,
                        over: label,
                        movable_limits: false,
                    }))
                } else {
                    Ok(self.arena.push_node(CompactNode::Underscript {
                        base: arrow,
                        under: label,
                        movable_limits: false,
                    }))
                }
            },
            // Integrals (selectors 21-26)
            21..=26 => {
                let op = match selector {
                    21 | 24 => '\u{222B}',
                    22 | 25 => '\u{222C}',
                    _ => '\u{222D}',
                };
                let limits = if selector >= 24 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op,
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Horizontal braces (selectors 27-28)
            27 => {
                let base = self.get_slot(&mut slots, 0);
                let label = self.get_slot(&mut slots, 1);
                let sid = self.arena.push_str("\u{23DF}");
                let brace = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
                let inner = self.arena.push_node(CompactNode::Underscript {
                    base,
                    under: brace,
                    movable_limits: false,
                });
                Ok(self.arena.push_node(CompactNode::Overscript {
                    base: inner,
                    over: label,
                    movable_limits: false,
                }))
            },
            28 => {
                let base = self.get_slot(&mut slots, 0);
                let label = self.get_slot(&mut slots, 1);
                let sid = self.arena.push_str("\u{23DE}");
                let brace = self
                    .arena
                    .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
                let inner = self.arena.push_node(CompactNode::Overscript {
                    base,
                    over: brace,
                    movable_limits: false,
                });
                Ok(self.arena.push_node(CompactNode::Underscript {
                    base: inner,
                    under: label,
                    movable_limits: false,
                }))
            },
            // Sums (29-30)
            29 | 30 => {
                let limits = if selector == 29 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op: '\u{2211}',
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Products (31-32)
            31 | 32 => {
                let limits = if selector == 31 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op: '\u{220F}',
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Coproducts (33-34)
            33 | 34 => {
                let limits = if selector == 33 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op: '\u{2210}',
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Unions (35-36)
            35 | 36 => {
                let limits = if selector == 35 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op: '\u{22C3}',
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Intersections (37-38)
            37 | 38 => {
                let limits = if selector == 37 {
                    LimitStyle::AboveBelow
                } else {
                    LimitStyle::SubSup
                };
                let (sub, sup, body) = self.parse_nary_slots(&mut slots, variation);
                Ok(self.arena.push_node(CompactNode::Nary {
                    op: '\u{22C2}',
                    sub,
                    sup,
                    body,
                    limits,
                }))
            },
            // Slash fraction (40)
            40 => {
                let num = self.get_slot(&mut slots, 0);
                let den = self.get_slot(&mut slots, 1);
                Ok(self.arena.push_node(CompactNode::Fraction {
                    kind: FractionKind::Skewed,
                    num,
                    den,
                }))
            },
            // Fallback
            _ => {
                let mut all: SmallVec<[NodeId; 8]> = SmallVec::new();
                for slot in &mut slots {
                    all.extend(std::mem::take(slot));
                }
                Ok(self.wrap_ids(&all))
            },
        }
    }

    /// Parse n-ary slots based on variation, returns (sub, sup, body).
    /// sub/sup are NodeId::NONE if absent.
    fn parse_nary_slots(
        &mut self,
        slots: &mut [SmallVec<[NodeId; 8]>],
        variation: u16,
    ) -> (NodeId, NodeId, NodeId) {
        let v = variation & 0x0F;
        match v {
            0 => {
                // lower only
                let sub = if !slots.is_empty() {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[0]);
                    self.wrap_ids(&ids)
                } else {
                    NodeId::NONE
                };
                let body = if slots.len() > 1 {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[1]);
                    self.wrap_ids(&ids)
                } else {
                    self.empty_row()
                };
                (sub, NodeId::NONE, body)
            },
            1 => {
                // both
                let sub = if !slots.is_empty() {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[0]);
                    self.wrap_ids(&ids)
                } else {
                    NodeId::NONE
                };
                let sup = if slots.len() > 1 {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[1]);
                    self.wrap_ids(&ids)
                } else {
                    NodeId::NONE
                };
                let body = if slots.len() > 2 {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[2]);
                    self.wrap_ids(&ids)
                } else {
                    self.empty_row()
                };
                (sub, sup, body)
            },
            2 => {
                // no limits
                let body = if !slots.is_empty() {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[0]);
                    self.wrap_ids(&ids)
                } else {
                    self.empty_row()
                };
                (NodeId::NONE, NodeId::NONE, body)
            },
            _ => {
                // Fallback: lower only
                let sub = if !slots.is_empty() {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[0]);
                    self.wrap_ids(&ids)
                } else {
                    NodeId::NONE
                };
                let body = if slots.len() > 1 {
                    let ids: SmallVec<[NodeId; 8]> = std::mem::take(&mut slots[1]);
                    self.wrap_ids(&ids)
                } else {
                    self.empty_row()
                };
                (sub, NodeId::NONE, body)
            },
        }
    }

    fn apply_embellishment(&mut self, base: NodeId, emb: u8) -> NodeId {
        let accent_char = match emb {
            EMB_DOT => '\u{0307}',
            EMB_DDOT => '\u{0308}',
            EMB_TDOT => '\u{20DB}',
            EMB_PRIME => '\u{2032}',
            EMB_DPRIME => '\u{2033}',
            EMB_TILDE => '\u{0303}',
            EMB_HAT => '\u{0302}',
            EMB_NOT => '\u{0338}',
            EMB_RARROW => '\u{20D7}',
            EMB_OBAR => '\u{0305}',
            EMB_MBAR => '\u{0332}',
            EMB_BREVE => '\u{0306}',
            _ => return base,
        };

        // Primes are appended, not accents
        if emb == EMB_PRIME || emb == EMB_DPRIME {
            let mut buf = [0u8; 4];
            let s = accent_char.encode_utf8(&mut buf);
            let sid = self.arena.push_str(s);
            let sym = self
                .arena
                .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
            let span = self.arena.push_children(&[base, sym]);
            return self.arena.push_node(CompactNode::Row(span));
        }

        let position = if emb == EMB_MBAR {
            AccentPosition::Below
        } else {
            AccentPosition::Above
        };

        self.arena.push_node(CompactNode::Accent {
            base,
            accent: accent_char,
            position,
        })
    }
}

// --- Free helper functions (no arena needed) ---

fn fence_delimiters(selector: u8, variation: u16) -> (&'static str, &'static str) {
    let (open, close) = match selector {
        0 => ("\u{27E8}", "\u{27E9}"),
        1 => ("(", ")"),
        2 => ("{", "}"),
        3 => ("[", "]"),
        4 => ("|", "|"),
        5 => ("\u{2016}", "\u{2016}"),
        6 => ("\u{230A}", "\u{230B}"),
        7 => ("\u{2308}", "\u{2309}"),
        8 => ("[", "["),
        9 => ("]", "]"),
        10 => ("]", "["),
        11 => ("[", ")"),
        12 => ("(", "]"),
        _ => ("(", ")"),
    };
    match variation {
        1 => (open, ""),
        2 => ("", close),
        _ => (open, close),
    }
}

fn resolve_mtef_char(typeface: u8, code: u16) -> char {
    match typeface {
        TF_LCGREEK => match code as u8 {
            0x61 => '\u{03B1}',
            0x62 => '\u{03B2}',
            0x63 => '\u{03C7}',
            0x64 => '\u{03B4}',
            0x65 => '\u{03B5}',
            0x66 => '\u{03C6}',
            0x67 => '\u{03B3}',
            0x68 => '\u{03B7}',
            0x69 => '\u{03B9}',
            0x6A => '\u{03D5}',
            0x6B => '\u{03BA}',
            0x6C => '\u{03BB}',
            0x6D => '\u{03BC}',
            0x6E => '\u{03BD}',
            0x6F => '\u{03BF}',
            0x70 => '\u{03C0}',
            0x71 => '\u{03B8}',
            0x72 => '\u{03C1}',
            0x73 => '\u{03C3}',
            0x74 => '\u{03C4}',
            0x75 => '\u{03C5}',
            0x76 => '\u{03D6}',
            0x77 => '\u{03C9}',
            0x78 => '\u{03BE}',
            0x79 => '\u{03C8}',
            0x7A => '\u{03B6}',
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_UCGREEK => match code as u8 {
            0x41 => '\u{0391}',
            0x42 => '\u{0392}',
            0x43 => '\u{03A7}',
            0x44 => '\u{0394}',
            0x45 => '\u{0395}',
            0x46 => '\u{03A6}',
            0x47 => '\u{0393}',
            0x48 => '\u{0397}',
            0x49 => '\u{0399}',
            0x4A => '\u{03D1}',
            0x4B => '\u{039A}',
            0x4C => '\u{039B}',
            0x4D => '\u{039C}',
            0x4E => '\u{039D}',
            0x4F => '\u{039F}',
            0x50 => '\u{03A0}',
            0x51 => '\u{0398}',
            0x52 => '\u{03A1}',
            0x53 => '\u{03A3}',
            0x54 => '\u{03A4}',
            0x55 => '\u{03A5}',
            0x56 => '\u{03C2}',
            0x57 => '\u{03A9}',
            0x58 => '\u{039E}',
            0x59 => '\u{03A8}',
            0x5A => '\u{0396}',
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_SYMBOL => match code as u8 {
            0x2B => '+',
            0x2D => '\u{2212}',
            0x3C => '<',
            0x3D => '=',
            0x3E => '>',
            0x40 => '\u{2245}',
            0x41 => '\u{0391}',
            0xB1 => '\u{00B1}',
            0xB4 => '\u{00D7}',
            0xB8 => '\u{00F7}',
            0xB9 => '\u{2260}',
            0xBA => '\u{2261}',
            0xBB => '\u{2248}',
            0xBC => '\u{2026}',
            0xC0 => '\u{2135}',
            0xC5 => '\u{220F}',
            0xC6 => '\u{221A}',
            0xC7 => '\u{22C5}',
            0xC8 => '\u{00AC}',
            0xC9 => '\u{2227}',
            0xCA => '\u{2228}',
            0xCB => '\u{21D4}',
            0xCC => '\u{21D0}',
            0xCD => '\u{21D1}',
            0xCE => '\u{21D2}',
            0xCF => '\u{21D3}',
            0xD1 => '\u{2207}',
            0xD4 => '\u{2122}',
            0xD5 => '\u{220F}',
            0xD6 => '\u{221A}',
            0xE0 => '\u{2660}',
            0xE5 => '\u{2200}',
            0xE6 => '\u{2203}',
            0xE7 => '\u{2205}',
            0xC4 => '\u{2297}',
            0xC3 => '\u{2295}',
            0xC2 => '\u{2284}',
            0xC1 => '\u{2283}',
            0xD9 => '\u{2229}',
            0xDA => '\u{222A}',
            0xDB => '\u{2283}',
            0xDC => '\u{2287}',
            0xDD => '\u{2284}',
            0xDE => '\u{2282}',
            0xDF => '\u{2286}',
            0xE2 => '\u{2208}',
            0xE3 => '\u{2209}',
            0xE4 => '\u{2220}',
            0xF1 => '\u{2329}',
            0xF2 => '\u{232A}',
            0xE8 => '\u{2205}',
            0xA5 => '\u{221E}',
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_MTEXTRA => match code as u8 {
            0x20 => '\u{200A}',
            0x25 => '\u{2032}',
            0x30 => '\u{210F}',
            0x40 => '\u{2113}',
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        _ => char::from_u32(code as u32).unwrap_or('?'),
    }
}

fn classify_symbol(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '\u{2212}' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}'
        | '\u{2227}' | '\u{2228}' | '\u{2295}' | '\u{2297}' => SymbolKind::BinaryOp,

        '=' | '<' | '>' | '\u{2260}' | '\u{2264}' | '\u{2265}' | '\u{226A}' | '\u{226B}'
        | '\u{2248}' | '\u{2261}' | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}'
        | '\u{2208}' | '\u{2209}' | '\u{21D2}' | '\u{21D4}' => SymbolKind::Relation,

        '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{2211}' | '\u{220F}' | '\u{2210}'
        | '\u{22C2}' | '\u{22C3}' => SymbolKind::LargeOp,

        '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}'
        | '\u{2308}' | '\u{2309}' | '\u{230A}' | '\u{230B}' => SymbolKind::Fence,

        _ => SymbolKind::Ordinary,
    }
}

fn is_operator_char(ch: char) -> bool {
    matches!(
        classify_symbol(ch),
        SymbolKind::BinaryOp | SymbolKind::Relation | SymbolKind::LargeOp | SymbolKind::Fence
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mtef_v5(body: &[u8]) -> Vec<u8> {
        let mut data = vec![
            5, // version
            1, // platform (Win)
            1, // product (MathType)
            5, // version major
            0, // version minor
            0, // null-terminated app key (empty)
            1, // inline
        ];
        data.extend_from_slice(body);
        data
    }

    fn parse_bytes(bytes: &[u8]) -> MathExpr {
        let config = Config::default();
        MtefReader::parse_bytes(bytes, &config).unwrap().expr
    }

    #[test]
    fn empty_input() {
        let expr = parse_bytes(&[]);
        assert!(expr.nodes.is_empty());
    }

    #[test]
    fn simple_number() {
        let data = make_mtef_v5(&[
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_NUMBER,
            0x32,
            0x00,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(!expr.nodes.is_empty());
        match &expr.nodes[0] {
            MathNode::Number(s) => assert_eq!(s, "2"),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn simple_identifier() {
        let data = make_mtef_v5(&[
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_VARIABLE,
            0x78,
            0x00,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(!expr.nodes.is_empty());
        match &expr.nodes[0] {
            MathNode::Identifier(s) => assert_eq!(s, "x"),
            other => panic!("expected Identifier, got {:?}", other),
        }
    }

    #[test]
    fn fraction_template() {
        let data = make_mtef_v5(&[
            TMPL,
            0x00,
            14,
            0x00,
            0x00,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_NUMBER,
            0x31,
            0x00,
            END,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_NUMBER,
            0x32,
            0x00,
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        match &expr.nodes[0] {
            MathNode::Fraction { num, den, .. } => {
                assert!(matches!(num.as_ref(), MathNode::Number(s) if s == "1"));
                assert!(matches!(den.as_ref(), MathNode::Number(s) if s == "2"));
            },
            other => panic!("expected Fraction, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_template() {
        let data = make_mtef_v5(&[
            TMPL,
            0x00,
            13,
            0x00,
            0x00,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_VARIABLE,
            0x78,
            0x00,
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(matches!(&expr.nodes[0], MathNode::Sqrt(_)));
    }

    #[test]
    fn subscript_template() {
        let data = make_mtef_v5(&[
            TMPL,
            0x00,
            15,
            0x01,
            0x00,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_VARIABLE,
            0x78,
            0x00,
            END,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_NUMBER,
            0x32,
            0x00,
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(matches!(&expr.nodes[0], MathNode::Subscript { .. }));
    }

    #[test]
    fn parentheses_template() {
        let data = make_mtef_v5(&[
            TMPL,
            0x00,
            1,
            0x00,
            0x00,
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_VARIABLE,
            0x78,
            0x00,
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        match &expr.nodes[0] {
            MathNode::Fenced { open, close, .. } => {
                assert_eq!(open, "(");
                assert_eq!(close, ")");
            },
            other => panic!("expected Fenced, got {:?}", other),
        }
    }

    #[test]
    fn hex_decode_roundtrip() {
        let bytes = vec![5, 1, 1, 5, 0, 0, 1, 0];
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let decoded = hex_decode(&hex).unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn greek_letter() {
        let data = make_mtef_v5(&[
            LINE,
            0x00,
            CHAR,
            0x00,
            128 + TF_LCGREEK,
            0x61,
            0x00,
            END,
        ]);
        let expr = parse_bytes(&data);
        match &expr.nodes[0] {
            MathNode::Identifier(s) => assert_eq!(s, "\u{03B1}"),
            other => panic!("expected Identifier(alpha), got {:?}", other),
        }
    }
}
