use mathema::AstArena;
use mathema::Config;
use mathema::error::{Error, LimitKind, Strictness};
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

const HUGE_REPETITION: &str = r"\begin{array}{*{18446744073709551615}{c}}a\end{array}";

#[test]
fn array_column_repetition_is_bounded_in_both_strictness_modes() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let result = LatexReader::parse_compact(HUGE_REPETITION, &config);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::ListLength,
                    ..
                })
            ),
            "oversized array preamble must remain a typed fatal limit error in {strictness:?}"
        );
    }
}

#[test]
fn overflowing_array_column_count_is_a_typed_limit_error() {
    let input = r"\begin{array}{*{999999999999999999999999999999999999999}{c}}a\end{array}";
    assert!(matches!(
        LatexReader::parse_compact(input, &Config::default()),
        Err(Error::LimitExceeded {
            kind: LimitKind::ListLength,
            ..
        })
    ));
}

#[test]
fn heap_entry_point_preserves_array_limit_errors() {
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let result = LatexReader::parse(HUGE_REPETITION, &config);
        assert!(matches!(
            result,
            Err(Error::LimitExceeded {
                kind: LimitKind::ListLength,
                ..
            })
        ));
    }
}

#[test]
fn empty_array_column_repetition_does_not_iterate_the_count() {
    let input = r"\begin{array}{*{18446744073709551615}{}}a\end{array}";
    let result = LatexReader::parse_compact(input, &Config::default());
    assert!(
        result.is_ok(),
        "empty repetition has no columns to materialize"
    );
}

#[test]
fn ordinary_nested_array_repetition_still_parses() {
    let input = r"\begin{array}{l*{2}{cr}}a&b&c\end{array}";
    assert!(LatexReader::parse_compact(input, &Config::default()).is_ok());
}

#[test]
fn terminal_separator_does_not_reject_a_row_at_the_next_row_budget() {
    // Before terminal separators were recognized before parsing their phantom
    // row, this valid row was compared with MAX_CHILDREN / 2 and rejected.
    let cells = AstArena::MAX_CHILDREN / 2 + 1;
    let mut input = String::with_capacity(cells * 2 + 32);
    input.push_str(r"\begin{array}{c}");
    input.push('a');
    for _ in 1..cells {
        input.push('&');
    }
    input.push_str(r"\\\end{array}");

    assert!(
        LatexReader::parse_compact(&input, &Config::default()).is_ok(),
        "terminal row separator must not consume the next-row child budget"
    );
}

#[test]
fn nested_array_column_repetition_has_a_depth_bound() {
    let mut spec = String::from("c");
    for _ in 0..=128 {
        spec = format!("*{{1}}{{{spec}}}");
    }
    let input = format!(r"\begin{{array}}{{{spec}}}a\end{{array}}");
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let result = LatexReader::parse_compact(&input, &config);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::Depth,
                    ..
                })
            ),
            "deeply nested repetition must be rejected before recursive stack growth in {strictness:?}"
        );
    }
}

#[test]
fn latex_parser_depth_limit_is_fatal_during_lenient_recovery() {
    let input = format!("{}x{}", "{".repeat(130), "}".repeat(130));
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default().with_strictness(strictness);
        let result = LatexReader::parse_compact(&input, &config);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::Depth,
                    ..
                })
            ),
            "parser recursion must remain a typed fatal limit in {strictness:?}"
        );
    }
}

#[test]
fn malformed_scoped_recovery_stays_within_node_budget() {
    let input = r"\rm {\frac{a}{b}";
    let lenient = Config::default().with_strictness(Strictness::Lenient);
    let recovered = LatexReader::parse_compact(input, &lenient);
    assert!(recovered.is_ok());
    let diagnostics = &recovered.unwrap().diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("skipped unexpected character"))
    );

    // A retry cannot reset the arena's accounting and manufacture another
    // recovery tree past the configured ceiling.
    let guarded = LatexReader::parse_compact(input, &lenient.with_max_nodes(Some(13)));
    assert!(matches!(
        guarded,
        Err(Error::LimitExceeded {
            kind: LimitKind::Nodes,
            limit: 13
        })
    ));

    // Repeated failed suffix scans stop at the linear recovery budget while
    // retaining the unread source as text.
    let budget_input = "\\rm ".repeat(70) + "{";
    let bounded = LatexReader::parse_compact(&budget_input, &lenient).unwrap();
    assert!(
        bounded
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("recovery work limit reached"))
    );
}
