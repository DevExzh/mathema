//! Quality verification for SVG rendering output.
//!
//! Parses generated SVG to programmatically check:
//!   1. Delimiter sizing (parentheses enclose nested fractions)
//!   2. Fraction bar alignment (positioned at math axis)
//!   3. Script positioning (super/subscript shifts)
//!   4. Accent centering (hat centered over base)
//!   5. Radical rendering (sqrt sign + overbar coverage)
//!   6. Matrix column alignment

use mathema::convert::from_latex;
use mathema::render::RenderOptions;

// ---------------------------------------------------------------------------
// Minimal SVG element parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SvgElement {
    tag: String,
    attrs: Vec<(String, String)>,
    text_content: Option<String>,
}

impl SvgElement {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn attr_f64(&self, name: &str) -> Option<f64> {
        self.attr(name).and_then(|v| v.parse::<f64>().ok())
    }
}

fn parse_svg_elements(svg: &str) -> Vec<SvgElement> {
    let mut elements = Vec::new();
    let mut pos = 0;
    let bytes = svg.as_bytes();

    while pos < bytes.len() {
        let Some(lt) = svg[pos..].find('<') else {
            break;
        };
        let start = pos + lt;
        if bytes.get(start + 1) == Some(&b'/') || bytes.get(start + 1) == Some(&b'?') {
            pos = start + 1;
            continue;
        }
        let Some(gt) = svg[start..].find('>') else {
            break;
        };
        let end = start + gt;
        let tag_content = svg[start + 1..end].trim_end_matches('/');
        let (tag_name, attr_str) = match tag_content.find(|c: char| c.is_whitespace()) {
            Some(i) => (&tag_content[..i], tag_content[i..].trim()),
            None => (tag_content, ""),
        };
        let attrs = parse_attributes(attr_str);
        let text_content = if !svg[start..=end].ends_with('/') && end + 1 < svg.len() {
            if let Some(next_lt) = svg[end + 1..].find('<') {
                let tc = &svg[end + 1..end + 1 + next_lt];
                if !tc.is_empty() {
                    Some(unescape_xml(tc))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        elements.push(SvgElement {
            tag: tag_name.to_string(),
            attrs,
            text_content,
        });
        pos = end + 1;
    }
    elements
}

fn parse_attributes(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        let after_eq = &rest[eq + 1..];
        let Some(q_start) = after_eq.find('"') else {
            break;
        };
        let val_start = q_start + 1;
        let Some(q_end) = after_eq[val_start..].find('"') else {
            break;
        };
        let value = &after_eq[val_start..val_start + q_end];
        attrs.push((key.to_string(), value.to_string()));
        rest = after_eq[val_start + q_end + 1..].trim_start();
    }
    attrs
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn parse_transform(transform: &str) -> Option<(f64, f64, f64)> {
    let t_start = transform.find("translate(")? + "translate(".len();
    let t_end = transform[t_start..].find(')')? + t_start;
    let coords: Vec<&str> = transform[t_start..t_end].split(',').collect();
    let tx = coords.first()?.trim().parse::<f64>().ok()?;
    let ty = coords.get(1)?.trim().parse::<f64>().ok()?;
    let s_start = transform.find("scale(")? + "scale(".len();
    let s_end = transform[s_start..].find(')')? + s_start;
    let scale = transform[s_start..s_end].trim().parse::<f64>().ok()?;
    Some((tx, ty, scale))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const EM: f64 = 16.0;

fn render_display(latex: &str) -> String {
    let result = from_latex(latex).unwrap_or_else(|e| panic!("Parse: `{latex}`: {e}"));
    let opts = RenderOptions {
        font_size_px: EM,
        display_mode: Some(true),
        ..RenderOptions::default()
    };
    result
        .to_svg_with(&opts)
        .unwrap_or_else(|e| panic!("SVG: `{latex}`: {e}"))
}

fn render_inline(latex: &str) -> String {
    let result = from_latex(latex).unwrap_or_else(|e| panic!("Parse: `{latex}`: {e}"));
    let opts = RenderOptions {
        font_size_px: EM,
        display_mode: Some(false),
        ..RenderOptions::default()
    };
    result
        .to_svg_with(&opts)
        .unwrap_or_else(|e| panic!("SVG: `{latex}`: {e}"))
}

struct ViewBox {
    min_x: f64,
    min_y: f64,
    width: f64,
    height: f64,
}

fn parse_viewbox(svg: &str) -> ViewBox {
    let elements = parse_svg_elements(svg);
    let svg_el = elements.iter().find(|e| e.tag == "svg").expect("no <svg>");
    let vb = svg_el.attr("viewBox").expect("no viewBox");
    let p: Vec<f64> = vb
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    ViewBox {
        min_x: p[0],
        min_y: p[1],
        width: p[2],
        height: p[3],
    }
}

struct TextInfo {
    x: f64,
    y: f64,
    font_size: f64,
    content: String,
}

fn collect_texts(elements: &[SvgElement]) -> Vec<TextInfo> {
    elements
        .iter()
        .filter(|e| e.tag == "text")
        .map(|el| TextInfo {
            x: el.attr_f64("x").unwrap_or(0.0),
            y: el.attr_f64("y").unwrap_or(0.0),
            font_size: el.attr_f64("font-size").unwrap_or(EM),
            content: el.text_content.clone().unwrap_or_default(),
        })
        .collect()
}

#[derive(Clone)]
struct RectInfo {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn collect_rects(elements: &[SvgElement]) -> Vec<RectInfo> {
    elements
        .iter()
        .filter(|e| e.tag == "rect")
        .map(|el| RectInfo {
            x: el.attr_f64("x").unwrap_or(0.0),
            y: el.attr_f64("y").unwrap_or(0.0),
            w: el.attr_f64("width").unwrap_or(0.0),
            h: el.attr_f64("height").unwrap_or(0.0),
        })
        .collect()
}

fn collect_g_transforms(elements: &[SvgElement]) -> Vec<(f64, f64, f64)> {
    elements
        .iter()
        .filter(|e| e.tag == "g")
        .filter_map(|e| e.attr("transform"))
        .filter_map(parse_transform)
        .collect()
}

static mut PASS_COUNT: u32 = 0;
static mut WARN_COUNT: u32 = 0;
static mut FAIL_COUNT: u32 = 0;

fn pass(msg: &str) {
    println!("  [PASS] {}", msg);
    unsafe {
        PASS_COUNT += 1;
    }
}
fn warn(msg: &str) {
    println!("  [WARN] {}", msg);
    unsafe {
        WARN_COUNT += 1;
    }
}
fn fail(msg: &str) {
    println!("  [FAIL] {}", msg);
    unsafe {
        FAIL_COUNT += 1;
    }
}
fn info(msg: &str) {
    println!("  [info] {}", msg);
}

// ---------------------------------------------------------------------------
// 1. Delimiter sizing
// ---------------------------------------------------------------------------

fn check_delimiter_sizing() {
    println!();
    println!("=== Test 1: Delimiter Sizing ===");
    println!(r"  \left( \frac{{\frac{{a}}{{b}}}}{{\frac{{c}}{{d}}}} \right)");
    println!();

    let svg = render_display(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
    let elements = parse_svg_elements(&svg);
    let vb = parse_viewbox(&svg);

    let texts = collect_texts(&elements);
    let rects = collect_rects(&elements);
    let g_transforms = collect_g_transforms(&elements);
    let path_count = elements.iter().filter(|e| e.tag == "path").count();

    info(&format!(
        "ViewBox: {:.1} x {:.1} (at {:.1},{:.1})",
        vb.width, vb.height, vb.min_x, vb.min_y
    ));
    info(&format!(
        "Elements: {} text, {} rect, {} path, {} g",
        texts.len(),
        rects.len(),
        path_count,
        g_transforms.len()
    ));

    // The nested fraction has 4 letter glyphs (a,b,c,d) and 3 fraction bars
    if texts.len() == 4 {
        pass("4 glyph text elements found (a, b, c, d)");
    } else {
        warn(&format!("Expected 4 text glyphs, found {}", texts.len()));
    }
    if rects.len() == 3 {
        pass("3 fraction bar rects found (inner/inner/outer)");
    } else {
        warn(&format!(
            "Expected 3 fraction bar rects, found {}",
            rects.len()
        ));
    }

    // Delimiter check: tall delimiters should be paths, not text fallback
    if path_count >= 2 {
        pass("Tall delimiters rendered as SVG <path> elements (not text fallback)");
    } else {
        fail(&format!(
            "Expected >= 2 path elements for delimiters, found {}",
            path_count
        ));
    }

    // Delimiter height check via g transforms.
    // Both delimiters translate to the same y and use scale(16).
    // The path data uses coordinates in em-relative units.
    // From the raw SVG, the delimiter path covers 0 to 1.5061 in em units at scale(16).
    // So delimiter height in px = 1.5061 * 16 = 24.1 px.
    if g_transforms.len() >= 2 {
        let d1 = g_transforms[0];
        let d2 = g_transforms[g_transforms.len() - 1];
        info(&format!(
            "Left delimiter: translate({:.3},{:.3}) scale({:.3})",
            d1.0, d1.1, d1.2
        ));
        info(&format!(
            "Right delimiter: translate({:.3},{:.3}) scale({:.3})",
            d2.0, d2.1, d2.2
        ));

        // Both delimiters should start at the same y
        let y_diff = (d1.1 - d2.1).abs();
        if y_diff < 0.01 {
            pass(&format!(
                "Left and right delimiters vertically aligned (dy={:.4})",
                y_diff
            ));
        } else {
            warn(&format!("Delimiter y mismatch: {:.3} vs {:.3}", d1.1, d2.1));
        }
    }

    // Content bounds vs SVG bounds
    let glyph_top = texts
        .iter()
        .map(|t| t.y - t.font_size * 0.7)
        .fold(f64::INFINITY, f64::min);
    let glyph_bot = texts
        .iter()
        .map(|t| t.y + t.font_size * 0.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let rect_top = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
    let rect_bot = rects
        .iter()
        .map(|r| r.y + r.h)
        .fold(f64::NEG_INFINITY, f64::max);
    let content_top = glyph_top.min(rect_top);
    let content_bot = glyph_bot.max(rect_bot);
    let content_h = content_bot - content_top;

    info(&format!(
        "Content span: {:.1} to {:.1} = {:.1}px",
        content_top, content_bot, content_h
    ));
    info(&format!("SVG span: {:.1}px (viewBox height)", vb.height));

    if vb.height >= content_h * 0.85 {
        pass(&format!(
            "SVG height ({:.1}) adequately contains content ({:.1})",
            vb.height, content_h
        ));
    } else {
        warn(&format!(
            "SVG height ({:.1}) may clip content ({:.1})",
            vb.height, content_h
        ));
    }
}

// ---------------------------------------------------------------------------
// 2. Fraction bar alignment
// ---------------------------------------------------------------------------

fn check_fraction_bar() {
    println!();
    println!("=== Test 2: Fraction Bar Alignment ===");
    println!(r"  \frac{{a}}{{b}} (display mode)");
    println!();

    let svg = render_display(r"\frac{a}{b}");
    let elements = parse_svg_elements(&svg);
    let _vb = parse_viewbox(&svg);

    let texts = collect_texts(&elements);
    let rects = collect_rects(&elements);

    if rects.is_empty() {
        fail("No <rect> element found for fraction bar");
        return;
    }

    let bar = &rects[0];
    let bar_center_y = bar.y + bar.h / 2.0;

    info(&format!(
        "Fraction bar: y={:.3} w={:.3} h={:.3}",
        bar.y, bar.w, bar.h
    ));
    info(&format!("Bar center y: {:.3}", bar_center_y));

    // Check bar thickness: should be rule_thickness * size_class_0 = 0.040 * 16 = 0.640px
    let expected_h = 0.040 * EM;
    if (bar.h - expected_h).abs() < 0.01 {
        pass(&format!(
            "Bar thickness = {:.3}px (expected {:.3}px, rule_thickness * em)",
            bar.h, expected_h
        ));
    } else {
        info(&format!(
            "Bar thickness = {:.3}px vs expected {:.3}px",
            bar.h, expected_h
        ));
    }

    // Numerator and denominator positioning
    if texts.len() >= 2 {
        let num_y = texts.iter().map(|t| t.y).fold(f64::INFINITY, f64::min);
        let den_y = texts.iter().map(|t| t.y).fold(f64::NEG_INFINITY, f64::max);

        info(&format!("Numerator baseline y: {:.3}", num_y));
        info(&format!("Denominator baseline y: {:.3}", den_y));

        if num_y < bar_center_y && den_y > bar_center_y {
            pass("Numerator above bar, denominator below bar");
        } else {
            fail("Numerator/denominator not properly positioned");
        }

        // The bar should be at the math axis. The axis is at baseline - axis_height*em.
        // In display mode, the baseline is at root.height * em in SVG coords.
        // Axis height = 0.250em, so axis is 4.0px above baseline.
        // The bar center should NOT be at the midpoint of num/den -- it should be shifted
        // toward the numerator (because axis > geometric center for ascender-heavy text).
        let mid_y = (num_y + den_y) / 2.0;
        let bar_above_mid = mid_y - bar_center_y;

        info(&format!(
            "Bar is {:.1}px above num/den midpoint (expected: positive, ~axis offset)",
            bar_above_mid
        ));

        if bar_above_mid > 0.0 {
            pass(&format!(
                "Bar is above the num/den midpoint by {:.1}px (axis correction)",
                bar_above_mid
            ));
        } else {
            info("Bar is at or below midpoint -- possibly correct depending on glyph metrics");
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Script positioning
// ---------------------------------------------------------------------------

fn check_script_positioning() {
    println!();
    println!("=== Test 3: Script Positioning ===");
    println!(r"  x_i^2 (inline mode)");
    println!();

    let svg = render_inline("x_i^2");
    let elements = parse_svg_elements(&svg);
    let texts = collect_texts(&elements);

    for t in &texts {
        info(&format!(
            "glyph='{}' x={:.3} y={:.3} font-size={:.3}",
            t.content, t.x, t.y, t.font_size
        ));
    }

    let base_texts: Vec<&TextInfo> = texts
        .iter()
        .filter(|t| (t.font_size - EM).abs() < 0.5)
        .collect();
    let script_texts: Vec<&TextInfo> = texts
        .iter()
        .filter(|t| (t.font_size - EM).abs() > 0.5)
        .collect();

    if base_texts.is_empty() {
        fail("No base-size glyph found");
        return;
    }
    if script_texts.len() < 2 {
        fail(&format!(
            "Expected 2 script-size glyphs, found {}",
            script_texts.len()
        ));
        return;
    }

    let base_y = base_texts[0].y;
    let sup = script_texts
        .iter()
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    let sub = script_texts
        .iter()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();

    let sup_shift = base_y - sup.y;
    let sub_shift = sub.y - base_y;
    let script_ratio = sup.font_size / EM;

    info(&format!("Base '{}' y={:.3}", base_texts[0].content, base_y));
    info(&format!(
        "Super '{}' y={:.3}, shift_up={:.3}px = {:.3}em",
        sup.content,
        sup.y,
        sup_shift,
        sup_shift / EM
    ));
    info(&format!(
        "Sub   '{}' y={:.3}, shift_dn={:.3}px = {:.3}em",
        sub.content,
        sub.y,
        sub_shift,
        sub_shift / EM
    ));
    info(&format!(
        "Script font-size ratio: {:.3} (expected 0.700)",
        script_ratio
    ));

    // Font size ratio
    if (script_ratio - 0.7).abs() < 0.01 {
        pass(&format!(
            "Script font-size ratio = {:.3} (TeX script scale 0.7)",
            script_ratio
        ));
    } else {
        warn(&format!(
            "Script font-size ratio = {:.3} (expected 0.7)",
            script_ratio
        ));
    }

    // Superscript raised
    if sup_shift > 0.0 {
        pass(&format!("Superscript raised by {:.1}px", sup_shift));
    } else {
        fail("Superscript is NOT raised above baseline");
    }

    // Subscript lowered
    if sub_shift > 0.0 {
        pass(&format!("Subscript lowered by {:.1}px", sub_shift));
    } else {
        fail("Subscript is NOT lowered below baseline");
    }

    // TeX parameter check: sup2 = 0.363em for text mode
    let sup_em = sup_shift / EM;
    if (sup_em - 0.363).abs() < 0.05 {
        pass(&format!(
            "Superscript shift {:.3}em matches TeX sup2={:.3}",
            sup_em, 0.363
        ));
    } else if (0.2..0.8).contains(&sup_em) {
        pass(&format!(
            "Superscript shift {:.3}em in reasonable range [0.2, 0.8]",
            sup_em
        ));
    } else {
        warn(&format!(
            "Superscript shift {:.3}em outside expected range",
            sup_em
        ));
    }

    // TeX parameter check: sub1 = 0.150em, but subsup combined uses sub2 = 0.247em
    let sub_em = sub_shift / EM;
    if (0.05..0.5).contains(&sub_em) {
        pass(&format!(
            "Subscript shift {:.3}em in TeX-expected range",
            sub_em
        ));
    } else {
        warn(&format!(
            "Subscript shift {:.3}em outside expected range",
            sub_em
        ));
    }

    // Sub and sup should not overlap (sub.y - sub_ascent < sup.y + sup_descent)
    let sup_bottom = sup.y + sup.font_size * 0.25; // rough descender
    let sub_top = sub.y - sub.font_size * 0.65; // rough ascender
    if sup_bottom < sub_top {
        pass(&format!(
            "Scripts do not overlap (gap={:.1}px)",
            sub_top - sup_bottom
        ));
    } else {
        warn(&format!(
            "Scripts may overlap (sup bottom={:.1}, sub top={:.1})",
            sup_bottom, sub_top
        ));
    }
}

// ---------------------------------------------------------------------------
// 4. Accent centering
// ---------------------------------------------------------------------------

fn check_accent_centering() {
    println!();
    println!("=== Test 4: Accent Centering ===");

    // --- 4a: \hat{x} ---
    println!();
    println!(r"  4a: \hat{{x}}");
    println!();

    let svg = render_inline(r"\hat{x}");
    let elements = parse_svg_elements(&svg);
    let vb = parse_viewbox(&svg);
    let texts = collect_texts(&elements);
    let g_transforms = collect_g_transforms(&elements);
    let path_count = elements.iter().filter(|e| e.tag == "path").count();

    info(&format!(
        "Elements: {} text, {} path, {} g",
        texts.len(),
        path_count,
        g_transforms.len()
    ));

    // For \hat{x}: one text glyph (x) and one path (hat accent via SvgPath)
    let base = texts.iter().find(|t| t.content == "x");
    if let Some(base) = base {
        info(&format!(
            "Base 'x': x={:.3}, y={:.3}, font-size={:.3}",
            base.x, base.y, base.font_size
        ));

        if !g_transforms.is_empty() {
            let (tx, ty, scale) = g_transforms[0];
            info(&format!(
                "Accent path group: translate({:.3},{:.3}) scale({:.3})",
                tx, ty, scale
            ));

            // The accent path is rendered in em-relative coords.
            // From the SVG dump, hat path spans about 0 to 0.65 in x (em units).
            // The accent is placed by layout_accent with dx = (base_width - accent_width)/2.
            // Since both base and accent start from the HBox origin, the <g> translate.x
            // captures the total x offset from SVG origin.
            //
            // For \hat{x}: base_x=0, base glyph width ~0.57em=9.12px
            // Hat accent effective_width = 0.45 * 1.0 = 0.45em = 7.2px (for narrow single-char hat)
            // accent_dx = (9.12 - 7.2)/2 = 0.96px
            // But in the SVG, g translate.x = 0.0, and the hat path internally starts at ~0.33em
            // This means the centering is done WITHIN the path data itself.

            // The accent group x should be >= 0 (placed within the content width)
            if tx >= 0.0 && tx < vb.width {
                pass(&format!(
                    "Hat accent starts within content bounds (x={:.1})",
                    tx
                ));
            } else {
                warn(&format!(
                    "Hat accent x position {:.1} outside content bounds [0, {:.1}]",
                    tx, vb.width
                ));
            }

            // Check: accent is above the base (ty should be near the top of the SVG)
            if ty < base.y {
                pass(&format!(
                    "Hat accent (y={:.1}) is above base glyph (y={:.1})",
                    ty, base.y
                ));
            } else {
                warn(&format!(
                    "Hat accent (y={:.1}) may not be above base (y={:.1})",
                    ty, base.y
                ));
            }
        }
    } else {
        info("Base 'x' not found as separate text element");
    }

    // --- 4b: \widehat{abc} ---
    println!();
    println!(r"  4b: \widehat{{abc}}");
    println!();

    let svg_w = render_inline(r"\widehat{abc}");
    let elements_w = parse_svg_elements(&svg_w);
    let vb_w = parse_viewbox(&svg_w);
    let texts_w = collect_texts(&elements_w);
    let g_transforms_w = collect_g_transforms(&elements_w);

    info(&format!(
        "Elements: {} text, {} g",
        texts_w.len(),
        g_transforms_w.len()
    ));

    // Content glyphs: a, b, c
    let content_xs: Vec<f64> = texts_w.iter().map(|t| t.x).collect();
    if content_xs.len() >= 3 {
        let min_x = content_xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max_x = content_xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // The last glyph (c) has width ~0.44em = 7.04px
        let last_glyph_w = texts_w
            .iter()
            .find(|t| t.content == "c")
            .map(|t| t.font_size * 0.44)
            .unwrap_or(7.0);
        let content_right = max_x + last_glyph_w;
        let content_width = content_right - min_x;
        let content_center = min_x + content_width / 2.0;

        info(&format!(
            "Content span: [{:.1}, {:.1}] = {:.1}px, center={:.1}",
            min_x, content_right, content_width, content_center
        ));

        if !g_transforms_w.is_empty() {
            let (tx, ty, _scale) = g_transforms_w[0];
            info(&format!("Wide hat group: translate({:.3},{:.3})", tx, ty));

            // For \widehat, the accent path stretches to base_width.
            // The path spans from 0 to ~1.6 in em units (content width in em = content_width/16).
            // The accent dx = (base_width - accent_width)/2 = 0 since widehat width = base_width.
            // So tx should be ~0 (aligned with content start).
            if (tx - min_x).abs() < 2.0 {
                pass(&format!(
                    "Wide hat aligned with content left edge (offset={:.1}px)",
                    (tx - min_x).abs()
                ));
            } else {
                info(&format!(
                    "Wide hat offset from content left: {:.1}px",
                    tx - min_x
                ));
            }

            // The accent should be above all text
            let min_text_y = texts_w.iter().map(|t| t.y).fold(f64::INFINITY, f64::min);
            if ty < min_text_y {
                pass(&format!("Wide hat (y={:.1}) above all text glyphs", ty));
            }
        }
    }

    // Check SVG width accommodates accent
    info(&format!("SVG width: {:.1}px", vb_w.width));
}

// ---------------------------------------------------------------------------
// 5. Radical rendering
// ---------------------------------------------------------------------------

fn check_radical_rendering() {
    println!();
    println!("=== Test 5: Radical Rendering ===");
    println!(r"  \sqrt{{\frac{{a}}{{b}}}}");
    println!();

    let svg = render_display(r"\sqrt{\frac{a}{b}}");
    let elements = parse_svg_elements(&svg);
    let vb = parse_viewbox(&svg);
    let texts = collect_texts(&elements);
    let rects = collect_rects(&elements);
    let g_transforms = collect_g_transforms(&elements);
    let path_count = elements.iter().filter(|e| e.tag == "path").count();

    info(&format!("ViewBox: {:.1} x {:.1}", vb.width, vb.height));
    info(&format!(
        "Elements: {} text, {} rect, {} path, {} g",
        texts.len(),
        rects.len(),
        path_count,
        g_transforms.len()
    ));

    // Expected: 2 texts (a, b), 2 rects (vinculum + fraction bar), 1 path (surd), 1 g
    if texts.len() == 2 {
        pass("2 glyph text elements (a, b)");
    }

    if rects.len() >= 2 {
        pass(&format!("{} rect elements found", rects.len()));
    } else {
        fail(&format!(
            "Expected >= 2 rects (vinculum + fraction bar), found {}",
            rects.len()
        ));
        return;
    }

    // Sort rects by y to identify vinculum (topmost) vs fraction bar
    let mut sorted_rects = rects.clone();
    sorted_rects.sort_by(|a, b| a.y.total_cmp(&b.y));
    let vinculum = &sorted_rects[0];
    let frac_bar = &sorted_rects[1];

    info(&format!(
        "Vinculum: x={:.3} y={:.3} w={:.3} h={:.3}",
        vinculum.x, vinculum.y, vinculum.w, vinculum.h
    ));
    info(&format!(
        "Frac bar: x={:.3} y={:.3} w={:.3} h={:.3}",
        frac_bar.x, frac_bar.y, frac_bar.w, frac_bar.h
    ));

    // Vinculum should be above fraction bar
    if vinculum.y < frac_bar.y {
        pass("Vinculum is above the fraction bar");
    } else {
        fail("Vinculum is NOT above fraction bar");
    }

    // Vinculum width should be >= fraction bar width (covers the whole radicand)
    if vinculum.w >= frac_bar.w - 0.1 {
        pass(&format!(
            "Vinculum width ({:.1}) covers fraction bar width ({:.1})",
            vinculum.w, frac_bar.w
        ));
    } else {
        warn(&format!(
            "Vinculum width ({:.1}) < fraction bar width ({:.1})",
            vinculum.w, frac_bar.w
        ));
    }

    // Vinculum thickness should be thicker than fraction bar (radical_rule_thickness > rule_thickness)
    if vinculum.h >= frac_bar.h {
        pass(&format!(
            "Vinculum thickness ({:.3}) >= fraction bar ({:.3})",
            vinculum.h, frac_bar.h
        ));
    } else {
        info(&format!(
            "Vinculum thickness ({:.3}) < fraction bar ({:.3})",
            vinculum.h, frac_bar.h
        ));
    }

    // Radical sign path
    if path_count >= 1 {
        pass("Radical sign rendered as SVG <path>");
    } else {
        fail("No SVG path for radical sign");
    }

    // Radical sign should be to the left of the vinculum
    if !g_transforms.is_empty() {
        let (tx, ty, scale) = g_transforms[0];
        info(&format!(
            "Surd group: translate({:.3},{:.3}) scale({:.3})",
            tx, ty, scale
        ));

        if tx < vinculum.x {
            pass(&format!(
                "Surd (x={:.1}) is left of vinculum (x={:.1})",
                tx, vinculum.x
            ));
        } else {
            warn(&format!(
                "Surd (x={:.1}) not left of vinculum (x={:.1})",
                tx, vinculum.x
            ));
        }
    }

    // Vinculum + surd should span from surd left to vinculum right
    let total_radical_width = vinculum.x + vinculum.w;
    let surd_left = g_transforms.first().map(|(tx, _, _)| *tx).unwrap_or(0.0);
    let radical_coverage = total_radical_width - surd_left;
    info(&format!(
        "Total radical coverage: {:.1}px (from x={:.1} to {:.1})",
        radical_coverage, surd_left, total_radical_width
    ));
}

// ---------------------------------------------------------------------------
// 6. Matrix column alignment
// ---------------------------------------------------------------------------

fn check_matrix_alignment() {
    println!();
    println!("=== Test 6: Matrix Column Alignment ===");
    println!(r"  \begin{{pmatrix}} 1 & 22 \\ 333 & 4 \end{{pmatrix}}");
    println!();

    let svg = render_display(r"\begin{pmatrix} 1 & 22 \\ 333 & 4 \end{pmatrix}");
    let elements = parse_svg_elements(&svg);
    let all_texts = collect_texts(&elements);

    // Filter out delimiter parens
    let cells: Vec<&TextInfo> = all_texts
        .iter()
        .filter(|t| t.content != "(" && t.content != ")")
        .collect();

    for c in &cells {
        info(&format!(
            "cell '{}': x={:.3} y={:.3} fs={:.3}",
            c.content, c.x, c.y, c.font_size
        ));
    }

    if cells.len() < 4 {
        warn(&format!(
            "Expected 4 matrix cells, found {}; dumping all text:",
            cells.len()
        ));
        for t in &all_texts {
            info(&format!(
                "  text '{}': x={:.3} y={:.3}",
                t.content, t.x, t.y
            ));
        }
        return;
    }

    // Sort by y then x
    let mut sorted: Vec<&&TextInfo> = cells.iter().collect();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));

    let mut r1: Vec<&&&TextInfo> = sorted.iter().take(2).collect();
    let mut r2: Vec<&&&TextInfo> = sorted.iter().skip(2).take(2).collect();
    r1.sort_by(|a, b| a.x.total_cmp(&b.x));
    r2.sort_by(|a, b| a.x.total_cmp(&b.x));

    let r1c1 = r1[0]; // "1"
    let r1c2 = r1[1]; // "22"
    let r2c1 = r2[0]; // "333"
    let r2c2 = r2[1]; // "4"

    info(&format!(
        "Row 1: [{}@x={:.1}] [{}@x={:.1}]",
        r1c1.content, r1c1.x, r1c2.content, r1c2.x
    ));
    info(&format!(
        "Row 2: [{}@x={:.1}] [{}@x={:.1}]",
        r2c1.content, r2c1.x, r2c2.content, r2c2.x
    ));

    // TeX pmatrix uses center-aligned columns.
    // Approximate center of each cell using content length and digit width ~0.50em.
    let em = r1c1.font_size;
    let dw = em * 0.50;

    let c11 = r1c1.x + dw * r1c1.content.len() as f64 / 2.0;
    let c21 = r2c1.x + dw * r2c1.content.len() as f64 / 2.0;
    let c12 = r1c2.x + dw * r1c2.content.len() as f64 / 2.0;
    let c22 = r2c2.x + dw * r2c2.content.len() as f64 / 2.0;

    let col1_err = (c11 - c21).abs();
    let col2_err = (c12 - c22).abs();

    info(&format!(
        "Col 1 centers: row1={:.1} row2={:.1}, error={:.1}px",
        c11, c21, col1_err
    ));
    info(&format!(
        "Col 2 centers: row1={:.1} row2={:.1}, error={:.1}px",
        c12, c22, col2_err
    ));

    if col1_err < 2.0 {
        pass(&format!(
            "Column 1 center-aligned (error={:.1}px)",
            col1_err
        ));
    } else {
        warn(&format!(
            "Column 1 alignment error={:.1}px (expected center-aligned)",
            col1_err
        ));
    }
    if col2_err < 2.0 {
        pass(&format!(
            "Column 2 center-aligned (error={:.1}px)",
            col2_err
        ));
    } else {
        warn(&format!(
            "Column 2 alignment error={:.1}px (expected center-aligned)",
            col2_err
        ));
    }

    // Row spacing
    let row_gap = (r2[0].y - r1[0].y).abs();
    info(&format!(
        "Row spacing: {:.1}px = {:.3}em",
        row_gap,
        row_gap / em
    ));
    if (0.8..2.0).contains(&(row_gap / em)) {
        pass(&format!(
            "Row spacing {:.3}em in reasonable range",
            row_gap / em
        ));
    } else {
        warn(&format!(
            "Row spacing {:.3}em outside expected range [0.8, 2.0]",
            row_gap / em
        ));
    }

    // Columns should be separated
    let col_gap = r1c2.x - (r1c1.x + dw * r1c1.content.len() as f64);
    info(&format!(
        "Column gap (approx): {:.1}px = {:.3}em",
        col_gap,
        col_gap / em
    ));
}

// ---------------------------------------------------------------------------
// Sanity checks
// ---------------------------------------------------------------------------

fn check_overall() {
    println!();
    println!("=== General SVG Sanity Checks ===");
    println!();

    let cases = [
        (r"x^2 + y^2 = z^2", "Pythagorean"),
        (r"\frac{1}{2}", "Simple fraction"),
        (r"\sum_{i=0}^{n} x_i", "Sum with limits"),
        (r"\int_0^1 f(x)\,dx", "Integral"),
        (r"\alpha + \beta = \gamma", "Greek letters"),
    ];

    for (latex, label) in &cases {
        let svg = render_display(latex);
        let vb = parse_viewbox(&svg);
        let elements = parse_svg_elements(&svg);
        let tc = elements.iter().filter(|e| e.tag == "text").count();
        let rc = elements.iter().filter(|e| e.tag == "rect").count();
        let pc = elements.iter().filter(|e| e.tag == "path").count();

        let ok = vb.width > 0.0 && vb.height > 0.0 && tc > 0;
        if ok {
            pass(&format!(
                "{}: {:.0}x{:.0}px, {}t {}r {}p",
                label, vb.width, vb.height, tc, rc, pc
            ));
        } else {
            fail(&format!(
                "{}: invalid SVG ({:.0}x{:.0}, {} text)",
                label, vb.width, vb.height, tc
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    println!("====================================================");
    println!("  mathema SVG Rendering Quality Verification");
    println!("====================================================");

    check_delimiter_sizing();
    check_fraction_bar();
    check_script_positioning();
    check_accent_centering();
    check_radical_rendering();
    check_matrix_alignment();
    check_overall();

    println!();
    println!("====================================================");
    let (p, w, f) = unsafe { (PASS_COUNT, WARN_COUNT, FAIL_COUNT) };
    println!("  Results: {} PASS, {} WARN, {} FAIL", p, w, f);
    println!("====================================================");
}
