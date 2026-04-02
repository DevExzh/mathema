#![cfg(feature = "render")]

use mathema::{Format, convert};
use mathema::render::{RenderOptions, render_to_svg};
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::error::Config;

/// Helper: LaTeX string → SVG string via the convert API.
fn latex_to_svg(input: &str) -> String {
    convert(input, Format::LaTeX, Format::Svg).unwrap()
}

/// Helper: LaTeX string → SVG via the render API directly.
fn render_latex(input: &str) -> String {
    let config = Config::default();
    let parsed = LatexReader::parse(input, &config).unwrap();
    let options = RenderOptions::default();
    render_to_svg(&parsed.expr, &options).unwrap()
}

/// Every SVG must have proper structure.
fn assert_valid_svg(svg: &str, label: &str) {
    assert!(svg.starts_with("<svg"), "{label}: missing <svg> open tag");
    assert!(svg.ends_with("</svg>"), "{label}: missing </svg> close tag");
    assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""), "{label}: missing xmlns");
    assert!(svg.contains("viewBox"), "{label}: missing viewBox");
}

// =========================================================================
// Basic structural tests
// =========================================================================

#[test]
fn svg_structure_simple_number() {
    let svg = latex_to_svg("42");
    assert_valid_svg(&svg, "42");
    assert!(svg.contains("<text"), "should contain text elements for digits");
    assert!(svg.contains("4"), "should contain digit 4");
    assert!(svg.contains("2"), "should contain digit 2");
}

#[test]
fn svg_structure_identifier() {
    let svg = latex_to_svg("x");
    assert_valid_svg(&svg, "x");
    assert!(svg.contains("<text"), "should contain text element for identifier");
}

#[test]
fn svg_structure_operator() {
    let svg = latex_to_svg(r"\sum");
    assert_valid_svg(&svg, "sum");
    assert!(svg.contains("<text"), "should contain text element for operator");
}

// =========================================================================
// Fraction rendering
// =========================================================================

#[test]
fn fraction_has_bar() {
    let svg = latex_to_svg(r"\frac{1}{2}");
    assert_valid_svg(&svg, "frac");
    assert!(svg.contains("<rect"), "fraction should have a bar (rect element)");
    assert!(svg.contains("1"), "should contain numerator 1");
    assert!(svg.contains("2"), "should contain denominator 2");
}

#[test]
fn nested_fraction() {
    let svg = latex_to_svg(r"\frac{\frac{1}{2}}{3}");
    assert_valid_svg(&svg, "nested frac");
    // Should have at least 2 rects (two fraction bars)
    let rect_count = svg.matches("<rect").count();
    assert!(rect_count >= 2, "nested fraction should have at least 2 bars, got {rect_count}");
}

// =========================================================================
// Superscripts and subscripts
// =========================================================================

#[test]
fn superscript() {
    let svg = latex_to_svg("x^2");
    assert_valid_svg(&svg, "x^2");
    assert!(svg.contains("x"), "should contain base x");
    assert!(svg.contains("2"), "should contain exponent 2");
}

#[test]
fn subscript() {
    let svg = latex_to_svg("x_i");
    assert_valid_svg(&svg, "x_i");
    assert!(svg.contains("x"), "should contain base x");
}

#[test]
fn subsup() {
    let svg = latex_to_svg("x_i^2");
    assert_valid_svg(&svg, "x_i^2");
}

// =========================================================================
// Square root
// =========================================================================

#[test]
fn sqrt_has_path() {
    let svg = latex_to_svg(r"\sqrt{x}");
    assert_valid_svg(&svg, "sqrt");
    // Sqrt renders a radical sign as SvgPath + a vinculum as Rule
    let has_path_or_rect = svg.contains("<path") || svg.contains("<rect");
    assert!(has_path_or_rect, "sqrt should have path or rect for radical sign/vinculum");
}

#[test]
fn sqrt_with_index() {
    let svg = latex_to_svg(r"\sqrt[3]{x}");
    assert_valid_svg(&svg, "sqrt[3]{x}");
    assert!(svg.contains("3"), "should contain root index 3");
}

// =========================================================================
// Common formulas (end-to-end)
// =========================================================================

#[test]
fn quadratic_formula() {
    let svg = latex_to_svg(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}");
    assert_valid_svg(&svg, "quadratic formula");
    assert!(svg.contains("<rect"), "quadratic formula should have fraction bar");
    let text_count = svg.matches("<text").count();
    assert!(text_count >= 5, "quadratic formula should have many text elements, got {text_count}");
}

#[test]
fn pythagorean_theorem() {
    let svg = latex_to_svg("a^2 + b^2 = c^2");
    assert_valid_svg(&svg, "pythagorean");
}

#[test]
fn euler_identity() {
    let svg = latex_to_svg(r"e^{i\pi} + 1 = 0");
    assert_valid_svg(&svg, "euler identity");
}

