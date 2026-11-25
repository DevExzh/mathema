//! What typesetting attributes do where nodes are read, joined, replaced and
//! dropped. Each test states a value worked out by hand.

use mathema::ast::compact::CompactExpr;
use mathema::ast::normalize::{normalize_compact_expr, normalize_expr};
use mathema::ast::{
    ColumnAlign, FontSize, LimitStyle, NodeAttrs, Rational, SymbolKind, TextStyle, TypeSize,
};
use mathema::{
    Config, Format, MathExpr, MathNode, MtefVersion, MtefWriteOptions, Strictness,
    TypesettingPolicy, convert, from_mathml, from_mtef_bytes, from_mtef_bytes_with, from_omml,
};

fn points(num: i32, den: u32) -> NodeAttrs {
    NodeAttrs::with_size(FontSize::Points(Rational::new(num, den)))
}

fn logical(level: TypeSize) -> NodeAttrs {
    NodeAttrs::with_size(FontSize::Logical {
        level,
        delta_pt: Rational::zero(),
    })
}

fn sized(attrs: NodeAttrs, content: MathNode) -> MathNode {
    MathNode::Attributed {
        attrs,
        content: Box::new(content),
    }
}

fn id(name: &str) -> MathNode {
    MathNode::Identifier(name.into())
}

fn row(nodes: Vec<MathNode>) -> Box<MathNode> {
    Box::new(MathNode::Row(nodes))
}

fn sub(base: MathNode, sub: MathNode) -> MathNode {
    MathNode::Subscript {
        base: Box::new(base),
        sub: Box::new(sub),
    }
}

fn compact(nodes: Vec<MathNode>) -> CompactExpr {
    CompactExpr::from_math_expr(&MathExpr::block(nodes)).unwrap()
}

/// What normalization makes of the nodes, the same on both routes and again.
#[track_caller]
fn normalized(nodes: Vec<MathNode>) -> Vec<MathNode> {
    let mut expr = compact(nodes.clone());
    normalize_compact_expr(&mut expr).unwrap();
    let mut heap = MathExpr::block(nodes);
    normalize_expr(&mut heap);
    assert_eq!(heap, expr.to_math_expr());
    let once = heap.clone();
    normalize_expr(&mut heap);
    assert_eq!(heap, once);
    normalize_compact_expr(&mut expr).unwrap();
    assert_eq!(expr.to_math_expr(), once);
    once.nodes
}

/// An MTEF 3 equation with a FULL record.
fn mtef3(content: &[u8]) -> Vec<u8> {
    [&[3, 1, 1, 3, 10, 1][..], content, &[0, 0]].concat()
}

/// The MTEF 3 SIZE record of a size in points.
fn size3(points: i16) -> Vec<u8> {
    [&[9, 101][..], &(points * 32).to_le_bytes()].concat()
}

fn nodes(bytes: &[u8]) -> Vec<MathNode> {
    from_mtef_bytes(bytes).unwrap().expr().nodes
}

