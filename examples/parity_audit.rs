// Parity audit: render each corpus formula to (a) standalone embedded-font SVG
// rasterized via resvg, and (b) PNG via the raster backend, then measure the
// pixel difference. Also dumps both PNGs side by side for visual inspection.
//
// Run: cargo run --example parity_audit --features "raster,font-embed"

use std::fs;
use std::path::Path;

use mathema::render::raster::{ImageFormat, RasterOptions};
use mathema::render::RenderOptions;

fn main() {
    let corpus = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/data/corpus.json".to_string());
    let data = fs::read_to_string(&corpus).expect("corpus json");
    let formulas: Vec<serde_json::Value> = serde_json::from_str(&data).expect("valid json");

    let out = Path::new("tests/data/parity");
    fs::create_dir_all(out).unwrap();

    let scale = 2.0f32;
    let mut rows = String::new();
    let mut worst: Vec<(String, f64)> = Vec::new();

    for f in &formulas {
        let name = f["name"].as_str().unwrap();
        let tex = f["tex"].as_str().unwrap();
        let display = f["display"].as_bool().unwrap_or(false);

        let render = RenderOptions {
            display_mode: Some(display),
            ..RenderOptions::default()
        };

        let parsed = match mathema::from_latex(tex) {
            Ok(p) => p,
            Err(e) => {
                rows.push_str(&format!(
                    "<tr><td>{name}</td><td colspan=3 class=err>PARSE ERROR: {e}</td></tr>"
                ));
                continue;
            },
        };

        // (a) standalone SVG -> rasterize with resvg. Use the compact path so the
        // comparison matches the compact path that to_png()/to_svg() use.
        let svg =
            mathema::render::render_to_svg_standalone_compact(parsed.compact_expr(), &render)
                .unwrap();
        fs::write(out.join(format!("{name}.svg")), &svg).unwrap();
        let svg_png = rasterize_svg(&svg, scale);

        // (b) raster PNG
        let ropts = RasterOptions {
            render: render.clone(),
            format: ImageFormat::Png,
            scale: scale as f64,
            background: Some([255, 255, 255, 255]),
            quality: 90,
        };
        let png_bytes = parsed.to_png_with(&ropts).unwrap();
        fs::write(out.join(format!("{name}_png.png")), &png_bytes).unwrap();
        let png_pix = decode_png(&png_bytes);

        if let (Some((sw, sh, sbuf)), Some((pw, ph, pbuf))) = (&svg_png, &png_pix) {
            fs::write(out.join(format!("{name}_svg.png")), encode_png(*sw, *sh, sbuf)).unwrap();
            let diff = compare(*sw, *sh, sbuf, *pw, *ph, pbuf);
            worst.push((name.to_string(), diff));
            let cls = if diff > 5.0 { "err" } else { "ok" };
            rows.push_str(&format!(
                "<tr><td>{name}</td><td>svg {sw}x{sh}</td><td>png {pw}x{ph}</td>\
                 <td class={cls}>{diff:.2}% diff</td></tr>\
                 <tr><td colspan=4><img src='{name}_svg.png'> <img src='{name}_png.png'></td></tr>"
            ));
        }
    }

    worst.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("=== SVG vs PNG parity (worst first) ===");
    for (n, d) in &worst {
        println!("  {d:6.2}%  {n}");
    }
    let avg = worst.iter().map(|x| x.1).sum::<f64>() / worst.len().max(1) as f64;
    println!("average diff: {avg:.2}%");

    let html = format!(
        "<!doctype html><html><head><meta charset=utf-8><style>\
        table{{border-collapse:collapse}}td{{border:1px solid #ccc;padding:4px}}\
        .err{{color:#b00;font-weight:bold}}.ok{{color:#080}}img{{background:#fff;border:1px solid #eee}}\
        </style></head><body><h1>SVG (left) vs PNG (right)</h1><table>{rows}</table></body></html>"
    );
    fs::write(out.join("parity.html"), html).unwrap();
    println!("wrote tests/data/parity/parity.html");
}

fn rasterize_svg(svg: &str, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::usvg;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt).ok()?;
    let size = tree.size();
    let w = (size.width() * scale).ceil() as u32;
    let h = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    // white background
    pixmap.fill(tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some((w, h, pixmap.data().to_vec()))
}

fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    // normalize to RGBA
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

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
    }
    out
}

/// Mean luminance difference over the best small-offset alignment, as a percentage.
/// Searching a small (dx,dy) window removes global sub-pixel/baseline shifts so the
/// metric reflects genuine content divergence rather than a 1px canvas offset.
fn compare(aw: u32, ah: u32, a: &[u8], bw: u32, bh: u32, b: &[u8]) -> f64 {
    let mut best = f64::INFINITY;
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            let d = diff_at(aw, ah, a, bw, bh, b, dx, dy);
            if d < best {
                best = d;
            }
        }
    }
    best * 100.0
}

fn diff_at(aw: u32, ah: u32, a: &[u8], bw: u32, bh: u32, b: &[u8], dx: i32, dy: i32) -> f64 {
    let w = aw.min(bw) as i32;
    let h = ah.min(bh) as i32;
    let mut total = 0.0f64;
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

fn luma(p: &[u8]) -> f64 {
    // composite over white
    let a = p[3] as f64 / 255.0;
    let r = p[0] as f64 * a + 255.0 * (1.0 - a);
    let g = p[1] as f64 * a + 255.0 * (1.0 - a);
    let bl = p[2] as f64 * a + 255.0 * (1.0 - a);
    0.299 * r + 0.587 * g + 0.114 * bl
}
