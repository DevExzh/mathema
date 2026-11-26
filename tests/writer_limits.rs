//! Direct writer boundaries reject caller-built trees before recursive work.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mathema::ast::compact::CompactExpr;
use mathema::ast::{DisplayStyle, MAX_AST_DEPTH, MathExpr, MathNode};
use mathema::error::{Config, Error, LimitKind};
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::mtef::{MtefWriteOptions, MtefWriter};
use mathema::writer::omml::{OmmlOptions, OmmlWriter};
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{AstArena, CompactNode};

fn deep_heap(depth: usize) -> MathExpr {
    let mut node = MathNode::Identifier("x".into());
    for _ in 0..depth {
        node = MathNode::Sqrt(Box::new(node));
    }
    MathExpr::inline(vec![node])
}

fn depth_error(result: mathema::Result<String>, limit: usize) {
    assert!(
        matches!(
            result,
            Err(Error::LimitExceeded {
                kind: LimitKind::Depth,
                limit: actual,
            }) if actual == limit
        ),
        "expected a depth error, got {result:?}"
    );
}

#[test]
fn every_heap_writer_checks_configured_depth_before_compacting() {
    let expr = deep_heap(32);
    let config = Config::default().with_max_depth(8);

    depth_error(LatexWriter::write(&expr, &config), 8);
    depth_error(MathmlWriter::write(&expr, &config), 8);
    depth_error(OmmlWriter::write(&expr, &config), 8);
    depth_error(UnicodeMathWriter::write(&expr, &config), 8);
    depth_error(MtefWriter::write(&expr, &config), 8);
}

#[test]
fn public_heap_to_compact_bridge_checks_the_hard_depth_cap() {
    let result = CompactExpr::from_math_expr(&deep_heap(MAX_AST_DEPTH + 1));
    assert!(matches!(
        result,
        Err(Error::LimitExceeded {
            kind: LimitKind::Depth,
            limit: MAX_AST_DEPTH,
        })
    ));
}

#[test]
fn every_compact_writer_and_diagnostic_path_checks_depth() {
    let compact = CompactExpr::from_math_expr(&deep_heap(32)).unwrap();
    let config = Config::default().with_max_depth(8);

    depth_error(LatexWriter::write_compact(&compact, &config), 8);
    depth_error(MathmlWriter::write_compact(&compact, &config), 8);
    depth_error(OmmlWriter::write_compact(&compact, &config), 8);
    depth_error(UnicodeMathWriter::write_compact(&compact, &config), 8);
    depth_error(MtefWriter::write_compact(&compact, &config), 8);

    assert!(LatexWriter::write_compact_with_diagnostics(&compact, &config).is_err());
    assert!(MathmlWriter::write_compact_with_diagnostics(&compact, &config).is_err());
    assert!(OmmlWriter::write_compact_with_diagnostics(&compact, &config).is_err());
    assert!(UnicodeMathWriter::write_compact_with_diagnostics(&compact, &config).is_err());
    assert!(MtefWriter::write_compact_with_diagnostics(&compact, &config).is_err());
    assert!(OmmlWriter::write_with_options(&compact, &config, &OmmlOptions::default()).is_err());
    assert!(
        MtefWriter::write_bytes_with_diagnostics(&compact, &config, &MtefWriteOptions::default())
            .is_err()
    );
}

#[test]
fn malformed_compact_handles_are_errors_not_panics() {
    let mut arena = AstArena::new();
    let root = arena.push_node(CompactNode::Sqrt(mathema::NodeId::NONE));
    let roots = arena.push_children(&[root]).unwrap();
    let compact = CompactExpr::new(arena, roots, DisplayStyle::Inline);
    let config = Config::default();

    let result = catch_unwind(AssertUnwindSafe(|| {
        LatexWriter::write_compact(&compact, &config)
    }));
    assert!(result.is_ok(), "writer panicked on a missing compact child");
    assert!(result.unwrap().is_err());

    let result = catch_unwind(AssertUnwindSafe(|| {
        MathmlWriter::write_compact_with_diagnostics(&compact, &config)
    }));
    assert!(
        result.is_ok(),
        "diagnostic traversal panicked on a missing child"
    );
    assert!(result.unwrap().is_err());

    let result = catch_unwind(AssertUnwindSafe(|| {
        OmmlWriter::write_with_options(&compact, &config, &OmmlOptions::default())
    }));
    assert!(
        result.is_ok(),
        "OMML option writer panicked on a missing child"
    );
    assert!(result.unwrap().is_err());

    // Dead arena slots are retained by normalization and some loss-aware
    // writers inspect them. A cycle there must still be rejected before that
    // all-slot scan can loop forever.
    let mut arena = AstArena::new();
    let cycle = arena.push_node(CompactNode::Color {
        color: mathema::StrId::EMPTY,
        content: mathema::NodeId::NONE,
    });
    *arena.node_mut(cycle) = CompactNode::Color {
        color: mathema::StrId::EMPTY,
        content: cycle,
    };
    let sid = arena.push_str("x");
    let live = arena.push_node(CompactNode::Identifier(sid));
    let roots = arena.push_children(&[live]).unwrap();
    let compact = CompactExpr::new(arena, roots, DisplayStyle::Inline);
    assert!(LatexWriter::write_compact(&compact, &config).is_err());
}

struct DefaultBridge;

impl Writer for DefaultBridge {
    fn write(_expr: &MathExpr, _config: &Config) -> mathema::Result<String> {
        Ok(String::new())
    }
}

#[test]
fn writer_trait_default_checks_compact_before_materializing_heap() {
    let mut arena = AstArena::new();
    let root = arena.push_node(CompactNode::Sqrt(mathema::NodeId::NONE));
    let roots = arena.push_children(&[root]).unwrap();
    let compact = CompactExpr::new(arena, roots, DisplayStyle::Inline);

    let result = catch_unwind(AssertUnwindSafe(|| {
        <DefaultBridge as Writer>::write_compact(&compact, &Config::default())
    }));
    assert!(result.is_ok());
    assert!(result.unwrap().is_err());
}
