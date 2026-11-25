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
fn accent_is_centred_on_the_attachment_point() {
    // Values of the embedded font: attachment point, advance + correction.
    for (ch, at, width) in [
        ('f', 0.464, 0.580),
        ('d', 0.442, 0.544),
        ('T', 0.398, 0.732),
    ] {
        let base = LayoutBox::glyph(ch, 2.0, true);
        assert!((base.width - 2.0 * width).abs() < 1e-9, "{ch}");
        assert!(
            (accent_skew(&base) - (2.0 * at - width)).abs() < 1e-9,
            "{ch}"
        );
    }
    // Not for text faces or several characters.
    assert_eq!(
        accent_skew(&LayoutBox::glyph_font('f', 1.0, false, FontId::RomanBold)),
        0.0
    );
    assert_eq!(accent_skew(&LayoutBox::kern(1.0)), 0.0);
}

/// The spacing of atoms, checked against XeTeX with Latin Modern Math.
#[test]
fn atoms_are_spaced_as_in_tex() {
    use MathStyle::{Display, Text};
    for (latex, style, width) in [
        // A `/` is ordinary, a `:` a relation, `\\colon` has its own space.
        ("a/b", Text, 1.472),
        ("\\pi/2", Text, 1.595),
        ("(a)/(b)", Text, 3.028),
        ("a:b", Text, 1.806),
        ("f:A\\to B", Text, 4.503),
        ("x:=y", Text, 2.702),
        ("a{:}b", Text, 1.250),
        ("a\\mathbin{:}b", Text, 1.694),
        ("a\\colon b", Text, 1.694),
        ("a\\colon", Text, 1.251),
        ("\\colon b", Text, 1.165),
        // Groups, fractions, boxes and accents are ordinary.
        ("2{x+y}z", Text, 3.307),
        ("f{(x)}", Text, 1.930),
        ("2\\frac{a}{b}c", Display, 1.727),
        ("\\frac ab\\frac cd", Display, 1.553),
        ("a=\\frac bc", Display, 2.561),
        ("a\\hat{=}b", Text, 1.750),
        // Dots on the line are inner.
        ("a\\ldots b", Text, 2.142),
        ("a\\cdots b", Text, 2.081),
        ("a,\\dots,b", Text, 2.865),
        ("(\\ldots)", Text, 1.615),
        // Spaces are no atoms.
        ("a\\,:b", Text, 1.972),
        ("a=\\,=b", Text, 3.250),
        ("\\,-b", Text, 1.388),
        ("(\\,-b)", Text, 2.166),
        ("a\\,-b", Text, 2.361),
        ("\\mathbf{a\\,=b}", Text, 2.698),
        ("\\sin\\,x", Text, 2.133),
        // The operand of a big operator is the next atom.
        ("\\int x\\,dx", Display, 3.020),
        ("\\int\\!\\int", Display, 1.998),
        ("\\sum\\nolimits\\,x", Display, 2.349),
        ("\\sum(x)", Display, 1.444 + 1.350),
        ("\\int\\int x\\,dx", Display, 3.020 + 0.999 + 1.0 / 6.0),
        ("a=\\kern1em=b", Text, 4.0833),
        ("2\\boxed{x}y", Text, 0.5 + 1.252 + 0.518),
        ("x=!y", Text, 2.424),
        ("n!\\sin x", Text, 3.011),
        // A binary operator nothing follows is ordinary.
        ("a+", Text, 1.307),
        ("a+)", Text, 1.696),
        ("a+=b", Text, 3.084),
        ("a+,b", Text, 2.195),
        ("a+\\,)", Text, 1.863),
        ("\\mathrm{a+}", Text, 0.5 + 0.778),
        ("\\left(a+\\right)", Text, 2.085),
        ("\\left(a+\\,\\right)", Text, 2.085 + 1.0 / 6.0),
        ("\\left(a+\\middle|b\\right)", Text, 2.806),
        ("a+\\displaystyle b", Text, 2.194),
        // Braces are operators, an overline is ordinary.
        ("a\\underbrace{x}b", Text, 1.877),
        ("a\\overbrace{x}b", Text, 1.877),
        ("a\\overbracket{x}b", Text, 1.877),
        ("\\underbrace{x}_n+y", Display, 2.035),
        ("a\\overline{+}b", Text, 1.750),
    ] {
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, style, &default_options());
        assert!((root.width - width).abs() < 2e-3, "{latex}: {}", root.width);
        assert_arena_heap_equivalent(latex, style);
    }
}

/// `\\fboxsep` and `\\fboxrule`, as in TeX; a `\\colorbox` has no frame.
#[test]
fn boxes_have_the_padding_of_tex() {
    for (latex, width, height, depth) in [
        ("\\boxed{x}", 1.252, 0.771, 0.340),
        ("\\fcolorbox{red}{blue}{$x$}", 1.252, 0.771, 0.340),
        ("\\colorbox{red}{$x$}", 1.172, 0.731, 0.300),
    ] {
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!((root.width - width).abs() < 1e-6, "{latex}: {}", root.width);
        assert!((root.height - height).abs() < 1e-6, "{latex}");
        assert!((root.depth - depth).abs() < 1e-6, "{latex}");
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let options = default_options();
        let svg = crate::render::render_to_svg(&expr, &options).unwrap();
        assert_eq!(
            svg.contains("stroke="),
            !latex.starts_with("\\colorbox"),
            "{svg}"
        );
        // The frame is inside the box.
        if latex.starts_with("\\boxed") {
            assert!(
                svg.contains(r#"<rect x=".32" y=".32" width="19.392""#),
                "{svg}"
            );
        }
    }
    // The colour of a cell takes no more space than before.
    let cell = "\\begin{array}{c}\\cellcolor{red}x\\end{array}";
    assert_arena_heap_equivalent(cell, MathStyle::Text);
    let expr = crate::from_latex(cell).unwrap().expr();
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!((root.width - 1.772).abs() < 1e-6, "{}", root.width);
}

/// Delimiters and radicals are the font's glyphs at their own size, as in
/// XeTeX with Latin Modern Math.
#[test]
fn fences_and_radicals_are_those_of_the_font() {
    use MathStyle::{Display, Text};
    for (latex, style, width, height, depth) in [
        ("\\left(x\\right)", Text, 1.350, 0.748, 0.248),
        ("\\left[x\\right]", Text, 1.128, 0.750, 0.250),
        ("\\left|x\\right|", Text, 1.128, 0.750, 0.250),
        ("\\left.x\\right|", Text, 0.970, 0.750, 0.250),
        ("\\left(x\\right.", Text, 1.081, 0.748, 0.248),
        ("\\left(\\frac ab\\right)", Display, 1.963, 1.146, 0.686),
        ("\\left/x\\right\\backslash", Text, 1.572, 0.750, 0.250),
        (
            "\\left(\\begin{matrix}a\\\\b\\\\c\\\\d\\end{matrix}\\right)",
            Text,
            2.294,
            2.650,
            2.150,
        ),
        (
            "\\begin{pmatrix}a&b\\end{pmatrix}",
            Text,
            2.816,
            0.850,
            0.350,
        ),
        ("\\binom nk", Display, 2.072, 1.446, 0.946),
        ("\\genfrac[]{0pt}{}{a}{b}", Display, 1.585, 1.450, 0.950),
        ("\\sqrt x", Display, 1.405, 0.849, 0.191),
        ("\\sqrt x", Text, 1.405, 0.801, 0.239),
        ("\\sqrt{b}", Text, 1.276, 0.9325, 0.1075),
        ("\\sqrt{}", Text, 0.833, 0.585, 0.455),
        ("\\sqrt{x+y}", Text, 3.145, 0.780, 0.260),
        ("\\sqrt{\\sqrt{x}}", Text, 2.405, 0.966, 0.274),
        ("\\sqrt{\\frac ab}", Display, 1.769, 1.545, 0.895),
    ] {
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, style, &default_options());
        assert!((root.width - width).abs() < 2e-3, "{latex}: {}", root.width);
        assert!(
            (root.height - height).abs() < 2e-3,
            "{latex}: {}",
            root.height
        );
        assert!((root.depth - depth).abs() < 2e-3, "{latex}: {}", root.depth);
        assert_arena_heap_equivalent(latex, style);
        // In a script everything is as much smaller.
        assert_arena_heap_equivalent(&format!("x^{{{latex}}}"), style);
    }
    // The vinculum's top is the surd's.
    let expr = crate::from_latex("\\sqrt{x}").unwrap().expr();
    let root = layout_expr(&expr, Text, &default_options());
    let BoxKind::HBox { children } = &root.kind else {
        panic!("{root:?}")
    };
    let BoxKind::HBox { children } = &children[0].layout.kind else {
        panic!("{root:?}")
    };
    let top = |child: &PositionedBox| child.dy + child.layout.height;
    assert!((top(&children[0]) - top(&children[1])).abs() < 1e-9);
    assert!((children[1].layout.height + children[1].layout.depth - 0.04).abs() < 1e-9);
    // A superscript in a cramped display is as low as in text.
    let height = |latex: &str, style| {
        assert_arena_heap_equivalent(latex, style);
        let expr = crate::from_latex(latex).unwrap().expr();
        layout_expr(&expr, style, &default_options()).height
    };
    assert!((height("\\sqrt{x^2}", Display) - 1.012).abs() < 2e-3);
    assert!(height("x^2", Display) > height("x^2", Text));
}

