use crate::ast::{DisplayStyle, MathExpr};

/// Position span in source input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    LatexParse {
        message: String,
        span: Option<Span>,
    },

    #[error("MathML parse error: {message}")]
    MathmlParse { message: String },

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("unsupported construct: {0}")]
    Unsupported(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
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

    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning)
    }
}

/// Strictness mode for parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Reject malformed input immediately with a detailed error.
    Strict,
    /// Recover where possible, collect warnings in diagnostics.
    Lenient,
}

impl Default for Strictness {
    fn default() -> Self {
        Strictness::Strict
    }
}

/// Configuration for readers and writers.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    LaTeX,
    MathML,
    Omml,
    Mtef,
    UnicodeMath,
}
