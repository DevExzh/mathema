//! The one-shot and parse-once APIs share target normalization.  In
//! particular, scripts on an empty group lead the following nucleus instead
//! of being emitted as a script on an empty base.

use mathema::NodeId;
use mathema::ast::DisplayStyle;
use mathema::ast::arena::AstArena;
use mathema::ast::compact::{CompactExpr, CompactNode};
use mathema::writer::Writer;
use mathema::{Format, convert, from_latex};

#[test]
fn builder_and_one_shot_use_the_same_target_normalization() {
    let source = r"{}_{a}^{b} X";
    let parsed = from_latex(source).unwrap();

    assert_eq!(
        parsed.to_latex().unwrap(),
        convert(source, Format::LaTeX, Format::LaTeX).unwrap()
    );
    assert_eq!(
        parsed.to_mathml().unwrap(),
        convert(source, Format::LaTeX, Format::MathML).unwrap()
    );
    assert_eq!(
        parsed.to_omml().unwrap(),
        convert(source, Format::LaTeX, Format::Omml).unwrap()
    );
    assert_eq!(
        parsed.to_unicode_math().unwrap(),
        convert(source, Format::LaTeX, Format::UnicodeMath).unwrap()
    );
    assert_eq!(
        parsed.to_mtef().unwrap(),
        convert(source, Format::LaTeX, Format::Mtef).unwrap()
    );
}

#[test]
fn diagnostic_builder_outputs_match_one_shot_outputs() {
    let source = r"{}_{a}^{b} X";
    let parsed = from_latex(source).unwrap();
    let config = mathema::Config::default();

    let (one_latex, _) =
        mathema::convert_with_config_and_diagnostics(source, Format::LaTeX, Format::LaTeX, &config)
            .unwrap();
    assert_eq!(parsed.to_latex_with_diagnostics().unwrap().0, one_latex);

    let (one_mathml, _) = mathema::convert_with_config_and_diagnostics(
        source,
        Format::LaTeX,
        Format::MathML,
        &config,
    )
    .unwrap();
    assert_eq!(parsed.to_mathml_with_diagnostics().unwrap().0, one_mathml);

    let (one_omml, _) =
        mathema::convert_with_config_and_diagnostics(source, Format::LaTeX, Format::Omml, &config)
            .unwrap();
    assert_eq!(parsed.to_omml_with_diagnostics().unwrap().0, one_omml);

    let (one_unicode, _) = mathema::convert_with_config_and_diagnostics(
        source,
        Format::LaTeX,
        Format::UnicodeMath,
        &config,
    )
    .unwrap();
    assert_eq!(
        parsed.to_unicode_math_with_diagnostics().unwrap().0,
        one_unicode
    );
}

#[test]
fn heap_and_compact_writers_attach_leading_scripts_alike() {
    let source = r"{}_{a}^{b} X";
    let parsed = from_latex(source).unwrap();
    let heap = parsed.expr();
    let config = mathema::Config::default();
    assert_eq!(
        mathema::writer::mathml::MathmlWriter::write(&heap, &config).unwrap(),
        parsed.to_mathml().unwrap()
    );
    assert_eq!(
        mathema::writer::omml::OmmlWriter::write(&heap, &config).unwrap(),
        parsed.to_omml().unwrap()
    );
    assert_eq!(
        mathema::writer::unicode_math::UnicodeMathWriter::write(&heap, &config).unwrap(),
        parsed.to_unicode_math().unwrap()
    );
    let compact = parsed.compact_expr();
    assert_eq!(
        mathema::writer::mathml::MathmlWriter::write_compact(compact, &config).unwrap(),
        parsed.to_mathml().unwrap()
    );
    assert_eq!(
        mathema::writer::omml::OmmlWriter::write_compact(compact, &config).unwrap(),
        parsed.to_omml().unwrap()
    );
    assert_eq!(
        mathema::writer::unicode_math::UnicodeMathWriter::write_compact(compact, &config).unwrap(),
        parsed.to_unicode_math().unwrap()
    );
    assert_eq!(
        mathema::writer::mathml::MathmlWriter::write_compact_with_diagnostics(compact, &config)
            .unwrap()
            .0,
        parsed.to_mathml_with_diagnostics().unwrap().0
    );
    let options = mathema::writer::mathml::MathmlOptions::standard();
    assert_eq!(
        mathema::writer::mathml::MathmlWriter::write_with_options(compact, &config, &options)
            .unwrap()
            .0,
        parsed.to_mathml_with_options(&options).unwrap()
    );
}

#[test]
fn unreachable_invalid_script_records_do_not_break_public_compact_writes() {
    let mut arena = AstArena::new();
    let x_text = arena.push_str("x");
    let x = arena.push_node(CompactNode::Identifier(x_text));
    let roots = arena.push_children(&[x]).unwrap();
    let _garbage = arena.push_node(CompactNode::Superscript {
        base: NodeId::NONE,
        sup: x,
    });
    let expr = CompactExpr::new(arena, roots, DisplayStyle::Block);
    for write in [
        mathema::writer::mathml::MathmlWriter::write_compact,
        mathema::writer::omml::OmmlWriter::write_compact,
        mathema::writer::unicode_math::UnicodeMathWriter::write_compact,
    ] {
        assert!(write(&expr, &mathema::Config::default()).is_err());
    }
}

#[test]
fn attaching_a_script_honors_the_requested_writer_depth() {
    use mathema::{Config, Error, LimitKind, MathExpr, MathNode};
    let heap = MathExpr::inline(vec![
        MathNode::Superscript {
            base: Box::new(MathNode::Row(vec![])),
            sup: Box::new(MathNode::Identifier("a".into())),
        },
        MathNode::Sqrt(Box::new(MathNode::Identifier("x".into()))),
    ]);
    let compact = CompactExpr::from_math_expr(&heap).unwrap();
    let config = Config::default().with_max_depth(1);
    for write in [
        mathema::writer::mathml::MathmlWriter::write_compact,
        mathema::writer::omml::OmmlWriter::write_compact,
        mathema::writer::unicode_math::UnicodeMathWriter::write_compact,
    ] {
        assert!(matches!(
            write(&compact, &config),
            Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: 1
            })
        ));
    }
}
