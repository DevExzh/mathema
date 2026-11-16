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

/// Generous bound for a few megabytes of hostile input in an unoptimized
/// build; quadratic behaviour takes minutes. (Exact work bounds are unit
/// tests of the parser, which counts its steps.)
const LARGE_INPUT_BUDGET: Duration = Duration::from_secs(20);

#[test]
fn large_hostile_inputs_parse_within_budget() {
    let nested_matrices = "\u{25A0}(".repeat(126);
    let cases = [
        format!("{nested_matrices}{}", "#".repeat(1 << 20)),
        format!("{nested_matrices}{}", ". ".repeat(1 << 19)),
        format!("\u{25A0}({}", ".".repeat(1 << 20)),
        format!(
            "\u{25A0}({})",
            format!("{}#", "x\u{2211}^(n(ln".repeat(12)).repeat(5_000)
        ),
        "\u{221A}(a+".repeat(200_000),
        "(a_i+b^2)/c ".repeat(50_000),
    ];
    for input in &cases {
        for strictness in [Strictness::Strict, Strictness::Lenient] {
            let config = Config::default().with_strictness(strictness);
            let start = Instant::now();
            let result = UnicodeMathReader::parse(input, &config);
            let elapsed = start.elapsed();
            assert!(
                elapsed < LARGE_INPUT_BUDGET,
                "{strictness:?}, {} bytes starting {:?}: {elapsed:?}",
                input.len(),
                input.chars().take(8).collect::<String>()
            );
            assert!(
                matches!(
                    result,
                    Ok(_) | Err(Error::UnicodeMathParse { .. } | Error::LimitExceeded { .. })
                ),
                "{result:?}"
            );
        }
    }
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
    // the matrix cell (Lenient mode skips the `(` and the `#`); the discarded
    // nodes still count.
    let input = format!("\u{25A0}({})", "(xxxxxxxx#".repeat(1_000));
    let config = Config::default()
        .with_strictness(Strictness::Lenient)
        .with_max_nodes(Some(10_000));
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
