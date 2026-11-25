//! Template (TMPL record) selectors and variations for MTEF 3 and MTEF 5, and
//! their mapping onto AST nodes.
//!
//! Slot order follows the template class (MTEFv3.md / MTEFv5.md "Template
//! subobject order"), with one correction confirmed by files written by
//! MathType and Equation Editor: BigOpBoxClass templates store the main slot,
//! then the *lower* limit, then the *upper* limit, then the operator character.
//! Likewise MTEF 5 writes the limit presence bits of BigOpBoxClass and
//! LimBoxClass variations as 0x10 (lower) and 0x20 (upper), not 0x01 and 0x02.

use smallvec::SmallVec;

use super::decode::{Decoder, Glyph, Item, ScriptSlots, SizeLoss, SlotSizes};
use crate::ast::compact::{CompactNode, NodeId};
use crate::ast::{
    AccentPosition, FontSize, FractionKind, LimitStyle, NodeAttrs, Rational, StrokeKind, SymbolKind,
};
use crate::error::Result;

/// A template, reduced to what the AST can express.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Desc {
    /// ParBoxClass: main slot, then the fence characters.
    Fence {
        open: &'static str,
        close: &'static str,
    },
    /// RootBoxClass: radicand, then index.
    Root,
    /// FracBoxClass / SlashBoxClass: numerator, denominator.
    Fraction(FractionKind),
    /// BarBoxClass: one slot.
    Bar { under: bool, double: bool },
    /// ArroBoxClass: label slot(s), then the arrow character. MTEF 3 has one
    /// label slot whose position is fixed by the variation; MTEF 5 has a top
    /// and a bottom slot.
    Arrow {
        arrow: char,
        v3_label_on_top: Option<bool>,
    },
    /// BigOpBoxClass: main, lower, upper, operator character(s). `op` is
    /// `None` when the operator is the template's character.
    BigOp {
        op: Option<char>,
        limits: LimitStyle,
    },
    /// LimBoxClass: main, lower, upper.
    Limit,
    /// HBrBoxClass / HFenceBoxClass: main slot, label slot, brace character.
    HBrace { top: bool, glyph: char },
    /// LDivBoxClass: dividend, quotient.
    LongDivision,
    /// ScrBoxClass: subscript, superscript.
    Script { pre: bool },
    /// DiracBoxClass: left slot, right slot, bracket characters.
    Dirac { left: bool, right: bool },
    /// VectorBoxClass, ArcBoxClass and HatBoxClass: one slot under an accent.
    Accent {
        accent: char,
        below: bool,
        wide: bool,
    },
    /// StrikeBoxClass: one slot.
    Strike(StrokeKind),
    /// TBoxBoxClass: one slot; `sides` holds the `tvBX_LEFT`, `tvBX_RIGHT`,
    /// `tvBX_TOP` and `tvBX_BOTTOM` bits (0x1E when all four are drawn).
    Box { sides: u16 },
}

impl Desc {
    /// The number of slots the class defines.
    fn slot_count(self, v5: bool) -> usize {
        match self {
            Desc::Fence { .. }
            | Desc::Bar { .. }
            | Desc::Accent { .. }
            | Desc::Strike(_)
            | Desc::Box { .. } => 1,
            Desc::Arrow { .. } => {
                if v5 {
                    2
                } else {
                    1
                }
            },
            Desc::Root
            | Desc::Fraction(_)
            | Desc::HBrace { .. }
            | Desc::LongDivision
            | Desc::Script { .. }
            | Desc::Dirac { .. } => 2,
            // INTOP and SUMOP keep their operator in a fourth slot.
            Desc::BigOp { op: None, .. } => 4,
            Desc::BigOp { .. } | Desc::Limit => 3,
        }
    }
}

impl Decoder<'_> {
    /// Which slots of a template are one step smaller than the template by
    /// their place: scripts, limits, labels, root indices and the slots of
    /// small fractions. The others are its own size. This is what MathType
    /// and Equation Editor write in all but a few hundred of the 300,000
    /// slots of the corpus.
    #[inline]
    pub(super) fn slot_sizes(&self, selector: u8, variation: u16) -> SlotSizes {
        let small = match (self.v5, selector) {
            // Scripts and arrows.
            (true, 14 | 27..=29) | (false, 15 | 18..=20 | 44) => true,
            (true, 11) => variation & 0x0001 != 0,
            (false, 14) => variation == 1,
            (false, 41) => variation == 2,
            (true, 21 | 22) | (false, 42 | 43) => return SlotSizes::Limits,
            // Big operators, limits, roots and horizontal braces.
            (true, 10 | 15..=20 | 23..=25) | (false, 13 | 21..=28 | 29..=39) => {
                return SlotSizes::FirstSame;
            },
            _ => false,
        };
        if small {
            SlotSizes::Smaller
        } else {
            SlotSizes::Same
        }
    }

    /// The size of a sign in the operator slot, as that of text there.
    fn slot_sign_size(&self, size: FontSize) -> Option<FontSize> {
        let FontSize::Points(points) = size else {
            return None;
        };
        let (text, normal) = match self.slot_size_level() {
            0 => (1000, 1500),
            1 => (700, 1000),
            _ => (500, 1000),
        };
        Some(FontSize::Points(Rational::new(
            points.num.checked_mul(text)?,
            points.den.checked_mul(normal)?,
        )))
    }

    /// The size of a mark in a limit slot, as that of text at the template.
    #[cold]
    fn limit_mark_size(&self, size: FontSize) -> Option<FontSize> {
        let (text, script) = match self.slot_size_level() {
            0 => (1000, 700),
            1 => (700, 500),
            _ => (500, 500),
        };
        let points = match size {
            FontSize::Points(points) => points,
            FontSize::Logical { .. } => {
                let full = self.full_size().unwrap_or(Rational::new(12, 1)).as_f64();
                Rational::new((size.scale(full) * full * 32.0).round() as i32, 32)
            },
        };
        Some(FontSize::Points(Rational::new(
            points.num.checked_mul(text)?,
            points.den.checked_mul(script)?,
        )))
    }
}

