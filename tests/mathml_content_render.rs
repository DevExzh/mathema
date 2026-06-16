//! End-to-end Content MathML → render checks.
//!
//! These exercise the full pipeline (parse Content MathML → presentation AST →
//! layout → SVG) for representative constructs from the W3C MathML test suite,
//! ensuring nothing is silently dropped and the renderer emits real geometry.
#![cfg(feature = "render")]

use mathema::error::{Config, Strictness};
use mathema::{ast::DisplayStyle, from_mathml_with};

fn render(mml: &str) -> String {
    let cfg = Config {
        strictness: Strictness::Lenient,
        display: DisplayStyle::Block,
    };
    from_mathml_with(mml, &cfg).unwrap().to_svg().unwrap()
}

fn latex(mml: &str) -> String {
    let cfg = Config {
        strictness: Strictness::Lenient,
        display: DisplayStyle::Block,
    };
    from_mathml_with(mml, &cfg).unwrap().to_latex().unwrap()
}

#[test]
fn content_samples_render_nonempty_svg() {
    let cases = [
        "<math><apply><ln/><ci>a</ci></apply></math>",
        "<math><apply><factorial/><cn>3</cn></apply></math>",
        "<math><apply><power/><ci>x</ci><cn>2</cn></apply></math>",
        "<math><apply><divide/><ci>a</ci><ci>b</ci></apply></math>",
        "<math><apply><root/><degree><cn>3</cn></degree><ci>x</ci></apply></math>",
        "<math><apply><sin/><apply><plus/><ci>x</ci><ci>y</ci></apply></apply></math>",
        "<math><apply><eq/><ci>a</ci><ci>b</ci></apply></math>",
        "<math><apply><in/><ci>x</ci><ci>S</ci></apply></math>",
        "<math><apply><sum/><bvar><ci>i</ci></bvar><lowlimit><cn>1</cn></lowlimit>\
         <uplimit><ci>n</ci></uplimit><ci>i</ci></apply></math>",
        "<math><apply><int/><bvar><ci>x</ci></bvar><lowlimit><cn>0</cn></lowlimit>\
         <uplimit><cn>1</cn></uplimit><ci>f</ci></apply></math>",
        "<math><apply><diff/><bvar><ci>x</ci></bvar><ci>f</ci></apply></math>",
        "<math><vector><cn>1</cn><cn>2</cn><cn>3</cn></vector></math>",
        "<math><matrix><matrixrow><cn>1</cn><cn>0</cn></matrixrow>\
         <matrixrow><cn>0</cn><cn>1</cn></matrixrow></matrix></math>",
    ];
    for mml in cases {
        let svg = render(mml);
        assert!(svg.contains("<svg"), "no svg for {mml}");
        // A meaningful render contains drawn geometry (glyph paths/uses or text).
        assert!(
            svg.contains("<path") || svg.contains("<use") || svg.contains("<text"),
            "empty render for {mml}: {svg}"
        );
    }
}

#[test]
fn invisible_operators_do_not_widen_output() {
    // ApplyFunction / InvisibleTimes must be zero-width: "a⁢b" == "ab".
    let with = render("<math><mi>a</mi><mo>&InvisibleTimes;</mo><mi>b</mi></math>");
    let without = render("<math><mi>a</mi><mi>b</mi></math>");
    let w_with = extract_svg_width(&with);
    let w_without = extract_svg_width(&without);
    assert!(
        (w_with - w_without).abs() < 0.5,
        "invisible times widened output: {w_with} vs {w_without}"
    );
}

#[test]
fn named_entities_and_string_elements() {
    let a = latex("<math><mi>&alpha;</mi></math>");
    assert!(a.contains('\u{03B1}') || a.contains("\\alpha"), "{a}");
    // <ms> is a quoted string literal.
    assert!(latex("<math><ms>hi</ms></math>").contains("\"hi\""));
}

fn extract_svg_width(svg: &str) -> f64 {
    // Parse width="...." from the root <svg> tag.
    let idx = svg.find("width=\"").map(|i| i + 7).unwrap_or(0);
    let rest = &svg[idx..];
    let end = rest.find(['"', 'p', 'e']).unwrap_or(rest.len());
    rest[..end].parse().unwrap_or(0.0)
}