/// A typesize record is the short way of specifying a SIZE record with nothing
/// added (MTEFv5.md "Typesize records"): they say the same of a script.
#[test]
fn size_records_and_typesize_records_are_alike() {
    let script = |record: &[u8]| {
        let content = [&[2, 131, b'x', 0, 3, 15, 1, 0, 1][..], record].concat();
        nodes(&mtef3(
            &[&content[..], &[2, 131, b'n', 0, 0, 0x11, 0]].concat(),
        ))
    };
    let by_place = vec![sub(id("x"), id("n"))];
    let full = vec![sub(id("x"), sized(logical(TypeSize::Full), id("n")))];
    assert_eq!(script(&[]), by_place);
    for record in [&[10][..], &[9, 100, 0, 0, 0], &[9, 0, 128]] {
        assert_eq!(script(record), full, "{record:?}");
    }
    // At the second script size, which it has not by its place.
    let second = vec![sub(id("x"), sized(logical(TypeSize::Sub2), id("n")))];
    for record in [&[12][..], &[9, 100, 2, 0, 0], &[9, 2, 128]] {
        assert_eq!(script(record), second, "{record:?}");
        // So full size is one elsewhere.
        let first = [2, 131, b'x', 0, 3, 15, 1, 0, 1, 2, 131, b'n', 0, 0, 0x11, 0];
        let next = [&[2, 131, b'y', 0, 3, 15, 1, 0, 1][..], record].concat();
        let next = [&next[..], &[2, 131, b'm', 0, 0, 0x11, 0]].concat();
        let both = nodes(&mtef3(&[&first[..], &next].concat()));
        assert_eq!(both[0], full[0], "{record:?}");
    }
    for record in [&[11][..], &[9, 100, 1, 0, 0], &[9, 1, 128]] {
        assert_eq!(script(record), by_place, "{record:?}");
        // With it the equation has script sizes: full size is one elsewhere.
        let second = [&[2, 131, b'y', 0, 3, 15, 1, 0, 1][..], record].concat();
        let second = [&second[..], &[2, 131, b'm', 0, 0, 0x11, 0]].concat();
        let first = [2, 131, b'x', 0, 3, 15, 1, 0, 1, 2, 131, b'n', 0, 0, 0x11, 0];
        assert_eq!(
            nodes(&mtef3(&[&first[..], &second].concat())),
            [full[0].clone(), sub(id("y"), id("m"))],
            "{record:?}"
        );
    }
    // MTEF 5, from the review: every `N` is at the size the records say.
    let hex = "050100060944534d543600011000e80300000000100000000000e803120000024150f4150f00\
               0a010003001c0100010101000964010000020083004e090080020083014e00000000";
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let read = from_mtef_bytes_with(&bytes, &lenient).unwrap();
    let spelled: Vec<u8> = bytes.clone();
    let at = |pattern: &[u8]| spelled.windows(pattern.len()).position(|w| w == pattern);
    let mut short = bytes.clone();
    if let Some(i) = at(&[9, 0, 128]) {
        short.splice(i..i + 3, [10]);
    }
    if let Some(i) = at(&[9, 100, 1, 0, 0]) {
        short.splice(i..i + 5, [11]);
    }
    let short = from_mtef_bytes_with(&short, &lenient).unwrap();
    assert_eq!(read.expr(), short.expr());
}

/// "-point size (16 bit integer)" in the specification, positive in files.
#[test]
fn size_records_in_points_of_either_sign() {
    for points_ in [20, -20] {
        let content = [&size3(points_)[..], &[2, 131, b'x', 0]].concat();
        assert_eq!(nodes(&mtef3(&content)), [sized(points(20, 1), id("x"))]);
    }
}

/// A prime is a raised character in MTEF and a script in the AST, of 0.7.
#[test]
fn sized_primes_are_scripts() {
    let content = [
        &[2, 131, b'x', 0][..],
        &size3(20),
        &[2, 130, b'\'', 0, 2, 130, b'\'', 0],
    ]
    .concat();
    let MathNode::Superscript { sup, .. } = nodes(&mtef3(&content)).remove(0) else {
        panic!("{:?}", nodes(&mtef3(&content)));
    };
    let prime = || MathNode::Symbol(SymbolKind::Ordinary, "\u{2032}".into());
    assert_eq!(
        *sup,
        MathNode::Row(vec![
            sized(points(14, 1), prime()),
            sized(points(14, 1), prime())
        ])
    );
}

