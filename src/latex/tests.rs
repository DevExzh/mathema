use crate::ast::DisplayStyle;
use crate::ast::compact::CompactNode;
use crate::ast::normalize::{normalize_compact_expr, normalize_expr};
use crate::error::{Config, Error, LimitKind, Strictness};
use crate::reader::Reader;
use crate::reader::latex::LatexReader;
use crate::writer::Writer;
use crate::writer::latex::LatexWriter;

/// Round-trip idempotency: parse → normalize → write → parse → normalize → write
/// must produce identical canonical LaTeX.
#[test]
fn round_trip_simple() {
    let config = Config::default();
    let cases = [
        "x^2 + 1",
        "\\frac{a}{b}",
        "\\sqrt{x}",
        "\\sqrt[3]{x}",
        "x_i^2",
    ];
    for input in &cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr).unwrap();
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr).unwrap();
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(out1, out2, "round-trip failed for input: {input}");
    }
    let text_mathml = crate::convert_with(
        r"\angl{n}",
        crate::Format::LaTeX,
        crate::Format::MathML,
        &config,
    )
    .unwrap();
    assert!(text_mathml.contains("<mtext>n</mtext>"));
    let math_mathml = crate::convert_with(
        r"\angl{$x^2$}",
        crate::Format::LaTeX,
        crate::Format::MathML,
        &config,
    )
    .unwrap();
    assert!(math_mathml.contains("<msup>"));
}

#[test]
fn math_style_switches_and_forced_fractions_keep_scope_and_kind() {
    let config = Config::default().with_display(DisplayStyle::Inline);

    let top = LatexReader::parse_compact(r"\displaystyle x", &config)
        .unwrap()
        .expr;
    assert_eq!(top.display, DisplayStyle::Block);
    assert!(matches!(
        *top.arena.node(top.arena.children(top.roots)[0]),
        CompactNode::StyleSwitch(0)
    ));

    let scoped = LatexReader::parse_compact(r"{\displaystyle x}y", &config)
        .unwrap()
        .expr;
    assert_eq!(scoped.display, DisplayStyle::Inline);
    let canonical = LatexWriter::write_compact(&scoped, &config).unwrap();
    assert_eq!(canonical, r"{\displaystyle x}y");

    let heap = scoped.to_math_expr();
    assert!(matches!(
        heap.nodes.first(),
        Some(crate::ast::MathNode::Row(children))
            if matches!(children.first(), Some(crate::ast::MathNode::StyleSwitch(0)))
    ));
    let rebuilt = crate::ast::compact::CompactExpr::from_math_expr(&heap).unwrap();
    assert_eq!(
        LatexWriter::write_compact(&rebuilt, &config).unwrap(),
        canonical
    );

    for (command, value) in [
        (r"\textstyle", 1),
        (r"\scriptstyle", 2),
        (r"\scriptscriptstyle", 3),
    ] {
        let expr = LatexReader::parse_compact(&format!("{{{command} x}}y"), &config)
            .unwrap()
            .expr;
        assert!(matches!(
            *expr.arena.node(expr.arena.children(expr.roots)[0]),
            CompactNode::Row(span)
                if matches!(expr.arena.children(span).first(), Some(&id)
                    if matches!(*expr.arena.node(id), CompactNode::StyleSwitch(actual) if actual == value))
        ));
    }

    let choice = LatexReader::parse_compact(r"\mathchoice{D}{T}{S}{SS}", &config)
        .unwrap()
        .expr;
    let choice_heap = choice.to_math_expr();
    assert!(matches!(
        choice_heap.nodes.first(),
        Some(crate::ast::MathNode::MathChoice { .. })
    ));
    let choice_rebuilt = crate::ast::compact::CompactExpr::from_math_expr(&choice_heap).unwrap();
    assert_eq!(
        LatexWriter::write_compact(&choice_rebuilt, &config).unwrap(),
        r"\mathchoice{D}{T}{S}{SS}"
    );

    let frac_cases = [
        (r"x_{\dfrac{a}{b}}", crate::ast::FractionKind::Display),
        (r"x_{\tfrac{a}{b}}", crate::ast::FractionKind::Inline),
    ];
    for (source, kind) in frac_cases {
        let expr = LatexReader::parse_compact(source, &config).unwrap().expr;
        let root = expr.arena.children(expr.roots)[0];
        let CompactNode::Subscript { sub, .. } = *expr.arena.node(root) else {
            panic!("expected script for {source}");
        };
        assert!(matches!(
            *expr.arena.node(sub),
            CompactNode::Fraction { kind: actual, .. } if actual == kind
        ));
    }

    let explicit = LatexReader::parse_compact(r"x_{\displaystyle\frac{a}{b}}", &config)
        .unwrap()
        .expr;
    let root = explicit.arena.children(explicit.roots)[0];
    let CompactNode::Subscript { sub, .. } = *explicit.arena.node(root) else {
        panic!("expected explicit-style script");
    };
    assert!(matches!(*explicit.arena.node(sub), CompactNode::Row(_)));
    assert!(
        LatexWriter::write_compact(&explicit, &config)
            .unwrap()
            .contains(r"\displaystyle")
    );
}

#[test]
fn style_switch_normalization_matches_heap_and_compact_scopes() {
    let config = Config::default();
    for source in [
        r"{\displaystyle x}y",
        r"{{\scriptstyle x}y}z",
        r"x_{{\displaystyle a}b}",
        r"{x_{\scriptscriptstyle a}}y",
    ] {
        let mut compact = LatexReader::parse_compact(source, &config).unwrap().expr;
        normalize_compact_expr(&mut compact).unwrap();
        let compact_latex = LatexWriter::write_compact(&compact, &config).unwrap();

        let mut heap = compact.to_math_expr();
        normalize_expr(&mut heap);
        let heap_latex = LatexWriter::write(&heap, &config).unwrap();
        assert_eq!(
            compact_latex, heap_latex,
            "heap/compact mismatch for {source}"
        );

        let mut reparsed = LatexReader::parse_compact(&compact_latex, &config)
            .unwrap()
            .expr;
        normalize_compact_expr(&mut reparsed).unwrap();
        assert_eq!(
            LatexWriter::write_compact(&reparsed, &config).unwrap(),
            compact_latex,
            "style scope was not stable for {source}"
        );
    }
}

#[test]
fn legacy_font_declarations_scope_like_grouped_fonts() {
    for (source, expected) in [
        (r"\rm xyz", r"\mathrm{xyz}"),
        (r"\sf xyz", r"\mathsf{xyz}"),
        (r"\tt xyz", r"\mathtt{xyz}"),
        (r"\bf xyz", r"\mathbf{xyz}"),
        (r"\it xyz", r"\mathit{xyz}"),
        (r"\cal xyz", r"\mathcal{xyz}"),
        (r"{\rm xyz}q", r"\mathrm{xyz}q"),
        (r"\rm x\sf y z", r"\mathrm{x}\mathsf{yz}"),
        (r"{\rm x{\sf y}z}q", r"\mathrm{x\mathsf{y}z}q"),
        (r"\rm x_i^2 y", r"\mathrm{x_i^{2}y}"),
    ] {
        let actual = crate::convert(source, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(actual, expected, "wrong legacy-font scope for {source}");
    }
    let legacy_mathml =
        crate::convert(r"\rm xyz", crate::Format::LaTeX, crate::Format::MathML).unwrap();
    assert!(
        legacy_mathml.contains(r#"<mstyle mathvariant="normal"><mi mathvariant="normal">xyz</mi>"#),
        "legacy declaration did not style its full remaining expression: {legacy_mathml}"
    );

    let matrix = crate::convert(
        r"\begin{matrix}\rm x&y\\z&\sf q\end{matrix}",
        crate::Format::LaTeX,
        crate::Format::LaTeX,
    )
    .unwrap();
    assert_eq!(
        matrix,
        r"\begin{array}{cc} \mathrm{x} & y \\ z & \mathsf{q} \end{array}"
    );
}

#[test]
fn legacy_font_scopes_cross_infix_fractions_without_hiding_markers() {
    for (input, expected, numerator, denominator) in [
        (
            r"x+\bf a\over b+c",
            r"\frac{x+ \mathbf{a}}{\mathbf{b+ c}}",
            "<mi>x</mi><mo>+</mo><mstyle mathvariant=\"bold\"><mi>a</mi></mstyle>",
            "<mstyle mathvariant=\"bold\"><mi>b</mi><mo>+</mo><mi>c</mi></mstyle>",
        ),
        (
            r"x+\bf\bf a\over b+c",
            r"\frac{x+ \mathbf{a}}{\mathbf{b+ c}}",
            "<mi>x</mi><mo>+</mo><mstyle mathvariant=\"bold\"><mi>a</mi></mstyle>",
            "<mstyle mathvariant=\"bold\"><mi>b</mi><mo>+</mo><mi>c</mi></mstyle>",
        ),
        (
            r"x+\bf\it a\over b+c",
            r"\frac{x+ \mathit{a}}{\mathit{b+ c}}",
            "<mstyle mathvariant=\"italic\"><mi>a</mi></mstyle>",
            "<mstyle mathvariant=\"italic\"><mi>b</mi>",
        ),
    ] {
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, expected, "wrong font/fraction scope for {input}");
        assert_eq!(
            first,
            crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
            "font/fraction output was unstable for {input}"
        );
        let mathml = crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert!(
            mathml.contains("<mfrac>"),
            "fraction marker was hidden: {mathml}"
        );
        assert!(
            mathml.contains(numerator),
            "numerator style changed: {mathml}"
        );
        assert!(
            mathml.contains(denominator),
            "denominator style changed: {mathml}"
        );
    }

    let grouped = crate::convert(
        r"{\bf a\over b}+c",
        crate::Format::LaTeX,
        crate::Format::LaTeX,
    )
    .unwrap();
    assert_eq!(grouped, r"\frac{\mathbf{a}}{\mathbf{b}}+ c");
}

#[test]
fn legacy_font_declarations_are_not_unbraced_script_arguments() {
    for input in [r"x^\bf", r"x_\bf", r"\frac\bf{a}{b}", r"\sqrt\bf x"] {
        assert!(
            LatexReader::parse_compact(input, &Config::default()).is_err(),
            "bare legacy declaration was accepted as an argument: {input}"
        );
    }
    for input in [r"x^{\bf a}", r"\frac{\bf a}{b}"] {
        assert!(
            LatexReader::parse_compact(input, &Config::default()).is_ok(),
            "grouped legacy declaration was rejected: {input}"
        );
    }
}

#[test]
fn font_fraction_does_not_escape_its_argument_or_cell() {
    for (input, equivalent) in [
        (
            r"p{q+\bf a\over b+c}r",
            r"p\frac{q+\mathbf{a}}{\mathbf{b+c}}r",
        ),
        (r"p{\bf a\over b+c}r", r"p\frac{\mathbf{a}}{\mathbf{b+c}}r"),
        (
            r"\left(x+\bf a\over b\right)y",
            r"\left(\frac{x+\mathbf{a}}{\mathbf{b}}\right)y",
        ),
        (
            r"\begin{matrix}p+\bf a\over b&y\end{matrix}",
            r"\begin{matrix}\frac{p+\mathbf{a}}{\mathbf{b}}&y\end{matrix}",
        ),
        (
            r"\sqrt[p+\bf a\over b]{x}",
            r"\sqrt[\frac{p+\mathbf{a}}{\mathbf{b}}]{x}",
        ),
    ] {
        let f = crate::from_latex(input).unwrap();
        assert_eq!(
            f.to_mathml().unwrap(),
            crate::from_latex(equivalent).unwrap().to_mathml().unwrap(),
            "{input}"
        );
        let written = f.to_latex().unwrap();
        assert_eq!(
            f.to_mathml().unwrap(),
            crate::from_latex(&written).unwrap().to_mathml().unwrap()
        );
    }
}

#[test]
fn legacy_font_tables_preserve_local_font_overrides() {
    for (input, expected) in [
        (
            r"\bf x\mathit{y}\begin{matrix}a&b\end{matrix}z",
            r"\mathbf{x\mathit{y}}\begin{matrix}a&b\end{matrix}\mathbf{z}",
        ),
        (
            r"\rm x\sqrt{\mathit{u}+\begin{matrix}a&b\end{matrix}}z",
            r"\mathrm{x}\sqrt{\mathrm{\mathit{u}+}\begin{matrix}a&b\end{matrix}}\mathrm{z}",
        ),
        (
            r"\mathbf{x\frac{a}{\begin{matrix}y\end{matrix}}z}",
            r"\mathbf{x}\frac{\mathbf{a}}{\begin{matrix}y\end{matrix}}\mathbf{z}",
        ),
    ] {
        let read = |s| {
            let mut parsed = LatexReader::parse_compact(s, &Config::default())
                .unwrap()
                .expr;
            normalize_compact_expr(&mut parsed).unwrap();
            crate::writer::mathml::MathmlWriter::write_compact(&parsed, &Config::default()).unwrap()
        };
        assert_eq!(read(input), read(expected), "{input}");
    }
}

#[test]
fn legacy_font_scope_resets_matrix_cells_but_keeps_siblings() {
    for (input, expected) in [
        (
            r"\bf x\begin{matrix}y&z\end{matrix}t",
            r"\mathbf{x}\begin{array}{cc} y & z \end{array}\mathbf{t}",
        ),
        (
            r"\bf x\frac{a}{\begin{matrix}u&v\end{matrix}}t",
            r"\mathbf{x}\frac{\mathbf{a}}{\begin{array}{cc} u & v \end{array}}\mathbf{t}",
        ),
        (
            r"\bf x\begin{matrix}\it y&\bf z\end{matrix}t",
            r"\mathbf{x}\begin{array}{cc} \mathit{y} & \mathbf{z} \end{array}\mathbf{t}",
        ),
        (
            r"x\begin{matrix}y&z\end{matrix}t",
            r"x\begin{array}{cc} y & z \end{array}t",
        ),
    ] {
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, expected, "{input}");
        assert_eq!(
            first,
            crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
            "unstable matrix font scope: {input}"
        );
    }
    let mathml = crate::convert(
        r"\bf x\begin{matrix}y&z\end{matrix}t",
        crate::Format::LaTeX,
        crate::Format::MathML,
    )
    .unwrap();
    assert!(mathml.contains("<mstyle mathvariant=\"bold\"><mi>x</mi></mstyle><mtable>"));
    assert!(mathml.contains("</mtable><mstyle mathvariant=\"bold\"><mi>t</mi></mstyle>"));
    assert!(!mathml.contains("<mstyle mathvariant=\"bold\"><mi>x</mi><mtable>"));
}

