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

use std::collections::HashSet;

use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::style::MathStyle;
use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};
use crate::reader::omml::{FUNCTION_APPLICATION, MATH_SPACES, OmmlJustification, OmmlParagraph};
use crate::writer::Writer as MathWriter;

const MATH_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const WML_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

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
        Self::write_compact(&CompactExpr::from_math_expr(expr)?, config)
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
    /// Write `expr` as OMML with the given options, returning the markup and
    /// a diagnostic for every piece of information OMML cannot hold.
    ///
    /// # Errors
    ///
    /// Fails when the expression nests deeper than [`MAX_AST_DEPTH`].
    pub fn write_with_options(
        expr: &CompactExpr,
        _config: &Config,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        crate::ast::validate_compact_depth(&expr.arena, expr.roots)?;
        let mut e = Emitter::new(
            &expr.arena,
            options.run_properties,
            MathStyle::from(expr.display),
        );
        if options.display_block {
            e.open_root("m:oMathPara");
            e.paragraph_props(options.justification);
            e.buf.push_str("<m:oMath>");
        } else {
            e.open_root("m:oMath");
        }
        e.seq(expr.arena.children(expr.roots), Fmt::default());
        e.buf.push_str("</m:oMath>");
        if options.display_block {
            e.buf.push_str("</m:oMathPara>");
        }
        Ok((e.buf, e.diags))
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
    /// Fails when an equation nests deeper than [`MAX_AST_DEPTH`].
    pub fn write_paragraph(
        paragraph: &OmmlParagraph,
        _config: &Config,
        options: &OmmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let empty = AstArena::new();
        let mut e = Emitter::new(&empty, options.run_properties, MathStyle::Text);
        e.open_root("m:oMathPara");
        e.paragraph_props(paragraph.justification.or(options.justification));
        for eq in &paragraph.equations {
            crate::ast::validate_compact_depth(&eq.arena, eq.roots)?;
            e.arena = &eq.arena;
            e.style = MathStyle::from(eq.display);
            e.buf.push_str("<m:oMath>");
            e.seq(eq.arena.children(eq.roots), Fmt::default());
            e.buf.push_str("</m:oMath>");
        }
        if paragraph.equations.is_empty() {
            // CT_OMathPara requires at least one m:oMath.
            e.buf.push_str("<m:oMath></m:oMath>");
        }
        e.buf.push_str("</m:oMathPara>");
        Ok((e.buf, e.diags))
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

struct Emitter<'a> {
    buf: String,
    arena: &'a AstArena,
    host: OmmlRunProperties,
    style: MathStyle,
    diags: Vec<Diagnostic>,
    seen: HashSet<String>,
}

impl<'a> Emitter<'a> {
    fn new(arena: &'a AstArena, host: OmmlRunProperties, style: MathStyle) -> Self {
        Self {
            buf: String::with_capacity(512),
            arena,
            host,
            style,
            diags: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn loss(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.seen.insert(message.clone()) {
            self.diags.push(Diagnostic {
                kind: DiagnosticKind::Warning,
                message,
                span: None,
            });
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

    /// Escape character data; characters XML 1.0 cannot carry are dropped.
    fn text(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '<' => self.buf.push_str("&lt;"),
                '>' => self.buf.push_str("&gt;"),
                '&' => self.buf.push_str("&amp;"),
                // A literal CR would be normalized to LF by the reader.
                '\r' => self.buf.push_str("&#13;"),
                c if is_xml_char(c) => self.buf.push(c),
                _ => self.loss("a character that XML cannot represent was dropped"),
            }
        }
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
        let sanitized: String;
        let text = if text.chars().all(is_xml_char) {
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
        if text.contains([' ', '\t', '\n', '\r']) {
            self.buf.push_str("<m:t xml:space=\"preserve\">");
        } else {
            self.open("m:t");
        }
        self.text(text);
        self.close("m:t");
        self.close("m:r");
    }

    fn host_props(&mut self, host: Host) {
        if !host.bold && !host.italic && host.color.is_none() {
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
                if host.bold || host.italic {
                    self.loss(
                        "bold or italic normal text needs host run properties \
                         (OmmlRunProperties::WordprocessingMl or DrawingMl) and was dropped",
                    );
                }
            },
            OmmlRunProperties::WordprocessingMl => {
                // CT_RPr child order: b, i, ..., color (ECMA-376 §17.3.2.28).
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
                self.open("m:fName");
                self.node(ids[i], fmt);
                self.close("m:fName");
                self.open("m:e");
                if let Some(&arg) = ids.get(i + 2) {
                    self.node(arg, fmt);
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

    fn is_apply(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Symbol(_, sid)
            if self.arena.str_val(sid) == FUNCTION_APPLICATION)
    }

    /// Write `<tag>` + node + `</tag>`; `NONE` gives an empty argument.
    fn arg(&mut self, tag: &str, id: NodeId, fmt: Fmt) {
        self.open(tag);
        if id.is_some() {
            self.node(id, fmt);
        }
        self.close(tag);
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
        match *self.arena.node(id) {
            CompactNode::Row(span) => self.children(span, fmt),
            _ => self.node(id, fmt),
        }
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

    fn node(&mut self, id: NodeId, fmt: Fmt) {
        match *self.arena.node(id) {
            CompactNode::Number(sid) => self.leaf(sid, Leaf::Number, fmt),
            CompactNode::Identifier(sid) => self.leaf(sid, Leaf::Identifier, fmt),
            CompactNode::Operator { name, .. } => self.leaf(name, Leaf::Operator, fmt),
            CompactNode::StructuredOperator { content, .. } => self.node(content, fmt),
            CompactNode::Symbol(_, sid) => self.leaf(sid, Leaf::Symbol, fmt),
            CompactNode::Text(style, sid) => self.text_node(style, sid, fmt),
            CompactNode::Space(width) => {
                let basis = self.arena.space_basis(id);
                let width = crate::ast::style::local_space_width(width, basis, self.style);
                self.space(width);
            },
            CompactNode::Row(span) => self.children(span, fmt),
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
                    FractionKind::Display => {
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
                self.with_style(style.fracnum(), |e| e.arg("m:num", num, fmt));
                self.with_style(style.fracden(), |e| e.arg("m:den", den, fmt));
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
                ..
            } => self.array(
                col_aligns,
                cells,
                num_cols,
                add_jot,
                arraystretch_pct != 100 || col_seps != 0 || row_seps != 0,
                fmt,
            ),
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
            Some(chr) if !movable && chr.chars().count() <= 1 => {
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
        let aligns: Vec<ColumnAlign> = arena
            .children(col_aligns)
            .iter()
            .filter_map(|&id| match *arena.node(id) {
                CompactNode::ColAlign(a) => Some(a),
                _ => None,
            })
            .collect();
        if spacing_lost {
            self.loss("OMML output does not preserve array row stretch or rules");
        }
        let cell_ids = arena.children(cells);
        let ncols = usize::from(num_cols.max(1));
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
                    self.with_style(MathStyle::Text, |e| e.cell_content(cell, fmt));
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
                self.val("m:mcJc", column_align_val(aligns[i]));
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
            for &cell in row {
                self.with_style(MathStyle::Text, |e| e.cell(cell, fmt));
            }
            self.close("m:mr");
        }
        self.close("m:m");
    }
}

/// Host-side run formatting for one run.
#[derive(Clone, Copy, Default)]
struct Host {
    bold: bool,
    italic: bool,
    color: Option<[u8; 3]>,
}

/// XML 1.0 `Char` production (XML 1.0 §2.2).
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}')
        || c >= '\u{10000}'
}

#[cfg(test)]
mod tests;
