pub mod latex;
pub mod mathml;
pub mod mtef;
pub mod omml;
pub mod unicode_math;

use crate::error::{CompactParseResult, Config, ParseResult, RequestControl, Result};

/// Trait for parsing a math format into the shared AST.
pub trait Reader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult>;

    /// Parse directly into a compact arena representation.
    ///
    /// The default implementation bridges via `parse()` + `from_math_expr`.
    /// Readers that build into an arena natively should override this.
    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        let result = Self::parse(input, config)?;
        CompactParseResult::from_parse_result(&result)
    }

    /// Parse into the compact arena while cooperatively observing a request
    /// control. Native readers also checkpoint during their traversal.
    fn parse_compact_with_control(
        input: &str,
        config: &Config,
        control: &RequestControl,
    ) -> Result<CompactParseResult> {
        control.run(|| {
            control.checkpoint(input.len())?;
            Self::parse_compact(input, config)
        })
    }
}
