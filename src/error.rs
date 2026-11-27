//! Error, result, configuration, and diagnostic types for the public API.

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr};
use std::cell::RefCell;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// Clamp a byte offset to a UTF-8 source and return the smallest safe
/// character span. EOF is represented by an empty span.
pub(crate) fn safe_byte_span(source: &str, offset: usize) -> Span {
    let offset = offset.min(source.len());
    if offset == source.len() {
        return Span::new(offset, offset);
    }
    let mut start = offset;
    while !source.is_char_boundary(start) {
        start -= 1;
    }
    let end = start + source[start..].chars().next().map_or(0, char::len_utf8);
    Span::new(start, end)
}

/// Mathema error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A caller cancelled an in-flight controlled request.
    #[error("request cancelled")]
    RequestCancelled,

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

    /// The MTEF writer produced data its own reader does not read back with
    /// the caller's limits. The writer refuses such output instead of
    /// returning it; this indicates a writer defect and is worth reporting.
    /// (Output that only exceeds the caller's depth or node limits is
    /// [`Error::LimitExceeded`] instead.)
    #[error("MTEF write error: {message}")]
    MtefWrite {
        /// Human-readable description, including the reader's error.
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

    /// Parsing stopped because the input needed more of a bounded resource
    /// than the [`Config`] allows (see [`LimitKind`]).
    #[error("{kind} limit of {limit} exceeded")]
    LimitExceeded {
        /// Which configured limit was exceeded.
        kind: LimitKind,
        /// The configured value of that limit.
        limit: usize,
    },

    /// Output was stopped because a caller-selected byte budget was reached.
    #[error("output byte limit of {limit} exceeded")]
    OutputLimit {
        /// The configured maximum number of output bytes.
        limit: usize,
    },
}

impl Error {
    /// Return the source span carried by this error, when the reader already
    /// knows one. Formats whose parser cannot safely map its failure back to
    /// source bytes return `None`.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::LatexParse { span, .. } => *span,
            _ => None,
        }
    }
}

/// An additive error wrapper carrying source-format context and a best-effort
/// byte span. The wrapped [`Error`] remains available for callers that match
/// the legacy error enum.
#[derive(Debug)]
pub struct LocatedError {
    /// The original structured error.
    pub error: Error,
    /// Source format whose reader produced the error.
    pub format: Option<Format>,
    /// Byte span in the original source, when available. For MTEF this refers
    /// to the decoded wire bytes, including an optional Equation Native header.
    pub span: Option<Span>,
}

impl LocatedError {
    /// Construct a location-aware view of a legacy error.
    #[must_use]
    pub fn new(error: Error, format: Option<Format>) -> Self {
        let span = error.span();
        Self {
            error,
            format,
            span,
        }
    }

    /// Construct a location-aware error with an explicitly observed span.
    #[must_use]
    pub fn with_span(error: Error, format: Option<Format>, span: Option<Span>) -> Self {
        Self {
            error,
            format,
            span,
        }
    }

    pub(crate) fn in_format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }

    /// Consume the wrapper and return the original error.
    #[must_use]
    pub fn into_error(self) -> Error {
        self.error
    }
}

impl From<Error> for LocatedError {
    fn from(error: Error) -> Self {
        Self::new(error, None)
    }
}

impl std::fmt::Display for LocatedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for LocatedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// A resource limit that readers enforce while parsing untrusted input.
///
/// Reported by [`Error::LimitExceeded`]. The input-size limit has its own
/// variant, [`Error::InputTooLarge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    /// Work units consumed by a caller-controlled request.
    Work,
    /// The number of AST nodes a single parse may create; see
    /// [`Config::max_nodes`].
    Nodes,
    /// The nesting depth of the AST; see [`Config::max_depth`].
    Depth,
    /// The number of children of one AST node, which the arena stores in 16
    /// bits (see `AstArena::MAX_CHILDREN`); not configurable.
    ListLength,
    /// The size of the AST arena: nodes, child references and string bytes
    /// are indexed with 32 bits (see `AstArena::MAX_ENTRIES`); not
    /// configurable.
    ArenaSize,
    /// Bytes emitted by a bounded output request.
    OutputBytes,
}