/// The bar of a fraction is as wide as the wider part, inside the null
/// delimiters, and on the axis.
#[test]
fn the_bar_of_a_fraction_is_inside_its_null_delimiters() {
    let expr = crate::from_latex("\\frac{m}{s}").unwrap().expr();
    let root = layout_expr(&expr, MathStyle::Display, &default_options());
    let BoxKind::HBox { children } = &root.kind else {
        panic!("{root:?}")
    };
    let BoxKind::VBox { children } = &children[0].layout.kind else {
        panic!("{root:?}")
    };
    let bar = children
        .iter()
        .find(|child| matches!(child.layout.kind, BoxKind::Rule { .. }))
        .unwrap();
    assert!((bar.dx - 0.12).abs() < 1e-9);
    let middle = bar.dy + (bar.layout.height - bar.layout.depth) / 2.0;
    assert!((middle - 0.25).abs() < 1e-9);
    assert!((bar.layout.width - 0.878).abs() < 1e-9);
    assert!((root.width - 0.878 - 0.24).abs() < 1e-9);
    assert_arena_heap_equivalent("\\frac{m}{s}", MathStyle::Display);
}

/// A bare integral sign as the base of a script, as other readers give it.
#[test]
fn a_display_integral_has_the_correction_of_its_variant() {
    let base = || Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into()));
    let expr = MathExpr::inline(vec![MathNode::Subscript {
        base: base(),
        sub: Box::new(MathNode::Number("2".into())),
    }]);
    let root = layout_expr(&expr, MathStyle::Display, &default_options());
    assert!(
        (root.width - 0.408 - 0.3983 - 0.05).abs() < 1e-9,
        "{}",
        root.width
    );
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!(
        (root.width - 0.333 - 0.3983 - 0.05).abs() < 1e-9,
        "{}",
        root.width
    );
}

/// The surd of a script is the glyph of the script's size.
#[test]
fn surds_are_chosen_at_the_size_of_their_text() {
    let expr = crate::from_latex("x^{\\sqrt{y}}").unwrap().expr();
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    let text = format!("{root:?}");
    let at = text.find("Delimiter").unwrap();
    assert!(text[at..].contains("scale: 0.7 "), "{}", &text[at..]);
}

/// A base of one character is that character, whatever it is wrapped in.
#[test]
fn wrapped_characters_take_scripts_and_accents_like_bare_ones() {
    let width = |latex: &str| {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        layout_expr(&expr, MathStyle::Text, &default_options()).width
    };
    for (latex, expected) in [
        ("\\mathfrak{K}_1", 0.668 + 0.448),
        ("\\mathfrak{K}^2", 0.718 + 0.448),
        ("\\mathnormal{V}_1", 0.583 + 0.448),
        ("\\mathord{V}_1", 0.583 + 0.448),
        ("\\mathord{\\vec V}_1", 0.583 + 0.448),
        ("\\mathord{\\mathfrak K}_1", 0.668 + 0.448),
        ("\\vec{\\mathfrak{K}}_1", 0.668 + 0.448),
        ("\\bar{\\bar V}_1", 0.583 + 0.448),
        // As in TeX and KaTeX, not a coloured letter or a longer group.
        ("\\textcolor{red}{V}_1", 0.797 + 0.448),
        ("\\mathfrak{KK}_1", 2.0 * 0.718 + 0.448),
        // A bold letter is the Math font's.
        ("\\boldsymbol{V}", 0.913),
        // A word has the correction of its last letter only.
        ("\\mathit{ff}", 0.490 + 0.580),
        ("\\mathit{f2}", 0.580 + 0.5),
        ("\\mathcal{FF}", 2.0 * 0.904),
    ] {
        let actual = width(latex);
        assert!((actual - expected).abs() < 2e-3, "{latex}: {actual}");
    }
    let offset = |latex: &str| {
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        let BoxKind::HBox { children } = &root.kind else {
            panic!("{root:?}")
        };
        let BoxKind::HBox { children } = &children[0].layout.kind else {
            panic!("{root:?}")
        };
        (children[1].dx, children[1].layout.width)
    };
    assert_eq!(offset("\\bar{\\bar V}"), offset("\\bar V"));
    assert_eq!(offset("\\hat{\\mathord{V}}"), offset("\\hat V"));
}

/// Numerators in text are spaced as scripts are, as in XeTeX.
#[test]
fn small_fractions_are_spaced_as_scripts() {
    for (latex, expected) in [
        ("\\frac{a+b}{c-d}", 1.573),
        ("\\frac{a+b}{c=d}", 1.573),
        ("\\frac{\\sin x}{\\log_2 x}", 2.223),
        ("\\frac{1}{2+\\frac{1}{3+\\frac{1}{x}}}", 2.788),
        ("x^{\\frac{2}{k+1}}", 1.970),
        ("\\displaystyle\\frac{a+b}{c-d}", 2.464),
    ] {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        let actual = layout_expr(&expr, MathStyle::Text, &default_options()).width;
        assert!((actual - expected).abs() < 2e-3, "{latex}: {actual}");
    }
}

/// An invisible operator is drawn as nothing and is no atom.
#[test]
fn invisible_operators_have_no_space() {
    let width = |nodes: &str| {
        let expr = crate::from_mathml(&format!("<math>{nodes}</math>"))
            .unwrap()
            .expr();
        let options = default_options();
        let compact = crate::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
        let mut arena = LayoutArena::new();
        assert_eq!(
            crate::render::render_to_svg(&expr, &options).unwrap(),
            crate::render::render_to_svg_compact_arena(&compact, &options, &mut arena).unwrap()
        );
        layout_expr(&expr, MathStyle::Text, &options).width
    };
    for (before, after) in [
        ("<mo>sin</mo>", "<mo>(</mo><mi>x</mi><mo>)</mo>"),
        ("<mo>sin</mo>", "<mi>x</mi>"),
        ("<mi>a</mi><mo>+</mo>", "<mi>x</mi>"),
        ("<mi>a</mi><mo>+</mo>", "<mo>=</mo>"),
        ("<mi>a</mi><mo>+</mo>", ""),
    ] {
        let plain = width(&format!("{before}{after}"));
        for entity in ["2061", "2062", "2063", "2064"] {
            let invisible = width(&format!("{before}<mo>&#x{entity};</mo>{after}"));
            assert!(
                (invisible - plain).abs() < 1e-9,
                "{before}{entity}{after}: {invisible}, not {plain}"
            );
        }
    }
}

/// Labels above and below are placed by Rule 13a, braces and lines are not.
#[test]
fn labels_are_as_far_away_as_limits() {
    for (latex, height, depth) in [
        ("\\overset{a}{=}", 0.976, 0.0),
        ("\\underset{a}{=}", 0.367, 0.707),
        ("\\overset{p}{X}", 1.339, 0.0),
        ("\\underset{T}{y}", 0.431, 0.933),
        ("\\overset{a}{\\underset{b}{X}}", 1.292, 0.759),
        ("\\xrightarrow{f}", 1.358, 0.011),
        ("\\xrightarrow[g]{f}", 1.358, 0.854),
        ("\\xrightarrow{abcdefgh}", 1.307, 0.0),
        ("\\xrightarrow[abcdefgh]{f}", 1.307, 0.902),
        ("\\underset{abcdefgh}{\\to}", 0.46, 0.902),
        ("\\underset{\\lim}{X}", 0.683, 0.752),
        ("x^{\\underset{a}{b}}", 1.139, 0.0),
        ("\\underbrace{ab}_{n}", 0.694, 1.355),
        ("\\overbrace{ab}^{n}", 2.051, 0.0),
        ("\\underbrace{ab}", 0.694, 0.648),
        ("\\overline{ab}", 0.894, 0.0),
    ] {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        assert!(
            (root.height - height).abs() < 2e-3,
            "{latex}: {}",
            root.height
        );
        assert!((root.depth - depth).abs() < 2e-3, "{latex}: {}", root.depth);
    }
}

