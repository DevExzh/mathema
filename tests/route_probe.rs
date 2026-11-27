use mathema::ast::visit::{Visitor, walk_node};
use mathema::ast::{MathExpr, MathNode};
use mathema::{
    Format, convert, convert_with_diagnostics, from_latex, from_mathml, from_omml,
    from_unicode_math,
};

fn has_prescript_base(expr: &MathExpr, expected: &str) -> bool {
    struct Search<'a> {
        expected: &'a str,
        found: bool,
    }

    impl Visitor for Search<'_> {
        fn visit_node(&mut self, node: &MathNode) {
            if let MathNode::PreScripts { base, .. } = node
                && matches!(base.as_ref(), MathNode::Identifier(name) if name == self.expected)
            {
                self.found = true;
            }
            walk_node(self, node);
        }
    }

    let mut search = Search {
        expected,
        found: false,
    };
    search.visit_expr(expr);
    search.found
}

fn has_box_colors(expr: &MathExpr, background: &str, border: Option<&str>) -> bool {
    struct Search<'a> {
        background: &'a str,
        border: Option<&'a str>,
        found: bool,
    }

    impl Visitor for Search<'_> {
        fn visit_node(&mut self, node: &MathNode) {
            if let MathNode::Boxed {
                bg_color,
                border_color,
                ..
            } = node
                && bg_color.as_deref() == Some(self.background)
                && border_color.as_deref() == self.border
            {
                self.found = true;
            }
            walk_node(self, node);
        }
    }

    let mut search = Search {
        background,
        border,
        found: false,
    };
    search.visit_expr(expr);
    search.found
}

fn space_width_em(expr: &MathExpr) -> f64 {
    struct Sum(f64);

    impl Visitor for Sum {
        fn visit_node(&mut self, node: &MathNode) {
            match node {
                MathNode::Space(width) | MathNode::ExplicitSpace { width, .. } => {
                    self.0 += f64::from(width.num) / f64::from(width.den);
                },
                _ => {},
            }
            walk_node(self, node);
        }
    }

    let mut sum = Sum(0.0);
    sum.visit_expr(expr);
    sum.0
}

/// The one-shot conversion and parse-once builder must apply the same target
/// normalization.  In particular, a script on an empty base is a leading
/// script of the following atom.  Comparing the serialized target catches the
/// route drift that a successful parse or a writer fixed-point check misses.
#[test]
fn leading_scripts_have_one_serialization_across_public_routes() {
    for (input, expected_base) in [
        (r"{}_{a}^{b} X", "X"),
        (r"x={}^{14}_{6}C", "C"),
        (r"t_\oiint_a^b x", "x"),
    ] {
        let builder = from_latex(input).unwrap();
        for target in [Format::MathML, Format::Omml, Format::UnicodeMath] {
            let direct = convert(input, Format::LaTeX, target).unwrap();
            let built = match target {
                Format::MathML => builder.to_mathml().unwrap(),
                Format::Omml => builder.to_omml().unwrap(),
                Format::UnicodeMath => builder.to_unicode_math().unwrap(),
                _ => unreachable!(),
            };
            assert_eq!(direct, built, "{input:?} -> {target:?}");

            let direct_expr = match target {
                Format::MathML => from_mathml(&direct).unwrap().expr(),
                Format::Omml => from_omml(&direct).unwrap().expr(),
                Format::UnicodeMath => from_unicode_math(&direct).unwrap().expr(),
                _ => unreachable!(),
            };
            let built_expr = match target {
                Format::MathML => from_mathml(&built).unwrap().expr(),
                Format::Omml => from_omml(&built).unwrap().expr(),
                Format::UnicodeMath => from_unicode_math(&built).unwrap().expr(),
                _ => unreachable!(),
            };
            assert!(
                has_prescript_base(&direct_expr, expected_base),
                "one-shot route detached leading script from {expected_base}: {direct}"
            );
            assert!(
                has_prescript_base(&built_expr, expected_base),
                "builder route detached leading script from {expected_base}: {built}"
            );
        }
    }
}

#[test]
fn builder_and_one_shot_preserve_box_colors_through_mathml() {
    for (source, background, border) in [
        (r"\colorbox{red}{x}", "red", None),
        (r"\fcolorbox{blue}{red}{x}", "red", Some("blue")),
    ] {
        let builder = from_latex(source).unwrap();
        let built_mathml = builder.to_mathml().unwrap();
        let direct_mathml = convert(source, Format::LaTeX, Format::MathML).unwrap();
        assert_eq!(built_mathml, direct_mathml, "builder route: {source}");

        let built_result = from_mathml(&built_mathml).unwrap();
        let built_latex = built_result.to_latex().unwrap();
        let direct_latex = convert(&direct_mathml, Format::MathML, Format::LaTeX).unwrap();
        assert_eq!(built_latex, direct_latex, "readback route: {source}");
        assert!(
            has_box_colors(&built_result.expr(), background, border),
            "MathML readback dropped box color: {source} -> {built_latex}"
        );
    }
}

#[test]
fn unicode_math_spacing_loss_is_visible_on_both_public_routes() {
    let source = r"x\qquad y";
    let builder = from_latex(source).unwrap();
    let (built, builder_diagnostics) = builder.to_unicode_math_with_diagnostics().unwrap();
    let (direct, direct_diagnostics) =
        convert_with_diagnostics(source, Format::LaTeX, Format::UnicodeMath).unwrap();
    assert_eq!(built, direct);
    assert_eq!(builder_diagnostics.len(), direct_diagnostics.len());
    let output_expr = from_unicode_math(&built).unwrap().expr();
    let output_width = space_width_em(&output_expr);
    if (output_width - 2.0).abs() > f64::EPSILON {
        assert!(
            builder_diagnostics.iter().any(|diagnostic| {
                let message = diagnostic.message.to_ascii_lowercase();
                message.contains("2em")
                    || message.contains("two-em")
                    || message.contains("two em")
                    || message.contains("qquad")
            }),
            "approximated two-em spacing needs a precise diagnostic: {builder_diagnostics:?}"
        );
    }
}