#[test]
fn legacy_font_scope_preserves_tables_in_compounds() {
    for (input, expected) in [
        (
            r"\bf x\sqrt{\begin{matrix}u&v\end{matrix}}t",
            r"\mathbf{x}\sqrt{\begin{array}{cc} u & v \end{array}}\mathbf{t}",
        ),
        (
            r"\bf x^{\begin{matrix}u&v\end{matrix}}t",
            r"{\mathbf{x}}^{\begin{array}{cc} u & v \end{array}}\mathbf{t}",
        ),
        (
            r"\bf x\sum_{\begin{matrix}u&v\end{matrix}}t",
            r"\mathbf{x}\sum_{\begin{array}{cc} u & v \end{array}} {\mathbf{t}}",
        ),
        (
            r"\bf x\hat{\begin{matrix}u&v\end{matrix}}t",
            r"\mathbf{x}\hat{\begin{array}{cc} u & v \end{array}}\mathbf{t}",
        ),
    ] {
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, expected, "{input}");
        assert_eq!(
            first,
            crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
            "unstable compound font scope: {input}"
        );
        let mathml = crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert!(mathml.contains("<mtable>"), "matrix disappeared: {mathml}");
        assert!(
            !mathml.contains("<mstyle mathvariant=\"bold\"><mtable>"),
            "table inherited outer style: {mathml}"
        );
    }
}

#[test]
fn canonical_tex_attaches_bounds_to_the_operator_itself() {
    for (source, canonical) in [
        (r"\lim_i^j", r"\lim_i^j"),
        (r"\sin\limits_i^j", r"\sin\limits_i^j"),
        (
            r"\operatorname{score}\limits_i^j",
            r"\operatorname{score}_i^j",
        ),
        (r"\mathop{a+b}_i^j", r"\mathop{a+ b}_i^j"),
        (r"\mathop{a+b}\limits_i^j", r"\mathop{a+ b}\limits_i^j"),
        (r"\mathop{a+b}\nolimits_i^j", r"\mathop{a+ b}\nolimits_i^j"),
    ] {
        let parsed = crate::from_latex(source).unwrap();
        assert_eq!(parsed.to_latex().unwrap(), canonical, "{source}");
        let round_trip = crate::from_mathml(&parsed.to_mathml().unwrap()).unwrap();
        assert_eq!(round_trip.to_latex().unwrap(), canonical, "{source}");
    }
}

#[test]
fn braces_keep_named_and_compound_operator_scripts_at_the_side() {
    for source in [
        r"{\lim}_i",
        r"{{\sin\limits}}_i",
        r"{\operatorname*{score}}_i",
        r"{\mathop{a+b}}_i",
        r"{\lim}^i",
    ] {
        let v = crate::from_latex(source).unwrap();
        let h = v.to_mathml().unwrap();
        assert!(
            h.contains("<msub>") || h.contains("<msup>"),
            "{source}: {h}"
        );
        let first = v.to_latex().unwrap();
        let again = crate::from_latex(&first).unwrap();
        assert_eq!(again.to_mathml().unwrap(), h, "{source}: {first}");
        assert_eq!(again.to_latex().unwrap(), first);
    }
}

