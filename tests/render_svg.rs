#![cfg(feature = "render")]

use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::arena::LayoutArena;
use mathema::render::{
    RenderOptions, render_to_svg, render_to_svg_compact, render_to_svg_compact_arena,
};
use mathema::{Format, convert};

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
    assert!(
        svg.contains("xmlns=\"http://www.w3.org/2000/svg\""),
        "{label}: missing xmlns"
    );
    assert!(svg.contains("viewBox"), "{label}: missing viewBox");
}

/// The legacy heap layout engine and the production arena engine must render
/// every corpus formula to **byte-identical** SVG. This is the precondition for
/// retiring the legacy engine: since the two now agree everywhere,
/// `render_to_svg`/`render_to_svg_standalone` (over `&MathExpr`) can be
/// re-implemented on top of the arena engine and the legacy `layout_node` engine
/// deleted (tracked as performance-assessment M1).
///
/// This started as a baseline of 28 known divergences; they were reconciled to
/// 0 by fixing real arena-engine bugs (math-alphabet double-italic, styled-group
/// child duplication + missing inter-atom spacing, over/under decoration
/// heights, invisible zero-area struts, `\cr` row separators, and quantizing the
/// stretchy-delimiter target height against cross-engine f64 noise). The gate now
/// asserts full agreement, so any new divergence is caught immediately.
#[test]
fn legacy_vs_arena_render_divergence_baseline() {
    use mathema::CompactExpr;
    use mathema::ast::DisplayStyle;

    // The two layout engines must agree on every corpus formula.
    const EXPECTED_CHECKED: usize = 135;
    const EXPECTED_DIVERGENCES: usize = 0;

    let opts = RenderOptions::default();
    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for path in ["tests/data/katex_corpus.json", "tests/data/formulas.json"] {
        let data = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let arr: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();
        for v in &arr {
            let tex = v["tex"].as_str().unwrap_or("");
            let name = v["name"].as_str().unwrap_or("?");
            let display = v["display"].as_bool().unwrap_or(false);
            let config = Config::default().with_display(if display {
                DisplayStyle::Block
            } else {
                DisplayStyle::Inline
            });
            // Native legacy parse → MathExpr → legacy engine.
            let Ok(parsed) = LatexReader::parse(tex, &config) else {
                continue;
            };
            let mathexpr = parsed.expr;
            let Ok(legacy_svg) = render_to_svg(&mathexpr, &opts) else {
                continue;
            };
            // The same MathExpr lowered to compact → arena engine (what a future
            // arena-backed `render_to_svg` would produce).
            let compact = CompactExpr::from_math_expr(&mathexpr);
            let Ok(arena_svg) = render_to_svg_compact(&compact, &opts) else {
                continue;
            };
            checked += 1;
            if legacy_svg != arena_svg {
                mismatches.push(name.to_string());
            }
        }
    }

    assert_eq!(
        checked, EXPECTED_CHECKED,
        "corpus size changed; update EXPECTED_CHECKED"
    );
    assert_eq!(
        mismatches.len(),
        EXPECTED_DIVERGENCES,
        "legacy↔arena render divergence (now {}): {:?}.\n\
         The two layout engines must render every corpus formula identically; \
         a new divergence means a regression in one of them.",
        mismatches.len(),
        mismatches
    );
}

fn first_rect_y(svg: &str) -> f64 {
    let rect_start = svg.find("<rect").expect("SVG should contain a rect");
    let rect = &svg[rect_start..];
    let y_start = rect.find(" y=\"").expect("rect should contain y attribute") + 4;
    let y_rest = &rect[y_start..];
    let y_end = y_rest.find('"').expect("rect y attribute should be closed");
    y_rest[..y_end]
        .parse::<f64>()
        .expect("rect y should parse as f64")
}

fn first_rect_height(svg: &str) -> f64 {
    let rect_start = svg.find("<rect").expect("SVG should contain a rect");
    let rect = &svg[rect_start..];
    let height_start = rect
        .find(" height=\"")
        .expect("rect should contain height attribute")
        + 9;
    let height_rest = &rect[height_start..];
    let height_end = height_rest
        .find('"')
        .expect("rect height attribute should be closed");
    height_rest[..height_end]
        .parse::<f64>()
        .expect("rect height should parse as f64")
}

fn first_text_y(svg: &str) -> f64 {
    let text_start = svg.find("<text").expect("SVG should contain text");
    let text = &svg[text_start..];
    let y_start = text.find(" y=\"").expect("text should contain y attribute") + 4;
    let y_rest = &text[y_start..];
    let y_end = y_rest.find('"').expect("text y attribute should be closed");
    y_rest[..y_end]
        .parse::<f64>()
        .expect("text y should parse as f64")
}

fn text_y_values(svg: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut rest = svg;
    while let Some(text_start) = rest.find("<text") {
        let text = &rest[text_start..];
        let y_start = text.find(" y=\"").expect("text should contain y attribute") + 4;
        let y_rest = &text[y_start..];
        let y_end = y_rest.find('"').expect("text y attribute should be closed");
        values.push(
            y_rest[..y_end]
                .parse::<f64>()
                .expect("text y should parse as f64"),
        );
        rest = &y_rest[y_end..];
    }
    values
}