impl std::fmt::Display for LimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LimitKind::Work => "request work",
            LimitKind::Nodes => "node count",
            LimitKind::Depth => "nesting depth",
            LimitKind::ListLength => "list length",
            LimitKind::ArenaSize => "arena size",
            LimitKind::OutputBytes => "output bytes",
        })
    }
}

/// Cooperative cancellation and work budget for one parsing or conversion
/// request.
///
/// A control can be cloned and shared with a worker that owns the request. The
/// reader checkpoints are thread-local to the controlled call, so concurrent
/// requests do not share counters or cancellation state.
#[derive(Clone, Debug)]
pub struct RequestControl {
    state: Arc<RequestState>,
}

#[derive(Debug)]
struct RequestState {
    cancelled: Arc<AtomicBool>,
    work: AtomicUsize,
    max_work: Option<usize>,
}

impl Default for RequestControl {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestControl {
    /// Create a request control with no work ceiling.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RequestState {
                cancelled: Arc::new(AtomicBool::new(false)),
                work: AtomicUsize::new(0),
                max_work: None,
            }),
        }
    }

    /// Create a request control with a maximum number of cooperative work
    /// units. A work unit is intentionally abstract and stable only within a
    /// reader implementation.
    #[must_use]
    pub fn with_max_work(max_work: usize) -> Self {
        Self {
            state: Arc::new(RequestState {
                cancelled: Arc::new(AtomicBool::new(false)),
                work: AtomicUsize::new(0),
                max_work: Some(max_work),
            }),
        }
    }

    /// Request cancellation. The next checkpoint returns
    /// [`Error::RequestCancelled`].
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
    }

    /// Return a cancellation flag usable by bounded output controls.
    #[must_use]
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.state.cancelled.clone()
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Number of work units charged so far.
    #[must_use]
    pub fn work(&self) -> usize {
        self.state.work.load(Ordering::Relaxed)
    }

    /// Check cancellation and charge cooperative work.
    pub fn checkpoint(&self, units: usize) -> Result<()> {
        if self.is_cancelled() {
            return Err(Error::RequestCancelled);
        }
        if let Some(limit) = self.state.max_work {
            self.state
                .work
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(units).filter(|next| *next <= limit)
                })
                .map_err(|_| Error::LimitExceeded {
                    kind: LimitKind::Work,
                    limit,
                })?;
        } else {
            self.state.work.fetch_add(units, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Run a closure with this control installed for parser and writer
    /// checkpoints. Nested scopes restore the previous control, including
    /// when the closure unwinds through a panic.
    pub fn scope<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        self.checkpoint(0)?;
        let guard =
            ActiveControlGuard(ACTIVE_CONTROL.with(|slot| slot.replace(Some(self.clone()))));
        let result = f();
        drop(guard);
        let value = result?;
        self.checkpoint(0)?;
        Ok(value)
    }

    /// Internal spelling used by the conversion wrappers.
    pub(crate) fn run<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        self.scope(f)
    }
}

thread_local! {
    static ACTIVE_CONTROL: RefCell<Option<RequestControl>> = const { RefCell::new(None) };
}

struct ActiveControlGuard(Option<RequestControl>);

impl Drop for ActiveControlGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        ACTIVE_CONTROL.with(|slot| {
            slot.replace(previous);
        });
    }
}

/// Charge work for the active controlled request. Uncontrolled paths are a
/// no-op, retaining the existing parser hot path and API behavior.
pub(crate) fn request_checkpoint(units: usize) -> Result<()> {
    ACTIVE_CONTROL.with(|slot| match slot.borrow().as_ref() {
        Some(control) => control.checkpoint(units),
        None => Ok(()),
    })
}

/// Whether the current thread is inside a controlled request. This lets
/// optional work accounting avoid adding a tree walk to ordinary rendering.
pub(crate) fn request_control_active() -> bool {
    ACTIVE_CONTROL.with(|slot| slot.borrow().is_some())
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

/// The stage that produced a structured diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticPhase {
    /// The source reader recovered input or interpreted an unsupported construct.
    Read,
    /// A target writer or renderer could not preserve source information.
    Write,
}

