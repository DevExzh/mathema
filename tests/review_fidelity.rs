//! End-to-end semantic checks for the first-priority library-review findings.
//!
//! These tests deliberately inspect the shared AST after a conversion instead
//! of pinning a writer's whitespace or command spelling.  A successful string
//! conversion is not enough here: the result must still describe the same
//! mathematical object, and recovery or target-format loss must be visible to
//! callers.

use mathema::ast::visit::{Visitor, walk_node};
use mathema::ast::{FencedItem, FractionKind, MathExpr, MathNode, SymbolKind};
use mathema::error::{Config, Diagnostic, DiagnosticKind, Strictness};
use mathema::{
    Format, convert_with_config_and_diagnostics, convert_with_diagnostics, from_latex,
    from_latex_with, from_mathml, from_omml_with, from_unicode_math, from_unicode_math_with,
};

#[cfg(feature = "cli")]
use assert_cmd::Command;
#[cfg(feature = "cli")]
use predicates::prelude::*;

fn lenient() -> Config {
    Config::default().with_strictness(Strictness::Lenient)
}

fn has_warning(diagnostics: &[Diagnostic], needles: &[&str]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::Warning
            && needles.iter().all(|needle| {
                diagnostic
                    .message
                    .to_ascii_lowercase()
                    .contains(&needle.to_ascii_lowercase())
            })
    })
}

fn has_any_message(diagnostics: &[Diagnostic], needles: &[&str]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        needles.iter().any(|needle| {
            diagnostic
                .message
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
    })
}

/// Visit every heap node, including nodes nested in table cells and fenced
/// separators.  Reuse the library's visitor so new composite nodes are not
/// accidentally omitted from these semantic checks.
fn visit_expr(expr: &MathExpr, mut visit: impl FnMut(&MathNode)) {
    struct Walker<'a, F>(&'a mut F);

    impl<F: FnMut(&MathNode)> Visitor for Walker<'_, F> {
        fn visit_node(&mut self, node: &MathNode) {
            self.0(node);
            walk_node(self, node);
        }
    }

    Walker(&mut visit).visit_expr(expr);
}

fn has_identifier(expr: &MathExpr, value: &str) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        found |= matches!(node, MathNode::Identifier(name) if name == value);
    });
    found
}

fn has_identifier_or_symbol(expr: &MathExpr, value: &str) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        found |= matches!(
            node,
            MathNode::Identifier(name) | MathNode::Symbol(_, name) if name == value
        );
    });
    found
}

fn has_number(expr: &MathExpr, value: &str) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        found |= matches!(node, MathNode::Number(number) if number == value);
    });
    found
}

fn scalar(node: &MathNode) -> Option<&str> {
    match node {
        MathNode::Identifier(value) | MathNode::Number(value) => Some(value),
        _ => None,
    }
}

fn has_power(node: &MathNode, base: &str, exponent: &str) -> bool {
    let mut found = false;
    visit_expr(&MathExpr::inline(vec![node.clone()]), |child| {
        if let MathNode::Superscript { base: b, sup } = child {
            found |= scalar(b) == Some(base) && scalar(sup) == Some(exponent);
        }
    });
    found
}

fn has_parenthesized_operator(expr: &MathExpr, operator: &str) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        if let MathNode::Fenced {
            open,
            close,
            content,
        } = node
            && open == "("
            && close == ")"
        {
            found |= content.iter().any(|item| {
                let FencedItem::Element(element) = item else {
                    return false;
                };
                let mut has_operator = false;
                visit_expr(&MathExpr::inline(vec![element.clone()]), |inner| {
                    has_operator |= matches!(
                        inner,
                        MathNode::Symbol(SymbolKind::BinaryOp, value) if value == operator
                    );
                });
                has_operator
            });
        }
    });
    found
}

fn has_integral_with_qualifier(expr: &MathExpr) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        if let MathNode::Nary {
            op: '\u{222B}',
            sub,
            sup,
            body,
            ..
        } = node
        {
            let has_zero = sub
                .as_deref()
                .is_some_and(|node| has_number(&MathExpr::inline(vec![node.clone()]), "0"))
                || sup
                    .as_deref()
                    .is_some_and(|node| has_number(&MathExpr::inline(vec![node.clone()]), "0"))
                || has_number(&MathExpr::inline(vec![body.as_ref().clone()]), "0");
            let has_one = sub
                .as_deref()
                .is_some_and(|node| has_number(&MathExpr::inline(vec![node.clone()]), "1"))
                || sup
                    .as_deref()
                    .is_some_and(|node| has_number(&MathExpr::inline(vec![node.clone()]), "1"))
                || has_number(&MathExpr::inline(vec![body.as_ref().clone()]), "1");
            found |= has_zero && has_one;
        }
    });
    found
}

