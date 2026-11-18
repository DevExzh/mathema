use std::collections::HashMap;

use super::SpaceBasis;
use super::compact::{CompactNode, NodeId, NodeSpan, StrId};

/// Arena-based storage for compact AST nodes.
///
/// Nodes, child references, and UTF-8 text each occupy a contiguous buffer.
/// `NodeId` selects a node, `NodeSpan` selects a range of child references,
/// and `StrId` locates a string through its byte offsets. This layout avoids
/// per-node heap allocations.
///
/// An `AstArena` is short-lived (one per `convert()` call). Dead nodes left by
/// normalization are unreachable garbage and are never reclaimed — the entire
/// arena is dropped at the end of the pipeline.
#[derive(Debug, Clone)]
pub struct AstArena {
    nodes: Vec<CompactNode>,
    /// Sparse provenance for explicitly typed spacing nodes.
    space_bases: HashMap<NodeId, SpaceBasis>,
    children: Vec<NodeId>,
    strings: String,
    /// Byte offsets into `strings`. String `i` spans `strings[str_starts[i]..str_starts[i+1]]`.
    /// Always has one more entry than the number of interned strings.
    str_starts: Vec<u32>,
    /// Set when [`push_children_deferred`](Self::push_children_deferred)
    /// refused a list; reported by [`check_limits`](Self::check_limits).
    list_overflow: bool,
    /// Set when [`push_node`](Self::push_node) or [`push_str`](Self::push_str)
    /// refused to grow past the 32-bit index space; reported by
    /// [`check_limits`](Self::check_limits).
    size_overflow: bool,
    /// [`MAX_ENTRIES`](Self::MAX_ENTRIES), lowered by tests.
    entry_limit: usize,
}

