use std::sync::{Arc, atomic::AtomicBool};

use mathema::ast::DisplayStyle;
use mathema::ast::arena::AstArena;
use mathema::writer::{
    Writer, latex::LatexWriter, mathml::MathmlWriter, omml::OmmlWriter,
    unicode_math::UnicodeMathWriter,
};
use mathema::{CompactExpr, CompactNode, NodeId, StrId};
use mathema::{Config, OutputLimits, RequestControl, from_latex};

#[test]
fn bounded_writer_output_matches_unlimited_for_each_text_target() {
    let parsed = from_latex(r"\frac{a_{i+1}}{b^{2n+1}}").unwrap();
    let config = Config::default();
    let limits_for = |output: &str| OutputLimits::with_max_bytes(output.len());

    let latex = LatexWriter::write_compact(parsed.compact_expr(), &config).unwrap();
    assert_eq!(
        LatexWriter::write_compact_with_limits(parsed.compact_expr(), &config, &limits_for(&latex))
            .unwrap(),
        latex
    );
    let mathml = MathmlWriter::write_compact(parsed.compact_expr(), &config).unwrap();
    assert_eq!(
        MathmlWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &limits_for(&mathml)
        )
        .unwrap(),
        mathml
    );
    let omml = OmmlWriter::write_compact(parsed.compact_expr(), &config).unwrap();
    assert_eq!(
        OmmlWriter::write_compact_with_limits(parsed.compact_expr(), &config, &limits_for(&omml))
            .unwrap(),
        omml
    );
    let unicode = UnicodeMathWriter::write_compact(parsed.compact_expr(), &config).unwrap();
    assert_eq!(
        UnicodeMathWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &limits_for(&unicode)
        )
        .unwrap(),
        unicode
    );
}

#[test]
fn bounded_writer_stops_at_a_small_limit_and_cancellation() {
    let parsed = from_latex(&"x+".repeat(1000)).unwrap();
    let config = Config::default();
    for result in [
        LatexWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &OutputLimits::with_max_bytes(64),
        ),
        MathmlWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &OutputLimits::with_max_bytes(64),
        ),
        OmmlWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &OutputLimits::with_max_bytes(64),
        ),
        UnicodeMathWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &OutputLimits::with_max_bytes(64),
        ),
    ] {
        assert!(
            result.is_err(),
            "small output budget unexpectedly succeeded"
        );
    }

    let control = RequestControl::new();
    control.cancel();
    let limits = OutputLimits::unlimited().with_request_control(&control);
    assert!(
        MathmlWriter::write_compact_with_limits(parsed.compact_expr(), &config, &limits).is_err()
    );
    let external = Arc::new(AtomicBool::new(true));
    assert!(
        UnicodeMathWriter::write_compact_with_limits(
            parsed.compact_expr(),
            &config,
            &OutputLimits::unlimited().with_cancellation(external),
        )
        .is_err()
    );
}

#[test]
fn output_limits_retain_all_composed_request_controls() {
    let parsed = from_latex("x+y").unwrap();
    let first = RequestControl::new();
    let second = RequestControl::new();
    let limits = OutputLimits::unlimited()
        .with_request_control(&first)
        .with_request_control(&second);

    first.cancel();
    assert!(matches!(
        MathmlWriter::write_compact_with_limits(parsed.compact_expr(), &Config::default(), &limits),
        Err(mathema::Error::RequestCancelled)
    ));

    let first = RequestControl::new();
    let second = RequestControl::new();
    let limits = OutputLimits::unlimited()
        .with_request_control(&first)
        .with_request_control(&second);
    second.cancel();
    assert!(matches!(
        MathmlWriter::write_compact_with_limits(parsed.compact_expr(), &Config::default(), &limits),
        Err(mathema::Error::RequestCancelled)
    ));
}

#[test]
fn public_bounded_writers_validate_caller_compact_arenas() {
    let mut arena = AstArena::new();
    let root = arena.push_node(CompactNode::Identifier(StrId::EMPTY));
    let roots = arena.push_children(&[root]).unwrap();
    let _invalid = arena.push_node(CompactNode::Superscript {
        base: NodeId::NONE,
        sup: root,
    });
    let expr = CompactExpr::new(arena, roots, DisplayStyle::Inline);
    let config = Config::default();
    let limits = OutputLimits::with_max_bytes(10_000);
    assert!(MathmlWriter::write_compact_with_limits(&expr, &config, &limits).is_err());
    assert!(OmmlWriter::write_compact_with_limits(&expr, &config, &limits).is_err());
    assert!(UnicodeMathWriter::write_compact_with_limits(&expr, &config, &limits).is_err());
    assert!(LatexWriter::write_compact_with_limits(&expr, &config, &limits).is_err());
    assert!(
        MathmlWriter::write_compact_with_diagnostics_and_limits(&expr, &config, &limits).is_err()
    );
    assert!(
        OmmlWriter::write_compact_with_diagnostics_and_limits(&expr, &config, &limits).is_err()
    );
    assert!(
        UnicodeMathWriter::write_compact_with_diagnostics_and_limits(&expr, &config, &limits)
            .is_err()
    );
    assert!(
        LatexWriter::write_compact_with_diagnostics_and_limits(&expr, &config, &limits).is_err()
    );
}
