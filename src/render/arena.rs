// src/render/arena.rs

use crate::ast::TextStyle;
use std::num::NonZeroU32;

use super::fonts_registry::FontId;

// ── Index types ────────────────────────────────────────────────────────

/// Index into `LayoutArena::boxes`. Uses NonZeroU32 so `Option<LayoutBoxId>` is 4 bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayoutBoxId(NonZeroU32);

impl LayoutBoxId {
    /// Create from a 0-based index. Panics if index >= u32::MAX - 1.
    #[inline]
    fn from_index(index: usize) -> Self {
        assert!(index < u32::MAX as usize, "layout arena overflow");
        Self(NonZeroU32::new((index as u32) + 1).expect("layout arena overflow"))
    }

    /// Convert back to 0-based index.
    #[inline]
    pub(crate) fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// Span of child entries in `LayoutArena::children`.
#[derive(Copy, Clone, Debug, Default)]
pub struct LayoutSpan {
    pub start: u32,
    pub len: u32,
}

impl LayoutSpan {
    pub const EMPTY: Self = Self { start: 0, len: 0 };

    #[inline]
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

// ── Arena box types ───────────────────────────────────────────────────

/// Arena-allocated layout box. No heap pointers except `SvgPath` and `Color`.
#[derive(Clone, Debug)]
pub struct ArenaLayoutBox {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub kind: ArenaBoxKind,
}

impl ArenaLayoutBox {
    /// Zero-width invisible kern box.
    pub fn kern(width: f64) -> Self {
        Self {
            width,
            height: 0.0,
            depth: 0.0,
            kind: ArenaBoxKind::Kern,
        }
    }

    /// Horizontal rule (fraction bar).
    pub fn rule(width: f64, thickness: f64) -> Self {
        Self {
            width,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            kind: ArenaBoxKind::Rule { thickness },
        }
    }
}

/// BoxKind variants using arena indices instead of Vec/Box.
#[derive(Clone, Debug)]
pub enum ArenaBoxKind {
    /// A single glyph character drawn from the embedded face `font`.
    Glyph {
        ch: char,
        size: f64,
        italic: bool,
        font: FontId,
    },
    /// Horizontal box: children laid out left to right.
    HBox { children: LayoutSpan },
    /// Vertical box: children stacked top to bottom.
    VBox { children: LayoutSpan },
    /// A horizontal rule (fraction bar, vinculum).
    Rule { thickness: f64 },
    /// Invisible spacing.
    Kern,
    /// A stretchy delimiter.
    Delimiter { ch: char, target_height: f64 },
    /// SVG path data (radical signs, cancel lines).
    SvgPath(String),
    /// Color wrapper.
    Color { color: String, child: LayoutBoxId },
    /// Invisible placeholder (takes space but doesn't render).
    Phantom,
    /// Box with border.
    Boxed {
        child: LayoutBoxId,
        padding: f64,
        bg_color: Option<String>,
        border_color: Option<String>,
    },
    /// A run of text characters rendered as a single `<text>` element.
    TextRun {
        text: String,
        size: f64,
        italic: bool,
        text_style: Option<TextStyle>,
    },
}

/// Positioned child reference stored in `LayoutArena::children`.
#[derive(Copy, Clone, Debug)]
pub struct ArenaPositionedBox {
    pub dx: f64,
    pub dy: f64,
    pub id: LayoutBoxId,
}

// ── Arena ─────────────────────────────────────────────────────────────

/// Flat arena storage for layout boxes. O(1) reset via `clear()`.
pub struct LayoutArena {
    boxes: Vec<ArenaLayoutBox>,
    children: Vec<ArenaPositionedBox>,
}

impl LayoutArena {
    /// Create a new arena with default capacity.
    pub fn new() -> Self {
        Self {
            boxes: Vec::with_capacity(128),
            children: Vec::with_capacity(256),
        }
    }

    /// Allocate a layout box, return its ID.
    pub fn alloc(&mut self, b: ArenaLayoutBox) -> LayoutBoxId {
        let id = LayoutBoxId::from_index(self.boxes.len());
        self.boxes.push(b);
        id
    }

    /// Begin a child span. Returns the start index for `finish_children`.
    #[inline]
    pub fn begin_children(&self) -> u32 {
        let len = self.children.len();
        assert!(len <= u32::MAX as usize, "layout arena children overflow");
        len as u32
    }

    /// Push a positioned child into the children array.
    #[inline]
    pub fn push_child(&mut self, child: ArenaPositionedBox) {
        self.children.push(child);
    }

    /// Finalize a child span from a start index returned by `begin_children`.
    #[inline]
    pub fn finish_children(&self, start: u32) -> LayoutSpan {
        LayoutSpan {
            start,
            len: (self.children.len() as u32) - start,
        }
    }

    /// Access a box by ID.
    #[inline]
    pub fn get(&self, id: LayoutBoxId) -> &ArenaLayoutBox {
        &self.boxes[id.index()]
    }

    /// Mutably access a box by ID.
    #[inline]
    pub fn get_mut(&mut self, id: LayoutBoxId) -> &mut ArenaLayoutBox {
        &mut self.boxes[id.index()]
    }

    /// Access children of a span.
    #[inline]
    pub fn children(&self, span: LayoutSpan) -> &[ArenaPositionedBox] {
        let start = span.start as usize;
        let end = start + span.len as usize;
        &self.children[start..end]
    }

    /// Mutably access children of a span.
    #[inline]
    pub fn children_mut(&mut self, span: LayoutSpan) -> &mut [ArenaPositionedBox] {
        let start = span.start as usize;
        let end = start + span.len as usize;
        &mut self.children[start..end]
    }