/// A symbol in braces is an ordinary atom, as in XeTeX.
#[test]
fn symbols_in_braces_have_no_space() {
    for (latex, expected) in [
        ("1{,}000", 2.278),
        ("a{+}b", 1.750),
        ("a{=}b", 1.750),
        ("a{\\leq}b", 1.750),
        ("a{\\not=}b", 1.750),
        ("a{=}{=}b", 2.528),
        ("a{=}^2b", 2.198),
        ("-{+}x", 2.128),
        ("a{;}b", 1.250),
        ("a=b", 2.306),
        ("\\begin{array}{rcl}a&{=}&b\\end{array}", 4.750),
    ] {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let read = crate::from_latex(latex).unwrap();
        let width = |read: &crate::ConvertResult| {
            layout_expr(&read.expr(), MathStyle::Text, &default_options()).width
        };
        assert!(
            (width(&read) - expected).abs() < 2e-3,
            "{latex}: {}",
            width(&read)
        );
        let written = read.to_latex().unwrap();
        let again = crate::from_latex(&written).unwrap();
        assert!(
            (width(&again) - expected).abs() < 2e-3,
            "{latex}: {written}"
        );
        assert_eq!(again.to_latex().unwrap(), written, "{latex}");
    }
    for (latex, expected) in [
        ("1{,}000", "1\\mathord{,}000"),
        ("a{=}b", "a\\mathord{=}b"),
        // Alone, or as an operand, the braces only delimit it.
        ("{=}", "="),
        ("x^{{+}}", "x^+"),
        ("\\sum{+}", "\\sum +"),
        ("\\frac{{=}}{a}", "\\frac{=}{a}"),
    ] {
        let written = crate::from_latex(latex).unwrap().to_latex().unwrap();
        assert_eq!(written, expected);
    }
}

/// Scripts are drawn with the font's designs for them (`ssty`), as in XeTeX.
#[test]
fn scripts_have_the_designs_of_the_font() {
    for (latex, expected) in [
        ("x^2", 1.020),
        ("x_{12}", 1.419),
        ("x^{y^{z^w}}", 1.983),
        ("x_{\\max}", 2.098),
        ("x_{\\text{a b}}", 1.732),
        ("x_{\\mathrm{f}}", 0.870),
        ("\\mathbf{f}_{\\mathbf f}", 0.680),
        ("x_{\\mathbf{ab}}", 1.565),
        ("x_{\\mathcal{F}}", 1.316),
        ("x^{\\partial f}", 1.528),
        ("x''", 1.075),
        ("f'''", 1.251),
        ("\\tfrac{a}{b}", 0.674),
        ("\\vec{v}_{\\hat T}", 1.108),
        // No design of its own.
        ("x^+", 1.167),
    ] {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        let actual = layout_expr(&expr, MathStyle::Text, &default_options()).width;
        assert!((actual - expected).abs() < 2e-3, "{latex}: {actual}");
    }
    let outlines = |latex: &str| {
        let svg = crate::from_latex(latex).unwrap().to_svg().unwrap();
        svg.matches("<path id=").count()
    };
    // One outline a size (and the letter of the base).
    assert_eq!(outlines("x^{x^x}"), 4);
    assert_eq!(outlines("x^xx^x"), 3);
}

/// A left-aligned column of `aligned` starts with amsmath's `{}`, as in XeTeX;
/// those of `array`, matrices and `cases` do not.
#[test]
fn left_columns_of_aligned_start_with_an_ordinary_atom() {
    let width = |latex: &str| {
        assert_arena_heap_equivalent(latex, MathStyle::Display);
        let expr = crate::from_latex(latex).unwrap().expr();
        layout_expr(&expr, MathStyle::Display, &default_options()).width
    };
    for (latex, expected) in [
        ("\\begin{aligned}a&=b\\\\c&=d\\end{aligned}", 2.407),
        ("\\begin{split}a&=b\\\\c&=d\\end{split}", 2.407),
        ("\\begin{aligned}a&+b\\\\&-c\\end{aligned}", 2.209),
        ("\\begin{aligned}a&\\sin x\\end{aligned}", 2.662),
        ("\\begin{aligned}a&,b\\end{aligned}", 1.417),
        ("\\begin{aligned}a=&b\\end{aligned}", 2.028),
        ("\\begin{aligned}&\\end{aligned}", 0.0),
        ("\\begin{array}{rl}a&=b\\end{array}", 4.028),
        ("\\begin{matrix}a&=b\\end{matrix}", 3.028),
        ("\\begin{cases}a&=b\\end{cases}", 3.772),
        ("\\begin{gathered}=a\\end{gathered}", 1.585),
    ] {
        let actual = width(latex);
        assert!((actual - expected).abs() < 2e-3, "{latex}: {actual}");
    }
    // Its MathML is laid out alike.
    let mathml = crate::from_latex("\\begin{aligned}a&=b\\end{aligned}").unwrap();
    let read = crate::from_mathml(&mathml.to_mathml().unwrap())
        .unwrap()
        .expr();
    let drawn = layout_expr(&read, MathStyle::Display, &default_options()).width;
    assert!((drawn - 2.306).abs() < 2e-3, "{drawn}");
}

/// An equals sign is drawn wider for a label wider than two of it.
#[test]
fn an_equals_sign_stretches_past_two_of_it() {
    let width = |label: f64| extensible_arrow_width('=', label, 1.0);
    assert_eq!(width(0.778), None);
    assert_eq!(width(1.556), None);
    assert!((width(1.557).unwrap() - 2.257).abs() < 1e-9);
    assert!((extensible_arrow_width('=', 0.8, 0.5).unwrap() - 1.15).abs() < 1e-9);
    assert_eq!(extensible_arrow_width('→', 0.455, 1.0), None);
    assert!((extensible_arrow_width('→', 0.456, 1.0).unwrap() - 1.001).abs() < 1e-9);
}

/// Trees only other readers and the API build: a class atom, numbers and
/// declarations in a face.
#[test]
fn atoms_and_faces_of_built_trees() {
    let id = |text: &str| MathNode::Identifier(text.into());
    let plus = || MathNode::Symbol(SymbolKind::BinaryOp, "+".into());
    let close = || MathNode::Symbol(SymbolKind::Close, ")".into());
    let width = |nodes: Vec<MathNode>| {
        let options = default_options();
        let expr = MathExpr::inline(nodes);
        let compact = crate::ast::compact::CompactExpr::from_math_expr(&expr).unwrap();
        let mut arena = LayoutArena::new();
        let heap = crate::render::render_to_svg(&expr, &options).unwrap();
        let other =
            crate::render::render_to_svg_compact_arena(&compact, &options, &mut arena).unwrap();
        assert_eq!(heap, other);
        layout_expr(&expr, MathStyle::Text, &options).width
    };
    let close_to = |actual: f64, expected: f64| {
        assert!((actual - expected).abs() < 2e-3, "{actual}, not {expected}");
    };
    // Scripts of an ordinary atom of one letter; another class is a box.
    let scripted = |atom_type| {
        width(vec![MathNode::Subscript {
            base: Box::new(MathNode::MathAtom {
                atom_type,
                content: Box::new(id("V")),
            }),
            sub: Box::new(MathNode::Number("1".into())),
        }])
    };
    close_to(scripted(MathAtomType::Ord), 0.583 + 0.448);
    // Declarations do not hide what follows an operator.
    let switch = || MathNode::StyleSwitch(1);
    close_to(width(vec![id("a"), plus(), switch(), close()]), 1.696);
    close_to(
        width(vec![id("a"), plus(), MathNode::SizeSwitch(5), close()]),
        1.307 + 0.389 * 1.2,
    );
    close_to(
        width(vec![id("a"), plus(), switch(), id("b")]),
        0.529 + 0.778 + 0.443 + 8.0 / 18.0,
    );
    // Operators in a face, before a closing, at its end and before a letter.
    let styled = |nodes| width(vec![MathNode::Styled(TextStyle::Normal, nodes)]);
    let bare = styled(vec![id("a"), id("b")]);
    close_to(
        styled(vec![id("a"), plus(), close()]),
        styled(vec![id("a")]) + 0.778 + 0.389,
    );
    close_to(styled(vec![id("a"), plus()]), styled(vec![id("a")]) + 0.778);
    close_to(
        styled(vec![id("a"), plus(), id("b")]),
        bare + 0.778 + 8.0 / 18.0,
    );
    // A number inside a composite in a face with Math letters is drawn.
    let number = width(vec![MathNode::Styled(
        TextStyle::DoubleStruck,
        vec![MathNode::Fraction {
            kind: FractionKind::Display,
            num: Box::new(MathNode::Number("12".into())),
            den: Box::new(id("R")),
        }],
    )]);
    assert!(number > 1.0, "{number}");
}