#[test]
fn named_operator_limit_styles_reach_mathml() {
    for (display, expected) in [
        (
            DisplayStyle::Inline,
            [
                (r"\lim_{x\to0}f(x)", r#"<munder><mo movablelimits="true">"#),
                (r"\sin_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{lim}_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{lim}\limits_{x\to0}f(x)", "<msub>"),
                (
                    r"\operatorname*{lim}_{x\to0}f(x)",
                    r#"<munder><mo movablelimits="true">"#,
                ),
                (r"\operatorname*{lim}\limits_{x\to0}f(x)", "<munder>"),
                (r"\lim\limits_{x\to0}f(x)", "<munder>"),
                (r"\lim\nolimits_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{score}\limits_{x\to0}f(x)", "<msub>"),
                (r"\operatorname*{score}\limits_{x\to0}f(x)", "<munder>"),
                (r"\operatorname*{sin}\limits_{x\to0}f(x)", "<munder>"),
                (r"\mathop{\rm lim}_{x\to0}f(x)", "<msub>"),
                (r"\mathop{\rm lim}\limits_{x\to0}f(x)", "<munder>"),
                (r"\mathop{\rm lim}\nolimits_{x\to0}f(x)", "<msub>"),
            ],
        ),
        (
            DisplayStyle::Block,
            [
                (r"\lim_{x\to0}f(x)", "<munder>"),
                (r"\sin_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{lim}_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{lim}\limits_{x\to0}f(x)", "<msub>"),
                (r"\operatorname*{lim}_{x\to0}f(x)", "<munder>"),
                (r"\operatorname*{lim}\limits_{x\to0}f(x)", "<munder>"),
                (r"\lim\limits_{x\to0}f(x)", "<munder>"),
                (r"\lim\nolimits_{x\to0}f(x)", "<msub>"),
                (r"\operatorname{score}\limits_{x\to0}f(x)", "<msub>"),
                (r"\operatorname*{score}\limits_{x\to0}f(x)", "<munder>"),
                (r"\operatorname*{sin}\limits_{x\to0}f(x)", "<munder>"),
                (r"\mathop{\rm lim}_{x\to0}f(x)", "<munder>"),
                (r"\mathop{\rm lim}\limits_{x\to0}f(x)", "<munder>"),
                (r"\mathop{\rm lim}\nolimits_{x\to0}f(x)", "<msub>"),
            ],
        ),
    ] {
        let config = Config::default().with_display(display);
        for (input, tag) in expected {
            let mathml =
                crate::convert_with(input, crate::Format::LaTeX, crate::Format::MathML, &config)
                    .unwrap();
            assert!(mathml.contains(tag), "{display:?} {input}: {mathml}");
        }
    }
}

#[test]
fn named_operator_scripts_preserve_operator_limit_metadata() {
    let parse = |input| {
        LatexReader::parse_compact(input, &Config::default())
            .unwrap()
            .expr
    };
    let expr = parse(r"\lim\limits_{x\to0}f");
    let root = expr.arena.children(expr.roots)[0];
    let CompactNode::Subscript { base, .. } = *expr.arena.node(root) else {
        panic!("named operator should retain an ordinary subscript wrapper");
    };
    assert!(matches!(
        *expr.arena.node(base),
        CompactNode::Operator {
            limits: crate::ast::LimitStyle::AboveBelow,
            ..
        }
    ));

    let expr = parse(r"\operatorname*{lim}_{x\to0}f");
    let root = expr.arena.children(expr.roots)[0];
    let CompactNode::Subscript { base, .. } = *expr.arena.node(root) else {
        panic!("starred operatorname should retain an ordinary subscript wrapper");
    };
    assert!(matches!(
        *expr.arena.node(base),
        CompactNode::Operator {
            limits: crate::ast::LimitStyle::Default,
            ..
        }
    ));

    let expr = parse(r"\operatorname{lim}\limits_{x\to0}f");
    let root = expr.arena.children(expr.roots)[0];
    let CompactNode::Subscript { base, .. } = *expr.arena.node(root) else {
        panic!("unstarred operatorname should retain an ordinary subscript wrapper");
    };
    assert!(matches!(
        *expr.arena.node(base),
        CompactNode::Operator {
            limits: crate::ast::LimitStyle::SubSup,
            ..
        }
    ));

    let expr = parse(r"\operatorname*{lim}\limits_{x\to0}f");
    let root = expr.arena.children(expr.roots)[0];
    let CompactNode::Subscript { base, .. } = *expr.arena.node(root) else {
        panic!("starred operatorname should retain an ordinary subscript wrapper");
    };
    assert!(matches!(
        *expr.arena.node(base),
        CompactNode::Operator {
            limits: crate::ast::LimitStyle::AboveBelow,
            ..
        }
    ));

    let expr = parse(r"\mathop{a+b}_{i}^{j}");
    let root = expr.arena.children(expr.roots)[0];
    assert!(matches!(
        *expr.arena.node(root),
        CompactNode::UnderOver {
            movable_limits: true,
            ..
        }
    ));

    let expr = parse(r"\mathop{a+b}\limits_{i}^{j}");
    let root = expr.arena.children(expr.roots)[0];
    assert!(matches!(
        *expr.arena.node(root),
        CompactNode::UnderOver {
            movable_limits: false,
            ..
        }
    ));

    let expr = parse(r"\mathop{a+b}\nolimits_{i}^{j}");
    let root = expr.arena.children(expr.roots)[0];
    assert!(matches!(*expr.arena.node(root), CompactNode::SubSup { .. }));
}

#[test]
fn relax_is_skipped_inside_unbraced_script_arguments() {
    let config = Config::default();
    let superscript = LatexReader::parse(r"x^\relax 2", &config).unwrap();
    assert_eq!(superscript.expr.nodes.len(), 1);
    assert!(matches!(
        superscript.expr.nodes.first(),
        Some(crate::ast::MathNode::Superscript { .. })
    ));
    let subscript = LatexReader::parse(r"x_\relax 2", &config).unwrap();
    assert_eq!(subscript.expr.nodes.len(), 1);
    assert!(matches!(
        subscript.expr.nodes.first(),
        Some(crate::ast::MathNode::Subscript { .. })
    ));
    for (input, expected) in [
        (r"x^\relax 2", r"x^2"),
        (r"x_\relax 2", r"x_2"),
        (r"x^\relax\relax   2", r"x^2"),
        (r"x_\relax\relax\ 2", r"x_\ 2"),
    ] {
        let actual =
            crate::convert_with(input, crate::Format::LaTeX, crate::Format::LaTeX, &config)
                .unwrap();
        let want = crate::convert_with(
            expected,
            crate::Format::LaTeX,
            crate::Format::LaTeX,
            &config,
        )
        .unwrap();
        assert_eq!(actual, want, "{input}");
    }
}

#[test]
fn relax_script_boundaries_do_not_escape_groups_or_arrays() {
    let config = Config::default();
    for (input, expected) in [
        (r"x^{\relax 2}", r"x^{2}"),
        (r"{x^\relax}2", r"{x}2"),
        (
            r"\begin{matrix}x^\relax 2&y_\relax 3\end{matrix}",
            r"\begin{matrix}x^2&y_3\end{matrix}",
        ),
    ] {
        let actual =
            crate::convert_with(input, crate::Format::LaTeX, crate::Format::LaTeX, &config)
                .unwrap();
        let want = crate::convert_with(
            expected,
            crate::Format::LaTeX,
            crate::Format::LaTeX,
            &config,
        )
        .unwrap();
        assert_eq!(actual, want, "{input}");
    }
    let cr = crate::convert_with(
        r"\begin{matrix}x^\relax\cr y\end{matrix}",
        crate::Format::LaTeX,
        crate::Format::MathML,
        &config,
    )
    .unwrap();
    let slash = crate::convert_with(
        r"\begin{matrix}x^\relax\\ y\end{matrix}",
        crate::Format::LaTeX,
        crate::Format::MathML,
        &config,
    )
    .unwrap();
    assert_eq!(
        cr, slash,
        r"\cr must terminate the first matrix row after relax"
    );
}

#[test]
fn user_relax_shadowing_and_aliases_keep_their_meaning() {
    let config = Config::default();
    for (input, expected) in [
        (r"\def\relax{z}x^\relax 2", r"x^z2"),
        (r"\let\r\relax x^\r 2", r"x^2"),
        (r"x^\relax+2", r"x^+2"),
    ] {
        let actual =
            crate::convert_with(input, crate::Format::LaTeX, crate::Format::LaTeX, &config)
                .unwrap();
        let want = crate::convert_with(
            expected,
            crate::Format::LaTeX,
            crate::Format::LaTeX,
            &config,
        )
        .unwrap();
        assert_eq!(actual, want, "{input}");
    }
}

#[test]
fn actuarial_angle_preserves_enclosure_and_argument_boundaries() {
    let config = Config::default();
    for (input, expected) in [
        (r"\angl{n}", r"\angl{n}"),
        (r"a_{\angl{n}i}", r"a_{\angl{n}i}"),
        (r"a_\angln", r"a_{\angl{n}}"),
    ] {
        let first = crate::from_latex_with(input, &config)
            .unwrap()
            .to_latex()
            .unwrap();
        assert_eq!(first, expected, "{input}");
        assert_eq!(
            crate::from_latex_with(&first, &config)
                .unwrap()
                .to_latex()
                .unwrap(),
            first
        );
        let mathml =
            crate::convert_with(input, crate::Format::LaTeX, crate::Format::MathML, &config)
                .unwrap();
        assert!(mathml.contains("notation=\"actuarial\""), "{mathml}");
    }
}

#[test]
fn actuarial_text_boxes_keep_mixed_content_and_inline_math() {
    for (input, fragments) in [
        (
            r"\angl{term $x^2$ costs \$5}",
            vec![
                "<mtext>term\u{a0}</mtext>",
                "<msup>",
                "<mtext>\u{a0}costs $5</mtext>",
            ],
        ),
        (
            r"\angl{term \(x+y\) due}",
            vec![
                "<mtext>term\u{a0}</mtext>",
                "<mi>x</mi>",
                "<mtext>\u{a0}due</mtext>",
            ],
        ),
        (
            r"\angl{\textbf{a\textit{b}}\{\%}",
            vec![
                r#"<mtext mathvariant="bold">a</mtext>"#,
                r#"<mtext mathvariant="bold-italic">b</mtext>"#,
                "{%",
            ],
        ),
    ] {
        let parsed = crate::from_latex(input).unwrap();
        let first = parsed.to_latex().unwrap();
        let second = crate::from_latex(&first).unwrap();
        assert_eq!(first, second.to_latex().unwrap(), "{input}");
        let ml = parsed.to_mathml().unwrap();
        assert_eq!(ml, second.to_mathml().unwrap(), "{input}");
        for fragment in fragments {
            assert!(ml.contains(fragment), "{input}: missing {fragment} in {ml}");
        }
    }
    assert_eq!(
        crate::from_latex(r"\def\term{end value}\angl\term")
            .unwrap()
            .to_latex()
            .unwrap(),
        r"\angl{end value}"
    );
    let styled = crate::from_latex(r"\angl{\textbf{a\textit{b}}}")
        .unwrap()
        .to_latex()
        .unwrap();
    assert_eq!(styled, r"\angl{\textbf{a}\textbf{\textit{b}}}");
    for invalid in [
        r"\angl{x^2}",
        r"\angl{missing $end}",
        r"\angl{lost\unknown}",
    ] {
        assert!(crate::from_latex(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn actuarial_unicode_math_script_round_trip_preserves_compound_operands() {
    for input in [
        r"a_{\angl n} + b^{\angl g}",
        r"a_{\boxed{x}} + c_{\phantom{y}}",
        r"a_{\angl n}z",
    ] {
        let unicode =
            crate::convert(input, crate::Format::LaTeX, crate::Format::UnicodeMath).unwrap();
        let latex =
            crate::convert(&unicode, crate::Format::UnicodeMath, crate::Format::LaTeX).unwrap();
        let unicode_again =
            crate::convert(&latex, crate::Format::LaTeX, crate::Format::UnicodeMath).unwrap();
        assert_eq!(unicode_again, unicode, "{input}: {unicode:?} -> {latex:?}");
    }
}

#[test]
fn mathml_top_and_right_sides_keep_additional_strikes() {
    for notation in ["right top", "actuarial"] {
        let ml = format!(
            r#"<math><menclose notation="{notation} updiagonalstrike"><mi>x</mi></menclose></math>"#
        );
        let tex = crate::convert(&ml, crate::Format::MathML, crate::Format::LaTeX).unwrap();
        assert_eq!(tex, r"\angl{$\cancel{x}$}");
    }
}

#[test]
fn actuarial_angle_maps_to_mathml_and_omml_enclosures() {
    let config = Config::default();
    let mathml = crate::convert_with(
        r"\angl{x}",
        crate::Format::LaTeX,
        crate::Format::MathML,
        &config,
    )
    .unwrap();
    assert!(mathml.contains("notation=\"actuarial\""));
    let from_mathml = crate::convert_with(
        &mathml,
        crate::Format::MathML,
        crate::Format::LaTeX,
        &config,
    )
    .unwrap();
    assert_eq!(from_mathml, r"\angl{x}");

    let omml = crate::convert_with(
        r"\angl{x}",
        crate::Format::LaTeX,
        crate::Format::Omml,
        &config,
    )
    .unwrap();
    assert!(omml.contains("<m:hideBot"));
    assert!(omml.contains("<m:hideLeft"));
    assert!(!omml.contains("<m:hideTop"));
    assert!(!omml.contains("<m:hideRight"));
    let from_omml =
        crate::convert_with(&omml, crate::Format::Omml, crate::Format::LaTeX, &config).unwrap();
    assert_eq!(from_omml, r"\angl{x}");

    let unicode = crate::convert_with(
        r"\angl{x}",
        crate::Format::LaTeX,
        crate::Format::UnicodeMath,
        &config,
    )
    .unwrap();
    assert_eq!(unicode, "▭(6&\"x\")");
    let from_unicode = crate::convert_with(
        &unicode,
        crate::Format::UnicodeMath,
        crate::Format::LaTeX,
        &config,
    )
    .unwrap();
    assert_eq!(from_unicode, r"\angl{x}");

    let result = crate::from_latex_with(r"\angl{x}", &config).unwrap();
    let bytes = result
        .to_mtef_bytes(&crate::writer::mtef::MtefWriteOptions::default())
        .unwrap();
    let back = crate::from_mtef_bytes_with(&bytes, &config).unwrap();
    assert_eq!(back.to_latex().unwrap(), r"\angl{x}");
}

#[test]
fn contour_integral_names_and_literals_keep_the_same_script_atom() {
    for (cmd, glyph) in [
        ("oiint", '∯'),
        ("oiiint", '∰'),
        ("varointclockwise", '∲'),
        ("ointctrclockwise", '∳'),
    ] {
        let by_name = crate::from_latex(&format!(r"t_\{cmd} q")).unwrap();
        let by_glyph = crate::from_latex(&format!("t_{glyph} q")).unwrap();
        let output = by_name.to_latex().unwrap();
        assert_eq!(
            by_glyph.to_mathml().unwrap(),
            by_name.to_mathml().unwrap(),
            "{cmd}"
        );
        assert_eq!(by_glyph.to_latex().unwrap(), output);
        let again = crate::from_latex(&output).unwrap();
        assert_eq!(
            again.to_mathml().unwrap(),
            by_name.to_mathml().unwrap(),
            "{output}"
        );
        assert_eq!(again.to_latex().unwrap(), output);
    }
    for source in [
        r"t_\oiint_a^b x",
        r"\frac{t_\oiint}{b}",
        r"\int_\oiint x",
        r"{}_\oiint x",
        r"{}_{\oiint}x",
    ] {
        let original = crate::from_latex(source).unwrap();
        let canonical = original.to_latex().unwrap();
        let again = crate::from_latex(&canonical).unwrap();
        assert_eq!(again.to_latex().unwrap(), canonical, "{source}");
        assert_eq!(
            original.to_mathml().unwrap(),
            again.to_mathml().unwrap(),
            "{source}: {canonical}"
        );
    }
}

#[test]
fn letterlike_operator_bodies_retain_their_atom_class_and_spelling() {
    for (glyph, command) in [
        ('ℜ', r"\Re"),
        ('ℑ', r"\Im"),
        ('ℏ', r"\hbar"),
        ('ℓ', r"\ell"),
        ('ℵ', r"\aleph"),
        ('α', r"\alpha"),
    ] {
        let raw = crate::from_latex(&format!(r"\int{glyph}!")).unwrap();
        let named = crate::from_latex(&format!(r"\int{command}!")).unwrap();
        assert_eq!(
            raw.to_mathml().unwrap(),
            named.to_mathml().unwrap(),
            "{glyph}"
        );
        let written = raw.to_latex().unwrap();
        let again = crate::from_latex(&written).unwrap();
        assert_eq!(written, again.to_latex().unwrap(), "{glyph}");
        assert_eq!(
            raw.to_mathml().unwrap(),
            again.to_mathml().unwrap(),
            "{glyph}"
        );
    }
    assert!(matches!(
        crate::from_latex("界").unwrap().expr().nodes.as_slice(),
        [crate::ast::MathNode::Identifier(_)]
    ));
}

#[test]
fn radical_indices_retain_grouped_brackets_and_nested_roots() {
    for tex in [
        r"\sqrt[{a]b}]{z}",
        r"\sqrt[{a[b]c}]{z}",
        r"\sqrt[{]}]{z}",
        r"\sqrt[{\sqrt[2]{x}}]{y}",
        r"\sqrt[\{]{x}",
        r"\sqrt[\}]{x}",
        r"\sqrt[\text{a]b}]{z}",
    ] {
        let original = crate::from_latex(tex).unwrap();
        let written = original.to_latex().unwrap();
        let again = crate::from_latex(&written).unwrap();
        assert_eq!(
            original.to_mathml().unwrap(),
            again.to_mathml().unwrap(),
            "{tex} -> {written}"
        );
        assert_eq!(written, again.to_latex().unwrap(), "{tex}");
    }
    for tex in [r"\sqrt[2]{x}", r"\sqrt[\alpha]{t}"] {
        assert_eq!(crate::from_latex(tex).unwrap().to_latex().unwrap(), tex);
    }
}

#[test]
fn text_control_symbols_keep_following_spaces() {
    for (input, expected) in [
        (r"\text{a\} b}", "a} b"),
        (r"\text{a\{ b}", "a{ b"),
        (r"\hbox{a\}\\b}", "a} b"),
        (r"\text{\! word}", "! word"),
        (r"\text{\KaTeX word}", "KaTeXword"),
    ] {
        let parsed = crate::from_latex(input).unwrap();
        assert_eq!(
            parsed.expr().nodes,
            [crate::ast::MathNode::Text(
                crate::ast::TextStyle::Normal,
                expected.into()
            )],
            "{input}"
        );
        let first = parsed.to_latex().unwrap();
        let again = crate::from_latex(&first).unwrap();
        assert_eq!(again.to_mathml().unwrap(), parsed.to_mathml().unwrap());
        assert_eq!(again.to_latex().unwrap(), first);
    }
}

#[test]
fn negated_operators_preserve_bounds_bodies_and_following_operands() {
    let config = Config::default();
    for input in [
        r"\centernot{\intop}t",
        r"\centernot\intop t",
        r"\centernot{\int_0^1}t",
        r"\centernot{\int\limits_a^b}t",
        r"\centernot{\int\nolimits_a^b x}t",
        r"\centernot\sum t",
        r"\centernot{\sum_{i=0}^n x}t",
        r"\centernot{\alpha}t",
        r"\not=",
        r"\not\in",
    ] {
        let original = crate::from_latex(input).unwrap();
        let first = original.to_latex().unwrap();
        let reparsed = crate::from_latex(&first).unwrap();
        assert_eq!(
            original.to_mathml().unwrap(),
            reparsed.to_mathml().unwrap(),
            "{input:?} wrote {first:?}"
        );
        assert_eq!(first, reparsed.to_latex().unwrap(), "{input:?}");
    }

    let precomposed = r"m\centernot{=}";
    let first = crate::convert(precomposed, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    assert_eq!(first, r"m\neq");
    assert_eq!(first, second);

    let split = crate::ast::MathExpr::inline(vec![
        crate::ast::MathNode::Identifier("m".into()),
        crate::ast::MathNode::Symbol(crate::ast::SymbolKind::Relation, "=".into()),
        crate::ast::MathNode::Symbol(crate::ast::SymbolKind::Ordinary, "\u{0338}".into()),
        crate::ast::MathNode::Identifier("t".into()),
    ]);
    let first = LatexWriter::write(&split, &config).unwrap();
    let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
    normalize_compact_expr(&mut reparsed.expr).unwrap();
    let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
    assert_eq!(first, second, "split ordinary overlay changed");
    assert!(first.contains('\u{0338}'), "split overlay glyph was lost");
}

#[test]
fn latex_node_limit_is_exact_for_compact_and_heap_readers() {
    // The whitespace before `^` exercises the parser's implicit-base path,
    // which creates the final script node after the preceding atom was parsed.
    let input = r"x ^2 + y";
    let unlimited = Config::default().with_max_nodes(None);
    let parsed = LatexReader::parse_compact(input, &unlimited).unwrap();
    let node_count = parsed.expr.arena.node_count();
    assert!(node_count > 0);

    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let exact = Config::default()
            .with_strictness(strictness)
            .with_max_nodes(Some(node_count));
        let compact = LatexReader::parse_compact(input, &exact).unwrap();
        assert_eq!(compact.expr.arena.node_count(), node_count);
        let heap = LatexReader::parse(input, &exact).unwrap();
        assert!(!heap.expr.nodes.is_empty());

        let below = exact.with_max_nodes(Some(node_count - 1));
        for result in [
            LatexReader::parse_compact(input, &below).map(|_| ()),
            LatexReader::parse(input, &below).map(|_| ()),
        ] {
            assert!(matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::Nodes,
                    limit
                }) if limit == node_count - 1
            ));
        }
    }

    let zero = Config::default().with_max_nodes(Some(0));
    assert!(matches!(
        LatexReader::parse_compact("x", &zero),
        Err(Error::LimitExceeded {
            kind: LimitKind::Nodes,
            limit: 0
        })
    ));
    assert_eq!(
        LatexReader::parse("x", &unlimited)
            .unwrap()
            .expr
            .nodes
            .len(),
        1
    );
}

#[test]
fn verbatim_macro_text_round_trips_through_public_api() {
    use crate::convert::from_latex;

    for (input, literal) in [
        (r"\verb!\def\inside{literal}!", r"\def\inside{literal}"),
        (
            r"\verb*|\frac{a}{b}\ip\newcommand{\inside}{literal}|",
            r"\newcommand{\inside}{literal}",
        ),
        (
            r"\def\outside{expanded}\verb!\def\inside{literal}!\outside",
            r"\def\inside{literal}",
        ),
    ] {
        let first = from_latex(input).unwrap().to_latex().unwrap();
        let second = from_latex(&first).unwrap().to_latex().unwrap();
        assert!(first.contains(literal), "lost verbatim content: {first:?}");
        assert_eq!(first, second, "verbatim round-trip failed for {input:?}");
    }
}

#[test]
fn round_trip_script_base_tokens() {
    let config = Config::default();
    let cases = [
        (
            "dd'a/\u{2113}'a/\u{2113}",
            r"d{d}^{\prime}a/ \ell^{\prime}a/ \ell",
        ),
        ("\u{2113}'", r"\ell^{\prime}"),
        (r"\ell'", r"\ell^{\prime}"),
        ("a/\u{2113}_2", r"a/ \ell_{2}"),
        (r"a/\ell_2'", r"a/ \ell_{2}^{\prime}"),
        ("a/\u{03B1}'", r"a/ \alpha^{\prime}"),
        (r"a/\alpha_2'", r"a/ \alpha_{2}^{\prime}"),
        ("a/\u{03B4}'", r"a/ \delta^{\prime}"),
        (r"a/\delta_2'", r"a/ \delta_{2}^{\prime}"),
        ("a/\u{221E}'", r"a/ {\infty}^{\prime}"),
        (r"a/\infty_2'", r"a/ {\infty}_{2}^{\prime}"),
        (r"a/\Finv_2'", "a/ \u{2132}_{2}^{\\prime}"),
        ("a/\u{2132}_2'", "a/ \u{2132}_{2}^{\\prime}"),
        (r"a/\textohm_2'", r"a/ \Omega_{2}^{\prime}"),
        ("a/\u{03A9}_2'", r"a/ \Omega_{2}^{\prime}"),
        (r"a/\textmu_2'", "a/ \u{00B5}_{2}^{\\prime}"),
        (r"a/\textmicro_2'", "a/ \u{00B5}_{2}^{\\prime}"),
        ("a/\u{00B5}_2'", "a/ \u{00B5}_{2}^{\\prime}"),
        (r"a/\textangstrom_2'", "a/ \u{00C5}_{2}^{\\prime}"),
        ("a/\u{00C5}_2'", "a/ \u{00C5}_{2}^{\\prime}"),
        (r"a/\textordmasculine_2'", "a/ \u{00BA}_{2}^{\\prime}"),
        ("a/\u{00BA}_2'", "a/ \u{00BA}_{2}^{\\prime}"),
        (r"a/\textordfeminine_2'", "a/ \u{00AA}_{2}^{\\prime}"),
        ("a/\u{00AA}_2'", "a/ \u{00AA}_{2}^{\\prime}"),
        (r"a/\textflorin_2'", "a/ \u{0192}_{2}^{\\prime}"),
        ("a/\u{0192}_2'", "a/ \u{0192}_{2}^{\\prime}"),
        (r"a/\circledS_2'", "a/ \u{24C8}_{2}^{\\prime}"),
        ("a/\u{24C8}_2'", "a/ \u{24C8}_{2}^{\\prime}"),
        ("\u{211C}''\t|'", r"\Re^{\prime\prime}|^{\prime}"),
        ("\\Re''\t|'", r"\Re^{\prime\prime}|^{\prime}"),
        (
            "'\u{211C}''\t|'",
            r"{{}^{\prime}\Re^{\prime\prime}}|^{\prime}",
        ),
        ("-'", r"-^{\prime}"),
        ("\u{2212}'", r"{\mathord{-}}^{\prime}"),
        (r"\middle-'", r"-^{\prime}"),
        ("a/+_2'", r"a/ +_{2}^{\prime}"),
        ("a/d_2'", r"a/ d_{2}^{\prime}"),
    ];
    for (input, expected) in cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr).unwrap();
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr).unwrap();
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(
            out1, out2,
            "script-base round-trip failed for input: {input:?}"
        );
        assert_eq!(out1, expected, "non-canonical output for input: {input:?}");
    }
}