impl DiagnosticPhase {
    /// Stable machine-readable spelling for logs and bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Stable category for a structured diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// The reader recovered malformed or incomplete source input.
    ReaderRecovery,
    /// The reader accepted a construct whose source semantics are only partially modeled.
    ReaderInterpretation,
    /// An undefined, misplaced, dangerous, or otherwise unexpanded control sequence.
    UndefinedCommand,
    /// A text or interchange writer dropped or approximated source information.
    TargetLoss,
    /// A renderer used a client-font or missing-glyph-box fallback.
    RenderMissingGlyph,
    /// A raster renderer rejected a CSS paint spelling.
    RenderColorFallback,
    /// An SVG paint is resolved by the viewer or surrounding document.
    RenderHostPaint,
    /// An SVG paint could not be recognized and is passed through to the viewer.
    RenderUnrecognizedPaint,
}

impl DiagnosticCode {
    /// Stable machine-readable spelling for logs and bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReaderRecovery => "reader_recovery",
            Self::ReaderInterpretation => "reader_interpretation",
            Self::UndefinedCommand => "undefined_command",
            Self::TargetLoss => "target_loss",
            Self::RenderMissingGlyph => "render_missing_glyph",
            Self::RenderColorFallback => "render_color_fallback",
            Self::RenderHostPaint => "render_host_paint",
            Self::RenderUnrecognizedPaint => "render_unrecognized_paint",
        }
    }
}

/// A diagnostic with a stable code and an explicit conversion phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    /// Stable machine-readable category.
    pub code: DiagnosticCode,
    /// Conversion phase that produced the record.
    pub phase: DiagnosticPhase,
    /// Existing severity retained for compatibility with [`Diagnostic`].
    pub kind: DiagnosticKind,
    /// Human-readable detail.
    pub message: String,
    /// Source byte range, when the reader knows one.
    pub span: Option<Span>,
}

/// Compact code metadata kept alongside the public legacy diagnostic vector.
/// The message and span remain in that vector and are copied only when a
/// caller explicitly asks for structured records.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiagnosticMetadata {
    pub(crate) index: usize,
    pub(crate) fingerprint: u64,
    pub(crate) code: DiagnosticCode,
}

impl DiagnosticMetadata {
    pub(crate) fn new(index: usize, diagnostic: &Diagnostic, code: DiagnosticCode) -> Self {
        Self {
            index,
            fingerprint: diagnostic_fingerprint(diagnostic),
            code,
        }
    }
}

impl DiagnosticRecord {
    pub(crate) fn from_owned_write(diagnostic: Diagnostic, code: DiagnosticCode) -> Self {
        Self {
            code,
            phase: DiagnosticPhase::Write,
            kind: diagnostic.kind,
            message: diagnostic.message,
            span: diagnostic.span,
        }
    }
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
    diagnostic_metadata: Vec<DiagnosticMetadata>,
}

impl ParseResult {
    /// Create a result holding `expr` with no diagnostics.
    pub fn new(expr: MathExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
            diagnostic_metadata: Vec::new(),
        }
    }

    /// Create a result holding `expr` and the given diagnostics.
    pub fn with_diagnostics(expr: MathExpr, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            expr,
            diagnostics,
            diagnostic_metadata: Vec::new(),
        }
    }

    /// Return `true` if any diagnostic has [`DiagnosticKind::Warning`] severity.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning)
    }

    /// Build a fresh structured snapshot from the current public diagnostic
    /// vector. Editing that vector updates messages/spans in subsequent
    /// snapshots. Moved entries retain their original codes when unambiguous.
    /// Newly added or edited entries fall back to their severity.
    #[must_use]
    pub fn diagnostic_records(&self) -> Vec<DiagnosticRecord> {
        records_from_metadata(&self.diagnostics, &self.diagnostic_metadata)
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
    diagnostic_metadata: Vec<DiagnosticMetadata>,
}

fn reader_code(diagnostic: &Diagnostic) -> DiagnosticCode {
    match diagnostic.kind {
        DiagnosticKind::Warning => DiagnosticCode::ReaderRecovery,
        DiagnosticKind::Info => DiagnosticCode::ReaderInterpretation,
    }
}