/// Blanks have no size that shows: none of their own, and they split no text.
#[test]
fn blanks_have_no_size() {
    let text = [
        &[2, 129, b'a', 0][..],
        &size3(20),
        &[2, 129, b' ', 0, 10, 2, 129, b'b', 0],
    ]
    .concat();
    assert_eq!(
        nodes(&mtef3(&text)),
        [MathNode::Text(TextStyle::Normal, "a b".into())]
    );
    // A blank before text at another size is of its run.
    let text = [&[2, 129, b' ', 0][..], &size3(20), &[2, 129, b'b', 0]].concat();
    assert_eq!(
        nodes(&mtef3(&text)),
        [sized(
            points(20, 1),
            MathNode::Text(TextStyle::Normal, " b".into())
        )]
    );
    // A thin space (MTEF 5, typeface 24).
    let content = [
        &[5, 1, 0, 6, 0][..],
        b"DSMT6\0\0",
        &[10, 1, 0, 9, 101, 128, 2, 2, 0, 0x98, 0x02, 0xEF, 0, 0],
    ]
    .concat();
    let read = from_mtef_bytes(&content).unwrap();
    assert!(!read.expr().nodes.is_empty());
    assert!(!read.compact_expr().arena.has_attrs(), "{:?}", read.expr());
}

/// A function name is one whatever the sizes of its letters, which are dropped
/// and reported.
#[test]
fn a_function_name_of_several_sizes() {
    let content = [
        &size3(20)[..],
        &[2, 130, b's', 0, 10, 2, 130, b'i', 0, 2, 130, b'n', 0],
    ]
    .concat();
    let read = from_mtef_bytes(&mtef3(&content)).unwrap();
    assert!(
        matches!(&read.expr().nodes[..], [MathNode::Operator(op)] if op.name == "sin"),
        "{:?}",
        read.expr()
    );
    let reported = read.diagnostics().iter();
    let reported = reported.filter(|d| d.message.contains("function name"));
    assert_eq!(reported.count(), 1, "{:?}", read.diagnostics());
}

/// The sign in the operator slot of SUMOP: 36 pt there is 1.5 times the size
/// of text, so 24. The node it was has no size.
#[test]
fn a_sized_sign_in_an_operator_slot() {
    let content = [
        &[
            3, 43, 1, 0, 1, 2, 131, b'x', 0, 0, 11, 1, 2, 131, b'i', 0, 0, 0x11,
        ][..],
        &size3(36),
        &[1, 2, 134, 0x11, 0x22, 0, 0],
    ]
    .concat();
    let nodes = nodes(&mtef3(&content));
    let [MathNode::Attributed { attrs, content }] = &nodes[..] else {
        panic!("{nodes:?}");
    };
    assert!(matches!(**content, MathNode::Nary { .. }), "{nodes:?}");
    assert_eq!(
        *attrs,
        NodeAttrs::with_glyph_size(FontSize::Points(Rational::new(24, 1)))
    );
}

/// A tilde alone over a letter, typed with the limit template: 21 pt in the
/// slot of a 0.7 limit is the mark at 30 for text.
#[test]
fn a_sized_mark_in_a_limit_slot() {
    let content = [
        &[3, 39, 0, 0, 1, 2, 131, b'x', 0, 0, 0x11, 1][..],
        &size3(21),
        &[2, 134, b'~', 0, 0, 0],
    ]
    .concat();
    let nodes = nodes(&mtef3(&content));
    let text = format!("{nodes:?}");
    assert!(text.contains("Accent"), "{text}");
    assert!(
        text.contains("glyph_size: Some(Points(Rational { num: 30, den: 1 }))"),
        "{text}"
    );
}

/// User 1 is 75 % of the full size by default: preferences that say so say
/// nothing.
#[test]
fn preferences_at_their_defaults_are_no_sizes() {
    let source =
        r#"<math mathsize="12pt"><mi mathsize="9pt" data-mathema-size="user1">x</mi></math>"#;
    let bytes = from_mathml(source)
        .unwrap()
        .to_mtef_bytes(&MtefWriteOptions::new(MtefVersion::V5))
        .unwrap();
    assert_eq!(nodes(&bytes), [sized(logical(TypeSize::User1), id("x"))]);
}

