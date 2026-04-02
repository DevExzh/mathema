pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
pub mod unicode_math;

use crate::error::{Config, ParseResult, Result};

/// Trait for parsing a math format into the shared AST.
pub trait Reader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult>;
}