fn diagnostic_fingerprint(diagnostic: &Diagnostic) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hash = DefaultHasher::new();
    (diagnostic.kind, &diagnostic.message, diagnostic.span).hash(&mut hash);
    hash.finish()
}

fn records_from_metadata(
    diagnostics: &[Diagnostic],
    metadata: &[DiagnosticMetadata],
) -> Vec<DiagnosticRecord> {
    // Usually the caller has left the public Vec untouched. In that case
    // tags are already ordered and the common path needs no lookup table.
    let moved = metadata.iter().any(|entry| {
        diagnostics
            .get(entry.index)
            .is_none_or(|d| diagnostic_fingerprint(d) != entry.fingerprint)
    });
    let mut by_content = std::collections::HashMap::new();
    if moved {
        for entry in metadata {
            by_content
                .entry(entry.fingerprint)
                .and_modify(|code| {
                    if *code != Some(entry.code) {
                        *code = None;
                    }
                })
                .or_insert(Some(entry.code));
        }
    }
    let mut tags = metadata.iter().peekable();
    diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let indexed = tags.next_if(|entry| entry.index == index);
            let code = if !moved {
                indexed.map(|entry| entry.code)
            } else {
                let key = diagnostic_fingerprint(diagnostic);
                indexed
                    .filter(|entry| entry.fingerprint == key)
                    .map(|entry| entry.code)
                    .or_else(|| by_content.get(&key).copied().flatten())
            }
            .unwrap_or_else(|| reader_code(diagnostic));
            DiagnosticRecord {
                phase: DiagnosticPhase::Read,
                code,
                kind: diagnostic.kind,
                message: diagnostic.message.clone(),
                span: diagnostic.span,
            }
        })
        .collect()
}

impl CompactParseResult {
    /// Create a result holding `expr` with no diagnostics.
    pub fn new(expr: CompactExpr) -> Self {
        Self {
            expr,
            diagnostics: Vec::new(),
            diagnostic_metadata: Vec::new(),
        }
    }

    /// Create a result holding `expr` and the given diagnostics.
    pub fn with_diagnostics(expr: CompactExpr, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            expr,
            diagnostics,
            diagnostic_metadata: Vec::new(),
        }
    }

    /// Create a parse result with compact code metadata without cloning
    /// messages into a parallel vector.
    pub(crate) fn with_diagnostic_metadata(
        expr: CompactExpr,
        diagnostics: Vec<Diagnostic>,
        diagnostic_metadata: Vec<DiagnosticMetadata>,
    ) -> Self {
        Self {
            expr,
            diagnostics,
            diagnostic_metadata,
        }
    }

    /// Bridge: build from a `ParseResult` by interning into a fresh arena.
    ///
    /// # Errors
    ///
    /// See [`CompactExpr::from_math_expr`].
    pub fn from_parse_result(result: &ParseResult) -> Result<Self> {
        Ok(Self {
            expr: CompactExpr::from_math_expr(&result.expr)?,
            diagnostics: result.diagnostics.clone(),
            diagnostic_metadata: result.diagnostic_metadata.clone(),
        })
    }

    /// Bridge: reconstruct a `ParseResult` from the arena.
    pub fn to_parse_result(&self) -> ParseResult {
        ParseResult {
            expr: self.expr.to_math_expr(),
            diagnostics: self.diagnostics.clone(),
            diagnostic_metadata: self.diagnostic_metadata.clone(),
        }
    }

    /// Return `true` if any diagnostic has [`DiagnosticKind::Warning`] severity.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Warning)
    }

    /// Build a fresh structured snapshot from the current public diagnostic
    /// vector. Legacy edits are reflected in the returned messages and spans.
    #[must_use]
    pub fn diagnostic_records(&self) -> Vec<DiagnosticRecord> {
        records_from_metadata(&self.diagnostics, &self.diagnostic_metadata)
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

/// How the LaTeX writer spells an undefined control sequence
/// ([`MathNode::UndefinedCommand`](crate::MathNode::UndefinedCommand)).
///
/// Only the LaTeX output differs: the AST node, its diagnostics, the MathML
/// `<merror>` and the rendering are the same in both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum UndefinedCommands {
    /// Write it as visible, inert text with its argument groups shown as
    /// literal braces (`\mathord{\text{\textbackslash{}foo}\{a\}}`), so an
    /// unknown command never comes out as live LaTeX. The default.
    #[default]
    Escape,
    /// Write it back as the live command (`\foo{a}`), for output compiled
    /// where the command is defined. Not a sandbox: dangerous commands
    /// on a fixed list (see [`crate::ast::undefined`]) are still escaped, but
    /// that list cannot be complete, so this mode is not safe for untrusted
    /// input compiled with file, Lua or shell access.
    Keep,
}