/// A script that goes leaves its size on the base that stays.
#[test]
fn a_node_that_takes_the_place_of_another_takes_its_size() {
    let node = sized(points(20, 1), sub(id("y"), MathNode::Row(vec![])));
    assert_eq!(normalized(vec![node]), [sized(points(20, 1), id("y"))]);
    // The inner size wins, and one of them is left.
    let inner = sized(points(10, 1), id("y"));
    let node = sized(points(20, 1), sub(inner.clone(), MathNode::Row(vec![])));
    assert_eq!(normalized(vec![node]), std::slice::from_ref(&inner));
    let node = sized(points(20, 1), inner.clone());
    assert_eq!(normalized(vec![node]), [inner]);
    // A list that stays of a scaled one has no size: its nodes have.
    let list = MathNode::Scaled {
        factor: Rational::new(2, 1),
        content: row(vec![id("\u{3b1}"), id("\u{3b2}")]),
    };
    let mut expr = compact(vec![sized(points(20, 1), list), id("x")]);
    normalize_compact_expr(&mut expr).unwrap();
    assert!(format!("{:?}", expr.to_math_expr()).contains("Attributed"));
    // A list has no size: its nodes have.
    let list = sub(
        *row(vec![id("\u{3b1}"), id("\u{3b2}")]),
        MathNode::Row(vec![]),
    );
    assert_eq!(
        normalized(vec![sized(points(20, 1), list)]),
        [
            sized(points(20, 1), id("\u{3b1}")),
            sized(points(20, 1), id("\u{3b2}"))
        ]
    );
}

/// Neighbours are joined if they have one size, or none.
#[test]
fn sizes_keep_neighbours_apart() {
    let number = |text: &str| MathNode::Number(text.into());
    let text = |text: &str| MathNode::Text(TextStyle::Normal, text.into());
    for (first, second, both) in [
        (number("1"), number("2"), number("12")),
        (text("a"), text("b"), text("ab")),
    ] {
        let apart = vec![sized(points(20, 1), first.clone()), second.clone()];
        assert_eq!(normalized(apart.clone()), apart);
        let joined = [first, second].map(|node| sized(points(20, 1), node));
        assert_eq!(normalized(joined.to_vec()), [sized(points(20, 1), both)]);
    }
}

/// The scripts of a sized script on nothing keep its size on another base.
#[test]
fn scripts_on_nothing_take_their_size_along() {
    let script = |content| sized(points(20, 1), sub(MathNode::Row(vec![]), content));
    let MathNode::PreScripts { base, pre_sub, .. } =
        normalized(vec![script(id("i")), id("x")]).remove(0)
    else {
        panic!();
    };
    assert_eq!(*base, id("x"));
    assert_eq!(pre_sub.as_deref(), Some(&sized(points(20, 1), id("i"))));
    let sum = MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into());
    let nodes = normalized(vec![sum.clone(), script(id("i"))]);
    assert_eq!(nodes, [sub(sum, sized(points(20, 1), id("i")))]);
}

/// Sizes go into lists and table cells, signs' sizes do not, and what draws
/// nothing has none.
#[test]
fn what_a_size_around_a_list_is_given_to() {
    let both = NodeAttrs::with_size(FontSize::Points(Rational::new(20, 1))).or(
        &NodeAttrs::with_glyph_size(FontSize::Points(Rational::new(30, 1))),
    );
    let list = sized(both, *row(vec![id("x"), id("y")]));
    let expected = [sized(points(20, 1), id("x")), sized(points(20, 1), id("y"))];
    assert_eq!(
        compact(vec![list]).to_math_expr().nodes[0],
        MathNode::Row(expected.to_vec())
    );
    let list = *row(vec![MathNode::StyleSwitch(2), id("x")]);
    let given = compact(vec![sized(points(20, 1), list)])
        .to_math_expr()
        .nodes;
    assert_eq!(
        given,
        [*row(vec![
            MathNode::StyleSwitch(2),
            sized(points(20, 1), id("x"))
        ])]
    );
    let cell = MathNode::AlignedCell {
        alignment: ColumnAlign::Left,
        content: Box::new(id("x")),
    };
    let table = MathNode::Array {
        columns: vec![ColumnAlign::Center],
        rows: vec![vec![vec![sized(points(20, 1), cell)]]],
        arraystretch: None,
        add_jot: false,
    };
    let given = compact(vec![table]).to_math_expr().nodes;
    let MathNode::Array { rows, .. } = &given[0] else {
        panic!("{given:?}");
    };
    let MathNode::AlignedCell { content, .. } = &rows[0][0][0] else {
        panic!("{rows:?}");
    };
    assert_eq!(**content, sized(points(20, 1), id("x")));
}