fn has_table(expr: &MathExpr, min_rows: usize, min_columns: usize) -> bool {
    let mut found = false;
    visit_expr(expr, |node| {
        let rows = match node {
            MathNode::Array { rows, .. }
            | MathNode::Matrix { rows, .. }
            | MathNode::NativeArray { rows, .. } => rows,
            _ => return,
        };
        found |= rows.len() >= min_rows && rows.iter().any(|row| row.len() >= min_columns);
    });
    found
}

#[test]
fn content_mathml_keeps_precedence_grouping() {
    let subtraction = from_mathml(
        "<math><apply><minus/><ci>a</ci><apply><minus/><ci>b</ci><ci>c</ci></apply></apply></math>",
    )
    .unwrap();
    assert!(
        has_parenthesized_operator(&subtraction.expr(), "−"),
        "nested subtraction must remain grouped: {:?}",
        subtraction.expr()
    );

    let product = from_mathml(
        "<math><apply><times/><ci>a</ci><apply><plus/><ci>b</ci><ci>c</ci></apply></apply></math>",
    )
    .unwrap();
    assert!(
        has_parenthesized_operator(&product.expr(), "+"),
        "a product factor containing a sum must remain grouped: {:?}",
        product.expr()
    );
}

#[test]
fn content_mathml_keeps_second_derivative_order() {
    let result = from_mathml(
        "<math><apply><diff/><bvar><ci>x</ci><degree><cn>2</cn></degree></bvar><ci>f</ci></apply></math>",
    )
    .unwrap();
    let expr = result.expr();
    let has_ordered_fraction = {
        let mut found = false;
        visit_expr(&expr, |node| {
            if let MathNode::Fraction { num, den, .. } = node {
                found |= has_power(num, "d", "2")
                    && has_power(den, "x", "2")
                    && has_identifier_or_symbol(&MathExpr::inline(vec![den.as_ref().clone()]), "d");
            }
        });
        found
    };
    assert!(
        has_ordered_fraction,
        "second derivative order must survive in d²/(dx²): {:?}",
        expr
    );
}

#[test]
fn content_mathml_interval_is_preserved_or_reported() {
    let result = from_mathml(
        "<math><apply><int/><bvar><ci>x</ci></bvar><interval><cn>0</cn><cn>1</cn></interval><ci>f</ci></apply></math>",
    )
    .unwrap();
    let expr = result.expr();
    assert!(
        has_integral_with_qualifier(&expr)
            || has_any_message(result.diagnostics(), &["interval", "domain"]),
        "integral interval must survive or have a specific loss diagnostic: AST={expr:?}, diagnostics={:?}",
        result.diagnostics()
    );
}

#[test]
fn word_content_controls_preserve_descendant_math_in_both_modes() {
    let omml = r#"<w:sdt xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:sdtContent><m:oMath><m:r><m:t>x</m:t></m:r><m:r><m:t>+</m:t></m:r><m:r><m:t>1</m:t></m:r></m:oMath></w:sdtContent></w:sdt>"#;

    for config in [Config::default(), lenient()] {
        let result = from_omml_with(omml, &config).unwrap_or_else(|error| {
            panic!("content-control wrapper should be transparent ({config:?}): {error:?}")
        });
        let expr = result.expr();
        assert!(
            has_identifier(&expr, "x"),
            "math inside w:sdt was lost: {expr:?}"
        );
        assert!(
            has_number(&expr, "1"),
            "math inside w:sdt was lost: {expr:?}"
        );
    }
}

#[test]
fn unicode_math_cases_round_trip_as_a_table_not_literal_text() {
    let source = r#"\begin{cases}x&x\ge 0\\-x&x<0\end{cases}"#;
    let builder = from_latex(source).unwrap();
    let (written, writer_diagnostics) = builder.to_unicode_math_with_diagnostics().unwrap();
    assert!(
        !has_warning(&writer_diagnostics, &["text", "fallback"]),
        "cases writer must not report text fallback: {writer_diagnostics:?}"
    );
    let rebuilt = from_unicode_math(&written).unwrap();
    assert!(
        has_table(&rebuilt.expr(), 2, 2),
        "UnicodeMath cases must rebuild as a two-row table, not {:?}; output={written:?}",
        rebuilt.expr()
    );
    assert!(
        !matches!(rebuilt.expr().nodes.as_slice(), [MathNode::Text(_, _)]),
        "UnicodeMath cases fell back to literal text: {written:?}"
    );
}