impl AstArena {
    /// Create a new, empty arena with no pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            space_bases: HashMap::new(),
            children: Vec::new(),
            strings: String::new(),
            str_starts: vec![0],
            list_overflow: false,
            size_overflow: false,
            entry_limit: Self::MAX_ENTRIES,
        }
    }

    /// Create an empty arena with pre-allocated capacity for `nodes` nodes,
    /// `children` child references, and `strings` bytes of interned string data.
    #[must_use]
    pub fn with_capacity(nodes: usize, children: usize, strings: usize) -> Self {
        let mut str_starts = Vec::with_capacity(nodes.min(strings) + 1);
        str_starts.push(0);
        Self {
            nodes: Vec::with_capacity(nodes),
            space_bases: HashMap::new(),
            children: Vec::with_capacity(children),
            strings: String::with_capacity(strings),
            str_starts,
            list_overflow: false,
            size_overflow: false,
            entry_limit: Self::MAX_ENTRIES,
        }
    }

    // ── Safe builder API ───────────────────────────────────────────────

    /// The most nodes, child references or string bytes an arena can hold:
    /// they are indexed with 32 bits.
    pub const MAX_ENTRIES: usize = u32::MAX as usize - 1;

    /// Push a node into the arena and return its `NodeId`.
    ///
    /// An arena that already holds [`MAX_ENTRIES`](Self::MAX_ENTRIES) nodes
    /// stores nothing, returns the id of its last node and remembers the
    /// failure, which [`check_limits`](Self::check_limits) reports.
    #[inline]
    pub fn push_node(&mut self, node: CompactNode) -> NodeId {
        let id = self.nodes.len();
        if id >= self.entry_limit {
            self.size_overflow = true;
            return NodeId::new((id - 1) as u32);
        }
        self.nodes.push(node);
        NodeId::new(id as u32)
    }

    /// Push a spacing node while retaining its source unit basis.
    #[inline]
    pub fn push_space(&mut self, width: crate::ast::Rational, basis: SpaceBasis) -> NodeId {
        let id = self.push_node(CompactNode::Space(width));
        if basis != SpaceBasis::Current {
            self.space_bases.insert(id, basis);
        }
        id
    }

    /// Intern a string and return its `StrId`.
    ///
    /// When the string data would grow past
    /// [`MAX_ENTRIES`](Self::MAX_ENTRIES) bytes, an empty string is interned
    /// instead and the failure is remembered for
    /// [`check_limits`](Self::check_limits).
    #[inline]
    pub fn push_str(&mut self, s: &str) -> StrId {
        let id = self.str_starts.len() - 1;
        if self.strings.len() + s.len() > self.entry_limit || id >= self.entry_limit {
            self.size_overflow = true;
        } else {
            self.strings.push_str(s);
        }
        self.str_starts.push(self.strings.len() as u32);
        StrId::new(id.min(self.entry_limit) as u32)
    }

    /// The most children one node can hold (a span length is 16 bits).
    pub const MAX_CHILDREN: usize = u16::MAX as usize;

    /// Push a contiguous span of child `NodeId`s and return the `NodeSpan`.
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`](crate::Error::LimitExceeded) with
    /// [`LimitKind::ListLength`](crate::LimitKind::ListLength) when `ids` is
    /// longer than [`MAX_CHILDREN`](Self::MAX_CHILDREN): a span stores its
    /// length in 16 bits, so a longer list cannot be represented.
    #[inline]
    pub fn push_children(&mut self, ids: &[NodeId]) -> crate::error::Result<NodeSpan> {
        let Ok(len) = u16::try_from(ids.len()) else {
            return Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::ListLength,
                limit: Self::MAX_CHILDREN,
            });
        };
        let start = self.children.len();
        if start + ids.len() > self.entry_limit {
            return Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::ArenaSize,
                limit: self.entry_limit,
            });
        }
        self.children.extend_from_slice(ids);
        Ok(NodeSpan::new(start as u32, len))
    }

    /// Push a span like [`push_children`](Self::push_children), for builders
    /// that cannot return an error at the point of the call (parser
    /// combinators, recursive conversions). A list that is too long is not
    /// stored: the span is empty and the arena remembers the failure, which
    /// [`check_limits`](Self::check_limits) reports. Every builder that uses
    /// this must call `check_limits` before handing the arena out.
    pub fn push_children_deferred(&mut self, ids: &[NodeId]) -> NodeSpan {
        match self.push_children(ids) {
            Ok(span) => span,
            Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::ArenaSize,
                ..
            }) => {
                self.size_overflow = true;
                NodeSpan::EMPTY
            },
            Err(_) => {
                self.list_overflow = true;
                NodeSpan::EMPTY
            },
        }
    }

    /// Report a list refused by
    /// [`push_children_deferred`](Self::push_children_deferred), or a node or
    /// string refused by [`push_node`](Self::push_node) or
    /// [`push_str`](Self::push_str). Every builder must call this before
    /// handing the arena out.
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`](crate::Error::LimitExceeded) with
    /// [`LimitKind::ArenaSize`](crate::LimitKind::ArenaSize) or
    /// [`LimitKind::ListLength`](crate::LimitKind::ListLength).
    pub fn check_limits(&self) -> crate::error::Result<()> {
        if self.size_overflow {
            return Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::ArenaSize,
                limit: self.entry_limit,
            });
        }
        if self.list_overflow {
            return Err(crate::error::Error::LimitExceeded {
                kind: crate::error::LimitKind::ListLength,
                limit: Self::MAX_CHILDREN,
            });
        }
        Ok(())
    }

    // ── Safe accessors (bounds-checked) ────────────────────────────────

    /// Return a shared reference to the node identified by `id`.
    #[inline]
    pub fn node(&self, id: NodeId) -> &CompactNode {
        &self.nodes[id.index()]
    }

    /// Return a mutable reference to the node identified by `id`.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut CompactNode {
        &mut self.nodes[id.index()]
    }

    /// Replace a node by an existing node, preserving sparse spacing data
    /// when normalization unwraps a group or elides an empty script field.
    pub(crate) fn replace_from(&mut self, destination: NodeId, source: NodeId) {
        if destination == source {
            return;
        }
        self.nodes[destination.index()] = self.nodes[source.index()];
        if self.space_bases.is_empty() {
            return;
        }
        if let Some(basis) = self.space_basis(source) {
            self.space_bases.insert(destination, basis);
        } else {
            self.space_bases.remove(&destination);
        }
    }

    /// Return the explicit unit basis for a spacing node, if one was parsed.
    #[inline]
    pub fn space_basis(&self, id: NodeId) -> Option<SpaceBasis> {
        self.space_bases.get(&id).copied()
    }

    /// Return the slice of child `NodeId`s covered by `span`.
    #[inline]
    pub fn children(&self, span: NodeSpan) -> &[NodeId] {
        let start = span.start() as usize;
        let end = start + span.len() as usize;
        &self.children[start..end]
    }

    /// Return the interned string identified by `id`.
    #[inline]
    pub fn str_val(&self, id: StrId) -> &str {
        let idx = id.index();
        let start = self.str_starts[idx] as usize;
        let end = self.str_starts[idx + 1] as usize;
        &self.strings[start..end]
    }

    // ── Unsafe hot-path accessors (crate-internal; not part of the public API) ──
    // Kept `pub(crate)` rather than `pub`: precondition-carrying `unsafe fn`s must
    // not be part of a math library's semver-committed surface. `#[allow(dead_code)]`
    // because they are currently exercised only by the parity regression tests
    // below; they remain available as an internal fast path.

    /// # Safety
    /// `id.raw()` must be `< self.nodes.len()`.
    #[inline]
    #[allow(dead_code)]
    pub(crate) unsafe fn node_unchecked(&self, id: NodeId) -> &CompactNode {
        unsafe { self.nodes.get_unchecked(id.index()) }
    }

    /// # Safety
    /// The entire range `span.start()..span.start()+span.len()` must be
    /// within `self.children` bounds.
    #[inline]
    #[allow(dead_code)]
    pub(crate) unsafe fn children_unchecked(&self, span: NodeSpan) -> &[NodeId] {
        let start = span.start() as usize;
        let end = start + span.len() as usize;
        unsafe { self.children.get_unchecked(start..end) }
    }

    /// # Safety
    /// `id.raw()` must be `< self.str_starts.len() - 1`.
    #[inline]
    #[allow(dead_code)]
    pub(crate) unsafe fn str_unchecked(&self, id: StrId) -> &str {
        unsafe {
            let idx = id.index();
            let start = *self.str_starts.get_unchecked(idx) as usize;
            let end = *self.str_starts.get_unchecked(idx + 1) as usize;
            self.strings.as_str().get_unchecked(start..end)
        }
    }

    // ── Stats ──────────────────────────────────────────────────────────

    /// Number of nodes currently stored in the arena.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of child references currently stored in the arena.
    #[must_use]
    pub fn children_count(&self) -> usize {
        self.children.len()
    }

    /// Number of interned strings currently stored in the arena.
    #[must_use]
    pub fn string_count(&self) -> usize {
        self.str_starts.len() - 1
    }

    /// Total heap bytes owned by this arena (excluding the `AstArena` struct itself).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.nodes.capacity() * size_of::<CompactNode>()
            + self.space_bases.capacity() * size_of::<(NodeId, SpaceBasis)>()
            + self.children.capacity() * size_of::<NodeId>()
            + self.strings.capacity()
            + self.str_starts.capacity() * size_of::<u32>()
    }
}