#[test]
fn round_trip_ascii_identifier_script_bases() {
    use crate::{Format, convert};

    for (ch, written) in [
        (":", ":"),
        ("[", "["),
        ("]", "]"),
        ("{", r"{\char`\{}"),
        ("}", r"{\char`\}}"),
        ("|", r"{\char`|}"),
    ] {
        let input = format!("<math><msup><mi>{ch}</mi><mn>2</mn></msup></math>");
        let expected = format!("{written}^{{2}}");
        let out1 = convert(&input, Format::MathML, Format::LaTeX).unwrap();
        let out2 = convert(&out1, Format::LaTeX, Format::LaTeX).unwrap();
        let out3 = convert(&out2, Format::LaTeX, Format::LaTeX).unwrap();
        assert_eq!(out1, expected, "non-canonical base for identifier {ch:?}");
        assert_eq!(out1, out2, "unstable base for identifier {ch:?}");
        assert_eq!(out2, out3, "unstable reparse for identifier {ch:?}");
    }
}

#[test]
fn zero_width_nary_bodies_keep_following_tokens_stable() {
    let config = Config::default();
    for (input, expected) in [
        (r"\int\kern0pt gray", r"\int{}gray"),
        (r"\int\hspace{} gray", r"\int{}gray"),
        (r"\int\kern0pt", r"\int"),
        (r"\int\hspace{}", r"\int"),
    ] {
        let mut parsed = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut parsed.expr).unwrap();
        let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
        assert_eq!(
            first, expected,
            "unexpected zero-width output for {input:?}"
        );

        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
        normalize_compact_expr(&mut reparsed.expr).unwrap();
        let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
        assert_eq!(second, first, "zero-width n-ary body changed for {input:?}");
    }
}

#[test]
fn repeated_limit_modifiers_use_the_last_style() {
    let config = Config::default();
    let cases = [
        (
            r"\int\limits\limits\limits_2^2",
            crate::ast::LimitStyle::AboveBelow,
        ),
        (
            r"\sum\limits\nolimits\limits_i^n x",
            crate::ast::LimitStyle::AboveBelow,
        ),
        (
            r"\sum\nolimits\limits\nolimits_i^n x",
            crate::ast::LimitStyle::SubSup,
        ),
        (r"\int\limitsize_2^2", crate::ast::LimitStyle::Default),
    ];
    for (input, expected_limits) in cases {
        let mut parsed = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut parsed.expr).unwrap();
        let roots = parsed.expr.arena.children(parsed.expr.roots);
        let CompactNode::Nary { limits, .. } = *parsed.expr.arena.node(roots[0]) else {
            panic!("expected n-ary root for {input:?}");
        };
        assert_eq!(limits, expected_limits, "wrong limit style for {input:?}");

        let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
        normalize_compact_expr(&mut reparsed.expr).unwrap();
        let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
        assert_eq!(first, second, "unstable repeated modifiers for {input:?}");
    }
}

#[test]
fn named_operator_limit_modifiers_round_trip() {
    let config = Config::default();
    for (input, expected) in [
        (r"\lim\limits\nolimits", r"\lim\nolimits"),
        (r"\sin\nolimits\limits", r"\sin\limits"),
        (r"\operatorname*{argmax}\nolimits", r"\operatorname{argmax}"),
    ] {
        let mut parsed = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut parsed.expr).unwrap();
        let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
        assert_eq!(first, expected, "wrong named operator output for {input:?}");
        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
        normalize_compact_expr(&mut reparsed.expr).unwrap();
        let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
        assert_eq!(
            first, second,
            "unstable named operator output for {input:?}"
        );
    }

    let comma_mathml = crate::convert(
        r"\operatorname*{arg,min}",
        crate::Format::LaTeX,
        crate::Format::MathML,
    )
    .unwrap();
    assert!(
        comma_mathml.contains("arg,min"),
        "ordinary comma changed: {comma_mathml}"
    );
    assert!(
        !comma_mathml.contains("arg\u{2009}min"),
        "ordinary comma became a thin space: {comma_mathml}"
    );
}

#[test]
fn argmin_aliases_canonicalize_to_spaced_operator_names() {
    for (alias, spaced) in [
        (r"\argmin", r"\operatorname*{arg\,min}"),
        (r"\argmax", r"\operatorname*{arg\,max}"),
    ] {
        let alias_latex =
            crate::convert(alias, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(alias_latex, spaced, "wrong alias spelling for {alias}");
        let alias_mathml =
            crate::convert(alias, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        let spaced_mathml =
            crate::convert(spaced, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert_eq!(
            alias_mathml, spaced_mathml,
            "alias structure changed for {alias}"
        );
        assert!(
            alias_mathml.contains("arg\u{2009}"),
            "thin space missing: {alias_mathml}"
        );
    }

    for (input, expected) in [
        (r"\operatorname*{argmin}", r"\operatorname*{argmin}"),
        (r"\operatorname*{argmax}", r"\operatorname*{argmax}"),
        (r"\operatorname*{arg,min}", r"\operatorname*{arg,min}"),
    ] {
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, expected, "custom operator changed: {input}");
        assert_eq!(
            first,
            crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
            "custom operator was unstable: {input}"
        );
    }
}

#[test]
fn operatorname_special_content_round_trips_through_public_apis() {
    let config = Config::default();
    for (input, expected) in [
        (r"\operatorname{rate\%}", r"\operatorname{rate\%}"),
        (r"\operatorname*{rate\%}", r"\operatorname*{rate\%}"),
        (r"\operatorname{a\{b\}c}", r"\operatorname{a\{b\}c}"),
        (
            r"\operatorname{a\textbackslash{}b}",
            r"\operatorname{a\textbackslash{}b}",
        ),
        (
            r"\operatorname{a\textasciitilde{}b}",
            r"\operatorname{a\textasciitilde{}b}",
        ),
        (r"\operatorname{a\textmu b}", r"\operatorname{aµ b}"),
    ] {
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, expected, "unexpected operator name for {input:?}");
        let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(second, first, "operator name changed for {input:?}");
    }

    let mathml = crate::convert(
        "<math><mo>rate%</mo></math>",
        crate::Format::MathML,
        crate::Format::LaTeX,
    )
    .unwrap();
    assert_eq!(mathml, r"\operatorname{rate\%}");
    assert_eq!(
        crate::convert(&mathml, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
        mathml
    );

    let direct = crate::ast::MathExpr::inline(vec![crate::ast::MathNode::Operator(
        crate::ast::OperatorAttr {
            name: "rate%".into(),
            limit_style: crate::ast::LimitStyle::AboveBelow,
        },
    )]);
    let direct_latex = LatexWriter::write(&direct, &config).unwrap();
    assert_eq!(direct_latex, r"\operatorname*{rate\%}\limits");
    assert_eq!(
        crate::convert(&direct_latex, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
        direct_latex
    );

    for malformed in [r"\operatorname{rate\%", r"\operatorname*{rate\textmu"] {
        assert!(
            LatexReader::parse(malformed, &config).is_err(),
            "unclosed operator name was accepted: {malformed:?}"
        );
    }
}

#[test]
fn nary_body_boundaries_allow_missing_bodies_and_propagate_errors() {
    let config = Config::default();
    for input in [
        r"{\sum}",
        r"\left(\sum\right)",
        r"\begin{matrix}\sum&x\end{matrix}",
        r"\begin{matrix}x&\sum\end{matrix}",
        r"\begin{matrix}\sum\cr x\end{matrix}",
    ] {
        assert!(
            LatexReader::parse_compact(input, &config).is_ok(),
            "valid empty n-ary body boundary rejected: {input:?}"
        );
    }

    let malformed = r"{\sum\frac{a}}";
    assert!(
        LatexReader::parse_compact(malformed, &config).is_err(),
        "malformed n-ary body should propagate a strict parse error"
    );
    let lenient = config.with_strictness(Strictness::Lenient);
    let malformed_with_valid = format!("{malformed}+b");
    let recovered = LatexReader::parse(&malformed_with_valid, &lenient)
        .expect("lenient recovery should remain available for malformed n-ary bodies");
    assert!(
        recovered.has_warnings(),
        "lenient recovery should report the malformed body"
    );
    assert!(
        recovered
            .expr
            .nodes
            .iter()
            .any(|node| matches!(node, crate::ast::MathNode::Identifier(name) if name == "b")),
        "lenient recovery should continue to following valid material"
    );
    assert!(
        LatexReader::parse_compact(malformed, &lenient).is_ok(),
        "lenient recovery should remain available for malformed n-ary bodies"
    );
}

#[test]
fn middle_large_operator_delimiters_remain_scalar() {
    let config = Config::default();
    for command in [
        "sum",
        "prod",
        "coprod",
        "int",
        "iint",
        "iiint",
        "iiiint",
        "oint",
        "oiint",
        "oiiint",
        "ointctrclockwise",
        "varointclockwise",
        "bigwedge",
        "bigvee",
        "bigcap",
        "bigcup",
        "bigodot",
        "bigoplus",
        "bigotimes",
        "biguplus",
        "bigsqcup",
    ] {
        let input = format!(r"\middle\{command} t'ene");
        let mut parsed = LatexReader::parse_compact(&input, &config).unwrap();
        normalize_compact_expr(&mut parsed.expr).unwrap();
        let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
        assert!(
            first.starts_with("\\mathord{"),
            "scalar large operator lost its ordinary class: {first:?}"
        );

        let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
        normalize_compact_expr(&mut reparsed.expr).unwrap();
        let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
        assert_eq!(first, second, "scalar middle operator changed: {input:?}");
    }

    let fenced = r"\left(a\middle\int t\right)";
    let mut parsed = LatexReader::parse_compact(fenced, &config).unwrap();
    normalize_compact_expr(&mut parsed.expr).unwrap();
    let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
    let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
    normalize_compact_expr(&mut reparsed.expr).unwrap();
    let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
    assert!(
        first.contains(r"\middle") && first.contains('∫'),
        "fenced middle delimiter lost: {first:?}"
    );
    assert_eq!(first, second, "fenced middle delimiter changed");

    let real = r"\int t'ene";
    let mut parsed = LatexReader::parse_compact(real, &config).unwrap();
    normalize_compact_expr(&mut parsed.expr).unwrap();
    let first = LatexWriter::write_compact(&parsed.expr, &config).unwrap();
    let mut reparsed = LatexReader::parse_compact(&first, &config).unwrap();
    normalize_compact_expr(&mut reparsed.expr).unwrap();
    let second = LatexWriter::write_compact(&reparsed.expr, &config).unwrap();
    assert_eq!(first, second, "real n-ary body changed");

    let mathml = crate::convert(
        "<math><mo>∫</mo><mi>t</mi></math>",
        crate::Format::MathML,
        crate::Format::LaTeX,
    )
    .unwrap();
    assert!(
        mathml == "\\int t",
        "MathML large operator should remain a real integral: {mathml:?}"
    );
    assert_eq!(
        crate::convert(&mathml, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
        mathml
    );

    let direct = crate::ast::MathExpr::inline(vec![
        crate::ast::MathNode::Nary {
            op: '∫',
            sub: None,
            sup: None,
            body: Box::new(crate::ast::MathNode::Row(Vec::new())),
            limits: crate::ast::LimitStyle::Default,
        },
        crate::ast::MathNode::Superscript {
            base: Box::new(crate::ast::MathNode::Identifier("t".into())),
            sup: Box::new(crate::ast::MathNode::Symbol(
                crate::ast::SymbolKind::Ordinary,
                "′".into(),
            )),
        },
        crate::ast::MathNode::Identifier("ene".into()),
    ]);
    let direct_latex = LatexWriter::write(&direct, &config).unwrap();
    assert!(
        direct_latex.starts_with("\\int{}"),
        "explicit empty operator lost its body boundary: {direct_latex:?}"
    );
    let mut reparsed = LatexReader::parse_compact(&direct_latex, &config).unwrap();
    normalize_compact_expr(&mut reparsed.expr).unwrap();
    assert_eq!(
        LatexWriter::write_compact(&reparsed.expr, &config).unwrap(),
        direct_latex
    );
}

#[test]
fn round_trip_spacing() {
    let config = Config::default();
    let cases = [
        "a\\,b",
        "a\\:b",
        "a\\;b",
        "a\\!b",
        "a\\ b",
        "a\\quad b",
        "a\\qquad b",
        "a\\enspace b",
    ];
    for input in &cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr).unwrap();
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr).unwrap();
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(out1, out2, "spacing round-trip failed for input: {input}");
    }
}

#[test]
fn math_spacing_aliases_preserve_their_width() {
    use crate::ast::{MathNode, Rational};
    let config = Config::default();
    for (source, mu, canonical) in [
        (r"\>", 4, r"\:"),
        (r"\thinspace", 3, r"\,"),
        (r"\medspace", 4, r"\:"),
        (r"\thickspace", 5, r"\;"),
        (r"\negthinspace", -3, r"\!"),
        (r"\negmedspace", -4, r"\negmedspace "),
        (r"\negthickspace", -5, r"\negthickspace "),
        (r"\enskip", 9, r"\enspace "),
    ] {
        let input = format!("a{source} b");
        let parsed = LatexReader::parse(&input, &config).unwrap().expr;
        assert_eq!(
            parsed.nodes[1],
            MathNode::Space(Rational::new(mu, 18)),
            "{source}"
        );
        let output = LatexWriter::write(&parsed, &config).unwrap();
        assert_eq!(output, format!("a{canonical}b"));
        assert_eq!(LatexReader::parse(&output, &config).unwrap().expr, parsed);
    }
}

#[test]
fn round_trip_scaled_delimiters() {
    let config = Config::default();
    let cases = ["\\big(", "\\Big[", "\\bigg|", "\\Bigg)"];
    for input in &cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr).unwrap();
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr).unwrap();
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(
            out1, out2,
            "scaled delimiter round-trip failed for input: {input}"
        );
    }
}

