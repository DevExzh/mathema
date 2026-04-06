use super::style::MathStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId};
/// Inter-atom spacing based on the TeXBook 8×8 spacing table (p. 170).
use crate::ast::{MathNode, SymbolKind};

/// TeX atom types for spacing classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomType {
    Ord,   // ordinary
    Op,    // large operator
    Bin,   // binary operator
    Rel,   // relation
    Open,  // opening delimiter
    Close, // closing delimiter
    Punct, // punctuation
    Inner, // inner (fractions, etc.)
}

/// Classify a MathNode into its atom type for spacing purposes.
pub fn classify_node(node: &MathNode) -> AtomType {
    classify_node_depth(node, 0)
}

/// Depth-limited classification to prevent stack overflow on malicious ASTs.
fn classify_node_depth(node: &MathNode, depth: usize) -> AtomType {
    // Bail to safe default if recursion is too deep
    if depth > crate::ast::MAX_AST_DEPTH {
        return AtomType::Ord;
    }
    match node {
        MathNode::Number(_) => AtomType::Ord,
        MathNode::Identifier(_) => AtomType::Ord,
        MathNode::Operator(_) => AtomType::Op,
        MathNode::Symbol(kind, _) => match kind {
            SymbolKind::Ordinary | SymbolKind::Alphabetic => AtomType::Ord,
            SymbolKind::LargeOp => AtomType::Op,
            SymbolKind::BinaryOp => AtomType::Bin,
            SymbolKind::Relation => AtomType::Rel,
            SymbolKind::Open => AtomType::Open,
            SymbolKind::Close => AtomType::Close,
            SymbolKind::Punctuation => AtomType::Punct,
            SymbolKind::Fence => AtomType::Open,
        },
        MathNode::Text(..) => AtomType::Ord,
        MathNode::Space(_) => AtomType::Ord,
        MathNode::Row(_) => AtomType::Inner,
        MathNode::Fenced { .. } => AtomType::Inner,
        MathNode::Fraction { .. } => AtomType::Inner,
        MathNode::Sqrt(_) | MathNode::Root { .. } => AtomType::Ord,
        MathNode::Nary { .. } => AtomType::Op,
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
        | MathNode::PreScripts { base, .. } => classify_node_depth(base, depth + 1),
        MathNode::Overscript { base, .. }
        | MathNode::Underscript { base, .. }
        | MathNode::UnderOver { base, .. } => classify_node_depth(base, depth + 1),
        MathNode::Accent { base, .. } => classify_node_depth(base, depth + 1),
        MathNode::Array { .. } => AtomType::Inner,
        MathNode::Phantom(_) => AtomType::Ord,
        MathNode::Boxed(_) => AtomType::Inner,
        MathNode::Cancel { content, .. } => classify_node_depth(content, depth + 1),
        MathNode::Scaled { content, .. } => classify_node_depth(content, depth + 1),
        MathNode::Styled(_, children) => {
            if let Some(first) = children.first() {
                classify_node_depth(first, depth + 1)
            } else {
                AtomType::Ord
            }
        },
    }
}

/// Classify a CompactNode into its atom type for spacing purposes.
pub fn classify_node_c(arena: &AstArena, id: NodeId) -> AtomType {
    classify_node_c_depth(arena, id, 0)
}

fn classify_node_c_depth(arena: &AstArena, id: NodeId, depth: usize) -> AtomType {
    if depth > crate::ast::MAX_AST_DEPTH {
        return AtomType::Ord;
    }
    match *arena.node(id) {
        CompactNode::Number(_) => AtomType::Ord,
        CompactNode::Identifier(_) => AtomType::Ord,
        CompactNode::Operator { .. } => AtomType::Op,
        CompactNode::Symbol(kind, _) => match kind {
            SymbolKind::Ordinary | SymbolKind::Alphabetic => AtomType::Ord,
            SymbolKind::LargeOp => AtomType::Op,
            SymbolKind::BinaryOp => AtomType::Bin,
            SymbolKind::Relation => AtomType::Rel,
            SymbolKind::Open => AtomType::Open,
            SymbolKind::Close => AtomType::Close,
            SymbolKind::Punctuation => AtomType::Punct,
            SymbolKind::Fence => AtomType::Open,
        },
        CompactNode::Text(..) => AtomType::Ord,
        CompactNode::Space(_) => AtomType::Ord,
        CompactNode::Row(_) => AtomType::Inner,
        CompactNode::Fenced { .. } => AtomType::Inner,
        CompactNode::Fraction { .. } => AtomType::Inner,
        CompactNode::Sqrt(_) | CompactNode::Root { .. } => AtomType::Ord,
        CompactNode::Nary { .. } => AtomType::Op,
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
        | CompactNode::PreScripts { base, .. } => classify_node_c_depth(arena, base, depth + 1),
        CompactNode::Overscript { base, .. }
        | CompactNode::Underscript { base, .. }
        | CompactNode::UnderOver { base, .. } => classify_node_c_depth(arena, base, depth + 1),
        CompactNode::Accent { base, .. } => classify_node_c_depth(arena, base, depth + 1),
        CompactNode::Array { .. } => AtomType::Inner,
        CompactNode::Phantom(_) => AtomType::Ord,
        CompactNode::Boxed(_) => AtomType::Inner,
        CompactNode::Cancel { content, .. } => classify_node_c_depth(arena, content, depth + 1),
        CompactNode::Scaled { content, .. } => classify_node_c_depth(arena, content, depth + 1),
        CompactNode::Color { content, .. } => classify_node_c_depth(arena, content, depth + 1),
        CompactNode::Styled(_, span) => {
            let children = arena.children(span);
            if let Some(&first) = children.first() {
                classify_node_c_depth(arena, first, depth + 1)
            } else {
                AtomType::Ord
            }
        },
        CompactNode::Middle(_) | CompactNode::ColAlign(_) | CompactNode::Rule { .. } | CompactNode::StyleSwitch(_) => AtomType::Ord,
    }
}

