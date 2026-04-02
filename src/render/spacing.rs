use super::style::MathStyle;
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

/// TeXBook spacing table values in mu (1 mu = 1/18 em).
/// 0 = no space, 1 = thin space (3mu), 2 = medium space (4mu), 3 = thick space (5mu).
/// Negative values indicate spacing only in display/text styles.
/// Entry: SPACING_TABLE[left][right]
const SPACING_TABLE: [[i8; 8]; 8] = [
    //         Ord  Op  Bin  Rel  Open Close Punct Inner
    /* Ord   */
    [0, 1, 2, 3, 0, 0, 0, 1],
    /* Op    */ [1, 1, -2, 3, 0, 0, 0, 1],
    /* Bin   */ [2, 2, 0, 0, 2, 0, 0, 2],
    /* Rel   */ [3, 3, 0, 0, 3, 0, 0, 3],
    /* Open  */ [0, 0, 0, 0, 0, 0, 0, 0],
    /* Close */ [0, 1, 2, 3, 0, 0, 0, 1],
    /* Punct */ [1, 1, 0, 1, 1, 1, 1, 1],
    /* Inner */ [1, 1, 2, 3, 1, 0, 1, 1],
];

/// Get the inter-atom spacing in em units.
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
            0
        } else {
            -entry
        }
    } else {
        entry
    };

    // Convert mu values to actual spacing in em
    // 0→0, 1→3mu, 2→4mu, 3→5mu, where 18mu = 1em
    match mu {
        0 => 0.0,
        1 => 3.0 / 18.0, // thin space
        2 => 4.0 / 18.0, // medium space
        3 => 5.0 / 18.0, // thick space
        _ => 0.0,
    }
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
    fn script_suppresses_negative_entries() {
        // Op-Bin has entry -2, suppressed in script styles
        let s = inter_atom_space(AtomType::Op, AtomType::Bin, MathStyle::Script);
        assert_eq!(s, 0.0);
        // But not in display
        let s = inter_atom_space(AtomType::Op, AtomType::Bin, MathStyle::Display);
        assert!((s - 4.0 / 18.0).abs() < 0.001);
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
    fn script_suppresses_all_negative() {
        // Op-Bin has entry -2; in script styles it should be suppressed to 0
        for style in [
            MathStyle::Script,
            MathStyle::ScriptCramped,
            MathStyle::ScriptScript,
            MathStyle::ScriptScriptCramped,
        ] {
            let s = inter_atom_space(AtomType::Op, AtomType::Bin, style);
            assert_eq!(s, 0.0, "script style should suppress negative spacing");
        }
    }

    #[test]
    fn display_and_text_allow_negative() {
        for style in [
            MathStyle::Display,
            MathStyle::Text,
            MathStyle::DisplayCramped,
            MathStyle::TextCramped,
        ] {
            let s = inter_atom_space(AtomType::Op, AtomType::Bin, style);
            assert!(s > 0.0, "{:?} should allow negative-origin spacing", style);
        }
    }
}