#[test]
fn successful_recovery_keeps_math_and_reports_reader_diagnostics() {
    let config = lenient();
    let source = r#"{a \above b}"#;
    let builder = from_latex_with(source, &config).unwrap();
    let builder_expr = builder.expr();
    assert!(
        builder_expr.nodes.iter().any(|node| matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        )),
        "lenient recovery must keep the recoverable fraction: {builder_expr:?}"
    );
    assert!(
        has_warning(builder.diagnostics(), &["dimension"]),
        "successful recovery must be visible to builder callers: {:?}",
        builder.diagnostics()
    );

    let (one_shot, diagnostics) =
        convert_with_config_and_diagnostics(source, Format::LaTeX, Format::MathML, &config)
            .unwrap();
    let one_shot_expr = from_mathml(&one_shot).unwrap().expr();
    assert!(
        one_shot_expr.nodes.iter().any(|node| matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        )),
        "one-shot conversion must retain recovered math: {one_shot_expr:?}"
    );
    assert!(
        has_warning(&diagnostics, &["dimension"]),
        "one-shot conversion must include reader diagnostics: {diagnostics:?}"
    );
}

#[test]
fn one_shot_diagnostics_include_reader_and_writer_phases() {
    let (mathml, reader_diagnostics) =
        convert_with_diagnostics(r#"\unknown{x}"#, Format::LaTeX, Format::MathML).unwrap();
    assert!(
        mathml.contains("merror"),
        "unknown command content was lost: {mathml}"
    );
    assert!(
        has_warning(&reader_diagnostics, &["undefined", "control"]),
        "reader warning must be included by one-shot API: {reader_diagnostics:?}"
    );

    let (unicode_math, writer_diagnostics) =
        convert_with_diagnostics(r#"\color{red}{x}"#, Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(unicode_math.contains('x'));
    assert!(
        has_warning(&writer_diagnostics, &["color"]),
        "writer loss must be included by one-shot API: {writer_diagnostics:?}"
    );
}

#[test]
fn one_shot_and_builder_diagnostics_agree_on_recovery_and_loss() {
    let config = lenient();
    let source = r#"{a \above b}"#;
    let builder = from_latex_with(source, &config).unwrap();
    let (_, builder_writer_diagnostics) = builder.to_mathml_with_diagnostics().unwrap();
    let (_, one_shot_diagnostics) =
        convert_with_config_and_diagnostics(source, Format::LaTeX, Format::MathML, &config)
            .unwrap();
    assert!(has_warning(builder.diagnostics(), &["dimension"]));
    assert!(has_warning(&one_shot_diagnostics, &["dimension"]));
    assert!(
        builder_writer_diagnostics.is_empty(),
        "MathML should not add incidental writer loss: {builder_writer_diagnostics:?}"
    );

    let colored = from_latex(r#"\color{red}{x}"#).unwrap();
    let (_, builder_writer_diagnostics) = colored.to_unicode_math_with_diagnostics().unwrap();
    assert!(has_warning(&builder_writer_diagnostics, &["color"]));
}

#[test]
fn lenient_unicode_math_matrix_recovery_keeps_table_and_reports_skips() {
    let result = from_unicode_math_with("x ■(a##b&c#)", &lenient()).unwrap();
    assert!(
        has_table(&result.expr(), 1, 2),
        "recovered matrix was lost: {:?}",
        result.expr()
    );
    assert!(
        has_warning(result.diagnostics(), &["skipped", "matrix"])
            || has_any_message(result.diagnostics(), &["skipped"]),
        "matrix recovery must be diagnosed: {:?}",
        result.diagnostics()
    );
}

#[test]
fn successful_unicode_math_literal_fallback_is_diagnosed() {
    // An unmatched group can still recover as literal text. Legacy one-sided
    // matrix cases have their own structural regression in unicode_math_cases.
    let result = from_unicode_math_with("〖unclosed", &lenient()).unwrap();
    assert!(
        matches!(result.expr().nodes.as_slice(), [MathNode::Text(_, _)]),
        "fixture should exercise literal fallback: {:?}",
        result.expr()
    );
    assert!(
        has_warning(result.diagnostics(), &["fallback", "text"]),
        "literal fallback must be diagnosed: {:?}",
        result.diagnostics()
    );
}

#[cfg(feature = "cli")]
#[test]
fn cli_reports_successful_lenient_recovery_without_hiding_math() {
    let output = Command::cargo_bin("mathema")
        .unwrap()
        .args([
            "convert",
            "--from",
            "latex",
            "--to",
            "mathml",
            "--lenient",
            r#"{a \above b}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("<mfrac>"));
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).to_ascii_lowercase();
    assert!(
        stderr.contains("warning") || stderr.contains("dimension"),
        "CLI should expose lenient recovery diagnostics; stderr={stderr:?}"
    );
}
