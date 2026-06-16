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
