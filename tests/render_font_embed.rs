//! Integration tests for font-embedded SVG output.
#![cfg(feature = "font-embed")]

use mathema::{RenderOptions, render_to_svg_standalone};

fn svg_from_latex(latex: &str) -> String {
    use mathema::reader::Reader;
    let config = mathema::Config::default();
    let result = mathema::reader::latex::LatexReader::parse(latex, &config).unwrap();
    let options = RenderOptions::default();
    render_to_svg_standalone(&result.expr, &options).unwrap()
}

#[test]
fn standalone_svg_is_valid_xml() {
    let svg = svg_from_latex("x + 1");
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert_eq!(
        svg.matches("<defs>").count(),
        svg.matches("</defs>").count(),
    );
}

#[test]
fn standalone_svg_no_font_family_dependency() {
    let svg = svg_from_latex("x^2 + y^2 = z^2");
    assert!(
        !svg.contains("font-family"),
        "standalone SVG should not reference font-family"
    );
}

#[test]
fn standalone_svg_contains_paths_for_all_chars() {
    let svg = svg_from_latex("a + b");
    // 'a'=97, '+'=43, 'b'=98
    assert!(svg.contains("id=\"g97\""), "should have glyph for 'a'");
    assert!(svg.contains("id=\"g43\""), "should have glyph for '+'");
    assert!(svg.contains("id=\"g98\""), "should have glyph for 'b'");
}

#[test]
fn quadratic_formula() {
    let svg = svg_from_latex("x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}");
    assert!(svg.contains("<defs>"));
    assert!(svg.contains("<use"));
    assert!(svg.contains("<rect")); // fraction bar
    assert!(svg.contains("<path")); // glyph outlines + radical sign
}

#[test]
fn complex_formula_completes_under_budget() {
    use std::time::Instant;
    let start = Instant::now();
    let svg = svg_from_latex(
        "\\sum_{i=1}^{n} \\frac{\\sqrt{x_i^2 + y_i^2}}{\\frac{a + b}{c}} = \\int_0^1 z"
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "Complex formula SVG took {elapsed:?}, should be <100ms"
    );
    assert!(svg.contains("<defs>"));
    assert!(!svg.is_empty());
}

#[test]
fn euler_identity() {
    let svg = svg_from_latex("e^{i\\pi} + 1 = 0");
    assert!(svg.contains("<defs>"));
    assert!(svg.contains("<use"));
}

#[test]
fn display_mode_fraction() {
    use mathema::reader::Reader;
    let config = mathema::Config::default();
    let result = mathema::reader::latex::LatexReader::parse("\\frac{1}{2}", &config).unwrap();
    let options = RenderOptions {
        display_mode: Some(true),
        ..RenderOptions::default()
    };
    let svg = render_to_svg_standalone(&result.expr, &options).unwrap();
    assert!(svg.contains("<rect"), "display fraction should have bar");
    assert!(svg.contains("<defs>"));
}

#[test]
fn custom_foreground_color() {
    use mathema::reader::Reader;
    let config = mathema::Config::default();
    let result = mathema::reader::latex::LatexReader::parse("x", &config).unwrap();
    let options = RenderOptions {
        fg_color: "blue".into(),
        ..RenderOptions::default()
    };
    let svg = render_to_svg_standalone(&result.expr, &options).unwrap();
    assert!(svg.contains("fill=\"blue\""));
}

#[test]
fn positive_dimensions() {
    let svg = svg_from_latex("\\int_0^1 x");
    let vb_start = svg.find("viewBox=\"").unwrap() + 9;
    let vb_end = svg[vb_start..].find('"').unwrap() + vb_start;
    let vb: Vec<f64> = svg[vb_start..vb_end]
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    assert_eq!(vb.len(), 4);
    assert!(vb[2] > 0.0, "width must be positive");
    assert!(vb[3] > 0.0, "height must be positive");
}

#[test]
fn standalone_vs_regular_same_dimensions() {
    use mathema::reader::Reader;
    let config = mathema::Config::default();
    let result = mathema::reader::latex::LatexReader::parse("x^2", &config).unwrap();
    let options = RenderOptions::default();

    let regular = mathema::render_to_svg(&result.expr, &options).unwrap();
    let standalone = render_to_svg_standalone(&result.expr, &options).unwrap();

    fn extract_viewbox(svg: &str) -> &str {
        let start = svg.find("viewBox=\"").unwrap() + 9;
        let end = svg[start..].find('"').unwrap() + start;
        &svg[start..end]
    }

    assert_eq!(
        extract_viewbox(&regular),
        extract_viewbox(&standalone),
        "standalone and regular SVG should have same viewBox"
    );
}
