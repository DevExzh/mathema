#![cfg(feature = "raster")]
//! Integration tests for raster image rendering (PNG/JPEG).
//!
//! Run with: `cargo test --features raster --test render_raster`

use mathema::{Format, ImageFormat, RasterOptions, from_latex, render_to_svg};

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
fn png_raisebox_renders() {
    let result = from_latex(r"\frac{a}{a\raisebox{0.5em}{b}}").unwrap();
    let png = result.to_png().unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(
        png.len() > 200,
        "raisebox fraction should produce non-trivial image"
    );
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
    assert_eq!(&png[..4], b"\x89PNG", "should produce valid PNG for: {tex}");
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
        &mathema::error::Config::default()
            .with_strictness(mathema::error::Strictness::Lenient)
            .with_display(mathema::ast::DisplayStyle::Block),
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
    assert_png(
        r"f(x) = \begin{cases} x^2 & \text{if } x \geq 0 \\ -x^2 & \text{if } x < 0 \end{cases}",
    );
}

#[test]
fn png_aligned() {
    assert_png(r"\begin{aligned} a &= b + c \\ d &= e + f \end{aligned}");
}

#[test]
fn png_top_level_linebreak_renders() {
    assert_png(r"a+b\\c+d");
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

// ---------------------------------------------------------------------------
// Additional coverage: edge cases and complex formulas
// ---------------------------------------------------------------------------

#[test]
fn png_single_character() {
    assert_png("x");
}

#[test]
fn png_display_mode_sum() {
    let result = from_latex(r"\sum_{i=0}^{\infty} a_i").unwrap();
    let opts = RasterOptions {
        render: mathema::RenderOptions {
            display_mode: Some(true),
            ..Default::default()
        },
        scale: 2.0,
        ..Default::default()
    };
    let png = result.to_png_with(&opts).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(png.len() > 500, "display sum should produce large image");
}

#[test]
fn png_text_mode_sum() {
    let result = from_latex(r"\sum_{i=0}^{n} a_i").unwrap();
    let opts = RasterOptions {
        render: mathema::RenderOptions {
            display_mode: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let png = result.to_png_with(&opts).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
}

#[test]
fn png_deep_nested_fractions() {
    assert_png(r"\frac{1}{1 + \frac{1}{1 + \frac{1}{1 + \frac{1}{x}}}}");
}

#[test]
fn png_multiline_aligned() {
    assert_png(r"\begin{aligned} f(x) &= x^2 + 2x + 1 \\ &= (x+1)^2 \end{aligned}");
}

#[test]
fn png_cases_env() {
    assert_png(r"|x| = \begin{cases} x & \text{if } x \geq 0 \\ -x & \text{if } x < 0 \end{cases}");
}

#[test]
fn png_complex_integral() {
    assert_png(r"\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}");
}

#[test]
fn png_complex_matrix() {
    assert_png(
        r"\begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}",
    );
}

#[test]
fn png_subscript_superscript_chain() {
    assert_png(r"x_1^2 + x_2^2 + x_3^2 + \cdots + x_n^2");
}

#[test]
fn png_nabla_cross_product() {
    assert_png(
        r"\nabla \times \vec{F} = \left(\frac{\partial F_z}{\partial y} - \frac{\partial F_y}{\partial z}\right)\hat{i}",
    );
}

#[test]
fn png_maxwell_equation() {
    assert_png(
        r"\oint_{\partial \Sigma} \vec{E} \cdot d\vec{l} = -\frac{d}{dt}\iint_{\Sigma} \vec{B} \cdot d\vec{S}",
    );
}

#[test]
fn png_overbrace_underbrace_complex() {
    assert_png(
        r"\overbrace{1 + 2 + \cdots + n}^{n \text{ terms}} = \underbrace{\frac{n(n+1)}{2}}_{\text{sum}}",
    );
}

#[test]
fn png_all_greek() {
    assert_png(
        r"\alpha \beta \gamma \delta \epsilon \zeta \eta \theta \iota \kappa \lambda \mu \nu \xi \pi \rho \sigma \tau \upsilon \phi \chi \psi \omega",
    );
}

#[test]
fn png_stacked_limits() {
    assert_png(r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e");
}

#[test]
fn png_multiple_roots() {
    assert_png(r"\sqrt{2} + \sqrt[3]{3} + \sqrt[4]{4} + \sqrt[n]{x}");
}

#[test]
fn png_high_scale_retina() {
    let result = from_latex(r"E = mc^2").unwrap();
    let opts = RasterOptions {
        scale: 4.0,
        ..Default::default()
    };
    let png = result.to_png_with(&opts).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    // At 4x scale, the image should be significantly larger
    assert!(png.len() > 1000, "4x scale should produce larger image");
}

#[test]
fn png_white_background_jpeg() {
    let result = from_latex(r"a^2 + b^2 = c^2").unwrap();
    let opts = RasterOptions {
        format: ImageFormat::Jpeg,
        quality: 95,
        background: Some([255, 255, 255, 255]),
        ..Default::default()
    };
    let jpg = result.to_jpeg_with(&opts).unwrap();
    assert_eq!(jpg[0], 0xFF);
    assert_eq!(jpg[1], 0xD8);
}

#[test]
fn png_middle_delimiter() {
    assert_png(r"\left( x \middle| y \right)");
}

#[test]
fn png_middle_delimiter_with_fraction() {
    assert_png(r"\left\{ \frac{a}{b} \middle| \frac{c}{d} \right\}");
}

// ---------------------------------------------------------------------------
// Accent rendering regression tests
// ---------------------------------------------------------------------------

#[test]
fn png_widetilde() {
    assert_png(r"\widetilde{ABC}");
}

#[test]
fn png_widehat_wide() {
    assert_png(r"\widehat{ABCDE}");
}

#[test]
fn png_hat_single_char() {
    assert_png(r"\hat{x}");
}

#[test]
fn png_tilde_single_char() {
    assert_png(r"\tilde{z}");
}

#[test]
fn png_check_accent() {
    assert_png(r"\check{y}");
}

#[test]
fn png_grave_acute() {
    assert_png(r"\grave{a} + \acute{e}");
}

#[test]
fn png_breve_accent() {
    assert_png(r"\breve{o}");
}

#[test]
fn png_mathring() {
    assert_png(r"\mathring{A}");
}

#[test]
fn png_double_accent() {
    assert_png(r"\hat{\bar{x}}");
}

#[test]
fn png_all_accents_combined() {
    assert_png(r"\hat{a} \bar{b} \tilde{c} \vec{d} \dot{e} \ddot{f} \check{g}");
}

// ---------------------------------------------------------------------------
// SVG path implicit repeat regression test
// ---------------------------------------------------------------------------

#[test]
fn png_widetilde_vs_hat_distinct() {
    // Widetilde and widehat should produce visually distinct images.
    let tilde = from_latex(r"\widetilde{X}").unwrap();
    let hat = from_latex(r"\widehat{X}").unwrap();
    let t_png = tilde.to_png().unwrap();
    let h_png = hat.to_png().unwrap();
    // They should both be valid PNGs but have different content.
    assert_eq!(&t_png[..4], b"\x89PNG");
    assert_eq!(&h_png[..4], b"\x89PNG");
    assert_ne!(
        t_png, h_png,
        "widetilde and widehat should produce different images"
    );
}

// ---------------------------------------------------------------------------
// Additional formula coverage
// ---------------------------------------------------------------------------

#[test]
fn png_divergence_theorem() {
    assert_png(r"\oint_{\partial V} \vec{F} \cdot d\vec{S} = \iiint_V \nabla \cdot \vec{F} \, dV");
}

#[test]
fn png_euler_lagrange() {
    assert_png(
        r"\frac{\partial L}{\partial q} - \frac{d}{dt}\frac{\partial L}{\partial \dot{q}} = 0",
    );
}

#[test]
fn png_set_builder() {
    assert_png(r"\{ x \in \mathbb{R} \mid x^2 < 2 \}");
}

#[test]
fn png_limit_definition() {
    assert_png(r"\lim_{h \to 0} \frac{f(x+h) - f(x)}{h}");
}

#[test]
fn png_complex_matrix_with_fracs() {
    assert_png(
        r"\begin{pmatrix} \frac{1}{2} & \frac{1}{3} \\ \frac{1}{4} & \frac{1}{5} \end{pmatrix}",
    );
}

#[test]
fn png_rule_in_delimiter_no_panic() {
    // Regression: \rule inside \left...\right caused tiny-skia hairline AA panic.
    assert_png(r"+\left\{\rule{0.1em}{1em}\right.");
}

#[test]
fn png_nested_rule_in_superscript() {
    // Deeply nested rules at small sizes must not panic.
    assert_png(
        r"+\left\{\rule{0.1em}{1em}\right. x^{+\left\{\rule{0.1em}{1em}\right. x^{+\left\{\rule{0.1em}{1em}\right.}}",
    );
}

#[test]
fn png_rule_lift_renders() {
    assert_png(r"x\rule[1em]{1em}{1em}y");
}

#[test]
fn png_empty_group() {
    assert_png(r"a{}b");
}

#[test]
fn png_empty_subscript_superscript() {
    assert_png(r"x^{}_{} + y");
}

#[test]
fn png_text_in_math() {
    assert_png(r"x = \text{the value of } y");
}

#[test]
fn png_mathbb_mathcal() {
    assert_png(r"\mathbb{R} \times \mathcal{F} \subseteq \mathbb{C}");
}

#[test]
fn png_spacing_commands() {
    assert_png(r"a\,b\;c\quad d\qquad e");
}

#[test]
fn png_long_expression() {
    assert_png(r"a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p");
}

#[test]
fn png_triple_integral() {
    assert_png(r"\iiint_V f \, dV");
}

#[test]
fn png_stackrel() {
    assert_png(r"A \stackrel{f}{\longrightarrow} B");
}

#[test]
fn png_phantom() {
    assert_png(r"a + \phantom{b} + c");
}

#[test]
fn png_boxed() {
    assert_png(r"\boxed{E = mc^2}");
}

#[test]
fn png_operatorname() {
    assert_png(r"\operatorname{div} \vec{F} = \nabla \cdot \vec{F}");
}

#[test]
fn png_underset_limit() {
    assert_png(r"\underset{x \to 0}{\lim} \frac{\sin x}{x} = 1");
}

#[test]
fn png_array_environment() {
    assert_png(r"\begin{array}{cc} a & b \\ c & d \end{array}");
}

#[test]
fn png_bmatrix() {
    assert_png(r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}");
}

#[test]
fn png_vmatrix() {
    assert_png(r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}");
}

#[test]
fn png_double_struck() {
    assert_png(
        r"\mathbb{N} \subset \mathbb{Z} \subset \mathbb{Q} \subset \mathbb{R} \subset \mathbb{C}",
    );
}

#[test]
fn png_product_with_limits() {
    assert_png(r"\prod_{k=1}^{n} \left(1 - \frac{1}{k^2}\right)");
}

#[test]
fn png_floor_ceil() {
    assert_png(r"\lfloor x \rfloor + \lceil y \rceil");
}

#[test]
fn png_norm() {
    assert_png(r"\| \vec{v} \| = \sqrt{v_1^2 + v_2^2 + v_3^2}");
}

#[test]
fn png_multiline_cases() {
    assert_png(r"\begin{cases} 0 & n = 0 \\ 1 & n = 1 \\ F_{n-1} + F_{n-2} & n \geq 2 \end{cases}");
}

#[test]
fn png_det_matrix() {
    assert_png(r"\det \begin{pmatrix} a & b \\ c & d \end{pmatrix} = ad - bc");
}

#[test]
fn png_tensor_notation() {
    assert_png(r"T^{\mu\nu} = g^{\mu\alpha} g^{\nu\beta} T_{\alpha\beta}");
}

#[test]
fn png_summation_with_conditions() {
    assert_png(r"\sum_{\substack{0 < i < n \\ 0 < j < m}} a_{ij}");
}

#[test]
fn png_large_parentheses_around_fraction() {
    assert_png(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
}

#[test]
fn png_mixed_scripts() {
    assert_png(r"{}^{A}_{Z}X^{+2}");
}

#[test]
fn png_overline_underline() {
    assert_png(r"\overline{a + b} + \underline{c + d}");
}

#[test]
fn png_cancel() {
    assert_png(r"\cancel{x} + \bcancel{y} + \xcancel{z}");
}

#[test]
fn png_dimension_sanity() {
    // Verify rendered images have reasonable dimensions
    let formulas = vec![
        ("simple", "x"),
        ("fraction", r"\frac{a}{b}"),
        ("matrix", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
    ];
    for (name, tex) in formulas {
        let result = from_latex(tex).unwrap();
        let opts = RasterOptions {
            scale: 2.0,
            ..Default::default()
        };
        let png = result.to_png_with(&opts).unwrap();
        assert!(png.len() > 24, "PNG too small for: {name}");
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert!(w > 0 && w < 2000, "width {w} unreasonable for: {name}");
        assert!(h > 0 && h < 2000, "height {h} unreasonable for: {name}");
    }
}

// =========================================================================
// Academic formula PNG tests
// =========================================================================

#[test]
fn png_normal_distribution() {
    assert_png(r"f(x) = \frac{1}{\sigma\sqrt{2\pi}} e^{-\frac{(x-\mu)^2}{2\sigma^2}}");
}

#[test]
fn png_einstein_field() {
    assert_png(r"G_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8\pi G}{c^4} T_{\mu\nu}");
}

#[test]
fn png_dirac_equation() {
    assert_png(r"(i\gamma^\mu \partial_\mu - m)\psi = 0");
}

#[test]
fn png_residue_theorem() {
    assert_png(r"\oint_C f(z) dz = 2\pi i \sum \text{Res}(f, a_k)");
}

#[test]
fn png_euler_totient() {
    assert_png(r"\phi(n) = n \prod_{p|n} \left(1 - \frac{1}{p}\right)");
}

#[test]
fn png_tall_delimiters() {
    assert_png(r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)");
}

#[test]
fn png_deep_power_tower() {
    assert_png(r"a^{b^{c^{d^e}}}");
}

#[test]
fn png_tensor_indices() {
    assert_png(r"T^{\mu\nu}_{\rho\sigma}");
}

#[test]
fn png_partial_deriv() {
    assert_png(r"\frac{\partial^2 f}{\partial x \partial y}");
}

#[test]
fn png_floor_ceiling() {
    assert_png(r"\lfloor x \rfloor + \lceil x \rceil");
}

#[test]
fn png_overset_underset_extended() {
    assert_png(r"\overset{!}{=}");
    assert_png(r"\underset{n\to\infty}{\lim}");
}

#[test]
fn png_gathered_env() {
    assert_png(r"\begin{gathered} a+b \\ c+d \end{gathered}");
}

#[test]
fn png_font_styles() {
    assert_png(r"\mathbf{A} + \mathit{v} + \mathcal{L} + \mathfrak{g} + \mathbb{R}");
}

// =========================================================================
// JPEG quality boundary tests
// =========================================================================

#[test]
fn jpeg_quality_zero_clamped() {
    // Quality 0 should be clamped to 1 (valid JPEG range)
    let result = from_latex("x").unwrap();
    let opts = RasterOptions {
        format: ImageFormat::Jpeg,
        quality: 0,
        ..RasterOptions::default()
    };
    let data = result.to_jpeg_with(&opts).unwrap();
    assert!(
        data.len() > 50,
        "should produce valid JPEG even with quality=0"
    );
}

#[test]
fn jpeg_quality_max_clamped() {
    // Quality 255 should be clamped to 100
    let result = from_latex("x").unwrap();
    let opts = RasterOptions {
        format: ImageFormat::Jpeg,
        quality: 255,
        ..RasterOptions::default()
    };
    let data = result.to_jpeg_with(&opts).unwrap();
    assert!(
        data.len() > 50,
        "should produce valid JPEG even with quality=255"
    );
}

// =========================================================================
// Root / nth-root rendering
// =========================================================================

#[test]
fn png_cube_root() {
    assert_png(r"\sqrt[3]{x}");
}

#[test]
fn png_fourth_root() {
    assert_png(r"\sqrt[4]{16}");
}

#[test]
fn png_nth_root_complex() {
    assert_png(r"\sqrt[n]{a^n + b^n}");
}

#[test]
fn png_nested_radicals() {
    assert_png(r"\sqrt{1 + \sqrt{1 + \sqrt{1 + \sqrt{x}}}}");
}

// =========================================================================
// Boxed expressions (stroke-width proportional to font size)
// =========================================================================

#[test]
fn png_boxed_fraction() {
    assert_png(r"\boxed{\frac{a}{b}}");
}

#[test]
fn png_boxed_integral() {
    assert_png(r"\boxed{\int_0^1 f(x)\,dx}");
}

// =========================================================================
// Hi-DPI rendering
// =========================================================================

#[test]
fn png_retina_3x_scale() {
    let result = from_latex(r"\frac{x^2}{y^2}").unwrap();
    let opts = RasterOptions {
        scale: 3.0,
        ..RasterOptions::default()
    };
    let png = result.to_png_with(&opts).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(png.len() > 500, "3x retina PNG should be substantial");
}

// =========================================================================
// Cancel operations
// =========================================================================

#[test]
fn png_cancel_basic() {
    assert_png(r"\cancel{x} + y");
}

#[test]
fn png_bcancel() {
    assert_png(r"\bcancel{abc}");
}

#[test]
fn png_xcancel() {
    assert_png(r"\xcancel{xyz}");
}

// =========================================================================
// Multiple integrals and contour integrals
// =========================================================================

#[test]
fn png_double_integral() {
    assert_png(r"\iint_D f(x,y)\,dA");
}

#[test]
fn png_triple_integral_contour() {
    assert_png(r"\iiint_V \rho\,dV");
}

#[test]
fn png_contour_integral() {
    assert_png(r"\oint_C \mathbf{F}\cdot d\mathbf{r}");
}

// =========================================================================
// Substack notation
// =========================================================================

#[test]
fn png_substack() {
    assert_png(r"\sum_{\substack{i=1\\j=1}}^{n} a_{ij}");
}

// =========================================================================
// Complex real-world formulas
// =========================================================================

#[test]
fn png_stirling_approximation() {
    assert_png(r"n! \sim \sqrt{2\pi n}\left(\frac{n}{e}\right)^n");
}

#[test]
fn png_gamma_function() {
    assert_png(r"\Gamma(z) = \int_0^{\infty} t^{z-1} e^{-t}\,dt");
}

#[test]
fn png_bayes_theorem() {
    assert_png(r"P(A \mid B) = \frac{P(B \mid A)P(A)}{P(B)}");
}

#[test]
fn png_binomial_theorem() {
    assert_png(r"\sum_{k=0}^{n} \binom{n}{k} x^k y^{n-k} = (x+y)^n");
}

#[test]
fn png_maxwell_equations() {
    assert_png(
        r"\begin{aligned} \nabla \cdot \mathbf{E} &= \frac{\rho}{\epsilon_0} \\ \nabla \times \mathbf{B} &= \mu_0 \mathbf{J} \end{aligned}",
    );
}

#[test]
fn png_piecewise_three_cases() {
    assert_png(r"f(x) = \begin{cases} x^2 & x > 0 \\ 0 & x = 0 \\ -x^2 & x < 0 \end{cases}");
}

#[test]
fn png_matrix_determinant() {
    assert_png(r"\begin{vmatrix} a & b & c \\ d & e & f \\ g & h & i \end{vmatrix}");
}

#[test]
fn png_arrow_chain() {
    assert_png(r"A \xrightarrow{f} B \xrightarrow{g} C");
}

#[test]
fn png_continued_fraction() {
    assert_png(r"a_0 + \cfrac{1}{a_1 + \cfrac{1}{a_2 + \cfrac{1}{a_3}}}");
}

#[test]
fn png_floor_ceiling_nested() {
    assert_png(r"\left\lfloor \frac{n}{2} \right\rfloor + \left\lceil \frac{n}{3} \right\rceil");
}

// =========================================================================
// Display mode operators
// =========================================================================

#[test]
fn png_display_product() {
    assert_png(r"\displaystyle\prod_{i=1}^{n} x_i");
}

#[test]
fn png_display_coprod() {
    assert_png(r"\displaystyle\coprod_{i \in I} X_i");
}

#[test]
fn png_display_bigcap_bigcup() {
    assert_png(r"\displaystyle\bigcap_{i=1}^{n} A_i \cup \bigcup_{j=1}^{m} B_j");
}

// =========================================================================
// Color rendering
// =========================================================================

#[test]
fn png_color_nested() {
    assert_png(r"{\color{red} x^2} + {\color{blue} y^2} = {\color{green} z^2}");
}

#[test]
fn png_css_rgb_color_matches_equivalent_hex() {
    let result = from_latex("x").unwrap();
    let hex_options = RasterOptions {
        render: mathema::RenderOptions {
            fg_color: "#ff0000".into(),
            ..Default::default()
        },
        background: Some([255, 255, 255, 255]),
        ..Default::default()
    };
    let rgb_options = RasterOptions {
        render: mathema::RenderOptions {
            fg_color: "rgb(255, 0, 0)".into(),
            ..Default::default()
        },
        background: Some([255, 255, 255, 255]),
        ..Default::default()
    };

    let hex_png = result.to_png_with(&hex_options).unwrap();
    let rgb_png = result.to_png_with(&rgb_options).unwrap();

    assert_eq!(
        rgb_png, hex_png,
        "CSS rgb() raster color should render the same pixels as equivalent hex"
    );
}

#[test]
fn png_css_rgba_is_unsupported_and_falls_back_to_black() {
    let result = from_latex("x").unwrap();
    let black_options = RasterOptions {
        render: mathema::RenderOptions {
            fg_color: "black".into(),
            ..Default::default()
        },
        background: Some([255, 255, 255, 255]),
        ..Default::default()
    };
    let rgba_options = RasterOptions {
        render: mathema::RenderOptions {
            fg_color: "rgba(255, 0, 0, 0.25)".into(),
            ..Default::default()
        },
        background: Some([255, 255, 255, 255]),
        ..Default::default()
    };

    let black_png = result.to_png_with(&black_options).unwrap();
    let rgba_png = result.to_png_with(&rgba_options).unwrap();

    assert_eq!(
        rgba_png, black_png,
        "rgba() is unsupported by the RGB-only raster color parser and should fall back to black"
    );
}

// =========================================================================
// Modular arithmetic
// =========================================================================

#[test]
fn png_pmod() {
    assert_png(r"a \equiv b \pmod{n}");
}

// =========================================================================
// Stretchy delimiters with tall content
// =========================================================================

#[test]
fn png_tall_delimiters_nested() {
    assert_png(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
}

#[test]
fn png_mixed_delimiters_brace_bracket() {
    assert_png(r"\left\{ \frac{a}{b} \right]");
}

// =========================================================================
// \smash — height/depth zeroing
// =========================================================================

#[test]
fn png_smash_bottom() {
    assert_png(r"\frac{1}{\smash[b]{q}}");
}

#[test]
fn png_smash_top() {
    assert_png(r"\smash[t]{X^2}");
}

#[test]
fn png_smash_both() {
    assert_png(r"\smash{X^2_j}");
}

#[test]
fn png_smash_in_fraction() {
    // \smash[b] should reduce the depth of the denominator
    let with_smash = from_latex(r"\frac{1}{\smash[b]{q}}").unwrap();
    let without_smash = from_latex(r"\frac{1}{q}").unwrap();
    let opts = RasterOptions {
        scale: 4.0,
        ..Default::default()
    };
    let png_with = with_smash.to_png_with(&opts).unwrap();
    let png_without = without_smash.to_png_with(&opts).unwrap();
    // The smash version should produce a smaller image (less total height)
    assert!(
        png_with.len() < png_without.len(),
        "smash[b] should reduce overall image size"
    );
}

// =========================================================================
// \mathop, \mathrel, \mathbin — atom type reclassification
// =========================================================================

#[test]
fn png_mathop() {
    assert_png(r"\mathop{T}_{i=1}^{n}");
}

#[test]
fn png_mathrel() {
    assert_png(r"a \mathrel{R} b");
}

#[test]
fn png_mathbin() {
    assert_png(r"a \mathbin{\diamond} b");
}

#[test]
fn png_mathord() {
    assert_png(r"\mathord{+}x");
}

// =========================================================================
// Display-mode large operators with limits
// =========================================================================

#[test]
fn png_display_sum_with_limits() {
    assert_png(r"\displaystyle\sum_{k=0}^{\infty} \frac{1}{k!}");
}

#[test]
fn png_display_product_with_limits() {
    assert_png(r"\displaystyle\prod_{p\text{ prime}} \frac{1}{1-p^{-s}}");
}

#[test]
fn png_display_integral_with_limits() {
    assert_png(r"\displaystyle\int_{-\infty}^{\infty} e^{-x^2}\,dx");
}

#[test]
fn png_display_coprod_indexed() {
    assert_png(r"\displaystyle\coprod_{\alpha \in A} G_\alpha");
}

// =========================================================================
// Nested radicals
// =========================================================================

#[test]
fn png_nested_sqrt_three_levels() {
    assert_png(r"\sqrt{\sqrt{\sqrt{x}}}");
}

#[test]
fn png_sqrt_with_index() {
    assert_png(r"\sqrt[3]{x^2 + y^2}");
}

// =========================================================================
// Wide accents
// =========================================================================

#[test]
fn png_widehat_six_chars() {
    assert_png(r"\widehat{ABCDEF}");
}

#[test]
fn png_widetilde_three_chars() {
    assert_png(r"\widetilde{xyz}");
}

// =========================================================================
// Continued fractions
// =========================================================================

#[test]
fn png_cfrac_deep() {
    assert_png(r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{x}}}}");
}

// =========================================================================
// Complex real-world formulas
// =========================================================================

#[test]
fn png_cauchy_schwarz_inequality() {
    assert_png(
        r"\left(\sum_{k=1}^n a_k b_k\right)^2 \leq \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
    );
}

#[test]
fn png_maxwell_curl_equation() {
    assert_png(r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}");
}

#[test]
fn png_schrodinger() {
    assert_png(r"i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi");
}

#[test]
fn png_euler_product() {
    assert_png(
        r"\zeta(s) = \sum_{n=1}^{\infty} \frac{1}{n^s} = \prod_{p\text{ prime}} \frac{1}{1-p^{-s}}",
    );
}

#[test]
fn png_stirling() {
    assert_png(r"n! \sim \sqrt{2\pi n}\left(\frac{n}{e}\right)^n");
}

// =========================================================================
// Spacing
// =========================================================================

#[test]
fn png_thin_medium_thick_quad_spaces() {
    assert_png(r"a\,b\;c\quad d\qquad e");
}

// =========================================================================
// Isotope notation (pre-scripts)
// =========================================================================

#[test]
fn png_isotope_carbon14() {
    assert_png(r"{}^{14}_{6}\text{C}");
}

// =========================================================================
// Quality: image dimensions and scaling
// =========================================================================

/// Extract PNG image dimensions from header (IHDR chunk).
fn png_data_dimensions(data: &[u8]) -> (u32, u32) {
    // PNG signature: 8 bytes, then IHDR chunk starts at offset 8
    // IHDR: 4 bytes length + 4 bytes "IHDR" + 4 bytes width + 4 bytes height
    assert!(data.len() >= 24, "PNG too small to contain IHDR");
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (w, h)
}

fn png_dimensions(tex: &str) -> (u32, u32) {
    let result = from_latex(tex).unwrap();
    let png = result.to_png().unwrap();
    png_data_dimensions(&png)
}

fn png_occupied_rows(data: &[u8]) -> Vec<usize> {
    let pixmap = tiny_skia::Pixmap::decode_png(data).expect("PNG should decode");
    let width = pixmap.width() as usize;
    let mut rows = Vec::new();
    for (idx, pixel) in pixmap.pixels().iter().enumerate() {
        if pixel.alpha() > 0 {
            rows.push(idx / width);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

fn svg_width(tex: &str) -> f64 {
    let result = from_latex(tex).unwrap();
    let svg = render_to_svg(&result.expr(), &mathema::RenderOptions::default()).unwrap();
    let start = svg.find("width=\"").expect("SVG should contain width") + 7;
    let rest = &svg[start..];
    let end = rest.find('"').expect("SVG width should be closed");
    rest[..end].parse().expect("SVG width should parse")
}

#[test]
fn png_leading_negative_space_keeps_trailing_layout_width() {
    let without_trailing = r"\!\!\!\!\!\!x";
    let with_trailing = r"\!\!\!\!\!\!x\qquad";
    let (without_trailing_width, _) = png_dimensions(without_trailing);
    let (with_trailing_width, _) = png_dimensions(with_trailing);
    let svg_width_delta = svg_width(with_trailing) - svg_width(without_trailing);
    let png_width_delta = with_trailing_width - without_trailing_width;

    assert!(
        with_trailing_width >= without_trailing_width + 20,
        "trailing layout width should remain in PNG bounds even after leading negative space: without={without_trailing_width}, with={with_trailing_width}"
    );
    assert!(
        (png_width_delta as f64 - svg_width_delta).abs() <= 2.0,
        "PNG and SVG trailing-width deltas should match within rounding: png_delta={png_width_delta}, svg_delta={svg_width_delta}"
    );
}

#[test]
fn png_scale_doubles_dimensions() {
    let result = from_latex("x^2").unwrap();
    let opts_1x = RasterOptions {
        scale: 1.0,
        ..Default::default()
    };
    let opts_2x = RasterOptions {
        scale: 2.0,
        ..Default::default()
    };
    let png_1x = result.to_png_with(&opts_1x).unwrap();
    let png_2x = result.to_png_with(&opts_2x).unwrap();
    let (w1, h1) = png_data_dimensions(&png_1x);
    let (w2, h2) = png_data_dimensions(&png_2x);
    // 2x should be approximately double in each dimension
    assert!(
        w2 >= w1 * 2 - 2 && w2 <= w1 * 2 + 2,
        "2x width {w2} should be ~{} (2 * {w1})",
        w1 * 2
    );
    assert!(
        h2 >= h1 * 2 - 2 && h2 <= h1 * 2 + 2,
        "2x height {h2} should be ~{} (2 * {h1})",
        h1 * 2
    );
}

#[test]
fn png_fraction_taller_than_simple() {
    let result_simple = from_latex("x").unwrap();
    let result_frac = from_latex(r"\frac{a}{b}").unwrap();
    let png_simple = result_simple.to_png().unwrap();
    let png_frac = result_frac.to_png().unwrap();
    let (_, h_simple) = png_data_dimensions(&png_simple);
    let (_, h_frac) = png_data_dimensions(&png_frac);
    assert!(
        h_frac > h_simple,
        "fraction ({h_frac}px) should be taller than simple ({h_simple}px)"
    );
}

#[test]
fn png_raisebox_increases_height_without_excess_width() {
    let (plain_width, plain_height) = png_dimensions(r"\frac{a}{ab}");
    let (raised_width, raised_height) = png_dimensions(r"\frac{a}{a\raisebox{0.5em}{b}}");

    assert!(
        raised_height > plain_height,
        "raised denominator should increase PNG height: plain={plain_height}, raised={raised_height}"
    );
    assert!(
        raised_width <= plain_width + 8,
        "raisebox should not add excessive width: plain={plain_width}, raised={raised_width}"
    );
}

#[test]
fn png_rule_height_tracks_requested_height() {
    let (_, short_height) = png_dimensions(r"\rule{1em}{0.25em}");
    let (_, tall_height) = png_dimensions(r"\rule{1em}{2em}");

    assert!(
        tall_height >= short_height + 20,
        "2em rule should be visibly taller than 0.25em rule: short={short_height}, tall={tall_height}"
    );
}

#[test]
fn png_explicit_1em_rule_is_not_vertically_clipped() {
    let result = from_latex(r"\rule{1em}{1em}").unwrap();
    let options = RasterOptions::default();
    let png = result.to_png_with(&options).unwrap();
    let (_, image_height) = png_data_dimensions(&png);
    let occupied_rows = png_occupied_rows(&png);

    let first_row = *occupied_rows
        .first()
        .expect("explicit rule should paint at least one row");
    let last_row = *occupied_rows
        .last()
        .expect("explicit rule should paint at least one row");

    assert!(
        occupied_rows.len() >= 15,
        "1em high rule at 16px font size should occupy about 16 rows, got {} rows ({first_row}..={last_row}) in {image_height}px image",
        occupied_rows.len()
    );
    assert!(
        first_row <= 3,
        "unlifted rule should start near the 2px top padding, got first painted row {first_row} in {image_height}px image"
    );
    assert!(
        last_row + 2 < image_height as usize,
        "unlifted rule should leave bottom padding and not be clipped, got last painted row {last_row} in {image_height}px image"
    );
}

#[test]
fn png_nested_fraction_taller_than_simple_fraction() {
    let result_simple = from_latex(r"\frac{a}{b}").unwrap();
    let result_nested = from_latex(r"\frac{\frac{a}{b}}{\frac{c}{d}}").unwrap();
    let png_simple = result_simple.to_png().unwrap();
    let png_nested = result_nested.to_png().unwrap();
    let (_, h_simple) = png_data_dimensions(&png_simple);
    let (_, h_nested) = png_data_dimensions(&png_nested);
    assert!(
        h_nested > h_simple,
        "nested fraction ({h_nested}px) should be taller than simple fraction ({h_simple}px)"
    );
}

#[test]
fn png_sum_wider_than_single_char() {
    let result_char = from_latex("x").unwrap();
    let result_sum = from_latex(r"\sum_{i=0}^{n} x_i").unwrap();
    let png_char = result_char.to_png().unwrap();
    let png_sum = result_sum.to_png().unwrap();
    let (w_char, _) = png_data_dimensions(&png_char);
    let (w_sum, _) = png_data_dimensions(&png_sum);
    assert!(
        w_sum > w_char * 3,
        "sum formula ({w_sum}px) should be much wider than single char ({w_char}px)"
    );
}

#[test]
fn png_image_has_reasonable_size() {
    // A moderately complex formula should produce an image between 100 bytes and 1MB
    let result = from_latex(r"E = mc^2").unwrap();
    let opts = RasterOptions {
        scale: 2.0,
        ..Default::default()
    };
    let png = result.to_png_with(&opts).unwrap();
    assert!(png.len() > 100, "PNG should be > 100 bytes");
    assert!(
        png.len() < 1_000_000,
        "PNG should be < 1MB for simple formula"
    );
}

#[test]
fn png_einstein_field_equations() {
    assert_png(
        r"R_{\mu\nu} - \frac{1}{2}Rg_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8\pi G}{c^4}T_{\mu\nu}",
    );
}

#[test]
fn png_fourier_transform() {
    assert_png(r"\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\,e^{-2\pi i x \xi}\,dx");
}

#[test]
fn png_taylor_series() {
    assert_png(r"f(x) = \sum_{n=0}^{\infty} \frac{f^{(n)}(a)}{n!}(x-a)^n");
}

#[test]
fn png_cauchy_integral() {
    assert_png(r"f(a) = \frac{1}{2\pi i} \oint_\gamma \frac{f(z)}{z - a}\,dz");
}

#[test]
fn png_binomial_theorem_full() {
    assert_png(r"(x+y)^n = \sum_{k=0}^{n} \binom{n}{k} x^{n-k} y^k");
}

// =========================================================================
// Regression: small radicand radical_path fix
// =========================================================================

#[test]
fn png_sqrt_single_digit() {
    // Small radicand used to cause radical path v-bottom escape
    assert_png(r"\sqrt{2}");
}

#[test]
fn png_sqrt_tiny_expression() {
    assert_png(r"\sqrt{.}");
}

#[test]
fn png_sqrt_nested_small() {
    // Deeply nested small sqrt — each level has small height
    assert_png(r"\sqrt{\sqrt{\sqrt{x}}}");
}

// =========================================================================
// Regression: stretchy arrow/accent scaled path (scientific notation fix)
// =========================================================================

#[test]
fn png_long_xrightarrow() {
    // Long labels produce stretched arrows that may have scientific notation in path data
    assert_png(r"A \xrightarrow[\text{below label}]{\text{above label}} B");
}

#[test]
fn png_wide_tilde_accent() {
    assert_png(r"\widetilde{ABCDEFGH}");
}

// =========================================================================
// Regression: nested color font-family fix
// =========================================================================

#[test]
fn png_nested_colors() {
    // Multiple color nesting should not corrupt font rendering
    assert_png(r"{\color{red} x^2} + {\color{blue} {\color{green} y^2}} = z^2");
}

#[test]
fn png_prescripts_different_widths() {
    // Pre-scripts with different widths should right-align within the pre-column
    assert_png(r"{}^{abc}_{x}\text{C}");
}

#[test]
fn png_prescripts_only_sup() {
    assert_png(r"{}^{14}\text{C}");
}

#[test]
fn png_prescripts_only_sub() {
    assert_png(r"{}_{6}\text{C}");
}
