//! OMML (Office Math Markup Language) writer.
//!
//! Serializes the shared AST as the math markup of ECMA-376 Part 1, §22.1.
//! Output follows the element order and required children of the schema in
//! ECMA-376 Part 1, Annex A (`shared-math.xsd`), declares every namespace
//! prefix it uses, and writes default-valued properties only where the
//! reader needs them to tell constructs apart.
//!
//! Formatting that OMML keeps outside the math properties (color, and bold
//! or italic on normal text) belongs to the host document's run properties.
//! [`OmmlOptions::run_properties`] selects the host dialect: none (the
//! default; such formatting is dropped with a diagnostic), WordprocessingML
//! (`w:rPr`) or DrawingML (`a:rPr`).

use super::xml_chars::is_xml_char;
use std::borrow::Cow;
use std::collections::HashSet;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::style::MathStyle;
use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};
use crate::reader::omml::{
    FUNCTION_APPLICATION, MATH_SPACES, OmmlJustification, OmmlParagraph, argument_style, cell_style,
};
use crate::writer::Writer as MathWriter;
use crate::writer::limits::{OutputFailure, OutputLimits};

const MATH_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const WML_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const DML_SIZE_MIN: u32 = 100;
const DML_SIZE_MAX: u32 = 400_000;

/// The host document's run-property dialect, used for formatting that OMML
/// stores outside `m:rPr` (ECMA-376 Part 1, §22.1.2.91: a math run may carry
/// the host's run properties after `m:rPr`).
///
/// ```
/// use mathema::{OmmlOptions, OmmlRunProperties};
/// let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl);
/// assert_eq!(options.run_properties, OmmlRunProperties::WordprocessingMl);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OmmlRunProperties {
    /// Pure OMML: no host run properties. Color and bold or italic normal
    /// text are dropped with a diagnostic.
    #[default]
    None,
    /// WordprocessingML (Word): `w:rPr` after `m:rPr`, `w` namespace
    /// declared on the root element.
    WordprocessingMl,
    /// DrawingML (PowerPoint): `a:rPr` after `m:rPr`, `a` namespace declared
    /// on the root element.
    DrawingMl,
}

/// Options for [`OmmlWriter::write_with_options`].
///
/// ```
/// use mathema::{OmmlJustification, OmmlOptions, OmmlRunProperties};
/// use mathema::writer::omml::OmmlWriter;
///
/// let result = mathema::from_latex(r"\textcolor{red}{x}").unwrap();
/// let options = OmmlOptions::default()
///     .with_run_properties(OmmlRunProperties::WordprocessingMl)
///     .with_display_block(Some(OmmlJustification::Left));
/// let (omml, _diagnostics) = result.to_omml_with_options(&options).unwrap();
/// assert!(omml.starts_with("<m:oMathPara"));
/// assert!(omml.contains(r#"<m:jc m:val="left"/>"#));
/// assert!(omml.contains(r#"<w:color w:val="FF0000"/>"#));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct OmmlOptions {
    /// Host run-property dialect (default: [`OmmlRunProperties::None`]).
    pub run_properties: OmmlRunProperties,
    /// Wrap the equation in a display-math paragraph (`m:oMathPara`)
    /// instead of writing a bare `m:oMath` (default: `false`).
    pub display_block: bool,
    /// Justification written as `m:oMathParaPr/m:jc` when
    /// [`display_block`](Self::display_block) is set.
    pub justification: Option<OmmlJustification>,
}

impl OmmlOptions {
    /// Builder: set the host run-property dialect.
    #[must_use]
    pub fn with_run_properties(mut self, run_properties: OmmlRunProperties) -> Self {
        self.run_properties = run_properties;
        self
    }

    /// Builder: write a display block (`m:oMathPara`) with an optional
    /// justification.
    #[must_use]
    pub fn with_display_block(mut self, justification: Option<OmmlJustification>) -> Self {
        self.display_block = true;
        self.justification = justification;
        self
    }
}

/// Writer for OMML output.
pub struct OmmlWriter;

impl MathWriter for OmmlWriter {
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        super::validate_heap_for_write(expr, config)?;
        let mut compact = CompactExpr::from_math_expr_after_validation(expr)?;
        if crate::ast::normalize::compact_has_empty_base_script(&compact) {
            super::normalize_leading_scripts_for_write(&mut compact, config)?;
        }
        Self::write_with_options_validated(&compact, &OmmlOptions::default()).map(|(s, _)| s)
    }

    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        Self::write_with_options(expr, config, &OmmlOptions::default()).map(|(s, _)| s)
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        Self::write_with_options(expr, config, &OmmlOptions::default())
    }
}

impl OmmlWriter {
    /// Write compact OMML while enforcing output limits and cancellation.
    pub fn write_compact_with_limits(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<String> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        Self::write_with_options_limited(&prepared, &OmmlOptions::default(), limits)
            .map(|(output, _)| output)
    }

    /// Write compact OMML with diagnostics and output controls.
    pub fn write_compact_with_diagnostics_and_limits(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        Self::write_with_options_limited(&prepared, &OmmlOptions::default(), limits)
    }

    pub(crate) fn write_compact_with_limits_trusted(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<String> {
        let prepared = super::prepare_leading_scripts_trusted(expr, config)?;
        Self::write_with_options_limited(&prepared, &OmmlOptions::default(), limits)
            .map(|(output, _)| output)
    }

    pub(crate) fn write_compact_with_diagnostics_and_limits_trusted(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts_trusted(expr, config)?;
        Self::write_with_options_limited(&prepared, &OmmlOptions::default(), limits)
    }

    /// Collect OMML loss diagnostics without retaining the pre-normalization
    /// document. This is used by bounded one-shot conversion so a discarded
    /// pure-OMML pass cannot consume the final output byte budget.
    pub(crate) fn compact_diagnostics_only(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<Vec<Diagnostic>> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        let mut e = Emitter::diagnostics(
            &prepared.arena,
            OmmlOptions::default().run_properties,
            MathStyle::from(prepared.display),
            limits,
        );
        e.open_root("m:oMath");
        e.start(&prepared);
        e.seq(prepared.arena.children(prepared.roots), Fmt::default());
        e.buf.push_str("</m:oMath>");
        let diagnostics = e.diags;
        e.buf.finish_limited()?;
        Ok(diagnostics)
    }

    /// Fast internal bridge for an expression returned by a parser or checked
    /// conversion result. Public option calls retain strict arena validation.
    pub(crate) fn write_with_options_trusted(
        expr: &CompactExpr,
        config: &Config,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        Self::write_with_options_validated(expr, options)
    }

    /// Trusted compact writer used by the parse-once conversion result.
    pub(crate) fn write_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<String> {
        Self::write_with_options_trusted(expr, config, &OmmlOptions::default()).map(|(s, _)| s)
    }

    /// Trusted diagnostic compact writer used by the parse-once conversion
    /// result.
    #[allow(dead_code)]
    pub(crate) fn write_compact_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        Self::write_with_options_trusted(expr, config, &OmmlOptions::default())
    }

    /// Write `expr` as OMML with the given options, returning the markup and
    /// a diagnostic for every piece of information OMML cannot hold.
    ///
    /// # Errors
    ///
    /// Fails when the expression nests deeper than `config.max_depth` (subject
    /// to the crate-wide safe depth ceiling), or when its arena references are
    /// structurally invalid.
    pub fn write_with_options(
        expr: &CompactExpr,
        config: &Config,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        Self::write_with_options_validated(&prepared, options)
    }

    fn write_with_options_limited(
        expr: &CompactExpr,
        options: &OmmlOptions,
        limits: &OutputLimits,
    ) -> Result<(String, Vec<Diagnostic>)> {
        Self::write_with_options_validated_and_limits(expr, options, Some(limits))
    }

    fn write_with_options_validated(
        expr: &CompactExpr,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        Self::write_with_options_validated_and_limits(expr, options, None)
    }

