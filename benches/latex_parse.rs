use criterion::{Criterion, criterion_group, criterion_main};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

const SIMPLE: &str = "x^2 + 1";
const QUADRATIC: &str = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
const NESTED: &str = "\\frac{\\frac{a}{b}}{\\frac{c}{d}}";
const SUM: &str = "\\sum_{i=0}^{n} \\frac{1}{i^2}";
const NAVIER_STOKES: &str = "\\rho \\left( \\frac{\\partial \\mathbf{v}}{\\partial t} + \\mathbf{v} \\cdot \\nabla \\mathbf{v} \\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{v} + \\mathbf{f}";

fn bench_latex_parse_focused(c: &mut Criterion) {
    let config = Config::default();
    let mut group = c.benchmark_group("latex_parse_focused");

    group.bench_function("simple", |b| {
        b.iter(|| LatexReader::parse_compact(std::hint::black_box(SIMPLE), &config))
    });
    group.bench_function("quadratic", |b| {
        b.iter(|| LatexReader::parse_compact(std::hint::black_box(QUADRATIC), &config))
    });
    group.bench_function("nested_frac", |b| {
        b.iter(|| LatexReader::parse_compact(std::hint::black_box(NESTED), &config))
    });
    group.bench_function("sum", |b| {
        b.iter(|| LatexReader::parse_compact(std::hint::black_box(SUM), &config))
    });
    group.bench_function("navier_stokes", |b| {
        b.iter(|| LatexReader::parse_compact(std::hint::black_box(NAVIER_STOKES), &config))
    });

    group.finish();
}

criterion_group!(benches, bench_latex_parse_focused);
criterion_main!(benches);
