#![cfg(feature = "render")]

use mathema::{Format, SvgOptions, convert, from_latex};
use std::sync::{Arc, atomic::AtomicBool};

#[test]
fn conversion_svg_defaults_to_embedded_outlines_and_text_is_explicit() {
    let converted = convert("x^2", Format::LaTeX, Format::Svg).unwrap();
    assert!(converted.contains("<path"), "{converted}");
    assert!(converted.contains("<use"), "{converted}");
    assert!(!converted.contains("<text"), "{converted}");

    let parsed = from_latex("x^2").unwrap();
    let text = parsed.to_svg_text().unwrap();
    assert!(text.contains("<text"), "{text}");
}

#[test]
fn svg_options_add_accessibility_metadata_and_prefixed_ids() {
    let parsed = from_latex("x^2 + y^2").unwrap();
    let options = SvgOptions::default()
        .with_title("Pythagorean <x> & y")
        .with_aria_label("equation & comparison")
        .with_id_prefix("equation_42_")
        .unwrap();
    let svg = parsed.to_svg_with_svg_options(&options).unwrap();

    assert!(svg.contains(r#"role="img""#), "{svg}");
    assert!(
        svg.contains(r#"aria-label="equation &amp; comparison""#),
        "{svg}"
    );
    assert!(
        svg.contains("<title>Pythagorean &lt;x&gt; &amp; y</title>"),
        "{svg}"
    );
    assert!(svg.contains(r#"id="equation_42_g"#), "{svg}");
    assert!(svg.contains(r##"href="#equation_42_g"##), "{svg}");
    assert!(!svg.contains(r#"id="g"#), "{svg}");
    assert!(!svg.contains(r##"href="#g"##), "{svg}");
    roxmltree::Document::parse(&svg).unwrap();

    let (diagnostic_svg, _) = parsed
        .to_svg_with_svg_options_and_diagnostics(&options)
        .unwrap();
    assert!(
        diagnostic_svg.contains(r#"id="equation_42_g"#),
        "{diagnostic_svg}"
    );
    assert!(
        diagnostic_svg.contains(r##"href="#equation_42_g"##),
        "{diagnostic_svg}"
    );
}

#[test]
fn svg_options_reject_an_invalid_id_prefix() {
    let error = SvgOptions::default()
        .with_id_prefix("bad prefix")
        .unwrap_err();
    assert!(error.to_string().contains("id_prefix"), "{error}");
}

#[test]
fn svg_id_prefix_does_not_rewrite_user_metadata_text() {
    let parsed = from_latex("x").unwrap();
    let options = SvgOptions::default()
        .with_title(r##"id="g120" href="#g120" url(#g120)"##)
        .with_id_prefix("doc_")
        .unwrap();
    let svg = parsed.to_svg_with_svg_options(&options).unwrap();
    assert!(
        svg.contains(r#"<title>id=&quot;g120&quot; href=&quot;#g120&quot; url(#g120)</title>"#),
        "{svg}"
    );
    assert!(svg.contains(r#"id="doc_g"#), "{svg}");
    assert!(svg.contains(r##"href="#doc_g"##), "{svg}");
}

#[test]
fn separate_inline_svg_documents_have_independent_generated_id_namespaces() {
    let parsed = from_latex(r"\sum_{i=0}^{n} x_i").unwrap();
    let mut documents = Vec::new();
    for prefix in ["inline_a_", "inline_b_"] {
        let options = SvgOptions::default().with_id_prefix(prefix).unwrap();
        let svg = parsed.to_svg_with_svg_options(&options).unwrap();
        assert!(svg.contains(&format!(r#"id="{prefix}g"#)), "{svg}");
        assert!(svg.contains(&format!(r##"href="#{prefix}g"##)), "{svg}");
        assert!(!svg.contains(r#"id="g"#), "{svg}");
        assert!(!svg.contains(r##"href="#g"##), "{svg}");
        roxmltree::Document::parse(&svg).unwrap();
        documents.push(svg);
    }
    assert_ne!(documents[0], documents[1]);
}

#[test]
fn metadata_is_counted_before_bounded_emission_and_prepared_output_matches() {
    let parsed = from_latex("x^2 + y^2").unwrap();
    let options = SvgOptions::default()
        .with_title("x".repeat(4096))
        .with_aria_label("equation")
        .with_id_prefix("bounded_")
        .unwrap();
    let limits = mathema::render::SvgLimits::with_max_bytes(128);
    assert!(
        parsed
            .to_svg_with_svg_options_and_limits(&options, &limits)
            .is_err()
    );

    let options = SvgOptions::default()
        .with_title("Equation")
        .with_id_prefix("prepared_")
        .unwrap();
    let prepared = parsed.prepare_render(&options.render).unwrap();
    let direct = parsed.to_svg_with_svg_options(&options).unwrap();
    let prepared_svg = prepared.to_svg_with_svg_options(&options).unwrap();
    assert_eq!(direct, prepared_svg);
    let (bounded, diagnostics) = prepared
        .to_svg_with_svg_options_and_limits_and_diagnostics(
            &options,
            &mathema::render::SvgLimits::with_max_bytes(direct.len() + 32),
        )
        .unwrap();
    assert_eq!(bounded, direct);
    assert!(diagnostics.is_empty());

    let cancelled =
        mathema::render::SvgLimits::default().with_cancellation(Arc::new(AtomicBool::new(true)));
    assert!(
        prepared
            .to_svg_with_svg_options_and_limits(&options, &cancelled)
            .is_err()
    );
}