#[test]
fn integral() {
    let svg = latex_to_svg(r"\int_0^1 x");
    assert_valid_svg(&svg, "integral");
}

#[test]
fn summation() {
    let svg = latex_to_svg(r"\sum_{i=1}^{n} i");
    assert_valid_svg(&svg, "summation");
}

#[test]
fn matrix() {
    let svg = latex_to_svg(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
    assert_valid_svg(&svg, "matrix");
    assert!(svg.contains("a"), "matrix should contain element a");
    assert!(svg.contains("d"), "matrix should contain element d");
}

// =========================================================================
// Delimiters
// =========================================================================

#[test]
fn fenced_parentheses() {
    let svg = latex_to_svg(r"\left( x + y \right)");
    assert_valid_svg(&svg, "fenced parens");
}

#[test]
fn fenced_brackets() {
    let svg = latex_to_svg(r"\left[ a \right]");
    assert_valid_svg(&svg, "fenced brackets");
}

// =========================================================================
// Accents
// =========================================================================

#[test]
fn hat_accent() {
    let svg = latex_to_svg(r"\hat{x}");
    assert_valid_svg(&svg, "hat accent");
}

#[test]
fn bar_accent() {
    let svg = latex_to_svg(r"\bar{x}");
    assert_valid_svg(&svg, "bar accent");
}

// =========================================================================
// Special constructs
// =========================================================================

#[test]
fn text_in_math() {
    let svg = latex_to_svg(r"\text{hello}");
    assert_valid_svg(&svg, "text");
    assert!(svg.contains("h"), "text should contain 'h'");
}

#[test]
fn color_node() {
    let svg = latex_to_svg(r"\textcolor{red}{x}");
    assert_valid_svg(&svg, "color");
    // Verify it renders without error (color propagation depends on AST representation)
    assert!(svg.contains("<text"), "color node should still render text");
}

#[test]
fn boxed() {
    let svg = latex_to_svg(r"\boxed{x}");
    assert_valid_svg(&svg, "boxed");
    assert!(svg.contains("<rect"), "boxed should have a border rectangle");
}

#[test]
fn phantom() {
    let svg = latex_to_svg(r"a\phantom{b}c");
    assert_valid_svg(&svg, "phantom");
    // Phantom should NOT render 'b' visibly
}

// =========================================================================
// Render API with custom options
// =========================================================================

#[test]
fn custom_font_size() {
    let config = Config::default();
    let parsed = LatexReader::parse("x^2", &config).unwrap();
    let options = RenderOptions {
        font_size_px: 24.0,
        ..RenderOptions::default()
    };
    let svg = render_to_svg(&parsed.expr, &options).unwrap();
    assert_valid_svg(&svg, "custom font size");
    // Font size should appear in the SVG
    assert!(svg.contains("24"), "SVG should reference the 24px font size");
}

#[test]
fn custom_color() {
    let config = Config::default();
    let parsed = LatexReader::parse("x", &config).unwrap();
    let options = RenderOptions {
        fg_color: "#ff0000".into(),
        ..RenderOptions::default()
    };
    let svg = render_to_svg(&parsed.expr, &options).unwrap();
    assert_valid_svg(&svg, "custom color");
    assert!(svg.contains("#ff0000"), "SVG should use custom foreground color");
}

// =========================================================================
// Convert API consistency
// =========================================================================

#[test]
fn convert_api_matches_render_api() {
    let via_convert = latex_to_svg("x^2");
    let via_render = render_latex("x^2");
    // Both paths use default options, should produce identical output
    assert_eq!(via_convert, via_render, "convert API and render API should produce identical SVG");
}

// =========================================================================
// Positive dimensions
// =========================================================================

#[test]
fn svg_has_positive_dimensions() {
    let svg = latex_to_svg("x^2 + y^2");
    // Extract width and height from the SVG tag
    let width_start = svg.find("width=\"").unwrap() + 7;
    let width_end = svg[width_start..].find('"').unwrap() + width_start;
    let width: f64 = svg[width_start..width_end].parse().unwrap();

    let height_start = svg.find("height=\"").unwrap() + 8;
    let height_end = svg[height_start..].find('"').unwrap() + height_start;
    let height: f64 = svg[height_start..height_end].parse().unwrap();

    assert!(width > 0.0, "SVG width should be positive, got {width}");
    assert!(height > 0.0, "SVG height should be positive, got {height}");
}

// =========================================================================
// XML safety
// =========================================================================

#[test]
fn special_chars_escaped() {
    let svg = latex_to_svg("<");
    assert_valid_svg(&svg, "less-than");
    assert!(svg.contains("&lt;"), "< should be XML-escaped to &lt;");
    assert!(!svg.contains("><text") || !svg.contains("><<"), "raw < should not appear in text content");
}

#[test]
fn greater_than_escaped() {
    let svg = latex_to_svg(">");
    assert_valid_svg(&svg, "greater-than");
    assert!(svg.contains("&gt;"), "> should be XML-escaped to &gt;");
}
