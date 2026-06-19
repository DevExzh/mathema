use super::*;

#[test]
fn invisible_operators_are_zero_width_kerns() {
    for ch in ['\u{2061}', '\u{2062}', '\u{2063}', '\u{2064}', '\u{200B}'] {
        assert!(is_invisible_char(ch), "{ch:?} should be invisible");
        let g = LayoutBox::glyph(ch, 1.0, false);
        assert_eq!(g.width, 0.0, "{ch:?} must have zero width");
        assert!(
            matches!(g.kind, BoxKind::Kern),
            "{ch:?} must not draw a glyph"
        );
    }
    assert!(!is_invisible_char('x'));
}

fn default_options() -> RenderOptions {
    RenderOptions::default()
}

#[test]
fn layout_simple_number() {
    let expr = MathExpr::inline(vec![MathNode::Number("42".into())]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_fraction_has_depth() {
    let expr = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Number("1".into())),
        den: Box::new(MathNode::Number("2".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Display, &default_options());
    assert!(root.height > 0.0);
    assert!(root.depth > 0.0);
}

#[test]
fn layout_superscript_raises() {
    let expr = MathExpr::inline(vec![MathNode::Superscript {
        base: Box::new(MathNode::Identifier("x".into())),
        sup: Box::new(MathNode::Number("2".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // Width should be base + sup
    assert!(root.width > 0.5);
}

#[test]
fn empty_row_has_zero_width() {
    let expr = MathExpr::inline(vec![]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert_eq!(root.width, 0.0);
}

#[test]
fn layout_subscript_lowers() {
    let expr = MathExpr::inline(vec![MathNode::Subscript {
        base: Box::new(MathNode::Identifier("x".into())),
        sub: Box::new(MathNode::Identifier("i".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.5);
    assert!(root.depth > 0.0, "subscript should add depth");
}

#[test]
fn layout_subsup_combined() {
    let expr = MathExpr::inline(vec![MathNode::SubSup {
        base: Box::new(MathNode::Identifier("x".into())),
        sub: Box::new(MathNode::Identifier("i".into())),
        sup: Box::new(MathNode::Number("2".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.5);
    assert!(root.height > 0.0);
    assert!(root.depth > 0.0, "subsup should have both height and depth");
}

#[test]
fn layout_fraction_display_wider_than_text() {
    let frac = vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Number("1".into())),
        den: Box::new(MathNode::Number("2".into())),
    }];
    let expr_d = MathExpr::inline(frac.clone());
    let expr_t = MathExpr::inline(frac);
    let opts = default_options();
    let display = layout_expr(&expr_d, MathStyle::Display, &opts);
    let text = layout_expr(&expr_t, MathStyle::Text, &opts);
    // Display fractions have larger numerator/denominator spacing
    assert!(
        display.height + display.depth >= text.height + text.depth,
        "display fraction should be at least as tall as text fraction"
    );
}

#[test]
fn layout_sqrt_has_positive_dimensions() {
    let expr = MathExpr::inline(vec![MathNode::Sqrt(Box::new(MathNode::Identifier(
        "x".into(),
    )))]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_sqrt_with_index() {
    let expr = MathExpr::inline(vec![MathNode::Root {
        index: Box::new(MathNode::Number("3".into())),
        radicand: Box::new(MathNode::Identifier("x".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_fenced_wider_than_content() {
    let content_nodes = vec![MathNode::Identifier("x".into())];
    let content_expr = MathExpr::inline(content_nodes.clone());
    let content_only = layout_expr(&content_expr, MathStyle::Text, &default_options());

    let fenced_nodes = vec![MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
    }];
    let fenced_expr = MathExpr::inline(fenced_nodes);
    let fenced = layout_expr(&fenced_expr, MathStyle::Text, &default_options());

    assert!(
        fenced.width > content_only.width,
        "fenced expression should be wider than content alone (delimiters add width)"
    );
}

#[test]
fn layout_fenced_has_interatom_spacing() {
    // \left(x + y\right) — content has three items with binary op spacing
    let fenced_spaced = MathExpr::inline(vec![MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![
            FencedItem::Element(MathNode::Identifier("x".into())),
            FencedItem::Element(MathNode::Symbol(SymbolKind::BinaryOp, "+".into())),
            FencedItem::Element(MathNode::Identifier("y".into())),
        ],
    }]);
    let root_spaced = layout_expr(&fenced_spaced, MathStyle::Text, &default_options());

    // \left(xy\right) — content has two identifiers, no binary op spacing
    let fenced_noop = MathExpr::inline(vec![MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![
            FencedItem::Element(MathNode::Identifier("x".into())),
            FencedItem::Element(MathNode::Identifier("y".into())),
        ],
    }]);
    let root_noop = layout_expr(&fenced_noop, MathStyle::Text, &default_options());

    assert!(
        root_spaced.width > root_noop.width,
        "\\left(x + y\\right) should be wider than \\left(xy\\right) due to inter-atom spacing \
             (spaced: {}, unspaced: {})",
        root_spaced.width,
        root_noop.width
    );
}

#[test]
fn layout_array_grid() {
    let expr = MathExpr::inline(vec![MathNode::Array {
        rows: vec![
            vec![
                vec![MathNode::Identifier("a".into())],
                vec![MathNode::Identifier("b".into())],
            ],
            vec![
                vec![MathNode::Identifier("c".into())],
                vec![MathNode::Identifier("d".into())],
            ],
        ],
        columns: vec![ColumnAlign::Center, ColumnAlign::Center],
        arraystretch: None,
        add_jot: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_nary_display_has_limits() {
    let expr = MathExpr::inline(vec![MathNode::Nary {
        op: '∑',
        sub: Some(Box::new(MathNode::Identifier("i".into()))),
        sup: Some(Box::new(MathNode::Identifier("n".into()))),
        body: Box::new(MathNode::Identifier("x".into())),
        limits: LimitStyle::Default,
    }]);
    let root = layout_expr(&expr, MathStyle::Display, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_accent_over_base() {
    let expr = MathExpr::inline(vec![MathNode::Accent {
        base: Box::new(MathNode::Identifier("x".into())),
        accent: '\u{0302}', // combining circumflex
        position: AccentPosition::Above,
        wide: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn accent_skew_nonzero_for_italic_f() {
    // 'f' has significant italic correction in math italic fonts.
    let base = MathNode::Identifier("f".into());
    let skew = accent_skew(&base, 1.0);
    assert!(skew > 0.0, "f should have positive accent skew, got {skew}");
}

#[test]
fn accent_skew_zero_for_multi_char() {
    // Multi-character identifiers should have no skew correction.
    let base = MathNode::Identifier("abc".into());
    assert_eq!(accent_skew(&base, 1.0), 0.0);
}

#[test]
fn accent_skew_zero_for_non_identifier() {
    // Non-identifier nodes should have no skew correction.
    let base = MathNode::Number("2".into());
    assert_eq!(accent_skew(&base, 1.0), 0.0);
}

#[test]
fn layout_overscript() {
    let expr = MathExpr::inline(vec![MathNode::Overscript {
        base: Box::new(MathNode::Identifier("x".into())),
        over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "→".into())),
        movable_limits: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_underscript() {
    let expr = MathExpr::inline(vec![MathNode::Underscript {
        base: Box::new(MathNode::Identifier("x".into())),
        under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "_".into())),
        movable_limits: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
}

#[test]
fn layout_underover() {
    let expr = MathExpr::inline(vec![MathNode::UnderOver {
        base: Box::new(MathNode::Identifier("x".into())),
        under: Box::new(MathNode::Number("1".into())),
        over: Box::new(MathNode::Number("2".into())),
        movable_limits: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
    assert!(root.depth > 0.0);
}

#[test]
fn layout_phantom_has_dimensions_but_invisible() {
    let content = MathNode::Identifier("x".into());
    let visible = layout_node(
        &content,
        &LayoutCtx {
            style: MathStyle::Text,
            params: TexParams::default(),
            options: &default_options(),
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        },
    );
    let expr = MathExpr::inline(vec![MathNode::Phantom(
        Box::new(content.clone()),
        PhantomKind::Full,
    )]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(
        (root.width - visible.width).abs() < 0.01,
        "phantom should have same width as visible content"
    );
    if let BoxKind::HBox { children } = &root.kind {
        let phantom_kind = &children[0].layout.kind;
        assert!(matches!(phantom_kind, BoxKind::Phantom));
    }
}

#[test]
fn layout_vphantom_has_zero_width() {
    let content = MathNode::Identifier("x".into());
    let visible = layout_node(
        &content,
        &LayoutCtx {
            style: MathStyle::Text,
            params: TexParams::default(),
            options: &default_options(),
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        },
    );
    let expr = MathExpr::inline(vec![MathNode::Phantom(
        Box::new(content.clone()),
        PhantomKind::Vertical,
    )]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // vphantom: zero width, preserve height/depth
    if let BoxKind::HBox { children } = &root.kind {
        let phantom = &children[0].layout;
        assert!(
            phantom.width < 0.01,
            "vphantom should have zero width, got {}",
            phantom.width
        );
        assert!(
            (phantom.height - visible.height).abs() < 0.01,
            "vphantom should preserve height"
        );
        assert!(
            (phantom.depth - visible.depth).abs() < 0.01,
            "vphantom should preserve depth"
        );
    }
}

#[test]
fn layout_hphantom_has_zero_height_depth() {
    let content = MathNode::Identifier("x".into());
    let visible = layout_node(
        &content,
        &LayoutCtx {
            style: MathStyle::Text,
            params: TexParams::default(),
            options: &default_options(),
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        },
    );
    let expr = MathExpr::inline(vec![MathNode::Phantom(
        Box::new(content.clone()),
        PhantomKind::Horizontal,
    )]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // hphantom: preserve width, zero height/depth
    if let BoxKind::HBox { children } = &root.kind {
        let phantom = &children[0].layout;
        assert!(
            (phantom.width - visible.width).abs() < 0.01,
            "hphantom should preserve width"
        );
        assert!(
            phantom.height < 0.01,
            "hphantom should have zero height, got {}",
            phantom.height
        );
        assert!(
            phantom.depth < 0.01,
            "hphantom should have zero depth, got {}",
            phantom.depth
        );
    }
}

#[test]
fn layout_smash_renders_with_zero_height_depth() {
    let expr = MathExpr::inline(vec![MathNode::Phantom(
        Box::new(MathNode::Identifier("X".into())),
        PhantomKind::SmashBoth,
    )]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // smash: visible (not BoxKind::Phantom) but with zero height/depth
    if let BoxKind::HBox { children } = &root.kind {
        let smash = &children[0].layout;
        assert!(
            smash.height < 0.01,
            "smash should have zero height, got {}",
            smash.height
        );
        assert!(
            smash.depth < 0.01,
            "smash should have zero depth, got {}",
            smash.depth
        );
        // smash should be visible (HBox, not Phantom)
        assert!(
            !matches!(smash.kind, BoxKind::Phantom),
            "smash content should be visible"
        );
    }
}

#[test]
fn layout_boxed_adds_padding() {
    let content = MathNode::Identifier("x".into());
    let inner = layout_node(
        &content,
        &LayoutCtx {
            style: MathStyle::Text,
            params: TexParams::default(),
            options: &default_options(),
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        },
    );
    let expr = MathExpr::inline(vec![MathNode::Boxed {
        content: Box::new(content.clone()),
        bg_color: None,
        border_color: None,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // Boxed should be wider than inner due to padding
    assert!(
        root.width > inner.width,
        "boxed should add padding to width"
    );
}

#[test]
fn layout_cancel_preserves_dimensions() {
    let expr = MathExpr::inline(vec![MathNode::Cancel {
        stroke: StrokeKind::Forward,
        content: Box::new(MathNode::Identifier("x".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_scaled_doubles_dimensions() {
    let content = MathNode::Identifier("x".into());
    let normal = layout_node(
        &content,
        &LayoutCtx {
            style: MathStyle::Text,
            params: TexParams::default(),
            options: &default_options(),
            depth: 0,
            size_scale: 1.0,
            spacing_style: None,
        },
    );
    let expr = MathExpr::inline(vec![MathNode::Scaled {
        factor: Rational::new(2, 1),
        content: Box::new(content.clone()),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // Width should be approximately double (within the HBox wrapper)
    if let BoxKind::HBox { children } = &root.kind {
        let scaled = &children[0].layout;
        assert!(
            (scaled.width - normal.width * 2.0).abs() < 0.01,
            "scaled by 2 should double the width"
        );
    }
}

#[test]
fn layout_scaled_scales_child_positions() {
    // Scale a multi-node expression and verify child positions are also scaled
    let content = MathNode::Row(vec![
        MathNode::Identifier("a".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
        MathNode::Identifier("b".into()),
    ]);
    let factor = Rational::new(2, 1);
    let ctx = LayoutCtx {
        style: MathStyle::Text,
        params: TexParams::default(),
        options: &default_options(),
        depth: 0,
        size_scale: 1.0,
        spacing_style: None,
    };
    let normal = layout_node(&content, &ctx);
    let scaled = layout_scaled(factor, &content, &ctx);

    // The scaled box should have approximately double the width
    assert!(
        (scaled.width - normal.width * 2.0).abs() < 0.01,
        "width should be scaled"
    );

    // If the inner is an HBox, verify children have scaled positions
    if let (BoxKind::HBox { children: nc }, BoxKind::HBox { children: sc }) =
        (&normal.kind, &scaled.kind)
        && nc.len() == sc.len()
        && nc.len() > 1
    {
        // dx of later children should be approximately 2x
        let normal_last_dx = nc.last().unwrap().dx;
        let scaled_last_dx = sc.last().unwrap().dx;
        if normal_last_dx > 0.01 {
            let ratio = scaled_last_dx / normal_last_dx;
            assert!(
                (ratio - 2.0).abs() < 0.1,
                "child dx should be scaled by 2x, got ratio {ratio:.2}"
            );
        }
    }
}

#[test]
fn layout_text_upright() {
    let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, "abc".into())]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
}

#[test]
fn layout_space_kern() {
    let expr = MathExpr::inline(vec![MathNode::Space(Rational::new(1, 2))]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0, "space should produce width");
}

#[test]
fn layout_identifier_single_char() {
    let expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
}

#[test]
fn layout_operator_large_in_display() {
    let expr = MathExpr::inline(vec![MathNode::Operator(OperatorAttr {
        name: "∑".into(),
        limit_style: LimitStyle::Default,
    })]);
    let opts = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &opts);
    let text = layout_expr(&expr, MathStyle::Text, &opts);
    assert!(
        display.height >= text.height,
        "large op in display should be at least as tall as in text"
    );
}

#[test]
fn layout_row_with_spacing() {
    let expr = MathExpr::inline(vec![
        MathNode::Identifier("x".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
        MathNode::Identifier("y".into()),
    ]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // Row with binary op spacing should be wider than just x and y
    let x_expr = MathExpr::inline(vec![MathNode::Identifier("x".into())]);
    let y_expr = MathExpr::inline(vec![MathNode::Identifier("y".into())]);
    let x_w = layout_expr(&x_expr, MathStyle::Text, &default_options()).width;
    let y_w = layout_expr(&y_expr, MathStyle::Text, &default_options()).width;
    assert!(
        root.width > x_w + y_w,
        "row with operator should include spacing"
    );
}

#[test]
fn layout_prescripts_before_base() {
    let expr = MathExpr::inline(vec![MathNode::PreScripts {
        base: Box::new(MathNode::Identifier("C".into())),
        pre_sub: Some(Box::new(MathNode::Number("6".into()))),
        pre_sup: Some(Box::new(MathNode::Number("12".into()))),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
}

#[test]
fn layout_styled_children() {
    let expr = MathExpr::inline(vec![MathNode::Styled(
        TextStyle::Bold,
        vec![MathNode::Identifier("x".into())],
    )]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
}

#[test]
fn layout_symbol_various_kinds() {
    for kind in [
        SymbolKind::Ordinary,
        SymbolKind::BinaryOp,
        SymbolKind::Relation,
        SymbolKind::Open,
        SymbolKind::Close,
        SymbolKind::Punctuation,
    ] {
        let expr = MathExpr::inline(vec![MathNode::Symbol(kind, "+".into())]);
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(root.width > 0.0, "symbol kind {:?} should have width", kind);
    }
}

#[test]
fn layout_deeply_nested_fraction() {
    let inner_frac = MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Number("1".into())),
        den: Box::new(MathNode::Number("2".into())),
    };
    let outer_frac = MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(inner_frac),
        den: Box::new(MathNode::Number("3".into())),
    };
    let expr = MathExpr::inline(vec![outer_frac]);
    let root = layout_expr(&expr, MathStyle::Display, &default_options());
    assert!(root.width > 0.0);
    assert!(root.height > 0.0);
    assert!(root.depth > 0.0);
}

#[test]
fn layout_fraction_nobar() {
    let expr = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::NoBar,
        num: Box::new(MathNode::Number("n".into())),
        den: Box::new(MathNode::Number("k".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(root.width > 0.0);
}

#[test]
fn script_style_shrinks() {
    let base_expr = MathExpr::inline(vec![MathNode::Number("2".into())]);
    let opts = default_options();
    let text_box = layout_expr(&base_expr, MathStyle::Text, &opts);
    let script_box = layout_expr(&base_expr, MathStyle::Script, &opts);
    assert!(
        script_box.width < text_box.width,
        "script style should produce smaller glyphs"
    );
}

#[test]
fn large_op_symbol_scales_in_display() {
    // Symbol(LargeOp, "∑") should be larger in display mode than text mode
    let expr = MathExpr::inline(vec![MathNode::Symbol(
        SymbolKind::LargeOp,
        "\u{2211}".into(),
    )]);
    let opts = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &opts);
    let text = layout_expr(&expr, MathStyle::Text, &opts);
    assert!(
        display.height > text.height,
        "LargeOp symbol in display should be taller: display={}, text={}",
        display.height,
        text.height,
    );
    assert!(
        display.width > text.width,
        "LargeOp symbol in display should be wider: display={}, text={}",
        display.width,
        text.width,
    );
}

#[test]
fn subsup_with_large_op_uses_limits_in_display() {
    // SubSup with LargeOp base in display mode should place scripts above/below
    let expr_display = MathExpr::inline(vec![MathNode::SubSup {
        base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
        sub: Box::new(MathNode::Identifier("i".into())),
        sup: Box::new(MathNode::Identifier("n".into())),
    }]);
    let opts = default_options();
    let display = layout_expr(&expr_display, MathStyle::Display, &opts);
    // In limits mode, the total height should be larger because sup is above
    // and sub is below the operator, adding to overall vertical extent
    let just_op = layout_expr(
        &MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]),
        MathStyle::Display,
        &opts,
    );
    assert!(
        display.height > just_op.height,
        "SubSup with limits should be taller than just the op: display={}, op={}",
        display.height,
        just_op.height,
    );
    assert!(
        display.depth > just_op.depth,
        "SubSup with limits should have more depth than just the op: display={}, op={}",
        display.depth,
        just_op.depth,
    );
}

#[test]
fn subscript_with_large_op_uses_limits_in_display() {
    // Subscript with LargeOp base in display mode should place sub below
    let expr = MathExpr::inline(vec![MathNode::Subscript {
        base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
        sub: Box::new(MathNode::Identifier("i".into())),
    }]);
    let opts = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &opts);
    let just_op = layout_expr(
        &MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]),
        MathStyle::Display,
        &opts,
    );
    assert!(
        display.depth > just_op.depth,
        "Subscript limit should add depth below op"
    );
}

#[test]
fn superscript_with_large_op_uses_limits_in_display() {
    // Superscript with LargeOp base in display mode should place sup above
    let expr = MathExpr::inline(vec![MathNode::Superscript {
        base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
        sup: Box::new(MathNode::Identifier("n".into())),
    }]);
    let opts = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &opts);
    let just_op = layout_expr(
        &MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]),
        MathStyle::Display,
        &opts,
    );
    assert!(
        display.height > just_op.height,
        "Superscript limit should add height above op"
    );
}

#[test]
fn subsup_with_large_op_no_limits_in_text() {
    // SubSup with LargeOp base in text mode should NOT use limits
    let expr = MathExpr::inline(vec![MathNode::SubSup {
        base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into())),
        sub: Box::new(MathNode::Identifier("i".into())),
        sup: Box::new(MathNode::Identifier("n".into())),
    }]);
    let opts = default_options();
    let text = layout_expr(&expr, MathStyle::Text, &opts);
    let just_op = layout_expr(
        &MathExpr::inline(vec![MathNode::Symbol(
            SymbolKind::LargeOp,
            "\u{2211}".into(),
        )]),
        MathStyle::Text,
        &opts,
    );
    // In non-limits mode, the width should be wider (scripts to the side)
    assert!(
        text.width > just_op.width,
        "SubSup in text mode should be wider than just the op (scripts beside, not above/below)"
    );
}

#[test]
fn nary_nolimits_still_scales_in_display() {
    // Nary with SubSup limit style in display mode should still scale the op glyph
    let expr = MathExpr::inline(vec![MathNode::Nary {
        op: '\u{2211}',
        sub: Some(Box::new(MathNode::Identifier("i".into()))),
        sup: Some(Box::new(MathNode::Identifier("n".into()))),
        body: Box::new(MathNode::Identifier("x".into())),
        limits: LimitStyle::SubSup,
    }]);
    let opts = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &opts);
    let text_expr = MathExpr::inline(vec![MathNode::Nary {
        op: '\u{2211}',
        sub: Some(Box::new(MathNode::Identifier("i".into()))),
        sup: Some(Box::new(MathNode::Identifier("n".into()))),
        body: Box::new(MathNode::Identifier("x".into())),
        limits: LimitStyle::SubSup,
    }]);
    let text = layout_expr(&text_expr, MathStyle::Text, &opts);
    assert!(
        display.height > text.height,
        "Nary with SubSup style in display should still be taller (scaled op): display={}, text={}",
        display.height,
        text.height,
    );
}

// === Arena layout tests ===

#[test]
fn arena_layout_simple_number() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("42", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
    assert!(b.height > 0.0);
}

#[test]
fn arena_layout_superscript() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.5, "superscript should have positive width");
}

#[test]
fn arena_layout_subscript() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("x_i", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.5);
    assert!(b.depth > 0.0, "subscript should add depth");
}

#[test]
fn arena_layout_subsup() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("x_i^2", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.5);
    assert!(b.height > 0.0);
    assert!(b.depth > 0.0);
}

#[test]
fn arena_layout_fraction() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("\\frac{1}{2}", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Display, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.height > 0.0);
    assert!(b.depth > 0.0);
}

#[test]
fn arena_layout_sqrt() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("\\sqrt{x}", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
    assert!(b.height > 0.0);
}

#[test]
fn arena_layout_fenced() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\left(x\\right)", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
}

#[test]
fn arena_layout_nary_display() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\sum_{i=0}^{n} x", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Display, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
    assert!(b.height > 0.0);
}

#[test]
fn arena_layout_overunder() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result =
        crate::reader::latex::LatexReader::parse_compact("\\overset{a}{b}", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
}

#[test]
fn arena_layout_array() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact(
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        &config,
    )
    .unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
}

#[test]
fn arena_layout_accent() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("\\hat{x}", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);
    let root = builder.layout(&mut arena, &result.expr);
    let b = arena.get(root);
    assert!(b.width > 0.0);
    assert!(b.height > 0.0);
}

#[test]
fn arena_layout_depth_limit() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let mut latex = String::new();
    for _ in 0..150 {
        latex.push_str("\\frac{");
    }
    latex.push('x');
    for _ in 0..150 {
        latex.push_str("}{1}");
    }
    let config = crate::error::Config::default();
    if let Ok(result) = crate::reader::latex::LatexReader::parse_compact(&latex, &config) {
        let mut arena = LayoutArena::new();
        let opts = default_options();
        let builder = LayoutBuilder::new(MathStyle::Text, &opts);
        // Must not panic or overflow stack
        let _root = builder.layout(&mut arena, &result.expr);
    }
}

#[test]
fn arena_clear_and_reuse() {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact("x^2", &config).unwrap();
    let mut arena = LayoutArena::new();
    let opts = default_options();
    let builder = LayoutBuilder::new(MathStyle::Text, &opts);

    let root1 = builder.layout(&mut arena, &result.expr);
    let w1 = arena.get(root1).width;

    arena.clear();
    let root2 = builder.layout(&mut arena, &result.expr);
    let w2 = arena.get(root2).width;

    assert!(
        (w1 - w2).abs() < f64::EPSILON,
        "reused arena should produce identical layout"
    );
}

// === Arena-heap equivalence tests ===

/// Helper: verify arena layout produces identical SVG to heap layout for a LaTeX string.
fn assert_arena_heap_equivalent(latex: &str, style: MathStyle) {
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    use crate::render::emit::{emit_svg, emit_svg_arena};

    let config = crate::error::Config::default();
    let result = crate::reader::latex::LatexReader::parse_compact(latex, &config)
        .unwrap_or_else(|e| panic!("parse failed for {latex:?}: {e}"));
    let options = default_options();

    let heap_root = layout_expr(&result.expr.to_math_expr(), style, &options);
    let heap_svg = emit_svg(&heap_root, &options);

    let mut arena = LayoutArena::new();
    let builder = LayoutBuilder::new(style, &options);
    let arena_root = builder.layout(&mut arena, &result.expr);
    let arena_svg = emit_svg_arena(&arena, arena_root, &options);

    assert_eq!(heap_svg, arena_svg, "mismatch for {latex:?}");
}

#[test]
fn equivalence_simple() {
    assert_arena_heap_equivalent("x^2 + 1", MathStyle::Text);
}

#[test]
fn equivalence_quadratic() {
    assert_arena_heap_equivalent(
        "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
        MathStyle::Display,
    );
}

#[test]
fn equivalence_nested_frac() {
    assert_arena_heap_equivalent("\\frac{\\frac{a}{b}}{\\frac{c}{d}}", MathStyle::Display);
}

#[test]
fn equivalence_sum() {
    assert_arena_heap_equivalent("\\sum_{i=0}^{n} \\frac{1}{i^2}", MathStyle::Display);
}

#[test]
fn equivalence_sqrt_with_index() {
    assert_arena_heap_equivalent("\\sqrt[3]{x}", MathStyle::Text);
}

#[test]
fn equivalence_fenced() {
    assert_arena_heap_equivalent("\\left(\\frac{a}{b}\\right)", MathStyle::Display);
}

#[test]
fn equivalence_matrix() {
    assert_arena_heap_equivalent(
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        MathStyle::Text,
    );
}

#[test]
fn equivalence_accent() {
    assert_arena_heap_equivalent("\\hat{x} + \\vec{v}", MathStyle::Text);
}

#[test]
fn equivalence_text() {
    assert_arena_heap_equivalent("\\text{hello}", MathStyle::Text);
}

#[test]
fn equivalence_navier_stokes() {
    assert_arena_heap_equivalent(
        "\\rho\\left(\\frac{\\partial \\mathbf{u}}{\\partial t} + \\mathbf{u} \\cdot \\nabla \\mathbf{u}\\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{u} + \\mathbf{f}",
        MathStyle::Display,
    );
}

// =====================================================================
// Aligned / multi-column alignment tests
// =====================================================================

/// Simple 2-column aligned environment: columns should be laid out
/// side by side rather than stacked vertically.
#[test]
fn aligned_two_column_has_grid_layout() {
    use crate::reader::Reader;
    let config = crate::error::Config::default();
    let latex = r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}";
    let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
    let opts = default_options();
    let root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &opts);
    assert!(
        root.width > 1.5,
        "aligned block should be wider than a single column, got {}",
        root.width
    );
}

/// Multi-pair aligned environment: 4 columns (rl rl).
#[test]
fn aligned_four_column_has_grid_layout() {
    use crate::reader::Reader;
    let config = crate::error::Config::default();
    let latex = r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}";
    let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();

    fn find_num_cols(arena: &AstArena, id: crate::ast::compact::NodeId) -> Option<u16> {
        match arena.node(id) {
            crate::ast::compact::CompactNode::Array { num_cols, .. } => Some(*num_cols),
            crate::ast::compact::CompactNode::Row(span) => {
                for &child_id in arena.children(*span) {
                    if let Some(nc) = find_num_cols(arena, child_id) {
                        return Some(nc);
                    }
                }
                None
            },
            _ => None,
        }
    }
    let nc = {
        let arena = &result.expr.arena;
        let mut found = None;
        for &root_id in arena.children(result.expr.roots) {
            found = find_num_cols(arena, root_id);
            if found.is_some() {
                break;
            }
        }
        found.expect("should find Array node")
    };
    assert_eq!(nc, 4, "aligned with 3 &'s per row should produce 4 columns");

    let opts = default_options();
    let root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &opts);
    assert!(
        root.width > 3.0,
        "4-column aligned block should be wide, got {}",
        root.width
    );
}

#[test]
fn equivalence_aligned() {
    assert_arena_heap_equivalent(
        r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}",
        MathStyle::Text,
    );
}

#[test]
fn equivalence_aligned_multi_pair() {
    assert_arena_heap_equivalent(
        r"\begin{aligned} a &= b & c &= d \\ e &= f & g &= h \end{aligned}",
        MathStyle::Text,
    );
}

#[test]
fn alignedat_parses_and_renders() {
    use crate::reader::Reader;
    let config = crate::error::Config::default();
    let latex = r"\begin{alignedat}{2} x &= 1 & \quad y &= 2 \end{alignedat}";
    let result = crate::reader::latex::LatexReader::parse_compact(latex, &config).unwrap();
    let opts = default_options();
    let root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &opts);
    assert!(
        root.width > 1.0,
        "alignedat should render with meaningful width"
    );
}

// ── Tests for P2 bug fixes ──────────────────────────────────────────

#[test]
fn delimiter_scales_with_content_height() {
    let small_fenced = MathExpr::inline(vec![MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
    }]);
    let small = layout_expr(&small_fenced, MathStyle::Text, &default_options());

    let tall_fenced = MathExpr::inline(vec![MathNode::Fenced {
        open: "(".into(),
        close: ")".into(),
        content: vec![FencedItem::Element(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Superscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sup: Box::new(MathNode::Number("2".into())),
            }),
            den: Box::new(MathNode::Identifier("y".into())),
        })],
    }]);
    let tall = layout_expr(&tall_fenced, MathStyle::Display, &default_options());

    assert!(
        tall.width > small.width,
        "fenced with fraction should be wider than fenced with single char: {} vs {}",
        tall.width,
        small.width
    );
}

#[test]
fn fraction_bar_visible_at_scriptscript() {
    let frac = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Number("1".into())),
        den: Box::new(MathNode::Number("2".into())),
    }]);
    let opts = default_options();
    let root = layout_expr(&frac, MathStyle::ScriptScript, &opts);

    fn find_rule_thickness(layout: &LayoutBox) -> Option<f64> {
        match &layout.kind {
            BoxKind::Rule { thickness } => Some(*thickness),
            BoxKind::HBox { children } | BoxKind::VBox { children } => {
                for child in children {
                    if let Some(t) = find_rule_thickness(&child.layout) {
                        return Some(t);
                    }
                }
                None
            },
            _ => None,
        }
    }

    let thickness = find_rule_thickness(&root).expect("fraction should contain a rule bar");
    // At ScriptScript (size=0.5), rule_thickness = 0.049 (cmex7), so
    // bar = 0.049 * 0.5 = 0.0245em.  Bars scale naturally with style.
    let expected = 0.049 * 0.5; // xi_rule[scriptscript] * size
    assert!(
        (thickness - expected).abs() < 0.001,
        "bar thickness at ScriptScript should be ~{}, got {}",
        expected,
        thickness
    );
}

#[test]
fn cdots_raised_to_axis() {
    let cdots_expr = MathExpr::inline(vec![MathNode::Symbol(
        SymbolKind::Ordinary,
        "\u{22EF}".into(),
    )]);
    let opts = default_options();
    let root = layout_expr(&cdots_expr, MathStyle::Text, &opts);

    let ldots_expr = MathExpr::inline(vec![MathNode::Symbol(SymbolKind::Ordinary, ".".into())]);
    let ldots = layout_expr(&ldots_expr, MathStyle::Text, &opts);

    assert!(
        root.height > ldots.height,
        "cdots should be taller than baseline dots: {} vs {}",
        root.height,
        ldots.height
    );
}

#[test]
fn fraction_has_side_padding() {
    let num_expr = MathExpr::inline(vec![MathNode::Identifier("a".into())]);
    let den_expr = MathExpr::inline(vec![MathNode::Identifier("b".into())]);
    let opts = default_options();
    let num_w = layout_expr(&num_expr, MathStyle::Text, &opts).width;
    let den_w = layout_expr(&den_expr, MathStyle::Text, &opts).width;
    let max_content_w = num_w.max(den_w);

    let frac_expr = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Identifier("a".into())),
        den: Box::new(MathNode::Identifier("b".into())),
    }]);
    let frac = layout_expr(&frac_expr, MathStyle::Text, &opts);

    assert!(
        frac.width > max_content_w,
        "fraction should include side padding: {} vs {}",
        frac.width,
        max_content_w
    );
}

#[test]
fn equivalence_cdots() {
    assert_arena_heap_equivalent("a \\cdots b", MathStyle::Text);
}

#[test]
fn equivalence_fenced_fraction() {
    assert_arena_heap_equivalent("\\left(\\frac{x^2}{y}\\right)", MathStyle::Display);
}

#[test]
fn displaystyle_propagation_in_text_mode() {
    // \displaystyle inside text mode should make \sum render at display size.
    // This is a compact-path feature: the legacy MathExpr engine cannot
    // represent inline \displaystyle switches (CompactExpr::to_math_expr drops
    // StyleSwitch nodes), so we exercise the production arena engine here.
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let opts = default_options();
    let mut arena = LayoutArena::new();

    // Text-mode sum (no \displaystyle)
    let text_result = crate::reader::latex::LatexReader::parse_compact("\\sum x", &config)
        .expect("parse text sum");
    let text_id = LayoutBuilder::new(MathStyle::Text, &opts).layout(&mut arena, &text_result.expr);
    let text_height = arena.get(text_id).height;

    // Text-mode with \displaystyle forcing display-size sum
    let display_result =
        crate::reader::latex::LatexReader::parse_compact("{\\displaystyle \\sum} x", &config)
            .expect("parse displaystyle sum");
    arena.clear();
    let display_id =
        LayoutBuilder::new(MathStyle::Text, &opts).layout(&mut arena, &display_result.expr);
    let display_height = arena.get(display_id).height;

    // The \displaystyle version should be taller (display-size operator)
    assert!(
        display_height > text_height,
        "\\displaystyle sum should be taller than text sum: {} vs {}",
        display_height,
        text_height
    );
}

#[test]
fn equivalence_displaystyle() {
    // The legacy MathExpr engine drops inline \displaystyle switches
    // (CompactExpr::to_math_expr reconstructs StyleSwitch as an empty row),
    // so a heap-vs-arena byte-equality guard is not possible for this input.
    // Instead verify the production arena engine honors \displaystyle: the
    // display-styled sum must render taller than its text-mode counterpart.
    use crate::reader::Reader;
    use crate::render::arena::LayoutArena;
    let config = crate::error::Config::default();
    let opts = default_options();
    let mut arena = LayoutArena::new();

    let plain = crate::reader::latex::LatexReader::parse_compact("\\sum_{i=0}^n a_i", &config)
        .expect("parse plain sum");
    let plain_id = LayoutBuilder::new(MathStyle::Text, &opts).layout(&mut arena, &plain.expr);
    let plain_extent = arena.get(plain_id).height + arena.get(plain_id).depth;

    let styled = crate::reader::latex::LatexReader::parse_compact(
        "{\\displaystyle \\sum_{i=0}^n} a_i",
        &config,
    )
    .expect("parse displaystyle sum");
    arena.clear();
    let styled_id = LayoutBuilder::new(MathStyle::Text, &opts).layout(&mut arena, &styled.expr);
    let styled_extent = arena.get(styled_id).height + arena.get(styled_id).depth;

    assert!(
        styled_extent > plain_extent,
        "\\displaystyle sum should be taller than plain text-mode sum: {} vs {}",
        styled_extent,
        plain_extent
    );
}

#[test]
fn lim_limits_below_in_display_mode() {
    // \lim_{n \to \infty} in display mode should place subscript below
    use crate::reader::Reader;
    let config = crate::error::Config::default();
    let opts = default_options();

    // Display-mode lim (subscript should go below)
    let display_result =
        crate::reader::latex::LatexReader::parse_compact("\\lim_{n \\to \\infty} a_n", &config)
            .expect("parse display lim");
    let display_root = layout_expr(
        &display_result.expr.to_math_expr(),
        MathStyle::Display,
        &opts,
    );

    // Text-mode lim (subscript to the side)
    let text_result =
        crate::reader::latex::LatexReader::parse_compact("\\lim_{n \\to \\infty} a_n", &config)
            .expect("parse text lim");
    let text_root = layout_expr(&text_result.expr.to_math_expr(), MathStyle::Text, &opts);

    // Display mode should be taller because subscript is below, not to the side
    assert!(
        display_root.height + display_root.depth > text_root.height + text_root.depth,
        "display \\lim should be taller than text \\lim: {}+{} vs {}+{}",
        display_root.height,
        display_root.depth,
        text_root.height,
        text_root.depth
    );
}

#[test]
fn wants_limits_above_below_operator_default_display() {
    let op = MathNode::Operator(OperatorAttr {
        name: "lim".into(),
        limit_style: LimitStyle::Default,
    });
    assert!(wants_limits_above_below(&op, MathStyle::Display));
    assert!(!wants_limits_above_below(&op, MathStyle::Text));
}

#[test]
fn wants_limits_above_below_operator_always() {
    let op = MathNode::Operator(OperatorAttr {
        name: "lim".into(),
        limit_style: LimitStyle::AboveBelow,
    });
    assert!(wants_limits_above_below(&op, MathStyle::Display));
    assert!(wants_limits_above_below(&op, MathStyle::Text));
}

#[test]
fn wants_limits_above_below_operator_subsup() {
    let op = MathNode::Operator(OperatorAttr {
        name: "lim".into(),
        limit_style: LimitStyle::SubSup,
    });
    assert!(!wants_limits_above_below(&op, MathStyle::Display));
    assert!(!wants_limits_above_below(&op, MathStyle::Text));
}

#[test]
fn wants_limits_above_below_large_op() {
    let sym = MathNode::Symbol(SymbolKind::LargeOp, "\u{2211}".into());
    assert!(wants_limits_above_below(&sym, MathStyle::Display));
    assert!(!wants_limits_above_below(&sym, MathStyle::Text));
}

#[test]
fn integral_large_op_no_above_below() {
    // Integral operators should NOT get above/below limits even in display mode
    let int = MathNode::Symbol(SymbolKind::LargeOp, "\u{222B}".into());
    assert!(!wants_limits_above_below(&int, MathStyle::Display));
    assert!(!wants_limits_above_below(&int, MathStyle::Text));
    // Double integral
    let iint = MathNode::Symbol(SymbolKind::LargeOp, "\u{222C}".into());
    assert!(!wants_limits_above_below(&iint, MathStyle::Display));
    // Contour integral
    let oint = MathNode::Symbol(SymbolKind::LargeOp, "\u{222E}".into());
    assert!(!wants_limits_above_below(&oint, MathStyle::Display));
}

#[test]
fn wants_limits_above_below_ordinary() {
    let node = MathNode::Identifier("x".into());
    assert!(!wants_limits_above_below(&node, MathStyle::Display));
}

#[test]
fn layout_lim_subscript_below_in_display_heap() {
    // Test that \lim_{x \to 0} in display mode places the subscript below via heap path
    use crate::reader::Reader;
    let config = crate::Config::default();
    let result = crate::reader::latex::LatexReader::parse("\\lim_{x \\to 0} f(x)", &config)
        .expect("parse lim");
    let opts = default_options();

    // Display mode: subscript should be below "lim"
    let display_root = layout_expr(&result.expr, MathStyle::Display, &opts);

    // Text mode: subscript should be to the right
    let text_root = layout_expr(&result.expr, MathStyle::Text, &opts);

    // Display mode should be taller because subscript is placed below, not to the side
    assert!(
        display_root.height + display_root.depth > text_root.height + text_root.depth,
        "heap display \\lim should be taller than text \\lim: {}+{} vs {}+{}",
        display_root.height,
        display_root.depth,
        text_root.height,
        text_root.depth
    );
}

// =====================================================================
// Accent sizing tests — verify accents are visually large enough
// =====================================================================

/// Helper: layout an accent expression and return (base_height, accent_height, total_height).
fn accent_dimensions(accent_char: char) -> (f64, f64, f64) {
    let expr = MathExpr::inline(vec![MathNode::Accent {
        base: Box::new(MathNode::Identifier("x".into())),
        accent: accent_char,
        position: AccentPosition::Above,
        wide: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // The x glyph height at size 1.0 is ~0.431 em
    let base_h = 0.431;
    let accent_h = root.height - base_h - 0.05; // subtract gap
    (base_h, accent_h, root.height)
}

#[test]
fn accent_hat_minimum_height() {
    let (_, accent_h, _) = accent_dimensions('\u{0302}');
    // Hat accent should be at least 0.15 em tall (clearly visible)
    assert!(
        accent_h >= 0.15,
        "hat accent should be at least 0.15 em, got {accent_h:.4}"
    );
}

#[test]
fn accent_tilde_minimum_height() {
    let (_, accent_h, _) = accent_dimensions('\u{0303}');
    assert!(
        accent_h >= 0.15,
        "tilde accent should be at least 0.15 em, got {accent_h:.4}"
    );
}

#[test]
fn accent_bar_visible_thickness() {
    let expr = MathExpr::inline(vec![MathNode::Accent {
        base: Box::new(MathNode::Identifier("y".into())),
        accent: '\u{0304}',
        position: AccentPosition::Above,
        wide: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    // Bar thickness (rule_thickness * 1.8, min 0.06) at size 1.0 =>
    // 0.04 * 1.8 = 0.072, so half is 0.036. Total height should be
    // base_height + gap + bar_height.
    let base_h = 0.431; // y height (no depth)
    let gap = 0.05;
    let min_bar = 0.06 / 2.0; // minimum bar half-height
    assert!(
        root.height >= base_h + gap + min_bar,
        "bar accent should produce visible rule, height={:.4}",
        root.height
    );
}

#[test]
fn accent_dot_visible_size() {
    let (_, accent_h, _) = accent_dimensions('\u{0307}');
    // Dot diameter = 2 * 0.07 = 0.14 em
    assert!(
        accent_h >= 0.10,
        "dot accent should be at least 0.10 em diameter, got {accent_h:.4}"
    );
}

#[test]
fn accent_vec_visible_arrow() {
    let (_, accent_h, _) = accent_dimensions('\u{20D7}');
    // Vec arrow height = 0.25 em
    assert!(
        accent_h >= 0.20,
        "vec accent should be at least 0.20 em tall, got {accent_h:.4}"
    );
}

#[test]
fn accent_gap_separates_from_base() {
    let expr = MathExpr::inline(vec![MathNode::Accent {
        base: Box::new(MathNode::Identifier("x".into())),
        accent: '\u{0302}',
        position: AccentPosition::Above,
        wide: false,
    }]);
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    let base_h = 0.431;
    // Total height = base_h + gap + accent_h. The gap should be 0.05 em.
    // So root.height - base_h should include at least 0.05 gap.
    let above_base = root.height - base_h;
    assert!(
        above_base >= 0.05,
        "accent should have at least 0.05 em gap above base, got {above_base:.4}"
    );
}

/// Verify that `\Gamma(n)=(n-1)!` has zero inter-atom space around
/// all parentheses and between `)` and `!` (factorial).
#[test]
fn gamma_paren_factorial_tight_spacing() {
    use crate::reader::Reader;
    let config = crate::error::Config::default();
    let result =
        crate::reader::latex::LatexReader::parse_compact(r"\Gamma(n)=(n-1)!", &config).unwrap();
    let options = default_options();
    let root = layout_expr(&result.expr.to_math_expr(), MathStyle::Text, &options);

    if let BoxKind::HBox { ref children } = root.kind {
        // Expect 11 glyphs: Γ ( n ) = ( n - 1 ) !
        assert!(
            children.len() >= 11,
            "expected at least 11 children, got {}",
            children.len()
        );

        // Helper: gap between child i's right edge and child i+1's left edge
        let gap = |i: usize| -> f64 {
            let right_edge = children[i].dx + children[i].layout.width;
            children[i + 1].dx - right_edge
        };

        // Γ( — Ord-Open should be 0
        assert!(
            gap(0).abs() < 0.001,
            "Γ-( gap should be ~0, got {:.4}",
            gap(0)
        );
        // (n — Open-Ord should be 0
        assert!(
            gap(1).abs() < 0.001,
            "(-n gap should be ~0, got {:.4}",
            gap(1)
        );
        // n) — Ord-Close should be 0
        assert!(
            gap(2).abs() < 0.001,
            "n-) gap should be ~0, got {:.4}",
            gap(2)
        );
        // )= — Close-Rel should be >0 (thick space)
        assert!(
            gap(3) > 0.1,
            ")-= gap should be thick space, got {:.4}",
            gap(3)
        );
        // =( — Rel-Open should be >0 (thick space)
        assert!(
            gap(4) > 0.1,
            "=-( gap should be thick space, got {:.4}",
            gap(4)
        );
        // (n — Open-Ord should be 0
        assert!(
            gap(5).abs() < 0.001,
            "(-n gap should be ~0, got {:.4}",
            gap(5)
        );

        // )! — Close-Ord should be 0 (! is Ordinary, not Relation)
        assert!(
            gap(9).abs() < 0.001,
            ")-! gap should be ~0 (factorial), got {:.4}",
            gap(9)
        );
    } else {
        panic!("expected HBox at root");
    }
}

/// Verify that binary operators inside a fraction numerator/denominator
/// receive non-zero inter-atom spacing in display mode.
#[test]
fn fraction_binop_spacing_display() {
    use crate::ast::SymbolKind;
    // \frac{a + b}{c - d}  in display mode
    let num = MathNode::Row(vec![
        MathNode::Identifier("a".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
        MathNode::Identifier("b".into()),
    ]);
    let den = MathNode::Row(vec![
        MathNode::Identifier("c".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "\u{2212}".into()), // minus
        MathNode::Identifier("d".into()),
    ]);
    let expr = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(num),
        den: Box::new(den),
    }]);
    let opts = default_options();
    let root = layout_expr(&expr, MathStyle::Display, &opts);

    // Dig into the VBox (fraction) to find the numerator HBox
    if let BoxKind::HBox { ref children } = root.kind {
        let frac = &children[0].layout;
        if let BoxKind::VBox { ref children } = frac.kind {
            // children[0] = numerator, children[1] = denominator
            let check_spacing = |label: &str, layout: &LayoutBox| {
                if let BoxKind::HBox { ref children } = layout.kind {
                    assert!(
                        children.len() >= 3,
                        "{label}: expected at least 3 children (a, op, b), got {}",
                        children.len()
                    );
                    // Gap before the operator
                    let gap_before = children[1].dx - (children[0].dx + children[0].layout.width);
                    // Gap after the operator
                    let gap_after = children[2].dx - (children[1].dx + children[1].layout.width);
                    assert!(
                        gap_before > 0.05,
                        "{label}: gap before binary op should be > 0.05, got {:.4}",
                        gap_before
                    );
                    assert!(
                        gap_after > 0.05,
                        "{label}: gap after binary op should be > 0.05, got {:.4}",
                        gap_after
                    );
                } else {
                    panic!("{label}: expected HBox");
                }
            };
            check_spacing("numerator", &children[0].layout);
            check_spacing("denominator", &children[1].layout);
        } else {
            panic!("fraction should be VBox");
        }
    } else {
        panic!("root should be HBox");
    }
}

/// Verify that binary operators inside a fraction numerator/denominator
/// receive non-zero inter-atom spacing even in inline (text) mode.
/// This tests the spacing floor added by the fraction layout functions.
#[test]
fn fraction_binop_spacing_inline() {
    use crate::ast::SymbolKind;
    // \frac{a + b}{c - d}  in inline (text) mode
    let num = MathNode::Row(vec![
        MathNode::Identifier("a".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
        MathNode::Identifier("b".into()),
    ]);
    let den = MathNode::Row(vec![
        MathNode::Identifier("c".into()),
        MathNode::Symbol(SymbolKind::BinaryOp, "\u{2212}".into()),
        MathNode::Identifier("d".into()),
    ]);
    let expr = MathExpr::inline(vec![MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(num),
        den: Box::new(den),
    }]);
    let opts = default_options();
    let root = layout_expr(&expr, MathStyle::Text, &opts);

    // In inline mode, fracnum(Text) = Script. Without the spacing floor
    // fix, medium space would be suppressed. With the fix, it should be
    // non-zero.
    if let BoxKind::HBox { ref children } = root.kind {
        let frac = &children[0].layout;
        if let BoxKind::VBox { ref children } = frac.kind {
            let check_spacing = |label: &str, layout: &LayoutBox| {
                if let BoxKind::HBox { ref children } = layout.kind {
                    assert!(
                        children.len() >= 3,
                        "{label}: expected at least 3 children, got {}",
                        children.len()
                    );
                    let gap_before = children[1].dx - (children[0].dx + children[0].layout.width);
                    assert!(
                        gap_before > 0.01,
                        "{label}: inline fraction should still have spacing around binary ops, got {:.4}",
                        gap_before
                    );
                } else {
                    panic!("{label}: expected HBox");
                }
            };
            check_spacing("numerator", &children[0].layout);
            check_spacing("denominator", &children[1].layout);
        } else {
            panic!("fraction should be VBox");
        }
    } else {
        panic!("root should be HBox");
    }
}

/// Verify the compact path produces identical spacing to the heap path
/// for fractions with binary operators.
#[test]
fn fraction_spacing_compact_matches_heap() {
    let latex = r"\frac{a + b}{c - d}";
    let parsed = crate::latex::parse::parse(latex, &Default::default()).expect("parse failed");
    let compact = crate::latex::parse::parse_compact(latex, &Default::default())
        .expect("compact parse failed");
    let opts = default_options();

    for &style in &[MathStyle::Display, MathStyle::Text] {
        let heap_root = layout_expr(&parsed.expr, style, &opts);
        let compact_root = layout_expr(&compact.expr.to_math_expr(), style, &opts);
        let diff = (heap_root.width - compact_root.width).abs();
        assert!(
            diff < 0.01,
            "heap and compact should match for {:?}: heap={:.4} compact={:.4}",
            style,
            heap_root.width,
            compact_root.width
        );
    }
}

#[test]
fn middle_delimiter_is_rendered() {
    // \left( x \middle| y \right) should render the | as a delimiter
    let latex = r"\left( x \middle| y \right)";
    let parsed = crate::latex::parse::parse(latex, &Default::default()).expect("parse failed");
    let opts = default_options();
    let root = layout_expr(&parsed.expr, MathStyle::Display, &opts);
    // Count Delimiter boxes recursively
    fn count_delimiters(b: &LayoutBox) -> usize {
        match &b.kind {
            BoxKind::Delimiter { .. } => 1,
            BoxKind::HBox { children } | BoxKind::VBox { children } => {
                children.iter().map(|c| count_delimiters(&c.layout)).sum()
            },
            BoxKind::Color { child, .. } => count_delimiters(&child.layout),
            _ => 0,
        }
    }
    let delim_count = count_delimiters(&root);
    assert!(
        delim_count >= 3,
        "expected at least 3 delimiters (open, middle, close), got {delim_count}"
    );
}

#[test]
fn middle_delimiter_width_increases_total() {
    // Expression with \middle should be wider than without
    let with_middle = r"\left( x \middle| y \right)";
    let without_middle = r"\left( x y \right)";
    let parsed_with =
        crate::latex::parse::parse(with_middle, &Default::default()).expect("parse failed");
    let parsed_without =
        crate::latex::parse::parse(without_middle, &Default::default()).expect("parse failed");
    let opts = default_options();
    let root_with = layout_expr(&parsed_with.expr, MathStyle::Display, &opts);
    let root_without = layout_expr(&parsed_without.expr, MathStyle::Display, &opts);
    assert!(
        root_with.width > root_without.width,
        "expression with \\middle should be wider: {:.4} vs {:.4}",
        root_with.width,
        root_without.width
    );
}

#[test]
fn middle_delimiter_compact_matches_heap() {
    let latex = r"\left( x \middle| y \right)";
    let parsed = crate::latex::parse::parse(latex, &Default::default()).expect("parse failed");
    let compact = crate::latex::parse::parse_compact(latex, &Default::default())
        .expect("compact parse failed");
    let opts = default_options();
    let heap_root = layout_expr(&parsed.expr, MathStyle::Display, &opts);
    let compact_root = layout_expr(&compact.expr.to_math_expr(), MathStyle::Display, &opts);
    let diff = (heap_root.width - compact_root.width).abs();
    assert!(
        diff < 0.01,
        "heap and compact \\middle should match: heap={:.4} compact={:.4}",
        heap_root.width,
        compact_root.width
    );
}

#[test]
fn scale_svg_path_preserves_arc_flags() {
    // Arc: A rx ry x-rotation large-arc-flag sweep-flag x y
    // Parameters 3,4,5 (rotation, flags) must NOT be scaled.
    let path = "M0,0 A10,20 45 1 0 30,40";
    let scaled = scale_svg_path_data(path, 2.0);
    // rx=10*2=20, ry=20*2=40, rot=45 (unchanged), flag=1 (unchanged),
    // flag=0 (unchanged), x=30*2=60, y=40*2=80.
    // Output is minimized: trailing zeros trimmed, minimal separators.
    assert_eq!(scaled, "M0 0A20 40 45 1 0 60 80");
}

#[test]
fn scale_svg_path_arc_relative() {
    let path = "M0,0 a5,10 30 0 1 15,20";
    let scaled = scale_svg_path_data(path, 3.0);
    // rx=15, ry=30, rot=30 (unchanged), flags 0/1 (unchanged), x=45, y=60.
    assert_eq!(scaled, "M0 0a15 30 30 0 1 45 60");
}

#[test]
fn scale_svg_path_fill_prefix_preserved() {
    let path = "F:M0,0 L10,20";
    let scaled = scale_svg_path_data(path, 2.0);
    assert!(scaled.starts_with("F:"));
    // 10*2=20, 20*2=40 (trailing zeros trimmed in minimized output).
    assert!(scaled.contains("20"));
    assert!(scaled.contains("40"));
}

#[test]
fn scale_svg_path_implicit_lineto_after_move() {
    // After M, implicit repeats become L
    let path = "M10,20 30,40";
    let scaled = scale_svg_path_data(path, 2.0);
    assert_eq!(scaled, "M20 40 60 80");
}
