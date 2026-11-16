//! Resource bounds of the UnicodeMath reader on hostile input: parsing takes
//! time roughly linear in the input, and every resource limit is reported as
//! a typed error rather than exhausting memory or the stack.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mathema::error::{Config, Strictness};
use mathema::reader::Reader;
use mathema::reader::unicode_math::UnicodeMathReader;
use mathema::writer::Writer;
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{Error, Format, LimitKind, convert};

/// Generous bound for one small input, even in an unoptimized build.
const SMALL_INPUT_BUDGET: Duration = Duration::from_secs(1);

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unicode_math")
}

/// Fuzzer inputs whose element nesting exceeds the default depth limit.
const DEPTH_LIMITED: &[&str] = &[
    "slow-unit-0a069a80.txt",
    "slow-unit-7591f1e9.txt",
    "slow-unit-d77302f7.txt",
    "slow-unit-f3897dd3.txt",
];

#[test]
fn fuzzer_inputs_parse_quickly_with_a_typed_result() {
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".txt"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 17, "{names:?}");
    for name in &names {
        let bytes = std::fs::read(fixture_dir().join(name)).unwrap();
        let input = std::str::from_utf8(&bytes).unwrap();
        for strictness in [Strictness::Strict, Strictness::Lenient] {
            let config = Config::default().with_strictness(strictness);
            let start = Instant::now();
            let result = UnicodeMathReader::parse(input, &config);
            // The fuzz target writes every parsed expression back.
            if let Ok(parsed) = &result {
                let _ = UnicodeMathWriter::write(&parsed.expr, &config);
            }
            let elapsed = start.elapsed();
            assert!(
                elapsed < SMALL_INPUT_BUDGET,
                "{name} ({strictness:?}) took {elapsed:?}"
            );
            if DEPTH_LIMITED.contains(&name.as_str()) {
                assert!(
                    matches!(
                        result,
                        Err(Error::LimitExceeded {
                            kind: LimitKind::Depth,
                            limit: 128
                        })
                    ),
                    "{name} ({strictness:?}): {result:?}"
                );
            } else {
                assert!(result.is_ok(), "{name} ({strictness:?}): {result:?}");
            }
        }
    }
}

/// Best of several runs, to keep scheduler noise out of the ratio.
fn best_time(input: &str) -> (Duration, usize) {
    let config = Config::default();
    let mut best = Duration::MAX;
    let mut nodes = 0;
    for _ in 0..5 {
        let start = Instant::now();
        let parsed = UnicodeMathReader::parse_compact(input, &config).unwrap();
        best = best.min(start.elapsed());
        nodes = parsed.expr.arena.node_count();
    }
    (best, nodes)
}

fn assert_linear(name: &str, unit: &str, prefix: &str, suffix: &str, count: usize) {
    let build = |n: usize| format!("{prefix}{}{suffix}", unit.repeat(n));
    let (small_time, small_nodes) = best_time(&build(count));
    let (large_time, large_nodes) = best_time(&build(2 * count));
    // Doubling the input doubles the nodes built (failed alternatives
    // included) and roughly doubles the time.
    assert!(
        large_nodes <= small_nodes * 2 + 16,
        "{name}: {small_nodes} -> {large_nodes} nodes"
    );
    assert!(
        large_time.as_secs_f64() <= small_time.as_secs_f64() * 3.0 + 0.005,
        "{name}: {small_time:?} -> {large_time:?}"
    );
}

#[test]
fn parse_time_scales_linearly() {
    // Well-formed scripts, fractions and groups.
    assert_linear("well-formed", "(a_i+b^2)/c ", "", "", 2_000);
    // Unclosed groups under n-ary scripts. In a matrix each failed element is
    // skipped one character at a time, so every offset inside the unclosed
    // groups is reached again.
    assert_linear(
        "unclosed in matrix",
        &format!("{}#", "x\u{2211}^(n(ln".repeat(12)),
        "\u{25A0}(",
        ")",
        200,
    );
    // Unclosed radicand groups.
    assert_linear("unclosed radicands", "\u{221A}(a+", "", "", 60);
    // A run of dots is not a number; inside a matrix every suffix is retried.
    assert_linear("dots in matrix", ".", "\u{25A0}(", ")", 100_000);
}

#[test]
fn node_limit_is_enforced() {
    let input = "x+".repeat(200);
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        let config = Config::default()
            .with_strictness(strictness)
            .with_max_nodes(Some(50));
        assert!(
            matches!(
                UnicodeMathReader::parse(&input, &config),
                Err(Error::LimitExceeded {
                    kind: LimitKind::Nodes,
                    limit: 50
                })
            ),
            "{strictness:?}"
        );
    }
    assert!(UnicodeMathReader::parse(&input, &Config::default()).is_ok());
    let unlimited = Config::default().with_max_nodes(None);
    assert!(UnicodeMathReader::parse(&input, &unlimited).is_ok());
}

#[test]
fn nodes_of_failed_alternatives_count_towards_the_node_limit() {
    // Each `(xxxxxxxx#` is parsed, fails at `#` and is then read again in
    // the matrix cell; the discarded nodes still count.
    let input = format!("\u{25A0}({})", "(xxxxxxxx#".repeat(1_000));
    let config = Config::default().with_max_nodes(Some(10_000));
    assert!(matches!(
        UnicodeMathReader::parse(&input, &config),
        Err(Error::LimitExceeded {
            kind: LimitKind::Nodes,
            limit: 10_000
        })
    ));
}

fn nested_parens(depth: usize) -> String {
    format!("{}x{}", "(".repeat(depth), ")".repeat(depth))
}

#[test]
fn depth_limit_follows_the_config() {
    let config = Config::default().with_max_depth(10);
    assert!(UnicodeMathReader::parse(&nested_parens(10), &config).is_ok());
    for strictness in [Strictness::Strict, Strictness::Lenient] {
        assert!(matches!(
            UnicodeMathReader::parse(&nested_parens(11), &config.with_strictness(strictness)),
            Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: 10
            })
        ));
    }
    // The reader accepts exactly the nesting the AST depth check accepts.
    assert!(convert(&nested_parens(128), Format::UnicodeMath, Format::LaTeX).is_ok());
    assert!(matches!(
        convert(&nested_parens(129), Format::UnicodeMath, Format::LaTeX),
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            ..
        })
    ));
}

#[test]
fn deep_nesting_is_refused_without_exhausting_the_stack() {
    let config = Config::default();
    for input in [
        // Box and phantom operators nest without brackets.
        "\u{25AD}".repeat(1_000_000),
        "\u{27E1}".repeat(1_000_000),
        "\u{221A}(".repeat(500_000),
        "x^(".repeat(500_000),
        "[".repeat(1_000_000),
    ] {
        let start = Instant::now();
        let result = UnicodeMathReader::parse(&input, &config);
        assert!(
            matches!(
                result,
                Err(Error::LimitExceeded {
                    kind: LimitKind::Depth,
                    ..
                })
            ),
            "{:?}",
            result.map(|r| r.expr.nodes.len())
        );
        assert!(start.elapsed() < SMALL_INPUT_BUDGET * 5);
    }
}