/// A delimiter taller than every glyph of a font without an assembly for it.
#[test]
fn tall_angle_brackets_are_the_largest_glyph() {
    let latex = "\\left\\langle\\begin{matrix}a\\\\b\\\\c\\\\d\\\\e\\end{matrix}\\right\\rangle";
    assert_arena_heap_equivalent(latex, MathStyle::Text);
    let expr = crate::from_latex(latex).unwrap().expr();
    let root = layout_expr(&expr, MathStyle::Text, &default_options());
    assert!((root.width - 2.360).abs() < 2e-3, "{}", root.width);
}

/// `\overrightarrow` and its like are centred on the base and as wide.
#[test]
fn wide_accents_have_the_width_of_the_base() {
    for (latex, centred) in [
        ("\\overrightarrow{T}", true),
        ("\\widehat{T}", false),
        ("\\widetilde{T}", false),
    ] {
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        let BoxKind::HBox { children } = &root.kind else {
            panic!("{root:?}")
        };
        let BoxKind::HBox { children } = &children[0].layout.kind else {
            panic!("{root:?}")
        };
        let [base, accent] = &children[..] else {
            panic!("{root:?}")
        };
        assert!(
            (accent.layout.width - base.layout.width).abs() < 1e-9,
            "{latex}"
        );
        assert_eq!(accent.dx.abs() < 1e-9, centred, "{latex}");
    }
}

/// An accent over one character has its own width.
#[test]
fn accents_of_characters_do_not_stretch() {
    for command in ["hat", "check", "tilde", "bar", "vec"] {
        let accent = |base: &str| {
            let latex = format!("\\{command}{{{base}}}");
            assert_arena_heap_equivalent(&latex, MathStyle::Text);
            let expr = crate::from_latex(&latex).unwrap().expr();
            let root = layout_expr(&expr, MathStyle::Text, &default_options());
            let BoxKind::HBox { children } = &root.kind else {
                panic!("{root:?}")
            };
            let BoxKind::HBox { children } = &children[0].layout.kind else {
                panic!("{root:?}")
            };
            children[1].layout.width
        };
        assert!((accent("M") - 0.45).abs() < 1e-9, "{command}");
        assert!((accent("i") - 0.45).abs() < 1e-9, "{command}");
        assert!(accent("AB") > 1.0, "{command}");
    }
}

/// A label that fits on an arrow or an equals sign does not stretch it.
#[test]
fn labels_stretch_what_they_do_not_fit_on() {
    let drawn = |latex: &str| {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, MathStyle::Text, &default_options());
        (root.width, format!("{root:?}").contains("SvgPath"))
    };
    // As in XeTeX with Latin Modern Math.
    for (latex, width) in [
        ("\\overset{!}{=}", 0.778),
        ("\\stackrel{?}{=}", 0.778),
        ("\\overset{(a)}{=}", 0.979),
        ("\\overset{\\text{def}}{=}", 1.045),
        // As wide as two signs.
        ("\\overset{\\kern1.555em}{=}", 1.555),
        ("\\xlongequal[a]{b}", 0.778),
        ("\\overset{f}{\\rightarrow}", 1.0),
        ("\\xrightarrow{f}", 1.0),
        ("\\xrightarrow[g]{f}", 1.0),
        ("\\underset{i}{\\to}", 1.0),
        ("\\overset{i}{\\mapsto}", 0.977),
    ] {
        let (actual, stretched) = drawn(latex);
        assert!((actual - width).abs() < 2e-3, "{latex}: {actual}");
        assert!(!stretched, "{latex}");
    }
    let label = drawn("\\scriptstyle abcdefghij").0;
    for latex in [
        "\\xrightarrow{abcdefghij}",
        "\\xrightarrow[abcdefghij]{f}",
        "\\xrightarrow[f]{abcdefghij}",
        "\\underset{abcdefghij}{\\to}",
        "\\xlongequal{abcdefghij}",
    ] {
        let (actual, stretched) = drawn(latex);
        let padding = if latex.contains("equal") { 0.7 } else { 0.545 };
        assert!((actual - label - padding).abs() < 1e-6, "{latex}: {actual}");
        assert!(stretched, "{latex}");
    }
}

/// A `\\middle` closes what is before it and opens what is after.
#[test]
fn middle_delimiters_have_no_space() {
    let width = |latex: &str| {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        let expr = crate::from_latex(latex).unwrap().expr();
        layout_expr(&expr, MathStyle::Text, &default_options()).width
    };
    let bar = width("\\left.a\\middle|b\\right.") - width("ab");
    // With two null delimiters.
    assert!((bar - 0.278 - 0.24).abs() < 1e-6, "{bar}");
    let relation = width("\\left.a=\\middle|b\\right.") - width("\\left.a=b\\right.");
    assert!((relation - (0.278 - 5.0 / 18.0)).abs() < 1e-6, "{relation}");
}

/// TeXBook Rules 12, 17 and 18, checked against XeTeX with Latin Modern Math.
#[test]
fn italic_correction_follows_the_texbook() {
    let place = |latex: &str, style| {
        assert_arena_heap_equivalent(latex, style);
        let expr = crate::from_latex(latex).unwrap().expr();
        let root = layout_expr(&expr, style, &default_options());
        let mut glyphs = Vec::new();
        fn walk(b: &LayoutBox, x: f64, out: &mut Vec<(char, f64)>) {
            match &b.kind {
                BoxKind::Glyph { ch, .. } => out.push((*ch, x)),
                BoxKind::TextRun { text, .. } => {
                    out.extend(text.chars().next().map(|ch| (ch, x)));
                },
                BoxKind::HBox { children } | BoxKind::VBox { children } => {
                    for child in children {
                        walk(&child.layout, x + child.dx, out);
                    }
                },
                _ => {},
            }
        }
        walk(&root, 0.0, &mut glyphs);
        (root.width, glyphs)
    };
    let check = |latex: &str, style, width: f64, last: f64| {
        let (w, glyphs) = place(latex, style);
        assert!((w - width).abs() < 1e-6, "{latex}: width {w}, not {width}");
        let x = glyphs.last().unwrap().1;
        assert!((x - last).abs() < 1e-6, "{latex}: last at {x}, not {last}");
    };
    use MathStyle::{Display, Text};
    check("f", Text, 0.580, 0.0);
    check("ff", Text, 1.160, 0.580);
    check("f)", Text, 0.969, 0.580);
    check("\\frac{f}{T}", Text, 0.818 * 0.7 + 0.24, 0.12);
    // A superscript follows the correction, primes too; a subscript does not.
    check("f^2", Text, 1.0283, 0.580);
    check("f'", Text, 0.9149, 0.580);
    check("f_2", Text, 0.9383, 0.490);
    check("T_2^2", Text, 0.584 + 0.148 + 0.4483, 0.584);
    check("V_{,}", Text, 0.583 + 0.339 * 0.7 + 0.05, 0.583);
    check("{ff}_2", Text, 1.160 + 0.4483, 1.160);
    // An accented character takes its scripts where the character does.
    check("\\vec{V}_2", Text, 0.583 + 0.4483, 0.583);
    check("\\hat{T}^2", Text, 0.732 + 0.4483, 0.732);
    check("\\Gamma_2", Text, 0.625 + 0.4483, 0.625);
    check("\\mathbf{f}_2", Text, 0.351 + 0.4483, 0.351);
    // Text has no corrections.
    check("\\textit{ff}", Text, 0.980, 0.0);
    check("\\text{∫}", Text, 0.665, 0.0);
    // The subscript of an integral is under its slope.
    check("\\int_2", Display, 0.408 + 0.4483, 0.408);
    check("\\int_2", Text, 0.333 + 0.4483, 0.333);
    check("\\int_2^2", Display, 0.999 + 0.4483, 0.408);
    check(
        "\\oint_2^2 f",
        Text,
        0.665 + 0.4483 + 1.0 / 6.0 + 0.580,
        0.665 + 0.4483 + 1.0 / 6.0,
    );
    check(
        "\\int\\limits_2",
        Display,
        0.999,
        (0.999 - 0.3983) / 2.0 - 0.591 / 2.0,
    );
}

