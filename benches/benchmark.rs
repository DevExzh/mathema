use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::writer::Writer;
use mathema::reader::latex::LatexReader;
use mathema::writer::latex::LatexWriter;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::mtef::MtefWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;

const SIMPLE: &str = "x^2 + 1";
const QUADRATIC: &str = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
const NESTED: &str = "\\frac{\\frac{a}{b}}{\\frac{c}{d}}";
const SUM: &str = "\\sum_{i=0}^{n} \\frac{1}{i^2}";

fn bench_latex_parse(c: &mut Criterion) {
    let config = Config::default();
    let mut group = c.benchmark_group("latex_parse");
    group.bench_function("simple", |b| {
        b.iter(|| LatexReader::parse(black_box(SIMPLE), &config))
    });
    group.bench_function("quadratic", |b| {
        b.iter(|| LatexReader::parse(black_box(QUADRATIC), &config))
    });
    group.bench_function("nested_frac", |b| {
        b.iter(|| LatexReader::parse(black_box(NESTED), &config))
    });
    group.bench_function("sum", |b| {
        b.iter(|| LatexReader::parse(black_box(SUM), &config))
    });
    group.finish();
}

fn bench_latex_write(c: &mut Criterion) {
    let config = Config::default();
    let simple_ast = LatexReader::parse(SIMPLE, &config).unwrap().expr;
    let quad_ast = LatexReader::parse(QUADRATIC, &config).unwrap().expr;

    let mut group = c.benchmark_group("latex_write");
    group.bench_function("simple", |b| {
        b.iter(|| LatexWriter::write(black_box(&simple_ast), &config))
    });
    group.bench_function("quadratic", |b| {
        b.iter(|| LatexWriter::write(black_box(&quad_ast), &config))
    });
    group.finish();
}

fn bench_mathml_write(c: &mut Criterion) {
    let config = Config::default();
    let simple_ast = LatexReader::parse(SIMPLE, &config).unwrap().expr;
    let quad_ast = LatexReader::parse(QUADRATIC, &config).unwrap().expr;

    let mut group = c.benchmark_group("mathml_write");
    group.bench_function("simple", |b| {
        b.iter(|| MathmlWriter::write(black_box(&simple_ast), &config))
    });
    group.bench_function("quadratic", |b| {
        b.iter(|| MathmlWriter::write(black_box(&quad_ast), &config))
    });
    group.finish();
}

fn bench_mathml_parse(c: &mut Criterion) {
    let config = Config::default();
    let simple_mathml = MathmlWriter::write(
        &LatexReader::parse(SIMPLE, &config).unwrap().expr,
        &config,
    ).unwrap();
    let quad_mathml = MathmlWriter::write(
        &LatexReader::parse(QUADRATIC, &config).unwrap().expr,
        &config,
    ).unwrap();

    let mut group = c.benchmark_group("mathml_parse");
    group.bench_function("simple", |b| {
        b.iter(|| MathmlReader::parse(black_box(&simple_mathml), &config))
    });
    group.bench_function("quadratic", |b| {
        b.iter(|| MathmlReader::parse(black_box(&quad_mathml), &config))
    });
    group.finish();
}

fn bench_cross_format(c: &mut Criterion) {
    let config = Config::default();
    let quad_ast = LatexReader::parse(QUADRATIC, &config).unwrap().expr;

    let mut group = c.benchmark_group("cross_format_write");
    group.bench_function("to_omml", |b| {
        b.iter(|| OmmlWriter::write(black_box(&quad_ast), &config))
    });
    group.bench_function("to_mtef", |b| {
        b.iter(|| MtefWriter::write(black_box(&quad_ast), &config))
    });
    group.bench_function("to_unicode_math", |b| {
        b.iter(|| UnicodeMathWriter::write(black_box(&quad_ast), &config))
    });
    group.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");
    group.bench_function("latex_to_mathml", |b| {
        b.iter(|| mathema::convert(black_box(QUADRATIC), mathema::Format::LaTeX, mathema::Format::MathML))
    });
    group.bench_function("latex_to_omml", |b| {
        b.iter(|| mathema::convert(black_box(QUADRATIC), mathema::Format::LaTeX, mathema::Format::Omml))
    });
    group.bench_function("latex_to_unicode_math", |b| {
        b.iter(|| mathema::convert(black_box(QUADRATIC), mathema::Format::LaTeX, mathema::Format::UnicodeMath))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_latex_parse,
    bench_latex_write,
    bench_mathml_write,
    bench_mathml_parse,
    bench_cross_format,
    bench_end_to_end,
);
criterion_main!(benches);
