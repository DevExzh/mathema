//! Inter-atom spacing based on the TeXBook 8×8 spacing table (p. 170).

use super::style::MathStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactNode, NodeId};
use crate::ast::{MathAtomType, MathNode, SymbolKind};

/// TeX atom types for spacing classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomType {
    /// Ordinary atom (e.g. a variable or number).
    Ord,
    /// Large operator (e.g. summation, integral).
    Op,
    /// Binary operator (e.g. `+`, `-`).
    Bin,
    /// Relation (e.g. `=`, `<`).
    Rel,
    /// Opening delimiter (e.g. `(`).
    Open,
    /// Closing delimiter (e.g. `)`).
    Close,
    /// Punctuation (e.g. `,`).
    Punct,
    /// Inner atom (e.g. fractions, fenced groups).
    Inner,
}

/// The atom of a symbol. `\ldots`, `\cdots` and `\ddots` are inner atoms.
fn symbol_atom(kind: SymbolKind, text: &str) -> AtomType {
    match kind {
        SymbolKind::Ordinary if matches!(text, "…" | "⋯" | "⋱" | "⋰") => AtomType::Inner,
        // amsmath's `\colon` has its own space around an ordinary colon.
        SymbolKind::Punctuation if text == ":" => AtomType::Ord,
        // TeX's math codes of these are closing ones.
        SymbolKind::Ordinary if matches!(text, "!" | "?") => AtomType::Close,
        SymbolKind::Ordinary | SymbolKind::Alphabetic | SymbolKind::Fence => AtomType::Ord,
        SymbolKind::LargeOp => AtomType::Op,
        SymbolKind::BinaryOp => AtomType::Bin,
        SymbolKind::Relation => AtomType::Rel,
        SymbolKind::Open => AtomType::Open,
        SymbolKind::Close => AtomType::Close,
        SymbolKind::Punctuation => AtomType::Punct,
    }
}

/// The atom of an accented node: `\underbrace`, `\overbracket` and the like
/// are operators.
fn accent_atom(accent: char) -> AtomType {
    match accent {
        '⏞' | '⏟' | '⎴' | '⎵' => AtomType::Op,
        _ => AtomType::Ord,
    }
}

/// The atom of a node with the mark `node` over or under it, `\underbrace` or
/// `\overline`, unless it is a label.
fn mark_atom(text: &str) -> Option<AtomType> {
    super::layout::canonical_stretchy_overunder(text)
        .and_then(|mark| mark.chars().next())
        .map(accent_atom)
}

/// Spaces are no atoms: the ones on their sides are still adjacent.
pub(super) fn is_space(node: &MathNode) -> bool {
    matches!(node, MathNode::Space(_) | MathNode::ExplicitSpace { .. })
}

/// Compact-AST version of `is_space`.
pub(super) fn is_space_c(arena: &AstArena, id: NodeId) -> bool {
    matches!(arena.node(id), CompactNode::Space(_))
}

/// The atom of the first of `nodes`, which a list ends with. TeX reads a binary
/// operator as ordinary at the end of a list and before a relation, a closing or
/// a punctuation.
pub(super) fn first_atom(nodes: &[MathNode], style: MathStyle) -> AtomType {
    let atom = classify_node_in(&nodes[0], style);
    if atom != AtomType::Bin {
        return atom;
    }
    let next = nodes[1..].iter().find(|node| {
        !is_space(node) && !matches!(node, MathNode::StyleSwitch(_) | MathNode::SizeSwitch(_))
    });
    bin_before(next.map(|node| classify_node_in(node, style)))
}

/// Compact-AST version of `first_atom`.
pub(super) fn first_atom_c(arena: &AstArena, ids: &[NodeId], style: MathStyle) -> AtomType {
    let atom = classify_node_c_in(arena, ids[0], style);
    if atom != AtomType::Bin {
        return atom;
    }
    let next = ids[1..].iter().find(|&&id| {
        !is_space_c(arena, id)
            && !matches!(
                arena.node(id),
                CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_)
            )
    });
    bin_before(next.map(|&id| classify_node_c_in(arena, id, style)))
}