/// TeXBook spacing table values in mu (1 mu = 1/18 em).
/// 0 = no space, 1 = thin space (3mu), 2 = medium space (4mu), 3 = thick space (5mu).
/// Negative values indicate spacing only in display/text styles.
/// Entry: SPACING_TABLE[left][right]
const SPACING_TABLE: [[i8; 8]; 8] = [
    //         Ord  Op  Bin  Rel  Open Close Punct Inner
    /* Ord   */
    [0, 1, -2, -3, 0, 0, 0, -1],
    /* Op    */ [1, 1, 0, -3, 0, 0, 0, -1],
    /* Bin   */ [-2, -2, 0, 0, -2, 0, 0, -2],
    /* Rel   */ [-3, -3, 0, 0, -3, 0, 0, -3],
    /* Open  */ [0, 0, 0, 0, 0, 0, 0, 0],
    /* Close */ [0, 1, -2, -3, 0, 0, 0, -1],
    /* Punct */ [-1, -1, 0, -1, -1, -1, -1, -1],
    /* Inner */ [-1, 1, -2, -3, -1, 0, -1, -1],
];

/// Pre-computed mu-to-em spacing values.
/// 0→0.0, 1→3/18, 2→4/18, 3→5/18
const MU_TO_EM: [f64; 4] = [0.0, 3.0 / 18.0, 4.0 / 18.0, 5.0 / 18.0];

/// Reclassify binary operators based on context (TeXBook Ch. 26).
///
/// In TeX, a `Bin` atom is reclassified as `Ord` in certain contexts:
///
///   * **Right atom is `Bin`**: becomes `Ord` when the left atom is
///     `None` (start of expression), `Bin`, `Op`, `Rel`, `Open`, or `Punct`.
///   * **Left atom is `Bin`**: becomes `Ord` when the right atom is
///     `Rel`, `Close`, `Punct`, or `None` (end of expression).
///
/// Returns the adjusted `(left, right)` atom types to be used for the
/// spacing-table lookup.
#[inline]
pub fn adjust_bin_context(
    left: Option<AtomType>,
    right: Option<AtomType>,
) -> (Option<AtomType>, Option<AtomType>) {
    let mut l = left;
    let mut r = right;

    // Right atom is Bin and context says it should be Ord
    if r == Some(AtomType::Bin) {
        match l {
            None
            | Some(AtomType::Bin)
            | Some(AtomType::Op)
            | Some(AtomType::Rel)
            | Some(AtomType::Open)
            | Some(AtomType::Punct) => {
                r = Some(AtomType::Ord);
            }
            _ => {}
        }
    }

    // Left atom is Bin and context says it should be Ord
    if l == Some(AtomType::Bin) {
        match r {
            None
            | Some(AtomType::Rel)
            | Some(AtomType::Close)
            | Some(AtomType::Punct) => {
                l = Some(AtomType::Ord);
            }
            _ => {}
        }
    }

    (l, r)
}

