mod joining;
pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
mod sizes;
pub mod unicode_math;

use crate::ast::MathExpr;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::visit::{CompactVisitor, walk_compact_node};
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};

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

    /// Write from compact arena representation. Default bridges via `to_math_expr()`.
    /// Migrated writers override this to read directly from the arena.
    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
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
