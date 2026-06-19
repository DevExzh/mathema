use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};

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
#[non_exhaustive]
pub enum Error {
    #[error("LaTeX parse error: {message}")]
    LatexParse { message: String, span: Option<Span> },

    #[error("MathML parse error: {message}")]
    MathmlParse { message: String },

    #[error("OMML parse error: {message}")]
    OmmlParse { message: String },

    #[error("MTEF parse error: {message}")]
    MtefParse { message: String },

    #[error("UnicodeMath parse error: {message}")]
    UnicodeMathParse { message: String },

    #[error("XML error: {message}")]
    Xml { message: String },

    #[error("render error: {message}")]
    Render { message: String },

    #[error("unsupported construct: {0}")]
    Unsupported(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("input size {size} bytes exceeds configured limit of {limit} bytes")]
    InputTooLarge { size: usize, limit: usize },
}

// `quick_xml` is an implementation detail of the MathML/OMML readers. Convert
// its errors into an opaque message so the third-party type does not leak into
// the public `Error` API (which would tie `mathema`'s semver to `quick-xml`'s).
impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::Xml {
            message: e.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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

/// Default upper bound on reader input size (4 MiB). Math formulas are
/// kilobytes at most; this guards against adversarial multi-megabyte inputs
/// that would otherwise be fully tokenized and parsed.
pub const DEFAULT_MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

/// Configuration for readers and writers.
// Both fields are `Copy`, so deriving `Copy` avoids unnecessary `.clone()` calls
// across all `from_*_with` conversion functions.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Config {
    pub strictness: Strictness,
    pub display: DisplayStyle,
    /// Maximum accepted input size in bytes. `None` disables the limit.
    /// Defaults to [`DEFAULT_MAX_INPUT_BYTES`].
    pub max_input_bytes: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strictness: Strictness::Strict,
            display: DisplayStyle::Block,
            max_input_bytes: Some(DEFAULT_MAX_INPUT_BYTES),
        }
    }
}

impl Config {
    /// Builder: set the parse strictness. Use with [`Config::default`] so new
    /// fields stay source-compatible (the struct is `#[non_exhaustive]`).
    #[must_use]
    pub fn with_strictness(mut self, strictness: Strictness) -> Self {
        self.strictness = strictness;
        self
    }

    /// Builder: set the default display style.
    #[must_use]
    pub fn with_display(mut self, display: DisplayStyle) -> Self {
        self.display = display;
        self
    }

    /// Builder: set the maximum accepted input size (`None` disables the limit).
    #[must_use]
    pub fn with_max_input_bytes(mut self, max_input_bytes: Option<usize>) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    /// Return an error if `input` exceeds the configured `max_input_bytes`.
    ///
    /// Called at reader entry points so oversized inputs are rejected before
    /// any tokenization or parsing work is done.
    pub fn check_input_size(&self, input: &str) -> Result<()> {
        if let Some(limit) = self.max_input_bytes
            && input.len() > limit
        {
            return Err(Error::InputTooLarge {
                size: input.len(),
                limit,
            });
        }
        Ok(())
    }
}

/// Supported formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
