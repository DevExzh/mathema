pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
pub mod unicode_math;

use crate::ast::MathExpr;
use crate::ast::compact::CompactExpr;
use crate::error::{Config, Diagnostic, Result};

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
