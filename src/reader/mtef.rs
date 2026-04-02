use crate::ast::*;
use crate::error::{Config, Error, ParseResult, Result};
use crate::reader::Reader;

pub struct MtefReader;

impl Reader for MtefReader {
    /// Parse MTEF binary input. Accepts hex-encoded binary string.
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        let bytes = hex_decode(input)?;
        Self::parse_bytes(&bytes, config)
    }
}

impl MtefReader {
    /// Parse raw MTEF binary bytes.
    pub fn parse_bytes(bytes: &[u8], _config: &Config) -> Result<ParseResult> {
        if bytes.is_empty() {
            return Ok(ParseResult::new(MathExpr::inline(vec![])));
        }
        let mut parser = MtefParser::new(bytes);
        parser.parse_header()?;
        let nodes = parser.parse_object_list()?;
        let display = if parser.inline {
            DisplayStyle::Inline
        } else {
            DisplayStyle::Block
        };
        Ok(ParseResult::new(MathExpr { nodes, display }))
    }
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() % 2 != 0 {
        return Err(Error::Unsupported("MTEF hex string has odd length".into()));
    }
    let mut out = Vec::with_capacity(chars.len() / 2);
    for chunk in chars.chunks(2) {
        let hi = hex_val(chunk[0])?;
        let lo = hex_val(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(c: char) -> Result<u8> {
    match c {
        '0'..='9' => Ok(c as u8 - b'0'),
        'a'..='f' => Ok(c as u8 - b'a' + 10),
        'A'..='F' => Ok(c as u8 - b'A' + 10),
        _ => Err(Error::Unsupported(format!("invalid hex char: {c}"))),
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
// const RULER: u8 = 7;
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
// const CHAR_FUNC_START: u8 = 0x02;
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

struct MtefParser<'a> {
    data: &'a [u8],
    pos: usize,
    version: u8,
    platform: u8,
    inline: bool,
    fonts: Vec<String>,
}

impl<'a> MtefParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            version: 5,
            platform: 1,
            inline: true,
            fonts: Vec::new(),
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

    fn read_null_terminated_string(&mut self) -> Result<String> {
        let start = self.pos;
        while self.pos < self.data.len() && self.data[self.pos] != 0 {
            self.pos += 1;
        }
        let s = String::from_utf8_lossy(&self.data[start..self.pos]).to_string();
        if self.pos < self.data.len() {
            self.pos += 1; // skip null terminator
        }
        Ok(s)
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
                self.platform = 0; // Mac
            },
            101 => {
                self.version = 1;
                self.platform = 1; // Win
            },
            2 | 3 | 4 => {
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
        // END tag (0) has no options byte in any version
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
            // long nudge: 4 more bytes
            self.pos += 4;
        }
        Ok(())
    }

    fn parse_object_list(&mut self) -> Result<Vec<MathNode>> {
        let mut nodes = Vec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved_pos = self.pos;
            let (tag, options) = self.get_tag_and_options()?;
            match tag {
                END => break,
                LINE => {
                    let line_nodes = self.parse_line(options)?;
                    nodes.extend(line_nodes);
                },
                CHAR => {
                    if let Some(node) = self.parse_char(options)? {
                        nodes.push(node);
                    }
                },
                TMPL => {
                    let node = self.parse_template(options)?;
                    nodes.push(node);
                },
                PILE => {
                    let node = self.parse_pile(options)?;
                    nodes.push(node);
                },
                MATRIX => {
                    let node = self.parse_matrix(options)?;
                    nodes.push(node);
                },
                EMBELL => {
                    // Standalone embellishment (unusual)
                    self.skip_embell(options)?;
                },
                FONT => {
                    self.skip_font_record()?;
                },
                SIZE => {
                    self.skip_size_record()?;
                },
                FULL | SUB | SUB2 | SYM | SUBSYM => {
                    // Size changes - skip
                },
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
                    // Unknown record, try to recover
                    if self.pos == saved_pos {
                        self.pos += 1;
                    }
                },
            }
        }
        Ok(nodes)
    }

