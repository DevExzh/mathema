//! Public resource-limit boundaries for parsed and caller-built expressions.
//!
//! These cases intentionally stay small.  They exercise the distinction between
//! a normal expression that fits a tiny budget and a construction/traversal
//! boundary that must report a typed limit error.

use mathema::ast::compact::CompactExpr;
use mathema::ast::{MathExpr, MathNode};
use mathema::error::{Error, LimitKind, Strictness};
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::mtef::MtefWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{Config, Format, convert_with, from_latex_with, from_mathml_with, from_omml_with};

const MATHML_SCALAR: &str = r#"<math><mi>x</mi></math>"#;
const OMML_SCALAR: &str = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r></m:oMath>"#;

fn assert_node_limit<T>(result: mathema::Result<T>, limit: usize) {
    let ok = matches!(
        &result,
        Err(Error::LimitExceeded {
            kind: LimitKind::Nodes,
            limit: actual,
        }) if *actual == limit
    );
    assert!(ok, "expected node limit {limit}, got {}", describe(&result));
}

fn assert_depth_limit<T>(result: mathema::Result<T>, limit: usize, label: &str) {
    let ok = is_depth_limit(&result, limit);
    assert!(
        ok,
        "{label}: expected depth limit {limit}, got {}",
        describe(&result)
    );
}

fn is_depth_limit<T>(result: &mathema::Result<T>, limit: usize) -> bool {
    matches!(
        result,
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            limit: actual,
        }) if *actual == limit
    )
}

fn describe<T>(result: &mathema::Result<T>) -> String {
    result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_else(|| "Ok".to_owned())
}

fn config(strictness: Strictness) -> Config {
    Config::default().with_strictness(strictness)
}

#[test]
fn xml_node_budget_has_the_same_boundary_in_both_strictness_modes() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let fits = config(strictness).with_max_nodes(Some(1));
        assert!(from_mathml_with(MATHML_SCALAR, &fits).is_ok());
        assert!(from_omml_with(OMML_SCALAR, &fits).is_ok());

        let rejects = config(strictness).with_max_nodes(Some(0));
        assert_node_limit(from_mathml_with(MATHML_SCALAR, &rejects), 0);
        assert_node_limit(from_omml_with(OMML_SCALAR, &rejects), 0);

        // The one-shot route must preserve the reader's typed resource error.
        assert_node_limit(
            convert_with(MATHML_SCALAR, Format::MathML, Format::LaTeX, &rejects),
            0,
        );
        assert_node_limit(
            convert_with(OMML_SCALAR, Format::Omml, Format::LaTeX, &rejects),
            0,
        );
    }
}

#[test]
fn xml_table_padding_is_part_of_the_node_budget_in_both_modes() {
    // The second row omits its final cell.  The reader must materialize one
    // empty cell to make the compact table rectangular, and that generated
    // node is still charged to max_nodes.
    let mathml = r#"
        <math><mtable>
          <mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr>
          <mtr><mtd><mi>c</mi></mtd></mtr>
        </mtable></math>
    "#;
    let omml = r#"
        <m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
          <m:m>
            <m:mr>
              <m:e><m:r><m:t>a</m:t></m:r></m:e>
              <m:e><m:r><m:t>b</m:t></m:r></m:e>
            </m:mr>
            <m:mr><m:e><m:r><m:t>c</m:t></m:r></m:e></m:mr>
          </m:m>
        </m:oMath>
    "#;

    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let unlimited = config(strictness).with_max_nodes(None);
        let mathml_nodes = from_mathml_with(mathml, &unlimited)
            .unwrap()
            .compact_expr()
            .arena
            .node_count();
        let omml_nodes = from_omml_with(omml, &unlimited)
            .unwrap()
            .compact_expr()
            .arena
            .node_count();
        assert!(mathml_nodes > 1);
        assert!(omml_nodes > 1);

        // One fewer than the completed arena is enough to distinguish a
        // bounded build from a parser that only checks the input bytes.
        let mathml_limit = config(strictness).with_max_nodes(Some(mathml_nodes - 1));
        let omml_limit = config(strictness).with_max_nodes(Some(omml_nodes - 1));
        assert_node_limit(from_mathml_with(mathml, &mathml_limit), mathml_nodes - 1);
        assert_node_limit(from_omml_with(omml, &omml_limit), omml_nodes - 1);
    }
}

#[test]
fn lenient_latex_does_not_recover_past_a_structural_limit() {
    // This is just beyond the parser's fixed structural recursion ceiling; it
    // is deliberately a small boundary case, not a large payload.
    let deep = format!("{}x{}", "{".repeat(129), "}".repeat(129));

    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let result = from_latex_with(&deep, &config(strictness));
        assert!(
            result.is_err(),
            "{strictness:?} accepted a depth-limited tree"
        );
    }
}

#[test]
fn latex_structural_limit_propagates_through_one_shot_conversion() {
    let input = r"\frac{\frac{x}{y}}{z}";
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let limited = config(strictness).with_max_depth(1);
        assert_depth_limit(
            convert_with(input, Format::LaTeX, Format::MathML, &limited),
            1,
            "one-shot LaTeX conversion",
        );
    }
}

fn nested_sqrts(levels: usize) -> MathExpr {
    let mut node = MathNode::Identifier("x".to_owned());
    for _ in 0..levels {
        node = MathNode::Sqrt(Box::new(node));
    }
    MathExpr::inline(vec![node])
}

#[test]
fn mathml_builder_honors_a_tighter_writer_depth_limit() {
    let parsed = mathema::from_latex(r"\frac{1}{\sqrt{x}}").unwrap();
    let fits = Config::default().with_max_depth(2);
    assert_eq!(
        parsed.to_mathml_with(&fits).unwrap(),
        parsed.to_mathml().unwrap()
    );
    assert_depth_limit(
        parsed.to_mathml_with(&Config::default().with_max_depth(1)),
        1,
        "MathML builder",
    );
}

#[test]
fn public_heap_and_compact_writers_validate_configured_depth() {
    let expr = nested_sqrts(3);
    let compact = CompactExpr::from_math_expr(&expr).unwrap();
    let limited = Config::default().with_max_depth(2);

    let heap_results = [
        ("LaTeX heap", LatexWriter::write(&expr, &limited)),
        ("MathML heap", MathmlWriter::write(&expr, &limited)),
        ("OMML heap", OmmlWriter::write(&expr, &limited)),
        ("MTEF heap", MtefWriter::write(&expr, &limited)),
        (
            "UnicodeMath heap",
            UnicodeMathWriter::write(&expr, &limited),
        ),
    ];
    let compact_results = [
        (
            "LaTeX compact",
            LatexWriter::write_compact(&compact, &limited),
        ),
        (
            "MathML compact",
            MathmlWriter::write_compact(&compact, &limited),
        ),
        (
            "OMML compact",
            OmmlWriter::write_compact(&compact, &limited),
        ),
        (
            "MTEF compact",
            MtefWriter::write_compact(&compact, &limited),
        ),
        (
            "UnicodeMath compact",
            UnicodeMathWriter::write_compact(&compact, &limited),
        ),
    ];
    let mut failures = Vec::new();
    for (writer, result) in heap_results.into_iter().chain(compact_results) {
        if !is_depth_limit(&result, 2) {
            failures.push(format!("{writer}: {}", describe(&result)));
        }
    }
    assert!(failures.is_empty(), "depth-limit failures: {failures:?}");
}
