//! Both layouts and all emitters draw a formula alike.

#![cfg(feature = "raster")]

use mathema::ast::compact::CompactExpr;
use mathema::ast::*;
use mathema::render::arena::LayoutArena;
use mathema::render::raster::{RasterOptions, render_to_image, render_to_image_compact};
use mathema::render::{
    RenderOptions, render_to_svg_standalone, render_to_svg_standalone_compact_arena,
};

fn png(expr: &MathExpr) -> tiny_skia::Pixmap {
    let options = RasterOptions {
        scale: 4.0,
        background: Some([255, 255, 255, 255]),
        ..RasterOptions::default()
    };
    let heap = render_to_image(expr, &options).unwrap();
    let compact = CompactExpr::from_math_expr(expr).unwrap();
    assert_eq!(heap, render_to_image_compact(&compact, &options).unwrap());
    tiny_skia::Pixmap::decode_png(&heap).unwrap()
}

#[test]
fn heap_and_arena_are_drawn_alike() {
    for latex in [
        r"\sin x+\text{abc}+\textit{diff}+12.5",
        r"\text{∫x}",
        r"\left(\frac{a}{b}\right)+\sqrt[3]{x}",
        r"\boxed{x}+\colorbox{yellow}{y}+\fcolorbox{red}{blue}{z}",
        r"\mathbb{\hat{I}}+\pmb{\frac{x^2}{x_1}}+\mathcal{\frac{A}{B}}",
        r"\hat f+\vec V+\bar M+\int_a^b f",
    ] {
        let expr = mathema::from_latex(latex).unwrap().expr();
        let options = RenderOptions::default();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let mut arena = LayoutArena::new();
        assert_eq!(
            render_to_svg_standalone(&expr, &options).unwrap(),
            render_to_svg_standalone_compact_arena(&compact, &options, &mut arena).unwrap(),
            "{latex}"
        );
        let mut arena = LayoutArena::new();
        assert_eq!(
            mathema::render::render_to_svg(&expr, &options).unwrap(),
            mathema::render::render_to_svg_compact_arena(&compact, &options, &mut arena).unwrap(),
            "{latex}"
        );
        png(&expr);
    }
}

/// The frame of a box is drawn inside it, and a background alone has none.
#[test]
fn frames_are_inside_their_boxes() {
    let colors = |latex: &str| {
        let image = png(&mathema::from_latex(latex).unwrap().expr());
        let (width, height) = (image.width(), image.height());
        let middle = height / 2;
        let row: Vec<_> = (0..width)
            .map(|x| image.pixel(x, middle).unwrap())
            .collect();
        let dark = row.iter().position(|p| p.red() < 64 && p.green() < 64);
        let yellow = row.iter().position(|p| p.red() > 200 && p.blue() < 64);
        (dark, yellow)
    };
    let (dark, yellow) = colors(r"\colorbox{yellow}{\phantom{xxxx}}");
    assert!(dark.is_none() && yellow.is_some(), "{dark:?} {yellow:?}");
    let left = yellow.unwrap();
    let (dark, yellow) = colors(r"\fcolorbox{black}{yellow}{\phantom{xxxx}}");
    let (dark, yellow) = (dark.unwrap(), yellow.unwrap());
    assert!(left <= dark && dark < yellow, "{left} {dark} {yellow}");
}

#[test]
fn colons_and_slashes_have_one_class_in_every_reader() {
    let symbols = |expr: MathExpr| -> Vec<(SymbolKind, String)> {
        expr.nodes
            .into_iter()
            .filter_map(|node| match node {
                MathNode::Symbol(kind, text) => Some((kind, text)),
                _ => None,
            })
            .collect()
    };
    let expected = vec![
        (SymbolKind::Relation, ":".to_owned()),
        (SymbolKind::Ordinary, "/".to_owned()),
    ];
    let mathml = "<math><mi>a</mi><mo>:</mo><mi>b</mi><mo>/</mo><mi>c</mi></math>";
    let omml = "<m:oMath xmlns:m='http://schemas.openxmlformats.org/officeDocument/2006/math'>\
                <m:r><m:t>a:b/c</m:t></m:r></m:oMath>";
    assert_eq!(
        symbols(mathema::from_latex("a:b/c").unwrap().expr()),
        expected
    );
    assert_eq!(
        symbols(mathema::from_mathml(mathml).unwrap().expr()),
        expected
    );
    assert_eq!(symbols(mathema::from_omml(omml).unwrap().expr()), expected);
    let linear = mathema::from_unicode_math("a:b\\/c").unwrap().expr();
    assert_eq!(symbols(linear), expected);

    use mathema::writer::Writer;
    for (kind, text, latex) in [
        (SymbolKind::Relation, ":", "a: b"),
        (SymbolKind::Ordinary, ":", "a{:}b"),
        (SymbolKind::Punctuation, ":", r"a\colon b"),
        (SymbolKind::BinaryOp, ":", r"a\mathbin{:} b"),
        (SymbolKind::Ordinary, "/", "a/b"),
        (SymbolKind::BinaryOp, "/", r"a\mathbin{/} b"),
        (SymbolKind::Relation, "/", r"a\mathrel{/} b"),
    ] {
        let id = |s: &str| MathNode::Identifier(s.into());
        let expr = MathExpr::inline(vec![id("a"), MathNode::Symbol(kind, text.into()), id("b")]);
        let written =
            mathema::writer::latex::LatexWriter::write(&expr, &mathema::Config::default()).unwrap();
        assert_eq!(written, latex, "{kind:?} {text}");
        let again = mathema::from_latex(&written).unwrap().to_latex().unwrap();
        assert_eq!(again, latex);
    }
    // An ordinary colon stays one wherever it is.
    for input in [
        r"x_{{:}}",
        r"x^{{:}}_{{:}}",
        r"{:}_{{:}}^{\prime}",
        r"\frac{{:}}{{:}}",
        r"\hat{{:}}",
        r"\sqrt{{:}}",
        r"\sum_{{:}}^{{:}}{:}",
        r"\overset{{:}}{{:}}",
        r"\mathrel{{:}}",
        r"\textcolor{red}{{:}}",
        r"\left{:}",
        r"\operatorname{a{:}b}",
        r"\char`:_\char`:^\char`:",
        "\\char'72='_\n\\char'72='_'-'4",
    ] {
        let Ok(read) = mathema::from_latex(input) else {
            continue;
        };
        let written = read.to_latex().unwrap();
        let again = mathema::from_latex(&written).unwrap().to_latex().unwrap();
        assert_eq!(again, written, "{input}");
    }
}
