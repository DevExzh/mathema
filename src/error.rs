use crate::ast::{DisplayStyle, MathExpr};
use crate::ast::compact::CompactExpr;

/// Position span in source input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Mathema error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("LaTeX parse error: {message}")]
    LatexParse { message: String, span: Option<Span> },

    #[error("MathML parse error: {message}")]
    MathmlParse { message: String },

    #[error("OMML parse error: {message}")]
    OmmlParse { message: String },

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("render error: {message}")]
    Render { message: String },

    #[error("unsupported construct: {0}")]
    Unsupported(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Option<Span>,
}

/// Result of parsing, containing the AST and any diagnostics.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub expr: MathExpr,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseResult {
    pub fn new(expr: MathExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(expr: MathExpr, diagnostics: Vec<Diagnostic>) -> Self {
        Self { expr, diagnostics }
    }

    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning)
    }
}

/// Result of parsing into the compact arena representation.
#[derive(Debug, Clone)]
pub struct CompactParseResult {
    pub expr: CompactExpr,
    pub diagnostics: Vec<Diagnostic>,
}

impl CompactParseResult {
    pub fn new(expr: CompactExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(expr: CompactExpr, diagnostics: Vec<Diagnostic>) -> Self {
        Self { expr, diagnostics }
    }

    /// Bridge: build from a `ParseResult` by interning into a fresh arena.
    pub fn from_parse_result(result: &ParseResult) -> Self {
        Self {
            expr: CompactExpr::from_math_expr(&result.expr),
            diagnostics: result.diagnostics.clone(),
        }
    }

    /// Bridge: reconstruct a `ParseResult` from the arena.
    pub fn to_parse_result(&self) -> ParseResult {
        ParseResult {
            expr: self.expr.to_math_expr(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning)
    }
}

/// Strictness mode for parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Strictness {
    /// Reject malformed input immediately with a detailed error.
    #[default]
    Strict,
    /// Recover where possible, collect warnings in diagnostics.
    Lenient,
}

/// Configuration for readers and writers.
// Both fields are `Copy`, so deriving `Copy` avoids unnecessary `.clone()` calls
// across all `from_*_with` conversion functions.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub strictness: Strictness,
    pub display: DisplayStyle,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strictness: Strictness::Strict,
            display: DisplayStyle::Inline,
        }
    }
}

/// Supported formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    LaTeX,
    MathML,
    Omml,
    Mtef,
    UnicodeMath,
    #[cfg(feature = "render")]
    Svg,
    #[cfg(feature = "raster")]
    Png,
    #[cfg(feature = "raster")]
    Jpeg,
}
