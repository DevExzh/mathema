use criterion::{Criterion, black_box, criterion_group, criterion_main};
use mathema::ast::TextStyle;
use mathema::unicode::{
    fonts::{styled_char, unstyled_char},
    is_nary_command, is_operator_command, latex_to_unicode, lookup_command, nary_op_to_latex,
    operator_has_limits,
    operators::{OperatorForm, is_stretchy, lookup_operator, lookup_operator_any},
    symbol_kind, unicode_to_latex,
};

fn bench_symbols(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbols");

    // PHF str→char lookups
    group.bench_function("latex_to_unicode/hit_alpha", |b| {
        b.iter(|| latex_to_unicode(black_box("alpha")))
    });
    group.bench_function("latex_to_unicode/hit_sum", |b| {
        b.iter(|| latex_to_unicode(black_box("sum")))
    });
    group.bench_function("latex_to_unicode/hit_rightarrow", |b| {
        b.iter(|| latex_to_unicode(black_box("rightarrow")))
    });
    group.bench_function("latex_to_unicode/miss", |b| {
        b.iter(|| latex_to_unicode(black_box("nonexistent")))
    });

    // PHF char→str lookups
    group.bench_function("unicode_to_latex/hit", |b| {
        b.iter(|| unicode_to_latex(black_box('\u{03B1}')))
    });
    group.bench_function("unicode_to_latex/miss", |b| {
        b.iter(|| unicode_to_latex(black_box('Q')))
    });

    // symbol_kind (PHF char lookup + default fallback)
    group.bench_function("symbol_kind/hit", |b| {
        b.iter(|| symbol_kind(black_box('\u{2211}')))
    });
    group.bench_function("symbol_kind/miss", |b| {
        b.iter(|| symbol_kind(black_box('Q')))
    });

    // lookup_command (PHF str→tuple)
    group.bench_function("lookup_command/hit", |b| {
        b.iter(|| lookup_command(black_box("sum")))
    });
    group.bench_function("lookup_command/miss", |b| {
        b.iter(|| lookup_command(black_box("xyz")))
    });

    // nary_op_to_latex (PHF char→str, small map)
    group.bench_function("nary_op_to_latex/hit", |b| {
        b.iter(|| nary_op_to_latex(black_box('\u{2211}')))
    });
    group.bench_function("nary_op_to_latex/miss", |b| {
        b.iter(|| nary_op_to_latex(black_box('+')))
    });

    // PHF set membership
    group.bench_function("is_nary_command/hit", |b| {
        b.iter(|| is_nary_command(black_box("sum")))
    });
    group.bench_function("is_nary_command/miss", |b| {
        b.iter(|| is_nary_command(black_box("sin")))
    });
    group.bench_function("is_operator_command/hit", |b| {
        b.iter(|| is_operator_command(black_box("sin")))
    });
    group.bench_function("is_operator_command/miss", |b| {
        b.iter(|| is_operator_command(black_box("alpha")))
    });
    group.bench_function("operator_has_limits/hit", |b| {
        b.iter(|| operator_has_limits(black_box("lim")))
    });
    group.bench_function("operator_has_limits/miss", |b| {
        b.iter(|| operator_has_limits(black_box("sin")))
    });

    group.finish();
}

fn bench_operators(c: &mut Criterion) {
    let mut group = c.benchmark_group("operators");

    // Exact (char, form) lookup
    group.bench_function("lookup_operator/hit_plus_infix", |b| {
        b.iter(|| lookup_operator(black_box('+'), black_box(OperatorForm::Infix)))
    });
    group.bench_function("lookup_operator/hit_paren_prefix", |b| {
        b.iter(|| lookup_operator(black_box('('), black_box(OperatorForm::Prefix)))
    });
    group.bench_function("lookup_operator/hit_arrow_infix", |b| {
        b.iter(|| lookup_operator(black_box('\u{2192}'), black_box(OperatorForm::Infix)))
    });
    group.bench_function("lookup_operator/miss", |b| {
        b.iter(|| lookup_operator(black_box('Q'), black_box(OperatorForm::Infix)))
    });

    // Fallback lookup
    group.bench_function("lookup_operator_any/exact_hit", |b| {
        b.iter(|| lookup_operator_any(black_box('+'), black_box(OperatorForm::Infix)))
    });
    group.bench_function("lookup_operator_any/fallback_hit", |b| {
        b.iter(|| lookup_operator_any(black_box('='), black_box(OperatorForm::Prefix)))
    });
    group.bench_function("lookup_operator_any/miss", |b| {
        b.iter(|| lookup_operator_any(black_box('Q'), black_box(OperatorForm::Prefix)))
    });

    // Stretchy check
    group.bench_function("is_stretchy/hit_true", |b| {
        b.iter(|| is_stretchy(black_box('(')))
    });
    group.bench_function("is_stretchy/hit_false", |b| {
        b.iter(|| is_stretchy(black_box('+')))
    });
    group.bench_function("is_stretchy/miss", |b| {
        b.iter(|| is_stretchy(black_box('Q')))
    });

    group.finish();
}

fn bench_fonts(c: &mut Criterion) {
    let mut group = c.benchmark_group("fonts");

    // Arithmetic offset computation
    group.bench_function("styled_char/bold_upper", |b| {
        b.iter(|| styled_char(black_box('A'), black_box(TextStyle::Bold)))
    });
    group.bench_function("styled_char/italic_h", |b| {
        b.iter(|| styled_char(black_box('h'), black_box(TextStyle::Italic)))
    });
    group.bench_function("styled_char/double_struck_R", |b| {
        b.iter(|| styled_char(black_box('R'), black_box(TextStyle::DoubleStruck)))
    });
    group.bench_function("styled_char/normal_passthrough", |b| {
        b.iter(|| styled_char(black_box('x'), black_box(TextStyle::Normal)))
    });
    group.bench_function("styled_char/unmappable", |b| {
        b.iter(|| styled_char(black_box('!'), black_box(TextStyle::Bold)))
    });

    // Reverse: styled → (base, style)
    group.bench_function("unstyled_char/bold_A", |b| {
        b.iter(|| unstyled_char(black_box('\u{1D400}')))
    });
    group.bench_function("unstyled_char/italic_h", |b| {
        b.iter(|| unstyled_char(black_box('\u{210E}')))
    });
    group.bench_function("unstyled_char/miss", |b| {
        b.iter(|| unstyled_char(black_box('x')))
    });

    group.finish();
}

criterion_group!(benches, bench_symbols, bench_operators, bench_fonts);
criterion_main!(benches);
