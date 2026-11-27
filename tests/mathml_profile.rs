use mathema::writer::Writer;
use mathema::writer::mathml::{MathmlOptions, MathmlProfile, MathmlWriter};
use mathema::{MathExpr, MathNode, from_latex, from_mathml};

#[test]
fn standard_profile_removes_private_round_trip_attributes() {
    let result = from_latex(r"\begin{matrix}a&b\\c&d\end{matrix}").unwrap();
    let round_trip = result.to_mathml().unwrap();
    assert!(round_trip.contains("data-"), "{round_trip}");

    let (standard, diagnostics) = result
        .to_mathml_with_options_and_diagnostics(&MathmlOptions::standard())
        .unwrap();
    assert!(!standard.contains("data-"), "{standard}");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("private round-trip")),
        "{diagnostics:?}"
    );
    roxmltree::Document::parse(&standard).unwrap();
}

#[test]
fn standard_profile_canonicalizes_css_colors_and_drops_unknown_values() {
    let named =
        from_mathml(r#"<math><mstyle mathcolor="rebeccapurple"><mi>x</mi></mstyle></math>"#)
            .unwrap();
    let standard = named
        .to_mathml_with_options(&MathmlOptions::standard())
        .unwrap();
    assert!(standard.contains(r##"mathcolor="#663399""##), "{standard}");

    let unknown =
        from_mathml(r#"<math><mstyle mathcolor="not-a-color"><mi>x</mi></mstyle></math>"#).unwrap();
    let (standard, diagnostics) = unknown
        .to_mathml_with_options_and_diagnostics(&MathmlOptions::standard())
        .unwrap();
    assert!(!standard.contains("mathcolor"), "{standard}");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unsupported mathcolor")),
        "{diagnostics:?}"
    );

    let whitespace =
        from_mathml("<math><mstyle mathcolor=\"\tRED \n\"><mi>x</mi></mstyle></math>").unwrap();
    let standard = whitespace
        .to_mathml_with_options(&MathmlOptions::standard())
        .unwrap();
    assert!(standard.contains("mathcolor=\"RED\""), "{standard}");
}

#[test]
fn standard_profile_preserves_smallmatrix_scriptlevel() {
    let result = from_latex(r"\begin{smallmatrix}a&b\\c&d\end{smallmatrix}").unwrap();
    let (standard, diagnostics) = result
        .to_mathml_with_options_and_diagnostics(&MathmlOptions::standard())
        .unwrap();
    assert!(
        standard.contains(r#"<mstyle scriptlevel="1"><mtable"#),
        "{standard}"
    );
    assert!(standard.contains(r#"rowspacing="0.1em" columnspacing="0.2778em""#));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("removed 1 scriptlevel"))
    );
    roxmltree::Document::parse(&standard).unwrap();
}

#[test]
fn standard_profile_drops_illegal_scriptlevel_from_table_children() {
    let result = from_mathml(
        r#"<math><mtable scriptlevel="2"><mtr><mtd><mi>x</mi></mtd></mtr></mtable></math>"#,
    )
    .unwrap();
    let (standard, diagnostics) = result
        .to_mathml_with_options_and_diagnostics(&MathmlOptions::standard())
        .unwrap();
    assert!(!standard.contains(r#"<mtable scriptlevel="#), "{standard}");
    let _ = diagnostics;
    roxmltree::Document::parse(&standard).unwrap();
}

#[test]
fn standard_profile_escapes_text_once() {
    let result = from_latex(r"\text{a\&b}").unwrap();
    let standard = result
        .to_mathml_with_options(&MathmlOptions::standard())
        .unwrap();
    assert!(standard.contains("a&amp;b"), "{standard}");
    assert!(!standard.contains("a&amp;amp;b"), "{standard}");
}

#[test]
fn standard_profile_escapes_allowed_style_attributes_once() {
    let expr = MathExpr::inline(vec![MathNode::Boxed {
        content: Box::new(MathNode::Identifier("x".into())),
        bg_color: Some("red&blue\"".into()),
        border_color: Some("black&white\"".into()),
    }]);
    let compact = mathema::CompactExpr::from_math_expr(&expr).unwrap();
    let standard =
        MathmlWriter::write_with_options(&compact, &Default::default(), &MathmlOptions::standard())
            .unwrap()
            .0;
    assert!(standard.contains("black&amp;white&quot;"), "{standard}");
    assert!(standard.contains("red&amp;blue&quot;"), "{standard}");
    assert!(!standard.contains("black&amp;amp;white"), "{standard}");
}

#[test]
fn compact_writer_options_match_builder_output() {
    let result = from_latex(r"x^2").unwrap();
    let options = MathmlOptions::default().with_profile(MathmlProfile::Standard);
    let compact =
        MathmlWriter::write_with_options(result.compact_expr(), &Default::default(), &options)
            .unwrap()
            .0;
    assert_eq!(compact, result.to_mathml_with_options(&options).unwrap());

    let heap = result.expr();
    let ordinary = MathmlWriter::write(&heap, &Default::default()).unwrap();
    assert!(ordinary.contains("<math"));
}
