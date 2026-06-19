//! Error, result, configuration, and diagnostic types for the public API.

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};

/// Position span in source input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Byte offset of the start of the span (inclusive).
    pub start: usize,
    /// Byte offset of the end of the span (exclusive).
    pub end: usize,
}

impl Span {
    /// Create a span covering the byte range `start..end`.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Mathema error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A LaTeX source string could not be parsed.
    #[error("LaTeX parse error: {message}")]
    LatexParse {
        /// Human-readable description of the failure.
        message: String,
        /// Source location of the error, when known.
        span: Option<Span>,
    },

    /// A MathML document could not be parsed.
    #[error("MathML parse error: {message}")]
    MathmlParse {
        /// Human-readable description of the failure.
        message: String,
    },

    /// An OMML (Office MathML) document could not be parsed.
    #[error("OMML parse error: {message}")]
    OmmlParse {
        /// Human-readable description of the failure.
        message: String,
    },

    /// An MTEF (MathType) binary stream could not be parsed.
    #[error("MTEF parse error: {message}")]
    MtefParse {
        /// Human-readable description of the failure.
        message: String,
    },

    /// A UnicodeMath string could not be parsed.
    #[error("UnicodeMath parse error: {message}")]
    UnicodeMathParse {
        /// Human-readable description of the failure.
        message: String,
    },

    /// The XML backing a MathML/OMML document was malformed.
    #[error("XML error: {message}")]
    Xml {
        /// Human-readable description of the XML failure.
        message: String,
    },

    /// The render/layout pipeline failed to produce output.
    #[error("render error: {message}")]
    Render {
        /// Human-readable description of the render failure.
        message: String,
    },

    /// A construct in the input has no representation in the target format.
    #[error("unsupported construct: {0}")]
    Unsupported(String),

    /// An underlying I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The input exceeded the configured [`Config::max_input_bytes`] limit.
    #[error("input size {size} bytes exceeds configured limit of {limit} bytes")]
    InputTooLarge {
        /// Size of the rejected input, in bytes.
        size: usize,
        /// The configured byte limit that was exceeded.
        limit: usize,
    },
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

/// Convenient alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Severity of a non-fatal [`Diagnostic`] collected during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// A potential problem that did not stop parsing.
    Warning,
    /// Informational note about how the input was interpreted.
    Info,
}

/// A non-fatal message produced while parsing input.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity of the diagnostic.
    pub kind: DiagnosticKind,
    /// Human-readable description of the diagnostic.
    pub message: String,
    /// Source location the diagnostic refers to, when known.
    pub span: Option<Span>,
}

/// Result of parsing, containing the AST and any diagnostics.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParseResult {
    /// The parsed expression tree.
    pub expr: MathExpr,
    /// Non-fatal diagnostics collected during parsing.
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseResult {
    /// Create a result holding `expr` with no diagnostics.
    pub fn new(expr: MathExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
        }
    }

    /// Create a result holding `expr` and the given diagnostics.
    pub fn with_diagnostics(expr: MathExpr, diagnostics: Vec<Diagnostic>) -> Self {
        Self { expr, diagnostics }
    }

    /// Return `true` if any diagnostic has [`DiagnosticKind::Warning`] severity.
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
    /// The parsed expression in the compact arena representation.
    pub expr: CompactExpr,
    /// Non-fatal diagnostics collected during parsing.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompactParseResult {
    /// Create a result holding `expr` with no diagnostics.
    pub fn new(expr: CompactExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
        }
    }

    /// Create a result holding `expr` and the given diagnostics.
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

    /// Return `true` if any diagnostic has [`DiagnosticKind::Warning`] severity.
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
    /// Parsing strictness mode (default: [`Strictness::Strict`]).
    pub strictness: Strictness,
    /// Default display style applied when the input does not specify one
    /// (default: [`DisplayStyle::Block`]).
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
    /// LaTeX math markup. Read and write.
    LaTeX,
    /// Presentation/Content MathML. Reads both; writes Presentation MathML only.
    MathML,
    /// Office MathML (OMML), used by Microsoft Office. Read and write.
    Omml,
    /// MathType binary equation format (MTEF), supplied hex-encoded. Read and write.
    Mtef,
    /// UnicodeMath linear notation. Read and write.
    UnicodeMath,
    /// Scalable Vector Graphics. Output-only; requires the `render` feature.
    #[cfg(feature = "render")]
    Svg,
    /// PNG raster image. Output-only; requires the `raster` feature.
    #[cfg(feature = "raster")]
    Png,
    /// JPEG raster image. Output-only; requires the `raster` feature.
    #[cfg(feature = "raster")]
    Jpeg,
}
