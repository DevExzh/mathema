//! SVG↔PNG parity regression test.
//!
//! Renders each corpus formula to (a) a standalone embedded-font SVG rasterized
//! with `resvg`, and (b) a PNG via the raster backend, then measures the pixel
//! difference under a small alignment search. Both paths share the layout pass
//! and the embedded Latin Modern Math outlines, so they must look nearly
//! identical; this test guards against regressions in that parity.

#![cfg(feature = "raster")]

use mathema::render::RenderOptions;
use mathema::render::raster::{ImageFormat, RasterOptions};

fn rasterize_svg(svg: &str, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::usvg;
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let w = (size.width() * scale).ceil() as u32;
    let h = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    pixmap.fill(tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some((w, h, pixmap.data().to_vec()))
}

fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

fn luma(p: &[u8]) -> f64 {
    let a = p[3] as f64 / 255.0;
    let r = p[0] as f64 * a + 255.0 * (1.0 - a);
    let g = p[1] as f64 * a + 255.0 * (1.0 - a);
    let b = p[2] as f64 * a + 255.0 * (1.0 - a);
    0.299 * r + 0.587 * g + 0.114 * b
}

#[allow(clippy::too_many_arguments)]
fn diff_at(aw: u32, ah: u32, a: &[u8], bw: u32, bh: u32, b: &[u8], dx: i32, dy: i32) -> f64 {
    let w = aw.min(bw) as i32;
    let h = ah.min(bh) as i32;
    let mut total = 0.0;
    let mut count = 0u64;
    for y in 0..h {
        let by = y + dy;
        if by < 0 || by >= bh as i32 {
            continue;
        }
        for x in 0..w {
            let bx = x + dx;
            if bx < 0 || bx >= bw as i32 {
                continue;
            }
            let ai = ((y as u32 * aw + x as u32) * 4) as usize;
            let bi = ((by as u32 * bw + bx as u32) * 4) as usize;
            total += (luma(&a[ai..ai + 4]) - luma(&b[bi..bi + 4])).abs();
            count += 1;
        }
    }
    total / count.max(1) as f64 / 255.0
}

/// Best alignment within a small window, removing sub-pixel/canvas offsets.
fn compare(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>)) -> f64 {
    let mut best = f64::INFINITY;
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            let d = diff_at(a.0, a.1, &a.2, b.0, b.1, &b.2, dx, dy);
            if d < best {
                best = d;
            }
        }
    }
    best * 100.0
}

#[test]
fn svg_matches_png_on_corpus() {
    // Inline corpus so the test does not depend on gitignored test data.
    // (name, tex, display)
    let formulas: &[(&str, &str, bool)] = &[
        ("ident_ops", "x + y = z - w", false),
        ("frac_nested", r"\frac{a + \frac{b}{c}}{d - e}", true),
        (
            "sqrt_index",
            r"\sqrt{x} + \sqrt[3]{y} + \sqrt[n]{1+x^2}",
            true,
        ),
        ("supersub", "x^{2} + y_{i}^{2} + z_{i+1}^{2k}", false),
        (
            "sum_int_display",
            r"\sum_{i=1}^{n} i^2 + \int_{0}^{\infty} e^{-x} dx",
            true,
        ),
        (
            "matrix_p",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            true,
        ),
        (
            "matrix_b",
            r"\begin{bmatrix} 1 & 0 & 0 \\ 0 & 1 & 0 \\ 0 & 0 & 1 \end{bmatrix}",
            true,
        ),
        (
            "cases",
            r"f(x) = \begin{cases} x^2 & \text{if } x > 0 \\ 0 & \text{otherwise} \end{cases}",
            true,
        ),
        (
            "accents",
            r"\hat{x} + \widehat{xyz} + \vec{v} + \tilde{a} + \widetilde{abc} + \bar{y}",
            false,
        ),
        (
            "braces",
            r"\overbrace{a+b+c}^{n} + \underbrace{x+y}_{m}",
            true,
        ),
        (
            "delims_big",
            r"\left( \frac{1}{1 + \frac{1}{1 + x}} \right) \left[ \sum_k a_k \right]",
            true,
        ),
        ("middle", r"\left\langle x \middle| y \right\rangle", true),
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", true),
        ("binom", r"\binom{n}{k} = \frac{n!}{k!(n-k)!}", true),
        ("nested_supersub", "e^{e^{e^{x}}} + a_{b_{c_{d}}}", false),
        ("color_boxed", r"\boxed{\color{red}{E = mc^2}}", false),
        (
            "integral_triple",
            r"\iiint_V f \, dV + \oint_C \vec{F} \cdot d\vec{r}",
            true,
        ),
        (
            "tall_frac_paren",
            r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)",
            true,
        ),
    ];

    let scale = 2.0f32;
    let mut diffs = Vec::new();
    let mut worst = ("".to_string(), 0.0f64);

    for &(name, tex, display) in formulas {
        let render = RenderOptions {
            display_mode: Some(display),
            ..RenderOptions::default()
        };
        let parsed = mathema::from_latex(tex).unwrap_or_else(|e| panic!("{name}: parse: {e}"));

        let svg = mathema::render::render_to_svg_standalone_compact(parsed.compact_expr(), &render)
            .unwrap();
        let svg_img = rasterize_svg(&svg, scale).expect("resvg should rasterize");

        let ropts = RasterOptions {
            render: render.clone(),
            format: ImageFormat::Png,
            scale: scale as f64,
            background: Some([255, 255, 255, 255]),
            quality: 90,
        };
        let png_img = decode_png(&parsed.to_png_with(&ropts).unwrap()).expect("decode png");

        let diff = compare(&svg_img, &png_img);
        if diff > worst.1 {
            worst = (name.to_string(), diff);
        }
        diffs.push(diff);

        // Per-formula ceiling: embedded SVG and PNG share layout + outlines,
        // so any divergence beyond this means a real rendering bug (not AA noise).
        assert!(
            diff < 12.0,
            "{name}: SVG vs PNG differ by {diff:.2}% (>12%) — likely a real divergence"
        );
    }

    let avg = diffs.iter().sum::<f64>() / diffs.len() as f64;
    println!(
        "SVG/PNG parity: avg {avg:.2}%, worst {} {:.2}%",
        worst.0, worst.1
    );
    assert!(
        avg < 5.0,
        "average SVG/PNG diff {avg:.2}% exceeds 5% — parity regression"
    );
}