/// What a binary operator is before `next`.
pub(super) fn bin_before(next: Option<AtomType>) -> AtomType {
    match next {
        None | Some(AtomType::Rel | AtomType::Close | AtomType::Punct) => AtomType::Ord,
        Some(_) => AtomType::Bin,
    }
}

/// The atom that what follows a node is spaced against. A big operator holds
/// its operand, which is the next atom of TeX's list.
pub(super) fn right_atom(node: &MathNode, atom: AtomType, style: MathStyle) -> AtomType {
    let (mut node, mut atom) = (node, atom);
    while let MathNode::Nary { body, .. } = node {
        let Some(next) = nary_body_atom(body, style) else {
            break;
        };
        (node, atom) = (body, next);
    }
    atom
}

/// The atom of a big operator's operand, unless it is a space or nothing.
pub(super) fn nary_body_atom(body: &MathNode, style: MathStyle) -> Option<AtomType> {
    if is_space(body) || matches!(body, MathNode::Row(nodes) if nodes.is_empty()) {
        return None;
    }
    adjust_bin_context(Some(AtomType::Op), Some(classify_node_in(body, style))).1
}

/// Compact-AST version of `right_atom`.
pub(super) fn right_atom_c(
    arena: &AstArena,
    id: NodeId,
    atom: AtomType,
    style: MathStyle,
) -> AtomType {
    let (mut id, mut atom) = (id, atom);
    while let CompactNode::Nary { body, .. } = *arena.node(id) {
        let Some(next) = nary_body_atom_c(arena, body, style) else {
            break;
        };
        (id, atom) = (body, next);
    }
    atom
}

/// Compact-AST version of `nary_body_atom`.
pub(super) fn nary_body_atom_c(
    arena: &AstArena,
    body: NodeId,
    style: MathStyle,
) -> Option<AtomType> {
    if is_space_c(arena, body)
        || matches!(*arena.node(body), CompactNode::Row(span) if span.is_empty())
    {
        return None;
    }
    adjust_bin_context(
        Some(AtomType::Op),
        Some(classify_node_c_in(arena, body, style)),
    )
    .1
}

/// Classify a MathNode into its atom type for spacing purposes.
pub fn classify_node(node: &MathNode) -> AtomType {
    classify_node_in(node, MathStyle::Display)
}

pub(super) fn classify_node_in(node: &MathNode, style: MathStyle) -> AtomType {
    classify_node_depth(node, style, 0)
}

