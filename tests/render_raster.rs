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