/// What writers of mathematical notation, LaTeX and UnicodeMath, do with
/// typesetting that has no mathematical meaning
/// ([`NodeAttrs`](crate::ast::NodeAttrs)). MathML, OMML and MTEF always keep
/// what they can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TypesettingPolicy {
    /// Keep what the format has a plain command for: a font size becomes the
    /// nearest LaTeX size command where TeX and KaTeX draw it, and is
    /// otherwise dropped, which the LaTeX writer reports as
    /// [`DiagnosticKind::Info`]. UnicodeMath drops sizes silently. The
    /// default.
    #[default]
    Semantic,
    /// Keep what can be expressed, and report as [`DiagnosticKind::Info`]
    /// what is only approximated or lost.
    Preserve,
}

/// The consumer profile for LaTeX output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LatexProfile {
    /// Keep the existing KaTeX-oriented fragment spelling.
    #[default]
    KaTeX,
    /// Emit a portable pdfLaTeX fragment. Colored boxes protect their math
    /// body with `\ensuremath`, and hexadecimal colors use xcolor models.
    PdfLaTeX,
}

/// Default upper bound on reader input size (4 MiB). Math formulas are
/// kilobytes at most; this guards against adversarial multi-megabyte inputs
/// that would otherwise be fully tokenized and parsed.
pub const DEFAULT_MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

/// Default upper bound on the number of AST nodes a single parse may create.
/// Real formulas need a few thousand at most.
pub const DEFAULT_MAX_NODES: usize = 1_000_000;

/// Default upper bound on the AST nesting depth. It is also the ceiling: the
/// recursive writers and the AST validators only accept trees up to
/// [`MAX_AST_DEPTH`](crate::ast::MAX_AST_DEPTH), so larger values have no effect.
pub const DEFAULT_MAX_DEPTH: usize = crate::ast::MAX_AST_DEPTH;

