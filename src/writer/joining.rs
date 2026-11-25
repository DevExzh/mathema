//! Joining neighbours that differ only in what a format does not write.

use smallvec::SmallVec;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId};
use crate::ast::normalize::number_decimal_points;

/// Builds a list, making one node of neighbours that only an unwritten
/// property keeps apart, where normalization would: text of one style, digits,
/// and ASCII identifiers.
///
/// `K` is the value of the property (of all of them, if there are several).
/// Neighbours in which it is the same are apart for another reason and stay
/// so: without the property the list is written as it would be anyway. Nothing
/// but the list decides this, so that it does not matter how it was built.
pub(super) struct Joiner<K> {
    /// The property of the node added last.
    previous: Option<K>,
    joined: Option<Joined>,
}

/// Neighbours that become one node.
struct Joined {
    /// The index of the first one in the list.
    at: usize,
    text: String,
    /// The decimal points in it, for numbers.
    points: usize,
}

impl<K: PartialEq> Joiner<K> {
    pub(super) fn new() -> Self {
        Self {
            previous: None,
            joined: None,
        }
    }

    /// Add the node `id` to the list `out`, where its property is `key`. Nodes
    /// pushed to `out` in between keep their neighbours apart.
    pub(super) fn add(
        &mut self,
        arena: &mut AstArena,
        out: &mut SmallVec<[NodeId; 8]>,
        id: NodeId,
        key: K,
    ) {
        let differs = self
            .previous
            .as_ref()
            .is_some_and(|previous| *previous != key);
        self.previous = Some(key);
        if differs
            && let Some(&last) = out.last()
            && self.join(arena, out.len() - 1, last, id)
        {
            return;
        }
        self.finish(arena, out);
        out.push(id);
    }

    /// Join `next` to `last`, at `at` in the list, if normalization would. Only
    /// the new part is looked at, whatever has been joined before.
    fn join(&mut self, arena: &AstArena, at: usize, last: NodeId, next: NodeId) -> bool {
        // What was joined before is the end of the list only if it is there.
        let joined = self.joined.as_ref().filter(|joined| joined.at == at);
        let points = |sid| number_decimal_points(arena.str_val(sid));
        let (before, after, dots) = match (*arena.node(last), *arena.node(next)) {
            (CompactNode::Text(a, before), CompactNode::Text(b, after)) if a == b => {
                (before, after, 0)
            },
            (CompactNode::Number(before), CompactNode::Number(after)) => {
                let so_far = match joined {
                    Some(joined) => Some(joined.points),
                    None => points(before),
                };
                // One decimal point at most.
                match (so_far, points(after)) {
                    (Some(a), Some(b)) if a + b <= 1 => (before, after, a + b),
                    _ => return false,
                }
            },
            (CompactNode::Identifier(before), CompactNode::Identifier(after))
                if (joined.is_some() || arena.str_val(before).is_ascii())
                    && arena.str_val(after).is_ascii() =>
            {
                (before, after, 0)
            },
            _ => return false,
        };
        if joined.is_none() && self.joined.is_some() {
            return false;
        }
        let joined = self.joined.get_or_insert_with(|| Joined {
            at,
            text: arena.str_val(before).to_owned(),
            points: 0,
        });
        joined.text.push_str(arena.str_val(after));
        joined.points = dots;
        true
    }

    /// Put joined text in the place of its first part.
    pub(super) fn finish(&mut self, arena: &mut AstArena, out: &mut [NodeId]) {
        if let Some(Joined { at, text, .. }) = self.joined.take() {
            let text = arena.push_str(&text);
            let node = match *arena.node(out[at]) {
                CompactNode::Text(style, _) => CompactNode::Text(style, text),
                CompactNode::Number(_) => CompactNode::Number(text),
                CompactNode::Identifier(_) => CompactNode::Identifier(text),
                _ => return,
            };
            out[at] = arena.push_node(node);
        }
    }
}