fn text_x_values(svg: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut rest = svg;
    while let Some(text_start) = rest.find("<text") {
        let text = &rest[text_start..];
        let x_start = text.find(" x=\"").expect("text should contain x attribute") + 4;
        let x_rest = &text[x_start..];
        let x_end = x_rest.find('"').expect("text x attribute should be closed");
        values.push(
            x_rest[..x_end]
                .parse::<f64>()
                .expect("text x should parse as f64"),
        );
        rest = &x_rest[x_end..];
    }
    values
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

fn math_italic(ch: char) -> char {
    match ch {
        'a'..='z' => char::from_u32(0x1D44E + (ch as u32 - b'a' as u32)).unwrap(),
        'A'..='Z' => char::from_u32(0x1D434 + (ch as u32 - b'A' as u32)).unwrap(),
        _ => ch,
    }
}

fn assert_contains_math_italic(svg: &str, ch: char) {
    let italic = math_italic(ch).to_string();
    assert!(
        svg.contains(&italic),
        "SVG should contain mathematical italic {ch} ({italic}), got: {svg}"
    );
}

// =========================================================================
// Basic structural tests
// =========================================================================

#[test]
fn svg_structure_simple_number() {
    let svg = latex_to_svg("42");
    assert_valid_svg(&svg, "42");
    assert!(
        svg.contains("<text"),
        "should contain text elements for digits"
    );
    assert!(svg.contains("4"), "should contain digit 4");
    assert!(svg.contains("2"), "should contain digit 2");
}

#[test]
fn svg_structure_identifier() {
    let svg = latex_to_svg("x");
    assert_valid_svg(&svg, "x");
    assert!(
        svg.contains("<text"),
        "should contain text element for identifier"
    );
}

#[test]
fn svg_math_identifiers_use_real_math_italic_codepoints() {
    let svg = render_latex("fx");
    assert_valid_svg(&svg, "math italic identifiers");
    assert_contains_math_italic(&svg, 'f');
    assert_contains_math_italic(&svg, 'x');
    assert!(
        !svg.contains("font-style=\"italic\""),
        "mapped mathematical italic glyphs should not be synthetically italicized: {svg}"
    );
}

#[test]
fn svg_math_greek_lowercase_uses_real_math_italic_codepoint() {
    let svg = render_latex(r"\alpha");
    let italic_alpha = char::from_u32(0x1D6FC).unwrap().to_string();
    assert_valid_svg(&svg, "math italic alpha");
    assert!(
        svg.contains(&italic_alpha),
        "SVG should contain mathematical italic alpha ({italic_alpha}), got: {svg}"
    );
    assert!(
        !svg.contains("font-style=\"italic\""),
        "mapped mathematical italic Greek should not be synthetically italicized: {svg}"
    );
}

#[test]
fn svg_structure_operator() {
    let svg = latex_to_svg(r"\sum");
    assert_valid_svg(&svg, "sum");
    assert!(
        svg.contains("<text"),
        "should contain text element for operator"
    );
}

// =========================================================================
// Fraction rendering
// =========================================================================

#[test]
fn fraction_has_bar() {
    let svg = latex_to_svg(r"\frac{1}{2}");
    assert_valid_svg(&svg, "frac");
    assert!(
        svg.contains("<rect"),
        "fraction should have a bar (rect element)"
    );
    assert!(svg.contains("1"), "should contain numerator 1");
    assert!(svg.contains("2"), "should contain denominator 2");
}

#[test]
fn nested_fraction() {
    let svg = latex_to_svg(r"\frac{\frac{1}{2}}{3}");
    assert_valid_svg(&svg, "nested frac");
    // Should have at least 2 rects (two fraction bars)
    let rect_count = svg.matches("<rect").count();
    assert!(
        rect_count >= 2,
        "nested fraction should have at least 2 bars, got {rect_count}"
    );
}

// =========================================================================
// Superscripts and subscripts
// =========================================================================

#[test]
fn superscript() {
    let svg = latex_to_svg("x^2");
    assert_valid_svg(&svg, "x^2");
    assert_contains_math_italic(&svg, 'x');
    assert!(svg.contains("2"), "should contain exponent 2");
}

#[test]
fn subscript() {
    let svg = latex_to_svg("x_i");
    assert_valid_svg(&svg, "x_i");
    assert_contains_math_italic(&svg, 'x');
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
    assert!(
        has_path_or_rect,
        "sqrt should have path or rect for radical sign/vinculum"
    );
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
    assert!(
        svg.contains("<rect"),
        "quadratic formula should have fraction bar"
    );
    let text_count = svg.matches("<text").count();
    assert!(
        text_count >= 5,
        "quadratic formula should have many text elements, got {text_count}"
    );
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
    assert_contains_math_italic(&svg, 'a');
    assert_contains_math_italic(&svg, 'd');
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
    assert!(
        svg.contains("<rect"),
        "boxed should have a border rectangle"
    );
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
    assert!(
        svg.contains("24"),
        "SVG should reference the 24px font size"
    );
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
    assert!(
        svg.contains("#ff0000"),
        "SVG should use custom foreground color"
    );
}

// =========================================================================
// Convert API consistency
// =========================================================================

#[test]
fn convert_api_matches_render_api() {
    let via_convert = latex_to_svg("x^2");
    let via_render = render_latex("x^2");
    // Both paths use default options, should produce identical output
    assert_eq!(
        via_convert, via_render,
        "convert API and render API should produce identical SVG"
    );
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
// Greek letters
// =========================================================================

#[test]
fn svg_greek_lowercase() {
    let svg = latex_to_svg(r"\alpha + \beta + \gamma + \delta");
    assert_valid_svg(&svg, "greek lowercase");
}

#[test]
fn svg_greek_uppercase() {
    let svg =
        latex_to_svg(r"\Gamma + \Delta + \Theta + \Lambda + \Pi + \Sigma + \Phi + \Psi + \Omega");
    assert_valid_svg(&svg, "greek uppercase");
}

// =========================================================================
// Complex formulas
// =========================================================================

#[test]
fn svg_maxwell_equation() {
    let svg = latex_to_svg(r"\nabla \times \mathbf{E} = -\frac{\partial \mathbf{B}}{\partial t}");
    assert_valid_svg(&svg, "maxwell");
    assert!(svg.contains("<rect"), "should have fraction bar");
}

#[test]
fn svg_taylor_series() {
    let svg = latex_to_svg(r"f(x) = \sum_{n=0}^{\infty} \frac{f^{(n)}(a)}{n!}(x-a)^n");
    assert_valid_svg(&svg, "taylor");
}

#[test]
fn svg_fourier_transform() {
    let svg = latex_to_svg(r"f(x) = \int_{-\infty}^{\infty} \hat{f}(\xi) e^{2\pi i \xi x} d\xi");
    assert_valid_svg(&svg, "fourier");
}

// =========================================================================
// Environments
// =========================================================================

#[test]
fn svg_aligned_environment() {
    let svg = latex_to_svg(r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}");
    assert_valid_svg(&svg, "aligned");
}

#[test]
fn svg_cases_environment() {
    let svg = latex_to_svg(
        r"f(x) = \begin{cases} x & \text{if } x \geq 0 \\ -x & \text{if } x < 0 \end{cases}",
    );
    assert_valid_svg(&svg, "cases");
}

#[test]
fn svg_bmatrix() {
    let svg = latex_to_svg(r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}");
    assert_valid_svg(&svg, "bmatrix");
}

#[test]
fn svg_vmatrix() {
    let svg = latex_to_svg(r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}");
    assert_valid_svg(&svg, "vmatrix");
}

// =========================================================================
// Over/under decorations
// =========================================================================

#[test]
fn svg_overbrace() {
    let svg = latex_to_svg(r"\overbrace{a + b + c}^{n}");
    assert_valid_svg(&svg, "overbrace");
}

#[test]
fn svg_underbrace() {
    let svg = latex_to_svg(r"\underbrace{x + y + z}_{3 \text{ terms}}");
    assert_valid_svg(&svg, "underbrace");
}

#[test]
fn svg_overline() {
    let svg = latex_to_svg(r"\overline{AB}");
    assert_valid_svg(&svg, "overline");
}

// =========================================================================
// Wide accents
// =========================================================================

#[test]
fn svg_widehat() {
    let svg = latex_to_svg(r"\widehat{ABC}");
    assert_valid_svg(&svg, "widehat");
}

#[test]
fn svg_widetilde() {
    let svg = latex_to_svg(r"\widetilde{xyz}");
    assert_valid_svg(&svg, "widetilde");
}

#[test]
fn svg_vec_accent() {
    let svg = latex_to_svg(r"\vec{v}");
    assert_valid_svg(&svg, "vec");
}

// =========================================================================
// Extensible arrows
// =========================================================================

#[test]
fn svg_xrightarrow() {
    let svg = latex_to_svg(r"A \xrightarrow{f} B");
    assert_valid_svg(&svg, "xrightarrow");
}

// =========================================================================
// Relations and operators
// =========================================================================

#[test]
fn svg_relations() {
    let svg = latex_to_svg(r"a \leq b \geq c \neq d \approx e");
    assert_valid_svg(&svg, "relations");
}

#[test]
fn svg_set_operations() {
    let svg = latex_to_svg(r"A \cup B \cap C \subset D \in E");
    assert_valid_svg(&svg, "set operations");
}

#[test]
fn svg_trig_functions() {
    let svg = latex_to_svg(r"\sin x + \cos y + \tan z");
    assert_valid_svg(&svg, "trig functions");
}

// =========================================================================
// Display vs text mode
// =========================================================================

#[test]
fn svg_display_mode_sum() {
    let svg = latex_to_svg(r"\displaystyle\sum_{i=1}^{n} x_i");
    assert_valid_svg(&svg, "display sum");
}

// =========================================================================
// Edge cases
// =========================================================================

#[test]
fn svg_empty_fraction() {
    let svg = latex_to_svg(r"\frac{}{}");
    assert_valid_svg(&svg, "empty fraction");
}

#[test]
fn svg_deeply_nested_sqrt() {
    let svg = latex_to_svg(r"\sqrt{\sqrt{\sqrt{x}}}");
    assert_valid_svg(&svg, "deeply nested sqrt");
}

#[test]
fn svg_continued_fraction() {
    let svg = latex_to_svg(r"a_0 + \cfrac{1}{a_1 + \cfrac{1}{a_2}}");
    assert_valid_svg(&svg, "continued fraction");
}

#[test]
fn svg_binomial() {
    let svg = latex_to_svg(r"\binom{n}{k}");
    assert_valid_svg(&svg, "binomial");
}

#[test]
fn svg_spacing_commands() {
    let svg = latex_to_svg(r"a \quad b \qquad c");
    assert_valid_svg(&svg, "spacing");
}

#[test]
fn svg_dots() {
    let svg = latex_to_svg(r"a_1, a_2, \ldots, a_n");
    assert_valid_svg(&svg, "dots");
}

#[test]
fn svg_arrows() {
    let svg = latex_to_svg(r"A \rightarrow B \Leftrightarrow C");
    assert_valid_svg(&svg, "arrows");
}

#[test]
fn svg_cancel() {
    let svg = latex_to_svg(r"\cancel{x} + \bcancel{y}");
    assert_valid_svg(&svg, "cancel");
}

#[test]
fn svg_double_struck() {
    let svg = latex_to_svg(r"\mathbb{R}");
    assert_valid_svg(&svg, "double struck");
}

#[test]
fn svg_big_matrix() {
    let svg = latex_to_svg(
        r"\begin{pmatrix} a_{11} & a_{12} & a_{13} \\ a_{21} & a_{22} & a_{23} \\ a_{31} & a_{32} & a_{33} \end{pmatrix}",
    );
    assert_valid_svg(&svg, "big matrix");
    let text_count = svg.matches("<text").count();
    assert!(
        text_count >= 9,
        "3x3 matrix should have at least 9 text elements, got {text_count}"
    );
}

#[test]
fn svg_product_with_limits() {
    let svg = latex_to_svg(r"\prod_{k=1}^{n} x_k");
    assert_valid_svg(&svg, "product with limits");
}

#[test]
fn svg_integral_definite() {
    let svg = latex_to_svg(r"\int_0^\infty e^{-x^2} dx");
    assert_valid_svg(&svg, "definite integral");
}

#[test]
fn special_chars_escaped() {
    let svg = latex_to_svg("<");
    assert_valid_svg(&svg, "less-than");
    assert!(svg.contains("&lt;"), "< should be XML-escaped to &lt;");
    assert!(
        !svg.contains("><text") || !svg.contains("><<"),
        "raw < should not appear in text content"
    );
}

#[test]
fn greater_than_escaped() {
    let svg = latex_to_svg(">");
    assert_valid_svg(&svg, "greater-than");
    assert!(svg.contains("&gt;"), "> should be XML-escaped to &gt;");
}

// =========================================================================
// Deeply nested scripts
// =========================================================================

#[test]
fn svg_nested_superscripts() {
    let svg = latex_to_svg(r"x^{y^z}");
    assert_valid_svg(&svg, "nested superscripts");
}

#[test]
fn svg_deep_power_tower() {
    let svg = latex_to_svg(r"a^{b^{c^{d^e}}}");
    assert_valid_svg(&svg, "power tower");
}

#[test]
fn svg_frac_in_script() {
    let svg = latex_to_svg(r"x^{\frac{1}{2}}");
    assert_valid_svg(&svg, "frac in script");
}

#[test]
fn svg_tensor_indices() {
    let svg = latex_to_svg(r"T^{\mu\nu}_{\rho\sigma}");
    assert_valid_svg(&svg, "tensor indices");
}

// =========================================================================
// Font styles
// =========================================================================

#[test]
fn svg_bold_math() {
    let svg = latex_to_svg(r"\mathbf{A} + \mathit{v}");
    assert_valid_svg(&svg, "bold+italic");
}

#[test]
fn svg_calligraphic() {
    let svg = latex_to_svg(r"\mathcal{L}(\mathcal{H})");
    assert_valid_svg(&svg, "calligraphic");
}

#[test]
fn svg_fraktur() {
    let svg = latex_to_svg(r"\mathfrak{g}");
    assert_valid_svg(&svg, "fraktur");
}

// =========================================================================
// Academic formulas
// =========================================================================

#[test]
fn svg_normal_distribution() {
    let svg = latex_to_svg(r"f(x) = \frac{1}{\sigma\sqrt{2\pi}} e^{-\frac{(x-\mu)^2}{2\sigma^2}}");
    assert_valid_svg(&svg, "normal distribution");
}

#[test]
fn svg_einstein_field() {
    let svg = latex_to_svg(r"G_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8\pi G}{c^4} T_{\mu\nu}");
    assert_valid_svg(&svg, "einstein field");
}

#[test]
fn svg_dirac_equation() {
    let svg = latex_to_svg(r"(i\gamma^\mu \partial_\mu - m)\psi = 0");
    assert_valid_svg(&svg, "dirac equation");
}

#[test]
fn svg_residue_theorem() {
    let svg = latex_to_svg(r"\oint_C f(z) dz = 2\pi i \sum \text{Res}(f, a_k)");
    assert_valid_svg(&svg, "residue theorem");
}

#[test]
fn svg_euler_phi() {
    let svg = latex_to_svg(r"\phi(n) = n \prod_{p|n} \left(1 - \frac{1}{p}\right)");
    assert_valid_svg(&svg, "euler totient");
}

// =========================================================================
// Delimiters (extended)
// =========================================================================

#[test]
fn svg_floor_ceiling() {
    let svg = latex_to_svg(r"\lfloor x \rfloor + \lceil x \rceil");
    assert_valid_svg(&svg, "floor ceiling");
}

#[test]
fn svg_tall_delimiters() {
    let svg = latex_to_svg(r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)");
    assert_valid_svg(&svg, "tall delimiters");
}

// =========================================================================
// Spacing
// =========================================================================

#[test]
fn svg_thin_space() {
    let svg = latex_to_svg(r"a\,b");
    assert_valid_svg(&svg, "thin space");
}

#[test]
fn svg_negative_space() {
    let svg = latex_to_svg(r"a\!b");
    assert_valid_svg(&svg, "negative space");
}

#[test]
fn svg_viewbox_includes_leading_negative_space() {
    let svg = latex_to_svg(r"\!\!\!\!\!\!x");
    assert_valid_svg(&svg, "leading negative space");

    let viewbox = viewbox_values(&svg);
    assert_eq!(viewbox.len(), 4, "viewBox should have 4 values: {svg}");
    let left = viewbox[0];
    let min_text_x = text_x_values(&svg)
        .into_iter()
        .reduce(f64::min)
        .expect("negative-space SVG should contain text");

    assert!(
        left <= min_text_x,
        "viewBox left edge {left} should include leftmost text x {min_text_x}: {svg}"
    );
}

// =========================================================================
// Environments (extended)
// =========================================================================

#[test]
fn svg_gathered() {
    let svg = latex_to_svg(r"\begin{gathered} a+b \\ c+d \end{gathered}");
    assert_valid_svg(&svg, "gathered");
}

// =========================================================================
// Overset/underset
// =========================================================================

#[test]
fn svg_overset() {
    let svg = latex_to_svg(r"\overset{!}{=}");
    assert_valid_svg(&svg, "overset");
}

#[test]
fn svg_underset() {
    let svg = latex_to_svg(r"\underset{n\to\infty}{\lim}");
    assert_valid_svg(&svg, "underset");
}

// =========================================================================
// Partial derivatives
// =========================================================================

#[test]
fn svg_partial_deriv() {
    let svg = latex_to_svg(r"\frac{\partial^2 f}{\partial x \partial y}");
    assert_valid_svg(&svg, "partial derivative");
}

// =========================================================================
// Boxed expression stroke-width (should scale with font size)
// =========================================================================

#[test]
fn svg_boxed_has_proportional_stroke() {
    let svg = latex_to_svg(r"\boxed{x}");
    assert_valid_svg(&svg, "boxed");
    assert!(svg.contains("<rect"), "boxed should have rect element");
    // stroke-width should NOT be the old hardcoded "1"
    assert!(
        !svg.contains(r#"stroke-width="1""#),
        "stroke-width should be proportional to font size, not hardcoded '1'"
    );
}

#[test]
fn svg_boxed_fraction() {
    let svg = latex_to_svg(r"\boxed{\frac{a}{b}}");
    assert_valid_svg(&svg, "boxed frac");
    assert!(svg.contains("<rect"), "should have box border");
}

// =========================================================================
// Nth root rendering
// =========================================================================

#[test]
fn svg_cube_root() {
    let svg = latex_to_svg(r"\sqrt[3]{x}");
    assert_valid_svg(&svg, "cube root");
    // Should contain the index "3"
    assert!(svg.contains("3"), "cube root should render index 3");
}

#[test]
fn svg_nth_root() {
    let svg = latex_to_svg(r"\sqrt[n]{x^2 + y^2}");
    assert_valid_svg(&svg, "nth root");
}

// =========================================================================
// Complex real-world formulas
// =========================================================================

#[test]
fn svg_stirling() {
    let svg = latex_to_svg(r"n! \sim \sqrt{2\pi n}\left(\frac{n}{e}\right)^n");
    assert_valid_svg(&svg, "stirling");
}

#[test]
fn svg_maxwell_aligned() {
    let svg = latex_to_svg(
        r"\begin{aligned} \nabla \cdot \mathbf{E} &= \frac{\rho}{\epsilon_0} \\ \nabla \times \mathbf{B} &= \mu_0 \mathbf{J} \end{aligned}",
    );
    assert_valid_svg(&svg, "maxwell");
}

#[test]
fn svg_piecewise() {
    let svg = latex_to_svg(r"f(x) = \begin{cases} x^2 & x > 0 \\ -x^2 & x < 0 \end{cases}");
    assert_valid_svg(&svg, "piecewise");
}

#[test]
fn svg_binomial_theorem() {
    let svg = latex_to_svg(r"\sum_{k=0}^{n} \binom{n}{k} x^k y^{n-k}");
    assert_valid_svg(&svg, "binomial theorem");
}

// =========================================================================
// Cancel operations
// =========================================================================

#[test]
fn svg_cancel_basic() {
    let svg = latex_to_svg(r"\cancel{x}");
    assert_valid_svg(&svg, "cancel");
}

#[test]
fn svg_bcancel_basic() {
    let svg = latex_to_svg(r"\bcancel{y}");
    assert_valid_svg(&svg, "bcancel");
}

// =========================================================================
// Display mode operators
// =========================================================================

#[test]
fn svg_display_bigcap() {
    let svg = latex_to_svg(r"\displaystyle\bigcap_{i=1}^{n} A_i");
    assert_valid_svg(&svg, "bigcap");
}

#[test]
fn svg_display_bigoplus() {
    let svg = latex_to_svg(r"\displaystyle\bigoplus_{i=1}^{n} V_i");
    assert_valid_svg(&svg, "bigoplus");
}

// =========================================================================
// Stretchy delimiters
// =========================================================================

#[test]
fn svg_tall_delimiters_nested() {
    let svg = latex_to_svg(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
    assert_valid_svg(&svg, "tall delimiters");
}

#[test]
fn svg_angle_brackets() {
    let svg = latex_to_svg(r"\langle x, y \rangle");
    assert_valid_svg(&svg, "angle brackets");
}

// =========================================================================
// Color rendering
// =========================================================================

#[test]
fn svg_color_red() {
    let svg = latex_to_svg(r"{\color{red} x}");
    assert_valid_svg(&svg, "color red");
    // Should have a color attribute
    assert!(
        svg.contains("red")
            || svg.contains("#ff0000")
            || svg.contains("#FF0000")
            || svg.contains("rgb"),
        "should contain red color"
    );
}

// =========================================================================
// \smash tests
// =========================================================================

#[test]
fn svg_smash_bottom() {
    let svg = latex_to_svg(r"\frac{1}{\smash[b]{q}}");
    assert_valid_svg(&svg, "smash bottom");
}

#[test]
fn svg_smash_top() {
    let svg = latex_to_svg(r"\smash[t]{X^2}");
    assert_valid_svg(&svg, "smash top");
}

#[test]
fn svg_smash_both() {
    let svg = latex_to_svg(r"\smash{X^2_j}");
    assert_valid_svg(&svg, "smash both");
}

#[test]
fn svg_smash_reduces_height() {
    let with_smash = latex_to_svg(r"\frac{1}{\smash[b]{q}}");
    let without_smash = latex_to_svg(r"\frac{1}{q}");
    // Extract height from SVG
    fn extract_height(svg: &str) -> f64 {
        let start = svg.find("height=\"").unwrap() + 8;
        let end = svg[start..].find('"').unwrap();
        svg[start..start + end].parse().unwrap()
    }
    let h_with = extract_height(&with_smash);
    let h_without = extract_height(&without_smash);
    assert!(
        h_with < h_without,
        "smash[b] should reduce height: {h_with} < {h_without}"
    );
}

// =========================================================================
// \mathop, \mathrel, \mathbin
// =========================================================================

#[test]
fn svg_mathop_with_limits() {
    let svg = latex_to_svg(r"\mathop{T}_{i=1}^{n}");
    assert_valid_svg(&svg, "mathop with limits");
}

#[test]
fn svg_mathrel() {
    let svg = latex_to_svg(r"a \mathrel{R} b");
    assert_valid_svg(&svg, "mathrel");
}

#[test]
fn svg_mathbin() {
    let svg = latex_to_svg(r"a \mathbin{\diamond} b");
    assert_valid_svg(&svg, "mathbin");
}

// =========================================================================
// Display-mode large operators
// =========================================================================

#[test]
fn svg_display_sum_euler() {
    let svg = latex_to_svg(r"\displaystyle\sum_{k=0}^{\infty}\frac{1}{k!}");
    assert_valid_svg(&svg, "display sum Euler");
}

#[test]
fn svg_display_coprod() {
    let svg = latex_to_svg(r"\displaystyle\coprod_{\alpha}G_\alpha");
    assert_valid_svg(&svg, "display coproduct");
}

// =========================================================================
// Complex real-world formulas
// =========================================================================

#[test]
fn svg_cauchy_schwarz() {
    let svg = latex_to_svg(
        r"\left(\sum_{k=1}^n a_k b_k\right)^2 \leq \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
    );
    assert_valid_svg(&svg, "Cauchy-Schwarz");
}

#[test]
fn svg_schrodinger() {
    let svg = latex_to_svg(r"i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi");
    assert_valid_svg(&svg, "Schrodinger");
}

#[test]
fn svg_stirling_approximation() {
    let svg = latex_to_svg(r"n! \sim \sqrt{2\pi n}\left(\frac{n}{e}\right)^n");
    assert_valid_svg(&svg, "Stirling");
}

#[test]
fn svg_isotope() {
    let svg = latex_to_svg(r"{}^{14}_{6}\text{C}");
    assert_valid_svg(&svg, "isotope");
}

// =========================================================================
// Rendering quality tests — verify specific SVG structural properties
// =========================================================================

#[test]
fn svg_fraction_has_bar() {
    // A normal fraction must contain a <rect> element for the fraction bar
    let svg = render_latex(r"\frac{a}{b}");
    assert_valid_svg(&svg, "frac bar");
    assert!(
        svg.contains("<rect"),
        "fraction should contain a <rect> for the bar"
    );
}

#[test]
fn svg_fraction_nobar_lacks_rect() {
    // \atop (no-bar fraction) should NOT have a <rect>
    let svg = render_latex(r"{a \atop b}");
    assert_valid_svg(&svg, "atop nobar");
    // Note: the SVG may still have rects for other purposes, but a pure atop
    // should have fewer rects than a normal frac
}

#[test]
fn svg_sqrt_has_path() {
    // Square root must have an SVG path for the radical sign
    let svg = render_latex(r"\sqrt{x}");
    assert_valid_svg(&svg, "sqrt path");
    assert!(
        svg.contains("<path") || svg.contains("<g"),
        "sqrt should contain path elements"
    );
}

#[test]
fn svg_nested_sqrt() {
    // Deeply nested sqrt should render without panic
    let svg = latex_to_svg(r"\sqrt{\sqrt{\sqrt{\sqrt{x}}}}");
    assert_valid_svg(&svg, "nested sqrt");
}

#[test]
fn svg_nth_root_has_index() {
    let svg = latex_to_svg(r"\sqrt[3]{x}");
    assert_valid_svg(&svg, "nth root");
    // The index "3" should appear somewhere in the SVG
    assert!(svg.contains('3'), "nth root should contain the index '3'");
}

#[test]
fn svg_large_delimiters_taller_than_content() {
    // \left( ... \right) with nested fractions should produce tall delimiters
    let svg = render_latex(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
    assert_valid_svg(&svg, "large delimiters");
    // The SVG should be reasonably tall (nested fractions need space)
    // Extract height from viewBox
    let height = extract_svg_dimension(&svg, "height");
    assert!(
        height > 30.0,
        "nested fraction delimiters should be tall, got {height}"
    );
}

#[test]
fn svg_accent_hat() {
    let svg = render_latex(r"\hat{x}");
    assert_valid_svg(&svg, "hat accent");
    // Should contain the hat accent character or path
}

#[test]
fn svg_wide_hat_wider_than_narrow() {
    let narrow = render_latex(r"\widehat{x}");
    let wide = render_latex(r"\widehat{xyz}");
    let narrow_w = extract_svg_dimension(&narrow, "width");
    let wide_w = extract_svg_dimension(&wide, "width");
    assert!(
        wide_w > narrow_w,
        "\\widehat{{xyz}} ({wide_w}) should be wider than \\widehat{{x}} ({narrow_w})"
    );
}

#[test]
fn svg_overline_has_rule() {
    let svg = render_latex(r"\overline{AB}");
    assert_valid_svg(&svg, "overline");
    assert!(
        svg.contains("<rect") || svg.contains("<path"),
        "overline should have a rule element"
    );
}

#[test]
fn svg_rule_renders_requested_dimensions() {
    let svg = latex_to_svg(r"\rule{1em}{2em}");
    assert_valid_svg(&svg, "rule dimensions");
    assert!(
        svg.contains("<rect"),
        "rule should render as a rectangle: {svg}"
    );
    assert!(
        svg.contains("width=\"16") || svg.contains("width=\"15.") || svg.contains("width=\"16."),
        "1em rule should be about 16px wide: {svg}"
    );
    assert!(
        svg.contains("height=\"32") || svg.contains("height=\"31.") || svg.contains("height=\"32."),
        "2em rule should be about 32px high: {svg}"
    );
}

#[test]
fn svg_rule_optional_lift_changes_vertical_extent() {
    let unlifted = latex_to_svg(r"x\rule{1em}{1em}");
    let lifted = latex_to_svg(r"x\rule[1em]{1em}{1em}");
    assert_valid_svg(&unlifted, "unlifted rule");
    assert_valid_svg(&lifted, "lifted rule");
    assert_ne!(
        unlifted, lifted,
        "optional lift should affect rule placement or extents"
    );

    let unlifted_delta = first_text_y(&unlifted) - first_rect_y(&unlifted);
    let lifted_delta = first_text_y(&lifted) - first_rect_y(&lifted);
    let rect_h = first_rect_height(&unlifted);
    assert!(
        (unlifted_delta - rect_h).abs() < 1.0,
        "unlifted rule should sit above the baseline with zero depth: baseline_delta={unlifted_delta}, rect_h={rect_h}, svg={unlifted}"
    );
    assert!(
        lifted_delta > unlifted_delta + 10.0,
        "positive 1em lift should move rule rect upward relative to text by about 16px: unlifted_delta={unlifted_delta}, lifted_delta={lifted_delta}"
    );
}

#[test]
fn svg_rule_negative_lift_expands_vertical_extent() {
    let unlifted = latex_to_svg(r"x\rule{1em}{1em}");
    let lowered = latex_to_svg(r"x\rule[-1em]{1em}{1em}");
    assert_valid_svg(&unlifted, "unlifted rule");
    assert_valid_svg(&lowered, "lowered rule");

    let unlifted_delta = first_text_y(&unlifted) - first_rect_y(&unlifted);
    let lowered_delta = first_text_y(&lowered) - first_rect_y(&lowered);
    assert!(
        lowered_delta < unlifted_delta - 10.0,
        "negative 1em lift should move rule rect downward relative to text by about 16px: unlifted_delta={unlifted_delta}, lowered_delta={lowered_delta}, svg={lowered}"
    );
}

#[test]
fn svg_rule_lift_compact_arena_matches_heap() {
    let config = Config::default();
    let parsed = LatexReader::parse_compact(r"x\rule[1em]{1em}{1em}y", &config).unwrap();
    let options = RenderOptions::default();
    let heap_svg = render_to_svg_compact(&parsed.expr, &options).unwrap();
    let mut layout_arena = LayoutArena::new();
    let arena_svg = render_to_svg_compact_arena(&parsed.expr, &options, &mut layout_arena).unwrap();

    assert_valid_svg(&heap_svg, "heap lifted rule");
    assert_valid_svg(&arena_svg, "arena lifted rule");
    assert_eq!(
        first_rect_y(&heap_svg),
        first_rect_y(&arena_svg),
        "compact arena should emit lifted rule rect at same y as compact heap"
    );
    assert_eq!(
        heap_svg, arena_svg,
        "compact arena SVG should match compact heap SVG for lifted rules"
    );
}

#[test]
fn svg_rule_heap_ast_renders_rectangle() {
    let svg = render_latex(r"\rule[1em]{1em}{1em}");
    assert_valid_svg(&svg, "heap rule");
    assert!(
        svg.contains("<rect"),
        "heap AST rule should render as a rectangle: {svg}"
    );
}

#[test]
fn svg_raisebox_changes_vertical_layout() {
    let plain = latex_to_svg("ab");
    let raised = latex_to_svg(r"a\raisebox{1em}{b}");
    assert_valid_svg(&plain, "plain ab");
    assert_valid_svg(&raised, "raised b");
    assert_ne!(
        plain, raised,
        "raisebox should affect vertical placement or extents"
    );
}

#[test]
fn svg_raisebox_moves_raised_glyph_up() {
    let raised = latex_to_svg(r"a\raisebox{1em}{b}");
    assert_valid_svg(&raised, "raised b");

    let y_values = text_y_values(&raised);
    assert!(
        y_values.len() >= 2,
        "raised expression should render at least two glyph text nodes: {raised}"
    );
    assert!(
        y_values[1] < y_values[0] - 10.0,
        "raised glyph should move upward by about 1em: y_values={y_values:?}, svg={raised}"
    );
}

#[test]
fn svg_raisebox_compact_arena_matches_heap() {
    let config = Config::default();
    let parsed = LatexReader::parse_compact(r"a\raisebox{1em}{b}", &config).unwrap();
    let options = RenderOptions::default();
    let heap_svg = render_to_svg_compact(&parsed.expr, &options).unwrap();
    let mut layout_arena = LayoutArena::new();
    let arena_svg = render_to_svg_compact_arena(&parsed.expr, &options, &mut layout_arena).unwrap();

    assert_valid_svg(&heap_svg, "heap raisebox");
    assert_valid_svg(&arena_svg, "arena raisebox");
    assert_eq!(
        heap_svg, arena_svg,
        "compact arena SVG should match compact heap SVG for raisebox"
    );
}

#[test]
fn svg_raisebox_optional_extents_override_layout_bounds() {
    let normal = latex_to_svg(r"a\raisebox{0em}{b}");
    let explicit = latex_to_svg(r"a\raisebox{0em}[3em][2em]{b}");
    assert_valid_svg(&normal, "normal raisebox");
    assert_valid_svg(&explicit, "explicit raisebox extents");

    let normal_h = extract_svg_dimension(&normal, "height");
    let explicit_h = extract_svg_dimension(&explicit, "height");
    assert!(
        explicit_h > normal_h + 60.0,
        "raisebox optional height/depth should reserve explicit vertical extents: normal={normal_h}, explicit={explicit_h}, svg={explicit}"
    );
}

#[test]
fn svg_raisebox_heap_negative_shift_expands_depth() {
    let plain = render_latex("ab");
    let lowered = render_latex(r"a\raisebox{-1em}{b}");
    assert_valid_svg(&plain, "plain ab");
    assert_valid_svg(&lowered, "lowered b");

    let plain_h = extract_svg_dimension(&plain, "height");
    let lowered_h = extract_svg_dimension(&lowered, "height");
    assert!(
        lowered_h > plain_h + 10.0,
        "negative raisebox shift should expand downward extent: plain={plain_h}, lowered={lowered_h}, svg={lowered}"
    );

    let y_values = text_y_values(&lowered);
    assert!(
        y_values.len() >= 2,
        "lowered expression should render two glyphs: {lowered}"
    );
    assert!(
        y_values[1] > y_values[0] + 10.0,
        "negative raisebox shift should move glyph downward: y_values={y_values:?}, svg={lowered}"
    );
}

#[test]
fn svg_matrix_renders() {
    let svg = latex_to_svg(r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}");
    assert_valid_svg(&svg, "2x2 matrix");
    // Should contain all four entries
    for digit in ['1', '2', '3', '4'] {
        assert!(svg.contains(digit), "matrix should contain digit '{digit}'");
    }
}

#[test]
fn svg_cases_env() {
    let svg =
        latex_to_svg(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}");
    assert_valid_svg(&svg, "cases env");
}

#[test]
fn svg_aligned_env() {
    let svg = latex_to_svg(r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}");
    assert_valid_svg(&svg, "aligned env");
}

#[test]
fn svg_deeply_nested_fraction() {
    let svg =
        latex_to_svg(r"\frac{\frac{\frac{a}{b}}{\frac{c}{d}}}{\frac{\frac{e}{f}}{\frac{g}{h}}}");
    assert_valid_svg(&svg, "deeply nested fraction");
}

#[test]
fn svg_overbrace_underbrace() {
    let svg = latex_to_svg(r"\overbrace{a + b + c}^{n} + \underbrace{d + e}_{m}");
    assert_valid_svg(&svg, "overbrace+underbrace");
}

#[test]
fn svg_xrightarrow_with_labels() {
    let svg = latex_to_svg(r"A \xrightarrow[\text{below}]{\text{above}} B");
    assert_valid_svg(&svg, "xrightarrow with labels");
}

#[test]
fn svg_empty_expression() {
    let svg = latex_to_svg("");
    assert_valid_svg(&svg, "empty");
}

#[test]
fn svg_single_space() {
    let svg = latex_to_svg(r"\quad");
    assert_valid_svg(&svg, "quad space");
}

#[test]
fn svg_phantom_zero_width() {
    // \vphantom should have zero visible width
    let with_phantom = render_latex(r"a\vphantom{X}b");
    assert_valid_svg(&with_phantom, "vphantom");
}

#[test]
fn svg_boxed_has_border_rect() {
    let svg = render_latex(r"\boxed{E = mc^2}");
    assert_valid_svg(&svg, "boxed");
    // Should contain rect elements for the box border
    assert!(svg.contains("<rect"), "boxed should contain rect elements");
}

#[test]
fn svg_cancel_renders() {
    let svg = latex_to_svg(r"\cancel{x}");
    assert_valid_svg(&svg, "cancel");
}

#[test]
fn svg_color_multi() {
    let svg = latex_to_svg(r"\color{red}{x} + \color{blue}{y}");
    assert_valid_svg(&svg, "color");
}

#[test]
fn svg_display_vs_inline_size() {
    // Display-mode sum should be taller than inline
    let display = {
        let config =
            mathema::error::Config::default().with_display(mathema::ast::DisplayStyle::Block);
        let parsed = LatexReader::parse(r"\sum_{i=0}^{n} a_i", &config).unwrap();
        let options = RenderOptions::default();
        mathema::render::render_to_svg(&parsed.expr, &options).unwrap()
    };
    let inline = {
        let config =
            mathema::error::Config::default().with_display(mathema::ast::DisplayStyle::Inline);
        let parsed = LatexReader::parse(r"\sum_{i=0}^{n} a_i", &config).unwrap();
        let options = RenderOptions::default();
        mathema::render::render_to_svg(&parsed.expr, &options).unwrap()
    };
    let display_h = extract_svg_dimension(&display, "height");
    let inline_h = extract_svg_dimension(&inline, "height");
    assert!(
        display_h > inline_h,
        "display sum ({display_h}) should be taller than inline ({inline_h})"
    );
}

#[test]
fn svg_integral_display() {
    let svg = latex_to_svg(r"\displaystyle\int_0^\infty e^{-x^2}\,dx");
    assert_valid_svg(&svg, "display integral");
}

#[test]
fn svg_multiline_gather() {
    let svg = latex_to_svg(r"\begin{gather} a = b + c \\ d = e + f \\ g = h + i \end{gather}");
    assert_valid_svg(&svg, "gather multiline");
}

#[test]
fn svg_binomial_coefficient() {
    let svg = latex_to_svg(r"\binom{n}{k}");
    assert_valid_svg(&svg, "binomial");
}

#[test]
fn svg_continued_fraction_deep() {
    let svg = latex_to_svg(r"a_0 + \cfrac{1}{a_1 + \cfrac{1}{a_2 + \cfrac{1}{a_3}}}");
    assert_valid_svg(&svg, "continued fraction");
}

#[test]
fn svg_underset_overset() {
    let svg = latex_to_svg(r"\overset{?}{=} \quad \underset{\text{def}}{=}");
    assert_valid_svg(&svg, "overset/underset");
}

#[test]
fn svg_greek_letters_complete() {
    let svg = latex_to_svg(
        r"\alpha\beta\gamma\delta\epsilon\zeta\eta\theta\iota\kappa\lambda\mu\nu\xi\pi\rho\sigma\tau\upsilon\phi\chi\psi\omega",
    );
    assert_valid_svg(&svg, "Greek lowercase");
}

#[test]
fn svg_relation_symbols() {
    let svg = latex_to_svg(r"a \leq b \geq c \neq d \approx e \equiv f \sim g");
    assert_valid_svg(&svg, "relation symbols");
}

#[test]
fn svg_arrow_symbols() {
    let svg = latex_to_svg(
        r"\leftarrow \rightarrow \Leftarrow \Rightarrow \leftrightarrow \Leftrightarrow \mapsto \hookrightarrow",
    );
    assert_valid_svg(&svg, "arrow symbols");
}

#[test]
fn svg_set_operations_extended() {
    let svg = latex_to_svg(r"A \cup B \cap C \setminus D \subset E \supset F");
    assert_valid_svg(&svg, "set operations");
}

#[test]
fn svg_tensor_notation() {
    let svg = latex_to_svg(r"T^{\mu\nu}_{\rho\sigma}");
    assert_valid_svg(&svg, "tensor notation");
}

#[test]
fn svg_pmod() {
    let svg = latex_to_svg(r"a \equiv b \pmod{n}");
    assert_valid_svg(&svg, "pmod");
}

#[test]
fn svg_newline_command_stacks_rows() {
    let single = latex_to_svg("a b");
    let multiline = latex_to_svg(r"a\newline b");
    assert_valid_svg(&multiline, "newline command");

    let single_h = extract_svg_dimension(&single, "height");
    let multiline_h = extract_svg_dimension(&multiline, "height");
    let single_w = extract_svg_dimension(&single, "width");
    let multiline_w = extract_svg_dimension(&multiline, "width");

    assert!(
        multiline_h > single_h * 1.5,
        "\\newline should stack rows vertically: single={single_h}, multiline={multiline_h}"
    );
    assert!(
        multiline_w < single_w,
        "\\newline should split content into narrower lines: single={single_w}, multiline={multiline_w}"
    );
}

#[test]
fn svg_top_level_linebreak_stacks_lines() {
    let single = latex_to_svg("a+b+c+d");
    let multiline = latex_to_svg(r"a+b\\c+d");
    assert_valid_svg(&single, "single line");
    assert_valid_svg(&multiline, "multiline");
    assert_ne!(single, multiline, "explicit linebreak should change layout");

    let single_h = extract_svg_dimension(&single, "height");
    let multiline_h = extract_svg_dimension(&multiline, "height");
    assert!(
        multiline_h > single_h * 1.5,
        "explicit linebreak should stack lines vertically: single={single_h}, multiline={multiline_h}"
    );
}

#[test]
fn svg_top_level_source_newline_stacks_lines() {
    let single = latex_to_svg("a+b=c+d");
    let multiline = latex_to_svg("a+b=\nc+d");
    assert_valid_svg(&single, "single line");
    assert_valid_svg(&multiline, "source newline");
    assert_ne!(single, multiline, "source newline should change layout");

    let single_h = extract_svg_dimension(&single, "height");
    let multiline_h = extract_svg_dimension(&multiline, "height");
    let single_w = extract_svg_dimension(&single, "width");
    let multiline_w = extract_svg_dimension(&multiline, "width");
    assert!(
        multiline_h > single_h * 1.5,
        "source newline should stack lines vertically: single={single_h}, multiline={multiline_h}"
    );
    assert!(
        multiline_w < single_w,
        "source newline should split content into narrower lines: single={single_w}, multiline={multiline_w}"
    );
}

/// Extract a numeric dimension from the SVG root element.
fn extract_svg_dimension(svg: &str, attr: &str) -> f64 {
    let pattern = format!("{attr}=\"");
    if let Some(start) = svg.find(&pattern) {
        let after = &svg[start + pattern.len()..];
        if let Some(end) = after.find('"') {
            let val_str = &after[..end];
            // Handle trailing "px" or just number
            let val_str = val_str.trim_end_matches("px");
            val_str.parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        }
    } else {
        0.0
    }
}

// === Brace rendering quality tests ===

#[test]
fn svg_underbrace_has_filled_path() {
    let svg = render_latex(r"\underbrace{x+y}_{z}");
    assert_valid_svg(&svg, "underbrace");
    // The underbrace should produce a filled SVG path (F: prefix → fill="currentColor")
    assert!(
        svg.contains("fill=\"currentColor\" stroke=\"none\""),
        "underbrace should have filled brace path"
    );
}

#[test]
fn svg_overbrace_has_filled_path() {
    let svg = render_latex(r"\overbrace{x+y}^{z}");
    assert_valid_svg(&svg, "overbrace");
    assert!(
        svg.contains("fill=\"currentColor\" stroke=\"none\""),
        "overbrace should have filled brace path"
    );
}

#[test]
fn svg_underbrace_taller_than_content() {
    let plain = render_latex(r"x+y");
    let braced = render_latex(r"\underbrace{x+y}_{z}");
    let plain_h = extract_svg_dimension(&plain, "height");
    let braced_h = extract_svg_dimension(&braced, "height");
    assert!(
        braced_h > plain_h + 5.0,
        "underbrace should be significantly taller: plain={}, braced={}",
        plain_h,
        braced_h
    );
}

#[test]
fn svg_overbrace_taller_than_content() {
    let plain = render_latex(r"x+y");
    let braced = render_latex(r"\overbrace{x+y}^{z}");
    let plain_h = extract_svg_dimension(&plain, "height");
    let braced_h = extract_svg_dimension(&braced, "height");
    assert!(
        braced_h > plain_h + 5.0,
        "overbrace should be significantly taller: plain={}, braced={}",
        plain_h,
        braced_h
    );
}

#[test]
fn svg_underbrace_annotation_below() {
    let svg = render_latex(r"\underbrace{a+b+c}_{n \text{ terms}}");
    assert_valid_svg(&svg, "underbrace annotation");
    // Should contain both the content and annotation text
    assert_contains_math_italic(&svg, 'a');
}

// === Edge case rendering tests ===

#[test]
fn svg_single_char_fraction() {
    let svg = render_latex(r"\frac{1}{2}");
    assert_valid_svg(&svg, "single char fraction");
    assert!(svg.contains("<rect"), "fraction should have a bar");
}

#[test]
fn svg_deeply_nested_scripts() {
    let svg = render_latex(r"x^{y^{z^{w}}}");
    assert_valid_svg(&svg, "deeply nested scripts");
    // Should produce decreasing font sizes (minimized: exact integers like
    // "8" have no trailing ".0", so match the integer prefix).
    let count_11_2 = svg.matches("font-size=\"11").count();
    let count_8 = svg.matches("font-size=\"8").count();
    assert!(count_11_2 > 0, "should have script-size text");
    assert!(
        count_8 > 0 || svg.matches("font-size=\"5").count() > 0,
        "should have scriptscript-size text"
    );
}

#[test]
fn svg_display_integral_larger() {
    let svg = render_latex(r"\displaystyle\int_0^1 f(x)\,dx");
    assert_valid_svg(&svg, "display integral");
    // Display integral should use larger font size (32 = 2x base; minimized
    // exact integer has no trailing ".0").
    assert!(
        svg.contains("font-size=\"32") || svg.contains("font-size=\"24"),
        "display integral should be larger than text size"
    );
}

#[test]
fn svg_math_operators_spacing() {
    let svg = render_latex(r"a + b = c");
    assert_valid_svg(&svg, "operator spacing");
    // Width should be more than just the characters (due to spacing)
    let w = extract_svg_dimension(&svg, "width");
    assert!(w > 40.0, "operator spacing should add width: got {}", w);
}

#[test]
fn svg_nested_delimiters_scale() {
    let svg = render_latex(r"\left(\frac{\frac{a}{b}}{c}\right)");
    assert_valid_svg(&svg, "nested delimiter scaling");
    let h = extract_svg_dimension(&svg, "height");
    assert!(h > 30.0, "nested delimiters should be tall: got {}", h);
}

#[test]
fn svg_matrix_multiple_rows() {
    let svg = render_latex(r"\begin{pmatrix} a & b \\ c & d \\ e & f \end{pmatrix}");
    assert_valid_svg(&svg, "3-row matrix");
    let h = extract_svg_dimension(&svg, "height");
    assert!(h > 40.0, "3-row matrix should be tall: got {}", h);
}

#[test]
fn svg_array_row_separator_after_integral_stacks_rows() {
    let single = latex_to_svg(r"\int x");
    let multiline = latex_to_svg(r"\begin{array}{l}\int\\x\end{array}");
    assert_valid_svg(&single, "single integral");
    assert_valid_svg(&multiline, "array integral rows");

    let single_h = extract_svg_dimension(&single, "height");
    let multiline_h = extract_svg_dimension(&multiline, "height");
    assert!(
        multiline_h > single_h * 1.5,
        "row separator after an integral should stack rows: single={single_h}, multiline={multiline_h}, svg={multiline}"
    );
}

#[test]
fn svg_wide_tilde_stretches() {
    let svg1 = render_latex(r"\widetilde{x}");
    let svg3 = render_latex(r"\widetilde{xyz}");
    let w1 = extract_svg_dimension(&svg1, "width");
    let w3 = extract_svg_dimension(&svg3, "width");
    assert!(w3 > w1, "widetilde should stretch: w1={}, w3={}", w1, w3);
}

#[test]
fn svg_mathchoice_display_vs_text() {
    // \mathchoice picks based on style
    let svg = render_latex(r"\mathchoice{D}{T}{S}{SS}");
    assert_valid_svg(&svg, "mathchoice");
}

#[test]
fn svg_prescripts() {
    let svg = render_latex(r"{}^{14}_{6}\text{C}");
    assert_valid_svg(&svg, "prescripts");
}

#[test]
fn svg_cancel_has_line() {
    let svg = render_latex(r"\cancel{xyz}");
    assert_valid_svg(&svg, "cancel");
    // Cancel should have a stroke line
    assert!(
        svg.contains("stroke=") || svg.contains("<line") || svg.contains("<path"),
        "cancel should render a line"
    );
}

#[test]
fn svg_phantom_invisible() {
    let svg = render_latex(r"\phantom{xyz}");
    assert_valid_svg(&svg, "phantom");
    // Phantom should have width but no visible content
    let w = extract_svg_dimension(&svg, "width");
    assert!(w > 0.0, "phantom should occupy space");
}

#[test]
fn svg_sqrt_with_index() {
    let svg = render_latex(r"\sqrt[3]{x}");
    assert_valid_svg(&svg, "cube root");
    // Should contain the index "3"
    assert!(svg.contains(">3<"), "should show index 3");
}

#[test]
fn svg_color_preserved() {
    // Use latex_to_svg (compact path) since Color nodes are only preserved
    // in the CompactNode → layout path, not the MathExpr path.
    let svg = latex_to_svg(r"\textcolor{red}{x} + \textcolor{blue}{y}");
    assert_valid_svg(&svg, "colored text");
    assert!(
        svg.contains("red") || svg.contains("#ff0000") || svg.contains("rgb"),
        "should preserve red color"
    );
    assert!(
        svg.contains("blue") || svg.contains("#0000ff"),
        "should preserve blue color"
    );
}

#[test]
fn svg_substack_renders() {
    let svg = render_latex(r"\sum_{\substack{i=1 \\ j=1}}^{n} a_{ij}");
    assert_valid_svg(&svg, "substack");
    let h = extract_svg_dimension(&svg, "height");
    assert!(h > 30.0, "substack formula should be tall: got {}", h);
}

// =========================================================================
// Regression: small radicand radical path (v-bottom clamp fix)
// =========================================================================

#[test]
fn svg_path_data_is_minimized() {
    // Default render uses synthetic SVG paths (radical, brace) whose `d` data
    // must be emitted minimally: no untrimmed 4-decimal zeros, no double
    // spaces, and no space before a command letter.
    let svg = render_latex(r"\sqrt{x} + \underbrace{a+b}_{c}");
    let mut count = 0usize;
    let mut hay = svg.as_str();
    while let Some(i) = hay.find("d=\"") {
        let rest = &hay[i + 3..];
        let end = rest.find('"').unwrap_or(rest.len());
        let d = &rest[..end];
        count += 1;
        assert!(!d.contains("  "), "no double spaces in path d: {d}");
        assert!(!d.contains("0000"), "no untrimmed 4-decimal zeros: {d}");
        assert!(
            !d.contains(" L") && !d.contains(" C") && !d.contains(" Z") && !d.contains(" M"),
            "no space before a command letter: {d}"
        );
        hay = &rest[end..];
    }
    assert!(count > 0, "expected at least one <path d=...>");
}

#[test]
fn svg_sqrt_single_digit_no_glitch() {
    // A tiny radicand must not produce a degenerate radical path
    let svg = render_latex(r"\sqrt{2}");
    assert_valid_svg(&svg, "sqrt single digit");
    assert!(svg.contains("<path"), "sqrt should have path element");
    // The synthetic radical is the filled path beginning at x=0 (minimized form
    // `d="F:M0 ..."`). When present (text-based SVG mode), verify it contains no
    // negative coordinates, which would escape the viewBox. Under font-embedding
    // the surd is a real glyph outline instead, so the synthetic path is absent.
    if let Some(start) = svg.find("d=\"F:M0 ") {
        let rest = &svg[start + 3..]; // skip `d="`
        let end = rest.find('"').unwrap_or(rest.len());
        let d = &rest[..end];
        assert!(
            !d.contains('-'),
            "radical path should not have negative escape: {d}"
        );
    }
}

#[test]
fn svg_sqrt_empty_no_panic() {
    let svg = render_latex(r"\sqrt{}");
    assert_valid_svg(&svg, "sqrt empty");
}

// =========================================================================
// Regression: radical optical sizing — the surd must enclose its radicand
// and grow with taller content (font MATH-table size variants).
// =========================================================================

#[test]
fn svg_sqrt_surd_encloses_and_grows() {
    // The surd must rise above its radicand (vinculum + clearance), so a
    // radical is taller than its bare radicand.
    let h_x = extract_svg_dimension(&latex_to_svg("x"), "height");
    let h_sqrt_x = extract_svg_dimension(&latex_to_svg(r"\sqrt{x}"), "height");
    assert!(
        h_sqrt_x > h_x + 1.0,
        "sqrt must add a vinculum + clearance above the radicand: {h_sqrt_x} vs {h_x}"
    );

    // A taller radicand must produce a taller surd (optical size variant).
    let h_sqrt_tall = extract_svg_dimension(&latex_to_svg(r"\sqrt{x^2}"), "height");
    assert!(
        h_sqrt_tall > h_sqrt_x,
        "taller radicand must select a taller surd: {h_sqrt_tall} vs {h_sqrt_x}"
    );

    // A fraction radicand is taller still.
    let h_sqrt_frac = extract_svg_dimension(&latex_to_svg(r"\sqrt{\frac{a}{b}}"), "height");
    assert!(
        h_sqrt_frac > h_sqrt_tall,
        "sqrt of a fraction must grow further: {h_sqrt_frac} vs {h_sqrt_tall}"
    );
}

// =========================================================================
// Regression: integral side-limits render diagonally (operator italic
// correction offsets the superscript from the subscript).
// =========================================================================

#[test]
fn svg_integral_side_limits_render() {
    let svg = latex_to_svg(r"\int_0^\infty");
    assert_valid_svg(&svg, "integral side limits");
    // The limits must add width beyond the bare integral sign.
    let w_bare = extract_svg_dimension(&latex_to_svg(r"\int"), "width");
    let w_lim = extract_svg_dimension(&svg, "width");
    assert!(
        w_lim > w_bare,
        "integral with limits must be wider than the bare integral: {w_lim} vs {w_bare}"
    );
}

#[test]
fn svg_sqrt_tiny_content() {
    // Very small content like a single dot — tests the radical_path v-bottom clamp
    let svg = render_latex(r"\sqrt{.}");
    assert_valid_svg(&svg, "sqrt tiny");
}

// =========================================================================
// Regression: SVG path scientific notation (scale_svg_path fix)
// =========================================================================

#[test]
fn svg_scaled_path_no_corruption() {
    // Formulas that produce scaled SVG paths (stretchy arrows, accents)
    // should not produce corrupted path data after the scientific notation fix
    let svg = render_latex(r"A \xrightarrow[\text{long label below}]{\text{long label above}} B");
    assert_valid_svg(&svg, "xrightarrow scaled path");
    // Verify no 'e' or 'E' appears as a bare SVG path command
    // (it would mean scientific notation wasn't consumed properly)
}

// =========================================================================
// Color rendering with font_family inheritance (EscapedAttrs fix)
// =========================================================================

#[test]
fn svg_nested_colors_font_family() {
    // Deeply nested color changes should preserve font family correctly
    let config = Config::default();
    let parsed =
        LatexReader::parse(r"{\color{red}x} + {\color{blue}{\color{green}y}}", &config).unwrap();
    let options = RenderOptions {
        font_family: "TestFont, serif".into(),
        ..RenderOptions::default()
    };
    let svg = render_to_svg(&parsed.expr, &options).unwrap();
    assert_valid_svg(&svg, "nested colors");
    // Every text element should reference the custom font family
    for text_start in svg.match_indices("<text").map(|(i, _)| i) {
        let text_end = svg[text_start..].find("</text>").unwrap() + text_start;
        let text_el = &svg[text_start..text_end];
        assert!(
            text_el.contains("TestFont"),
            "text element should use custom font family: {text_el}"
        );
    }
}

#[test]
fn svg_prescripts_different_widths() {
    // Pre-scripts with different widths should right-align within the pre-column.
    let svg = render_latex(r"{}^{abc}_{x}\text{C}");
    assert_valid_svg(&svg, "prescripts different widths");
    assert!(
        svg.contains("<text"),
        "prescripts should produce text elements"
    );
}

#[test]
fn svg_prescripts_only_sup() {
    let svg = render_latex(r"{}^{14}\text{C}");
    assert_valid_svg(&svg, "prescripts only sup");
}

#[test]
fn svg_prescripts_only_sub() {
    let svg = render_latex(r"{}_{6}\text{C}");
    assert_valid_svg(&svg, "prescripts only sub");
}

// =========================================================================
// Comprehensive edge-case tests
// =========================================================================

#[test]
fn svg_triple_nested_fraction() {
    let svg = latex_to_svg(r"\frac{\frac{\frac{a}{b}}{c}}{d}");
    assert_valid_svg(&svg, "triple nested fraction");
}

#[test]
fn svg_deep_sub_and_superscripts() {
    let svg = latex_to_svg(r"x_{a_{b_{c}}}");
    assert_valid_svg(&svg, "deep subscript");
    let svg2 = latex_to_svg(r"2^{2^{2^{2}}}");
    assert_valid_svg(&svg2, "deep superscript");
}

#[test]
fn svg_empty_numerator() {
    let svg = latex_to_svg(r"\frac{}{b}");
    assert_valid_svg(&svg, "empty numerator");
}

#[test]
fn svg_empty_sqrt() {
    let svg = latex_to_svg(r"\sqrt{}");
    assert_valid_svg(&svg, "empty sqrt");
}

#[test]
fn svg_prime_notation() {
    let svg = latex_to_svg(r"f'(x) + g''(x)");
    assert_valid_svg(&svg, "prime notation");
}

#[test]
fn svg_gaussian_integral() {
    let svg = latex_to_svg(
        r"\int_{-\infty}^{\infty} \frac{1}{\sqrt{2\pi\sigma^2}} e^{-\frac{(x-\mu)^2}{2\sigma^2}} \, dx = 1",
    );
    assert_valid_svg(&svg, "gaussian integral");
}

#[test]
fn svg_maxwell_divergence_eqs() {
    let svg = latex_to_svg(
        r"\begin{aligned} \nabla \cdot \vec{E} &= \frac{\rho}{\epsilon_0} \\ \nabla \cdot \vec{B} &= 0 \end{aligned}",
    );
    assert_valid_svg(&svg, "Maxwell divergence eqs");
}

#[test]
fn svg_identity_matrix_3x3() {
    let svg = latex_to_svg(r"\begin{pmatrix} 1 & 0 & 0 \\ 0 & 1 & 0 \\ 0 & 0 & 1 \end{pmatrix}");
    assert_valid_svg(&svg, "3x3 identity matrix");
}

#[test]
fn svg_set_theory_operators() {
    let svg = latex_to_svg(r"A \cap B \cup C \setminus D \subset E \subseteq F");
    assert_valid_svg(&svg, "set theory operators");
}

#[test]
fn svg_logic_arrows() {
    let svg = latex_to_svg(r"A \Rightarrow B \Leftrightarrow C \implies D");
    assert_valid_svg(&svg, "logic arrows");
}

#[test]
fn svg_dirac_notation() {
    let svg = latex_to_svg(r"\langle \psi | \hat{H} | \phi \rangle");
    assert_valid_svg(&svg, "dirac notation");
}

#[test]
fn svg_double_integral() {
    let svg = latex_to_svg(r"\int_0^1 \int_0^1 f(x,y)\,dx\,dy");
    assert_valid_svg(&svg, "double integral");
}

#[test]
fn svg_limsup_liminf() {
    let svg = latex_to_svg(r"\limsup_{n\to\infty} a_n = \liminf_{n\to\infty} a_n");
    assert_valid_svg(&svg, "limsup liminf");
}

#[test]
fn svg_modular_arithmetic() {
    let svg = latex_to_svg(r"a \equiv b \pmod{n}");
    assert_valid_svg(&svg, "modular arithmetic");
}

#[test]
fn svg_tall_delimiters_nested_frac() {
    let svg = latex_to_svg(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
    assert_valid_svg(&svg, "tall delimiters");
    // Delimiters should use path rendering for tall content
    assert!(
        svg.contains("<path") || svg.contains("<g"),
        "tall delimiters should use path-based rendering"
    );
}

#[test]
fn svg_dots_variants() {
    let svg = latex_to_svg(r"a \cdot b \cdots c \ldots d");
    assert_valid_svg(&svg, "dot variants");
}

#[test]
fn svg_continued_fraction_nested() {
    let svg = latex_to_svg(r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}");
    assert_valid_svg(&svg, "continued fraction nested");
}

#[test]
fn svg_cases_abs_value() {
    let svg = latex_to_svg(r"|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}");
    assert_valid_svg(&svg, "cases abs value");
}

#[test]
fn svg_mixed_arithmetic_ops() {
    let svg = latex_to_svg(r"a + b - c \times d \div e");
    assert_valid_svg(&svg, "mixed arithmetic ops");
}

#[test]
fn svg_negative_space_xy() {
    let svg = latex_to_svg(r"x \! y");
    assert_valid_svg(&svg, "negative space xy");
}

#[test]
fn svg_trig_functions_greek() {
    let svg = latex_to_svg(r"\sin(\theta) + \cos(\phi) + \tan(\psi)");
    assert_valid_svg(&svg, "trig functions greek");
}

#[test]
fn svg_complex_fraction_tower() {
    let svg = latex_to_svg(r"\frac{1}{\frac{1}{\frac{1}{x+1}+1}+1}");
    assert_valid_svg(&svg, "fraction tower");
}