    fn parse_line(&mut self, options: u8) -> Result<Vec<MathNode>> {
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
            return Ok(vec![]);
        }
        self.parse_object_list()
    }

    fn parse_char(&mut self, options: u8) -> Result<Option<MathNode>> {
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

        // Parse embellishment list if present
        let embells = if options & CHAR_EMBELL != 0 {
            self.parse_embell_list()?
        } else {
            vec![]
        };

        // Resolve character to Unicode
        let unicode_char = resolve_mtef_char(typeface, character);
        let text = unicode_char.to_string();

        // Build the base node based on typeface
        let base_node = match typeface {
            TF_NUMBER => MathNode::Number(text),
            TF_TEXT => MathNode::Text(TextStyle::Normal, text),
            TF_FUNCTION => MathNode::Operator(OperatorAttr {
                name: text,
                limit_style: LimitStyle::Default,
            }),
            TF_VARIABLE => MathNode::Identifier(text),
            TF_LCGREEK | TF_UCGREEK => MathNode::Identifier(text),
            TF_SYMBOL => {
                let kind = classify_symbol(unicode_char);
                MathNode::Symbol(kind, text)
            },
            TF_VECTOR => MathNode::Identifier(text),
            _ => {
                if unicode_char.is_ascii_digit() || (text.len() > 1 && text.chars().all(|c| c.is_ascii_digit())) {
                    MathNode::Number(text)
                } else if is_operator_char(unicode_char) {
                    let kind = classify_symbol(unicode_char);
                    MathNode::Symbol(kind, text)
                } else {
                    MathNode::Identifier(text)
                }
            },
        };

        // Apply embellishments
        if embells.is_empty() {
            Ok(Some(base_node))
        } else {
            let mut node = base_node;
            for emb in embells {
                node = apply_embellishment(node, emb);
            }
            Ok(Some(node))
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
                self.pos = saved; // put it back
                break;
            }
            if options & XF_LMOVE != 0 {
                self.skip_nudge()?;
            }
            let emb_type = self.read_byte()?;
            embells.push(emb_type);
        }
        // Consume trailing END if present
        if self.peek_byte() == Some(END) {
            let _ = self.get_tag_and_options();
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

    fn parse_template(&mut self, options: u8) -> Result<MathNode> {
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

        // Parse subobjects (slots)
        let slots = self.parse_template_slots()?;

        template_to_ast(selector, variation, slots)
    }

    fn parse_template_slots(&mut self) -> Result<Vec<Vec<MathNode>>> {
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
                    let nodes = self.parse_line(options)?;
                    slots.push(nodes);
                },
                PILE => {
                    let pile_node = self.parse_pile(options)?;
                    slots.push(vec![pile_node]);
                },
                _ => {
                    // Not a slot-level record, put it back
                    self.pos = saved;
                    break;
                },
            }
        }
        Ok(slots)
    }

    fn parse_pile(&mut self, options: u8) -> Result<MathNode> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let _halign = self.read_byte()?;
        if options & XF_RULER != 0 {
            self.skip_ruler()?;
        }

        let mut rows: Vec<Vec<MathNode>> = Vec::new();
        loop {
            if self.remaining() == 0 {
                break;
            }
            let saved = self.pos;
            let (tag, opts) = self.get_tag_and_options()?;
            match tag {
                END => break,
                LINE => {
                    let nodes = self.parse_line(opts)?;
                    rows.push(nodes);
                },
                _ => {
                    self.pos = saved;
                    break;
                },
            }
        }

        // A pile is a vertical stack of lines — map to Array with single-column rows
        let array_rows: Vec<Vec<Vec<MathNode>>> = rows
            .into_iter()
            .map(|row| vec![row])
            .collect();
        Ok(MathNode::Array {
            columns: vec![ColumnAlign::Center],
            rows: array_rows,
        })
    }

    fn parse_matrix(&mut self, options: u8) -> Result<MathNode> {
        if options & XF_LMOVE != 0 {
            self.skip_nudge()?;
        }
        let _valign = self.read_byte()?;
        let _halign = self.read_byte()?;
        let num_rows = self.read_byte()? as usize;
        let num_cols = self.read_byte()? as usize;

        // Read column alignment for each column
        let mut columns = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            let _col_align = self.read_byte()?;
            columns.push(ColumnAlign::Center);
        }

        if options & XF_RULER != 0 {
            self.skip_ruler()?;
        }

        // Read cells: num_rows * num_cols LINE records
        let mut rows = Vec::with_capacity(num_rows);
        for _ in 0..num_rows {
            let mut row = Vec::with_capacity(num_cols);
            for _ in 0..num_cols {
                if self.remaining() == 0 {
                    row.push(vec![]);
                    continue;
                }
                let saved = self.pos;
                let (tag, opts) = self.get_tag_and_options()?;
                if tag == LINE {
                    let nodes = self.parse_line(opts)?;
                    row.push(nodes);
                } else {
                    self.pos = saved;
                    row.push(vec![]);
                }
            }
            rows.push(row);
        }

        Ok(MathNode::Array { columns, rows })
    }

    fn skip_ruler(&mut self) -> Result<()> {
        // Ruler: count byte + count * 2 bytes (tab stops)
        if self.remaining() == 0 {
            return Ok(());
        }
        let count = self.read_byte()? as usize;
        for _ in 0..count {
            if self.remaining() >= 2 {
                self.pos += 2;
            }
        }
        Ok(())
    }

    fn skip_font_record(&mut self) -> Result<()> {
        // FONT record: font index (in v5, read font_def_index)
        if self.version >= 5 {
            let _tface = self.read_byte()?;
            let _style = self.read_byte()?;
        } else {
            let _tface = self.read_byte()?;
            let _style = self.read_byte()?;
        }
        Ok(())
    }

    fn skip_size_record(&mut self) -> Result<()> {
        // SIZE: size_code + optional explicit size
        let code = self.read_byte()?;
        if code == 101 {
            // explicit point size (2 bytes)
            self.pos += 2;
        }
        Ok(())
    }

    fn skip_color(&mut self) -> Result<()> {
        if self.version >= 5 {
            let _color_def_index = self.read_u16_le()?;
        } else {
            self.pos += 2; // RGB-ish
        }
        Ok(())
    }

    fn skip_color_def(&mut self) -> Result<()> {
        let _options = self.read_byte()?;
        // RGB values
        self.pos += 6; // 3 x u16
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
                // sizes: read count + data
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.pos += 2; // 16-bit size
                }
            }
            if option & 0x02 != 0 {
                // spacing: read count + data
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.pos += 2;
                }
            }
            if option & 0x04 != 0 {
                // styles: read count + data
                let count = self.read_byte()? as usize;
                for _ in 0..count {
                    self.pos += 2;
                }
            }
        }
        Ok(())
    }
}

