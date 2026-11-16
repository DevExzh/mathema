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