/// Fence variation of the MTEF 3 bracket templates: 0 both, 1 left, 2 right.
fn v3_fence(variation: u16, open: &'static str, close: &'static str) -> Option<Desc> {
    match variation {
        0 => Some(Desc::Fence { open, close }),
        1 => Some(Desc::Fence { open, close: "" }),
        2 => Some(Desc::Fence { open: "", close }),
        _ => None,
    }
}

fn big_op(op: char, limits: LimitStyle) -> Desc {
    Desc::BigOp {
        op: Some(op),
        limits,
    }
}

/// MTEF 3 selectors and variations (MTEFv3.md "Template selectors and
/// variations").
pub(super) fn describe_v3(selector: u8, variation: u16) -> Option<Desc> {
    use LimitStyle::{AboveBelow, SubSup};
    let var = variation;
    let desc = match selector {
        0 => return v3_fence(var, "\u{27E8}", "\u{27E9}"),
        1 => return v3_fence(var, "(", ")"),
        2 => return v3_fence(var, "{", "}"),
        3 => return v3_fence(var, "[", "]"),
        4 => return v3_fence(var, "|", "|"),
        5 => return v3_fence(var, "\u{2016}", "\u{2016}"),
        6 => return v3_fence(var, "\u{230A}", "\u{230B}"),
        7 => return v3_fence(var, "\u{2308}", "\u{2309}"),
        8 if var == 0 => Desc::Fence {
            open: "[",
            close: "[",
        },
        9 if var == 0 => Desc::Fence {
            open: "]",
            close: "]",
        },
        10 if var == 0 => Desc::Fence {
            open: "]",
            close: "[",
        },
        11 if var == 0 => Desc::Fence {
            open: "[",
            close: ")",
        },
        12 if var == 0 => Desc::Fence {
            open: "(",
            close: "]",
        },
        13 if var <= 1 => Desc::Root,
        14 if var == 0 => Desc::Fraction(FractionKind::Normal),
        14 if var == 1 => Desc::Fraction(FractionKind::Inline),
        15 if var <= 2 => Desc::Script { pre: false },
        16 | 17 if var <= 1 => Desc::Bar {
            under: selector == 16,
            double: var == 1,
        },
        18..=20 if var <= 1 => Desc::Arrow {
            arrow: ['\u{2190}', '\u{2192}', '\u{2194}'][usize::from(selector - 18)],
            v3_label_on_top: Some(var == 0),
        },
        21 if var <= 2 => big_op('\u{222B}', SubSup),
        21 if var <= 4 => big_op('\u{222E}', SubSup),
        22 if var <= 3 => big_op('\u{222C}', SubSup),
        23 if var <= 3 => big_op('\u{222D}', SubSup),
        24 if var <= 1 => big_op('\u{222B}', AboveBelow),
        24 if var == 2 => big_op('\u{222E}', AboveBelow),
        25 if var <= 1 => big_op('\u{222C}', AboveBelow),
        26 if var <= 1 => big_op('\u{222D}', AboveBelow),
        27 | 28 if var == 0 => Desc::HBrace {
            top: selector == 27,
            glyph: if selector == 27 {
                '\u{23DE}'
            } else {
                '\u{23DF}'
            },
        },
        29 if var <= 2 => big_op('\u{2211}', AboveBelow),
        30 if var <= 1 => big_op('\u{2211}', SubSup),
        31 if var <= 2 => big_op('\u{220F}', AboveBelow),
        32 if var <= 1 => big_op('\u{220F}', SubSup),
        33 if var <= 2 => big_op('\u{2210}', AboveBelow),
        34 if var <= 1 => big_op('\u{2210}', SubSup),
        35 if var <= 2 => big_op('\u{22C3}', AboveBelow),
        36 if var <= 1 => big_op('\u{22C3}', SubSup),
        37 if var <= 2 => big_op('\u{22C2}', AboveBelow),
        38 if var <= 1 => big_op('\u{22C2}', SubSup),
        39 if var <= 2 => Desc::Limit,
        40 if var <= 1 => Desc::LongDivision,
        41 if var <= 2 => Desc::Fraction(FractionKind::Skewed),
        42 if var <= 2 => Desc::BigOp {
            op: None,
            limits: SubSup,
        },
        43 if var <= 2 => Desc::BigOp {
            op: None,
            limits: AboveBelow,
        },
        44 if var <= 2 => Desc::Script { pre: true },
        45 if var <= 2 => Desc::Dirac {
            left: var != 2,
            right: var != 1,
        },
        46 | 47 if var <= 2 => Desc::Accent {
            accent: ['\u{2190}', '\u{2192}', '\u{2194}'][usize::from(var)],
            below: selector == 46,
            wide: false,
        },
        48 if var == 0 => Desc::Accent {
            accent: '\u{23DC}',
            below: false,
            wide: false,
        },
        _ => return None,
    };
    Some(desc)
}