fn wrap_nodes(nodes: Vec<MathNode>) -> MathNode {
    match nodes.len() {
        0 => MathNode::Row(vec![]),
        1 => nodes.into_iter().next().unwrap(),
        _ => MathNode::Row(nodes),
    }
}

fn template_to_ast(selector: u8, variation: u16, mut slots: Vec<Vec<MathNode>>) -> Result<MathNode> {
    let get_slot = |slots: &mut Vec<Vec<MathNode>>, i: usize| -> MathNode {
        if i < slots.len() {
            wrap_nodes(std::mem::take(&mut slots[i]))
        } else {
            MathNode::Row(vec![])
        }
    };

    match selector {
        // Fences (selectors 0-12)
        0..=12 => {
            let (open, close) = fence_delimiters(selector, variation);
            let content_node = if !slots.is_empty() {
                get_slot(&mut slots, 0)
            } else {
                MathNode::Row(vec![])
            };
            Ok(MathNode::Fenced {
                open: open.into(),
                close: close.into(),
                content: vec![FencedItem::Element(content_node)],
            })
        },
        // Root (selector 13)
        13 => {
            if variation == 0 {
                // Square root
                let content = get_slot(&mut slots, 0);
                Ok(MathNode::Sqrt(Box::new(content)))
            } else {
                // Nth root
                let index = get_slot(&mut slots, 0);
                let radicand = get_slot(&mut slots, 1);
                Ok(MathNode::Root {
                    index: Box::new(index),
                    radicand: Box::new(radicand),
                })
            }
        },
        // Fraction (selector 14)
        14 => {
            let num = get_slot(&mut slots, 0);
            let den = get_slot(&mut slots, 1);
            Ok(MathNode::Fraction {
                kind: FractionKind::Normal,
                num: Box::new(num),
                den: Box::new(den),
            })
        },
        // Scripts (selector 15)
        15 => {
            match variation & 0x0F {
                0 => {
                    // Superscript
                    let base = get_slot(&mut slots, 0);
                    let sup = get_slot(&mut slots, 1);
                    Ok(MathNode::Superscript {
                        base: Box::new(base),
                        sup: Box::new(sup),
                    })
                },
                1 => {
                    // Subscript
                    let base = get_slot(&mut slots, 0);
                    let sub = get_slot(&mut slots, 1);
                    Ok(MathNode::Subscript {
                        base: Box::new(base),
                        sub: Box::new(sub),
                    })
                },
                2 => {
                    // SubSup
                    let base = get_slot(&mut slots, 0);
                    let sub = get_slot(&mut slots, 1);
                    let sup = get_slot(&mut slots, 2);
                    Ok(MathNode::SubSup {
                        base: Box::new(base),
                        sub: Box::new(sub),
                        sup: Box::new(sup),
                    })
                },
                _ => {
                    Ok(get_slot(&mut slots, 0))
                },
            }
        },
        // Underbar (selector 16)
        16 => {
            let base = get_slot(&mut slots, 0);
            Ok(MathNode::Underscript {
                base: Box::new(base),
                under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{0332}".into())),
                movable_limits: false,
            })
        },
        // Overbar (selector 17)
        17 => {
            let base = get_slot(&mut slots, 0);
            Ok(MathNode::Overscript {
                base: Box::new(base),
                over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{203E}".into())),
                movable_limits: false,
            })
        },
        // Arrows with labels (selectors 18-20)
        18..=20 => {
            let arrow_char = match selector {
                18 => '\u{2190}', // leftarrow
                19 => '\u{2192}', // rightarrow
                _ => '\u{2194}',  // leftrightarrow
            };
            let label = get_slot(&mut slots, 0);
            let arrow = MathNode::Symbol(SymbolKind::Relation, arrow_char.to_string());
            if variation == 0 {
                Ok(MathNode::Overscript {
                    base: Box::new(arrow),
                    over: Box::new(label),
                    movable_limits: false,
                })
            } else {
                Ok(MathNode::Underscript {
                    base: Box::new(arrow),
                    under: Box::new(label),
                    movable_limits: false,
                })
            }
        },
        // Integrals (selectors 21-26)
        21..=26 => {
            let op = match selector {
                21 | 24 => '\u{222B}', // single integral
                22 | 25 => '\u{222C}', // double integral
                _ => '\u{222D}',        // triple integral
            };
            let limits = if selector >= 24 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op,
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Horizontal braces (selectors 27-28)
        27 => {
            let base = get_slot(&mut slots, 0);
            let label = get_slot(&mut slots, 1);
            Ok(MathNode::Overscript {
                base: Box::new(MathNode::Underscript {
                    base: Box::new(base),
                    under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{23DF}".into())),
                    movable_limits: false,
                }),
                over: Box::new(label),
                movable_limits: false,
            })
        },
        28 => {
            let base = get_slot(&mut slots, 0);
            let label = get_slot(&mut slots, 1);
            Ok(MathNode::Underscript {
                base: Box::new(MathNode::Overscript {
                    base: Box::new(base),
                    over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{23DE}".into())),
                    movable_limits: false,
                }),
                under: Box::new(label),
                movable_limits: false,
            })
        },
        // Sums (selectors 29-30)
        29 | 30 => {
            let limits = if selector == 29 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op: '\u{2211}',
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Products (selectors 31-32)
        31 | 32 => {
            let limits = if selector == 31 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op: '\u{220F}',
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Coproducts (selectors 33-34)
        33 | 34 => {
            let limits = if selector == 33 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op: '\u{2210}',
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Unions (selectors 35-36)
        35 | 36 => {
            let limits = if selector == 35 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op: '\u{22C3}',
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Intersections (selectors 37-38)
        37 | 38 => {
            let limits = if selector == 37 {
                LimitStyle::AboveBelow
            } else {
                LimitStyle::SubSup
            };
            let (sub, sup, body) = parse_nary_slots(&mut slots, variation);
            Ok(MathNode::Nary {
                op: '\u{22C2}',
                sub,
                sup,
                body: Box::new(body),
                limits,
            })
        },
        // Long division (39), slash fraction (40), big operators (41-47)
        40 => {
            let num = get_slot(&mut slots, 0);
            let den = get_slot(&mut slots, 1);
            Ok(MathNode::Fraction {
                kind: FractionKind::Skewed,
                num: Box::new(num),
                den: Box::new(den),
            })
        },
        // Fallback: return content as a row
        _ => {
            let all: Vec<MathNode> = slots.into_iter().flatten().collect();
            Ok(wrap_nodes(all))
        },
    }
}

fn parse_nary_slots(
    slots: &mut Vec<Vec<MathNode>>,
    variation: u16,
) -> (Option<Box<MathNode>>, Option<Box<MathNode>>, MathNode) {
    let v = variation & 0x0F;
    match v {
        0 => {
            // lower only
            let sub = if !slots.is_empty() {
                Some(Box::new(wrap_nodes(std::mem::take(&mut slots[0]))))
            } else {
                None
            };
            let body = if slots.len() > 1 {
                wrap_nodes(std::mem::take(&mut slots[1]))
            } else {
                MathNode::Row(vec![])
            };
            (sub, None, body)
        },
        1 => {
            // both
            let sub = if !slots.is_empty() {
                Some(Box::new(wrap_nodes(std::mem::take(&mut slots[0]))))
            } else {
                None
            };
            let sup = if slots.len() > 1 {
                Some(Box::new(wrap_nodes(std::mem::take(&mut slots[1]))))
            } else {
                None
            };
            let body = if slots.len() > 2 {
                wrap_nodes(std::mem::take(&mut slots[2]))
            } else {
                MathNode::Row(vec![])
            };
            (sub, sup, body)
        },
        2 => {
            // no limits
            let body = if !slots.is_empty() {
                wrap_nodes(std::mem::take(&mut slots[0]))
            } else {
                MathNode::Row(vec![])
            };
            (None, None, body)
        },
        _ => {
            // Fallback: treat like lower only
            let sub = if !slots.is_empty() {
                Some(Box::new(wrap_nodes(std::mem::take(&mut slots[0]))))
            } else {
                None
            };
            let body = if slots.len() > 1 {
                wrap_nodes(std::mem::take(&mut slots[1]))
            } else {
                MathNode::Row(vec![])
            };
            (sub, None, body)
        },
    }
}

fn fence_delimiters(selector: u8, variation: u16) -> (&'static str, &'static str) {
    let (open, close) = match selector {
        0 => ("\u{27E8}", "\u{27E9}"), // angle brackets
        1 => ("(", ")"),
        2 => ("{", "}"),
        3 => ("[", "]"),
        4 => ("|", "|"),
        5 => ("\u{2016}", "\u{2016}"), // double bar
        6 => ("\u{230A}", "\u{230B}"), // floor
        7 => ("\u{2308}", "\u{2309}"), // ceiling
        8 => ("[", "["),
        9 => ("]", "]"),
        10 => ("]", "["),
        11 => ("[", ")"),
        12 => ("(", "]"),
        _ => ("(", ")"),
    };
    match variation {
        1 => (open, ""),   // left only
        2 => ("", close),  // right only
        _ => (open, close),
    }
}

fn resolve_mtef_char(typeface: u8, code: u16) -> char {
    match typeface {
        TF_LCGREEK => match code as u8 {
            0x61 => '\u{03B1}', // alpha
            0x62 => '\u{03B2}', // beta
            0x63 => '\u{03C7}', // chi
            0x64 => '\u{03B4}', // delta
            0x65 => '\u{03B5}', // epsilon
            0x66 => '\u{03C6}', // phi
            0x67 => '\u{03B3}', // gamma
            0x68 => '\u{03B7}', // eta
            0x69 => '\u{03B9}', // iota
            0x6A => '\u{03D5}', // phi variant
            0x6B => '\u{03BA}', // kappa
            0x6C => '\u{03BB}', // lambda
            0x6D => '\u{03BC}', // mu
            0x6E => '\u{03BD}', // nu
            0x6F => '\u{03BF}', // omicron
            0x70 => '\u{03C0}', // pi
            0x71 => '\u{03B8}', // theta
            0x72 => '\u{03C1}', // rho
            0x73 => '\u{03C3}', // sigma
            0x74 => '\u{03C4}', // tau
            0x75 => '\u{03C5}', // upsilon
            0x76 => '\u{03D6}', // pi variant
            0x77 => '\u{03C9}', // omega
            0x78 => '\u{03BE}', // xi
            0x79 => '\u{03C8}', // psi
            0x7A => '\u{03B6}', // zeta
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_UCGREEK => match code as u8 {
            0x41 => '\u{0391}', // Alpha
            0x42 => '\u{0392}', // Beta
            0x43 => '\u{03A7}', // Chi
            0x44 => '\u{0394}', // Delta
            0x45 => '\u{0395}', // Epsilon
            0x46 => '\u{03A6}', // Phi
            0x47 => '\u{0393}', // Gamma
            0x48 => '\u{0397}', // Eta
            0x49 => '\u{0399}', // Iota
            0x4A => '\u{03D1}', // Theta variant
            0x4B => '\u{039A}', // Kappa
            0x4C => '\u{039B}', // Lambda
            0x4D => '\u{039C}', // Mu
            0x4E => '\u{039D}', // Nu
            0x4F => '\u{039F}', // Omicron
            0x50 => '\u{03A0}', // Pi
            0x51 => '\u{0398}', // Theta
            0x52 => '\u{03A1}', // Rho
            0x53 => '\u{03A3}', // Sigma
            0x54 => '\u{03A4}', // Tau
            0x55 => '\u{03A5}', // Upsilon
            0x56 => '\u{03C2}', // sigma final
            0x57 => '\u{03A9}', // Omega
            0x58 => '\u{039E}', // Xi
            0x59 => '\u{03A8}', // Psi
            0x5A => '\u{0396}', // Zeta
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_SYMBOL => match code as u8 {
            0x2B => '+',
            0x2D => '\u{2212}', // minus
            0x3C => '<',
            0x3D => '=',
            0x3E => '>',
            0x40 => '\u{2245}', // approximately equal
            0x41 => '\u{0391}',
            0xB1 => '\u{00B1}', // plus-minus
            0xB4 => '\u{00D7}', // times
            0xB8 => '\u{00F7}', // division
            0xB9 => '\u{2260}', // not equal
            0xBA => '\u{2261}', // identical
            0xBB => '\u{2248}', // almost equal
            0xBC => '\u{2026}', // ellipsis
            0xC0 => '\u{2135}', // alef
            0xC5 => '\u{220F}', // product
            0xC6 => '\u{221A}', // radical
            0xC7 => '\u{22C5}', // dot operator
            0xC8 => '\u{00AC}', // not
            0xC9 => '\u{2227}', // and
            0xCA => '\u{2228}', // or
            0xCB => '\u{21D4}', // iff
            0xCC => '\u{21D0}', // left double arrow
            0xCD => '\u{21D1}', // up double arrow
            0xCE => '\u{21D2}', // right double arrow
            0xCF => '\u{21D3}', // down double arrow
            0xD1 => '\u{2207}', // nabla
            0xD4 => '\u{2122}', // trademark
            0xD5 => '\u{220F}', // product (alt)
            0xD6 => '\u{221A}', // radical (alt)
            0xE0 => '\u{2660}', // spade
            0xE5 => '\u{2200}', // for all
            0xE6 => '\u{2203}', // exists
            0xE7 => '\u{2205}', // empty set
            0xC4 => '\u{2297}', // circled times
            0xC3 => '\u{2295}', // circled plus
            0xC2 => '\u{2284}', // not subset
            0xC1 => '\u{2283}', // superset
            0xD9 => '\u{2229}', // intersection
            0xDA => '\u{222A}', // union
            0xDB => '\u{2283}', // superset
            0xDC => '\u{2287}', // superset eq
            0xDD => '\u{2284}', // not subset
            0xDE => '\u{2282}', // subset
            0xDF => '\u{2286}', // subset eq
            0xE2 => '\u{2208}', // element of
            0xE3 => '\u{2209}', // not element of
            0xE4 => '\u{2220}', // angle
            0xF1 => '\u{2329}', // left angle bracket
            0xF2 => '\u{232A}', // right angle bracket
            0xE8 => '\u{2205}', // empty set
            0xA5 => '\u{221E}', // infinity
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        TF_MTEXTRA => match code as u8 {
            0x20 => '\u{200A}', // hair space
            0x25 => '\u{2032}', // prime
            0x30 => '\u{210F}', // hbar
            0x40 => '\u{2113}', // script l
            _ => char::from_u32(code as u32).unwrap_or('?'),
        },
        _ => {
            // For TEXT, FUNCTION, VARIABLE, NUMBER, VECTOR, etc.
            // These use standard character codes
            char::from_u32(code as u32).unwrap_or('?')
        },
    }
}

fn classify_symbol(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '\u{2212}' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}'
        | '\u{22C5}' | '\u{2227}' | '\u{2228}' | '\u{2295}' | '\u{2297}' => SymbolKind::BinaryOp,

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
    matches!(classify_symbol(ch), SymbolKind::BinaryOp | SymbolKind::Relation | SymbolKind::LargeOp | SymbolKind::Fence)
}

fn apply_embellishment(base: MathNode, emb: u8) -> MathNode {
    let accent_char = match emb {
        EMB_DOT => '\u{0307}',    // combining dot above
        EMB_DDOT => '\u{0308}',   // combining diaeresis
        EMB_TDOT => '\u{20DB}',   // combining three dots above
        EMB_PRIME => '\u{2032}',  // prime (not an accent, but append)
        EMB_DPRIME => '\u{2033}', // double prime
        EMB_TILDE => '\u{0303}',  // combining tilde
        EMB_HAT => '\u{0302}',    // combining circumflex
        EMB_NOT => '\u{0338}',    // combining long solidus overlay
        EMB_RARROW => '\u{20D7}', // combining right arrow above
        EMB_OBAR => '\u{0305}',   // combining overline
        EMB_MBAR => '\u{0332}',   // combining low line
        EMB_BREVE => '\u{0306}',  // combining breve
        _ => return base,         // unknown embellishment, skip
    };

    // Primes are appended, not accents
    if emb == EMB_PRIME || emb == EMB_DPRIME {
        return MathNode::Row(vec![
            base,
            MathNode::Symbol(SymbolKind::Ordinary, accent_char.to_string()),
        ]);
    }

    let position = if emb == EMB_MBAR {
        AccentPosition::Below
    } else {
        AccentPosition::Above
    };

    MathNode::Accent {
        base: Box::new(base),
        accent: accent_char,
        position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mtef_v5(body: &[u8]) -> Vec<u8> {
        let mut data = vec![
            5,    // version
            1,    // platform (Win)
            1,    // product (MathType)
            5,    // version major
            0,    // version minor
            0,    // null-terminated app key (empty)
            1,    // inline
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
        // MTEF v5: LINE with a CHAR record for '2' in NUMBER typeface
        let data = make_mtef_v5(&[
            LINE, 0x00, // LINE tag, no options
            CHAR, 0x00, // CHAR tag, no options
            128 + TF_NUMBER, // typeface = NUMBER (slot 8 + 128)
            0x32, 0x00, // mt_char = 0x0032 = '2'
            END,  // end of LINE
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
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_VARIABLE, // typeface = VARIABLE
            0x78, 0x00, // 'x'
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
        // Template selector 14 (fraction), variation 0
        let data = make_mtef_v5(&[
            TMPL, 0x00, // TMPL tag, no options
            14,         // selector: fraction
            0x00,       // variation: full fraction
            0x00,       // options
            // Slot 0 (numerator): LINE with '1'
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_NUMBER,
            0x31, 0x00, // '1'
            END,
            // Slot 1 (denominator): LINE with '2'
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_NUMBER,
            0x32, 0x00, // '2'
            END,
            END, // end of template slots
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
            TMPL, 0x00,
            13,         // selector: root
            0x00,       // variation: square root
            0x00,
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_VARIABLE,
            0x78, 0x00, // 'x'
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(matches!(&expr.nodes[0], MathNode::Sqrt(_)));
    }

    #[test]
    fn subscript_template() {
        let data = make_mtef_v5(&[
            TMPL, 0x00,
            15,         // selector: scripts
            0x01,       // variation: subscript
            0x00,
            // base
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_VARIABLE,
            0x78, 0x00,
            END,
            // subscript
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_NUMBER,
            0x32, 0x00,
            END,
            END,
        ]);
        let expr = parse_bytes(&data);
        assert!(matches!(&expr.nodes[0], MathNode::Subscript { .. }));
    }

    #[test]
    fn parentheses_template() {
        let data = make_mtef_v5(&[
            TMPL, 0x00,
            1,          // selector: parentheses
            0x00,       // variation: both
            0x00,
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_VARIABLE,
            0x78, 0x00,
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
            LINE, 0x00,
            CHAR, 0x00,
            128 + TF_LCGREEK,
            0x61, 0x00, // alpha
            END,
        ]);
        let expr = parse_bytes(&data);
        match &expr.nodes[0] {
            MathNode::Identifier(s) => assert_eq!(s, "\u{03B1}"),
            other => panic!("expected Identifier(alpha), got {:?}", other),
        }
    }
}