/// Sizes directly in sizes count against the depth limit, and are refused
/// before they overflow the stack.
#[test]
fn nested_attributes_are_a_depth() {
    let nest = |depth: usize| (0..depth).fold(id("x"), |node, _| sized(points(20, 1), node));
    let valid = |depth| mathema::ast::validate_ast_depth(&MathExpr::block(vec![nest(depth)]));
    assert!(valid(3).is_ok());
    assert!(valid(200).is_err());
    // Normalization leaves what is too deep.
    for depth in [200, 1000] {
        let mut deep = MathExpr::block(vec![nest(depth)]);
        normalize_expr(&mut deep);
    }
    // Nor is a compact tree gone through that is too deep: its sizes stay.
    let deep = (0..400).fold(sized(points(20, 1), id("x")), |node, _| {
        MathNode::Sqrt(Box::new(node))
    });
    let mut expr = compact(vec![deep]);
    let _ = normalize_compact_expr(&mut expr);
    assert!(expr.arena.has_attrs());
}

/// Trees too deep for recursion are not searched for attributes.
#[test]
fn very_deep_trees_are_not_searched() {
    use mathema::ast::compact::CompactNode;
    const DEPTH: usize = 400_000;
    let search = || {
        let deep = (0..DEPTH).fold(id("x"), |node, _| MathNode::Sqrt(Box::new(node)));
        let mut expr = MathExpr::block(vec![deep]);
        normalize_expr(&mut expr);
        // (Dropping it would go as deep.)
        std::mem::forget(expr);
        let mut expr = compact(vec![sized(points(20, 1), id("y"))]);
        let mut node = expr.arena.children(expr.roots)[0];
        for _ in 0..DEPTH {
            node = expr.arena.push_node(CompactNode::Sqrt(node));
        }
        expr.roots = expr.arena.push_children(&[node]).unwrap();
        let _ = normalize_compact_expr(&mut expr);
        assert!(expr.arena.has_attrs());
    };
    let thread = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(search);
    thread.unwrap().join().unwrap();
}