/// Depth-limited classification to prevent stack overflow on malicious ASTs.
fn classify_node_depth(node: &MathNode, style: MathStyle, depth: usize) -> AtomType {
    // Bail to safe default if recursion is too deep
    if depth > crate::ast::MAX_AST_DEPTH {
        return AtomType::Ord;
    }
    match node {
        MathNode::Attributed { content, .. } => classify_node_depth(content, style, depth + 1),
        MathNode::Number(_) => AtomType::Ord,
        MathNode::Identifier(_) => AtomType::Ord,
        MathNode::Operator(_) | MathNode::StructuredOperator { .. } => AtomType::Op,
        MathNode::Symbol(kind, text) => symbol_atom(*kind, text),
        MathNode::Text(..)
        | MathNode::StyleSwitch(_)
        | MathNode::SizeSwitch(_)
        | MathNode::LineBreak
        | MathNode::UndefinedCommand { .. } => AtomType::Ord,
        MathNode::Space(_) | MathNode::ExplicitSpace { .. } | MathNode::Rule { .. } => {
            AtomType::Ord
        },
        // A group is ordinary, and LaTeX's `\frac` and `\boxed` are groups.
        MathNode::Row(_) | MathNode::Fraction { .. } => AtomType::Ord,
        MathNode::Boxed { .. } | MathNode::Actuarial { .. } => AtomType::Ord,
        MathNode::Fenced { .. } => AtomType::Inner,
        MathNode::Sqrt(_) | MathNode::Root { .. } => AtomType::Ord,
        MathNode::Nary { .. } => AtomType::Op,
        MathNode::Subscript { base, .. }
        | MathNode::Superscript { base, .. }
        | MathNode::SubSup { base, .. }
        | MathNode::PreScripts { base, .. } => classify_node_depth(base, style, depth + 1),
        MathNode::Overscript {
            base, over: mark, ..
        }
        | MathNode::Underscript {
            base, under: mark, ..
        } => match &**mark {
            MathNode::Symbol(_, text) => mark_atom(text),
            _ => None,
        }
        .unwrap_or_else(|| classify_node_depth(base, style, depth + 1)),
        MathNode::UnderOver { base, .. } => classify_node_depth(base, style, depth + 1),
        MathNode::Accent { accent, .. } => accent_atom(*accent),
        // TeX's bare `array`, `matrix`, `smallmatrix`, `aligned`, and
        // `gathered` environments produce an ordinary/vcenter noad. Their
        // table padding belongs to the node itself and does not trigger the
        // thin Ord–Inner spaces that an explicitly fenced matrix receives.
        // MathJax likewise emits a bare `mtable` with texClass=ORD.
        MathNode::Array { .. } | MathNode::Matrix { .. } | MathNode::NativeArray { .. } => {
            AtomType::Ord
        },
        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            classify_node_depth(content, style, depth + 1)
        },
        MathNode::Phantom(_, _) => AtomType::Ord,
        MathNode::Reflect { .. } => AtomType::Ord,
        MathNode::Cancel { content, .. } => classify_node_depth(content, style, depth + 1),
        // TeX's `\big` boxes its delimiter, so the result is an ordinary
        // atom; `\bigl`/`\bigr`/`\bigm` add an explicit MathAtom class.
        MathNode::Scaled { .. } => AtomType::Ord,
        MathNode::RaiseBox { content, .. } => classify_node_depth(content, style, depth + 1),
        MathNode::Styled(_, children) => {
            if let Some(first) = children.first() {
                classify_node_depth(first, style, depth + 1)
            } else {
                AtomType::Ord
            }
        },
        MathNode::Color { content, .. } => classify_node_depth(content, style, depth + 1),
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => classify_node_depth(
            style.choose(display, text, script, script_script),
            style,
            depth + 1,
        ),
        MathNode::MathAtom { atom_type, .. } => match atom_type {
            MathAtomType::Ord => AtomType::Ord,
            MathAtomType::Op => AtomType::Op,
            MathAtomType::Bin => AtomType::Bin,
            MathAtomType::Rel => AtomType::Rel,
            MathAtomType::Open => AtomType::Open,
            MathAtomType::Close => AtomType::Close,
            MathAtomType::Punct => AtomType::Punct,
            MathAtomType::Inner => AtomType::Inner,
        },
    }
}

/// Classify a CompactNode into its atom type for spacing purposes.
pub fn classify_node_c(arena: &AstArena, id: NodeId) -> AtomType {
    classify_node_c_in(arena, id, MathStyle::Display)
}

pub(super) fn classify_node_c_in(arena: &AstArena, id: NodeId, style: MathStyle) -> AtomType {
    classify_node_c_depth(arena, id, style, 0)
}

