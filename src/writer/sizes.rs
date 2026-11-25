//! Font sizes as ordinary nodes: the sizes a format has no attribute for
//! become the size and style declarations it does have, before its writer
//! sees the tree.

use smallvec::SmallVec;

use super::joining::Joiner;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::ast::style::MathStyle;
use crate::ast::{FontSize, MathAtomType, SymbolKind, TypeSize};

/// The index of `\normalsize` in `SIZE_SCALE_PER_MILLE`.
const NORMAL: u8 = 4;

/// A declaration that stands for a font size where it is used.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Declaration {
    /// The size and style in force already.
    None,
    Size(u8),
    Style(u8),
}

/// What is in force where a node is written.
#[derive(Clone, Copy)]
struct Context {
    style: MathStyle,
    /// The size declaration in force.
    size: u8,
}

struct Lowering<'a> {
    arena: &'a mut AstArena,
    base_pt: f64,
    approximated: usize,
    dropped: usize,
    /// Whether sizes are declared at all.
    declares: bool,
}

/// `expr` with its font sizes as size and style declarations.
pub(super) struct Lowered {
    pub(super) expr: CompactExpr,
    /// How many sizes are declared as the nearest size command.
    pub(super) approximated: usize,
    /// How many sizes no declaration draws.
    pub(super) dropped: usize,
}

/// Lower the font sizes of `expr`.
#[cold]
pub(super) fn lower_sizes(expr: &CompactExpr) -> crate::error::Result<Lowered> {
    lower(expr, true)
}

/// `expr` without its typesetting attributes, for a writer that has no place
/// for them.
#[cold]
pub(super) fn without_sizes(expr: &CompactExpr) -> crate::error::Result<CompactExpr> {
    Ok(lower(expr, false)?.expr)
}

fn lower(expr: &CompactExpr, declares: bool) -> crate::error::Result<Lowered> {
    let mut lowered = expr.clone();
    let mut lowering = Lowering {
        base_pt: crate::ast::attrs::base_size_pt(expr.meta.as_deref()),
        arena: &mut lowered.arena,
        approximated: 0,
        dropped: 0,
        declares,
    };
    let context = Context {
        style: expr.display.into(),
        size: NORMAL,
    };
    let roots = lowering.list(expr.roots, context);
    let (approximated, dropped) = (lowering.approximated, lowering.dropped);
    lowered.roots = roots;
    lowered.arena.clear_attrs();
    lowered.arena.check_limits()?;
    Ok(Lowered {
        expr: lowered,
        approximated,
        dropped,
    })
}

/// The style marker of a style a declaration can name.
fn marker(style: MathStyle) -> u8 {
    match style.size_class() {
        0 if style.is_display() => 0,
        0 => 1,
        1 => 2,
        _ => 3,
    }
}