impl Default for AstArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::error::{Error, LimitKind};

    fn small_arena(limit: usize) -> AstArena {
        AstArena {
            entry_limit: limit,
            ..AstArena::new()
        }
    }

    #[test]
    fn string_growth_past_the_limit_is_refused() {
        let mut arena = small_arena(8);
        let a = arena.push_str("abcde");
        let b = arena.push_str("fghij");
        assert_eq!(arena.str_val(a), "abcde");
        assert_eq!(arena.str_val(b), "");
        assert!(matches!(
            arena.check_limits(),
            Err(Error::LimitExceeded {
                kind: LimitKind::ArenaSize,
                limit: 8
            })
        ));
    }

    #[test]
    fn node_growth_past_the_limit_is_refused() {
        let mut arena = small_arena(2);
        let first = arena.push_node(CompactNode::Newline);
        let second = arena.push_node(CompactNode::Newline);
        let third = arena.push_node(CompactNode::Newline);
        assert_ne!(first, second);
        assert_eq!(third, second);
        assert_eq!(arena.node_count(), 2);
        assert!(matches!(
            arena.check_limits(),
            Err(Error::LimitExceeded {
                kind: LimitKind::ArenaSize,
                ..
            })
        ));
    }

    #[test]
    fn child_growth_past_the_limit_is_refused() {
        let mut arena = small_arena(3);
        let id = arena.push_node(CompactNode::Newline);
        assert!(arena.push_children(&[id, id]).is_ok());
        assert!(matches!(
            arena.push_children(&[id, id]),
            Err(Error::LimitExceeded {
                kind: LimitKind::ArenaSize,
                ..
            })
        ));
        assert!(arena.push_children_deferred(&[id, id]).is_empty());
        assert!(arena.check_limits().is_err());
    }

    use super::*;
    use crate::ast::FractionKind;
    use crate::ast::compact::CompactNode;

    #[test]
    fn push_and_retrieve_nodes() {
        let mut arena = AstArena::new();
        let s = arena.push_str("42");
        let id = arena.push_node(CompactNode::Number(s));
        assert_eq!(*arena.node(id), CompactNode::Number(s));
    }

    #[test]
    fn push_and_retrieve_strings() {
        let mut arena = AstArena::new();
        let s1 = arena.push_str("hello");
        let s2 = arena.push_str("world");
        let s3 = arena.push_str("");
        assert_eq!(arena.str_val(s1), "hello");
        assert_eq!(arena.str_val(s2), "world");
        assert_eq!(arena.str_val(s3), "");
    }

    #[test]
    fn unicode_string_slices_remain_valid() {
        let mut arena = AstArena::new();
        let greek = arena.push_str("αβ");
        let math = arena.push_str("𝔸");
        assert_eq!(arena.str_val(greek), "αβ");
        assert_eq!(arena.str_val(math), "𝔸");
        // The internal fast accessor shares the same UTF-8 checked backing
        // store and must return the exact slice as the public accessor.
        unsafe {
            assert_eq!(arena.str_unchecked(greek), "αβ");
            assert_eq!(arena.str_unchecked(math), "𝔸");
        }
    }

    #[test]
    fn push_and_retrieve_children() {
        let mut arena = AstArena::new();
        let s = arena.push_str("x");
        let a = arena.push_node(CompactNode::Identifier(s));
        let s = arena.push_str("y");
        let b = arena.push_node(CompactNode::Identifier(s));
        let s = arena.push_str("z");
        let c = arena.push_node(CompactNode::Identifier(s));
        let span = arena.push_children(&[a, b, c]).unwrap();
        let kids = arena.children(span);
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[0], a);
        assert_eq!(kids[1], b);
        assert_eq!(kids[2], c);
    }

    #[test]
    fn empty_children_span() {
        let arena = AstArena::new();
        let span = NodeSpan::EMPTY;
        assert_eq!(arena.children(span).len(), 0);
    }

    #[test]
    fn node_id_none_sentinel() {
        assert!(NodeId::NONE.is_none());
        assert!(!NodeId::NONE.is_some());
        let id = NodeId::new(0);
        assert!(!id.is_none());
        assert!(id.is_some());
    }

    #[test]
    fn stats() {
        let mut arena = AstArena::new();
        assert_eq!(arena.node_count(), 0);
        assert_eq!(arena.string_count(), 0);
        assert_eq!(arena.children_count(), 0);

        let s = arena.push_str("1");
        arena.push_node(CompactNode::Number(s));
        let s = arena.push_str("2");
        arena.push_node(CompactNode::Number(s));
        assert_eq!(arena.node_count(), 2);
        assert_eq!(arena.string_count(), 2);
    }

    #[test]
    fn unsafe_accessors_match_safe() {
        let mut arena = AstArena::new();
        let s = arena.push_str("test");
        let id = arena.push_node(CompactNode::Identifier(s));
        let s2 = arena.push_str("child");
        let c = arena.push_node(CompactNode::Identifier(s2));
        let span = arena.push_children(&[c]).unwrap();

        unsafe {
            assert_eq!(*arena.node_unchecked(id), *arena.node(id));
            assert_eq!(arena.children_unchecked(span), arena.children(span));
            assert_eq!(arena.str_unchecked(s), arena.str_val(s));
        }
    }

    #[test]
    fn fraction_round_trip() {
        let mut arena = AstArena::new();
        let s1 = arena.push_str("1");
        let num = arena.push_node(CompactNode::Number(s1));
        let s2 = arena.push_str("2");
        let den = arena.push_node(CompactNode::Number(s2));
        let frac = arena.push_node(CompactNode::Fraction {
            kind: FractionKind::Normal,
            num,
            den,
        });
        match *arena.node(frac) {
            CompactNode::Fraction {
                kind,
                num: n,
                den: d,
            } => {
                assert_eq!(kind, FractionKind::Normal);
                assert_eq!(
                    arena.str_val(match *arena.node(n) {
                        CompactNode::Number(s) => s,
                        _ => panic!("expected Number"),
                    }),
                    "1"
                );
                assert_eq!(
                    arena.str_val(match *arena.node(d) {
                        CompactNode::Number(s) => s,
                        _ => panic!("expected Number"),
                    }),
                    "2"
                );
            },
            _ => panic!("expected Fraction"),
        }
    }

    #[test]
    fn with_capacity_works() {
        let arena = AstArena::with_capacity(100, 50, 200);
        assert_eq!(arena.node_count(), 0);
        assert!(arena.heap_bytes() > 0);
    }
}