/// What was dropped in lenient reading has no size to report, and writes like
/// the tree built from the heap.
#[test]
fn dropped_nodes_have_no_sizes() {
    // A size on content that the parser gives up.
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let source = r#"<math><mi>x</mi><mi mathsize="20pt">y</mi><mfrac><mn>1</mn></math>"#;
    let read = mathema::from_mathml_with(source, &lenient).unwrap();
    let sizes = |expr: &MathExpr| format!("{expr:?}").contains("Attributed");
    assert_eq!(read.compact_expr().arena.has_attrs(), sizes(&read.expr()));
    for source in [
        r#"<mfrac><mn>1</mn><mn>2</mn><mi mathsize="20pt">y</mi></mfrac>"#,
        r#"<msub><mi>x</mi><mi>a</mi><mi mathsize="20pt">y</mi></msub>"#,
    ] {
        let source = format!("<math>{source}</math>");
        let read = mathema::from_mathml_with(&source, &lenient).unwrap();
        assert!(!sizes(&read.expr()));
        assert!(!read.compact_expr().arena.has_attrs(), "{source}");
        assert!(read.to_omml_with_diagnostics().unwrap().1.is_empty());
    }
    // A hidden limit in OMML, in an equation and in a paragraph.
    let nary = r#"<m:nary><m:naryPr><m:subHide m:val="1"/><m:supHide m:val="1"/></m:naryPr><m:sub><m:r><w:rPr><w:sz w:val="40"/></w:rPr><m:t>q</m:t></m:r></m:sub><m:sup/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary>"#;
    let namespaces = r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let read = from_omml(&format!("<m:oMath {namespaces}>{nary}</m:oMath>")).unwrap();
    assert!(!sizes(&read.expr()));
    assert!(!read.compact_expr().arena.has_attrs());
    let paragraph = format!("<m:oMathPara {namespaces}><m:oMath>{nary}</m:oMath></m:oMathPara>");
    let paragraph =
        mathema::reader::omml::OmmlReader::parse_paragraph(&paragraph, &Config::default()).unwrap();
    assert!(!paragraph.equations[0].arena.has_attrs());
    // Nor a size of the text around it, which the preferences give: a script
    // among the slots of a fraction is dropped.
    let source = r#"<math mathsize="14pt"><mi mathsize="20pt">q</mi></math>"#;
    let options = MtefWriteOptions::new(MtefVersion::V5);
    let bytes = from_mathml(source)
        .unwrap()
        .to_mtef_bytes(&options)
        .unwrap();
    let at = bytes.windows(3).rposition(|w| w == [1, 0, 9]).unwrap();
    let slot = |content: &[u8]| [&[1, 0][..], content, &[0]].concat();
    let letter = |c| [2, 0, 131, c, 0];
    let fraction = [
        &[10, 1, 0, 3, 0, 11, 0, 0][..],
        &slot(&letter(b'a')),
        &slot(&letter(b'b')),
        &[3, 0, 27, 0, 0],
        &slot(&[&[9, 101, 128, 2][..], &letter(b'c')].concat()),
        &[1, 1, 0],
        &[0, 0, 0],
    ]
    .concat();
    let read = from_mtef_bytes_with(&[&bytes[..at], &fraction].concat(), &lenient).unwrap();
    assert!(
        matches!(read.expr().nodes[..], [MathNode::Fraction { .. }]),
        "{:?}",
        read.expr()
    );
    assert!(!read.compact_expr().arena.has_attrs());
    assert_eq!(read.expr().meta, Default::default());
    // Normalization drops a sized node too.
    let nothing = sized(points(20, 1), MathNode::Space(Rational::zero()));
    let mut expr = compact(vec![id("x"), nothing]);
    assert!(expr.arena.has_attrs());
    normalize_compact_expr(&mut expr).unwrap();
    assert_eq!(expr.to_math_expr().nodes, [id("x")]);
    assert!(!expr.arena.has_attrs());
}

/// `convert` drops what LaTeX cannot declare before it joins text, and then
/// declares sizes for lists as `to_latex` does.
#[test]
fn latex_sizes_through_convert() {
    let source = r#"<math><msup><mi>x</mi><mrow><mtext mathsize="13pt">a</mtext><mtext>b</mtext></mrow></msup></math>"#;
    let latex = convert(source, Format::MathML, Format::LaTeX).unwrap();
    assert!(latex.contains(r"\text{ab}"), "{latex}");
    // An overbar over a slot, whose list is no group in LaTeX.
    let content = [&[3, 17, 0, 0, 1][..], &size3(20), &[2, 131, b'b', 0, 0, 0]].concat();
    let bytes = mtef3(&content);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        convert(&hex, Format::Mtef, Format::LaTeX).unwrap(),
        from_mtef_bytes(&bytes).unwrap().to_latex().unwrap()
    );
    // A group of MathML is one in LaTeX, as on the other route.
    let source = r#"<math><mo>∑</mo><mrow><mi>x</mi><mi mathsize="18pt">y</mi></mrow><mo>+</mo><mi>z</mi></math>"#;
    assert_eq!(
        convert(source, Format::MathML, Format::LaTeX).unwrap(),
        from_mathml(source).unwrap().to_latex().unwrap()
    );
    let prime = [2, 130, b'\'', 0];
    let content = [
        &[2, 131, b'B', 0, 3, 15, 1, 0, 0x11, 1, 2, 131, b'n', 0, 0, 0][..],
        &[9, 100, 0, 0, 0],
        &prime,
        &prime,
    ]
    .concat();
    let bytes = mtef3(&content);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        convert(&hex, Format::Mtef, Format::LaTeX).unwrap(),
        from_mtef_bytes(&bytes).unwrap().to_latex().unwrap()
    );
    // Full size in a script, which \textstyle declares, with two primes.
    let script = [
        &[2, 131, b'x', 0, 3, 15, 1, 0, 1, 11, 2, 131, b'y', 0, 10][..],
        &prime,
        &prime,
        &[0, 0x11, 0],
    ]
    .concat();
    let bytes = mtef3(&script);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        convert(&hex, Format::Mtef, Format::LaTeX).unwrap(),
        from_mtef_bytes(&bytes).unwrap().to_latex().unwrap()
    );
}

