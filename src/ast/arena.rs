use std::collections::HashMap;

use super::attrs::NodeAttrs;
use super::compact::{CompactNode, NodeId, NodeSpan, StrId};
use super::{ColumnAlign, MatrixKind, SpaceBasis, SpanBoundary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    SpaceBasis(SpaceBasis),
    Array(MatrixKind),
    NativeArray(RuleDashes),
    MatrixPadding,
}

/// The native table rules drawn dashed, parallel to an array's `col_seps`
/// and `row_seps` masks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuleDashes {
    pub(crate) cols: u32,
    pub(crate) rows: u32,
}

type ProvenanceEntry = (NodeId, Provenance);

/// Keep the overwhelmingly common single metadata entry in the arena itself.
/// A boxed map is allocated only when an arena needs a second distinct entry.
#[derive(Debug, Clone, Default)]
#[allow(clippy::box_collection)]
enum ProvenanceStore {
    #[default]
    Empty,
    Inline(ProvenanceEntry),
    Overflow(Box<HashMap<NodeId, Provenance>>),
}

impl ProvenanceStore {
    #[inline]
    fn insert(&mut self, id: NodeId, value: Provenance) {
        match self {
            Self::Empty => *self = Self::Inline((id, value)),
            Self::Inline((key, stored)) if *key == id => *stored = value,
            Self::Inline(first) => {
                *self = Self::Overflow(Box::new(HashMap::from([*first, (id, value)])));
            },
            Self::Overflow(map) => {
                map.insert(id, value);
            },
        }
    }

    #[inline]
    fn get(&self, id: NodeId) -> Option<Provenance> {
        match self {
            Self::Empty => None,
            Self::Inline((entry_id, value)) => (*entry_id == id).then_some(*value),
            Self::Overflow(map) => map.get(&id).copied(),
        }
    }

    #[inline]
    fn remove(&mut self, id: NodeId) {
        match self {
            Self::Empty => {},
            Self::Inline((entry_id, _)) if *entry_id == id => *self = Self::Empty,
            Self::Inline(_) => {},
            Self::Overflow(map) => {
                // Overflow starts with two entries and returns to inline as
                // soon as only one remains.
                if map.remove(&id).is_some() && map.len() == 1 {
                    let (&key, &value) = map.iter().next().expect("one provenance entry");
                    *self = Self::Inline((key, value));
                }
            },
        }
    }

    #[inline]
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Overflow(map) => {
                size_of::<HashMap<NodeId, Provenance>>()
                    + map.capacity() * size_of::<ProvenanceEntry>()
            },
            _ => 0,
        }
    }
}

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
    /// Sparse provenance for explicitly typed spaces and parsed table forms.
    provenance: ProvenanceStore,
    /// Typesetting attributes of the nodes that have any.
    attrs: Option<HashMap<NodeId, NodeAttrs>>,
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
    /// See [`has_size_scale`](Self::has_size_scale).
    has_size_scale: bool,
    /// [`MAX_ENTRIES`](Self::MAX_ENTRIES), lowered by tests.
    entry_limit: usize,
}