impl Lowering<'_> {
    /// A style changes how a structure is drawn, not only its size: the limits
    /// of a sum, the size of its sign. Only a character takes one.
    fn declaration(&mut self, id: NodeId, size: FontSize, context: Context) -> Declaration {
        if !self.declares {
            return Declaration::None;
        }
        let declaration = self.any_declaration(size, context);
        if matches!(declaration, Declaration::Style(_)) && !self.is_character(id) {
            self.dropped += 1;
            return Declaration::None;
        }
        declaration
    }

    fn is_character(&self, id: NodeId) -> bool {
        match *self.arena.node(id) {
            CompactNode::Number(_) | CompactNode::Identifier(_) | CompactNode::Text(..) => true,
            CompactNode::Symbol(kind, _) => kind != SymbolKind::LargeOp,
            CompactNode::Operator { .. } => true,
            CompactNode::Styled(_, span) => self
                .arena
                .children(span)
                .iter()
                .all(|&id| self.is_character(id)),
            CompactNode::Color { content, .. } => self.is_character(content),
            // Marks over a character.
            CompactNode::Accent { base, .. } => self.is_character(base),
            _ => false,
        }
    }

    fn any_declaration(&mut self, size: FontSize, context: Context) -> Declaration {
        if let FontSize::Logical { level, delta_pt } = size
            && delta_pt.num == 0
        {
            let class = match level {
                TypeSize::Full => Some(0),
                TypeSize::Sub => Some(1),
                TypeSize::Sub2 => Some(2),
                _ => None,
            };
            if let Some(class) = class {
                return self.style(class, context);
            }
        }
        let scale = size.scale(self.base_pt);
        if context.style.size_class() > 0 {
            // A size command in a script is absolute in KaTeX, relative here
            // and no command in TeX. Only a style means the same in all.
            return match [1.0, 0.7, 0.5]
                .iter()
                .position(|style| (scale / style - 1.0_f64).abs() < 0.04)
            {
                Some(class) => self.style(class, context),
                None => {
                    self.dropped += 1;
                    Declaration::None
                },
            };
        }
        let (index, exact) = crate::ast::nearest_size_scale(scale).unwrap_or((NORMAL, false));
        let drawn = f64::from(crate::ast::SIZE_SCALE_PER_MILLE[usize::from(index)]) / 1000.0;
        // Far from every command, or nearest to none.
        if (drawn / scale - 1.0).abs() > 0.1
            || (index == NORMAL && !exact && context.size == NORMAL)
        {
            self.dropped += 1;
            return Declaration::None;
        }
        if !exact {
            self.approximated += 1;
        }
        if index == context.size {
            Declaration::None
        } else {
            Declaration::Size(index)
        }
    }

    fn style(&mut self, class: usize, context: Context) -> Declaration {
        if class == context.style.size_class() && context.size == NORMAL {
            Declaration::None
        } else if context.size != NORMAL && context.style.size_class() > 0 {
            // The size cannot be reset in a script.
            self.dropped += 1;
            Declaration::None
        } else {
            Declaration::Style(class as u8 + 1)
        }
    }

    fn enter(declaration: Declaration, context: Context) -> Context {
        match declaration {
            Declaration::None => context,
            Declaration::Size(size) => Context { size, ..context },
            Declaration::Style(style) => Context {
                style: MathStyle::from_switch(style),
                size: NORMAL,
            },
        }
    }

    fn push(&mut self, out: &mut SmallVec<[NodeId; 8]>, node: CompactNode) {
        let id = self.arena.push_node(node);
        out.push(id);
    }

    /// Declarations that bring `from` to `to`.
    fn switch(&mut self, out: &mut SmallVec<[NodeId; 8]>, from: Context, to: Context) {
        // KaTeX's size commands end a script style.
        if from.size != to.size {
            self.push(out, CompactNode::SizeScale(to.size));
        }
        if marker(from.style) != marker(to.style) {
            self.push(out, CompactNode::StyleSwitch(marker(to.style)));
        }
    }

    /// Lower a list, declaring each run's size before it and restoring the
    /// one around it after.
    fn list(&mut self, span: NodeSpan, context: Context) -> NodeSpan {
        let children: SmallVec<[NodeId; 8]> = self.arena.children(span).into();
        let mut out: SmallVec<[NodeId; 8]> = SmallVec::new();
        // The declarations of the list itself.
        let mut ambient = context;
        let mut current = context;
        // What differs only in sizes that are not declared is one node, as it
        // would be without them.
        let mut joiner = Joiner::new();
        for id in children {
            match *self.arena.node(id) {
                CompactNode::StyleSwitch(value) => {
                    self.switch(&mut out, current, ambient);
                    ambient.style = MathStyle::from_switch(value);
                    current = ambient;
                    out.push(id);
                    continue;
                },
                CompactNode::SizeScale(value) => {
                    self.switch(&mut out, current, ambient);
                    ambient.size = value;
                    current = ambient;
                    out.push(id);
                    continue;
                },
                // It has no size, and a declaration lasts across it.
                CompactNode::Newline => {
                    out.push(id);
                    continue;
                },
                // TeX ends a declaration at a middle delimiter, KaTeX does
                // not: end it there and declare it again.
                CompactNode::Middle(_) => {
                    self.switch(&mut out, current, context);
                    current = context;
                    out.push(id);
                    continue;
                },
                _ => {},
            }
            let wanted = match self.arena.attrs(id).and_then(|attrs| attrs.size) {
                Some(size) => Self::enter(self.declaration(id, size, ambient), ambient),
                None => ambient,
            };
            self.switch(&mut out, current, wanted);
            current = wanted;
            let lowered = self.node(id, wanted);
            let key = self.arena.attrs(id).cloned();
            joiner.add(self.arena, &mut out, lowered, key);
        }
        joiner.finish(self.arena, &mut out);
        self.arena.push_children_deferred(&out)
    }

    /// Lower a node in a position that holds one node: its declaration goes
    /// in a group with it.
    fn single(&mut self, id: NodeId, context: Context) -> NodeId {
        self.grouped(id, context, false)
    }

    /// Like `single`, for the base of a script or limit, whose class the
    /// group must keep.
    fn base(&mut self, id: NodeId, context: Context) -> NodeId {
        self.grouped(id, context, true)
    }

    fn grouped(&mut self, id: NodeId, context: Context, base: bool) -> NodeId {
        if !id.is_some() {
            return id;
        }
        let Some(size) = self.arena.attrs(id).and_then(|attrs| attrs.size) else {
            return self.node(id, context);
        };
        // A named or big operator in a group loses its limits.
        if base
            && matches!(
                *self.arena.node(id),
                CompactNode::Operator { .. }
                    | CompactNode::StructuredOperator { .. }
                    | CompactNode::Symbol(SymbolKind::LargeOp, _)
                    | CompactNode::MathAtom {
                        atom_type: MathAtomType::Op,
                        ..
                    }
            )
        {
            self.dropped += 1;
            return self.node(id, context);
        }
        let wanted = Self::enter(self.declaration(id, size, context), context);
        let mut group: SmallVec<[NodeId; 8]> = SmallVec::new();
        self.switch(&mut group, context, wanted);
        let content = self.node(id, wanted);
        if group.is_empty() {
            return content;
        }
        group.push(content);
        let class = match *self.arena.node(content) {
            CompactNode::Symbol(kind, _) => match kind {
                SymbolKind::LargeOp => MathAtomType::Op,
                SymbolKind::BinaryOp => MathAtomType::Bin,
                SymbolKind::Relation => MathAtomType::Rel,
                SymbolKind::Open => MathAtomType::Open,
                SymbolKind::Close => MathAtomType::Close,
                SymbolKind::Punctuation => MathAtomType::Punct,
                _ => MathAtomType::Ord,
            },
            CompactNode::Operator { .. } | CompactNode::StructuredOperator { .. } => {
                MathAtomType::Op
            },
            _ => MathAtomType::Ord,
        };
        let span = self.arena.push_children_deferred(&group);
        let content = self.arena.push_node(CompactNode::Row(span));
        if base {
            self.arena.push_node(CompactNode::MathAtom {
                atom_type: class,
                content,
            })
        } else {
            content
        }
    }

    /// Lower what is inside a node.
    fn node(&mut self, id: NodeId, context: Context) -> NodeId {
        let at = |style: MathStyle| Context { style, ..context };
        let sup = at(context.style.sup());
        let sub = at(context.style.sub());
        if self
            .arena
            .attrs(id)
            .is_some_and(|attrs| attrs.glyph_size.is_some())
        {
            self.dropped += 1;
        }
        let mut node = *self.arena.node(id);
        match &mut node {
            CompactNode::Row(span)
            | CompactNode::Styled(_, span)
            | CompactNode::Fenced { content: span, .. }
            | CompactNode::UndefinedCommand {
                arguments: span, ..
            } => *span = self.list(*span, context),
            CompactNode::Array { cells, .. } => {
                let ids: SmallVec<[NodeId; 8]> = self.arena.children(*cells).into();
                let cell = at(if context.style.is_display() {
                    MathStyle::Text
                } else {
                    context.style
                });
                let ids: SmallVec<[NodeId; 8]> =
                    ids.into_iter().map(|id| self.single(id, cell)).collect();
                *cells = self.arena.push_children_deferred(&ids);
            },
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                *content = self.single(*content, context);
            },
            CompactNode::Subscript { base, sub: below } => {
                *base = self.base(*base, context);
                *below = self.single(*below, sub);
            },
            CompactNode::Superscript { base, sup: above } => {
                *base = self.base(*base, context);
                *above = self.single(*above, sup);
            },
            CompactNode::SubSup {
                base,
                sub: below,
                sup: above,
            }
            | CompactNode::PreScripts {
                base,
                pre_sub: below,
                pre_sup: above,
            }
            | CompactNode::UnderOver {
                base,
                under: below,
                over: above,
                ..
            } => {
                *base = self.base(*base, context);
                *below = self.single(*below, sub);
                *above = self.single(*above, sup);
            },
            CompactNode::Nary {
                sub: below,
                sup: above,
                body,
                ..
            } => {
                *below = self.single(*below, sub);
                *above = self.single(*above, sup);
                *body = self.single(*body, context);
            },
            CompactNode::Underscript { base, under, .. } => {
                *base = self.base(*base, context);
                *under = self.single(*under, sub);
            },
            CompactNode::Overscript { base, over, .. } => {
                *base = self.base(*base, context);
                *over = self.single(*over, sup);
            },
            CompactNode::Fraction { kind, num, den } => {
                let style = kind.forced_style().unwrap_or(context.style);
                // A skewed fraction is written as an ordinary one.
                let (above, below) = (style.fracnum(), style.fracden());
                *num = self.single(*num, at(above));
                *den = self.single(*den, at(below));
            },
            CompactNode::Root { radicand, index } => {
                *radicand = self.single(*radicand, context);
                *index = self.single(*index, at(MathStyle::ScriptScript));
            },
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => {
                for branch in [display, text, script, script_script] {
                    *branch = self.single(*branch, context);
                }
            },
            CompactNode::Sqrt(content)
            | CompactNode::Phantom(content, _)
            | CompactNode::Accent { base: content, .. }
            | CompactNode::Boxed { content, .. }
            | CompactNode::Actuarial { content, .. }
            | CompactNode::Reflect { content, .. }
            | CompactNode::Cancel { content, .. }
            | CompactNode::Scaled { content, .. }
            | CompactNode::RaiseBox { content, .. }
            | CompactNode::Color { content, .. }
            | CompactNode::MathAtom { content, .. }
            | CompactNode::StructuredOperator { content, .. } => {
                *content = self.single(*content, context);
            },
            _ => return id,
        }
        let lowered = self.arena.push_node(node);
        self.arena.copy_provenance(id, lowered);
        lowered
    }
}
