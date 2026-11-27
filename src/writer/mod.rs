pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
mod sizes;
pub mod unicode_math;
pub(crate) mod xml_chars;

use crate::ast::MathExpr;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::visit::{CompactVisitor, walk_compact_node};
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};

/// Validate a caller-supplied heap tree before a concrete writer recursively
/// compacts or emits it. The configured limit is clamped by the AST validator
/// to the crate-wide safe ceiling.
pub(crate) fn validate_heap_for_write(expr: &MathExpr, config: &Config) -> Result<()> {
    crate::ast::validate_ast_depth_with(expr, config.effective_max_depth())
}

/// Validate a caller-supplied compact tree before a concrete writer or loss
/// diagnostic traversal follows arena references.
pub(crate) fn validate_compact_for_write(expr: &CompactExpr, config: &Config) -> Result<()> {
    crate::ast::validate_compact_for_write(&expr.arena, expr.roots, config.effective_max_depth())
}

/// Validate a compact tree known to come from a parser or an already checked
/// conversion result. This follows only reachable nodes; public direct writer
/// calls use [`validate_compact_for_write`] because their arena may have been
/// edited or assembled from foreign handles.
pub(crate) fn validate_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<()> {
    crate::ast::validate_compact_depth_with(&expr.arena, expr.roots, config.effective_max_depth())
}

/// The contents of a table cell after an environment-provided style marker.
fn cell_after_style<'a>(arena: &'a AstArena, cell: &'a NodeId, marker: u8) -> Option<&'a [NodeId]> {
    let (_, nodes) = arena.table_cell_parts(cell);
    match nodes.split_first() {
        None => Some(nodes),
        Some((&first, rest)) if matches!(*arena.node(first), CompactNode::StyleSwitch(value) if value == marker) => {
            Some(rest)
        },
        _ => None,
    }
}

/// The alignment of an occupied logical slot when projecting to a format
/// with a single alignment per matrix or per column.
fn effective_cell_alignment(
    arena: &AstArena,
    id: NodeId,
    declared: crate::ast::ColumnAlign,
) -> Option<crate::ast::ColumnAlign> {
    match *arena.node(id) {
        CompactNode::AlignedCell { alignment, .. } | CompactNode::SpannedCell { alignment, .. } => {
            Some(alignment)
        },
        // A bare empty slot has no aligned content. A whole-cell wrapper
        // above still contributes an explicit alignment even if it is empty.
        CompactNode::Row(span) if span.is_empty() => None,
        _ => Some(declared),
    }
}

/// Walk a compact expression and collect a `Warning` for every node the target
/// writer cannot represent, using a per-format `classify` closure that returns
/// the warning message for a lossy node (or `None`). Shared by the format
/// writers so AST→format data loss is reported consistently (see
/// `write_compact_with_diagnostics`).
pub(crate) fn collect_loss_diagnostics<F>(expr: &CompactExpr, classify: F) -> Vec<Diagnostic>
where
    F: Fn(&AstArena, &CompactNode) -> Option<String>,
{
    struct LossVisitor<F> {
        diagnostics: Vec<Diagnostic>,
        classify: F,
    }
    impl<F: Fn(&AstArena, &CompactNode) -> Option<String>> CompactVisitor for LossVisitor<F> {
        fn visit_node(&mut self, arena: &AstArena, id: NodeId) {
            if let Some(message) = (self.classify)(arena, arena.node(id)) {
                self.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message,
                    span: None,
                });
            }
            walk_compact_node(self, arena, id);
        }
    }
    let mut visitor = LossVisitor {
        diagnostics: Vec::new(),
        classify,
    };
    visitor.visit_expr(expr);
    visitor.diagnostics
}

/// Trait for serializing the shared AST into a math format.
pub trait Writer {
    fn write(expr: &MathExpr, config: &Config) -> Result<String>;

    /// Write from compact arena representation. The default validates the
    /// arena before bridging via `to_math_expr()`; migrated writers override
    /// this to read directly from the arena.
    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        validate_compact_for_write(expr, config)?;
        let math_expr = expr.to_math_expr();
        Self::write(&math_expr, config)
    }

    /// Write from compact representation, also returning any diagnostics for
    /// information lost in the conversion (e.g. a lossy target format dropping
    /// decoration). The default reports no loss; formats with silent drops
    /// (notably MTEF) override this so callers can detect data loss.
    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        Ok((Self::write_compact(expr, config)?, Vec::new()))
    }
}
