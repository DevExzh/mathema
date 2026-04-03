use criterion::{Criterion, black_box, criterion_group, criterion_main};
use mathema::ast::compact::CompactExpr;
use mathema::ast::normalize::{normalize_compact_expr, normalize_expr};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::mtef::MtefWriter;
use mathema::writer::omml::OmmlWriter;
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
    let simple_mathml =
        MathmlWriter::write(&LatexReader::parse(SIMPLE, &config).unwrap().expr, &config).unwrap();
    let quad_mathml = MathmlWriter::write(
        &LatexReader::parse(QUADRATIC, &config).unwrap().expr,
        &config,
    )
    .unwrap();

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
        b.iter(|| {
            mathema::convert(
                black_box(QUADRATIC),
                mathema::Format::LaTeX,
                mathema::Format::MathML,
            )
        })
    });
    group.bench_function("latex_to_omml", |b| {
        b.iter(|| {
            mathema::convert(
                black_box(QUADRATIC),
                mathema::Format::LaTeX,
                mathema::Format::Omml,
            )
        })
    });
    group.bench_function("latex_to_unicode_math", |b| {
        b.iter(|| {
            mathema::convert(
                black_box(QUADRATIC),
                mathema::Format::LaTeX,
                mathema::Format::UnicodeMath,
            )
        })
    });
    group.finish();
}

// === Compact arena benchmarks ===

fn bench_parse_compact_vs_legacy(c: &mut Criterion) {
    let config = Config::default();
    let mut group = c.benchmark_group("parse_compact_vs_legacy");
    group.bench_function("parse_legacy/quadratic", |b| {
        b.iter(|| LatexReader::parse(black_box(QUADRATIC), &config))
    });
    group.bench_function("parse_compact/quadratic", |b| {
        b.iter(|| LatexReader::parse_compact(black_box(QUADRATIC), &config))
    });
    group.bench_function("parse_legacy/sum", |b| {
        b.iter(|| LatexReader::parse(black_box(SUM), &config))
    });
    group.bench_function("parse_compact/sum", |b| {
        b.iter(|| LatexReader::parse_compact(black_box(SUM), &config))
    });
    group.finish();
}

fn bench_write_compact_vs_legacy(c: &mut Criterion) {
    let config = Config::default();
    let quad_ast = LatexReader::parse(QUADRATIC, &config).unwrap().expr;
    let quad_compact = LatexReader::parse_compact(QUADRATIC, &config).unwrap().expr;

    let mut group = c.benchmark_group("write_compact_vs_legacy");
    group.bench_function("latex_legacy", |b| {
        b.iter(|| LatexWriter::write(black_box(&quad_ast), &config))
    });
    group.bench_function("latex_compact", |b| {
        b.iter(|| LatexWriter::write_compact(black_box(&quad_compact), &config))
    });
    group.bench_function("mathml_legacy", |b| {
        b.iter(|| MathmlWriter::write(black_box(&quad_ast), &config))
    });
    group.bench_function("mathml_compact", |b| {
        b.iter(|| MathmlWriter::write_compact(black_box(&quad_compact), &config))
    });
    group.bench_function("unicode_legacy", |b| {
        b.iter(|| UnicodeMathWriter::write(black_box(&quad_ast), &config))
    });
    group.bench_function("unicode_compact", |b| {
        b.iter(|| UnicodeMathWriter::write_compact(black_box(&quad_compact), &config))
    });
    group.finish();
}

fn bench_ast_traversal(c: &mut Criterion) {
    let config = Config::default();

    let mut group = c.benchmark_group("ast_traversal");

    // Normalization: MathExpr vs CompactExpr
    group.bench_function("normalize_legacy/quadratic", |b| {
        let base = LatexReader::parse(QUADRATIC, &config).unwrap().expr;
        b.iter(|| {
            let mut expr = base.clone();
            normalize_expr(black_box(&mut expr));
        })
    });
    group.bench_function("normalize_compact/quadratic", |b| {
        let base = LatexReader::parse_compact(QUADRATIC, &config).unwrap().expr;
        b.iter(|| {
            let mut expr = base.clone();
            normalize_compact_expr(black_box(&mut expr));
        })
    });

    // Depth validation: MathExpr vs CompactExpr
    group.bench_function("validate_depth_legacy/quadratic", |b| {
        let expr = LatexReader::parse(QUADRATIC, &config).unwrap().expr;
        b.iter(|| mathema::ast::validate_ast_depth(black_box(&expr)))
    });
    group.bench_function("validate_depth_compact/quadratic", |b| {
        let compact = LatexReader::parse_compact(QUADRATIC, &config).unwrap().expr;
        b.iter(|| {
            mathema::ast::validate_compact_depth(
                black_box(&compact.arena),
                black_box(compact.roots),
            )
        })
    });

    // Count nodes via visitor (compact only — measures traversal speed)
    group.bench_function("visitor_count_nodes/quadratic", |b| {
        let compact = LatexReader::parse_compact(QUADRATIC, &config).unwrap().expr;
        b.iter(|| count_arena_nodes(black_box(&compact)))
    });

    group.finish();
}