/// Configuration for readers and writers.
// All fields are `Copy`, so deriving `Copy` avoids unnecessary `.clone()` calls
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
    /// Maximum number of AST nodes a single parse may create. `None` disables
    /// the limit. Exceeding it fails with [`Error::LimitExceeded`] and
    /// [`LimitKind::Nodes`]. Defaults to [`DEFAULT_MAX_NODES`]. Enforced by
    /// all readers while constructing their AST, including generated MathML
    /// and OMML table nodes.
    pub max_nodes: Option<usize>,
    /// Maximum AST nesting depth: a top-level node is at depth 0, its
    /// children at depth 1, and so on. Exceeding it fails with
    /// [`Error::LimitExceeded`] and [`LimitKind::Depth`]. Defaults to
    /// [`DEFAULT_MAX_DEPTH`] (128), which is also the ceiling: larger values
    /// are clamped. Unlike the other limits it cannot be disabled, because the
    /// writers and renderers walk the tree recursively. Enforced while nodes
    /// are built by the MTEF and UnicodeMath readers, and on every parsed tree by
    /// [`convert_with`](crate::convert_with) and the `from_*` functions.
    pub max_depth: usize,
    /// How the LaTeX writer spells an undefined control sequence (default:
    /// [`UndefinedCommands::Escape`], never live LaTeX).
    pub undefined_commands: UndefinedCommands,
    /// What the LaTeX and UnicodeMath writers do with typesetting (default:
    /// [`TypesettingPolicy::Semantic`]).
    pub typesetting: TypesettingPolicy,
    /// Which LaTeX consumer profile the writer targets (default:
    /// [`LatexProfile::KaTeX`]).
    pub latex_profile: LatexProfile,
    /// Retain conservative source byte ranges on compact nodes for editing
    /// clients. Disabled by default to keep ordinary parsing allocation-free.
    pub source_ranges: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strictness: Strictness::Strict,
            display: DisplayStyle::Block,
            max_input_bytes: Some(DEFAULT_MAX_INPUT_BYTES),
            max_nodes: Some(DEFAULT_MAX_NODES),
            max_depth: DEFAULT_MAX_DEPTH,
            undefined_commands: UndefinedCommands::Escape,
            typesetting: TypesettingPolicy::Semantic,
            latex_profile: LatexProfile::KaTeX,
            source_ranges: false,
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

    /// Builder: set the maximum number of AST nodes a parse may create
    /// (`None` disables the limit).
    #[must_use]
    pub fn with_max_nodes(mut self, max_nodes: Option<usize>) -> Self {
        self.max_nodes = max_nodes;
        self
    }

    /// Builder: set the maximum AST nesting depth (clamped to
    /// [`DEFAULT_MAX_DEPTH`]).
    #[must_use]
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Builder: set how the LaTeX writer spells undefined control sequences.
    #[must_use]
    pub fn with_undefined_commands(mut self, undefined_commands: UndefinedCommands) -> Self {
        self.undefined_commands = undefined_commands;
        self
    }

    /// Builder: set what the LaTeX and UnicodeMath writers do with typesetting.
    #[must_use]
    pub fn with_typesetting(mut self, typesetting: TypesettingPolicy) -> Self {
        self.typesetting = typesetting;
        self
    }

    /// Builder: choose the LaTeX output consumer profile.
    #[must_use]
    pub fn with_latex_profile(mut self, latex_profile: LatexProfile) -> Self {
        self.latex_profile = latex_profile;
        self
    }

    /// Retain source byte ranges on compact nodes produced by readers.
    #[must_use]
    pub fn with_source_ranges(mut self, source_ranges: bool) -> Self {
        self.source_ranges = source_ranges;
        self
    }

    /// Return an error if `input` exceeds the configured `max_input_bytes`.
    ///
    /// Called at reader entry points so oversized inputs are rejected before
    /// any tokenization or parsing work is done.
    pub fn check_input_size(&self, input: &str) -> Result<()> {
        self.check_input_len(input.len())
    }

    /// Return an error if an input of `len` bytes exceeds `max_input_bytes`.
    /// The byte-slice counterpart of [`check_input_size`](Self::check_input_size).
    pub fn check_input_len(&self, len: usize) -> Result<()> {
        if let Some(limit) = self.max_input_bytes
            && len > limit
        {
            return Err(Error::InputTooLarge { size: len, limit });
        }
        Ok(())
    }

    /// The effective AST depth limit: [`max_depth`](Self::max_depth) clamped
    /// to [`DEFAULT_MAX_DEPTH`].
    #[must_use]
    pub fn effective_max_depth(&self) -> usize {
        self.max_depth.min(DEFAULT_MAX_DEPTH)
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

impl Format {
    /// Return the canonical lower-case spelling accepted by [`FromStr`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LaTeX => "latex",
            Self::MathML => "mathml",
            Self::Omml => "omml",
            Self::Mtef => "mtef",
            Self::UnicodeMath => "unicodemath",
            #[cfg(feature = "render")]
            Self::Svg => "svg",
            #[cfg(feature = "raster")]
            Self::Png => "png",
            #[cfg(feature = "raster")]
            Self::Jpeg => "jpeg",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

impl FromStr for Format {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "latex" | "tex" => Ok(Self::LaTeX),
            "mathml" | "mml" => Ok(Self::MathML),
            "omml" => Ok(Self::Omml),
            "mtef" => Ok(Self::Mtef),
            "unicodemath" | "unicode-math" | "um" => Ok(Self::UnicodeMath),
            #[cfg(feature = "render")]
            "svg" => Ok(Self::Svg),
            #[cfg(feature = "raster")]
            "png" => Ok(Self::Png),
            #[cfg(feature = "raster")]
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            _ => Err(format!("unknown format '{value}'")),
        }
    }
}
