//! Integration tests for font-embedded SVG output.
#![cfg(feature = "font-embed")]

use mathema::{RenderOptions, render_to_svg_standalone, render_to_svg_standalone_compact_arena};

fn svg_from_latex(latex: &str) -> String {
    use mathema::reader::Reader;
    let config = mathema::Config::default();
    let result = mathema::reader::latex::LatexReader::parse(latex, &config).unwrap();
    let options = RenderOptions::default();
    render_to_svg_standalone(&result.expr, &options).unwrap()
}

fn viewbox_values(svg: &str) -> Vec<f64> {
    let start = svg.find("viewBox=\"").expect("SVG should contain viewBox") + 9;
    let rest = &svg[start..];
    let end = rest.find('"').expect("viewBox should be closed");
    rest[..end]
        .split_whitespace()
        .map(|part| part.parse::<f64>().expect("viewBox value should parse"))
        .collect()
}

fn use_translate_x_values(svg: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut rest = svg;
    while let Some(use_start) = rest.find("<use") {
        let use_el = &rest[use_start..];
        if let Some(translate_start) = use_el.find("translate(") {
            let x_start = translate_start + "translate(".len();
            let x_rest = &use_el[x_start..];
            let x_end = x_rest
                .find(',')
                .expect("translate should contain comma after x");
            values.push(
                x_rest[..x_end]
                    .parse::<f64>()
                    .expect("translate x should parse as f64"),
            );
        }
        rest = &use_el[4..];
    }
    values
}

fn first_rect_attr(svg: &str, attr: &str) -> f64 {
    let rect_start = svg.find("<rect").expect("SVG should contain a rect");
    let rect = &svg[rect_start..];
    let needle = format!(" {attr}=\"");
    let attr_start = rect.find(&needle).expect("rect should contain attribute") + needle.len();
    let attr_rest = &rect[attr_start..];
    let attr_end = attr_rest
        .find('"')
        .expect("rect attribute should be closed");
    attr_rest[..attr_end]
        .parse::<f64>()
        .expect("rect attribute should parse as f64")
}

fn assert_first_rect_inside_vertical_viewbox(svg: &str) {
    let viewbox = viewbox_values(svg);
    assert_eq!(viewbox.len(), 4, "viewBox should have 4 values: {svg}");
    let viewbox_top = viewbox[1];
    let viewbox_bottom = viewbox[1] + viewbox[3];
    let rect_y = first_rect_attr(svg, "y");
    let rect_height = first_rect_attr(svg, "height");

    assert!(
        rect_y >= viewbox_top - 0.001,
        "rule rect top {rect_y} should be inside viewBox top {viewbox_top}: {svg}"
    );
    assert!(
        rect_y + rect_height <= viewbox_bottom + 0.001,
        "rule rect bottom {} should be inside viewBox bottom {viewbox_bottom}: {svg}",
        rect_y + rect_height
    );
}

#[test]
fn standalone_svg_rule_rect_is_not_vertically_clipped() {
    use mathema::reader::Reader;

    let heap_svg = svg_from_latex(r"\rule{1em}{1em}");
    assert_first_rect_inside_vertical_viewbox(&heap_svg);

    let config = mathema::Config::default();
    let parsed =
        mathema::reader::latex::LatexReader::parse_compact(r"\rule{1em}{1em}", &config).unwrap();
    let options = RenderOptions::default();
    let mut layout_arena = mathema::render::arena::LayoutArena::new();
    let arena_svg =
        render_to_svg_standalone_compact_arena(&parsed.expr, &options, &mut layout_arena).unwrap();
    assert_first_rect_inside_vertical_viewbox(&arena_svg);

    assert_eq!(
        first_rect_attr(&heap_svg, "y"),
        first_rect_attr(&arena_svg, "y"),
        "heap and arena standalone SVG should place explicit rules at the same y"
    );
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
fn standalone_svg_embeds_math_italic_text_run_glyphs() {
    let svg = svg_from_latex(r"\textit{ab}");
    let italic_a = 0x1D44E_u32;
    let italic_b = 0x1D44F_u32;

    assert!(
        svg.contains(&format!("id=\"g{italic_a}\"")),
        "standalone SVG should define math italic 'a' glyph: {svg}"
    );
    assert!(
        svg.contains(&format!("id=\"g{italic_b}\"")),
        "standalone SVG should define math italic 'b' glyph: {svg}"
    );
    assert!(
        svg.contains(&format!("href=\"#g{italic_a}\"")),
        "standalone SVG should use math italic 'a' glyph: {svg}"
    );
    assert!(
        !svg.contains("skewX"),
        "standalone SVG should not synthesize italic when math italic glyphs exist: {svg}"
    );
}

#[test]
fn standalone_svg_viewbox_includes_leading_negative_space() {
    let svg = svg_from_latex(r"\!\!\!\!\!\!x");
    let viewbox = viewbox_values(&svg);
    assert_eq!(viewbox.len(), 4, "viewBox should have 4 values: {svg}");
    let left = viewbox[0];
    let min_use_x = use_translate_x_values(&svg)
        .into_iter()
        .reduce(f64::min)
        .expect("standalone SVG should contain glyph uses");

    assert!(
        left <= min_use_x,
        "viewBox left edge {left} should include leftmost glyph x {min_use_x}: {svg}"
    );
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
        "\\sum_{i=1}^{n} \\frac{\\sqrt{x_i^2 + y_i^2}}{\\frac{a + b}{c}} = \\int_0^1 z",
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