/// Count all reachable nodes in a CompactExpr arena.
fn count_arena_nodes(expr: &CompactExpr) -> usize {
    use mathema::ast::compact::{CompactNode, NodeId, NodeSpan};

    fn count_span(arena: &mathema::AstArena, span: NodeSpan) -> usize {
        let mut n = 0;
        for &id in arena.children(span) {
            n += count_node(arena, id);
        }
        n
    }

    fn count_node(arena: &mathema::AstArena, id: NodeId) -> usize {
        if id.is_none() {
            return 0;
        }
        let mut n = 1;
        match *arena.node(id) {
            CompactNode::Row(span) | CompactNode::Styled(_, span) => n += count_span(arena, span),
            CompactNode::Fenced { content, .. } => n += count_span(arena, content),
            CompactNode::Subscript { base, sub } => {
                n += count_node(arena, base) + count_node(arena, sub);
            },
            CompactNode::Superscript { base, sup } => {
                n += count_node(arena, base) + count_node(arena, sup);
            },
            CompactNode::SubSup { base, sub, sup } => {
                n += count_node(arena, base) + count_node(arena, sub) + count_node(arena, sup);
            },
            CompactNode::Fraction { num, den, .. } => {
                n += count_node(arena, num) + count_node(arena, den);
            },
            CompactNode::Sqrt(inner) => n += count_node(arena, inner),
            CompactNode::Root { index, radicand } => {
                n += count_node(arena, index) + count_node(arena, radicand);
            },
            CompactNode::Nary { sub, sup, body, .. } => {
                n += count_node(arena, sub) + count_node(arena, sup) + count_node(arena, body);
            },
            CompactNode::Overscript { base, over, .. } => {
                n += count_node(arena, base) + count_node(arena, over);
            },
            CompactNode::Underscript { base, under, .. } => {
                n += count_node(arena, base) + count_node(arena, under);
            },
            CompactNode::UnderOver {
                base, under, over, ..
            } => {
                n += count_node(arena, base) + count_node(arena, under) + count_node(arena, over);
            },
            CompactNode::Accent { base, .. } => n += count_node(arena, base),
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => {
                n += count_node(arena, base)
                    + count_node(arena, pre_sub)
                    + count_node(arena, pre_sup);
            },
            CompactNode::Array {
                col_aligns, cells, ..
            } => {
                n += count_span(arena, col_aligns) + count_span(arena, cells);
            },
            CompactNode::Phantom(inner) | CompactNode::Boxed(inner) => {
                n += count_node(arena, inner);
            },
            CompactNode::Cancel { content, .. } | CompactNode::Scaled { content, .. } => {
                n += count_node(arena, content);
            },
            _ => {}, // leaf nodes
        }
        n
    }

    count_span(&expr.arena, expr.roots)
}

fn bench_arena_memory(c: &mut Criterion) {
    let config = Config::default();

    let mut group = c.benchmark_group("arena_memory");

    // Measure memory: CompactExpr construction (includes allocation)
    group.bench_function("compact_alloc/quadratic", |b| {
        b.iter(|| LatexReader::parse_compact(black_box(QUADRATIC), &config))
    });

    // Measure round-trip: CompactExpr → MathExpr materialization cost
    group.bench_function("to_math_expr/quadratic", |b| {
        let compact = LatexReader::parse_compact(QUADRATIC, &config).unwrap().expr;
        b.iter(|| black_box(&compact).to_math_expr())
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
    bench_parse_compact_vs_legacy,
    bench_write_compact_vs_legacy,
    bench_ast_traversal,
    bench_arena_memory,
);
criterion_main!(benches);