#[test]
fn round_trip_command_spacing() {
    let config = Config::default();
    // \nabla followed by a plain letter needs space separation
    let input = "\\nabla p";
    let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
    normalize_compact_expr(&mut r1.expr).unwrap();
    let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

    let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
    normalize_compact_expr(&mut r2.expr).unwrap();
    let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

    assert_eq!(out1, out2, "command spacing round-trip failed");
    assert!(out1.contains("\\nabla"), "output should contain \\nabla");
}

#[test]
fn depth_exceeded() {
    let config = Config::default();
    let depth = 130;
    let mut input = String::new();
    for _ in 0..depth {
        input.push('{');
    }
    input.push('x');
    for _ in 0..depth {
        input.push('}');
    }
    let result = LatexReader::parse(&input, &config);
    assert!(result.is_err(), "expected depth exceeded error");
}

#[test]
fn empty_group() {
    let config = Config::default();
    let result = LatexReader::parse("{}", &config).unwrap();
    assert!(result.expr.nodes.is_empty() || result.expr.nodes.len() == 1);
}

#[test]
fn nested_fractions() {
    let config = Config::default();
    let input = "\\frac{\\frac{a}{b}}{\\frac{c}{d}}";
    let result = LatexReader::parse(input, &config);
    assert!(result.is_ok(), "nested fractions should parse");
}

#[test]
fn navier_stokes() {
    let config = Config::default();
    let input = "\\rho \\left( \\frac{\\partial \\mathbf{v}}{\\partial t} + \\mathbf{v} \\cdot \\nabla \\mathbf{v} \\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{v} + \\mathbf{f}";
    let result = LatexReader::parse(input, &config);
    assert!(
        result.is_ok(),
        "Navier-Stokes equation should parse: {:?}",
        result.err()
    );
}

#[test]
fn navier_stokes_round_trip() {
    let config = Config::default();
    let input = "\\rho \\left( \\frac{\\partial \\mathbf{v}}{\\partial t} + \\mathbf{v} \\cdot \\nabla \\mathbf{v} \\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{v} + \\mathbf{f}";
    let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
    normalize_compact_expr(&mut r1.expr).unwrap();
    let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

    let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
    normalize_compact_expr(&mut r2.expr).unwrap();
    let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

    assert_eq!(out1, out2, "Navier-Stokes round-trip not idempotent");
}

#[test]
fn unmatched_delimiter_strict() {
    let config = Config::default();
    let result = LatexReader::parse("\\left( x", &config);
    assert!(
        result.is_err(),
        "unmatched \\left should fail in strict mode"
    );
}

#[test]
fn unicode_commands_round_trip() {
    let config = Config::default();
    let cases = [
        "\\alpha + \\beta",
        "\\Gamma \\Delta",
        "\\infty",
        "\\partial",
        "\\nabla",
    ];
    for input in &cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr).unwrap();
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr).unwrap();
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(out1, out2, "unicode command round-trip failed for: {input}");
    }
}

#[test]
fn stretchy_accents_parse() {
    use crate::ast::compact::CompactNode;

    let config = Config::default();

    // These commands should parse without error and produce Accent nodes
    let accent_cases = [
        "\\overrightarrow{AB}",
        "\\overleftarrow{AB}",
        "\\overleftrightarrow{AB}",
        "\\overrightharpoon{x}",
        "\\overleftharpoon{x}",
        "\\widehat{abc}",
        "\\widetilde{abc}",
        "\\widecheck{abc}",
    ];
    for input in &accent_cases {
        let result = LatexReader::parse_compact(input, &config);
        assert!(
            result.is_ok(),
            "failed to parse stretchy accent: {input}: {:?}",
            result.err()
        );
        let expr = result.unwrap().expr;
        let roots = expr.arena.children(expr.roots);
        assert!(
            !roots.is_empty(),
            "stretchy accent produced empty AST: {input}"
        );
        // The root node should be an Accent
        let root_node = expr.arena.node(roots[0]);
        assert!(
            matches!(root_node, CompactNode::Accent { .. }),
            "stretchy accent did not produce Accent node for {input}, got: {:?}",
            root_node
        );
    }
}

#[test]
fn horizontal_braces_parse() {
    use crate::ast::compact::CompactNode;

    let config = Config::default();

    // Overbrace/underbrace commands remain script nodes. Brackets are typed
    // accents so they retain their square-bracket identity through LaTeX
    // serialization and do not become line segments.
    let brace_cases = [
        ("\\overbrace{x+y}", true),     // Overscript
        ("\\underbrace{x+y}", false),   // Underscript
        ("\\overbracket{x+y}", true),   // Overscript
        ("\\underbracket{x+y}", false), // Underscript
    ];
    for (input, is_over) in &brace_cases {
        let result = LatexReader::parse_compact(input, &config);
        assert!(
            result.is_ok(),
            "failed to parse brace command: {input}: {:?}",
            result.err()
        );
        let expr = result.unwrap().expr;
        let roots = expr.arena.children(expr.roots);
        assert!(
            !roots.is_empty(),
            "brace command produced empty AST: {input}"
        );
        let root_node = expr.arena.node(roots[0]);
        if input.contains("bracket") {
            assert!(
                matches!(
                    root_node,
                    CompactNode::Accent {
                        accent: '\u{23B4}' | '\u{23B5}',
                        wide: true,
                        ..
                    }
                ),
                "bracket did not produce a typed wide Accent for {input}, got: {:?}",
                root_node
            );
        } else if *is_over {
            assert!(
                matches!(root_node, CompactNode::Overscript { .. }),
                "overbrace/bracket did not produce Overscript for {input}, got: {:?}",
                root_node
            );
        } else {
            assert!(
                matches!(root_node, CompactNode::Underscript { .. }),
                "underbrace/bracket did not produce Underscript for {input}, got: {:?}",
                root_node
            );
        }
    }
}

#[test]
#[cfg(feature = "render")]
fn stretchy_accents_render_svg() {
    use crate::convert::from_latex;

    // These should produce SVG output (not error, not empty) containing path elements
    let cases = [
        "\\overrightarrow{AB}",
        "\\overleftarrow{AB}",
        "\\widehat{abc}",
        "\\widetilde{abc}",
        "\\widecheck{abc}",
        "\\overbrace{x+y}",
        "\\underbrace{a+b}",
        "\\overbracket{x+y}",
        "\\underbracket{a+b}",
        "\\overbrace{x+y}^{n}",
        "\\underbrace{a+b}_{sum}",
    ];
    for input in &cases {
        let result = from_latex(input).unwrap().to_svg();
        assert!(
            result.is_ok(),
            "SVG rendering failed for {input}: {:?}",
            result.err()
        );
        let svg = result.unwrap();
        assert!(!svg.is_empty(), "SVG output empty for {input}");
        // Stretchy decorations should produce <path> elements (not literal text)
        assert!(
            svg.contains("<path") || svg.contains("<text"),
            "SVG for {input} should contain rendered elements, got:\n{svg}"
        );
    }
}

// ---------------------------------------------------------------------------
// amscd `CD` commutative-diagram environment
// ---------------------------------------------------------------------------

#[test]
fn cd_basic_square_parses_to_array() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} A @>f>> B \\ @VVV @VVV \\ C @>>g> D \end{CD}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Laid out as an <mtable> grid.
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
    // Object letters present.
    for obj in ["A", "B", "C", "D"] {
        assert!(mathml.contains(obj), "missing object {obj} in:\n{mathml}");
    }
    // Horizontal right arrow and vertical down arrow glyphs.
    assert!(mathml.contains('\u{2192}'), "missing → arrow in:\n{mathml}");
    assert!(mathml.contains('\u{2193}'), "missing ↓ arrow in:\n{mathml}");
    // Labels appear above/below the horizontal arrows.
    assert!(
        mathml.contains("<mover"),
        "expected over-label for f in:\n{mathml}"
    );
    assert!(
        mathml.contains("<munder"),
        "expected under-label for g in:\n{mathml}"
    );
    assert!(mathml.contains(">f<"), "missing label f in:\n{mathml}");
    assert!(mathml.contains(">g<"), "missing label g in:\n{mathml}");
}

#[test]
fn cd_horizontal_chain_parses() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} X @>>> Y @>>> Z \end{CD}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(mathml.contains("<mtable>"));
    for obj in ["X", "Y", "Z"] {
        assert!(mathml.contains(obj), "missing object {obj}");
    }
    // Two right arrows in a single row.
    assert_eq!(
        mathml.matches('\u{2192}').count(),
        2,
        "expected two → in:\n{mathml}"
    );
}

#[test]
fn cd_cr_rows_and_escaped_braces_keep_their_grid_shape() {
    use crate::convert::from_latex;
    for src in [
        r"\begin{CD} A \cr B \end{CD}",
        r"\begin{CD} \{ @>>> B \cr C @>>> \} \end{CD}",
    ] {
        let first = crate::convert(src, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(first, second, "{src} => {first:?}");
        let mathml = from_latex(src).unwrap().to_mathml().unwrap();
        assert_eq!(mathml.matches("<mtr>").count(), 2, "{src}: {mathml}");
    }
}

#[test]
fn cd_equals_and_double_line_parse() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} A @= B \\ @VVV @| \\ C @>>> D \end{CD}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(mathml.contains("<mtable>"));
    // Horizontal equals (`@=`).
    assert!(mathml.contains(">=<"), "missing = for @= in:\n{mathml}");
    // Vertical double line (`@|`).
    assert!(
        mathml.contains('\u{2016}'),
        "missing ‖ for @| in:\n{mathml}"
    );
    // Down arrow and right arrow.
    assert!(mathml.contains('\u{2193}'));
    assert!(mathml.contains('\u{2192}'));
}

#[test]
fn cd_vert_is_single_bar_distinct_from_double() {
    use crate::convert::from_latex;
    // `@\vert` is a single bar `|`; `@|` / `@\Vert` are the double bar `‖`.
    let single = from_latex(r"\begin{CD} A @VVV B \\ @\vert @\vert \\ C @>>> D \end{CD}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        single.contains(">|</mo>"),
        "expected single bar | for @\\vert, got:\n{single}"
    );
    let double = from_latex(r"\begin{CD} A @VVV B \\ @\Vert @\Vert \\ C @>>> D \end{CD}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        double.contains('\u{2016}'),
        "expected double bar ‖ for @\\Vert, got:\n{double}"
    );
}

