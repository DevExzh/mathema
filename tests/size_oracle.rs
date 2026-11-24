//! Glyph sizes against an independent oracle: a generated tree is spelled as
//! MathML and as LaTeX, its leaves' sizes are computed from the tree alone,
//! and every path must draw each leaf at that size.
#![cfg(feature = "render")]

use mathema::render::{RenderOptions, render_to_svg, render_to_svg_compact};
use mathema::{CompactExpr, ConvertResult, Format, convert, from_latex, from_mathml};
use proptest::prelude::*;
use std::collections::BTreeMap;

const SIZES: [(&str, f64); 10] = [
    ("tiny", 0.5),
    ("scriptsize", 0.7),
    ("footnotesize", 0.8),
    ("small", 0.9),
    ("normalsize", 1.0),
    ("large", 1.2),
    ("Large", 1.44),
    ("LARGE", 1.728),
    ("huge", 2.074),
    ("Huge", 2.488),
];

#[derive(Clone, Debug)]
enum Tree {
    Leaf,
    Text,
    /// `+`, `∑`, `∫`, a comma or a space: material beside the leaves.
    Op(usize),
    /// Content at one of `SIZES`.
    Sized(usize, Vec<Tree>),
    Group(Vec<Tree>),
    Sqrt(Vec<Tree>),
    Fenced(Vec<Tree>),
    Bold(Vec<Tree>),
    Table(Vec<Tree>, Vec<Tree>),
    Script(Vec<Tree>),
}

fn trees() -> impl Strategy<Value = Vec<Tree>> {
    let leaf = prop_oneof![
        4 => Just(Tree::Leaf),
        1 => Just(Tree::Text),
        3 => (0..5usize).prop_map(Tree::Op),
    ];
    let tree = leaf.prop_recursive(4, 20, 3, |inner| {
        let list = || prop::collection::vec(inner.clone(), 1..=3);
        prop_oneof![
            // Often the normal size, as a run that changes nothing.
            5 => (prop_oneof![2 => 0..10usize, 1 => Just(4)], list()).prop_map(|(size, list)| Tree::Sized(size, list)),
            1 => list().prop_map(Tree::Group),
            1 => list().prop_map(Tree::Sqrt),
            1 => list().prop_map(Tree::Fenced),
            1 => list().prop_map(Tree::Bold),
            1 => (list(), list()).prop_map(|(a, b)| Tree::Table(a, b)),
            1 => list().prop_map(Tree::Script),
        ]
    });
    prop::collection::vec(tree, 1..=4)
}

/// The two spellings and the expected size of each leaf.
#[derive(Default)]
struct Spelling {
    leaves: BTreeMap<char, f64>,
}

impl Spelling {
    fn leaf(&mut self, size: f64, level: usize) -> Option<char> {
        // `i` and `l` can be part of operators' names.
        let letter = "abcdefghjkmnopqrstuvwxyz".chars().nth(self.leaves.len())?;
        let script = [1.0, 0.7, 0.5][level.min(2)];
        self.leaves.insert(letter, size * script);
        Some(letter)
    }

    /// `(mathml, latex)` of a list; `size` is the size in force around it.
    fn list(&mut self, list: &[Tree], size: f64, level: usize) -> (String, String) {
        let mut mathml = String::new();
        let mut latex = String::new();
        for tree in list {
            let (m, l) = self.tree(tree, size, level);
            mathml += &m;
            latex += &l;
        }
        (mathml, latex)
    }

