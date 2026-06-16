use crate::ast::normalize::normalize_compact_expr;
use crate::error::Config;
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
        normalize_compact_expr(&mut r1.expr);
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr);
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(out1, out2, "round-trip failed for input: {input}");
    }
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
        normalize_compact_expr(&mut r1.expr);
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr);
        let out2 = LatexWriter::write_compact(&r2.expr, &config).unwrap();

        assert_eq!(out1, out2, "spacing round-trip failed for input: {input}");
    }
}

#[test]
fn round_trip_scaled_delimiters() {
    let config = Config::default();
    let cases = ["\\big(", "\\Big[", "\\bigg|", "\\Bigg)"];
    for input in &cases {
        let mut r1 = LatexReader::parse_compact(input, &config).unwrap();
        normalize_compact_expr(&mut r1.expr);
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr);
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
    normalize_compact_expr(&mut r1.expr);
    let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

    let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
    normalize_compact_expr(&mut r2.expr);
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
    normalize_compact_expr(&mut r1.expr);
    let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

    let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
    normalize_compact_expr(&mut r2.expr);
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
        normalize_compact_expr(&mut r1.expr);
        let out1 = LatexWriter::write_compact(&r1.expr, &config).unwrap();

        let mut r2 = LatexReader::parse_compact(&out1, &config).unwrap();
        normalize_compact_expr(&mut r2.expr);
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

    // Overbrace/underbrace/bracket commands should parse into Overscript/Underscript nodes
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
        if *is_over {
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
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
    // Object letters present.
    for obj in ["A", "B", "C", "D"] {
        assert!(mathml.contains(obj), "missing object {obj} in:\n{mathml}");
    }
    // Horizontal right arrow and vertical down arrow glyphs.
    assert!(mathml.contains('\u{2192}'), "missing → arrow in:\n{mathml}");
    assert!(mathml.contains('\u{2193}'), "missing ↓ arrow in:\n{mathml}");
    // Labels appear above/below the horizontal arrows.
    assert!(mathml.contains("<mover"), "expected over-label for f in:\n{mathml}");
    assert!(mathml.contains("<munder"), "expected under-label for g in:\n{mathml}");
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
    assert_eq!(mathml.matches('\u{2192}').count(), 2, "expected two → in:\n{mathml}");
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
    assert!(mathml.contains('\u{2016}'), "missing ‖ for @| in:\n{mathml}");
    // Down arrow and right arrow.
    assert!(mathml.contains('\u{2193}'));
    assert!(mathml.contains('\u{2192}'));
}

#[test]
fn cd_vertical_labels_and_left_arrow() {
    use crate::convert::from_latex;
    let src = r"\begin{CD} A @V{g}V{h}V B @<<< C \end{CD}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(mathml.contains('\u{2193}'), "missing ↓ in:\n{mathml}");
    assert!(mathml.contains('\u{2190}'), "missing ← in:\n{mathml}");
    assert!(mathml.contains(">g<") && mathml.contains(">h<"), "missing v-labels in:\n{mathml}");
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
    assert!(svg.contains("<path") || svg.contains("<text"), "no rendered elements:\n{svg}");
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
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
    assert!(mathml.contains('{'), "expected big left brace, got:\n{mathml}");
    // Shared left expression and the `=`.
    assert!(mathml.contains(">x<"), "missing x in:\n{mathml}");
    assert!(mathml.contains(">=<"), "missing = in:\n{mathml}");
    // Per-row upright equation numbers (1a), (1b).
    assert!(mathml.contains("(1a)"), "missing (1a) label in:\n{mathml}");
    assert!(mathml.contains("(1b)"), "missing (1b) label in:\n{mathml}");
    // Conditions present.
    assert!(mathml.contains('\u{2265}'), "missing ≥ in:\n{mathml}");
}

#[test]
fn subnumcases_uses_sub_letter_labels() {
    use crate::convert::from_latex;
    let src = r"\begin{subnumcases}{f} a & p\\ b & q \end{subnumcases}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
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
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
    assert!(mathml.contains(">a<") && mathml.contains(">d<"), "missing cells in:\n{mathml}");
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
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
    assert!(mathml.contains(">x<"), "missing x in:\n{mathml}");
}

#[test]
fn empheq_left_delimiter_wraps_in_fence() {
    use crate::convert::from_latex;
    let src = r"\begin{empheq}[left=\Rightarrow]{align*} a &= b \end{empheq}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Left delimiter ⇒ should appear as a fence operator.
    assert!(mathml.contains('\u{21D2}'), "missing ⇒ delimiter in:\n{mathml}");
    assert!(mathml.contains("<mtable>"), "expected mtable, got:\n{mathml}");
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
    assert!(mathml.contains(">a<") && mathml.contains(">d<"), "missing cells in:\n{mathml}");
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
fn colortbl_arrayrulecolor_is_accepted_but_ignored() {
    use crate::convert::from_latex;
    let src = r"\begin{array}{cc} \arrayrulecolor{green} a & b \end{array}";
    let mathml = from_latex(src).unwrap().to_mathml().unwrap();
    // Deferred: no background is produced for rule colors.
    assert!(
        !mathml.contains("background-color"),
        "arrayrulecolor should not produce a cell background, got:\n{mathml}"
    );
    assert!(mathml.contains(">a<") && mathml.contains(">b<"), "missing cells in:\n{mathml}");
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
    let r = from_latex(r"\begin{prooftree}\AxiomC{A}\AxiomC{B}\BinaryInfC{C}\end{prooftree}")
        .unwrap();
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
    assert!(ml.contains("<mi>A</mi>") && ml.contains("<mi>B</mi>"), "{ml}");
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
        assert!(ml.contains(&format!("<mi>{obj}</mi>")), "missing {obj}:\n{ml}");
    }
    // Arrow glyphs for each direction.
    assert!(ml.contains('\u{2192}'), "missing → :\n{ml}"); // r
    assert!(ml.contains('\u{2193}'), "missing ↓ :\n{ml}"); // d
    assert!(ml.contains('\u{2190}'), "missing ← :\n{ml}"); // l
    // Arrow label.
    assert!(ml.contains("<mi>f</mi>"), "missing label f:\n{ml}");
    assert!(r.to_svg().is_ok());
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