/// Limit placement of an MTEF 5 BigOpBoxClass variation (`tvBO_SUM`).
fn v5_limits(variation: u16) -> LimitStyle {
    if variation & 0x0040 != 0 {
        LimitStyle::AboveBelow
    } else {
        LimitStyle::SubSup
    }
}

/// The integral sign of an MTEF 5 `tmINTEG` variation: sign count in bits
/// 0-1, contour loop in bits 2-3.
fn v5_integral(variation: u16) -> char {
    let count = variation & 0x0003;
    match (variation & 0x000C, count) {
        (0, 2) => '\u{222C}',
        (0, 3) => '\u{222D}',
        (0, _) => '\u{222B}',
        (_, 2) => '\u{222F}',
        (_, 3) => '\u{2230}',
        (0x8, _) => '\u{2232}',
        (0xC, _) => '\u{2233}',
        _ => '\u{222E}',
    }
}

/// The arrow of an MTEF 5 `tmARROW` variation (used when the template does
/// not carry its arrow character).
fn v5_arrow(variation: u16) -> char {
    let double = variation & 0x0001 != 0;
    let harpoon = variation & 0x0002 != 0;
    let left = variation & 0x0010 != 0;
    let right = variation & 0x0020 != 0;
    if harpoon {
        '\u{21CC}'
    } else if double {
        '\u{21C4}'
    } else if left && !right {
        '\u{2190}'
    } else if right && !left {
        '\u{2192}'
    } else {
        '\u{2194}'
    }
}

/// MTEF 5 selectors and variations (MTEFv5.md "Templates").
pub(super) fn describe_v5(selector: u8, variation: u16) -> Option<Desc> {
    let var = variation;
    const FENCES: [(&str, &str); 9] = [
        ("\u{27E8}", "\u{27E9}"),
        ("(", ")"),
        ("{", "}"),
        ("[", "]"),
        ("|", "|"),
        ("\u{2016}", "\u{2016}"),
        ("\u{230A}", "\u{230B}"),
        ("\u{2308}", "\u{2309}"),
        ("\u{27E6}", "\u{27E7}"),
    ];
    const INTERVAL: [&str; 4] = ["(", ")", "[", "]"];
    let desc = match selector {
        0..=8 if var <= 3 => {
            let (open, close) = FENCES[usize::from(selector)];
            Desc::Fence {
                open: if var & 0x1 != 0 { open } else { "" },
                close: if var & 0x2 != 0 { close } else { "" },
            }
        },
        // Left fence in the low nibble, right fence in the next one.
        9 if var & 0x000F <= 3 && var >> 4 <= 3 => Desc::Fence {
            open: INTERVAL[usize::from(var & 0x000F)],
            close: INTERVAL[usize::from(var >> 4)],
        },
        10 if var <= 1 => Desc::Root,
        11 if var <= 7 => Desc::Fraction(if var & 0x0006 != 0 {
            FractionKind::Skewed
        } else if var & 0x0001 != 0 {
            FractionKind::Inline
        } else {
            FractionKind::Normal
        }),
        12 | 13 if var <= 1 => Desc::Bar {
            under: selector == 12,
            double: var == 1,
        },
        14 if var <= 0x3F => Desc::Arrow {
            arrow: v5_arrow(var),
            v3_label_on_top: None,
        },
        15 => big_op(v5_integral(var), v5_limits(var)),
        16 => big_op('\u{2211}', v5_limits(var)),
        17 => big_op('\u{220F}', v5_limits(var)),
        18 => big_op('\u{2210}', v5_limits(var)),
        19 => big_op('\u{22C3}', v5_limits(var)),
        20 => big_op('\u{22C2}', v5_limits(var)),
        21 | 22 => Desc::BigOp {
            op: None,
            limits: v5_limits(var),
        },
        23 => Desc::Limit,
        24 | 25 if var <= 1 => {
            let top = var == 1;
            let glyph = match (selector, top) {
                (24, true) => '\u{23DE}',
                (24, false) => '\u{23DF}',
                (_, true) => '\u{23B4}',
                (_, false) => '\u{23B5}',
            };
            Desc::HBrace { top, glyph }
        },
        26 if var <= 1 => Desc::LongDivision,
        27..=29 if var <= 1 => Desc::Script { pre: var == 1 },
        30 if var <= 3 => Desc::Dirac {
            left: var & 0x1 != 0 || var == 0,
            right: var & 0x2 != 0 || var == 0,
        },
        31 if var <= 0x0F => {
            let left = var & 0x1 != 0;
            let right = var & 0x2 != 0;
            let below = var & 0x4 != 0;
            let harpoon = var & 0x8 != 0;
            // Harpoons under the slot have their barb pointing down.
            let accent = match (left, right, harpoon, below) {
                (true, true, ..) => '\u{2194}',
                (true, false, true, false) => '\u{21BC}',
                (true, false, true, true) => '\u{21BD}',
                (true, false, false, _) => '\u{2190}',
                (false, _, true, false) => '\u{21C0}',
                (false, _, true, true) => '\u{21C1}',
                (false, _, false, _) => '\u{2192}',
            };
            Desc::Accent {
                accent,
                below,
                wide: false,
            }
        },
        32 => Desc::Accent {
            accent: '\u{0303}',
            below: false,
            wide: true,
        },
        33 => Desc::Accent {
            accent: '\u{0302}',
            below: false,
            wide: true,
        },
        34 => Desc::Accent {
            accent: '\u{23DC}',
            below: false,
            wide: false,
        },
        35 => Desc::Accent {
            accent: '\u{20E7}',
            below: false,
            wide: false,
        },
        36 if var <= 7 => Desc::Strike(strike_stroke(var)),
        37 if var <= 0x1F => Desc::Box { sides: var & 0x1E },
        _ => return None,
    };
    Some(desc)
}