#[test]
fn cd_vertical_labels_and_left_arrow() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} A @V{g}V{h}V B @<<< C \end{CD}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(mathml.contains('\u{2193}'), "missing ↓ in:\n{mathml}");
    assert!(mathml.contains('\u{2190}'), "missing ← in:\n{mathml}");
    assert!(
        mathml.contains(">g<") && mathml.contains(">h<"),
        "missing v-labels in:\n{mathml}"
    );
}

#[test]
fn cd_malformed_does_not_panic() {
    use crate::convert::from_latex;
    // Bodies that are degenerate must recover (no panic).
    let _ = from_latex(r"\begin{CD} \end{CD}");
    let _ = from_latex(r"\begin{CD} @>>> \end{CD}");
    let _ = from_latex(r"\begin{CD} @ @. @VV \end{CD}");
    let _ = from_latex(r"\begin{CD} A @>{f\circ g}>> B \end{CD}");
}

#[test]
#[cfg(feature = "render")]
fn cd_renders_svg() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} A @>f>> B \\ @VVV @VVV \\ C @>>g> D \end{CD}";
    let svg = from_latex(src).unwrap().to_svg().unwrap();
    assert!(!svg.is_empty(), "SVG output empty for CD");
    assert!(
        svg.contains("<path") || svg.contains("<text"),
        "no rendered elements:\n{svg}"
    );
}

// ---------------------------------------------------------------------------
// cases package: numcases / subnumcases
// ---------------------------------------------------------------------------

#[test]
fn numcases_builds_braced_grid_with_number_labels() {
    use crate::convert::from_latex;
    let src = r"\begin{numcases}{|x|} x, & x\ge0\\ -x, & x<0 \end{numcases}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Cases rows live in an <mtable> grid wrapped in a big left brace fence.
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
    assert!(
        mathml.contains('{'),
        "expected big left brace, got:\n{mathml}"
    );
    // Shared left expression and the `=`.
    assert!(mathml.contains(">x<"), "missing x in:\n{mathml}");
    assert!(mathml.contains(">=<"), "missing = in:\n{mathml}");
    // `numcases` numbers each row with a full master number: (1), (2).
    assert!(mathml.contains("(1)"), "missing (1) label in:\n{mathml}");
    assert!(mathml.contains("(2)"), "missing (2) label in:\n{mathml}");
    // Conditions present.
    assert!(mathml.contains('\u{2265}'), "missing ≥ in:\n{mathml}");
}

#[test]
fn numcases_and_subnumcases_label_differently() {
    use crate::convert::from_latex;
    // numcases → (1),(2); subnumcases → (1a),(1b). They must not be identical.
    let num = from_latex(r"\begin{numcases}{f} a & p\\ b & q \end{numcases}")
        .unwrap()
        .to_mathml()
        .unwrap();
    let sub = from_latex(r"\begin{subnumcases}{f} a & p\\ b & q \end{subnumcases}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(num.contains("(1)") && num.contains("(2)"), "got:\n{num}");
    assert!(
        !num.contains("(1a)"),
        "numcases must not use sub-letters:\n{num}"
    );
    assert!(sub.contains("(1a)") && sub.contains("(1b)"), "got:\n{sub}");
    assert_ne!(num, sub, "numcases and subnumcases must differ");
}

#[test]
fn subnumcases_uses_sub_letter_labels() {
    use crate::convert::from_latex;
    let src = r"\begin{subnumcases}{f} a & p\\ b & q \end{subnumcases}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
    assert!(mathml.contains("(1a)"), "missing (1a) label in:\n{mathml}");
    assert!(mathml.contains("(1b)"), "missing (1b) label in:\n{mathml}");
    // Shared left expression `f`.
    assert!(mathml.contains(">f<"), "missing f in:\n{mathml}");
}

#[test]
fn numcases_malformed_does_not_panic() {
    use crate::convert::from_latex;
    let _ = from_latex(r"\begin{numcases} \end{numcases}");
    let _ = from_latex(r"\begin{numcases}{ \end{numcases}");
    let _ = from_latex(r"\begin{numcases}{x}\end{numcases}");
    let _ = from_latex(r"\begin{subnumcases}{}\end{subnumcases}");
}

// ---------------------------------------------------------------------------
// empheq package: boxed / delimited displays
// ---------------------------------------------------------------------------

#[test]
fn empheq_box_wraps_inner_align_in_boxed() {
    use crate::convert::from_latex;
    let src = r"\begin{empheq}[box=\fbox]{align} a &= b\\ c &= d \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Boxed renders as <menclose notation="box">.
    assert!(
        mathml.contains("menclose notation=\"box\""),
        "expected boxed menclose, got:\n{mathml}"
    );
    // Inner aligned grid is preserved.
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
    assert!(
        mathml.contains(">a<") && mathml.contains(">d<"),
        "missing cells in:\n{mathml}"
    );
}

#[test]
fn empheq_left_brace_delimiter_and_combined_box() {
    use crate::convert::from_latex;
    // `left=\{` (escaped brace) plus a combined `box=` key must both apply.
    let src = r"\begin{empheq}[left=\{,box=\fbox]{align} a &= b\\ c &= d \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(
        mathml.contains(">{</mo>"),
        "expected a left brace delimiter, got:\n{mathml}"
    );
    assert!(
        mathml.contains("menclose notation=\"box\""),
        "expected the box to also apply, got:\n{mathml}"
    );
}

#[test]
fn empheq_innerbox_key_and_empheqset_accepted() {
    use crate::convert::from_latex;
    // `\empheqset{...}` is accepted/ignored; `innerbox=` is a box alias.
    let src = r"\empheqset{box=\fbox}\begin{empheq}[innerbox=\fbox]{align} a &= b \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(
        mathml.contains("menclose notation=\"box\""),
        "expected innerbox to wrap in a box, got:\n{mathml}"
    );
}

#[test]
fn colorbox_optional_model_resolves() {
    use crate::convert::from_latex;
    // \colorbox[rgb]{1,0,0}{x} — optional model must parse and resolve to red.
    let mathml = from_latex(r"\colorbox[rgb]{1,0,0}{x}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        mathml.contains("background-color: #ff0000"),
        "expected red background from rgb model, got:\n{mathml}"
    );
    // \fcolorbox[RGB]{0,0,255}{255,255,0}{z} — both specs use the model.
    let m2 = from_latex(r"\fcolorbox[RGB]{0,0,255}{255,255,0}{z}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        m2.contains("border-color: #0000ff") && m2.contains("background-color: #ffff00"),
        "expected blue border + yellow bg, got:\n{m2}"
    );
}

#[test]
fn empheq_colorbox_sets_background() {
    use crate::convert::from_latex;
    let src = r"\begin{empheq}[box=\colorbox{yellow}]{align} a &= b \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(
        mathml.contains("menclose notation=\"box\""),
        "expected boxed menclose, got:\n{mathml}"
    );
    assert!(
        mathml.contains("background-color"),
        "expected background color, got:\n{mathml}"
    );
}

#[test]
fn empheq_without_options_renders_inner_unchanged() {
    use crate::convert::from_latex;
    let src = r"\begin{empheq}{gather} x \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // No box.
    assert!(
        !mathml.contains("menclose notation=\"box\""),
        "did not expect a box, got:\n{mathml}"
    );
    // Inner gather grid present.
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
    assert!(mathml.contains(">x<"), "missing x in:\n{mathml}");
}

#[test]
fn empheq_left_delimiter_wraps_in_fence() {
    use crate::convert::from_latex;
    let src = r"\begin{empheq}[left=\Rightarrow]{align*} a &= b \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Left delimiter ⇒ should appear as a fence operator.
    assert!(
        mathml.contains('\u{21D2}'),
        "missing ⇒ delimiter in:\n{mathml}"
    );
    assert!(
        mathml.contains("<mtable>"),
        "expected mtable, got:\n{mathml}"
    );
}

#[test]
fn empheq_malformed_does_not_panic() {
    use crate::convert::from_latex;
    let _ = from_latex(r"\begin{empheq}{align}\end{empheq}");
    let _ = from_latex(r"\begin{empheq}[box=]{align} a \end{empheq}");
    let _ = from_latex(r"\begin{empheq}[box=\fbox]{}\end{empheq}");
    let _ = from_latex(r"\begin{empheq}[bogus=1]{align} a &= b \end{empheq}");
    let _ = from_latex(r"\begin{empheq}\end{empheq}");
}

// ---------------------------------------------------------------------------
// colortbl: \cellcolor / \rowcolor / \columncolor / \arrayrulecolor
// ---------------------------------------------------------------------------

#[test]
fn colortbl_rowcolor_and_cellcolor_set_backgrounds() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{cc} \rowcolor{gray} a & b \\ \cellcolor{yellow} c & d \end{array}";
    let res = from_latex(src).unwrap();
    let mathml = res.to_mathml().unwrap();
    // \rowcolor colors both cells of row 1; \cellcolor colors one cell of row 2.
    assert_eq!(
        mathml.matches("background-color").count(),
        3,
        "expected 3 colored cells, got:\n{mathml}"
    );
    assert!(
        mathml.contains("menclose notation=\"box\""),
        "expected boxed menclose, got:\n{mathml}"
    );
    // Cell contents survive the Boxed wrapping.
    assert!(
        mathml.contains(">a<") && mathml.contains(">d<"),
        "missing cells in:\n{mathml}"
    );
}

#[cfg(feature = "render")]
#[test]
fn colortbl_cellcolor_renders_filled_rect_in_svg() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{c} \cellcolor{red!30} x \end{array}";
    let res = from_latex(src).unwrap();
    let mathml = res.to_mathml().unwrap();
    // `red!30` is a computed xcolor mix, so it resolves to a #rrggbb value.
    assert!(
        mathml.contains("background-color: #"),
        "expected resolved hex background, got:\n{mathml}"
    );
    let svg = res.to_svg().unwrap();
    // Boxed background paints a filled <rect>.
    assert!(svg.contains("<rect"), "expected a rect in svg:\n{svg}");
    assert!(
        svg.contains("fill=\"#"),
        "expected a filled rect in svg:\n{svg}"
    );
}

#[test]
fn colortbl_inline_columncolor_colors_its_cell() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{cc} \columncolor{blue} p & q \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert_eq!(
        mathml.matches("background-color").count(),
        1,
        "expected the columncolor cell to be filled, got:\n{mathml}"
    );
}

#[test]
fn colortbl_rowcolors_alternates_rows() {
    use crate::convert::from_latex;
    // \rowcolors{1}{red}{blue} → row1 red, row2 blue, row3 red.
    let src = r"\rowcolors{1}{red}{blue}\begin{array}{cc} a & b \\ c & d \\ e & f \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert_eq!(
        mathml.matches("background-color: red").count(),
        4,
        "rows 1 and 3 (4 cells) should be red, got:\n{mathml}"
    );
    assert_eq!(
        mathml.matches("background-color: blue").count(),
        2,
        "row 2 (2 cells) should be blue, got:\n{mathml}"
    );
}

#[test]
fn colortbl_rowcolors_respects_cellcolor_override() {
    use crate::convert::from_latex;
    let src =
        r"\rowcolors{1}{red}{blue}\begin{array}{cc} \cellcolor{yellow} a & b \\ c & d \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert_eq!(mathml.matches("background-color: yellow").count(), 1);
    assert_eq!(mathml.matches("background-color: red").count(), 1);
    assert_eq!(mathml.matches("background-color: blue").count(), 2);
}

#[test]
fn colortbl_columncolor_spec_colors_whole_column() {
    use crate::convert::from_latex;
    // >{\columncolor{green}} in the spec colors the entire first column.
    let src = r"\begin{array}{>{\columncolor{green}}c c} a & b \\ c & d \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert_eq!(
        mathml.matches("background-color: green").count(),
        2,
        "both first-column cells should be green, got:\n{mathml}"
    );
}

#[test]
fn colortbl_arrayrulecolor_is_accepted_but_ignored() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{cc} \arrayrulecolor{green} a & b \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Deferred: no background is produced for rule colors.
    assert!(
        !mathml.contains("background-color"),
        "arrayrulecolor should not produce a cell background, got:\n{mathml}"
    );
    assert!(
        mathml.contains(">a<") && mathml.contains(">b<"),
        "missing cells in:\n{mathml}"
    );
}

#[test]
fn colortbl_overhang_args_are_ignored() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{cc} \rowcolor{gray}[2pt][3pt] a & b \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert_eq!(
        mathml.matches("background-color").count(),
        2,
        "expected overhang args ignored and row colored, got:\n{mathml}"
    );
}

#[test]
fn colortbl_malformed_color_does_not_panic() {
    use crate::convert::from_latex;
    // Missing braces / args must degrade gracefully without panicking.
    let _ = from_latex(r"\begin{array}{cc} \cellcolor a & b \end{array}");
    let _ = from_latex(r"\begin{array}{cc} \rowcolor & b \end{array}");
    let _ = from_latex(r"\begin{array}{c} \cellcolor{ \end{array}");
    let _ = from_latex(r"\begin{matrix} \rowcolor{red} a & b \end{matrix}");
}

// ── bussproofs (proof trees) ───────────────────────────────────────────

#[test]
fn bussproofs_binary_inference() {
    use crate::convert::from_latex;
    let r =
        from_latex(r"\begin{prooftree}\AxiomC{A}\AxiomC{B}\BinaryInfC{C}\end{prooftree}").unwrap();
    let ml = r.to_mathml().unwrap();
    // Leaves and conclusion present.
    assert!(ml.contains("<mi>A</mi>"), "missing A:\n{ml}");
    assert!(ml.contains("<mi>B</mi>"), "missing B:\n{ml}");
    assert!(ml.contains("<mi>C</mi>"), "missing C:\n{ml}");
    // Inference line: Rule renders as <mspace> with explicit height.
    assert!(ml.contains("<mspace"), "missing inference rule:\n{ml}");
    assert!(ml.contains("height=\"0.1em\""), "rule has no height:\n{ml}");
    // Composed from an Array → <mtable>.
    assert!(ml.contains("<mtable>"), "not an Array:\n{ml}");
    // SVG must render without panic.
    #[cfg(feature = "render")]
    assert!(r.to_svg().is_ok());
}

