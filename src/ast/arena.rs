use super::compact::{CompactNode, NodeId, NodeSpan, StrId};

/// Arena-based storage for compact AST nodes.
///
/// All nodes, child references, and strings are stored contiguously in vectors.
/// `NodeId`, `NodeSpan`, and `StrId` index into these vectors. This layout
/// eliminates per-node heap allocations and gives cache-friendly traversal.
///
/// An `AstArena` is short-lived (one per `convert()` call). Dead nodes left by
/// normalization are unreachable garbage and are never reclaimed — the entire
/// arena is dropped at the end of the pipeline.
#[derive(Debug, Clone)]
pub struct AstArena {
    nodes: Vec<CompactNode>,
    children: Vec<NodeId>,
    strings: Vec<u8>,
    /// Byte offsets into `strings`. String `i` spans `strings[str_starts[i]..str_starts[i+1]]`.
    /// Always has one more entry than the number of interned strings.
    str_starts: Vec<u32>,
}

impl AstArena {
    /// Create a new, empty arena with no pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            children: Vec::new(),
            strings: Vec::new(),
            str_starts: vec![0],
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
            children: Vec::with_capacity(children),
            strings: Vec::with_capacity(strings),
            str_starts,
        }
    }

    // ── Safe builder API ───────────────────────────────────────────────

    /// Push a node into the arena and return its `NodeId`.
    #[inline]
    pub fn push_node(&mut self, node: CompactNode) -> NodeId {
        let id = self.nodes.len();
        debug_assert!(id < u32::MAX as usize, "arena node overflow");
        self.nodes.push(node);
        NodeId::new(id as u32)
    }

    /// Intern a string and return its `StrId`.
    #[inline]
    pub fn push_str(&mut self, s: &str) -> StrId {
        let id = self.str_starts.len() - 1;
        debug_assert!(id < u32::MAX as usize, "arena string overflow");
        self.strings.extend_from_slice(s.as_bytes());
        self.str_starts.push(self.strings.len() as u32);
        StrId::new(id as u32)
    }

    /// Push a contiguous span of child `NodeId`s and return the `NodeSpan`.
    #[inline]
    pub fn push_children(&mut self, ids: &[NodeId]) -> NodeSpan {
        let start = self.children.len();
        debug_assert!(start < u32::MAX as usize, "arena children overflow");
        debug_assert!(ids.len() <= u16::MAX as usize, "too many children");
        self.children.extend_from_slice(ids);
        NodeSpan::new(start as u32, ids.len() as u16)
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
        // Safety: we only store valid UTF-8 via push_str
        unsafe { std::str::from_utf8_unchecked(&self.strings[start..end]) }
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
            std::str::from_utf8_unchecked(self.strings.get_unchecked(start..end))
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
    fn push_and_retrieve_children() {
        let mut arena = AstArena::new();
        let s = arena.push_str("x");
        let a = arena.push_node(CompactNode::Identifier(s));
        let s = arena.push_str("y");
        let b = arena.push_node(CompactNode::Identifier(s));
        let s = arena.push_str("z");
        let c = arena.push_node(CompactNode::Identifier(s));
        let span = arena.push_children(&[a, b, c]);
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
        let span = arena.push_children(&[c]);

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