/// Get the inter-atom spacing in em units.
#[inline]
pub fn inter_atom_space(left: AtomType, right: AtomType, style: MathStyle) -> f64 {
    let l = left as usize;
    let r = right as usize;
    let entry = SPACING_TABLE[l][r];

    // Negative entries only apply in display/text styles
    let mu = if entry < 0 {
        if matches!(
            style,
            MathStyle::Script
                | MathStyle::ScriptCramped
                | MathStyle::ScriptScript
                | MathStyle::ScriptScriptCramped
        ) {
            return 0.0;
        }
        (-entry) as usize
    } else {
        entry as usize
    };

    // Direct array lookup instead of match chain
    if mu < 4 { MU_TO_EM[mu] } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ord_bin_spacing() {
        let s = inter_atom_space(AtomType::Ord, AtomType::Bin, MathStyle::Display);
        assert!((s - 4.0 / 18.0).abs() < 0.001);
    }

    #[test]
    fn ord_rel_thick_space() {
        let s = inter_atom_space(AtomType::Ord, AtomType::Rel, MathStyle::Text);
        assert!((s - 5.0 / 18.0).abs() < 0.001);
    }

    #[test]
    fn open_anything_zero() {
        for right in [AtomType::Ord, AtomType::Op, AtomType::Bin, AtomType::Rel] {
            assert_eq!(
                inter_atom_space(AtomType::Open, right, MathStyle::Display),
                0.0
            );
        }
    }

    #[test]
    fn op_bin_zero_space() {
        // Op-Bin is 0 in all styles (matching KaTeX)
        let s = inter_atom_space(AtomType::Op, AtomType::Bin, MathStyle::Script);
        assert_eq!(s, 0.0);
        let s = inter_atom_space(AtomType::Op, AtomType::Bin, MathStyle::Display);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn classify_number() {
        let node = MathNode::Number("42".into());
        assert_eq!(classify_node(&node), AtomType::Ord);
    }

    #[test]
    fn classify_symbol_kinds() {
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::BinaryOp, "+".into())),
            AtomType::Bin
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Relation, "=".into())),
            AtomType::Rel
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Open, "(".into())),
            AtomType::Open
        );
    }

    #[test]
    fn classify_all_symbol_kinds() {
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Close, ")".into())),
            AtomType::Close
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Punctuation, ",".into())),
            AtomType::Punct
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::LargeOp, "∑".into())),
            AtomType::Op
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Ordinary, "a".into())),
            AtomType::Ord
        );
        assert_eq!(
            classify_node(&MathNode::Symbol(SymbolKind::Fence, "|".into())),
            AtomType::Open
        );
    }

    #[test]
    fn classify_identifier() {
        assert_eq!(
            classify_node(&MathNode::Identifier("x".into())),
            AtomType::Ord
        );
    }

    #[test]
    fn classify_operator() {
        let op = MathNode::Operator(crate::ast::OperatorAttr {
            name: "lim".into(),
            limit_style: crate::ast::LimitStyle::Default,
        });
        assert_eq!(classify_node(&op), AtomType::Op);
    }

    #[test]
    fn classify_fraction() {
        let frac = MathNode::Fraction {
            kind: crate::ast::FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        };
        assert_eq!(classify_node(&frac), AtomType::Inner);
    }

    #[test]
    fn classify_superscript() {
        let sup = MathNode::Superscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sup: Box::new(MathNode::Number("2".into())),
        };
        assert_eq!(classify_node(&sup), AtomType::Ord);
    }

    #[test]
    fn classify_sqrt() {
        assert_eq!(
            classify_node(&MathNode::Sqrt(Box::new(MathNode::Number("2".into())))),
            AtomType::Ord
        );
    }

    #[test]
    fn classify_fenced() {
        let fenced = MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![],
        };
        assert_eq!(classify_node(&fenced), AtomType::Inner);
    }

    #[test]
    fn rel_ord_thick_space() {
        let s = inter_atom_space(AtomType::Rel, AtomType::Ord, MathStyle::Text);
        assert!(
            (s - 5.0 / 18.0).abs() < 0.001,
            "Rel-Ord should be thick space"
        );
    }

    #[test]
    fn bin_ord_medium_space() {
        let s = inter_atom_space(AtomType::Bin, AtomType::Ord, MathStyle::Text);
        assert!(
            (s - 4.0 / 18.0).abs() < 0.001,
            "Bin-Ord should be medium space"
        );
    }

    #[test]
    fn close_bin_medium_space() {
        let s = inter_atom_space(AtomType::Close, AtomType::Bin, MathStyle::Display);
        assert!(
            (s - 4.0 / 18.0).abs() < 0.001,
            "Close-Bin should be medium space"
        );
    }

    #[test]
    fn ord_ord_zero_space() {
        let s = inter_atom_space(AtomType::Ord, AtomType::Ord, MathStyle::Text);
        assert_eq!(s, 0.0, "Ord-Ord should have no spacing");
    }

    #[test]
    fn punct_ord_thin_space() {
        let s = inter_atom_space(AtomType::Punct, AtomType::Ord, MathStyle::Text);
        assert!(
            (s - 3.0 / 18.0).abs() < 0.001,
            "Punct-Ord should be thin space"
        );
    }

    #[test]
    fn inner_op_thin_space() {
        let s = inter_atom_space(AtomType::Inner, AtomType::Op, MathStyle::Display);
        assert!(
            (s - 3.0 / 18.0).abs() < 0.001,
            "Inner-Op should be thin space"
        );
    }

    #[test]
    fn punct_rel_thin_space() {
        // Punct-Rel is thin space in display/text (suppressed in script), matching KaTeX
        let s = inter_atom_space(AtomType::Punct, AtomType::Rel, MathStyle::Display);
        assert!(
            (s - 3.0 / 18.0).abs() < 0.001,
            "Punct-Rel should be thin space in display mode"
        );
        // Suppressed in script style
        let s2 = inter_atom_space(AtomType::Punct, AtomType::Rel, MathStyle::Script);
        assert!(
            s2.abs() < 0.001,
            "Punct-Rel should be suppressed in script mode"
        );
    }

    // ── adjust_bin_context tests (TeXBook Ch.26) ─────────────────────

    #[test]
    fn bin_at_start_becomes_ord() {
        // Right is Bin, left is None (start of expression) => Bin -> Ord
        let (l, r) = adjust_bin_context(None, Some(AtomType::Bin));
        assert_eq!(l, None);
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_open_becomes_ord() {
        // Right is Bin, left is Open => Bin -> Ord
        let (l, r) = adjust_bin_context(Some(AtomType::Open), Some(AtomType::Bin));
        assert_eq!(l, Some(AtomType::Open));
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_bin_becomes_ord() {
        // Right is Bin, left is Bin => right Bin -> Ord
        let (l, r) = adjust_bin_context(Some(AtomType::Bin), Some(AtomType::Bin));
        // left Bin also checked: right was Bin but is now Ord, so left stays Bin
        // Actually: right becomes Ord first, then left Bin check sees right=Ord
        // which is not in {Rel, Close, Punct, None}, so left stays Bin.
        assert_eq!(l, Some(AtomType::Bin));
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_rel_becomes_ord() {
        let (_, r) = adjust_bin_context(Some(AtomType::Rel), Some(AtomType::Bin));
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_op_becomes_ord() {
        let (_, r) = adjust_bin_context(Some(AtomType::Op), Some(AtomType::Bin));
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_punct_becomes_ord() {
        let (_, r) = adjust_bin_context(Some(AtomType::Punct), Some(AtomType::Bin));
        assert_eq!(r, Some(AtomType::Ord));
    }

    #[test]
    fn bin_after_ord_stays_bin() {
        // Ord-Bin: no reclassification
        let (l, r) = adjust_bin_context(Some(AtomType::Ord), Some(AtomType::Bin));
        assert_eq!(l, Some(AtomType::Ord));
        assert_eq!(r, Some(AtomType::Bin));
    }

    #[test]
    fn bin_after_close_stays_bin() {
        // Close-Bin: no reclassification of right
        let (l, r) = adjust_bin_context(Some(AtomType::Close), Some(AtomType::Bin));
        assert_eq!(l, Some(AtomType::Close));
        assert_eq!(r, Some(AtomType::Bin));
    }

    #[test]
    fn bin_before_rel_becomes_ord() {
        // Left is Bin, right is Rel => left Bin -> Ord
        let (l, r) = adjust_bin_context(Some(AtomType::Bin), Some(AtomType::Rel));
        assert_eq!(l, Some(AtomType::Ord));
        assert_eq!(r, Some(AtomType::Rel));
    }

    #[test]
    fn bin_before_close_becomes_ord() {
        let (l, _) = adjust_bin_context(Some(AtomType::Bin), Some(AtomType::Close));
        assert_eq!(l, Some(AtomType::Ord));
    }

    #[test]
    fn bin_before_punct_becomes_ord() {
        let (l, _) = adjust_bin_context(Some(AtomType::Bin), Some(AtomType::Punct));
        assert_eq!(l, Some(AtomType::Ord));
    }

    #[test]
    fn bin_at_end_becomes_ord() {
        // Left is Bin, right is None (end of expression) => left Bin -> Ord
        let (l, r) = adjust_bin_context(Some(AtomType::Bin), None);
        assert_eq!(l, Some(AtomType::Ord));
        assert_eq!(r, None);
    }

    #[test]
    fn non_bin_atoms_unchanged() {
        // Non-Bin atoms should pass through unmodified
        let (l, r) = adjust_bin_context(Some(AtomType::Ord), Some(AtomType::Rel));
        assert_eq!(l, Some(AtomType::Ord));
        assert_eq!(r, Some(AtomType::Rel));

        let (l, r) = adjust_bin_context(Some(AtomType::Open), Some(AtomType::Close));
        assert_eq!(l, Some(AtomType::Open));
        assert_eq!(r, Some(AtomType::Close));
    }
}