impl AstArena {
    /// Create a new, empty arena with no pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            provenance: ProvenanceStore::default(),
            attrs: None,
            children: Vec::new(),
            strings: String::new(),
            str_starts: vec![0],
            list_overflow: false,
            size_overflow: false,
            has_size_scale: false,
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
            provenance: ProvenanceStore::default(),
            attrs: None,
            children: Vec::with_capacity(children),
            strings: String::with_capacity(strings),
            str_starts,
            list_overflow: false,
            size_overflow: false,
            has_size_scale: false,
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
        self.has_size_scale |= matches!(node, CompactNode::SizeScale(_));
        self.nodes.push(node);
        NodeId::new(id as u32)
    }

    /// Whether a size declaration was pushed. One stored through
    /// [`node_mut`](Self::node_mut) is not counted.
    pub(crate) fn has_size_scale(&self) -> bool {
        self.has_size_scale
    }

    /// Push a spacing node while retaining its source unit basis.
    #[inline]
    pub fn push_space(&mut self, width: crate::ast::Rational, basis: SpaceBasis) -> NodeId {
        let before = self.nodes.len();
        let id = self.push_node(CompactNode::Space(width));
        if self.nodes.len() != before {
            self.provenance.insert(id, Provenance::SpaceBasis(basis));
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
        self.store_string(s.len(), |pool| pool.push_str(s))
    }

    pub(crate) fn push_span_boundaries(&mut self, boundaries: &[SpanBoundary]) -> StrId {
        if boundaries.is_empty() {
            return StrId::EMPTY;
        }
        let bytes = boundaries
            .iter()
            .fold(0usize, |total, b| total.saturating_add(b.latex().len()));
        self.store_string(bytes, |pool| {
            pool.reserve(bytes);
            for boundary in boundaries {
                pool.push_str(boundary.latex());
            }
        })
    }

    #[inline]
    fn store_string(&mut self, bytes: usize, append: impl FnOnce(&mut String)) -> StrId {
        let id = self.str_starts.len() - 1;
        if id >= self.entry_limit {
            self.size_overflow = true;
            return StrId::EMPTY;
        }
        if bytes > self.entry_limit.saturating_sub(self.strings.len()) {
            self.size_overflow = true;
        } else {
            append(&mut self.strings);
        }
        self.str_starts.push(self.strings.len() as u32);
        StrId::new(id as u32)
    }

    /// The most children one node can hold (a span length is 16 bits).
    pub const MAX_CHILDREN: usize = u16::MAX as usize;

    /// Bound a rectangular cell span before allocating padding for ragged input.
    pub(crate) fn array_stride_deferred(&mut self, rows: usize, width: usize) -> Option<u16> {
        let stride = width.max(1);
        if stride > Self::MAX_CHILDREN || rows > Self::MAX_CHILDREN / stride {
            self.list_overflow = true;
            None
        } else {
            Some(stride as u16)
        }
    }

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
        if ids.len() > self.entry_limit.saturating_sub(start) {
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
    ///
    /// What is written there can be another node altogether, so the node's
    /// typesetting attributes are removed; see [`attrs`](Self::attrs) and
    /// [`add_attrs`](Self::add_attrs) to keep them.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut CompactNode {
        if let Some(store) = &mut self.attrs {
            store.remove(&id);
            if store.is_empty() {
                self.attrs = None;
            }
        }
        self.node_mut_in_place(id)
    }

    /// Like [`node_mut`](Self::node_mut) for an edit that leaves the node
    /// what it is.
    #[inline]
    pub(crate) fn node_mut_in_place(&mut self, id: NodeId) -> &mut CompactNode {
        &mut self.nodes[id.index()]
    }

    /// Every node in the arena, reachable or not.
    #[inline]
    pub(crate) fn nodes(&self) -> &[CompactNode] {
        &self.nodes
    }

    /// Replace a node by an existing node that it held or stands for,
    /// preserving its source metadata when normalization unwraps a group or
    /// elides an empty script field.
    pub(crate) fn replace_from(&mut self, destination: NodeId, source: NodeId) {
        if destination == source {
            return;
        }
        self.nodes[destination.index()] = self.nodes[source.index()];
        if self.attrs.is_some() {
            // The wrapper's attributes applied to what it held.
            let mut outer = self.attrs(destination).cloned().unwrap_or_default();
            outer.glyph_size = None;
            let inner = self.attrs(source).cloned().unwrap_or_default();
            let merged = inner.or(&outer);
            if let Some(store) = &mut self.attrs {
                store.remove(&destination);
            }
            self.add_attrs(destination, &merged);
        }
        if matches!(self.provenance, ProvenanceStore::Empty) {
            return;
        }
        if let Some(provenance) = self.provenance.get(source) {
            self.provenance.insert(destination, provenance);
        } else {
            self.provenance.remove(destination);
        }
    }

    // ── Typesetting attributes ─────────────────────────────────────────

    /// Whether any node has typesetting attributes.
    #[inline]
    #[must_use]
    pub fn has_attrs(&self) -> bool {
        self.attrs.is_some()
    }

    /// The typesetting attributes of a node, if it has any of its own.
    #[inline]
    #[must_use]
    pub fn attrs(&self, id: NodeId) -> Option<&NodeAttrs> {
        self.attrs.as_ref()?.get(&id)
    }

    /// Give a node typesetting attributes; those it already has win. The
    /// attributes of a `Row`, which is only a list, go to each of its
    /// children, so that rows can be opened and unwrapped freely.
    pub fn add_attrs(&mut self, id: NodeId, attrs: &NodeAttrs) {
        // A refused push returns the last existing id (see `push_node`).
        if attrs.is_empty() || self.size_overflow {
            return;
        }
        // A sign's size is not that of what a list or cell holds.
        let inner = NodeAttrs {
            glyph_size: None,
            ..attrs.clone()
        };
        let mut pending = vec![(id, attrs)];
        while let Some((id, attrs)) = pending.pop() {
            if attrs.is_empty() {
                continue;
            }
            match *self.node(id) {
                CompactNode::Row(span) => {
                    pending.extend(self.children(span).iter().map(|&id| (id, &inner)));
                },
                // The wrapper of a table cell, which writers look for.
                CompactNode::AlignedCell { content, .. }
                | CompactNode::SpannedCell { content, .. } => pending.push((content, &inner)),
                // These draw nothing and stand for no text.
                CompactNode::StyleSwitch(_)
                | CompactNode::SizeScale(_)
                | CompactNode::Newline
                | CompactNode::ColAlign(_)
                | CompactNode::Middle(_) => {},
                _ => {
                    let store = self.attrs.get_or_insert_default();
                    let own = store.remove(&id).unwrap_or_default();
                    store.insert(id, own.or(attrs));
                },
            }
        }
    }

    /// Move the attributes of a node that became a `Row` to its children.
    pub(crate) fn settle_attrs(&mut self, id: NodeId) {
        if let Some(store) = &mut self.attrs
            && let Some(attrs) = store.remove(&id)
        {
            self.add_attrs(id, &attrs);
        }
    }

    /// Remove the typesetting attributes of a node, if they are `attrs`.
    pub(crate) fn remove_attrs(&mut self, id: NodeId, attrs: &NodeAttrs) {
        if let Some(store) = &mut self.attrs
            && store.get(&id) == Some(attrs)
        {
            store.remove(&id);
        }
        if self.attrs.as_ref().is_some_and(|store| store.is_empty()) {
            self.attrs = None;
        }
    }

    /// Give a node made from another one its source metadata.
    pub(crate) fn copy_provenance(&mut self, from: NodeId, to: NodeId) {
        if !self.size_overflow
            && let Some(provenance) = self.provenance.get(from)
        {
            self.provenance.insert(to, provenance);
        }
    }

    /// Remove every typesetting attribute.
    pub(crate) fn clear_attrs(&mut self) {
        self.attrs = None;
    }

    /// Give the nodes inside a wrapper about to be dropped its attributes.
    pub(crate) fn move_attrs(&mut self, from: NodeId, to: NodeId) {
        if let Some(attrs) = self.attrs(from).cloned() {
            self.add_attrs(to, &attrs);
        }
    }

    /// Return the explicit unit basis for a spacing node, if one was parsed.
    #[inline]
    pub fn space_basis(&self, id: NodeId) -> Option<SpaceBasis> {
        match self.provenance.get(id)? {
            Provenance::SpaceBasis(basis) if matches!(self.node(id), CompactNode::Space(_)) => {
                Some(basis)
            },
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn mark_array_environment(&mut self, id: NodeId, environment: MatrixKind) {
        // A refused push returns the last existing id. Its metadata belongs to
        // the original node, even while propagating the arena size error.
        if !self.size_overflow {
            self.provenance.insert(id, Provenance::Array(environment));
        }
    }

    #[inline]
    pub(crate) fn array_environment(&self, id: NodeId) -> Option<MatrixKind> {
        match self.provenance.get(id)? {
            Provenance::Array(env) if matches!(self.node(id), CompactNode::Array { .. }) => {
                Some(env)
            },
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn mark_native_array(&mut self, id: NodeId, dashes: RuleDashes) {
        if !self.size_overflow {
            self.provenance.insert(id, Provenance::NativeArray(dashes));
        }
    }

    #[inline]
    pub(crate) fn is_native_array(&self, id: NodeId) -> bool {
        matches!(self.provenance.get(id), Some(Provenance::NativeArray(_)))
            && matches!(self.node(id), CompactNode::Array { .. })
    }

    /// The dashed rules of a native array, limited to the rules it draws.
    #[inline]
    pub(crate) fn rule_dashes(&self, id: NodeId) -> RuleDashes {
        match (self.provenance.get(id), self.node(id)) {
            (
                Some(Provenance::NativeArray(dashes)),
                &CompactNode::Array {
                    col_seps, row_seps, ..
                },
            ) => RuleDashes {
                cols: dashes.cols & col_seps,
                rows: dashes.rows & row_seps,
            },
            _ => RuleDashes::default(),
        }
    }

    /// Return matrix provenance only when the current compact array still has
    /// the metadata shape that a matrix environment can represent.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn valid_matrix_environment(
        &self,
        id: NodeId,
        col_aligns: NodeSpan,
        cells: NodeSpan,
        num_cols: u16,
        arraystretch_pct: u8,
        add_jot: bool,
        array_style: bool,
        col_seps: u32,
        row_seps: u32,
    ) -> Option<(MatrixKind, super::ColumnAlign)> {
        let kind = self.array_environment(id)?;
        let expected_pitch = if kind == MatrixKind::SmallMatrix {
            50
        } else {
            100
        };
        if arraystretch_pct != expected_pitch
            || add_jot
            || array_style
            || col_seps != 0
            || row_seps != 0
        {
            return None;
        }
        let columns = self.children(col_aligns);
        let cell_count = self.children(cells).len();
        if cell_count != 0 && columns.len() != num_cols as usize {
            return None;
        }
        let mut alignment = None;
        for &column in columns {
            let CompactNode::ColAlign(value) = *self.node(column) else {
                return None;
            };
            if let Some(previous) = alignment {
                if previous != value {
                    return None;
                }
            } else {
                alignment = Some(value);
            }
        }
        Some((kind, alignment.unwrap_or(super::ColumnAlign::Center)))
    }

    #[inline]
    pub(crate) fn mark_matrix_padding(&mut self, id: NodeId) {
        if !self.size_overflow {
            self.provenance.insert(id, Provenance::MatrixPadding);
        }
    }

    #[inline]
    pub(crate) fn is_matrix_padding(&self, id: NodeId) -> bool {
        matches!(self.provenance.get(id), Some(Provenance::MatrixPadding))
            && matches!(self.node(id), CompactNode::Row(span) if span.is_empty())
    }

    /// Cap an authored span to the consecutive empty physical slots currently
    /// available after the cell. This keeps public arena edits coherent when a
    /// span is enlarged without allocating new padding or crossing content.
    pub(crate) fn effective_cell_span_in_row(
        &self,
        row: &[NodeId],
        index: usize,
        declared: u16,
    ) -> usize {
        let target = usize::from(declared.max(1)).min(row.len().saturating_sub(index));
        let mut width = 1;
        while width < target {
            let id = row[index + width];
            if !matches!(*self.node(id), CompactNode::Row(span) if span.is_empty()) {
                break;
            }
            width += 1;
        }
        width
    }

    /// Resolve the cell's alignment and the math list to typeset in it.
    pub(crate) fn table_cell_parts<'a>(
        &'a self,
        cell: &'a NodeId,
    ) -> (Option<ColumnAlign>, &'a [NodeId]) {
        let (alignment, content) = match self.node(*cell) {
            CompactNode::AlignedCell { alignment, content }
            | CompactNode::SpannedCell {
                alignment, content, ..
            } => (Some(*alignment), content),
            _ => (None, cell),
        };
        let nodes = match *self.node(*content) {
            CompactNode::Row(span) => self.children(span),
            _ => std::slice::from_ref(content),
        };
        (alignment, nodes)
    }

    /// A cell directive occupies the grid only when it is the entire cell.
    pub(crate) fn table_cell_wrapper(&self, cell: &[NodeId]) -> Option<(NodeId, usize)> {
        let &[id] = cell else {
            return None;
        };
        match *self.node(id) {
            CompactNode::AlignedCell { .. } => Some((id, 1)),
            CompactNode::SpannedCell { span, .. } => Some((id, usize::from(span.max(1)))),
            _ => None,
        }
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
        if id == StrId::EMPTY {
            return "";
        }
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
        if id == StrId::EMPTY {
            return "";
        }
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
            + self.provenance.heap_bytes()
            + self.attrs.as_ref().map_or(0, |attrs| {
                size_of::<HashMap<NodeId, NodeAttrs>>()
                    + attrs.capacity() * size_of::<(NodeId, NodeAttrs)>()
            })
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
impl AstArena {
    /// An arena that holds at most `limit` entries.
    pub(crate) fn with_entry_limit(limit: usize) -> Self {
        Self {
            entry_limit: limit,
            ..Self::new()
        }
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
    fn boundary_strings_share_the_arena_limits_and_empty_sentinel() {
        let mut arena = small_arena(4);
        for _ in 0..8 {
            assert_eq!(arena.push_span_boundaries(&[]), StrId::EMPTY);
        }
        assert_eq!(arena.string_count(), 0);
        let id = arena.push_span_boundaries(&[SpanBoundary::Rule, SpanBoundary::SuppressGlue]);
        assert_eq!(arena.str_val(id), "|@{}");
        for _ in 0..8 {
            let empty = arena.push_str("a");
            assert_eq!(arena.str_val(empty), "");
        }
        assert_eq!(arena.string_count(), 4);
        assert!(matches!(
            arena.check_limits(),
            Err(Error::LimitExceeded {
                kind: LimitKind::ArenaSize,
                limit: 4
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
    fn absent_or_refused_nodes_do_not_acquire_provenance() {
        let mut arena = small_arena(1);
        assert_eq!(arena.space_basis(NodeId::NONE), None);
        let space = arena.push_space(crate::ast::Rational::new(1, 2), SpaceBasis::TextEm);
        let refused = arena.push_node(CompactNode::Newline);
        assert_eq!(refused, space);
        arena.mark_array_environment(refused, MatrixKind::Matrix);
        assert_eq!(arena.space_basis(space), Some(SpaceBasis::TextEm));
    }

    #[test]
    fn ragged_array_stride_respects_the_span_limit_before_padding() {
        let mut arena = AstArena::new();
        assert_eq!(arena.array_stride_deferred(255, 257), Some(257));
        assert!(arena.check_limits().is_ok());
        assert_eq!(arena.array_stride_deferred(256, 256), None);
        assert!(matches!(
            arena.check_limits(),
            Err(Error::LimitExceeded {
                kind: LimitKind::ListLength,
                ..
            })
        ));
        assert_eq!(arena.node_count(), 0);
    }

    #[test]
    fn provenance_store_keeps_one_entry_inline_and_overflows_safely() {
        let mut one = AstArena::new();
        let first = one.push_space(crate::ast::Rational::one(), SpaceBasis::TextEm);
        assert!(matches!(one.provenance, ProvenanceStore::Inline(_)));
        assert_eq!(one.space_basis(first), Some(SpaceBasis::TextEm));

        let second = one.push_space(crate::ast::Rational::one(), SpaceBasis::MathMu);
        assert!(matches!(one.provenance, ProvenanceStore::Overflow(_)));
        assert_eq!(one.space_basis(first), Some(SpaceBasis::TextEm));
        assert_eq!(one.space_basis(second), Some(SpaceBasis::MathMu));

        let destination = one.push_node(CompactNode::Newline);
        one.replace_from(destination, first);
        assert_eq!(one.space_basis(destination), Some(SpaceBasis::TextEm));
        one.replace_from(destination, second);
        assert_eq!(one.space_basis(destination), Some(SpaceBasis::MathMu));
        let empty = one.push_node(CompactNode::Newline);
        one.replace_from(destination, empty);
        assert_eq!(one.space_basis(destination), None);
        one.provenance.remove(first);
        assert!(matches!(one.provenance, ProvenanceStore::Inline(_)));
        one.provenance.remove(second);
        assert!(matches!(one.provenance, ProvenanceStore::Empty));
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