    fn write_with_options_validated_and_limits(
        expr: &CompactExpr,
        options: &OmmlOptions,
        limits: Option<&OutputLimits>,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let mut e = Emitter::new(
            &expr.arena,
            options.run_properties,
            MathStyle::from(expr.display),
            limits.cloned(),
        );
        if options.display_block {
            e.host_style = MathStyle::Display;
            e.open_root("m:oMathPara");
            e.paragraph_props(options.justification);
            e.buf.push_str("<m:oMath>");
        } else {
            e.open_root("m:oMath");
        }
        e.start(expr);
        e.seq(expr.arena.children(expr.roots), Fmt::default());
        e.buf.push_str("</m:oMath>");
        if options.display_block {
            e.buf.push_str("</m:oMathPara>");
        }
        Ok((e.buf.finish_limited()?, e.diags))
    }

    /// Write a display-math paragraph (`m:oMathPara`) holding one `m:oMath`
    /// per equation. The paragraph's justification wins over
    /// `options.justification`; `options.display_block` is ignored.
    ///
    /// ```
    /// use mathema::{Config, OmmlOptions};
    /// use mathema::reader::omml::OmmlReader;
    /// use mathema::writer::omml::OmmlWriter;
    ///
    /// let omml = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMathParaPr><m:jc m:val="right"/></m:oMathParaPr><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#;
    /// let config = Config::default();
    /// let para = OmmlReader::parse_paragraph(omml, &config).unwrap();
    /// let (out, _) = OmmlWriter::write_paragraph(&para, &config, &OmmlOptions::default()).unwrap();
    /// assert_eq!(out, omml);
    /// ```
    ///
    /// # Errors
    ///
    /// Fails when an equation nests deeper than `config.max_depth` (subject to
    /// the crate-wide safe depth ceiling), or when its arena references are
    /// structurally invalid.
    pub fn write_paragraph(
        paragraph: &OmmlParagraph,
        config: &Config,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let empty = AstArena::new();
        let mut e = Emitter::new(&empty, options.run_properties, MathStyle::Text, None);
        e.open_root("m:oMathPara");
        e.paragraph_props(paragraph.justification.or(options.justification));
        for eq in &paragraph.equations {
            super::validate_compact_for_write(eq, config)?;
            e.arena = &eq.arena;
            e.style = MathStyle::from(eq.display);
            // A paragraph is display math.
            e.host_style = MathStyle::Display;
            e.buf.push_str("<m:oMath>");
            e.start(eq);
            e.seq(eq.arena.children(eq.roots), Fmt::default());
            e.buf.push_str("</m:oMath>");
        }
        if paragraph.equations.is_empty() {
            // CT_OMathPara requires at least one m:oMath.
            e.buf.push_str("<m:oMath></m:oMath>");
        }
        e.buf.push_str("</m:oMathPara>");
        Ok((e.buf.finish_limited()?, e.diags))
    }
}

fn dropped_size_message(host: OmmlRunProperties) -> &'static str {
    match host {
        OmmlRunProperties::None => {
            "font sizes need host run properties (OmmlRunProperties::WordprocessingMl or DrawingMl) and were dropped"
        },
        OmmlRunProperties::WordprocessingMl => {
            "font sizes were dropped from an OMML construct that has no size property"
        },
        OmmlRunProperties::DrawingMl => {
            "DrawingML has no host property for this OMML control glyph size and it was dropped"
        },
    }
}

/// Whether there is a color to drop.
pub(crate) fn has_colors(expr: &CompactExpr) -> bool {
    let is_color = |node: &CompactNode| matches!(node, CompactNode::Color { .. });
    expr.arena.nodes().iter().any(is_color)
}

/// Drop every color, so that normalization joins what a color kept apart as
/// it joins it in the OMML read back. (Nesting is bounded: a chain of colors
/// is at most as long as the tree is deep.)
pub(crate) fn drop_colors(expr: &mut CompactExpr) {
    for raw in 0..expr.arena.nodes().len() as u32 {
        let id = NodeId::new(raw);
        let mut content = id;
        while let CompactNode::Color { content: inner, .. } = *expr.arena.node(content) {
            content = inner;
        }
        expr.arena.replace_from(id, content);
    }
}

/// Map a [`PhantomKind`] to OMML `phantPr` flags
/// `(show, zero_wid, zero_asc, zero_desc)`, where `show` is whether the
/// content is visible (`\smash` family) versus invisible (`\phantom` family).
pub(crate) fn phantom_pr_flags(kind: PhantomKind) -> (bool, bool, bool, bool) {
    match kind {
        // Invisible, all dimensions kept.
        PhantomKind::Full => (false, false, false, false),
        // \vphantom: invisible, zero width, keep height/depth.
        PhantomKind::Vertical => (false, true, false, false),
        // \hphantom: invisible, keep width, zero height/depth.
        PhantomKind::Horizontal => (false, false, true, true),
        // \smash: visible, zero height and depth.
        PhantomKind::SmashBoth => (true, false, true, true),
        // \smash[t]: visible, zero height (ascent) only.
        PhantomKind::SmashTop => (true, false, true, false),
        // \smash[b]: visible, zero depth (descent) only.
        PhantomKind::SmashBottom => (true, false, false, true),
    }
}

/// Decompose a [`TextStyle`] into OMML `(sty, scr)` values.
fn decompose_style(style: TextStyle) -> (&'static str, Option<&'static str>) {
    match style {
        TextStyle::Normal => ("p", None),
        TextStyle::Bold => ("b", None),
        TextStyle::Italic => ("i", None),
        TextStyle::BoldItalic => ("bi", None),
        TextStyle::Script => ("p", Some("script")),
        TextStyle::BoldScript => ("b", Some("script")),
        TextStyle::Fraktur => ("p", Some("fraktur")),
        TextStyle::BoldFraktur => ("b", Some("fraktur")),
        TextStyle::DoubleStruck => ("p", Some("double-struck")),
        TextStyle::SansSerif => ("p", Some("sans-serif")),
        TextStyle::SansSerifBold => ("b", Some("sans-serif")),
        TextStyle::SansSerifItalic => ("i", Some("sans-serif")),
        TextStyle::SansSerifBoldItalic => ("bi", Some("sans-serif")),
        TextStyle::Monospace => ("p", Some("monospace")),
    }
}

fn column_align_val(align: ColumnAlign) -> &'static str {
    match align {
        ColumnAlign::Left => "left",
        ColumnAlign::Center => "center",
        ColumnAlign::Right => "right",
    }
}

/// Formatting inherited from enclosing `Styled` and `Color` nodes.
#[derive(Clone, Copy, Default)]
struct Fmt {
    style: Option<TextStyle>,
    color: Option<[u8; 3]>,
}

/// Leaf kinds written as math runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Number,
    Identifier,
    Operator,
    Symbol,
}

struct BoundedText {
    text: String,
    logical_len: usize,
    retain: bool,
    limits: Option<OutputLimits>,
    failure: Option<OutputFailure>,
}

impl BoundedText {
    fn new(limits: Option<OutputLimits>) -> Self {
        Self {
            text: String::with_capacity(512),
            logical_len: 0,
            retain: true,
            limits,
            failure: None,
        }
    }

    fn diagnostics(limits: &OutputLimits) -> Self {
        Self {
            text: String::new(),
            logical_len: 0,
            retain: false,
            limits: Some(limits.with_byte_ceiling(None)),
            failure: None,
        }
    }

    fn check(&mut self, additional: usize) -> bool {
        if self.failure.is_some() {
            return false;
        }
        let Some(limits) = self.limits.as_ref() else {
            return true;
        };
        if let Err(error) = crate::error::request_checkpoint(1) {
            self.failure = Some(OutputFailure::from_error(error));
            return false;
        }
        if limits.is_cancelled() {
            self.failure = Some(OutputFailure::Cancelled);
            return false;
        }
        if let Some(max) = limits.max_bytes
            && self.logical_len.saturating_add(additional) > max
        {
            self.failure = Some(OutputFailure::Bytes(max));
            return false;
        }
        true
    }

    fn push(&mut self, value: char) {
        if self.check(value.len_utf8()) {
            self.logical_len += value.len_utf8();
            if self.retain {
                self.text.push(value);
            }
        }
    }

    fn push_str(&mut self, value: &str) {
        if self.check(value.len()) {
            self.logical_len += value.len();
            if self.retain {
                self.text.push_str(value);
            }
        }
    }

    fn len(&self) -> usize {
        self.logical_len
    }

    fn truncate(&mut self, len: usize) {
        self.logical_len = len;
        if self.retain {
            self.text.truncate(len);
        }
    }