#[test]
fn bracket_paths_span_the_operand_and_keep_their_vertical_gap() {
    fn bracket_path(root: &LayoutBox) -> (&str, f64, f64) {
        let BoxKind::HBox { children } = &root.kind else {
            panic!("missing expression row")
        };
        let BoxKind::VBox { children } = &children[0].layout.kind else {
            panic!("missing bracket stack")
        };
        let bracket = &children[1];
        let BoxKind::SvgPath(path) = &bracket.layout.kind else {
            panic!("expected a scalable bracket path")
        };
        assert!(bracket.layout.width >= children[0].layout.width);
        (path, bracket.layout.width, bracket.dy)
    }
    for (command, mark, above) in [("overbracket", "⎴", true), ("underbracket", "⎵", false)] {
        let mut widths = Vec::new();
        for content in ["x", "abcdefg"] {
            let latex = format!(r"\{command}{{{content}}}");
            let expr = crate::from_latex(&latex).unwrap().expr();
            let [
                MathNode::Accent {
                    base, wide: true, ..
                },
            ] = &expr.nodes[..]
            else {
                panic!("expected a wide bracket: {latex}");
            };
            let base = base.clone();
            let symbol = Box::new(MathNode::Symbol(SymbolKind::Ordinary, mark.into()));
            let original = if above {
                MathNode::Overscript {
                    base,
                    over: symbol,
                    movable_limits: false,
                }
            } else {
                MathNode::Underscript {
                    base,
                    under: symbol,
                    movable_limits: false,
                }
            };
            let options = default_options();
            let actual = layout_expr(&expr, MathStyle::Text, &options);
            let expected =
                layout_expr(&MathExpr::inline(vec![original]), MathStyle::Text, &options);
            assert_eq!(bracket_path(&actual), bracket_path(&expected), "{latex}");
            widths.push(bracket_path(&actual).1);
            assert_arena_heap_equivalent(&latex, MathStyle::Text);
        }
        assert!(widths[1] > widths[0]);
    }
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
fn singleton_rows_preserve_inline_integral_limit_geometry() {
    let parse = |base: &str| {
        let xml = format!(
            r#"<math display="inline"><munderover>{base}<mi>a</mi><mi>b</mi></munderover></math>"#
        );
        crate::from_mathml(&xml).unwrap()
    };
    let integral = r#"<mo movablelimits="true">∫</mo>"#;
    let bare = parse(integral);
    let wrapped = parse(&format!("<mrow><mrow>{integral}</mrow></mrow>"));
    for style in [MathStyle::Text, MathStyle::Script] {
        let b = layout_expr(&bare.expr(), style, &default_options());
        let w = layout_expr(&wrapped.expr(), style, &default_options());
        assert_eq!((b.width, b.height, b.depth), (w.width, w.height, w.depth));
    }
    assert_eq!(bare.to_svg_text().unwrap(), wrapped.to_svg_text().unwrap());
}

#[test]
fn movable_large_operator_limits_follow_math_style_on_heap_and_arena() {
    fn movable_integral() -> MathNode {
        MathNode::UnderOver {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into())),
            under: Box::new(MathNode::Number("0".into())),
            over: Box::new(MathNode::Number("1".into())),
            movable_limits: true,
        }
    }

    let options = default_options();
    let inline = layout_expr(
        &MathExpr::inline(vec![movable_integral()]),
        MathStyle::Text,
        &options,
    );
    let script = layout_expr(
        &MathExpr::inline(vec![movable_integral()]),
        MathStyle::Script,
        &options,
    );
    let display = layout_expr(
        &MathExpr::inline(vec![movable_integral()]),
        MathStyle::Display,
        &options,
    );
    fn child_kind(root: &LayoutBox) -> &BoxKind {
        match &root.kind {
            BoxKind::HBox { children } => &children[0].layout.kind,
            _ => panic!("single-node expression should have an outer HBox"),
        }
    }
    assert!(matches!(child_kind(&inline), BoxKind::HBox { .. }));
    assert!(matches!(child_kind(&script), BoxKind::HBox { .. }));
    assert!(matches!(child_kind(&display), BoxKind::VBox { .. }));

    let fixed = layout_expr(
        &MathExpr::inline(vec![MathNode::UnderOver {
            base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into())),
            under: Box::new(MathNode::Number("0".into())),
            over: Box::new(MathNode::Number("1".into())),
            movable_limits: false,
        }]),
        MathStyle::Text,
        &options,
    );
    let ordinary = layout_expr(
        &MathExpr::inline(vec![MathNode::UnderOver {
            base: Box::new(MathNode::Identifier("x".into())),
            under: Box::new(MathNode::Number("0".into())),
            over: Box::new(MathNode::Number("1".into())),
            movable_limits: true,
        }]),
        MathStyle::Text,
        &options,
    );
    assert!(matches!(child_kind(&fixed), BoxKind::VBox { .. }));
    assert!(matches!(child_kind(&ordinary), BoxKind::VBox { .. }));

    let one_bound = |style| {
        layout_expr(
            &MathExpr::inline(vec![MathNode::Overscript {
                base: Box::new(MathNode::Symbol(SymbolKind::LargeOp, "∫".into())),
                over: Box::new(MathNode::Number("1".into())),
                movable_limits: true,
            }]),
            style,
            &options,
        )
    };
    assert!(matches!(
        child_kind(&one_bound(MathStyle::Script)),
        BoxKind::HBox { .. }
    ));
    assert!(matches!(
        child_kind(&one_bound(MathStyle::Display)),
        BoxKind::VBox { .. }
    ));

    use crate::reader::Reader;
    use crate::render::arena::{ArenaBoxKind, LayoutArena};
    let compact = crate::reader::mathml::MathmlReader::parse_compact(
        "<math><munderover><mo movablelimits=\"true\">∫</mo><mn>0</mn><mn>1</mn></munderover></math>",
        &crate::error::Config::default(),
    )
    .unwrap()
    .expr;
    let mut arena = LayoutArena::new();
    let text_builder = LayoutBuilder::new(MathStyle::Text, &options);
    let text_id = text_builder.layout(&mut arena, &compact);
    let text_child = match arena.get(text_id).kind {
        ArenaBoxKind::HBox { children } => arena.children(children)[0].id,
        _ => panic!("single-node arena expression should have an outer HBox"),
    };
    assert!(matches!(
        arena.get(text_child).kind,
        ArenaBoxKind::HBox { .. }
    ));
    arena.clear();
    let display_builder = LayoutBuilder::new(MathStyle::Display, &options);
    let display_id = display_builder.layout(&mut arena, &compact);
    let display_child = match arena.get(display_id).kind {
        ArenaBoxKind::HBox { children } => arena.children(children)[0].id,
        _ => panic!("single-node arena expression should have an outer HBox"),
    };
    assert!(matches!(
        arena.get(display_child).kind,
        ArenaBoxKind::VBox { .. }
    ));
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
            base_pt: 12.0,
            display_choice: false,
            styled_face: None,
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
            base_pt: 12.0,
            display_choice: false,
            styled_face: None,
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
            base_pt: 12.0,
            display_choice: false,
            styled_face: None,
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
fn named_operator_limits_follow_math_style() {
    let expr = MathExpr::inline(vec![MathNode::Subscript {
        base: Box::new(MathNode::Operator(OperatorAttr {
            name: "lim".into(),
            limit_style: LimitStyle::Default,
        })),
        sub: Box::new(MathNode::Identifier("x".into())),
    }]);
    let options = default_options();
    let display = layout_expr(&expr, MathStyle::Display, &options);
    let inline = layout_expr(&expr, MathStyle::Text, &options);
    assert!(
        display.height + display.depth > inline.height + inline.depth,
        "display limit should occupy more vertical extent"
    );
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
            base_pt: 12.0,
            display_choice: false,
            styled_face: None,
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
            base_pt: 12.0,
            display_choice: false,
            styled_face: None,
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
        base_pt: 12.0,
        display_choice: false,
        styled_face: None,
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
fn layout_named_operator_thin_space_uses_math_width() {
    let spaced = MathExpr::inline(vec![MathNode::Operator(OperatorAttr {
        name: "arg\u{2009}min".into(),
        limit_style: LimitStyle::Default,
    })]);
    let plain = MathExpr::inline(vec![MathNode::Operator(OperatorAttr {
        name: "argmin".into(),
        limit_style: LimitStyle::Default,
    })]);
    let spaced_width = layout_expr(&spaced, MathStyle::Text, &default_options()).width;
    let plain_width = layout_expr(&plain, MathStyle::Text, &default_options()).width;
    assert!(spaced_width > plain_width, "thin operator space was lost");
    assert!(
        (spaced_width - plain_width - 1.0 / 6.0).abs() < 0.02,
        "unexpected thin-space width: spaced={spaced_width}, plain={plain_width}"
    );
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
    // Both layout paths consume the declaration as a scoped row style.
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
    let heap_text_height =
        layout_expr(&text_result.expr.to_math_expr(), MathStyle::Text, &opts).height;

    // Text-mode with \displaystyle forcing display-size sum
    let display_result =
        crate::reader::latex::LatexReader::parse_compact("{\\displaystyle \\sum} x", &config)
            .expect("parse displaystyle sum");
    arena.clear();
    let display_id =
        LayoutBuilder::new(MathStyle::Text, &opts).layout(&mut arena, &display_result.expr);
    let display_height = arena.get(display_id).height;
    let heap_display_height =
        layout_expr(&display_result.expr.to_math_expr(), MathStyle::Text, &opts).height;

    // The \displaystyle version should be taller (display-size operator)
    assert!(
        display_height > text_height,
        "\\displaystyle sum should be taller than text sum: {} vs {}",
        display_height,
        text_height
    );
    assert!(heap_display_height > heap_text_height);
}

#[test]
fn equivalence_displaystyle() {
    // The heap and arena paths must agree when a row contains a declaration.
    assert_arena_heap_equivalent("{\\displaystyle \\sum_{i=0}^n} a_i", MathStyle::Text);
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
fn style_switches_match_heap_and_compact_layouts_in_nested_contexts() {
    use crate::render::arena::LayoutArena;

    fn assert_dimensions_match(expr: &MathExpr, style: MathStyle) {
        let options = default_options();
        let heap = layout_expr(expr, style, &options);
        let compact = CompactExpr::from_math_expr(expr).unwrap();
        let mut arena = LayoutArena::new();
        let id = LayoutBuilder::new(style, &options).layout(&mut arena, &compact);
        let compact = arena.get(id);
        assert!((heap.width - compact.width).abs() < 1e-9);
        assert!((heap.height - compact.height).abs() < 1e-9);
        assert!((heap.depth - compact.depth).abs() < 1e-9);
    }

    let fraction = || MathNode::Fraction {
        kind: FractionKind::Normal,
        num: Box::new(MathNode::Identifier("a".into())),
        den: Box::new(MathNode::Identifier("b".into())),
    };
    for (style, marker) in [
        (MathStyle::Display, 0),
        (MathStyle::Text, 1),
        (MathStyle::Script, 2),
        (MathStyle::ScriptScript, 3),
    ] {
        assert_dimensions_match(
            &MathExpr::inline(vec![MathNode::StyleSwitch(marker), fraction()]),
            MathStyle::Text,
        );
        assert_dimensions_match(
            &MathExpr::inline(vec![MathNode::Superscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sup: Box::new(MathNode::Row(vec![
                    MathNode::StyleSwitch(marker),
                    fraction(),
                ])),
            }]),
            style,
        );
    }

    let scoped = MathExpr::inline(vec![
        MathNode::Row(vec![MathNode::StyleSwitch(0), fraction()]),
        fraction(),
    ]);
    let root = layout_expr(&scoped, MathStyle::Text, &default_options());
    let BoxKind::HBox { children } = root.kind else {
        panic!("row layout should be horizontal");
    };
    assert_eq!(children.len(), 2);
    assert!(children[0].layout.height > children[1].layout.height);
    assert_dimensions_match(&scoped, MathStyle::Text);

    let array = MathNode::Array {
        columns: vec![ColumnAlign::Center, ColumnAlign::Center],
        rows: vec![vec![
            vec![MathNode::StyleSwitch(0), fraction()],
            vec![fraction()],
        ]],
        arraystretch: None,
        add_jot: false,
    };
    assert_dimensions_match(&MathExpr::inline(vec![array]), MathStyle::Text);
}

#[test]
fn mathchoice_selects_each_active_style_on_heap_and_compact_paths() {
    use crate::render::arena::LayoutArena;

    let branches = [
        MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        },
        MathNode::Identifier("t".into()),
        MathNode::Number("s".into()),
        MathNode::Identifier("q".into()),
    ];
    let choice = || MathNode::MathChoice {
        display: Box::new(branches[0].clone()),
        text: Box::new(branches[1].clone()),
        script: Box::new(branches[2].clone()),
        script_script: Box::new(branches[3].clone()),
    };
    for (style, expected) in [
        (MathStyle::Display, &branches[0]),
        (MathStyle::Text, &branches[1]),
        (MathStyle::Script, &branches[2]),
        (MathStyle::ScriptScript, &branches[3]),
    ] {
        let options = default_options();
        let selected = layout_expr(&MathExpr::inline(vec![choice()]), style, &options);
        let expected = layout_expr(&MathExpr::inline(vec![expected.clone()]), style, &options);
        assert_eq!(selected.width, expected.width);
        assert_eq!(selected.height, expected.height);
        assert_eq!(selected.depth, expected.depth);

        let compact = CompactExpr::from_math_expr(&MathExpr::inline(vec![choice()])).unwrap();
        let mut arena = LayoutArena::new();
        let id = LayoutBuilder::new(style, &options).layout(&mut arena, &compact);
        let compact = arena.get(id);
        assert_eq!(selected.width, compact.width);
        assert_eq!(selected.height, compact.height);
        assert_eq!(selected.depth, compact.depth);
    }
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
fn variable_limit_compounds_move_only_in_display() {
    use crate::reader::Reader;
    use crate::reader::latex::LatexReader;

    fn child_kind(root: &LayoutBox) -> &BoxKind {
        match &root.kind {
            BoxKind::HBox { children } => &children[0].layout.kind,
            _ => panic!("single-node expression should have an outer HBox"),
        }
    }

    let config = crate::error::Config::default();
    let opts = default_options();
    for source in [r"\varinjlim_i", r"\varlimsup_i"] {
        let expr = LatexReader::parse_compact(source, &config)
            .expect("variable-limit source should parse")
            .expr
            .to_math_expr();
        let display = layout_expr(&expr, MathStyle::Display, &opts);
        let inline = layout_expr(&expr, MathStyle::Text, &opts);
        let bare = crate::from_latex(source.strip_suffix("_i").unwrap())
            .unwrap()
            .expr();
        let bare_display = layout_expr(&bare, MathStyle::Display, &opts);
        let bare_inline = layout_expr(&bare, MathStyle::Text, &opts);
        assert!(
            (display.width - bare_display.width).abs() < 1e-8,
            "{source}"
        );
        assert!(display.depth > bare_display.depth, "{source}");
        assert!(inline.width > bare_inline.width, "{source}");
    }

    let literal = LatexReader::parse_compact(r"\underrightarrow{\lim}_i", &config)
        .expect("literal decorated limit should parse")
        .expr
        .to_math_expr();
    let display = layout_expr(&literal, MathStyle::Display, &opts);
    let inline = layout_expr(&literal, MathStyle::Text, &opts);
    assert!(matches!(child_kind(&display), BoxKind::HBox { .. }));
    assert!(matches!(child_kind(&inline), BoxKind::HBox { .. }));
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
    for glyph in ['∫', '∬', '∭', '⨌', '∮', '∯', '∰', '∲', '∳'] {
        let integral = MathNode::Symbol(SymbolKind::LargeOp, glyph.to_string());
        assert!(
            !wants_limits_above_below(&integral, MathStyle::Display),
            "{glyph}"
        );
        assert!(
            !wants_limits_above_below(&integral, MathStyle::Text),
            "{glyph}"
        );
    }
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

// ── `\big`-family delimiters ──────────────────────────────────────────

/// The first `Delimiter` box in `b` with the raise of its parent chain.
fn find_delimiter(b: &LayoutBox, dy: f64) -> Option<(f64, &LayoutBox)> {
    match &b.kind {
        BoxKind::Delimiter { .. } => Some((dy, b)),
        BoxKind::HBox { children } | BoxKind::VBox { children } => children
            .iter()
            .find_map(|c| find_delimiter(&c.layout, dy + c.dy)),
        BoxKind::Color { child, .. } => find_delimiter(&child.layout, dy + child.dy),
        _ => None,
    }
}

fn big_delimiter_layout(latex: &str) -> LayoutBox {
    let parsed = crate::latex::parse::parse(latex, &Default::default()).expect("parse failed");
    layout_expr(&parsed.expr, MathStyle::Text, &default_options())
}

#[test]
fn big_family_selects_tex_sized_variants() {
    use glyph_variants::{BigDelimiterGlyph, big_delimiter_glyph};
    // (glyph, expected variant advance per size: \big, \Big, \bigg, \Bigg);
    // `None` means the extensible assembly.
    let cases: [(char, [Option<f64>; 4]); 7] = [
        ('(', [Some(1.195), Some(1.793), Some(2.393), Some(2.991)]),
        ('[', [Some(1.201), Some(1.801), Some(2.401), Some(3.001)]),
        (
            '\u{27E8}',
            [Some(1.201), Some(1.801), Some(2.401), Some(3.001)],
        ),
        ('|', [Some(1.203), Some(1.735), Some(2.503), Some(3.005)]),
        ('/', [Some(1.311), Some(1.717), Some(2.249), Some(2.945)]),
        ('\u{2191}', [Some(1.349), None, None, None]),
        // `\lgroup`: the nearest variant, not the shorter 1.127 em one.
        (
            '\u{27EE}',
            [Some(1.229), Some(1.837), Some(2.445), Some(3.053)],
        ),
    ];
    for (ch, expected) in cases {
        for (size, want) in crate::ast::BIG_DELIM_SIZES.iter().zip(expected) {
            let got = big_delimiter_glyph(ch, size.scale.as_f64())
                .unwrap_or_else(|| panic!("{ch:?} has variants"));
            match (got, want) {
                (BigDelimiterGlyph::Variant(v), Some(want)) => assert!(
                    (v.advance_em - want).abs() < 0.002,
                    "\\{}{ch}: variant {:.3} em, want {want}",
                    size.command,
                    v.advance_em
                ),
                (BigDelimiterGlyph::Assembly, None) => {},
                (got, want) => panic!("\\{}{ch}: got {got:?}, want {want:?}", size.command),
            }
        }
    }
    // A glyph without MATH variants keeps the scaled fallback.
    assert!(big_delimiter_glyph('x', 1.2).is_none());
}

#[test]
fn big_family_heights_match_tex_sizes() {
    let axis = TexParams::default().axis_height;
    for (size, open, close) in [
        ("big", '(', ')'),
        ("Big", '[', ']'),
        ("bigg", '{', '}'),
        ("Bigg", '(', ')'),
    ] {
        let tex = crate::ast::BIG_DELIM_SIZES
            .iter()
            .find(|s| s.command == size)
            .unwrap()
            .scale
            .as_f64();
        for ch in [open, close] {
            let latex = format!(
                "\\{size}{}",
                if ch == '{' || ch == '}' {
                    format!("\\{ch}")
                } else {
                    ch.to_string()
                }
            );
            let root = big_delimiter_layout(&latex);
            let (dy, d) =
                find_delimiter(&root, 0.0).unwrap_or_else(|| panic!("{latex}: no Delimiter box"));
            let BoxKind::Delimiter { target_height, .. } = d.kind else {
                unreachable!()
            };
            let total = d.height + d.depth;
            // TeX's size1–size4 heights (1.2/1.8/2.4/3.0 em), within the
            // font's rounding.
            assert!(
                (total - tex).abs() < 0.012,
                "{latex}: height {total:.3}, TeX {tex}"
            );
            assert!(
                (target_height - total).abs() < 0.01,
                "{latex}: target {target_height:.3}"
            );
            // Centered on the math axis, as TeX centers delimiters.
            let center = dy + (d.height - d.depth) / 2.0;
            assert!(
                (center - axis).abs() < 1e-9,
                "{latex}: center {center:.4}, axis {axis}"
            );
        }
    }
}

#[test]
fn big_family_assembly_spans_the_tex_height() {
    let root = big_delimiter_layout(r"\Bigg\uparrow");
    let (_, d) = find_delimiter(&root, 0.0).expect("Delimiter box");
    let BoxKind::Delimiter {
        ch,
        target_height,
        scale,
    } = d.kind
    else {
        unreachable!()
    };
    assert_eq!(ch, '\u{2191}');
    assert_eq!(scale, 1.0);
    assert!((target_height - 3.0).abs() < 1e-9);
    assert!((d.height + d.depth - 3.0).abs() < 1e-9);
    // The emitters fall through to the assembly for this target.
    assert!(glyph_variants::delimiter_variant(ch, target_height).is_none());
    assert!(glyph_variants::vertical_assembly(ch, target_height).is_some());
}

#[test]
fn big_family_draws_the_variant_unscaled() {
    // The box is as wide as the variant glyph, not a scaled base glyph.
    let root = big_delimiter_layout(r"\Bigg(");
    let (_, d) = find_delimiter(&root, 0.0).expect("Delimiter box");
    let v = glyph_variants::delimiter_variant('(', 2.991).expect("size4 variant");
    assert!((d.width - v.h_adv_em).abs() < 1e-9, "width {}", d.width);
    assert!(d.width < 3.0 * metrics::glyph_metrics('(').width);
}

#[test]
fn big_null_delimiter_is_nulldelimiterspace_wide() {
    for latex in [r"\big.", r"\Biggl."] {
        let root = big_delimiter_layout(latex);
        assert!(
            (root.width - 0.12).abs() < 1e-9,
            "{latex}: width {}",
            root.width
        );
        assert!(find_delimiter(&root, 0.0).is_none());
    }
}

#[test]
fn scaled_delimiter_with_other_factor_keeps_scaling() {
    let content = MathNode::Symbol(SymbolKind::Open, "(".into());
    let ctx = LayoutCtx {
        style: MathStyle::Text,
        params: TexParams::default(),
        options: &default_options(),
        depth: 0,
        size_scale: 1.0,
        base_pt: 12.0,
        display_choice: false,
        styled_face: None,
    };
    let normal = layout_node(&content, &ctx);
    let scaled = layout_scaled(Rational::new(2, 1), &content, &ctx);
    assert!((scaled.width - 2.0 * normal.width).abs() < 1e-9);
    assert!(find_delimiter(&scaled, 0.0).is_none());
}

#[test]
fn big_family_arena_matches_heap() {
    for latex in [
        r"\bigl( \Bigl[ \biggl\{ \Biggl\langle x \Biggr\rangle \biggr\} \Bigr] \bigr)",
        r"\big| \Big\| \bigg/ \Bigg\backslash \big\uparrow \Bigg\uparrow \big.",
        r"x_{\big( \Bigg\uparrow}",
    ] {
        assert_arena_heap_equivalent(latex, MathStyle::Text);
        assert_arena_heap_equivalent(latex, MathStyle::Display);
    }
}

/// The `Delimiter` box, its parent-chain raise and its scale for `latex`
/// rendered at `style` with `size_scale`.
fn big_delimiter_at(latex: &str, size_scale: f64) -> (f64, LayoutBox) {
    let parsed = crate::latex::parse::parse(latex, &Default::default()).expect("parse failed");
    let options = default_options();
    let ctx = LayoutCtx {
        style: MathStyle::Text,
        params: TexParams::default(),
        options: &options,
        depth: 0,
        size_scale,
        base_pt: 12.0,
        display_choice: false,
        styled_face: None,
    };
    let root = layout_node(&MathNode::Row(parsed.expr.nodes.clone()), &ctx);
    let (dy, d) = find_delimiter(&root, 0.0).expect("Delimiter box");
    (dy, d.clone())
}

#[test]
fn big_family_keeps_its_variant_at_large_sizes() {
    // Under `\Huge` (2.488×) the variant chosen at normal size is drawn
    // 2.488× larger, centered on the axis, rather than falling back to
    // scaling the base glyph.
    let huge = 2.488;
    let axis = TexParams::default().axis_height * huge;
    for (latex, advance) in [
        (r"\Bigg\langle", 3.001),
        (r"\Bigg/", 2.945),
        (r"\big(", 1.195),
    ] {
        let (dy, d) = big_delimiter_at(latex, huge);
        let BoxKind::Delimiter {
            ch,
            target_height,
            scale,
        } = d.kind
        else {
            unreachable!()
        };
        assert_eq!(scale, huge, "{latex}");
        assert!(
            (target_height - advance * huge).abs() < 0.005,
            "{latex}: {target_height}"
        );
        let v = glyph_variants::delimiter_variant(ch, target_height / scale).expect("variant");
        assert!(
            (v.advance_em - advance).abs() < 0.002,
            "{latex}: {}",
            v.advance_em
        );
        let center = dy + (d.height - d.depth) / 2.0;
        assert!((center - axis).abs() < 1e-9, "{latex}: center {center}");
    }
}

#[test]
fn big_family_stays_text_size_in_scripts() {
    // TeX, amsmath and KaTeX set `\big` at text size inside a subscript.
    let text = big_delimiter_layout(r"\Big(");
    let script = big_delimiter_layout(r"x_{\Big(}");
    let (_, t) = find_delimiter(&text, 0.0).expect("text delimiter");
    let (_, s) = find_delimiter(&script, 0.0).expect("script delimiter");
    assert!((t.height + t.depth - (s.height + s.depth)).abs() < 1e-9);
    assert!((t.width - s.width).abs() < 1e-9);
}

#[test]
fn text_svg_squeezes_assembled_big_arrows_into_their_box() {
    // The text SVG has no assembly: it draws the arrow glyph at the full
    // height, squeezed to the assembly's width so it cannot overlap `x`.
    let root = big_delimiter_layout(r"\Bigg\uparrow \Bigg\Downarrow x");
    let svg = crate::render::emit::emit_svg(&root, &default_options());
    assert_eq!(
        svg.matches(r#"<text transform="translate("#).count(),
        2,
        "{svg}"
    );
    for part in svg.split(") scale(").skip(1) {
        let sx: f64 = part.split(',').next().unwrap().parse().unwrap();
        assert!(sx > 0.2 && sx < 0.5, "squeeze {sx}: {svg}");
    }
}

#[test]
fn text_svg_draws_large_big_delimiter_paths_at_their_chosen_size() {
    // Under `\Huge` the `\Bigg(` glyph is chosen at normal size and magnified;
    // the text SVG's built-in path must be magnified the same way (thick
    // strokes), not redrawn at the full height with hairline strokes.
    fn path_scale(latex: &str) -> f64 {
        let svg = crate::from_latex(latex).unwrap().to_svg_text().unwrap();
        let part = svg
            .split(r#"<g transform="translate("#)
            .nth(1)
            .and_then(|g| g.split(") scale(").nth(1))
            .unwrap_or_else(|| panic!("no delimiter path: {svg}"));
        part.split(')').next().unwrap().parse().unwrap()
    }
    let normal = path_scale(r"\Bigg(");
    let huge = path_scale(r"\Huge\Bigg(");
    assert!((huge / normal - 2.488).abs() < 0.01, "{normal} vs {huge}");
    // `\left…\right` keeps drawing its path at the full height (scale 1,
    // i.e. the 16px em).
    assert_eq!(path_scale(r"\left( \frac{a}{b} \right)"), 16.0);
}

/// A leading script is placed by its nucleus, not by the trailing scripts
/// beside it, on the heap and the arena path.
#[test]
fn leading_scripts_ignore_trailing_ones() {
    use crate::render::arena::{ArenaBoxKind, LayoutArena, LayoutBoxId};
    use crate::render::emit::{emit_svg, emit_svg_arena};

    let options = default_options();
    let id = |s: &str| Box::new(MathNode::Identifier(s.into()));
    let symbol = |s: &str| Box::new(MathNode::Symbol(SymbolKind::Ordinary, s.into()));
    // The offsets of the leading scripts, which come first in their box, on
    // each path; the two paths must also draw the same.
    let offsets = |base: &MathNode, sub: bool, sup: bool| -> [Vec<f64>; 2] {
        let count = usize::from(sub) + usize::from(sup);
        let expr = MathExpr::inline(vec![MathNode::PreScripts {
            base: Box::new(base.clone()),
            pre_sub: sub.then(|| id("d")),
            pre_sup: sup.then(|| id("e")),
        }]);
        fn on_heap(layout: &LayoutBox, count: usize) -> Option<Vec<f64>> {
            let BoxKind::HBox { children } = &layout.kind else {
                return None;
            };
            if children.len() == count + 1 {
                return Some(children[..count].iter().map(|c| c.dy).collect());
            }
            children.iter().find_map(|c| on_heap(&c.layout, count))
        }
        fn on_arena(arena: &LayoutArena, id: LayoutBoxId, count: usize) -> Option<Vec<f64>> {
            let ArenaBoxKind::HBox { children } = arena.get(id).kind else {
                return None;
            };
            let children = arena.children(children);
            if children.len() == count + 1 {
                return Some(children[..count].iter().map(|c| c.dy).collect());
            }
            children.iter().find_map(|c| on_arena(arena, c.id, count))
        }
        let heap_root = layout_expr(&expr, MathStyle::Text, &options);
        let compact = crate::CompactExpr::from_math_expr(&expr).unwrap();
        let mut arena = LayoutArena::new();
        let arena_root = LayoutBuilder::new(MathStyle::Text, &options).layout(&mut arena, &compact);
        assert_eq!(
            emit_svg(&heap_root, &options),
            emit_svg_arena(&arena, arena_root, &options),
            "{base:?}"
        );
        [
            on_heap(&heap_root, count).unwrap(),
            on_arena(&arena, arena_root, count).unwrap(),
        ]
    };
    let nuclei = [
        MathNode::Identifier("t".into()),
        // Accents and bars: their scripts are beside them.
        MathNode::Accent {
            base: id("T"),
            accent: '\u{302}',
            position: crate::ast::AccentPosition::Above,
            wide: false,
        },
        MathNode::Overscript {
            base: id("T"),
            over: symbol("\u{203E}"),
            movable_limits: false,
        },
        MathNode::Underscript {
            base: id("T"),
            under: symbol("\u{203E}"),
            movable_limits: false,
        },
    ];
    for nucleus in nuclei {
        let nucleus = Box::new(nucleus);
        let scripted = [
            MathNode::Subscript {
                base: nucleus.clone(),
                sub: id("m"),
            },
            MathNode::SubSup {
                base: nucleus.clone(),
                sub: Box::new(MathNode::Fraction {
                    kind: FractionKind::Normal,
                    num: id("a"),
                    den: id("b"),
                }),
                sup: id("n"),
            },
            MathNode::Subscript {
                base: Box::new(MathNode::Superscript {
                    base: nucleus.clone(),
                    sup: id("n"),
                }),
                sub: id("m"),
            },
        ];
        for (sub, sup) in [(true, false), (false, true), (true, true)] {
            let [heap, arena] = offsets(&nucleus, sub, sup);
            for base in &scripted {
                let [on_heap, on_arena] = offsets(base, sub, sup);
                assert_eq!(on_heap, heap, "heap {base:?}");
                assert_eq!(on_arena, arena, "arena {base:?}");
            }
        }
    }
    // A brace label is stacked: the whole labelled brace is the nucleus.
    let braced = MathNode::Superscript {
        base: Box::new(MathNode::Overscript {
            base: id("x"),
            over: symbol("\u{23DE}"),
            movable_limits: false,
        }),
        sup: id("n"),
    };
    offsets(&braced, false, true);
}