/// The accent a lone character in an upper limit slot draws.
fn limit_accent(ch: char) -> Option<char> {
    Some(match ch {
        '~' | '\u{223C}' | '\u{02DC}' => '\u{0303}',
        '^' | '\u{02C6}' => '\u{0302}',
        '\u{2192}' => '\u{20D7}',
        '.' | '\u{22C5}' | '\u{02D9}' => '\u{0307}',
        '\u{00AF}' | '_' | '\u{2212}' => '\u{0304}',
        _ => return None,
    })
}

/// `tmSTRIKE` variation bits (`tvST_HORIZ`, `tvST_UP`, `tvST_DOWN`).
fn strike_stroke(variation: u16) -> StrokeKind {
    let horizontal = variation & 0x1 != 0;
    let up = variation & 0x2 != 0;
    let down = variation & 0x4 != 0;
    match (horizontal, up, down) {
        (true, _, _) => StrokeKind::Horizontal,
        (false, true, true) => StrokeKind::Cross,
        (false, false, true) => StrokeKind::Backward,
        (false, _, false) => StrokeKind::Forward,
    }
}

impl Decoder<'_> {
    fn take(slots: &mut [Option<NodeId>], index: usize) -> Option<NodeId> {
        slots.get_mut(index).and_then(Option::take)
    }

    fn take_or_empty(&mut self, slots: &mut [Option<NodeId>], index: usize) -> Result<NodeId> {
        match Self::take(slots, index) {
            Some(id) => Ok(id),
            None => self.empty_row(),
        }
    }

    /// A slot that draws something (not null and not an empty line, also not
    /// one holding only empty structures, which would become an empty line
    /// when written again).
    fn take_filled(&mut self, slots: &mut [Option<NodeId>], index: usize) -> Option<NodeId> {
        Self::take(slots, index).filter(|&id| !self.draws_nothing(id))
    }

    fn line_mark(&mut self) -> Result<NodeId> {
        self.char_node('\u{203E}', |s| CompactNode::Symbol(SymbolKind::Ordinary, s))
    }

    fn over_under(
        &mut self,
        base: NodeId,
        over: Option<NodeId>,
        under: Option<NodeId>,
    ) -> Result<NodeId> {
        match (over, under) {
            (Some(over), Some(under)) => self.node(CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits: false,
            }),
            (Some(over), None) => self.node(CompactNode::Overscript {
                base,
                over,
                movable_limits: false,
            }),
            (None, Some(under)) => self.node(CompactNode::Underscript {
                base,
                under,
                movable_limits: false,
            }),
            (None, None) => Ok(base),
        }
    }

    fn fenced(&mut self, open: &str, close: &str, content: &[NodeId]) -> Result<NodeId> {
        let content = self.children(content)?;
        let open = self.arena.push_str(open);
        let close = self.arena.push_str(close);
        self.node(CompactNode::Fenced {
            open,
            close,
            content,
        })
    }

    /// The slot and mark of a limit template that is an accent: a tilde, caret,
    /// dot or arrow above a character, or a combining mark below its base (as
    /// the writer writes one).
    fn accent_of_limits(
        &self,
        base: NodeId,
        lower: Option<NodeId>,
        upper: Option<NodeId>,
    ) -> Option<(NodeId, char, AccentPosition)> {
        match (lower, upper) {
            (None, Some(slot)) => {
                self.single_char(base)?;
                let accent = limit_accent(self.single_char(slot)?)?;
                Some((slot, accent, AccentPosition::Above))
            },
            (Some(slot), None) => {
                let mark = self.single_char(slot)?;
                crate::unicode::symbols::is_mark_below(mark).then_some((
                    slot,
                    mark,
                    AccentPosition::Below,
                ))
            },
            _ => None,
        }
    }

    /// The character of a node that is (a row holding only) a one-character
    /// symbol, identifier or operator.
    pub(super) fn single_char(&self, id: NodeId) -> Option<char> {
        let children = self.children_of(id);
        let [only] = children.as_slice() else {
            return None;
        };
        let sid = match *self.arena.node(*only) {
            CompactNode::Symbol(_, sid) | CompactNode::Identifier(sid) => sid,
            CompactNode::Operator { name, .. } => name,
            _ => return None,
        };
        let mut chars = self.arena.str_val(sid).chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Some(ch),
            _ => None,
        }
    }

    fn scripts(&mut self, base: NodeId, slots: ScriptSlots) -> Result<NodeId> {
        match (slots.sub, slots.sup) {
            (Some(sub), Some(sup)) => self.node(CompactNode::SubSup { base, sub, sup }),
            (Some(sub), None) => self.node(CompactNode::Subscript { base, sub }),
            (None, Some(sup)) => self.node(CompactNode::Superscript { base, sup }),
            (None, None) => Ok(base),
        }
    }

    fn fixed_limit_scripts(
        &mut self,
        base: NodeId,
        lower: Option<NodeId>,
        upper: Option<NodeId>,
    ) -> Result<NodeId> {
        match (lower, upper) {
            (Some(under), Some(over)) => self.node(CompactNode::UnderOver {
                base,
                under,
                over,
                movable_limits: false,
            }),
            (Some(under), None) => self.node(CompactNode::Underscript {
                base,
                under,
                movable_limits: false,
            }),
            (None, Some(over)) => self.node(CompactNode::Overscript {
                base,
                over,
                movable_limits: false,
            }),
            (None, None) => Ok(base),
        }
    }

    /// Build the AST for a template and push it onto `items`.
    pub(super) fn build_template(
        &mut self,
        start: usize,
        selector: u8,
        variation: u16,
        mut slots: Vec<Option<NodeId>>,
        glyphs: Vec<Glyph>,
        items: &mut Vec<Item>,
    ) -> Result<()> {
        let desc = if self.v5 {
            describe_v5(selector, variation)
        } else {
            describe_v3(selector, variation)
        };
        if matches!(desc, Some(Desc::Fence { .. }))
            && glyphs
                .iter()
                .any(|sign| self.sign_size(sign, false).is_some())
        {
            self.lose_size(SizeLoss::Delimiter);
        }
        let Some(desc) = desc else {
            let version = if self.v5 { 5 } else { 3 };
            self.recover(
                start,
                format!(
                    "unknown MTEF {version} template: selector {selector}, variation {variation:#x}; \
                     its slots were kept without the template structure"
                ),
            )?;
            let ids: SmallVec<[NodeId; 8]> = slots.into_iter().flatten().collect();
            let id = self.wrap(&ids)?;
            items.push(Item::Node(id));
            return Ok(());
        };

        let count = desc.slot_count(self.v5);
        let extra: SmallVec<[NodeId; 4]> = if slots.len() > count {
            slots
                .split_off(count)
                .into_iter()
                .flatten()
                .filter(|&id| !self.is_empty_row(id))
                .collect()
        } else {
            SmallVec::new()
        };
        if !extra.is_empty() {
            self.recover(
                start,
                format!(
                    "template selector {selector} holds more than its {count} slot(s); \
                     the extra slots were kept after it"
                ),
            )?;
        }

        let item = match desc {
            Desc::Fence { open, close } => {
                let main = self.take_or_empty(&mut slots, 0)?;
                let content = self.children_of(main);
                Item::Node(self.fenced(open, close, &content)?)
            },
            Desc::Root => {
                let radicand = self.take_or_empty(&mut slots, 0)?;
                let id = match self.take_filled(&mut slots, 1) {
                    Some(index) => self.node(CompactNode::Root { index, radicand })?,
                    None => self.node(CompactNode::Sqrt(radicand))?,
                };
                Item::Node(id)
            },
            Desc::Fraction(kind) => {
                let num = self.take_or_empty(&mut slots, 0)?;
                let den = self.take_or_empty(&mut slots, 1)?;
                Item::Node(self.node(CompactNode::Fraction { kind, num, den })?)
            },
            Desc::Bar { under, double } => {
                let mut id = self.take_or_empty(&mut slots, 0)?;
                for _ in 0..if double { 2 } else { 1 } {
                    let line = self.line_mark()?;
                    id = if under {
                        self.over_under(id, None, Some(line))?
                    } else {
                        self.over_under(id, Some(line), None)?
                    };
                }
                Item::Node(id)
            },
            Desc::Arrow {
                arrow,
                v3_label_on_top,
            } => {
                // The double and harpoon variations are drawn with two arrow
                // characters, which the variation names together. Any other
                // template has one character, the arrow itself.
                let is_pair = matches!(arrow, '\u{21C4}' | '\u{21CC}');
                let arrow = match glyphs.as_slice() {
                    [] => arrow,
                    [glyph] => glyph.ch,
                    [first, second]
                        if is_pair
                            && [first, second]
                                .iter()
                                .all(|g| ('\u{2190}'..='\u{21FF}').contains(&g.ch)) =>
                    {
                        arrow
                    },
                    [first, ..] => {
                        self.recover(
                            start,
                            format!(
                                "arrow template holds {} characters; the first was used",
                                glyphs.len()
                            ),
                        )?;
                        first.ch
                    },
                };
                let base =
                    self.char_node(arrow, |s| CompactNode::Symbol(SymbolKind::Relation, s))?;
                let (top, bottom) = match v3_label_on_top {
                    Some(true) => (Self::take(&mut slots, 0), None),
                    Some(false) => (None, Self::take(&mut slots, 0)),
                    None => (Self::take(&mut slots, 0), Self::take(&mut slots, 1)),
                };
                Item::Node(self.over_under(base, top, bottom)?)
            },
            Desc::BigOp { op, limits } => {
                let body = self.take_or_empty(&mut slots, 0)?;
                // An empty limit line draws nothing.
                let sub = self.take_filled(&mut slots, 1);
                let sup = self.take_filled(&mut slots, 2);
                // INTOP / SUMOP: the operator is the fourth slot (a line) or,
                // in MTEF 3, the template's character.
                let op_slot = if op.is_none() {
                    Self::take(&mut slots, 3)
                } else {
                    None
                };
                let op = op
                    .or_else(|| {
                        op_slot
                            .and_then(|id| self.single_char(id))
                            .filter(|ch| crate::unicode::symbols::nary_op_to_latex(*ch).is_some())
                    })
                    .or_else(|| {
                        if op_slot.is_none() {
                            glyphs.first().map(|g| g.ch)
                        } else {
                            None
                        }
                    });
                let sign = match op_slot {
                    None => glyphs.first().and_then(|sign| self.sign_size(sign, true)),
                    Some(_) => None,
                };
                let op_size = op_slot
                    .and_then(|id| self.single_char(id).map(|_| id))
                    .and_then(|id| self.children_of(id).first().copied())
                    .and_then(|id| self.arena.attrs(id).cloned());
                match (op, op_slot) {
                    (Some(op), _) => {
                        // The sign is no node.
                        if let Some(slot) = op_slot {
                            for id in self.children_of(slot) {
                                if let Some(rare) = &mut self.rare_sizes {
                                    // Its slot is the last of the template.
                                    if rare.dropped_symbols.last() == Some(&id) {
                                        rare.dropped_symbols.pop();
                                    }
                                }
                                if let Some(attrs) = self.arena.attrs(id).cloned() {
                                    self.arena.remove_attrs(id, &attrs);
                                }
                            }
                        }
                        let id = self.node(CompactNode::Nary {
                            op,
                            sub: sub.unwrap_or(NodeId::NONE),
                            sup: sup.unwrap_or(NodeId::NONE),
                            body,
                            limits,
                        })?;
                        // The sign of a general big operator is in a slot
                        // at the size of a big operator's sign.
                        let slot_sign = op_size
                            .and_then(|attrs| attrs.size)
                            .and_then(|size| self.slot_sign_size(size));
                        if let Some(size) = sign.or(slot_sign) {
                            self.arena.add_attrs(id, &NodeAttrs::with_glyph_size(size));
                        }
                        Item::Node(id)
                    },
                    // Without limits the operator slot is drawn as it is.
                    (None, Some(base)) if sub.is_none() && sup.is_none() => {
                        // Parts of zero spaces draw nothing.
                        let mut ids: SmallVec<[NodeId; 8]> = SmallVec::new();
                        for part in [base, body] {
                            if !self.draws_nothing(part) {
                                ids.extend(self.children_of(part));
                            }
                        }
                        let id = self.wrap(&ids)?;
                        self.template_runs.insert(id);
                        Item::Node(id)
                    },
                    (None, Some(base)) => {
                        let slots = ScriptSlots {
                            sub,
                            sup,
                            color: None,
                        };
                        let id = if let Some(name) = self.named_with_limits(base, limits) {
                            self.scripts(name, slots)?
                        } else {
                            // Preserve the complete operator expression in the
                            // fourth slot for compound and single-symbol bases.
                            let base = self.node(CompactNode::MathAtom {
                                atom_type: crate::ast::MathAtomType::Op,
                                content: base,
                            })?;
                            if limits == LimitStyle::AboveBelow {
                                self.fixed_limit_scripts(base, sub, sup)?
                            } else {
                                self.scripts(base, slots)?
                            }
                        };
                        let id = if self.is_empty_row(body) {
                            id
                        } else {
                            self.wrap(&[id, body])?
                        };
                        self.template_runs.insert(id);
                        Item::Node(id)
                    },
                    (None, None) => {
                        self.recover(start, "big-operator template has no operator character")?;
                        let op = if limits == LimitStyle::AboveBelow {
                            '\u{2211}'
                        } else {
                            '\u{222B}'
                        };
                        Item::Node(self.node(CompactNode::Nary {
                            op,
                            sub: sub.unwrap_or(NodeId::NONE),
                            sup: sup.unwrap_or(NodeId::NONE),
                            body,
                            limits,
                        })?)
                    },
                }
            },
            Desc::Limit => {
                let base = self.take_or_empty(&mut slots, 0)?;
                // An empty limit line draws nothing.
                let lower = self.take_filled(&mut slots, 1);
                let upper = self.take_filled(&mut slots, 2);
                let id = if let Some(base) = self.named_with_limits(base, LimitStyle::Default) {
                    // `lim`, `max`, ...: limits are the operator's scripts.
                    self.scripts(
                        base,
                        ScriptSlots {
                            sub: lower,
                            sup: upper,
                            color: None,
                        },
                    )?
                } else if let Some((slot, accent, position)) =
                    self.accent_of_limits(base, lower, upper)
                {
                    // An accent mark alone beside a character, typed with the
                    // limit template.
                    let mark = self
                        .children_of(slot)
                        .first()
                        .and_then(|&id| self.arena.attrs(id)?.size)
                        .and_then(|size| self.limit_mark_size(size));
                    let id = self.node(CompactNode::Accent {
                        base,
                        accent,
                        position,
                        wide: false,
                    })?;
                    if let Some(size) = mark {
                        self.arena.add_attrs(id, &NodeAttrs::with_glyph_size(size));
                    }
                    id
                } else {
                    if lower.is_none() && upper.is_none() {
                        // One that draws nothing keeps scripts around it apart.
                        if !self.draws_nothing(base) {
                            self.template_runs.insert(base);
                        }
                    }
                    self.over_under(base, upper, lower)?
                };
                Item::Node(id)
            },
            Desc::HBrace { top, glyph } => {
                let main = self.take_or_empty(&mut slots, 0)?;
                let label = Self::take(&mut slots, 1);
                let inner = if matches!(glyph, '⎴' | '⎵') {
                    // MTEF 5 tmHBRACK stretches across the main slot.
                    self.node(CompactNode::Accent {
                        base: main,
                        accent: glyph,
                        wide: true,
                        position: if top {
                            AccentPosition::Above
                        } else {
                            AccentPosition::Below
                        },
                    })?
                } else {
                    let brace =
                        self.char_node(glyph, |s| CompactNode::Symbol(SymbolKind::Ordinary, s))?;
                    if top {
                        self.over_under(main, Some(brace), None)?
                    } else {
                        self.over_under(main, None, Some(brace))?
                    }
                };
                let id = match (top, label) {
                    (_, None) => inner,
                    (true, label) => self.over_under(inner, label, None)?,
                    (false, label) => self.over_under(inner, None, label)?,
                };
                Item::Node(id)
            },
            Desc::LongDivision => {
                let dividend = self.take_or_empty(&mut slots, 0)?;
                let quotient = self.take_filled(&mut slots, 1);
                let bracket =
                    self.char_node('\u{27CC}', |s| CompactNode::Symbol(SymbolKind::Ordinary, s))?;
                let line = self.line_mark()?;
                let barred = self.over_under(dividend, Some(line), None)?;
                let base = self.wrap(&[bracket, barred])?;
                Item::Node(self.over_under(base, quotient, None)?)
            },
            Desc::Script { pre } => {
                // An empty script line draws nothing.
                let slots = ScriptSlots {
                    sub: self.take_filled(&mut slots, 0),
                    sup: self.take_filled(&mut slots, 1),
                    color: None,
                };
                if slots.sub.is_none() && slots.sup.is_none() {
                    if !extra.is_empty() {
                        let id = self.wrap(&extra)?;
                        items.push(Item::Node(id));
                    }
                    return Ok(());
                }
                if pre {
                    Item::PreScript(slots)
                } else {
                    Item::Script(slots)
                }
            },
            Desc::Dirac { left, right } => {
                // A part is drawn when the variation asks for it or its slot
                // holds something.
                let left_slot = Self::take(&mut slots, 0);
                let right_slot = Self::take(&mut slots, 1);
                let left = left || left_slot.is_some_and(|id| !self.is_empty_row(id));
                let right = right || right_slot.is_some_and(|id| !self.is_empty_row(id));
                let (open, close) = match (left, right) {
                    (true, false) => ("\u{27E8}", "|"),
                    (false, true) => ("|", "\u{27E9}"),
                    _ => ("\u{27E8}", "\u{27E9}"),
                };
                let mut content: SmallVec<[NodeId; 8]> = SmallVec::new();
                if let Some(id) = left_slot.filter(|_| left) {
                    content.extend(self.children_of(id));
                }
                if left && right {
                    let bar = self.arena.push_str("|");
                    content.push(self.node(CompactNode::Middle(bar))?);
                }
                if let Some(id) = right_slot.filter(|_| right) {
                    content.extend(self.children_of(id));
                }
                Item::Node(self.fenced(open, close, &content)?)
            },
            Desc::Accent {
                accent,
                below,
                wide,
            } => {
                let base = self.take_or_empty(&mut slots, 0)?;
                let position = if below {
                    AccentPosition::Below
                } else {
                    AccentPosition::Above
                };
                let id = self.node(CompactNode::Accent {
                    base,
                    accent,
                    position,
                    wide,
                })?;
                if let Some(size) = glyphs.first().and_then(|mark| self.sign_size(mark, false)) {
                    self.arena.add_attrs(id, &NodeAttrs::with_glyph_size(size));
                }
                Item::Node(id)
            },
            Desc::Strike(stroke) => {
                let content = self.take_or_empty(&mut slots, 0)?;
                Item::Node(self.node(CompactNode::Cancel { stroke, content })?)
            },
            Desc::Box { sides: 0 } => {
                // No side is present (MTEFv5.md, tmBOX variations): nothing is
                // drawn around the slot.
                Item::Node(self.take_or_empty(&mut slots, 0)?)
            },
            Desc::Box { sides } => {
                let content = self.take_or_empty(&mut slots, 0)?;
                if sides == 0x0C {
                    Item::Node(self.node(CompactNode::Actuarial {
                        content,
                        text_mode: false,
                    })?)
                } else {
                    if sides != 0x1E {
                        self.warn(
                            start,
                            "box template draws only some of its sides; it was read as a full box",
                        );
                    }
                    let none = self.arena.push_str("");
                    Item::Node(self.node(CompactNode::Boxed {
                        content,
                        bg_color: none,
                        border_color: none,
                    })?)
                }
            },
        };
        items.push(item);
        if !extra.is_empty() {
            let id = self.wrap(&extra)?;
            items.push(Item::Node(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_and_v5_selectors_differ() {
        // Selector 14 is a fraction in MTEF 3 and an arrow in MTEF 5.
        assert_eq!(
            describe_v3(14, 0),
            Some(Desc::Fraction(FractionKind::Normal))
        );
        assert!(matches!(describe_v5(14, 0x24), Some(Desc::Arrow { .. })));
        // Selector 15 is a script in MTEF 3 and an integral in MTEF 5.
        assert_eq!(describe_v3(15, 1), Some(Desc::Script { pre: false }));
        assert!(matches!(
            describe_v5(15, 0x71),
            Some(Desc::BigOp {
                op: Some('\u{222B}'),
                limits: LimitStyle::AboveBelow
            })
        ));
    }

    #[test]
    fn slot_sizes_agree_with_the_templates() {
        for v5 in [false, true] {
            let config = crate::error::Config::default();
            let mut decoder = Decoder::new(&[], &config);
            decoder.v5 = v5;
            for (selector, variation) in (0..=255).flat_map(|s| (0..0x200).map(move |v| (s, v))) {
                let desc = if v5 {
                    describe_v5(selector, variation)
                } else {
                    describe_v3(selector, variation)
                };
                let expected = match desc {
                    None => continue,
                    Some(Desc::BigOp { op: None, .. }) => SlotSizes::Limits,
                    Some(Desc::Script { .. } | Desc::Arrow { .. }) => SlotSizes::Smaller,
                    Some(Desc::Fraction(_))
                        if (v5 && variation & 1 != 0)
                            || (!v5 && (selector, variation) == (14, 1))
                            || (!v5 && (selector, variation) == (41, 2)) =>
                    {
                        SlotSizes::Smaller
                    },
                    Some(Desc::BigOp { .. } | Desc::Limit | Desc::Root | Desc::HBrace { .. }) => {
                        SlotSizes::FirstSame
                    },
                    _ => SlotSizes::Same,
                };
                assert_eq!(
                    decoder.slot_sizes(selector, variation),
                    expected,
                    "v5 {v5}: {selector} {variation:#x}"
                );
            }
        }
    }

    #[test]
    fn v5_interval_nibbles() {
        assert_eq!(
            describe_v5(9, 0x30),
            Some(Desc::Fence {
                open: "(",
                close: "]"
            })
        );
        assert_eq!(
            describe_v5(9, 0x12),
            Some(Desc::Fence {
                open: "[",
                close: ")"
            })
        );
        assert_eq!(describe_v5(9, 0x44), None);
    }

    #[test]
    fn v5_integral_signs() {
        assert_eq!(v5_integral(0x02), '\u{222C}');
        assert_eq!(v5_integral(0x05), '\u{222E}');
        assert_eq!(v5_integral(0x09), '\u{2232}');
        assert_eq!(v5_integral(0x0D), '\u{2233}');
    }

    #[test]
    fn unknown_variations_are_rejected() {
        assert_eq!(describe_v3(13, 2), None);
        assert_eq!(describe_v3(49, 0), None);
        assert_eq!(describe_v5(38, 0), None);
        assert_eq!(describe_v5(0, 4), None);
    }
}