    fn tree(&mut self, tree: &Tree, size: f64, level: usize) -> (String, String) {
        match tree {
            Tree::Leaf | Tree::Text => {
                let Some(letter) = self.leaf(size, level) else {
                    return Default::default();
                };
                if matches!(tree, Tree::Leaf) {
                    (format!("<mi>{letter}</mi>"), letter.to_string())
                } else {
                    (
                        format!("<mtext>{letter}</mtext>"),
                        format!(r"\text{{{letter}}}"),
                    )
                }
            },
            Tree::Op(op) => {
                let (m, l) = [
                    ("<mo>+</mo>", "+"),
                    ("<mo>∑</mo>", r"\sum "),
                    ("<mo>∫</mo>", r"\int "),
                    ("<mo>,</mo>", ","),
                    (r#"<mspace width="1em"/>"#, r"\quad "),
                ][*op];
                (m.to_owned(), l.to_owned())
            },
            Tree::Sized(index, list) => {
                let (name, scale) = SIZES[*index];
                let (m, l) = self.list(list, scale, level);
                // MathML sizes are relative, TeX's are not.
                let percent = 100.0 * scale / size;
                (
                    format!(r#"<mstyle mathsize="{percent:.4}%">{m}</mstyle>"#),
                    format!(r"{{\{name} {l}}}"),
                )
            },
            Tree::Group(list) => {
                let (m, l) = self.list(list, size, level);
                (format!("<mrow>{m}</mrow>"), format!("{{{l}}}"))
            },
            Tree::Sqrt(list) => {
                let (m, l) = self.list(list, size, level);
                (format!("<msqrt>{m}</msqrt>"), format!(r"\sqrt{{{l}}}"))
            },
            Tree::Fenced(list) => {
                let (m, l) = self.list(list, size, level);
                (
                    format!(r#"<mrow><mo fence="true">(</mo>{m}<mo fence="true">)</mo></mrow>"#),
                    format!(r"\left({l}\right)"),
                )
            },
            Tree::Bold(list) => {
                let (m, l) = self.list(list, size, level);
                (
                    format!(r#"<mstyle mathvariant="bold">{m}</mstyle>"#),
                    format!(r"\mathbf{{{l}}}"),
                )
            },
            Tree::Table(a, b) => {
                // TeX sets the cells in text style, MathML at the level around
                // the table; they agree outside scripts.
                if level > 0 {
                    return Default::default();
                }
                let (am, al) = self.list(a, size, level);
                let (bm, bl) = self.list(b, size, level);
                (
                    format!("<mtable><mtr><mtd>{am}</mtd><mtd>{bm}</mtd></mtr></mtable>"),
                    format!(r"\begin{{matrix}}{al}&{bl}\end{{matrix}}"),
                )
            },
            Tree::Script(list) => {
                let (m, l) = self.list(list, size, level + 1);
                (
                    format!("<msup><mo>+</mo><mrow>{m}</mrow></msup>"),
                    format!("{{+}}^{{{l}}}"),
                )
            },
        }
    }
}

/// The base letter of a drawn glyph (math italic and bold letters).
fn letter(c: char) -> Option<char> {
    if c.is_ascii_lowercase() {
        return Some(c);
    }
    if c == 'ℎ' {
        return Some('h');
    }
    [0x1D41A, 0x1D44E, 0x1D482]
        .into_iter()
        .filter_map(|start| (c as u32).checked_sub(start))
        .find(|offset| *offset < 26)
        .map(|offset| char::from(b'a' + offset as u8))
}

fn drawn(svg: &str) -> BTreeMap<char, f64> {
    let mut sizes = BTreeMap::new();
    for text in svg.split("<text ").skip(1) {
        let size = text.split("font-size=\"").nth(1).unwrap();
        let size: f64 = size.split('"').next().unwrap().parse().unwrap();
        let content = text.split('>').nth(1).unwrap().split('<').next().unwrap();
        for c in content.chars().filter_map(letter) {
            sizes.insert(c, size / 16.0);
        }
    }
    sizes
}

fn check(leaves: &BTreeMap<char, f64>, svg: &str, path: &str, source: &str) {
    let drawn = drawn(svg);
    for (leaf, size) in leaves {
        let found = drawn.get(leaf).copied().unwrap_or(0.0);
        assert!(
            (found - size).abs() < 0.01,
            "{path}: {leaf} is {found}, expected {size}\n{source}"
        );
    }
}

fn compact_svg(expr: &CompactExpr) -> String {
    render_to_svg_compact(expr, &RenderOptions::default()).unwrap()
}

/// Every path from one source must draw the same sizes.
fn check_paths(leaves: &BTreeMap<char, f64>, parsed: &ConvertResult, source: &str, from: Format) {
    let svg = |result: ConvertResult| compact_svg(result.compact_expr());
    let heap = render_to_svg(&parsed.expr(), &RenderOptions::default()).unwrap();
    check(
        leaves,
        &compact_svg(parsed.compact_expr()),
        "direct",
        source,
    );
    check(leaves, &heap, "heap", source);
    let latex = parsed.to_latex().unwrap();
    let reread = svg(from_latex(&latex).unwrap());
    check(leaves, &reread, "latex", &format!("{source}\n{latex}"));
    let mathml = parsed.to_mathml().unwrap();
    let reread = svg(from_mathml(&mathml).unwrap());
    check(leaves, &reread, "mathml", &format!("{source}\n{mathml}"));
    // `convert` normalizes.
    let latex = convert(source, from, Format::LaTeX).unwrap();
    let reread = svg(from_latex(&latex).unwrap());
    check(
        leaves,
        &reread,
        "convert latex",
        &format!("{source}\n{latex}"),
    );
    let mathml = convert(source, from, Format::MathML).unwrap();
    let reread = svg(from_mathml(&mathml).unwrap());
    check(
        leaves,
        &reread,
        "convert mathml",
        &format!("{source}\n{mathml}"),
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    #[test]
    fn every_path_draws_the_oracle_sizes(trees in trees()) {
        let mut spelling = Spelling::default();
        let (mathml, latex) = spelling.list(&trees, 1.0, 0);
        let mathml = format!(r#"<math display="inline">{mathml}</math>"#);
        let parsed = from_mathml(&mathml).unwrap();
        check_paths(&spelling.leaves, &parsed, &mathml, Format::MathML);
        let parsed = from_latex(&latex).unwrap();
        check_paths(&spelling.leaves, &parsed, &latex, Format::LaTeX);
    }
}

#[test]
fn a_declaration_after_a_big_operator_is_no_body() {
    let sizes = |pairs: &[(char, f64)]| pairs.iter().copied().collect::<BTreeMap<_, _>>();
    for (mathml, latex, leaves) in [
        (
            r#"<math><mo>∑</mo><mstyle mathsize="90%"><mtext>s</mtext></mstyle></math>"#,
            r"\sum{}\small \text{s}",
            sizes(&[('s', 0.9)]),
        ),
        (
            r#"<math><mstyle mathsize="144%"><mi>r</mi><mo>∑</mo></mstyle><mi>e</mi></math>"#,
            r"\Large r\sum{}\normalsize e",
            sizes(&[('r', 1.44), ('e', 1.0)]),
        ),
        (
            r#"<math><mo>∫</mo><mo>∫</mo><mstyle mathsize="50%"><mi>e</mi></mstyle></math>"#,
            r"\int {\int}\tiny e",
            sizes(&[('e', 0.5)]),
        ),
    ] {
        let parsed = from_mathml(mathml).unwrap();
        assert_eq!(parsed.to_latex().unwrap(), latex);
        check_paths(&leaves, &parsed, mathml, Format::MathML);
    }
    for (latex, leaves) in [
        (r"\int \large a b", sizes(&[('a', 1.2), ('b', 1.2)])),
        (
            r"\sum_{k} \scriptstyle a b",
            sizes(&[('a', 0.7), ('b', 0.7)]),
        ),
        (r"\int \int \tiny a", sizes(&[('a', 0.5)])),
        (
            r"\mathbf{\scriptstyle \begin{matrix}x\end{matrix}b}c",
            sizes(&[('b', 0.7), ('c', 1.0)]),
        ),
    ] {
        let parsed = from_latex(latex).unwrap();
        check_paths(&leaves, &parsed, latex, Format::LaTeX);
    }
}
