#![cfg(feature = "raster")]
//! Integration tests for raster image rendering (PNG/JPEG).
//!
//! Run with: `cargo test --features raster --test render_raster`

use mathema::{Format, ImageFormat, RasterOptions, from_latex};

#[test]
fn latex_to_png() {
    let result = from_latex("x^2 + y^2 = z^2").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG", "should produce valid PNG");
    assert!(png.len() > 100, "PNG should have meaningful content");
}

#[test]
fn latex_to_jpeg() {
    let result = from_latex("x^2").unwrap();
    let jpg = result.to_jpeg().unwrap();
    assert_eq!(jpg[0], 0xFF);
    assert_eq!(jpg[1], 0xD8);
}

#[test]
fn fraction_png() {
    let result = from_latex(r"\frac{a}{b}").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(png.len() > 200, "fraction should produce non-trivial image");
}

#[test]
fn sqrt_png() {
    let result = from_latex(r"\sqrt{x}").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn superscript_subscript_png() {
    let result = from_latex("x_i^2").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn builder_to_png_with_options() {
    let result = from_latex("x").unwrap();
    let options = RasterOptions {
        scale: 2.0,
        ..RasterOptions::default()
    };
    let png = result.to_png_with(&options).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn render_to_image_api() {
    let expr = from_latex("a + b").unwrap();
    let options = RasterOptions::default();
    let png = mathema::render_to_image(&expr.expr(), &options).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn png_dimensions_reasonable() {
    // A simple expression should produce a small image, not a huge one
    let result = from_latex("x").unwrap();
    let png = result.to_png().unwrap();
    // PNG IHDR chunk starts at byte 8: [width(4) height(4) ...]
    // Bytes 16-19 = width (big-endian u32), bytes 20-23 = height
    assert!(png.len() > 24, "PNG should have IHDR chunk");
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    assert!(
        width > 0 && width < 1000,
        "width {width} should be reasonable"
    );
    assert!(
        height > 0 && height < 1000,
        "height {height} should be reasonable"
    );
}

#[test]
fn empty_expression_renders() {
    let result = from_latex("").unwrap();
    let png = result.to_png().unwrap();
    assert!(!png.is_empty());
}

#[test]
fn complex_expression_renders() {
    // Multiple node types: fraction, superscript, identifiers, operators
    let result = from_latex(r"\frac{x^2 + 1}{y - 1}").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(
        png.len() > 500,
        "complex expression should produce substantial image"
    );
}

#[test]
fn convert_text_format_still_works() {
    // Ensure text-based convert() still works with raster feature enabled
    let result = mathema::convert("x^2", Format::LaTeX, Format::MathML).unwrap();
    assert!(result.contains("<math"));
}

#[test]
fn raster_format_not_parseable() {
    let result = mathema::convert("x", Format::Png, Format::LaTeX);
    assert!(result.is_err(), "PNG should not be parseable as input");
}

#[test]
fn jpeg_with_quality() {
    let result = from_latex("x").unwrap();
    let low_q = RasterOptions {
        format: ImageFormat::Jpeg,
        quality: 10,
        ..RasterOptions::default()
    };
    let high_q = RasterOptions {
        format: ImageFormat::Jpeg,
        quality: 95,
        ..RasterOptions::default()
    };
    let low = result.to_jpeg_with(&low_q).unwrap();
    let high = result.to_jpeg_with(&high_q).unwrap();
    // Higher quality should generally produce larger file
    assert!(
        high.len() >= low.len(),
        "higher quality JPEG should be >= lower quality"
    );
}

// =========================================================================
// Comprehensive formula rendering tests
// =========================================================================

fn assert_png(tex: &str) {
    let result = from_latex(tex).unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(
        &png[..4],
        b"\x89PNG",
        "should produce valid PNG for: {tex}"
    );
    assert!(
        png.len() > 50,
        "PNG should have content for: {tex} (got {} bytes)",
        png.len()
    );
}

#[test]
fn png_nested_fractions() {
    assert_png(r"\frac{1}{1+\frac{1}{1+\frac{1}{x}}}");
}

#[test]
fn png_display_integral() {
    let result = mathema::from_latex_with(
        r"\int_{0}^{\infty} e^{-x^2} \, dx = \frac{\sqrt{\pi}}{2}",
        &mathema::error::Config {
            strictness: mathema::error::Strictness::Lenient,
            display: mathema::ast::DisplayStyle::Block,
        },
    )
    .unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn png_matrix() {
    assert_png(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
}

#[test]
fn png_cases() {
    assert_png(r"f(x) = \begin{cases} x^2 & \text{if } x \geq 0 \\ -x^2 & \text{if } x < 0 \end{cases}");
}

#[test]
fn png_aligned() {
    assert_png(r"\begin{aligned} a &= b + c \\ d &= e + f \end{aligned}");
}

#[test]
fn png_accents() {
    assert_png(r"\hat{x} + \bar{y} + \tilde{z} + \vec{v} + \dot{a} + \ddot{b}");
}

#[test]
fn png_wide_accents() {
    assert_png(r"\widehat{abc} + \widetilde{xyz} + \overline{pqr}");
}

#[test]
fn png_delimiters() {
    assert_png(r"\left( \frac{a}{b} \right) + \left[ \frac{c}{d} \right]");
}

#[test]
fn png_overbrace_underbrace() {
    assert_png(r"\overbrace{a+b+c}^{\text{sum}} + \underbrace{x+y}_{\text{terms}}");
}

#[test]
fn png_cfrac() {
    assert_png(r"a_0 + \cfrac{1}{a_1 + \cfrac{1}{a_2 + \cfrac{1}{a_3}}}");
}

#[test]
fn png_color() {
    assert_png(r"\color{red}{a} + \color{blue}{b}");
}

#[test]
fn png_bold_greek() {
    assert_png(r"\mathbf{A} \cdot \boldsymbol{\alpha}");
}

#[test]
fn png_xrightarrow() {
    assert_png(r"A \xrightarrow{f} B \xrightarrow{g} C");
}

#[test]
fn png_binomial() {
    assert_png(r"\binom{n}{k} = \frac{n!}{k!(n-k)!}");
}

#[test]
fn png_big_operators() {
    assert_png(r"\bigcup_{i=1}^{n} A_i \cap \bigcap_{j=1}^{m} B_j");
}

#[test]
fn png_overset_underset() {
    assert_png(r"\overset{def}{=} \quad \underset{x \to 0}{\lim}");
}

#[test]
fn png_deeply_nested_scripts() {
    assert_png(r"x^{x^{x^x}} \quad a_{a_{a_a}}");
}

#[test]
fn png_prescripts() {
    assert_png(r"{}^{14}_{6}\text{C} \quad {}^{235}_{92}\text{U}");
}

#[test]
fn png_bra_ket() {
    assert_png(r"\langle \psi | \hat{H} | \psi \rangle = E");
}