    fn finish_limited(self) -> Result<String> {
        if let Some(failure) = self.failure {
            return Err(failure.into_error());
        }
        Ok(self.text)
    }
}

impl std::ops::Deref for BoundedText {
    type Target = String;
    fn deref(&self) -> &String {
        &self.text
    }
}

impl std::ops::DerefMut for BoundedText {
    fn deref_mut(&mut self) -> &mut String {
        &mut self.text
    }
}

struct Emitter<'a> {
    buf: BoundedText,
    arena: &'a AstArena,
    host: OmmlRunProperties,
    /// The size of ordinary text in points.
    base_pt: f64,
    /// A host run size in hundredths of a point. Word's `w:sz` is emitted in
    /// half points from this canonical unit.
    run_size: Option<u32>,
    style: MathStyle,
    /// Whether the equation has sizes.
    sized: bool,
    /// Set when `node` is called for the node itself, its size applied.
    entered: bool,
    /// The style that the reader of the output knows, for the sizes of runs.
    host_style: MathStyle,
    diags: Vec<Diagnostic>,
    seen: HashSet<Cow<'static, str>>,
    /// Whether the name of a function is written, whose letters are one run.
    in_name: bool,
    /// The last run of such letters.
    name_run: Option<NameRun>,
}

/// Letters of a name of a function, as written in one run.
struct NameRun {
    start: usize,
    end: usize,
    text: String,
    /// Whether a style is not given, but that of words or variables.
    plain: bool,
    sty: Option<&'static str>,
    scr: Option<&'static str>,
    size: Option<u32>,
}

impl<'a> Emitter<'a> {
    fn new(
        arena: &'a AstArena,
        host: OmmlRunProperties,
        style: MathStyle,
        limits: Option<OutputLimits>,
    ) -> Self {
        Self {
            buf: BoundedText::new(limits),
            arena,
            host,
            base_pt: f64::from(crate::ast::attrs::DEFAULT_BASE_SIZE_PT),
            run_size: None,
            style,
            host_style: style,
            sized: false,
            entered: false,
            diags: Vec::new(),
            seen: HashSet::new(),
            in_name: false,
            name_run: None,
        }
    }

    fn diagnostics(
        arena: &'a AstArena,
        host: OmmlRunProperties,
        style: MathStyle,
        limits: &OutputLimits,
    ) -> Self {
        Self {
            buf: BoundedText::diagnostics(limits),
            arena,
            host,
            base_pt: f64::from(crate::ast::attrs::DEFAULT_BASE_SIZE_PT),
            run_size: None,
            style,
            host_style: style,
            sized: false,
            entered: false,
            diags: Vec::new(),
            seen: HashSet::new(),
            in_name: false,
            name_run: None,
        }
    }

    fn loss(&mut self, message: impl Into<Cow<'static, str>>) {
        self.report_once(DiagnosticKind::Warning, message.into());
    }

    fn report_once(&mut self, kind: DiagnosticKind, message: Cow<'static, str>) {
        if !self.seen.contains(message.as_ref()) {
            self.diags.push(Diagnostic {
                kind,
                message: message.to_string(),
                span: None,
            });
            self.seen.insert(message);
        }
    }

    // ── XML primitives ──────────────────────────────────────────────────

    fn open_root(&mut self, tag: &str) {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(" xmlns:m=\"");
        self.buf.push_str(MATH_NS);
        self.buf.push('"');
        match self.host {
            OmmlRunProperties::None => {},
            OmmlRunProperties::WordprocessingMl => {
                self.buf.push_str(" xmlns:w=\"");
                self.buf.push_str(WML_NS);
                self.buf.push('"');
            },
            OmmlRunProperties::DrawingMl => {
                self.buf.push_str(" xmlns:a=\"");
                self.buf.push_str(DML_NS);
                self.buf.push('"');
            },
        }
        self.buf.push('>');
    }

    fn paragraph_props(&mut self, jc: Option<OmmlJustification>) {
        if let Some(jc) = jc {
            self.buf.push_str("<m:oMathParaPr>");
            self.val("m:jc", jc.as_omml());
            self.buf.push_str("</m:oMathParaPr>");
        }
    }

    fn open(&mut self, tag: &str) {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push('>');
    }

    fn close(&mut self, tag: &str) {
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push('>');
    }

    /// Run a child writer under a temporary TeX math style.
    fn with_style<T>(&mut self, style: MathStyle, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.style;
        self.style = style;
        let result = f(self);
        self.style = previous;
        result
    }

    /// `<tag m:val="val"/>`.
    fn val(&mut self, tag: &str, val: &str) {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(" m:val=\"");
        self.attr_text(val);
        self.buf.push_str("\"/>");
    }

    fn on(&mut self, tag: &str) {
        self.val(tag, "1");
    }

    /// Escape the run text after `run` has removed illegal XML characters.
    fn text(&mut self, s: &str) {
        fn entity(c: char) -> Option<&'static str> {
            match c {
                '<' => Some("&lt;"),
                '>' => Some("&gt;"),
                '&' => Some("&amp;"),
                // A literal CR would be normalized to LF by the reader.
                '\r' => Some("&#13;"),
                _ => None,
            }
        }

        if self.buf.limits.is_some() {
            for c in s.chars() {
                match entity(c) {
                    Some(escaped) => self.buf.push_str(escaped),
                    None => self.buf.push(c),
                }
            }
            return;
        }

