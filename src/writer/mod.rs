pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
pub mod unicode_math;

use crate::ast::MathExpr;
use crate::error::{Config, Result};

/// Trait for serializing the shared AST into a math format.
pub trait Writer {
    fn write(expr: &MathExpr, config: &Config) -> Result<String>;
}