fn classify_node_c_depth(arena: &AstArena, id: NodeId, style: MathStyle, depth: usize) -> AtomType {
    if depth > crate::ast::MAX_AST_DEPTH {
        return AtomType::Ord;
    }
    match *arena.node(id) {
        CompactNode::Number(_) => AtomType::Ord,
        CompactNode::Identifier(_) => AtomType::Ord,
        CompactNode::Operator { .. } | CompactNode::StructuredOperator { .. } => AtomType::Op,
        CompactNode::Symbol(kind, sid) => symbol_atom(kind, arena.str_val(sid)),
        CompactNode::Text(..) | CompactNode::UndefinedCommand { .. } => AtomType::Ord,
        CompactNode::Space(_) => AtomType::Ord,
        // A group is ordinary, and LaTeX's `\frac` and `\boxed` are groups.
        CompactNode::Row(_) | CompactNode::Fraction { .. } => AtomType::Ord,
        CompactNode::Boxed { .. } | CompactNode::Actuarial { .. } => AtomType::Ord,
        CompactNode::Fenced { .. } => AtomType::Inner,
        CompactNode::Sqrt(_) | CompactNode::Root { .. } => AtomType::Ord,
        CompactNode::Nary { .. } => AtomType::Op,
        CompactNode::Subscript { base, .. }
        | CompactNode::Superscript { base, .. }
        | CompactNode::SubSup { base, .. }
        | CompactNode::PreScripts { base, .. } => {
            classify_node_c_depth(arena, base, style, depth + 1)
        },
        CompactNode::Overscript {
            base, over: mark, ..
        }
        | CompactNode::Underscript {
            base, under: mark, ..
        } => match *arena.node(mark) {
            CompactNode::Symbol(_, sid) => mark_atom(arena.str_val(sid)),
            _ => None,
        }
        .unwrap_or_else(|| classify_node_c_depth(arena, base, style, depth + 1)),
        CompactNode::UnderOver { base, .. } => classify_node_c_depth(arena, base, style, depth + 1),
        CompactNode::Accent { accent, .. } => accent_atom(accent),
        // Keep heap and compact classification identical. Native tables are
        // ordinary noads until a fence or explicit MathAtom wrapper changes
        // their class.
        CompactNode::Array { .. } => AtomType::Ord,
        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            classify_node_c_depth(arena, content, style, depth + 1)
        },
        CompactNode::Phantom(..) => AtomType::Ord,
        CompactNode::Reflect { .. } => AtomType::Ord,
        CompactNode::Cancel { content, .. } => {
            classify_node_c_depth(arena, content, style, depth + 1)
        },
        // TeX's `\big` boxes its delimiter, so the result is an ordinary
        // atom; `\bigl`/`\bigr`/`\bigm` add an explicit MathAtom class.
        CompactNode::Scaled { .. } => AtomType::Ord,
        CompactNode::RaiseBox { content, .. } => {
            classify_node_c_depth(arena, content, style, depth + 1)
        },
        CompactNode::Color { content, .. } => {
            classify_node_c_depth(arena, content, style, depth + 1)
        },
        CompactNode::Styled(_, span) => {
            let children = arena.children(span);
            if let Some(&first) = children.first() {
                classify_node_c_depth(arena, first, style, depth + 1)
            } else {
                AtomType::Ord
            }
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => classify_node_c_depth(
            arena,
            style.choose(display, text, script, script_script),
            style,
            depth + 1,
        ),
        CompactNode::MathAtom { atom_type, .. } => match atom_type {
            MathAtomType::Ord => AtomType::Ord,
            MathAtomType::Op => AtomType::Op,
            MathAtomType::Bin => AtomType::Bin,
            MathAtomType::Rel => AtomType::Rel,
            MathAtomType::Open => AtomType::Open,
            MathAtomType::Close => AtomType::Close,
            MathAtomType::Punct => AtomType::Punct,
            MathAtomType::Inner => AtomType::Inner,
        },
        CompactNode::Middle(_)
        | CompactNode::ColAlign(_)
        | CompactNode::Rule { .. }
        | CompactNode::StyleSwitch(_)
        | CompactNode::SizeScale(_)
        | CompactNode::Newline => AtomType::Ord,
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
            },
            _ => {},
        }
    }

    // Left atom is Bin and context says it should be Ord
    if l == Some(AtomType::Bin) {
        match r {
            None | Some(AtomType::Rel) | Some(AtomType::Close) | Some(AtomType::Punct) => {
                l = Some(AtomType::Ord);
            },
            _ => {},
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
            AtomType::Ord
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
        assert_eq!(classify_node(&frac), AtomType::Ord);
    }

    #[test]
    fn bare_tables_are_ordinary_atoms_on_heap_and_compact_paths() {
        let rows = vec![vec![vec![MathNode::Identifier("x".into())]]];
        let array = MathNode::Array {
            columns: vec![crate::ast::ColumnAlign::Center],
            rows: rows.clone(),
            arraystretch: None,
            add_jot: false,
        };
        let matrix = MathNode::Matrix {
            kind: crate::ast::MatrixKind::Matrix,
            alignment: crate::ast::ColumnAlign::Center,
            rows: rows.clone(),
        };
        let native = MathNode::NativeArray {
            columns: vec![crate::ast::ColumnAlign::Center],
            rows,
            arraystretch: None,
            add_jot: false,
            array_style: true,
            col_seps: 0,
            row_seps: 0,
            dashed_col_seps: 0,
            dashed_row_seps: 0,
        };
        for node in [&array, &matrix, &native] {
            assert_eq!(classify_node(node), AtomType::Ord);
            let compact = crate::ast::compact::CompactExpr::from_math_expr(
                &crate::ast::MathExpr::inline(vec![node.clone()]),
            )
            .expect("table compaction");
            let id = compact
                .arena
                .children(compact.roots)
                .first()
                .expect("compact table root");
            assert_eq!(classify_node_c(&compact.arena, *id), AtomType::Ord);
        }
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

    #[test]
    fn exclamation_and_question_marks_close() {
        // TeX's math codes: `x=!y` has no space after the relation.
        for text in ["!", "?"] {
            let node = MathNode::Symbol(SymbolKind::Ordinary, text.into());
            assert_eq!(classify_node(&node), AtomType::Close);
        }
        let node = MathNode::Symbol(SymbolKind::Ordinary, "@".into());
        assert_eq!(classify_node(&node), AtomType::Ord);
    }

    #[test]
    fn close_ord_zero_space() {
        // Close-Ord (e.g. )! in factorial) should have zero spacing
        let s = inter_atom_space(AtomType::Close, AtomType::Ord, MathStyle::Text);
        assert_eq!(s, 0.0, "Close-Ord should be zero (factorial '!' after ')')");
    }

    #[test]
    fn paren_spacing_zero_around_content() {
        // Verify all paren-adjacent pairs have zero spacing
        // Ord-Open: Γ(
        assert_eq!(
            inter_atom_space(AtomType::Ord, AtomType::Open, MathStyle::Text),
            0.0,
            "Ord-Open should be zero"
        );
        // Open-Ord: (n
        assert_eq!(
            inter_atom_space(AtomType::Open, AtomType::Ord, MathStyle::Text),
            0.0,
            "Open-Ord should be zero"
        );
        // Ord-Close: n)
        assert_eq!(
            inter_atom_space(AtomType::Ord, AtomType::Close, MathStyle::Text),
            0.0,
            "Ord-Close should be zero"
        );
    }

    #[test]
    fn big_delimiters_use_tex_atom_classes() {
        // `\big` boxes its delimiter (Ord); `\bigl`/`\bigr`/`\bigm` are
        // `\mathopen`/`\mathclose`/`\mathrel` around it.
        for (source, class) in [
            (r"\big(", AtomType::Ord),
            (r"\Big)", AtomType::Ord),
            (r"\bigl(", AtomType::Open),
            (r"\biggr]", AtomType::Close),
            (r"\Biggm|", AtomType::Rel),
        ] {
            let parsed = crate::from_latex(source).unwrap();
            let expr = parsed.expr();
            assert_eq!(classify_node(&expr.nodes[0]), class, "{source}");
            let compact = crate::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
            let id = compact.arena.children(compact.roots)[0];
            assert_eq!(classify_node_c(&compact.arena, id), class, "{source}");
        }
    }
}