        let mut start = 0;
        for (at, b) in s.bytes().enumerate() {
            if let Some(escaped) = entity(char::from(b)) {
                // Each entity begins at a single-byte ASCII character.
                self.buf.push_str(&s[start..at]);
                self.buf.push_str(escaped);
                start = at + 1;
            }
        }
        self.buf.push_str(&s[start..]);
    }

    fn attr_text(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '<' => self.buf.push_str("&lt;"),
                '>' => self.buf.push_str("&gt;"),
                '&' => self.buf.push_str("&amp;"),
                '"' => self.buf.push_str("&quot;"),
                // Attribute-value normalization would turn these into spaces.
                '\t' => self.buf.push_str("&#9;"),
                '\n' => self.buf.push_str("&#10;"),
                '\r' => self.buf.push_str("&#13;"),
                c if is_xml_char(c) => self.buf.push(c),
                _ => self.loss("a character that XML cannot represent was dropped"),
            }
        }
    }

    /// A `CT_Char` value: at most one character.
    fn char_val(&mut self, tag: &str, s: &str, what: &str) {
        let mut chars = s.chars();
        if let (Some(first), Some(_)) = (chars.next(), chars.next()) {
            self.loss(format!(
                "OMML {what} holds one character; only {first:?} of {s:?} was written"
            ));
            let mut tmp = [0u8; 4];
            self.val(tag, first.encode_utf8(&mut tmp));
        } else {
            self.val(tag, s);
        }
    }

    // ── Runs ────────────────────────────────────────────────────────────

    /// `<m:r>` with math properties `(scr, sty)` or `nor`, host properties
    /// and text.
    fn run(&mut self, text: &str, sty: Option<&str>, scr: Option<&str>, nor: bool, host: Host) {
        // Most unstyled runs are a single identifier, numeral or operator.
        // Keep bounded writes on their usual per-fragment checkpoints.
        if self.buf.limits.is_none()
            && !nor
            && sty.is_none()
            && scr.is_none()
            && self.run_size.is_none()
            && !host.bold
            && !host.italic
            && host.color.is_none()
            && text
                .chars()
                .all(|c| c > ' ' && !matches!(c, '<' | '>' | '&' | '\u{FFFE}' | '\u{FFFF}'))
        {
            self.buf.push_str("<m:r><m:t>");
            self.buf.push_str(text);
            self.buf.push_str("</m:t></m:r>");
            return;
        }

        let sanitized: String;
        let plain = text.bytes().all(|b| matches!(b, b' '..=b'~'));
        let text = if plain || text.chars().all(is_xml_char) {
            text
        } else {
            self.loss("a character that XML cannot represent was dropped");
            sanitized = text.chars().filter(|&c| is_xml_char(c)).collect();
            if sanitized.is_empty() {
                return;
            }
            &sanitized
        };
        self.open("m:r");
        if nor || sty.is_some() || scr.is_some() {
            // CT_RPR: lit?, (nor | (scr?, sty?)), brk?, aln?
            self.open("m:rPr");
            if nor {
                self.buf.push_str("<m:nor/>");
            } else {
                if let Some(scr) = scr {
                    self.val("m:scr", scr);
                }
                if let Some(sty) = sty {
                    self.val("m:sty", sty);
                }
            }
            self.close("m:rPr");
        }
        self.host_props(host);
        if text
            .bytes()
            .any(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.buf.push_str("<m:t xml:space=\"preserve\">");
        } else {
            self.open("m:t");
        }
        self.text(text);
        self.close("m:t");
        self.close("m:r");
    }

    /// Start an equation. The size of ordinary text
    /// is written too, if it is known and the host has one.
    fn start(&mut self, expr: &CompactExpr) {
        if expr.meta.is_none() && !expr.arena.has_attrs() {
            self.sized = false;
            self.run_size = None;
            return;
        }
        self.base_pt = crate::ast::attrs::base_size_pt(expr.meta.as_deref());
        self.sized = expr.arena.has_attrs()
            && matches!(
                self.host,
                OmmlRunProperties::WordprocessingMl | OmmlRunProperties::DrawingMl
            );
        if expr.arena.has_attrs() && !self.sized {
            self.drop_size();
        }
        self.entered = false;
        let known = expr
            .meta
            .as_deref()
            .is_some_and(|meta| meta.base().is_some());
        self.run_size = (known
            && matches!(
                self.host,
                OmmlRunProperties::WordprocessingMl | OmmlRunProperties::DrawingMl
            ))
        .then(|| {
            let size = (self.base_pt * 100.0).round();
            match self.host {
                OmmlRunProperties::DrawingMl => {
                    size.clamp(f64::from(DML_SIZE_MIN), f64::from(DML_SIZE_MAX)) as u32
                },
                _ => size.clamp(1.0, f64::from(u32::MAX)) as u32,
            }
        });
    }

    /// The size of an object's own sign.
    fn control(&mut self, id: NodeId) {
        if self.run_size.is_some() || self.sized {
            self.sized_control(id);
        }
    }

    #[cold]
    fn sized_control(&mut self, id: NodeId) {
        let own = self.arena.attrs(id).and_then(|attrs| attrs.glyph_size);
        let Some(size) = own.map(|size| self.hundredths(size)).or(self.run_size) else {
            return;
        };
        if self.host == OmmlRunProperties::WordprocessingMl {
            self.buf.push_str(&format!(
                "<m:ctrlPr><w:rPr><w:sz w:val=\"{}\"/></w:rPr></m:ctrlPr>",
                Self::word_half_points(size).min(u32::from(u16::MAX))
            ));
        } else {
            self.drop_size();
        }
    }

    fn drop_size(&mut self) {
        self.report_once(
            DiagnosticKind::Info,
            Cow::Borrowed(dropped_size_message(self.host)),
        );
    }

    /// Whether a node or its own sign has a font size.
    fn has_size(&self, id: NodeId) -> bool {
        self.sized && self.arena.attrs(id).is_some()
    }

    /// The run size that gives the node its font size where it is.
    #[cold]
    fn size_of(&self, id: NodeId) -> Option<u32> {
        Some(self.hundredths(self.arena.attrs(id)?.size?))
    }

    /// The run size in hundredths of a point where it is written.
    fn hundredths(&self, size: FontSize) -> u32 {
        let level = [1.0, 0.7, 0.5][self.host_style.size_class().min(2)];
        let points = size.scale(self.base_pt) * self.base_pt / level;
        let size = (points * 100.0).round();
        match self.host {
            OmmlRunProperties::DrawingMl => {
                size.clamp(f64::from(DML_SIZE_MIN), f64::from(DML_SIZE_MAX)) as u32
            },
            _ => size.clamp(1.0, f64::from(u32::MAX)) as u32,
        }
    }

    fn word_half_points(size: u32) -> u32 {
        (size.saturating_add(25) / 50).max(1)
    }

    fn host_props(&mut self, host: Host) {
        if !host.bold && !host.italic && host.color.is_none() && self.run_size.is_none() {
            return;
        }
        match self.host {
            OmmlRunProperties::None => {
                if host.color.is_some() {
                    self.loss(
                        "run color needs host run properties (OmmlRunProperties::WordprocessingMl \
                         or DrawingMl) and was dropped",
                    );
                }
                if self.run_size.is_some() {
                    self.drop_size();
                }
                if host.bold || host.italic {
                    self.loss(
                        "bold or italic normal text needs host run properties \
                         (OmmlRunProperties::WordprocessingMl or DrawingMl) and was dropped",
                    );
                }
            },
            OmmlRunProperties::WordprocessingMl => {
                // CT_RPr child order: b, i, ..., color sz
                // (ECMA-376 §17.3.2.28).
                self.open("w:rPr");
                if host.bold {
                    self.buf.push_str("<w:b/>");
                }
                if host.italic {
                    self.buf.push_str("<w:i/>");
                }
                if let Some([r, g, b]) = host.color {
                    self.buf
                        .push_str(&format!("<w:color w:val=\"{r:02X}{g:02X}{b:02X}\"/>"));
                }
                if let Some(size) = self.run_size {
                    self.buf.push_str(&format!(
                        "<w:sz w:val=\"{}\"/>",
                        Self::word_half_points(size).min(u32::from(u16::MAX))
                    ));
                }
                self.close("w:rPr");
            },
            OmmlRunProperties::DrawingMl => {
                self.buf.push_str("<a:rPr");
                if host.bold {
                    self.buf.push_str(" b=\"1\"");
                }
                if host.italic {
                    self.buf.push_str(" i=\"1\"");
                }
                if let Some(size) = self.run_size {
                    self.buf.push_str(&format!(" sz=\"{size}\""));
                }
                match host.color {
                    Some([r, g, b]) => self.buf.push_str(&format!(
                        "><a:solidFill><a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/></a:solidFill></a:rPr>"
                    )),
                    None => self.buf.push_str("/>"),
                }
            },
        }
    }

    fn leaf(&mut self, sid: StrId, kind: Leaf, fmt: Fmt) {
        let arena = self.arena;
        let text = arena.str_val(sid);
        let style = match fmt.style {
            Some(TextStyle::Italic) if kind != Leaf::Operator => None,
            other => other,
        };
        let (sty, scr) = match (kind, style) {
            (Leaf::Identifier, None) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    // A single letter is math italic by default.
                    (Some(c), None) if c.is_alphabetic() => (None, None),
                    // One of several letters is upright (see the MathML writer)
                    // unless it is in an italic style.
                    (Some(_), Some(_))
                        if fmt.style.is_none() && text.chars().all(char::is_alphabetic) =>
                    {
                        (Some("p"), None)
                    },
                    _ => (Some("i"), None),
                }
            },
            (Leaf::Operator, None) => (Some("p"), None),
            (Leaf::Number | Leaf::Symbol, None | Some(TextStyle::Normal)) => (None, None),
            (_, Some(st)) => {
                let (sty, scr) = decompose_style(st);
                (Some(sty), scr)
            },
        };
        let host = Host {
            color: fmt.color,
            ..Host::default()
        };
        if self.in_name
            && kind == Leaf::Identifier
            && fmt.color.is_none()
            && !text.is_empty()
            && text.chars().all(char::is_alphabetic)
        {
            return self.name_letters(text, style.is_none(), (sty, scr), host);
        }
        self.run(text, sty, scr, false, host);
    }

    fn text_node(&mut self, own: TextStyle, sid: StrId, fmt: Fmt) {
        let arena = self.arena;
        let text = arena.str_val(sid);
        if text.is_empty() {
            return;
        }
        let style = if own == TextStyle::Normal {
            fmt.style.unwrap_or(TextStyle::Normal)
        } else {
            own
        };
        if text.chars().all(char::is_whitespace) {
            // Whitespace reads back the same from a math run.
            let host = Host {
                color: fmt.color,
                ..Host::default()
            };
            self.run(text, None, None, false, host);
            return;
        }
        let (bold, italic) = match style {
            TextStyle::Normal => (false, false),
            TextStyle::Bold => (true, false),
            TextStyle::Italic => (false, true),
            TextStyle::BoldItalic => (true, true),
            other => {
                self.loss(format!(
                    "normal text cannot carry the {other:?} math style; written as plain text"
                ));
                (false, false)
            },
        };
        self.run(
            text,
            None,
            None,
            true,
            Host {
                bold,
                italic,
                color: fmt.color,
            },
        );
    }

    fn space(&mut self, width: Rational) {
        if width.num == 0 {
            return;
        }
        if width.num < 0 {
            self.loss("OMML cannot represent negative space; it was dropped");
            return;
        }
        let exact = MATH_SPACES
            .iter()
            .find(|&&(_, n, d)| Rational::new(n, d) == width)
            .map(|&(c, _, _)| c);
        let text: String = match exact {
            Some(c) => c.to_string(),
            None if width.den == 1 => "\u{2003}".repeat(width.num.min(64) as usize),
            None => {
                let target = width.as_f64();
                let (c, _, _) = MATH_SPACES
                    .iter()
                    .copied()
                    .min_by(|a, b| {
                        let da = (f64::from(a.1) / f64::from(a.2) - target).abs();
                        let db = (f64::from(b.1) / f64::from(b.2) - target).abs();
                        da.total_cmp(&db)
                    })
                    .expect("table is not empty");
                self.loss(format!(
                    "space of {width} em has no exact Unicode space; written as the nearest one"
                ));
                c.to_string()
            },
        };
        self.run(&text, None, None, false, Host::default());
    }

    // ── Structure ───────────────────────────────────────────────────────

    /// Write a sequence of siblings; `[name, U+2061, argument]` becomes
    /// `m:func`.
    fn seq(&mut self, ids: &[NodeId], fmt: Fmt) {
        let previous_style = self.style;
        let mut i = 0;
        while i < ids.len() {
            if let CompactNode::StyleSwitch(value) = *self.arena.node(ids[i]) {
                self.style = MathStyle::branch_from_switch(value);
                self.loss("OMML output does not preserve math style or size switches");
                i += 1;
                continue;
            }
            if i + 1 < ids.len() && self.is_apply(ids[i + 1]) {
                self.open("m:func");
                self.name(ids[i], fmt);
                self.open("m:e");
                if let Some(&arg) = ids.get(i + 2) {
                    let in_name = std::mem::replace(&mut self.in_name, false);
                    self.node(arg, fmt);
                    self.in_name = in_name;
                }
                self.close("m:e");
                self.close("m:func");
                i += 3;
            } else {
                self.node(ids[i], fmt);
                i += 1;
            }
        }
        self.style = previous_style;
    }

    fn name(&mut self, id: NodeId, fmt: Fmt) {
        self.open("m:fName");
        let in_name = std::mem::replace(&mut self.in_name, true);
        self.node(id, fmt);
        self.in_name = in_name;
        self.close("m:fName");
    }

    /// Write letters of a name of a function, in one run with those before
    /// them as the reader joins them.
    fn name_letters(
        &mut self,
        text: &str,
        plain: bool,
        (mut sty, scr): (Option<&'static str>, Option<&'static str>),
        host: Host,
    ) {
        let mut text = text.to_owned();
        let mut start = self.buf.len();
        let size = self.run_size;
        if let Some(last) = self.name_run.take()
            && last.end == start
            && last.size == size
            && (plain && last.plain || (last.sty, last.scr) == (sty, scr))
        {
            if plain && last.plain {
                sty = Some("p");
            }
            text.insert_str(0, &last.text);
            start = last.start;
            self.buf.truncate(start);
        }
        self.run(&text, sty, scr, false, host);
        self.name_run = Some(NameRun {
            start,
            end: self.buf.len(),
            text,
            plain,
            sty,
            scr,
            size,
        });
    }

    fn is_apply(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Symbol(_, sid)
            if self.arena.str_val(sid) == FUNCTION_APPLICATION)
    }

    /// Write `<tag>` + node + `</tag>`; `NONE` gives an empty argument.
    #[cold]
    #[inline(never)]
    fn sized_arg(&mut self, tag: &str, id: NodeId, fmt: Fmt) {
        let outer = self.host_style;
        self.host_style = argument_style(outer, tag);
        self.node(id, fmt);
        self.host_style = outer;
        self.close(tag);
    }

    fn arg(&mut self, tag: &str, id: NodeId, fmt: Fmt) {
        // As in the reader, what a name has beside it is no part of it.
        let in_name = self.in_name;
        self.in_name &= tag == "m:e";
        self.open(tag);
        if id.is_some() && self.sized {
            self.sized_arg(tag, id, fmt);
        } else {
            if id.is_some() {
                self.node(id, fmt);
            }
            self.close(tag);
        }
        self.in_name = in_name;
    }

    fn children(&mut self, span: NodeSpan, fmt: Fmt) {
        let arena = self.arena;
        self.seq(arena.children(span), fmt);
    }

    /// Write an array cell (a `Row` of the cell's nodes).
    fn cell(&mut self, id: NodeId, fmt: Fmt) {
        self.open("m:e");
        self.cell_content(id, fmt);
        self.close("m:e");
    }

    fn cell_content(&mut self, id: NodeId, fmt: Fmt) {
        let outer = self.host_style;
        if self.sized {
            self.host_style = cell_style(outer);
        }
        self.cell_nodes(id, fmt);
        self.host_style = outer;
    }

    fn cell_nodes(&mut self, id: NodeId, fmt: Fmt) {
        let in_name = std::mem::replace(&mut self.in_name, false);
        match *self.arena.node(id) {
            CompactNode::Row(span) => self.children(span, fmt),
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                self.node(content, fmt)
            },
            _ => self.node(id, fmt),
        }
        self.in_name = in_name;
    }

    /// Resolve the effective placement of a named operator in the current
    /// OMML math style. `Default` limits stack in display style and sit at the
    /// side in text or script styles. Explicit `\limits` always stack, while
    /// `\nolimits` always stays a side script.
    fn named_operator_stacks(&self, id: NodeId) -> bool {
        let limits = match *self.arena.node(id) {
            CompactNode::Operator { limits, .. } => limits,
            CompactNode::StructuredOperator { form, .. } => form.limit_style(),
            _ => return false,
        };
        match limits {
            LimitStyle::AboveBelow => true,
            LimitStyle::Default => self.style.is_display(),
            LimitStyle::SubSup => false,
        }
    }

    /// Whether a cell has a top-level `&` symbol.
    fn has_ampersand(&self, id: NodeId) -> bool {
        let arena = self.arena;
        let is_amp = |id: NodeId| matches!(*arena.node(id), CompactNode::Symbol(_, sid) if arena.str_val(sid) == "&");
        match *arena.node(id) {
            CompactNode::Row(span) => arena.children(span).iter().any(|&c| is_amp(c)),
            _ => is_amp(id),
        }
    }

    /// Write a node of an equation with sizes: its runs have its size.
    #[cold]
    #[inline(never)]
    fn sized_node(&mut self, id: NodeId, fmt: Fmt) {
        let outer = self.run_size;
        if let Some(size) = self.size_of(id) {
            self.run_size = Some(size);
        }
        self.entered = true;
        self.node(id, fmt);
        self.run_size = outer;
    }

    fn node(&mut self, id: NodeId, fmt: Fmt) {
        if self.sized && !std::mem::take(&mut self.entered) {
            return self.sized_node(id, fmt);
        }
        if fmt.color.is_some()
            && matches!(
                *self.arena.node(id),
                CompactNode::Fenced { .. }
                    | CompactNode::Nary { .. }
                    | CompactNode::Fraction { .. }
                    | CompactNode::Sqrt(_)
                    | CompactNode::Root { .. }
                    | CompactNode::Accent { .. }
                    | CompactNode::Overscript { .. }
                    | CompactNode::Underscript { .. }
                    | CompactNode::UnderOver { .. }
                    | CompactNode::Boxed { .. }
                    | CompactNode::Cancel { .. }
            )
        {
            self.loss(
                "OMML output colors text runs only; the color of delimiters, large operators, \
                 rules, radicals and accents was dropped",
            );
        }
        match *self.arena.node(id) {
            CompactNode::Number(sid) => self.leaf(sid, Leaf::Number, fmt),
            CompactNode::Identifier(sid) => self.leaf(sid, Leaf::Identifier, fmt),
            CompactNode::Operator { name, .. } => self.leaf(name, Leaf::Operator, fmt),
            CompactNode::StructuredOperator { content, .. } => {
                // OMML's standard function container accepts rich math in
                // fName. An empty e makes the container a name-only function,
                // which keeps decorated names grouped without a private
                // attribute.
                self.open("m:func");
                self.name(content, fmt);
                self.open("m:e");
                self.close("m:e");
                self.close("m:func");
            },
            CompactNode::Symbol(SymbolKind::Ordinary, sid)
                if ordinary_operator_glyph(self.arena.str_val(sid)) =>
            {
                // A plain run would read back with the glyph's operator class
                // (`\dag` as `\dagger`); a box keeps it ordinary.
                self.open("m:box");
                self.open("m:e");
                self.leaf(sid, Leaf::Symbol, fmt);
                self.close("m:e");
                self.close("m:box");
            },
            CompactNode::Symbol(_, sid) => self.leaf(sid, Leaf::Symbol, fmt),
            CompactNode::Text(style, sid) => self.text_node(style, sid, fmt),
            // OMML has no error element: the command is plain text, followed
            // by its possible arguments.
            CompactNode::UndefinedCommand { name, arguments } => {
                self.text_node(TextStyle::Normal, name, fmt);
                for &group in self.arena.children(arguments) {
                    self.node(group, fmt);
                }
            },
            CompactNode::Space(width) => {
                let basis = self.arena.space_basis(id);
                let width = crate::ast::style::local_space_width(width, basis, self.style);
                self.space(width);
            },
            CompactNode::Row(span) => self.children(span, fmt),
            CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
                self.node(content, fmt)
            },
            CompactNode::Fenced {
                open,
                close,
                content,
            } => self.fenced(open, close, content, fmt),
            CompactNode::Subscript { base, sub } => {
                let style = self.style;
                if self.named_operator_stacks(base) {
                    self.open("m:limLow");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sub(), |e| e.arg("m:lim", sub, fmt));
                    self.close("m:limLow");
                } else {
                    self.open("m:sSub");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sub(), |e| e.arg("m:sub", sub, fmt));
                    self.close("m:sSub");
                }
            },
            CompactNode::Superscript { base, sup } => {
                let style = self.style;
                if self.named_operator_stacks(base) {
                    self.open("m:limUpp");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sup(), |e| e.arg("m:lim", sup, fmt));
                    self.close("m:limUpp");
                } else {
                    self.open("m:sSup");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sup(), |e| e.arg("m:sup", sup, fmt));
                    self.close("m:sSup");
                }
            },
            CompactNode::SubSup { base, sub, sup } => {
                let style = self.style;
                if self.named_operator_stacks(base) {
                    self.open("m:limUpp");
                    self.open("m:e");
                    self.open("m:limLow");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sub(), |e| e.arg("m:lim", sub, fmt));
                    self.close("m:limLow");
                    self.close("m:e");
                    self.with_style(style.sup(), |e| e.arg("m:lim", sup, fmt));
                    self.close("m:limUpp");
                } else {
                    self.open("m:sSubSup");
                    self.with_style(style, |e| e.arg("m:e", base, fmt));
                    self.with_style(style.sub(), |e| e.arg("m:sub", sub, fmt));
                    self.with_style(style.sup(), |e| e.arg("m:sup", sup, fmt));
                    self.close("m:sSubSup");
                }
            },
            CompactNode::PreScripts {
                base,
                pre_sub,
                pre_sup,
            } => {
                let style = self.style;
                self.open("m:sPre");
                self.with_style(style.sub(), |e| e.arg("m:sub", pre_sub, fmt));
                self.with_style(style.sup(), |e| e.arg("m:sup", pre_sup, fmt));
                self.with_style(style, |e| e.arg("m:e", base, fmt));
                self.close("m:sPre");
            },
            CompactNode::Overscript {
                base,
                over,
                movable_limits,
            } => self.over_under(base, over, movable_limits, true, fmt),
            CompactNode::Underscript {
                base,
                under,
                movable_limits,
            } => self.over_under(base, under, movable_limits, false, fmt),
            CompactNode::UnderOver {
                base, under, over, ..
            } => {
                let style = self.style;
                self.open("m:limUpp");
                self.open("m:e");
                self.open("m:limLow");
                self.with_style(style, |e| e.arg("m:e", base, fmt));
                self.with_style(style.sub(), |e| e.arg("m:lim", under, fmt));
                self.close("m:limLow");
                self.close("m:e");
                self.with_style(style.sup(), |e| e.arg("m:lim", over, fmt));
                self.close("m:limUpp");
            },
            CompactNode::Fraction { kind, num, den } => {
                let style = kind.forced_style().unwrap_or(self.style);
                self.open("m:f");
                let ty = match kind {
                    FractionKind::Normal => None,
                    FractionKind::Display | FractionKind::Continued => {
                        self.loss("OMML has no display-style fraction; written as a bar fraction");
                        None
                    },
                    FractionKind::NoBar => Some("noBar"),
                    FractionKind::Skewed => Some("skw"),
                    FractionKind::Inline => Some("lin"),
                };
                if let Some(ty) = ty {
                    self.open("m:fPr");
                    self.val("m:type", ty);
                    self.close("m:fPr");
                }
                let host = self.host_style;
                if !matches!(kind, FractionKind::Display | FractionKind::Continued)
                    && let Some(style) = kind.forced_style()
                {
                    self.host_style = style;
                }
                self.with_style(style.fracnum(), |e| e.arg("m:num", num, fmt));
                self.with_style(style.fracden(), |e| e.arg("m:den", den, fmt));
                self.host_style = host;
                self.close("m:f");
            },
            CompactNode::Sqrt(radicand) => {
                self.open("m:rad");
                self.open("m:radPr");
                self.on("m:degHide");
                self.close("m:radPr");
                self.buf.push_str("<m:deg/>");
                self.with_style(self.style.cramped(), |e| e.arg("m:e", radicand, fmt));
                self.close("m:rad");
            },
            CompactNode::Root { index, radicand } => {
                self.open("m:rad");
                self.with_style(MathStyle::ScriptScript, |e| e.arg("m:deg", index, fmt));
                self.with_style(self.style.cramped(), |e| e.arg("m:e", radicand, fmt));
                self.close("m:rad");
            },
            CompactNode::Nary {
                op,
                sub,
                sup,
                body,
                limits,
            } => {
                let style = self.style;
                // CT_NaryPr: chr, limLoc, grow, subHide, supHide.
                self.open("m:nary");
                self.open("m:naryPr");
                let mut tmp = [0u8; 4];
                self.val("m:chr", op.encode_utf8(&mut tmp));
                match limits {
                    LimitStyle::AboveBelow => self.val("m:limLoc", "undOvr"),
                    LimitStyle::SubSup => self.val("m:limLoc", "subSup"),
                    LimitStyle::Default => {},
                }
                if sub.is_none() {
                    self.on("m:subHide");
                }
                if sup.is_none() {
                    self.on("m:supHide");
                }
                self.control(id);
                self.close("m:naryPr");
                self.with_style(style.sub(), |e| e.arg("m:sub", sub, fmt));
                self.with_style(style.sup(), |e| e.arg("m:sup", sup, fmt));
                self.with_style(style, |e| e.arg("m:e", body, fmt));
                self.close("m:nary");
            },
            CompactNode::Accent {
                base,
                accent,
                position,
                wide,
            } => {
                let mut tmp = [0u8; 4];
                let chr = accent.encode_utf8(&mut tmp);
                if wide {
                    self.loss(
                        "OMML accents stretch on their own; the wide-accent flag is not kept",
                    );
                }
                if position == AccentPosition::Below {
                    self.loss("OMML accents sit above the base; a below accent was written as a group character");
                    if self.has_size(id) {
                        self.loss("OMML group characters have no font size; that of a below accent was dropped");
                    }
                    self.open("m:groupChr");
                    self.open("m:groupChrPr");
                    self.val("m:chr", chr);
                    self.val("m:pos", "bot");
                    self.close("m:groupChrPr");
                    self.arg("m:e", base, fmt);
                    self.close("m:groupChr");
                } else {
                    self.open("m:acc");
                    self.open("m:accPr");
                    self.val("m:chr", chr);
                    self.control(id);
                    self.close("m:accPr");
                    self.arg("m:e", base, fmt);
                    self.close("m:acc");
                }
            },
            CompactNode::Array {
                col_aligns,
                cells,
                num_cols,
                arraystretch_pct,
                add_jot,
                col_seps,
                row_seps,
                array_style,
            } => {
                if array_style {
                    self.loss("OMML output omits TeX outer padding.");
                }
                self.array(
                    col_aligns,
                    cells,
                    num_cols,
                    add_jot,
                    arraystretch_pct != 100 || col_seps != 0 || row_seps != 0,
                    fmt,
                );
            },
            CompactNode::Phantom(content, kind) => {
                let (show, zero_wid, zero_asc, zero_desc) = phantom_pr_flags(kind);
                // m:show defaults to on; write it only to hide the content.
                self.open("m:phant");
                self.open("m:phantPr");
                if !show {
                    self.val("m:show", "0");
                }
                if zero_wid {
                    self.on("m:zeroWid");
                }
                if zero_asc {
                    self.on("m:zeroAsc");
                }
                if zero_desc {
                    self.on("m:zeroDesc");
                }
                self.close("m:phantPr");
                self.arg("m:e", content, fmt);
                self.close("m:phant");
            },
            CompactNode::Boxed {
                content,
                bg_color,
                border_color,
            } => {
                if !self.arena.str_val(bg_color).is_empty()
                    || !self.arena.str_val(border_color).is_empty()
                {
                    self.loss("OMML boxes have no background or border color; colors dropped");
                }
                // A visible box around a cancellation is one m:borderBox.
                let (strikes, inner) = self.strike_chain(content);
                self.open("m:borderBox");
                if strikes != [false; 4] {
                    self.open("m:borderBoxPr");
                    self.strike_props(strikes);
                    self.close("m:borderBoxPr");
                }
                self.arg("m:e", inner, fmt);
                self.close("m:borderBox");
            },
            CompactNode::Actuarial { content, .. } => {
                self.open("m:borderBox");
                self.open("m:borderBoxPr");
                self.on("m:hideBot");
                self.on("m:hideLeft");
                self.close("m:borderBoxPr");
                self.arg("m:e", content, fmt);
                self.close("m:borderBox");
            },
            CompactNode::Cancel { .. } => {
                let (strikes, inner) = self.strike_chain(id);
                // CT_BorderBoxPr: hideTop, hideBot, hideLeft, hideRight,
                // strikeH, strikeV, strikeBLTR, strikeTLBR.
                self.open("m:borderBox");
                self.open("m:borderBoxPr");
                self.on("m:hideTop");
                self.on("m:hideBot");
                self.on("m:hideLeft");
                self.on("m:hideRight");
                self.strike_props(strikes);
                self.close("m:borderBoxPr");
                self.arg("m:e", inner, fmt);
                self.close("m:borderBox");
            },
            CompactNode::Scaled { content, .. } => {
                self.loss("OMML output does not preserve the \\scalebox factor");
                self.node(content, fmt);
            },
            CompactNode::RaiseBox { content, .. } => {
                self.loss("OMML output does not preserve the \\raisebox shift");
                self.node(content, fmt);
            },
            CompactNode::Styled(style, span) => self.children(
                span,
                Fmt {
                    style: Some(style),
                    ..fmt
                },
            ),
            CompactNode::Color { color, content } => {
                let resolved = crate::css_colors::resolve_named_or_hex(self.arena.str_val(color));
                if resolved.is_none() {
                    self.loss(
                        "OMML output could not resolve the color value; only the content is written",
                    );
                }
                self.node(
                    content,
                    Fmt {
                        color: resolved.or(fmt.color),
                        ..fmt
                    },
                );
            },
            CompactNode::MathChoice {
                display,
                text,
                script,
                script_script,
            } => {
                self.loss("OMML output keeps only the applicable style branch of \\mathchoice");
                let branch = self.style.choose(display, text, script, script_script);
                self.node(branch, fmt);
            },
            CompactNode::MathAtom { atom_type, content } => match atom_type {
                MathAtomType::Ord => {
                    self.open("m:box");
                    self.arg("m:e", content, fmt);
                    self.close("m:box");
                },
                MathAtomType::Op => {
                    self.open("m:box");
                    self.open("m:boxPr");
                    self.on("m:opEmu");
                    self.close("m:boxPr");
                    self.arg("m:e", content, fmt);
                    self.close("m:box");
                },
                _ => {
                    self.loss("OMML output does not preserve the math atom spacing class");
                    self.node(content, fmt);
                },
            },
            CompactNode::Rule { .. } => {
                self.loss("OMML cannot represent a rule (\\rule/\\hline); written as a space");
                self.run("\u{00A0}", None, None, false, Host::default());
            },
            CompactNode::Middle(sid) => {
                self.loss("a stray middle delimiter was written as a plain symbol");
                self.leaf(sid, Leaf::Symbol, fmt);
            },
            CompactNode::Newline => {
                self.loss("OMML output does not preserve line breaks inside an equation")
            },
            CompactNode::StyleSwitch(value) => {
                self.style = MathStyle::branch_from_switch(value);
                self.loss("OMML output does not preserve math style or size switches");
            },
            CompactNode::SizeScale(_) => {
                self.loss("OMML output does not preserve math style or size switches")
            },
            CompactNode::ColAlign(_) => {},
            CompactNode::Reflect { content, .. } => {
                self.loss("OMML cannot represent horizontal reflection; the content was written");
                self.node(content, fmt);
            },
        }
    }

    fn fenced(&mut self, open: StrId, close: StrId, content: NodeSpan, fmt: Fmt) {
        let arena = self.arena;
        let mut segments: Vec<Vec<NodeId>> = vec![Vec::new()];
        let mut separator: Option<&str> = None;
        for &child in arena.children(content) {
            if let CompactNode::Middle(sid) = *arena.node(child) {
                let sep = arena.str_val(sid);
                match separator {
                    None => separator = Some(sep),
                    Some(first) if first != sep => self.loss(
                        "OMML delimiters have one separator character; later separators were \
                         written as the first one",
                    ),
                    Some(_) => {},
                }
                segments.push(Vec::new());
            } else if let Some(last) = segments.last_mut() {
                last.push(child);
            }
        }
        let (open, close) = (arena.str_val(open), arena.str_val(close));
        self.open("m:d");
        // CT_DPr: begChr, sepChr, endChr. Defaults: "(", "|", ")".
        let sep = separator.filter(|&s| s != "|");
        if open != "(" || close != ")" || sep.is_some() {
            self.open("m:dPr");
            if open != "(" {
                self.char_val("m:begChr", open, "delimiter");
            }
            if let Some(sep) = sep {
                self.char_val("m:sepChr", sep, "delimiter");
            }
            if close != ")" {
                self.char_val("m:endChr", close, "delimiter");
            }
            self.close("m:dPr");
        }
        for segment in &segments {
            self.open("m:e");
            self.seq(segment, fmt);
            self.close("m:e");
        }
        self.close("m:d");
    }

    fn over_under(&mut self, base: NodeId, script: NodeId, movable: bool, top: bool, fmt: Fmt) {
        let style = self.style;
        let (limit_tag, pos) = if top {
            ("m:limUpp", "top")
        } else {
            ("m:limLow", "bot")
        };
        let arena = self.arena;
        let symbol = match *arena.node(script) {
            CompactNode::Symbol(_, sid) => Some(arena.str_val(sid)),
            _ => None,
        };
        match symbol {
            Some("\u{203E}") if !movable => {
                self.open("m:bar");
                self.open("m:barPr");
                self.val("m:pos", pos);
                self.close("m:barPr");
                self.with_style(style, |e| e.arg("m:e", base, fmt));
                self.close("m:bar");
            },
            // A group character has no size: a mark that has one is a limit.
            Some(chr)
                if !movable
                    && chr.chars().count() <= 1
                    && !self.has_size(script)
                    && chr
                        .chars()
                        .next()
                        .is_some_and(crate::unicode::symbols::is_accent_glyph) =>
            {
                // CT_GroupChrPr: chr, pos, vertJc.
                self.open("m:groupChr");
                self.open("m:groupChrPr");
                self.val("m:chr", chr);
                self.val("m:pos", pos);
                self.close("m:groupChrPr");
                self.with_style(style, |e| e.arg("m:e", base, fmt));
                self.close("m:groupChr");
            },
            _ => {
                if !movable {
                    self.loss(
                        "OMML has no fixed over/under script for general content; written as a limit",
                    );
                }
                self.open(limit_tag);
                self.with_style(style, |e| e.arg("m:e", base, fmt));
                let script_style = if top { style.sup() } else { style.sub() };
                self.with_style(script_style, |e| e.arg("m:lim", script, fmt));
                self.close(limit_tag);
            },
        }
    }

    /// Collect the strike flags of a chain of nested `Cancel` nodes
    /// (`[H, V, BLTR, TLBR]`) and return the innermost content.
    fn strike_chain(&self, mut id: NodeId) -> ([bool; 4], NodeId) {
        let mut strikes = [false; 4];
        while let CompactNode::Cancel { stroke, content } = *self.arena.node(id) {
            match stroke {
                StrokeKind::Horizontal => strikes[0] = true,
                StrokeKind::Vertical => strikes[1] = true,
                StrokeKind::Forward => strikes[2] = true,
                StrokeKind::Backward => strikes[3] = true,
                StrokeKind::Cross => {
                    strikes[2] = true;
                    strikes[3] = true;
                },
            }
            id = content;
        }
        (strikes, id)
    }

    fn strike_props(&mut self, strikes: [bool; 4]) {
        for (on, tag) in
            strikes
                .into_iter()
                .zip(["m:strikeH", "m:strikeV", "m:strikeBLTR", "m:strikeTLBR"])
        {
            if on {
                self.on(tag);
            }
        }
    }

    fn array(
        &mut self,
        col_aligns: NodeSpan,
        cells: NodeSpan,
        num_cols: u16,
        add_jot: bool,
        spacing_lost: bool,
        fmt: Fmt,
    ) {
        let arena = self.arena;
        if spacing_lost {
            self.loss("OMML output does not preserve array row stretch or rules");
        }
        let cell_ids = arena.children(cells);
        let ncols = usize::from(num_cols.max(1));
        let aligns = matrix_column_alignments(arena, arena.children(col_aligns), cell_ids, ncols);
        let centred = aligns.iter().all(|&a| a == ColumnAlign::Center);
        // Right/left alternating columns are an aligned equation array.
        let aligned = ncols >= 2
            && aligns.len() == ncols
            && aligns.iter().enumerate().all(|(i, &a)| {
                a == if i % 2 == 0 {
                    ColumnAlign::Right
                } else {
                    ColumnAlign::Left
                }
            });
        if add_jot && ((ncols == 1 && centred) || aligned) {
            // m:eqArr: one m:e per row, cells joined by `&` alignment points.
            self.open("m:eqArr");
            if cell_ids.is_empty() {
                self.buf.push_str("<m:e></m:e>");
            }
            for row in cell_ids.chunks(ncols) {
                // Every cell is written, empty ones too, so a trailing `&`
                // survives the round trip.
                self.open("m:e");
                for (i, &cell) in row.iter().enumerate() {
                    if i > 0 {
                        self.buf.push_str("<m:r><m:t>&amp;</m:t></m:r>");
                    }
                    if self.has_ampersand(cell) {
                        self.loss(
                            "a literal & in an equation array reads back as an alignment point",
                        );
                    }
                    let column = aligns.get(i).copied().unwrap_or(ColumnAlign::Center);
                    self.note_cell_alignment(row, i, column);
                    self.with_style(MathStyle::Text, |e| e.cell_content(cell, fmt));
                }
                for _ in row.len()..ncols {
                    self.buf.push_str("<m:r><m:t>&amp;</m:t></m:r>");
                }
                self.close("m:e");
            }
            self.close("m:eqArr");
            return;
        }
        self.open("m:m");
        if !aligns.is_empty() {
            // CT_MPr: ..., mcs. Equal neighbouring columns share one m:mc
            // (count is ST_Integer255).
            self.open("m:mPr");
            self.open("m:mcs");
            let mut i = 0;
            while i < aligns.len() {
                let mut count = 1;
                while i + count < aligns.len() && aligns[i + count] == aligns[i] && count < 255 {
                    count += 1;
                }
                self.open("m:mc");
                self.open("m:mcPr");
                self.val("m:count", &count.to_string());
                if self.host == OmmlRunProperties::DrawingMl {
                    // DrawingML reuses the OMML matrix model but forbids
                    // `mcJc` in `mcPr` (MS-ODRAWXML §2.2.5).  Center is the
                    // default, so it needs no replacement.  PowerPoint and
                    // Excel do not support non-centred array columns.
                    if aligns[i] != ColumnAlign::Center {
                        self.loss(
                            "DrawingML does not support non-centred matrix columns; column alignment was dropped",
                        );
                    }
                } else {
                    self.val("m:mcJc", column_align_val(aligns[i]));
                }
                self.close("m:mcPr");
                self.close("m:mc");
                i += count;
            }
            self.close("m:mcs");
            self.close("m:mPr");
        }
        if cell_ids.is_empty() {
            // CT_M needs one m:mr, and CT_MR one m:e.
            self.buf.push_str("<m:mr><m:e></m:e></m:mr>");
        }
        for row in cell_ids.chunks(ncols) {
            self.open("m:mr");
            for (column_index, &cell) in row.iter().enumerate() {
                let column = aligns
                    .get(column_index)
                    .copied()
                    .unwrap_or(ColumnAlign::Center);
                self.note_cell_alignment(row, column_index, column);
                self.with_style(MathStyle::Text, |e| e.cell(cell, fmt));
            }
            for _ in row.len()..ncols {
                self.buf.push_str("<m:e></m:e>");
            }
            self.close("m:mr");
        }
        self.close("m:m");
    }

    fn note_cell_alignment(&mut self, row: &[NodeId], index: usize, column: ColumnAlign) {
        let cell = row[index];
        let alignment = match *self.arena.node(cell) {
            CompactNode::AlignedCell { alignment, .. } => alignment,
            CompactNode::SpannedCell {
                alignment,
                span,
                left_boundaries,
                right_boundaries,
                ..
            } => {
                if self.arena.effective_cell_span_in_row(row, index, span) > 1 {
                    self.loss("OMML writes a cell-span construct as separate matrix entries.");
                }
                if !self.arena.str_val(left_boundaries).is_empty()
                    || !self.arena.str_val(right_boundaries).is_empty()
                {
                    self.loss("OMML omits per-cell boundary material.");
                }
                alignment
            },
            _ => return,
        };
        if alignment != column {
            self.loss(format!("OMML uses the {column:?} column alignment for a cell that requested {alignment:?} cell alignment"));
        }
    }
}

