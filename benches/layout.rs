use criterion::{Criterion, criterion_group, criterion_main};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::RenderOptions;
use mathema::render::arena::LayoutArena;
use mathema::render::emit::{emit_svg, emit_svg_arena};
use mathema::render::layout::{LayoutBuilder, layout_expr};
use mathema::render::style::MathStyle;

const SIMPLE: &str = "x^2 + 1";
const QUADRATIC: &str = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
const NESTED: &str = "\\frac{\\frac{a}{b}}{\\frac{c}{d}}";
const SUM: &str = "\\sum_{i=0}^{n} \\frac{1}{i^2}";
const NAVIER_STOKES: &str = "\\rho\\left(\\frac{\\partial \\mathbf{u}}{\\partial t} + \\mathbf{u} \\cdot \\nabla \\mathbf{u}\\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{u} + \\mathbf{f}";

fn bench_layout_heap(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("layout_heap");

    for (name, latex) in [
        ("simple", SIMPLE),
        ("quadratic", QUADRATIC),
        ("nested", NESTED),
        ("sum", SUM),
        ("navier_stokes", NAVIER_STOKES),
    ] {
        let expr = LatexReader::parse_compact(latex, &config).unwrap().expr;
        let mexpr = expr.to_math_expr();
        group.bench_function(name, |b| {
            b.iter(|| layout_expr(std::hint::black_box(&mexpr), MathStyle::Display, &options))
        });
    }
    group.finish();
}

fn bench_layout_arena(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("layout_arena");

    for (name, latex) in [
        ("simple", SIMPLE),
        ("quadratic", QUADRATIC),
        ("nested", NESTED),
        ("sum", SUM),
        ("navier_stokes", NAVIER_STOKES),
    ] {
        let expr = LatexReader::parse_compact(latex, &config).unwrap().expr;
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        group.bench_function(name, |b| {
            let mut arena = LayoutArena::new();
            b.iter(|| {
                arena.clear();
                builder.layout(std::hint::black_box(&mut arena), std::hint::black_box(&expr))
            })
        });
    }
    group.finish();
}

fn bench_layout_arena_cold(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("layout_arena_cold");

    for (name, latex) in [
        ("simple", SIMPLE),
        ("quadratic", QUADRATIC),
        ("navier_stokes", NAVIER_STOKES),
    ] {
        let expr = LatexReader::parse_compact(latex, &config).unwrap().expr;
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut arena = LayoutArena::new();
                builder.layout(std::hint::black_box(&mut arena), std::hint::black_box(&expr))
            })
        });
    }
    group.finish();
}

fn bench_full_pipeline_arena(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("full_pipeline_arena");

    for (name, latex) in [("quadratic", QUADRATIC), ("navier_stokes", NAVIER_STOKES)] {
        let expr = LatexReader::parse_compact(latex, &config).unwrap().expr;
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        group.bench_function(name, |b| {
            let mut arena = LayoutArena::new();
            b.iter(|| {
                arena.clear();
                let root = builder.layout(&mut arena, std::hint::black_box(&expr));
                emit_svg_arena(std::hint::black_box(&arena), root, &options)
            })
        });
    }
    group.finish();
}

fn bench_full_pipeline_heap(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("full_pipeline_heap");

    for (name, latex) in [("quadratic", QUADRATIC), ("navier_stokes", NAVIER_STOKES)] {
        let expr = LatexReader::parse_compact(latex, &config).unwrap().expr;
        let mexpr = expr.to_math_expr();
        group.bench_function(name, |b| {
            b.iter(|| {
                let root = layout_expr(std::hint::black_box(&mexpr), MathStyle::Display, &options);
                emit_svg(std::hint::black_box(&root), &options)
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_layout_heap,
    bench_layout_arena,
    bench_layout_arena_cold,
    bench_full_pipeline_arena,
    bench_full_pipeline_heap,
);
criterion_main!(benches);