#[test]
fn bussproofs_unary_chain_with_label() {
    use crate::convert::from_latex;
    let r = from_latex(
        r"\begin{prooftree}\AxiomC{$P$}\UnaryInfC{$Q$}\RightLabel{r}\UnaryInfC{$R$}\end{prooftree}",
    )
    .unwrap();
    let ml = r.to_mathml().unwrap();
    assert!(ml.contains("<mi>P</mi>"), "missing P:\n{ml}");
    assert!(ml.contains("<mi>Q</mi>"), "missing Q:\n{ml}");
    assert!(ml.contains("<mi>R</mi>"), "missing R:\n{ml}");
    assert!(ml.contains("<mi>r</mi>"), "missing label r:\n{ml}");
    assert!(ml.contains("<mspace"), "missing inference rule:\n{ml}");
}

#[test]
fn bussproofs_inline_display_proof() {
    use crate::convert::from_latex;
    let r = from_latex(r"\AxiomC{A}\UnaryInfC{B}\DisplayProof").unwrap();
    let ml = r.to_mathml().unwrap();
    assert!(
        ml.contains("<mi>A</mi>") && ml.contains("<mi>B</mi>"),
        "{ml}"
    );
    assert!(ml.contains("<mtable>"), "{ml}");
}

#[test]
fn bussproofs_lenient_underflow() {
    use crate::convert::from_latex;
    // Stack underflow / leftovers must not panic.
    assert!(from_latex(r"\begin{prooftree}\UnaryInfC{X}\end{prooftree}").is_ok());
    assert!(from_latex(r"\begin{prooftree}\BinaryInfC{X}\end{prooftree}").is_ok());
    assert!(from_latex(r"\begin{prooftree}\AxiomC{A}\AxiomC{B}\end{prooftree}").is_ok());
    assert!(from_latex(r"\begin{prooftree}\end{prooftree}").is_ok());
}

#[test]
fn bussproofs_line_styles() {
    use crate::convert::from_latex;
    // \noLine suppresses the inference rule (no filled rect / mspace gap only).
    let no_line = from_latex(r"\begin{prooftree}\AxiomC{A}\noLine\UnaryInfC{B}\end{prooftree}")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(
        no_line.contains("<mtable"),
        "expected proof grid: {no_line}"
    );
    // \doubleLine draws two stacked rules (nested table) and must not panic.
    assert!(
        from_latex(r"\begin{prooftree}\AxiomC{A}\doubleLine\UnaryInfC{B}\end{prooftree}").is_ok()
    );
    // \singleLine / \solidLine / \dottedLine are accepted.
    assert!(
        from_latex(r"\begin{prooftree}\AxiomC{A}\dottedLine\UnaryInfC{B}\end{prooftree}").is_ok()
    );
}

// ── xypic (xymatrix) ───────────────────────────────────────────────────

#[test]
fn xymatrix_grid_with_arrows() {
    use crate::convert::from_latex;
    let r = from_latex(r"\xymatrix{ A \ar[r]^f & B \ar[d] \\ C & D \ar[l] }").unwrap();
    let ml = r.to_mathml().unwrap();
    // Array grid.
    assert!(ml.contains("<mtable>"), "not an Array:\n{ml}");
    // Objects present.
    for obj in ["A", "B", "C", "D"] {
        assert!(
            ml.contains(&format!("<mi>{obj}</mi>")),
            "missing {obj}:\n{ml}"
        );
    }
    // Arrow glyphs for each direction.
    assert!(ml.contains('\u{2192}'), "missing → :\n{ml}"); // r
    assert!(ml.contains('\u{2193}'), "missing ↓ :\n{ml}"); // d
    assert!(ml.contains('\u{2190}'), "missing ← :\n{ml}"); // l
    // Arrow label.
    assert!(ml.contains("<mi>f</mi>"), "missing label f:\n{ml}");
    #[cfg(feature = "render")]
    assert!(r.to_svg().is_ok());
}

#[test]
fn xymatrix_cr_keeps_empty_row_shape_and_stable_export() {
    use crate::convert::from_latex;

    let write =
        |input: &str| crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    let first = write(r"\xymatrix{\cr}");
    assert!(
        !first.contains(r"\\"),
        "empty xymatrix row gained a separator: {first:?}"
    );
    assert_eq!(first, write(&first));

    let mathml = from_latex(r"\xymatrix{\cr}").unwrap().to_mathml().unwrap();
    assert_eq!(mathml.matches("<mtr>").count(), 1, "{mathml}");
    assert_eq!(mathml.matches("<mtd ").count(), 1, "{mathml}");

    for input in [
        r"\xymatrix{A\cr B}",
        r"\xymatrix{&A\cr B&}",
        r"\xymatrix{A\\B}",
        r"\begin{xy}\xymatrix{\cr}\end{xy}",
    ] {
        let first = write(input);
        assert_eq!(first, write(&first), "{input} => {first:?}");
    }
}

#[test]
fn xymatrix_tolerates_ar_modifiers_and_spacing() {
    use crate::convert::from_latex;
    // `\ar` shaft/curve modifiers and `@R=`/`@C=` spacing must not break the
    // arrow direction.
    let ml = from_latex(r"\xymatrix@R=1pc@C=2pc{ A \ar@{->}[r] & B \ar@/^1pc/[d]^f \\ C & D }")
        .unwrap()
        .to_mathml()
        .unwrap();
    assert!(ml.contains("<mtable>"), "expected grid:\n{ml}");
    assert!(
        ml.contains('\u{2192}'),
        "missing → from \\ar@{{->}}[r]:\n{ml}"
    );
    assert!(
        ml.contains('\u{2193}'),
        "missing ↓ from \\ar@/^1pc/[d]:\n{ml}"
    );
    assert!(
        ml.contains("<mi>f</mi>"),
        "missing curved-arrow label:\n{ml}"
    );
}

#[test]
fn xymatrix_diagonal_and_env() {
    use crate::convert::from_latex;
    let r = from_latex(r"\begin{xy}\xymatrix{ A \ar[ur]^g & B }\end{xy}").unwrap();
    let ml = r.to_mathml().unwrap();
    assert!(ml.contains('\u{2197}'), "missing ↗ :\n{ml}"); // ur
    assert!(ml.contains("<mi>g</mi>"), "missing label g:\n{ml}");
}

#[test]
fn xymatrix_lenient_malformed() {
    use crate::convert::from_latex;
    // Must never panic (unterminated input may report a parse error, but a
    // well-formed-but-empty diagram parses cleanly).
    let _ = from_latex(r"\xymatrix{");
    let _ = from_latex(r"\xymatrix{ A \ar[ & B }");
    assert!(from_latex(r"\xymatrix{}").is_ok());
    assert!(from_latex(r"\begin{xy}\end{xy}").is_ok());
}

#[test]
fn fuzz_repro_dd() {
    let config = Config::default();
    // These inputs previously caused parser panics: span underflow after
    // builtin-macro expansion, and out-of-bounds indexing when a \big-like
    // command is followed by an empty or non-ASCII delimiter argument.
    for input in [
        r"*\*\dd",
        "\0\0\0\0\0\0\\dd",
        r"\d\big",
        "\\bigg\u{06C5}",
        r"\url{",
        r"\middle~",
    ] {
        if let Ok(parsed) = LatexReader::parse(input, &config) {
            let _ = LatexWriter::write(&parsed.expr, &config);
        }
    }
}

#[test]
fn fuzz_repro_roundtrip_misc() {
    use crate::ast::normalize::normalize_compact_expr;
    let config = Config::default();
    // Inputs that previously broke LaTeX round-trip idempotency:
    // "~" was mangled by trimming, "\&" was written as a bare "&",
    // "'/\n'" triggered a prime-handling regression, "^$" is an empty-base
    // superscript with a special char, and "26$\n \n'1" mixes $/newline/primes.
    for input in [
        "\u{2212}-B",
        "A\u{2062}",
        r"\teal^\teal",
        r"\teal^\teal^\utilde'",
        "$\u{2004}-'\t'\t\t",
        "±|@",
        "×x",
        "≤x",
        "\n\\&",
        "~",
        "/\n'",
        "^$",
        "^6\n6",
        "\\Z\\,f,\\,f,ffff6*ff6)?\n^66)??\n^6\n6",
        "26$\n \n'1",
        "P6$\n6'\n1",
        "'ev$'ev",
        "''\t_\t$",
        "'_`_@",
        "'^'^\\prim",
        "mapmodx'\\verb*an\\cw\\o\\mand",
        "p'pmod;;;;\\verb_;;;;;;;;;;;;\n'pp'`p'p'p''p'`",
        "\\verb*&d'7vvvv~vvvo_'\n^oc|",
        "\\bigg\\bg",
        "\\bigg\\b_ ? \\b_ ?i",
        "f^6)\\p!\\ip\\ip?\n^2)6)?\n^?\n^6)?\n^6)6)?\n^6)",
        "\\red\n\n\\lnc",
        "5.\r6'~''''",
        ",\\boxed\\tfracboxed\\redlime?6",
        "\\ip!le_3",
        "2_\r2\r2",
        "$\\$\\intop$\\$jitt",
        "6$\n1\n6$\r\n\n1\n6(\n1$\n$\n1\n6$^7_1\n1",
        "d_7^\\newcommand",
        "<<\\,<<<\\,<d\\def=f_dd\\def=f_\\over]f\\;s\\overd",
        "\\verb*arg&d^o\u{2}\0\0 \0\0\0:Big~,\n,=,=i!p~\\\n,$g|,\\ip!\n\\",
        "\\unders\\underset\\undet\\und",
        "_6$\n1\n$\n0$\n1\n'_9A",
        "_'^$",
        "\\atop^o",
        "Bigp\\Bigg\\o'",
        "\\tt a\\brace b",
        "/\\nGGGGGGGLGGGGGGGGYYY++6\\ip+{\\ip+{ot}YYYYYYYYYYYYYYYYYY}YYYYYYYYYYYYYYYYYYYYYYYYBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBYYYY  ,YYYY",
        "\\genfrac'{\\}.{\\}{}{}{}{}",
        "6_'^1\n1$\n_\'^1\n1'",
        "\\genfrac;;;;;;;;\\|v;;;!!!!!!!!!!!",
        "\\verb's`_\\verb's`_*e\\middle *er_\\verb's .;H_{",
        "\\not< <",
        "\\lll+ llllll(",
        "\\intop_\\t$ '",
        r"\\left\\{\\frac{a}{\\left\\langle \\brace \\right|}\\right\\}",
        "_'sTfc(\\middle~\\s-'",
        " ' \\middle' oe~\\middle-' ' \\middle' oe \\middloe'",
        "'_\u{211C}?'_\u{211C}",
        ",_v76_\\color{\n5sf\n_v}",
        ";\\int\\pod \\int\\podop ",
        "$\\\\$\\\\$\\idotsint$\\\\$jitt",
        "\\text{\\fonment{e\\\t- *\\} '\\t& (*a\\ngln#}  (",
        "\\verb*&%\\verbb*&\\ver{} irnB cig)t\\ip!\\newe!\\color\t {+'\t\t\\ip!\\newbox]onmeox]onmeXXXXX\r\nB)\\newenv      \t{} irnBJJJJJJJJJJJJJJJJJJ\\middle^oc|",
        "xAg'\\ov       {} irn\\middle\"!\\newe \\color\t {+Yp!\\newe!\\color\t ~   {+Yp n!{e\\ov       {} irn\\midd~e !\\newe \\color\t {+YvvoBivv&ip    e\\ov       {} irn\\middlvev\\%",
        "]//\\su7\r        m{7{mbox'}$6}'-\\i7\r*\\} '\\text{t\\kad[4`1quad[ `;.etxd[ lan`\\{}(",
        "6 '\r\r\r\r\r\r\r7^7\r\r\r\r\r\r.\r\r7^7_$'7",
        "\\ip{op$'},",
        "Tdv  dman;sm\\Big\\label:'r]    m\\label:     dman;sm\\label:'r]'f\\otw''6d\\{\\d",
        "\\d\\begin{1}''~\n\n,'ddefe\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\22222222222\\\\      ",
        "\\Big\\ggg-gggggggggggggggggggggggggggggggggggggggggggggggggg  ",
        "_ss\n\nss\nsf\\Big\t/g\t_\tf",
        "~\\color\t {+{} _o",
        "\\floor\n\nv\r\\bigg\\t",
        ")\\color\t{ {}'@",
        "\n\\genfrac\\eo>>>eo",
        "$\\$\\choose   \\newmmncaod ",
        "\u{03C1}\u{02C1}\r'",
        "m\\textcolor{'_'}=m",
        "]_1.$_W",
        r#",[[f{]
?\ce{[[[{[f]
{
\f{]
  [^07Qne{{
\sf{d     b}}  b}}}}}}"#,
        "\\choose|\r'",
        ",\\bigm\\pu{rm #}c",
        r#"e\\substack{o\\kern}o"#,
        "=\\\\kern\r",
        r#"\\v/\\kern+a"#,
        r#"=\\kern'"#,
        "\\ip \\idotsint t\t",
        " {\\,\\xymatrix[{ {\\cr}+}}",
        "\\big \\int\\ ",
        ".$'$\\choose",
        "1F_\\AXC\\AXC ",
        "' \\kern_O",
    ] {
        let Ok(mut r1) = LatexReader::parse_compact(input, &config) else {
            continue;
        };
        normalize_compact_expr(&mut r1.expr).unwrap();
        let Ok(out1) = LatexWriter::write_compact(&r1.expr, &config) else {
            continue;
        };
        let Ok(mut r2) = LatexReader::parse_compact(&out1, &config) else {
            panic!(
                "round-trip failed: first write output could not be re-parsed.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}"
            );
        };
        normalize_compact_expr(&mut r2.expr).unwrap();
        let Ok(out2) = LatexWriter::write_compact(&r2.expr, &config) else {
            panic!(
                "round-trip failed: second write panicked.\n\
                 input:  {input:?}\n\
                 write1: {out1:?}"
            );
        };
        assert_eq!(
            out1, out2,
            "LaTeX round-trip not idempotent.\n\
             input:  {input:?}\n\
             write1: {out1:?}\n\
             write2: {out2:?}"
        );
    }
}

#[test]
fn empty_base_prime_after_middle_stays_stable_in_nested_fraction() {
    let input = "''>_-'\\ip\\over'_- '\\i\\ip\\r'enew^iJ";
    let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap(),
        crate::convert(&first, crate::Format::LaTeX, crate::Format::MathML).unwrap(),
    );
}

#[test]
fn prescript_and_scripted_middle_canonicalize_as_one_fenced_sequence() {
    let input = "''>_-'\\ip\t\t\t''\t\t\t\t\t\t\t\t\t\t\t'\t\t\t\t\\r'enew\\p\\r'>_-'\\ip\\r'enew>_-'\\ip\\r'enew\\pw>_\\r'>_-'\\ip\\r'enew^iJ";
    let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    let second = crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap(),
        crate::convert(&first, crate::Format::LaTeX, crate::Format::MathML).unwrap(),
    );
}