/// OMML can encode an override as a column property whenever all actual
/// cells in that column agree. A slot covered by another cell is empty in
/// the projected matrix and contributes no alignment requirement.
fn matrix_column_alignments(
    arena: &AstArena,
    columns: &[NodeId],
    cells: &[NodeId],
    stride: usize,
) -> Vec<ColumnAlign> {
    let mut result: Vec<_> = columns
        .iter()
        .map(|&id| match *arena.node(id) {
            CompactNode::ColAlign(align) => align,
            _ => ColumnAlign::Center,
        })
        .collect();
    for col in 0..stride.min(cells.len()) {
        let declared = result.get(col).copied().unwrap_or(ColumnAlign::Center);
        let mut requirements = cells[col..]
            .iter()
            .step_by(stride)
            .filter_map(|&id| super::effective_cell_alignment(arena, id, declared));
        if let Some(requested) = requirements.next()
            && requested != declared
            && requirements.all(|alignment| alignment == requested)
        {
            if col >= result.len() {
                result.resize(col + 1, ColumnAlign::Center);
            }
            result[col] = requested;
        }
    }
    result
}

/// Host-side run formatting for one run.
#[derive(Clone, Copy, Default)]
struct Host {
    bold: bool,
    italic: bool,
    color: Option<[u8; 3]>,
}

/// Whether a lone glyph reads back from a plain OMML run with an operator
/// class from the symbol table, so an ordinary use of it must be boxed. The
/// `‖` bar reads as a fence, which TeX output already writes as ordinary.
fn ordinary_operator_glyph(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|ch| {
        chars.as_str().is_empty()
            && ch != '\u{2016}'
            && crate::unicode::symbols::operator_glyph_kind(ch).is_some()
    })
}

#[cfg(test)]
mod tests;
