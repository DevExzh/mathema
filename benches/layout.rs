use criterion::{Criterion, criterion_group, criterion_main};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::render::RenderOptions;
use mathema::render::arena::LayoutArena;
use mathema::render::emit::{emit_svg, emit_svg_arena};
use mathema::render::layout::{LayoutBuilder, layout_expr};
use mathema::render::style::MathStyle;
use std::fmt::Write as _;

const SIMPLE: &str = "x^2 + 1";
const QUADRATIC: &str = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
const NESTED: &str = "\\frac{\\frac{a}{b}}{\\frac{c}{d}}";
const SUM: &str = "\\sum_{i=0}^{n} \\frac{1}{i^2}";
const NAVIER_STOKES: &str = "\\rho\\left(\\frac{\\partial \\mathbf{u}}{\\partial t} + \\mathbf{u} \\cdot \\nabla \\mathbf{u}\\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{u} + \\mathbf{f}";

fn nested_formula(command: &str, depth: usize) -> String {
    let mut formula = String::from("x");
    for _ in 0..depth {
        formula = if command == "frac" {
            format!("\\frac{{{formula}}}{{x}}")
        } else {
            format!("\\{command}{{{formula}}}")
        };
    }
    formula
}

fn flat_row(terms: usize) -> String {
    let mut formula = String::with_capacity(terms * 3);
    for index in 0..terms {
        if index != 0 {
            formula.push('+');
        }
        write!(formula, "x_{{{index}}}").expect("writing to String cannot fail");
    }
    formula
}

fn matrix(size: usize) -> String {
    let mut formula = String::from("\\begin{matrix}");
    for row in 0..size {
        if row != 0 {
            formula.push_str("\\\\");
        }
        for column in 0..size {
            if column != 0 {
                formula.push('&');
            }
            write!(formula, "x_{{{row}{column}}}").expect("writing to String cannot fail");
        }
    }
    formula.push_str("\\end{matrix}");
    formula
}

fn bench_depth_and_shape_sweep(c: &mut Criterion) {
    let config = Config::default();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("layout_depth_and_shape");

    for (name, latex) in [
        ("frac_depth4", nested_formula("frac", 4)),
        ("frac_depth8", nested_formula("frac", 8)),
        ("frac_depth12", nested_formula("frac", 12)),
        ("sqrt_depth4", nested_formula("sqrt", 4)),
        ("sqrt_depth8", nested_formula("sqrt", 8)),
        ("sqrt_depth12", nested_formula("sqrt", 12)),
        ("row_terms100", flat_row(100)),
        ("row_terms1000", flat_row(1_000)),
        ("matrix2", matrix(2)),
        ("matrix10", matrix(10)),
    ] {
        let expr = LatexReader::parse_compact(&latex, &config)
            .unwrap_or_else(|error| panic!("{name} fixture did not parse: {error:?}"))
            .expr;
        let builder = LayoutBuilder::new(MathStyle::Display, &options);
        group.bench_function(name, |b| {
            let mut arena = LayoutArena::new();
            b.iter(|| {
                arena.clear();
                let root = builder.layout(&mut arena, std::hint::black_box(&expr));
                std::hint::black_box(emit_svg_arena(std::hint::black_box(&arena), root, &options))
            })
        });
    }
    group.finish();
}

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
                builder.layout(
                    std::hint::black_box(&mut arena),
                    std::hint::black_box(&expr),
                )
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
                builder.layout(
                    std::hint::black_box(&mut arena),
                    std::hint::black_box(&expr),
                )
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
    bench_depth_and_shape_sweep,
);
criterion_main!(benches);