#[test]
fn token_selectors_choose_before_expanding() {
    for (input, expected) in [
        (r"\@firstoftwo{ab}{\missing}", "ab"),
        (r"\@secondoftwo{\frac\bad}{xy}", "xy"),
        (r"\@ifstar{a}{b}  *!", "a!"),
        (r"\@ifstar{a}{b}  ?!", "b?!"),
        (r"\@ifnextchar{\alpha}{a}{b}  \alpha x", r"a\alpha x"),
        (r"\@ifnextchar{ab}{a}{b}ab", "bab"),
        (r"\def\a{x}\@firstoftwo{}{\def\a{y}}\a", "x"),
        (r"\def\pick{\@ifnextchar x{a}{b}}\pick x", "ax"),
        (
            r"\let\pick\@ifstar\def\@ifstar{z}\pick{a}{b}*\@ifstar",
            "az",
        ),
    ] {
        assert_eq!(
            crate::from_latex(input).unwrap().to_mathml().unwrap(),
            crate::from_latex(expected).unwrap().to_mathml().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn text_or_math_follows_the_actual_argument_mode() {
    for (input, expected) in [
        (r"x+\TextOrMath{text}{math}", "x+math"),
        (
            r"\text{hello \TextOrMath{world}{wrong}}",
            r"\text{hello world}",
        ),
        (
            r"\text{$x+\TextOrMath{bad}{y}$ \TextOrMath{cost}{bad}}",
            r"\text{$x+y$ cost}",
        ),
        (
            r"\text{\(\TextOrMath{bad}{x}\) \$\TextOrMath{yes}{bad}}",
            r"\text{\(x\) \$yes}",
        ),
        (
            r"\def\pick{\TextOrMath{t}{m}}\textbf\pick+\pick",
            r"\textbf{t}+m",
        ),
        (r"\let\label\text\label{\TextOrMath{t}{m}}", r"\text{t}"),
        (r"\angl{\TextOrMath{n}{bad}}", r"\angl{n}"),
        (r"\reflectbox{\TextOrMath{abc}{bad}}", r"\reflectbox{abc}"),
        (
            r"\mathreflectbox{\TextOrMath{bad}{x^2}}",
            r"\mathreflectbox{x^2}",
        ),
        (r"\def\a{x}\TextOrMath{\def\a{y}}{}\a", "x"),
    ] {
        assert_eq!(
            crate::from_latex(input).unwrap().to_mathml().unwrap(),
            crate::from_latex(expected).unwrap().to_mathml().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn binrel_uses_the_sample_class_and_writes_only_the_body() {
    for (args, equivalent) in [
        ("+xR", r"\mathbin{x}R"),
        ("=xR", r"\mathrel{x}R"),
        (" xxR", r"\mathord{x}R"),
        (r"{\times x}{a+b}R", r"\mathbin{a+b}R"),
        (r"{\leq x}{\alpha}R", r"\mathrel{\alpha}R"),
        (r"{x+}{a}R", r"\mathord{a}R"),
        (r"{{+}}{a}R", r"\mathord{a}R"),
        (r"{}{a}R", r"\mathord{a}R"),
    ] {
        let input = format!(r"L\@binrel{args}");
        let expected = format!("L{equivalent}");
        let canonical = crate::from_latex(&input).unwrap().to_latex().unwrap();
        assert_eq!(
            canonical,
            crate::from_latex(&expected).unwrap().to_latex().unwrap(),
            "{input}"
        );
        assert_eq!(
            canonical,
            crate::from_latex(&canonical).unwrap().to_latex().unwrap()
        );
    }
    assert!(crate::from_latex(r"\@binrel{+}").is_err());
    assert!(crate::from_latex(r"\@binrelExtra{+}{x}").is_err());
    assert_eq!(
        crate::from_latex(r"\alpha@x").unwrap().to_latex().unwrap(),
        r"\alpha@x"
    );
}

#[test]
fn relation_aliases_keep_their_glyphs_in_fences_and_math() {
    for (source, expected) in [
        (r"a\lt b\gt c", "a<b>c"),
        (
            r"\left\lt\frac1x\right\gt",
            r"\left\langle\frac1x\right\rangle",
        ),
        (r"a\mapsfrom b", "a↤b"),
    ] {
        let a = crate::from_latex(source).unwrap();
        let b = crate::from_latex(expected).unwrap();
        assert_eq!(a.to_mathml().unwrap(), b.to_mathml().unwrap(), "{source}");
        let written = a.to_latex().unwrap();
        assert_eq!(
            crate::from_latex(&written).unwrap().to_latex().unwrap(),
            written
        );
    }
}

#[test]
fn vertical_bar_commands_work_as_middle_delimiters_and_preserve_atom_class() {
    for (named, literal) in [
        (
            r"\left\langle a\middle\vert b\right\rangle",
            r"\left\langle a\middle|b\right\rangle",
        ),
        (
            r"\left\Vert a\middle\Vert b\right\Vert",
            r"\left\|a\middle\|b\right\|",
        ),
    ] {
        assert_eq!(
            crate::from_latex(named).unwrap().to_mathml().unwrap(),
            crate::from_latex(literal).unwrap().to_mathml().unwrap()
        );
    }
    for src in [
        r"a\vert b",
        r"a\Vert b",
        r"\lvert x\rvert^2",
        r"\lVert x\rVert",
    ] {
        let parsed = LatexReader::parse(src, &Config::default()).unwrap().expr;
        let first = LatexWriter::write(&parsed, &Config::default()).unwrap();
        let second = LatexReader::parse(&first, &Config::default()).unwrap().expr;
        assert_eq!(parsed.nodes, second.nodes, "{src} => {first}");
    }
}

#[test]
fn script_grouping_follows_the_content_of_an_unwritable_color() {
    for (source, canonical) in [
        (r"a^{\color{bad^name}ױ{}}+b", "a^ױ+ b"),
        (r"a_{\textcolor{bad_name}{\alpha}}b", r"a_\alpha b"),
        (r"a^{\textcolor{bad_name}{2}}", "a^{2}"),
        (r"a^{\textcolor{bad_name}{x+y}}", "a^{x+ y}"),
        (r"a^{\textcolor{teal}{ױ}}", r"a^{\textcolor{teal}{ױ}}"),
    ] {
        let convert =
            |tex| crate::convert(tex, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        let first = convert(source);
        assert_eq!(first, canonical, "{source}");
        assert_eq!(convert(&first), first);
    }
}

#[test]
fn nested_script_arguments_keep_only_their_visible_base_and_markers() {
    for (input, equivalent) in [
        (r"a^{^{\color{red}}}+b", "a+b"),
        (r"a_{_{\color{red}}}+b", "a+b"),
        (r"a^{x^{\color{red}}}+b", "a^x+b"),
        (r"a_{x_{\color{red}}}+b", "a_x+b"),
        (r"a^{x_{\color{red}}^2}+b", "a^{x^2}+b"),
        (r"a_{x_2^{\color{red}}}+b", "a_{x_2}+b"),
    ] {
        let write = |s| crate::convert(s, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        let first = write(input);
        assert_eq!(first, write(equivalent), "{input}");
        assert_eq!(first, write(&first), "{input}");
    }
}

#[test]
fn elided_prescripts_keep_operator_spacing_and_valid_markers() {
    let write = |input: &str| -> String {
        crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap()
    };
    for (input, expected, structure) in [
        (
            r"{}^{\textcolor{bad_name}{}}+b",
            "+ b",
            "<mo>+</mo><mi>b</mi>",
        ),
        (
            r"{}_{i}^{\textcolor{bad_name}{}}+b",
            r"{{}_{i}+} b",
            "<mmultiscripts><mo>+</mo><mprescripts/><mi>i</mi><none/></mmultiscripts><mi>b</mi>",
        ),
        (
            r"\alpha{}^{\textcolor{bad_name}{}}x",
            r"\alpha x",
            "<mi>α</mi><mi>x</mi>",
        ),
        (
            r"{}^{\textcolor{bad_name}{}}\left(x\right)y",
            r"\left( x \right)y",
            "<mrow><mo fence=\"true\">(</mo><mi>x</mi><mo fence=\"true\">)</mo></mrow><mi>y</mi>",
        ),
        (
            r"{}^{\textcolor{bad_name}{}}\frac{a}{b}x",
            r"\frac{a}{b}x",
            "<mfrac><mi>a</mi><mi>b</mi></mfrac><mi>x</mi>",
        ),
    ] {
        let first = write(input);
        assert_eq!(first, expected, "{input}");
        let second = write(&first);
        assert_eq!(first, second, "{input} changed on the second write");
        let mathml = crate::convert(&first, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert!(mathml.contains(structure), "{input} -> {first} -> {mathml}");
        assert!(!mathml.contains("mathcolor=\"bad_name\""), "{mathml}");
    }
}

#[test]
fn trailing_empty_implicit_scripts_do_not_regroup_the_preceding_base() {
    for input in [
        r"mathrm\mathrm\teal^\teal^\teal",
        r"a\mathrm{b}^{}^{}",
        r"\textcolor{blue}{x}^{}^{}",
        r"\textcolor{blue}{x}^{}^{}+b",
        r"\teal^\teal^d~",
        r"x^{}^d\quad y",
        r"\textcolor{blue}{x}^{}^2",
    ] {
        let write = |s| crate::convert(s, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        let first = write(input);
        assert_eq!(first, write(&first), "{input}");
    }
}

#[test]
fn empty_colored_prescripts_normalize_before_following_scripts() {
    let write = |input: &str| -> String {
        crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap()
    };
    for (input, expected, structure) in [
        (
            r"{}^{\color{red}{}}x{}^2",
            r"x^{2}",
            "<msup><mi>x</mi><mn>2</mn></msup>",
        ),
        (
            r"{}_{\color{red}{}}x{}_2",
            r"x_{2}",
            "<msub><mi>x</mi><mn>2</mn></msub>",
        ),
        (
            r"{}^{\color{red}{}}x{}^2+b",
            r"x {{}^{2}+} b",
            "<mmultiscripts><mo>+</mo><mprescripts/><none/><mn>2</mn></mmultiscripts>",
        ),
        (
            r"{}^{\color{red}{a}}x{}^2",
            r"{{}^{\textcolor{red}{a}}x}{}^{2}",
            "mathcolor=\"red\"><mi>a</mi>",
        ),
        (
            r"{}^{\color{red}{\ }}x{}^2",
            r"{{}^{\textcolor{red}{\ }}x}{}^{2}",
            "mathcolor=\"red\"><mspace width=\"0.25em\"/>",
        ),
    ] {
        let first = write(input);
        assert_eq!(first, expected, "{input}");
        let second = write(&first);
        assert_eq!(first, second, "{input} changed on the second write");
        let mathml = crate::convert(input, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        let canonical_mathml =
            crate::convert(&first, crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert_eq!(mathml, canonical_mathml, "{input} changed structure");
        assert!(
            canonical_mathml.contains(structure),
            "{input} -> {first} -> {canonical_mathml}"
        );
    }
}

#[test]
fn fence_boundaries_match_complete_control_words() {
    for (input, equiv) in [
        (r"\left(\rightarrow\right)", r"\left(\to\right )"),
        (
            r"\left(\color{red}a\rightarrow b\right)",
            r"\left(\textcolor{red}{a\to b}\right)",
        ),
        (
            r"\left\langle a\middle|\rightleftharpoons b\right \rangle",
            r"\left\langle a\middle\vert \rightleftharpoons b\right\rangle",
        ),
    ] {
        assert_eq!(
            crate::from_latex(input).unwrap().to_mathml().unwrap(),
            crate::from_latex(equiv).unwrap().to_mathml().unwrap(),
            "{input}"
        );
        let first = crate::convert(input, crate::Format::LaTeX, crate::Format::LaTeX).unwrap();
        assert_eq!(
            first,
            crate::convert(&first, crate::Format::LaTeX, crate::Format::LaTeX).unwrap(),
            "{input}"
        );
    }
}