    /// Recursively scale all positions and dimensions in a subtree rooted at `id`.
    /// Used by `layout_scaled_arena` to properly scale `\scalebox` content.
    pub fn scale_subtree(&mut self, id: LayoutBoxId, f: f64) {
        let bx = &mut self.boxes[id.index()];
        bx.width *= f;
        bx.height *= f;
        bx.depth *= f;
        match &mut bx.kind {
            ArenaBoxKind::HBox { children } | ArenaBoxKind::VBox { children } => {
                let span = *children;
                let child_ids: Vec<LayoutBoxId> =
                    self.children(span).iter().map(|c| c.id).collect();
                // Scale child positions
                for c in self.children_mut(span) {
                    c.dx *= f;
                    c.dy *= f;
                }
                // Recurse into children
                for child_id in child_ids {
                    self.scale_subtree(child_id, f);
                }
            },
            ArenaBoxKind::Color { child, .. } => {
                let child_id = *child;
                self.scale_subtree(child_id, f);
            },
            ArenaBoxKind::Boxed { child, padding, .. } => {
                *padding *= f;
                let child_id = *child;
                self.scale_subtree(child_id, f);
            },
            ArenaBoxKind::Glyph { size, .. } => {
                *size *= f;
            },
            ArenaBoxKind::TextRun { size, .. } => {
                *size *= f;
            },
            ArenaBoxKind::Rule { thickness } => {
                *thickness *= f;
            },
            ArenaBoxKind::Delimiter { target_height, .. } => {
                *target_height *= f;
            },
            ArenaBoxKind::Kern | ArenaBoxKind::Phantom => {},
            ArenaBoxKind::SvgPath(path) => {
                *path = super::layout::scale_svg_path_data(path, f);
            },
        }
    }

    /// Reset for reuse. O(1) — clears without deallocating backing memory.
    pub fn clear(&mut self) {
        self.boxes.clear();
        self.children.clear();
    }

    /// Number of allocated boxes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Whether the arena is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
}

impl Default for LayoutArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_get() {
        let mut arena = LayoutArena::new();
        let id = arena.alloc(ArenaLayoutBox::kern(1.5));
        let b = arena.get(id);
        assert!((b.width - 1.5).abs() < f64::EPSILON);
        assert!(matches!(b.kind, ArenaBoxKind::Kern));
    }

    #[test]
    fn children_span() {
        let mut arena = LayoutArena::new();
        let a = arena.alloc(ArenaLayoutBox::kern(1.0));
        let b = arena.alloc(ArenaLayoutBox::kern(2.0));

        let start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: 0.0,
            id: a,
        });
        arena.push_child(ArenaPositionedBox {
            dx: 1.0,
            dy: 0.0,
            id: b,
        });
        let span = arena.finish_children(start);

        assert_eq!(span.len, 2);
        let kids = arena.children(span);
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].id, a);
        assert_eq!(kids[1].id, b);
        assert!((kids[1].dx - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clear_resets() {
        let mut arena = LayoutArena::new();
        arena.alloc(ArenaLayoutBox::kern(1.0));
        arena.alloc(ArenaLayoutBox::kern(2.0));
        assert_eq!(arena.len(), 2);

        arena.clear();
        assert_eq!(arena.len(), 0);
        assert!(arena.is_empty());

        // Can reuse after clear
        let id = arena.alloc(ArenaLayoutBox::kern(3.0));
        assert_eq!(arena.len(), 1);
        assert!((arena.get(id).width - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_span() {
        let arena = LayoutArena::new();
        let span = LayoutSpan::EMPTY;
        assert!(span.is_empty());
        assert_eq!(arena.children(span).len(), 0);
    }

    #[test]
    fn multiple_spans() {
        let mut arena = LayoutArena::new();
        let a = arena.alloc(ArenaLayoutBox::kern(1.0));
        let b = arena.alloc(ArenaLayoutBox::kern(2.0));
        let c = arena.alloc(ArenaLayoutBox::kern(3.0));

        // First span: [a, b]
        let s1_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: 0.0,
            id: a,
        });
        arena.push_child(ArenaPositionedBox {
            dx: 1.0,
            dy: 0.0,
            id: b,
        });
        let span1 = arena.finish_children(s1_start);

        // Second span: [c]
        let s2_start = arena.begin_children();
        arena.push_child(ArenaPositionedBox {
            dx: 0.0,
            dy: 0.0,
            id: c,
        });
        let span2 = arena.finish_children(s2_start);

        assert_eq!(arena.children(span1).len(), 2);
        assert_eq!(arena.children(span2).len(), 1);
        assert_eq!(arena.children(span2)[0].id, c);
    }

    #[test]
    fn rule_box() {
        let mut arena = LayoutArena::new();
        let id = arena.alloc(ArenaLayoutBox::rule(5.0, 0.04));
        let b = arena.get(id);
        assert!((b.width - 5.0).abs() < f64::EPSILON);
        assert!((b.height - 0.02).abs() < f64::EPSILON);
        assert!((b.depth - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn glyph_box() {
        let mut arena = LayoutArena::new();
        let id = arena.alloc(ArenaLayoutBox {
            width: 0.5,
            height: 0.683,
            depth: 0.0,
            kind: ArenaBoxKind::Glyph {
                ch: 'x',
                size: 1.0,
                italic: false,
                font: FontId::Math,
            },
        });
        let b = arena.get(id);
        assert!(matches!(b.kind, ArenaBoxKind::Glyph { ch: 'x', .. }));
    }

    #[test]
    fn layout_box_id_option_size() {
        // NonZeroU32 enables niche optimization
        assert_eq!(
            std::mem::size_of::<Option<LayoutBoxId>>(),
            std::mem::size_of::<LayoutBoxId>()
        );
    }
}