/// Digits of several sizes are one number in pure OMML, as without sizes.
#[test]
fn omml_drops_sizes_before_joining() {
    let digits = [
        &size3(9)[..],
        &[2, 136, b'3', 0],
        &size3(10),
        &[2, 136, b'4', 0],
    ]
    .concat();
    let read = from_mtef_bytes(&mtef3(&digits)).unwrap();
    assert!(read.compact_expr().arena.has_attrs());
    for omml in [
        read.to_omml().unwrap(),
        read.to_omml_with_diagnostics().unwrap().0,
        read.to_omml_with_options(&Default::default()).unwrap().0,
    ] {
        assert!(omml.contains(">34<"), "{omml}");
    }
}

/// The size of ordinary text of one equation is not the next one's.
#[test]
fn equations_of_a_paragraph_have_their_own_sizes() {
    use mathema::reader::omml::OmmlReader;
    use mathema::writer::omml::{OmmlOptions, OmmlRunProperties, OmmlWriter};
    let source = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:oMath><m:r><w:rPr><w:sz w:val="40"/></w:rPr><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#;
    let config = Config::default();
    let paragraph = OmmlReader::parse_paragraph(source, &config).unwrap();
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl);
    let (written, _) = OmmlWriter::write_paragraph(&paragraph, &config, &options).unwrap();
    assert_eq!(written.matches("<w:sz").count(), 1, "{written}");
}

/// The nearest size command is within 10 % or there is none: 36 pt is 3 times
/// the 12 pt of text, `\Huge` 2.488 times.
#[test]
fn latex_sizes_are_within_ten_percent() {
    for (size, command) in [("30pt", Some(r"\Huge")), ("36pt", None)] {
        let source = format!(r#"<math><mi mathsize="{size}">x</mi><mi>y</mi></math>"#);
        let (latex, diagnostics) = from_mathml(&source)
            .unwrap()
            .to_latex_with_diagnostics()
            .unwrap();
        assert_eq!(latex.contains(r"\Huge"), command.is_some(), "{latex}");
        assert_eq!(diagnostics.is_empty(), command.is_some(), "{diagnostics:?}");
    }
}

/// A named operator with side scripts is another node than its run: the size
/// goes along.
#[test]
fn a_sized_operator_name_with_side_scripts_in_omml() {
    let run = |text: &str, size: &str| {
        format!(
            r#"<m:r><m:rPr><m:sty m:val="p"/></m:rPr><w:rPr>{size}</w:rPr><m:t>{text}</m:t></m:r>"#
        )
    };
    let omml = format!(
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub></m:oMath>"#,
        run("lim", r#"<w:sz w:val="40"/>"#),
        run("n", "")
    );
    let nodes = from_omml(&omml).unwrap().expr().nodes;
    let text = format!("{nodes:?}");
    let [MathNode::Subscript { base, .. }] = &nodes[..] else {
        panic!("{text}");
    };
    assert_eq!(
        **base,
        sized(
            points(20, 1),
            MathNode::Operator(mathema::ast::OperatorAttr {
                name: "lim".into(),
                limit_style: LimitStyle::SubSup,
            })
        )
    );
}

#[test]
fn sizes_are_semantic_by_default() {
    assert_eq!(Config::default().typesetting, TypesettingPolicy::Semantic);
}
