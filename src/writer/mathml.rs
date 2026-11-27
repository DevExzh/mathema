use crate::ast::style::MathStyle;
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::borrow::Cow;
use std::io::Cursor;

use super::limits::{OutputFailure, OutputLimits};
use super::xml_chars;
use crate::ast::arena::{AstArena, RuleDashes};
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};
use crate::unicode::operators::{OperatorForm, lookup_operator_any, symbol_kind_to_form};
use crate::writer::Writer as MathWriter;

fn boundary_attr(boundaries: &[SpanBoundary]) -> Option<String> {
    if boundaries.is_empty() {
        return None;
    }
    let mut value = String::new();
    for (index, boundary) in boundaries.iter().enumerate() {
        if index > 0 {
            value.push('|');
        }
        value.push_str(match boundary {
            SpanBoundary::SuppressGlue => "padding",
            SpanBoundary::Rule => "rule",
        });
    }
    Some(value)
}

fn packed_boundary_attr(boundaries: &str) -> Option<String> {
    if boundaries.is_empty() {
        return None;
    }
    let mut rest = boundaries;
    let mut value = String::new();
    loop {
        let (name, next) = if let Some(next) = rest.strip_prefix("@{}") {
            ("padding", next)
        } else if let Some(next) = rest.strip_prefix('|') {
            ("rule", next)
        } else {
            break;
        };
        if !value.is_empty() {
            value.push('|');
        }
        value.push_str(name);
        rest = next;
    }
    (!value.is_empty()).then_some(value)
}

/// Classifies a compact node that the MathML writer cannot fully represent.
/// MathML preserves `Color`/`Boxed`/`Phantom`/`Cancel`, so only the genuinely
/// lossy kinds are reported.
fn mathml_loss(arena: &AstArena, node: &CompactNode) -> Option<String> {
    match *node {
        // `\big`-family delimiters keep their size and class, as MathJax writes them.
        CompactNode::Scaled { factor, content } if big_delim_fast(arena, factor, content).is_some() => {
            None
        },
        // `\bigl`/`\bigr`/`\bigm` round-trip through MathJax's texclass.
        CompactNode::MathAtom { atom_type, content }
            if big_delim_class(atom_type).is_some()
                && matches!(
                    *arena.node(content),
                    CompactNode::Scaled { factor, content }
                        if big_delim_fast(arena, factor, content).is_some()
                ) =>
        {
            None
        },
        CompactNode::MathChoice { .. } => {
            Some("MathML output keeps only the applicable branch of \\mathchoice".into())
        },
        CompactNode::MathAtom { atom_type: MathAtomType::Op, .. } => {
            Some("MathML records the compound operator class as an annotation. Native operator spacing can differ.".into())
        },
        CompactNode::MathAtom { atom_type: MathAtomType::Open | MathAtomType::Close, .. } => {
            Some("MathML annotates the TeX fence spacing class. Native operator spacing can differ.".into())
        },
        CompactNode::MathAtom { .. } => {
            Some("MathML output does not preserve the math atom spacing class".into())
        },
        CompactNode::Scaled { .. } => {
            Some("MathML output does not preserve the \\scalebox factor".into())
        },
        CompactNode::RaiseBox { .. } => {
            Some("MathML output does not preserve the \\raisebox shift".into())
        },
        _ => None,
    }
}

/// The amount of MathML metadata emitted by the writer.
///
/// [`RoundTrip`](Self::RoundTrip) keeps Mathema and MathJax metadata used to
/// recover TeX spacing classes, sizes, and table provenance. [`Standard`](Self::Standard)
/// removes that private metadata and canonicalizes colors that are outside the
/// MathML 3 color grammar, making the result suitable for schema-oriented
/// consumers. Standard output can lose formatting details that round-trip
/// output preserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MathmlProfile {
    /// Preserve Mathema's private round-trip metadata (the default).
    #[default]
    RoundTrip,
    /// Emit MathML 3 Presentation markup without private attributes.
    Standard,
}

/// Options for [`MathmlWriter`] and
/// [`crate::convert::ConvertResult::to_mathml_with_options`].
///
/// The default retains the historical round-trip output. Use
/// [`MathmlOptions::standard`] for a schema-oriented document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct MathmlOptions {
    /// Output profile selected for the document.
    pub profile: MathmlProfile,
}

impl MathmlOptions {
    /// Return options for standard MathML 3 Presentation output.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            profile: MathmlProfile::Standard,
        }
    }

    /// Select an output profile.
    #[must_use]
    pub const fn with_profile(mut self, profile: MathmlProfile) -> Self {
        self.profile = profile;
        self
    }
}

pub struct MathmlWriter;

impl MathWriter for MathmlWriter {
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        super::validate_heap_for_write(expr, config)?;
        let needs_compact =
            !expr.meta.is_empty() || crate::ast::normalize::math_expr_has_empty_base_script(expr);
        if needs_compact {
            let mut compact = CompactExpr::from_math_expr_after_validation(expr)?;
            super::normalize_leading_scripts_for_write(&mut compact, config)?;
            return write_compact_validated(&compact);
        }
        let mut writer = MathOutput::new(Writer::new(Cursor::new(Vec::with_capacity(512))));

        let mut math = BytesStart::new("math");
        math.push_attribute(("xmlns", "http://www.w3.org/1998/Math/MathML"));
        math.push_attribute((
            "display",
            if expr.display == DisplayStyle::Block {
                "block"
            } else {
                "inline"
            },
        ));
        writer.write_event(Event::Start(math)).map_err(to_err)?;

        write_nodes(&mut writer, &expr.nodes, expr.display.into(), false)?;
        if writer.met_attributes {
            let compact = CompactExpr::from_math_expr_after_validation(expr)?;
            return write_compact_validated(&compact);
        }

        writer
            .write_event(Event::End(BytesEnd::new("math")))
            .map_err(to_err)?;

        let buf = writer.target.into_inner().into_inner();
        String::from_utf8(buf)
            .map(xml_chars::replace_non_xml_chars)
            .map_err(|e| {
                crate::error::Error::Unsupported(format!(
                    "MathML output contained invalid UTF-8: {e}"
                ))
            })
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        write_with_diagnostics_validated(&prepared)
    }

    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        write_compact_validated(&prepared)
    }
}

impl MathmlWriter {
    /// Write compact MathML while enforcing a byte budget and cancellation.
    pub fn write_compact_with_limits(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<String> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        write_compact_limited_validated(&prepared, limits)
    }

    /// Write compact MathML with diagnostics and output controls.
    pub fn write_compact_with_diagnostics_and_limits(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        write_limited_with_diagnostics(&prepared, limits)
    }

    pub(crate) fn write_compact_with_limits_trusted(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<String> {
        let prepared = super::prepare_leading_scripts_trusted(expr, config)?;
        write_compact_limited_validated(&prepared, limits)
    }

    pub(crate) fn write_compact_with_diagnostics_and_limits_trusted(
        expr: &CompactExpr,
        config: &Config,
        limits: &OutputLimits,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts_trusted(expr, config)?;
        write_limited_with_diagnostics(&prepared, limits)
    }
    /// Fast internal bridge for expressions returned by a parser or checked
    /// conversion result. Public trait calls retain strict arena validation.
    pub(crate) fn write_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<String> {
        super::validate_compact_trusted(expr, config)?;
        write_compact_validated(expr)
    }

    /// Write a compact expression with an explicit MathML output profile.
    ///
    /// The returned diagnostics include both ordinary AST-to-MathML loss and
    /// details removed by the standard profile.
    pub fn write_with_options(
        expr: &CompactExpr,
        config: &Config,
        options: &MathmlOptions,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let prepared = super::prepare_leading_scripts(expr, config)?;
        write_with_options_validated(&prepared, options)
    }

    /// Trusted counterpart of [`Writer::write_compact_with_diagnostics`].
    pub(crate) fn write_compact_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        write_with_diagnostics_validated(expr)
    }
}

/// Write an immutable expression whose arena and depth have already been checked.
pub(crate) fn write_with_diagnostics_validated(
    expr: &CompactExpr,
) -> Result<(String, Vec<Diagnostic>)> {
    write_with_options_validated(expr, &MathmlOptions::default())
}

pub(crate) fn write_with_options_validated(
    expr: &CompactExpr,
    options: &MathmlOptions,
) -> Result<(String, Vec<Diagnostic>)> {
    let (output, replaced) = write_compact_text(expr);
    let mut diagnostics = super::collect_loss_diagnostics(expr, mathml_loss);
    if replaced {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: xml_chars::REPLACED.into(),
            span: None,
        });
    }
    if options.profile == MathmlProfile::Standard {
        let standardized = standardize_mathml(&output)?;
        diagnostics.extend(standardized.diagnostics);
        return Ok((standardized.output, diagnostics));
    }
    Ok((output, diagnostics))
}

/// Write an immutable expression whose arena and depth have already been checked.
pub(crate) fn write_compact_validated(expr: &CompactExpr) -> Result<String> {
    Ok(write_compact_text(expr).0)
}

/// Return the XML and whether any character in that output needed replacement.
fn write_compact_text(expr: &CompactExpr) -> (String, bool) {
    let mut buf = write_math_fast(expr, expr.arena.has_size_scale());
    if buf.missed_size {
        buf = write_math_fast(expr, true);
    }
    // Strings can outlive their referring nodes. Check the actual output only
    // when an interned string or an inline operator glyph could require repair.
    let suspect = buf.invalid_glyph || xml_chars::has_non_xml_char(expr.arena.string_storage());
    if suspect && xml_chars::has_non_xml_char(&buf.target) {
        return (xml_chars::replace_non_xml_chars(buf.target), true);
    }
    (buf.target, false)
}

fn write_limited_with_diagnostics(
    prepared: &CompactExpr,
    limits: &OutputLimits,
) -> Result<(String, Vec<Diagnostic>)> {
    let (output, replaced) = write_compact_limited_validated_with_replacement(prepared, limits)?;
    let mut diagnostics = super::collect_loss_diagnostics(prepared, mathml_loss);
    if replaced {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: xml_chars::REPLACED.into(),
            span: None,
        });
    }
    Ok((output, diagnostics))
}

fn write_compact_limited_validated(expr: &CompactExpr, limits: &OutputLimits) -> Result<String> {
    write_compact_limited_validated_with_replacement(expr, limits).map(|(output, _)| output)
}

fn write_compact_limited_validated_with_replacement(
    expr: &CompactExpr,
    limits: &OutputLimits,
) -> Result<(String, bool)> {
    let mut buf = write_math_fast_with_limits(expr, expr.arena.has_size_scale(), Some(limits));
    if buf.missed_size {
        buf = write_math_fast_with_limits(expr, true, Some(limits));
    }
    let suspect = buf.invalid_glyph || xml_chars::has_non_xml_char(expr.arena.string_storage());
    let raw = buf.finish_limited()?;
    if suspect && xml_chars::has_non_xml_char(&raw) {
        let repaired_len = repaired_xml_len(&raw, limits)?;
        limits.check_bytes(repaired_len)?;
        let output = xml_chars::replace_non_xml_chars(raw);
        Ok((output, true))
    } else {
        Ok((raw, false))
    }
}

fn repaired_xml_len(raw: &str, limits: &OutputLimits) -> Result<usize> {
    let mut length = 0usize;
    for (index, ch) in raw.chars().enumerate() {
        if index & 4095 == 0 {
            limits.check_cancelled()?;
        }
        length = length.saturating_add(if xml_chars::is_xml_char(ch) {
            ch.len_utf8()
        } else {
            '\u{FFFD}'.len_utf8()
        });
    }
    Ok(length)
}

struct StandardizedMathml {
    output: String,
    diagnostics: Vec<Diagnostic>,
}

/// Remove private attributes and repair values outside MathML 3's color type.
/// Parsing the writer's own output through quick-xml keeps this cleanup tied
/// to XML syntax rather than to fragile string matching.
fn standardize_mathml(input: &str) -> Result<StandardizedMathml> {
    let mut reader = quick_xml::Reader::from_str(input);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(input.len())));
    let mut private = 0usize;
    let mut scriptlevel = 0usize;
    let mut colors = 0usize;
    let mut wrapped_scriptlevel = Vec::new();
    loop {
        match reader.read_event().map_err(crate::error::Error::from)? {
            Event::Start(start) => {
                let (start, counts, wrapper) = standardize_start(&start)?;
                private += counts.private;
                scriptlevel += counts.scriptlevel;
                colors += counts.colors;
                if let Some(level) = wrapper.as_deref() {
                    let mut style = BytesStart::new("mstyle");
                    style.push_attribute(("scriptlevel", level));
                    writer
                        .write_event(Event::Start(style))
                        .map_err(crate::error::Error::from)?;
                }
                writer
                    .write_event(Event::Start(start))
                    .map_err(crate::error::Error::from)?;
                wrapped_scriptlevel.push(wrapper.is_some());
            },
            Event::Empty(start) => {
                let (start, counts, wrapper) = standardize_start(&start)?;
                private += counts.private;
                scriptlevel += counts.scriptlevel;
                colors += counts.colors;
                if let Some(level) = wrapper.as_deref() {
                    let mut style = BytesStart::new("mstyle");
                    style.push_attribute(("scriptlevel", level));
                    writer
                        .write_event(Event::Start(style))
                        .map_err(crate::error::Error::from)?;
                }
                writer
                    .write_event(Event::Empty(start))
                    .map_err(crate::error::Error::from)?;
                if wrapper.is_some() {
                    writer
                        .write_event(Event::End(BytesEnd::new("mstyle")))
                        .map_err(crate::error::Error::from)?;
                }
            },
            Event::Eof => break,
            Event::End(end) => {
                writer
                    .write_event(Event::End(end))
                    .map_err(crate::error::Error::from)?;
                if wrapped_scriptlevel.pop().unwrap_or(false) {
                    writer
                        .write_event(Event::End(BytesEnd::new("mstyle")))
                        .map_err(crate::error::Error::from)?;
                }
            },
            event => writer
                .write_event(event)
                .map_err(crate::error::Error::from)?,
        }
    }
    let output = String::from_utf8(writer.into_inner().into_inner()).map_err(|error| {
        crate::error::Error::Unsupported(format!("MathML output contained invalid UTF-8: {error}"))
    })?;
    let mut diagnostics = Vec::new();
    if private != 0 {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Info,
            message: format!(
                "standard MathML profile removed {private} private round-trip attribute{}",
                if private == 1 { "" } else { "s" }
            ),
            span: None,
        });
    }
    if scriptlevel != 0 {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: format!(
                "standard MathML profile removed {scriptlevel} scriptlevel attribute{} from non-mstyle elements",
                if scriptlevel == 1 { "" } else { "s" }
            ),
            span: None,
        });
    }
    if colors != 0 {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: format!(
                "standard MathML profile dropped {colors} unsupported mathcolor value{}",
                if colors == 1 { "" } else { "s" }
            ),
            span: None,
        });
    }
    Ok(StandardizedMathml {
        output,
        diagnostics,
    })
}

#[derive(Default)]
struct StandardizeCounts {
    private: usize,
    scriptlevel: usize,
    colors: usize,
}

fn standardize_start(
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<(
    quick_xml::events::BytesStart<'static>,
    StandardizeCounts,
    Option<String>,
)> {
    let name = start.local_name().into_inner();
    let mut output = quick_xml::events::BytesStart::new(name).into_owned();
    let mut counts = StandardizeCounts::default();
    let mut wrapper = None;
    for attr in start.attributes().with_checks(false) {
        let attr = attr.map_err(|error| crate::error::Error::Xml {
            message: error.to_string(),
        })?;
        let key = attr.key.into_inner();
        if key.starts_with("data-") {
            counts.private += 1;
            continue;
        }
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|error| crate::error::Error::Xml {
                message: error.to_string(),
            })?;
        if key == "scriptlevel" {
            if name == "mtable" {
                if value.trim().parse::<i64>().is_ok() {
                    wrapper = Some(value.into_owned());
                } else {
                    counts.scriptlevel += 1;
                }
                continue;
            }
            if name != "mstyle" {
                counts.scriptlevel += 1;
                continue;
            }
        }
        if key == "mathcolor" && !is_mathml_color(&value) {
            if let Some(rgb) = crate::css_colors::resolve_named_or_hex(&value) {
                let canonical = crate::css_colors::rgb_to_hex(rgb);
                output.push_attribute((key, canonical.as_str()));
            } else {
                counts.colors += 1;
            }
            continue;
        }
        output.push_attribute((key, value.as_ref()));
    }
    Ok((output, counts, wrapper))
}

fn is_mathml_color(value: &str) -> bool {
    let value = value.trim_matches(|character| matches!(character, ' ' | '\t' | '\n' | '\r'));
    let hex = value.strip_prefix('#');
    if let Some(hex) = hex {
        return matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    matches!(
        value.to_ascii_lowercase().as_str(),
        "aqua"
            | "black"
            | "blue"
            | "fuchsia"
            | "gray"
            | "green"
            | "lime"
            | "maroon"
            | "navy"
            | "olive"
            | "purple"
            | "red"
            | "silver"
            | "teal"
            | "white"
            | "yellow"
    )
}

/// Sizes in points are seen against the size of ordinary text: say that on
/// the `<math>` tag, just written, when it is known.
#[cold]
fn write_base_size(buf: &mut XmlBuffer, expr: &CompactExpr) {
    buf.base_pt = crate::ast::attrs::base_size_pt(expr.meta.as_deref());
    if let Some(base) = expr.meta.as_deref().and_then(crate::ast::ExprMeta::base) {
        let tag = buf.len() - 1;
        buf.truncate(tag);
        buf.push_str(" mathsize=\"");
        base.write_scaled_decimal(buf, 1);
        buf.push_str("pt\">");
    }
}

/// The attributes that give an element a font size: where it is one of
/// MathType's logical sizes, with the size itself for the reader.
pub(crate) fn size_attributes(size: FontSize, base_pt: f64) -> String {
    let mut out = String::new();
    let points = |out: &mut String, points: f64| {
        use std::fmt::Write as _;
        // Thousandths of a point.
        let _ = write!(
            out,
            " mathsize=\"{}pt\"",
            (points * 1000.0).round() / 1000.0
        );
    };
    match size {
        FontSize::Points(size) => {
            out.push_str(" mathsize=\"");
            size.write_scaled_decimal(&mut out, 1);
            out.push_str("pt\"");
        },
        FontSize::Logical { level, delta_pt } => {
            let script_level = match level {
                TypeSize::Full => Some('0'),
                TypeSize::Sub => Some('1'),
                TypeSize::Sub2 => Some('2'),
                _ => None,
            };
            match script_level {
                Some(level) if delta_pt.num == 0 => {
                    out.push_str(" scriptlevel=\"");
                    out.push(level);
                    out.push('"');
                },
                _ => points(&mut out, size.scale(base_pt) * base_pt),
            }
            out.push(' ');
            out.push_str(SIZE_ATTRIBUTE);
            out.push_str("=\"");
            out.push_str(logical_size_name(level));
            if delta_pt.num != 0 {
                if delta_pt.num > 0 {
                    out.push('+');
                }
                delta_pt.write_scaled_decimal(&mut out, 1);
            }
            out.push('"');
        },
    }
    out
}

/// A MathType logical size, with the points added to it (`sub+2`).
pub(crate) const SIZE_ATTRIBUTE: &str = "data-mathema-size";

pub(crate) const LOGICAL_SIZES: [(TypeSize, &str); 7] = [
    (TypeSize::Full, "full"),
    (TypeSize::Sub, "sub"),
    (TypeSize::Sub2, "sub2"),
    (TypeSize::Sym, "sym"),
    (TypeSize::SubSym, "subsym"),
    (TypeSize::User1, "user1"),
    (TypeSize::User2, "user2"),
];

fn logical_size_name(level: TypeSize) -> &'static str {
    LOGICAL_SIZES
        .iter()
        .find(|(size, _)| *size == level)
        .map_or("full", |(_, name)| name)
}

/// Write a node with a font size: on its own tag when it is one token that
/// may have it, else on an `mstyle` around it.
#[cold]
#[inline(never)]
fn write_attributed_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    single_argument: bool,
    display_style: MathStyle,
) {
    write_sized(buf, arena, id, display_style, |buf| {
        write_bare_node_fast(buf, arena, id, single_argument, display_style);
    });
}

/// Give the `<mo>` that `write` writes for a node's sign the sign's size.
#[inline(always)]
fn write_sign(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    write: impl FnOnce(&mut XmlBuffer),
) {
    let start = buf.len();
    write(buf);
    if arena.has_attrs() {
        size_sign(buf, arena, id, start);
    }
}

#[cold]
#[inline(never)]
fn size_sign(buf: &mut XmlBuffer, arena: &AstArena, id: NodeId, start: usize) {
    if let Some(size) = arena.attrs(id).and_then(|attrs| attrs.glyph_size) {
        let points = (size.scale(buf.base_pt) * buf.base_pt * 1000.0).round() / 1000.0;
        buf.insert_str(start + "<mo".len(), &format!(" mathsize=\"{points}pt\""));
    }
}

/// Give what `write` writes for a node the node's size.
fn write_sized(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    display_style: MathStyle,
    write: impl FnOnce(&mut XmlBuffer),
) {
    let Some(size) = arena.attrs(id).and_then(|attrs| attrs.size) else {
        return write(buf);
    };
    let start = buf.len();
    let outer = buf.size_per_mille;
    let level = [1.0, 0.7, 0.5][display_style.size_class().min(2)];
    let scale = size.scale(buf.base_pt) / level * 1000.0;
    buf.size_per_mille = scale.round().clamp(1.0, f64::from(u16::MAX)) as u16;
    write(buf);
    buf.size_per_mille = outer;
    if buf.len() == start {
        return;
    }
    let attributes = size_attributes(size, buf.base_pt);
    // A delimiter keeps its tag plain: the reader tells fences by theirs.
    let is_token = match *arena.node(id) {
        CompactNode::Symbol(kind, _) => !matches!(
            kind,
            SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence
        ),
        CompactNode::Number(_)
        | CompactNode::Identifier(_)
        | CompactNode::Operator { .. }
        | CompactNode::Text(..)
        | CompactNode::Space(_) => true,
        _ => false,
    };
    let tag_len = is_token
        .then(|| token_tag_len(&buf[start..]))
        .flatten()
        .filter(|_| !attributes.starts_with(" scriptlevel"));
    match tag_len {
        Some(len) => buf.insert_str(start + len, &attributes),
        None => {
            buf.insert_str(start, &format!("<mstyle{attributes}>"));
            buf.push_str("</mstyle>");
        },
    }
}

/// The length of the opening tag's name when `element` is one token element.
fn token_tag_len(element: &str) -> Option<usize> {
    let name = element
        .strip_prefix('<')?
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()?;
    if !matches!(name, "mi" | "mn" | "mo" | "mtext" | "mspace") {
        return None;
    }
    // Its own two tags: no more of a longer one is gone through.
    let one = element.match_indices('<').nth(2).is_none() && !element.contains(" mathsize=");
    one.then_some(name.len() + 1)
}

fn write_math_fast(expr: &CompactExpr, has_sizes: bool) -> XmlBuffer {
    write_math_fast_with_limits(expr, has_sizes, None)
}

fn write_math_fast_with_limits(
    expr: &CompactExpr,
    has_sizes: bool,
    limits: Option<&OutputLimits>,
) -> XmlBuffer {
    let arena = &expr.arena;
    let mut buf = MathOutput::new_with_limits(String::with_capacity(512), limits.cloned());
    buf.has_sizes = has_sizes;

    if expr.display == DisplayStyle::Block {
        buf.push_str("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">");
    } else {
        buf.push_str("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"inline\">");
    }
    if arena.has_attrs() || expr.meta.is_some() {
        write_base_size(&mut buf, expr);
    }

    write_nodes_fast(
        &mut buf,
        arena,
        arena.children(expr.roots),
        expr.display.into(),
        false,
    );

    buf.push_str("</math>");
    buf
}

/// MathML Core needs the upright variant on individual identifiers. Keep
/// the operator-name font in scope through fractions, accents and scripts.
struct MathOutput<T> {
    target: T,
    limits: Option<OutputLimits>,
    failure: Option<OutputFailure>,
    operator_font: Option<TextStyle>,
    /// The style of the `mstyle` around, which a letter spells itself where
    /// that of the `mstyle` is not enough.
    letter_style: Option<TextStyle>,
    /// The size of ordinary text in points.
    base_pt: f64,
    /// Text size in force from enclosing `mathsize`, in thousandths of the
    /// normal size. MathML sizes are relative to it, TeX's are not.
    size_per_mille: u16,
    /// Whether the expression holds any size declaration, so that the rows
    /// of one that does not skip the size handling. Set by the arena writer
    /// only; the heap writer looks at each row.
    has_sizes: bool,
    /// Whether the heap writer met a node with attributes: the expression is
    /// then written from the compact representation, whose writer looks at
    /// nodes rather than at what carries their attributes.
    met_attributes: bool,
    /// Whether a size declaration was met with `has_sizes` unset.
    missed_size: bool,
    /// An emitted glyph stored directly on a compact node is invalid in XML.
    invalid_glyph: bool,
}

impl<T> MathOutput<T> {
    fn new(target: T) -> Self {
        Self::new_with_limits(target, None)
    }

    fn new_with_limits(target: T, limits: Option<OutputLimits>) -> Self {
        Self {
            target,
            limits,
            failure: None,
            operator_font: None,
            letter_style: None,
            base_pt: f64::from(crate::ast::attrs::DEFAULT_BASE_SIZE_PT),
            size_per_mille: 1000,
            has_sizes: false,
            missed_size: false,
            invalid_glyph: false,
            met_attributes: false,
        }
    }
    /// A space's width in ems of the current font. Only a physical length is
    /// unaffected by the text size in force.
    fn space_width(
        &self,
        width: Rational,
        basis: Option<SpaceBasis>,
        style: MathStyle,
    ) -> Rational {
        let width = crate::ast::style::local_space_width(width, basis, style);
        if basis == Some(SpaceBasis::Physical) && self.size_per_mille != 1000 {
            width.scaled_by(1000, u32::from(self.size_per_mille))
        } else {
            width
        }
    }

    fn with_operator_font<R>(
        &mut self,
        font: Option<TextStyle>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.operator_font, font);
        let result = f(self);
        self.operator_font = previous;
        result
    }

    /// Write what is in a style.
    fn with_letter_style<R>(&mut self, style: TextStyle, write: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.letter_style.replace(style);
        let result = write(self);
        self.letter_style = previous;
        result
    }
}

impl MathOutput<String> {
    fn check_output(&mut self, additional: usize) -> bool {
        if self.failure.is_some() {
            return false;
        }
        let Some(limits) = self.limits.as_ref() else {
            return true;
        };
        if limits.is_cancelled() {
            self.failure = Some(OutputFailure::Cancelled);
            return false;
        }
        if let Some(max_bytes) = limits.max_bytes
            && self.target.len().saturating_add(additional) > max_bytes
        {
            self.failure = Some(OutputFailure::Bytes(max_bytes));
            return false;
        }
        true
    }

    #[inline]
    fn push_str(&mut self, value: &str) {
        if self.check_output(value.len()) {
            self.target.push_str(value);
        }
    }

    #[inline]
    fn push(&mut self, value: char) {
        if self.check_output(value.len_utf8()) {
            self.target.push(value);
        }
    }

    #[inline]
    fn stopped(&self) -> bool {
        self.failure.is_some()
    }

    fn finish_limited(self) -> Result<String> {
        if let Some(failure) = self.failure {
            return Err(failure.into_error());
        }
        Ok(self.target)
    }
}

impl<T> std::ops::Deref for MathOutput<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.target
    }
}

impl<T> std::ops::DerefMut for MathOutput<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.target
    }
}

type XmlWriter = MathOutput<Writer<Cursor<Vec<u8>>>>;
type XmlBuffer = MathOutput<String>;

fn to_err(e: std::io::Error) -> crate::error::Error {
    crate::error::Error::Io(e)
}

fn math_style_attrs(style: MathStyle) -> (&'static str, &'static str) {
    let depth = match style.size_class() {
        0 => "0",
        1 => "1",
        _ => "2",
    };
    (if style.is_display() { "true" } else { "false" }, depth)
}

fn start_math_style(w: &mut XmlWriter, style: MathStyle) -> Result<()> {
    open_math_style(w, math_style_attrs(style))
}

fn open_math_style(w: &mut XmlWriter, (display, depth): (&str, &str)) -> Result<()> {
    let mut tag = BytesStart::new("mstyle");
    tag.push_attribute(("displaystyle", display));
    tag.push_attribute(("scriptlevel", depth));
    w.write_event(Event::Start(tag)).map_err(to_err)?;
    Ok(())
}

fn start_math_style_fast(buf: &mut XmlBuffer, style: MathStyle) {
    open_math_style_fast(buf, math_style_attrs(style));
}

fn open_math_style_fast(buf: &mut XmlBuffer, (display, depth): (&str, &str)) {
    buf.push_str("<mstyle displaystyle=\"");
    buf.push_str(display);
    buf.push_str("\" scriptlevel=\"");
    buf.push_str(depth);
    buf.push_str("\">");
}

fn marker_attrs(marker: u8) -> (&'static str, &'static str) {
    let (display, level) = math_style_attrs(MathStyle::from_switch(marker));
    (
        if MathStyle::marker_is_display(marker) {
            "true"
        } else {
            display
        },
        level,
    )
}

fn start_math_style_marker(w: &mut XmlWriter, marker: u8) -> Result<MathStyle> {
    open_math_style(w, marker_attrs(marker))?;
    Ok(MathStyle::branch_from_switch(marker))
}

fn start_math_style_marker_fast(buf: &mut XmlBuffer, marker: u8) -> MathStyle {
    open_math_style_fast(buf, marker_attrs(marker));
    MathStyle::branch_from_switch(marker)
}

/// If every cell starts in the same style, a nested table can keep its
/// inherited size. Aligned environments retain display layout at that size.
fn inherited_table_marker(
    style: MathStyle,
    add_jot: bool,
    all_start_with: impl Fn(u8) -> bool,
) -> Option<u8> {
    let depth = style.size_class();
    if depth == 0 {
        return None;
    }
    if add_jot && all_start_with(0) {
        return Some(0);
    }
    let display_marker = if depth == 1 {
        MathStyle::DISPLAY_AT_SCRIPT
    } else {
        MathStyle::DISPLAY_AT_SCRIPTSCRIPT
    };
    if all_start_with(display_marker) {
        return Some(display_marker);
    }
    let marker = (depth + 1) as u8;
    all_start_with(marker).then_some(marker)
}

/// What one node of a row means to the declarations around it.
#[derive(Clone, Copy)]
enum RowItem {
    /// A math-style switch (`\scriptstyle`), in force to the end of its row.
    Style(u8),
    /// A text-size switch (`\large`), likewise.
    Size(u8),
    /// A line break.
    Break,
    /// A binary operator or relation, whose spacing depends on what precedes
    /// and follows it.
    Infix,
    Other,
}

fn row_item(node: &MathNode) -> RowItem {
    match node {
        MathNode::StyleSwitch(value) => RowItem::Style(*value),
        MathNode::SizeSwitch(value) => RowItem::Size(*value),
        MathNode::LineBreak => RowItem::Break,
        MathNode::Symbol(SymbolKind::BinaryOp | SymbolKind::Relation, _) => RowItem::Infix,
        _ => RowItem::Other,
    }
}

fn row_item_fast(arena: &AstArena, id: NodeId) -> RowItem {
    match *arena.node(id) {
        CompactNode::StyleSwitch(value) => RowItem::Style(value),
        CompactNode::SizeScale(value) => RowItem::Size(value),
        CompactNode::Newline => RowItem::Break,
        CompactNode::Symbol(SymbolKind::BinaryOp | SymbolKind::Relation, _) => RowItem::Infix,
        _ => RowItem::Other,
    }
}

/// The size of a `\tiny` … `\Huge` declaration. Like the LaTeX writer, read
/// an index past the last one as `\Huge`.
fn size_per_mille(scale: u8) -> u16 {
    let sizes = crate::ast::SIZE_SCALE_PER_MILLE;
    sizes[usize::from(scale).min(sizes.len() - 1)]
}

/// The `mathsize` percentage that takes the size `from` to the size `to`:
/// `120%` for `\large` at the normal size.
fn mathsize_percent(from: u16, to: u16) -> String {
    use std::fmt::Write as _;
    // Thousandths of a percent, rounded.
    let (to, from) = (u32::from(to), u32::from(from));
    let thousandths = (to * 100_000 + from / 2) / from;
    let mut value = String::new();
    let _ = write!(value, "{}.{:03}", thousandths / 1000, thousandths % 1000);
    let trimmed = value.trim_end_matches('0').trim_end_matches('.').len();
    value.truncate(trimmed);
    value.push('%');
    value
}

/// The writer operations a sized row needs of either output.
trait RowOutput {
    fn open_mstyle(&mut self, style: Option<u8>, size: Option<&str>) -> Result<()>;
    fn open_mrow(&mut self) -> Result<()>;
    fn close(&mut self, tag: &str) -> Result<()>;
    fn written(&self) -> usize;
    /// Give the `<mo>` starting at byte `at` the infix form, which an
    /// operator at the edge of an `mstyle` otherwise does not have.
    fn force_infix(&mut self, at: usize);
}

/// Where to insert ` form="infix"` in output starting with an `<mo>` that
/// has no form of its own.
fn infix_insertion(element: &[u8]) -> Option<usize> {
    let tag = element.strip_prefix(b"<mo")?;
    let end = tag.iter().position(|&byte| byte == b'>')?;
    let attributes = &tag[..end];
    let plain = attributes.is_empty()
        || attributes.starts_with(b" ") && !attributes.windows(6).any(|window| window == b" form=");
    plain.then_some(b"<mo".len())
}

const INFIX_FORM: &str = " form=\"infix\"";

impl RowOutput for XmlBuffer {
    fn open_mstyle(&mut self, style: Option<u8>, size: Option<&str>) -> Result<()> {
        self.push_str("<mstyle");
        if let Some(marker) = style {
            let (display, depth) = marker_attrs(marker);
            self.push_str(" displaystyle=\"");
            self.push_str(display);
            self.push_str("\" scriptlevel=\"");
            self.push_str(depth);
            self.push('"');
        }
        if let Some(size) = size {
            self.push_str(" mathsize=\"");
            self.push_str(size);
            self.push('"');
        }
        self.push('>');
        Ok(())
    }
    fn open_mrow(&mut self) -> Result<()> {
        self.push_str("<mrow>");
        Ok(())
    }
    fn close(&mut self, tag: &str) -> Result<()> {
        close_tag(self, tag);
        Ok(())
    }
    fn written(&self) -> usize {
        self.len()
    }
    fn force_infix(&mut self, at: usize) {
        if let Some(offset) = infix_insertion(&self.as_bytes()[at..]) {
            self.insert_str(at + offset, INFIX_FORM);
        }
    }
}

impl RowOutput for XmlWriter {
    fn open_mstyle(&mut self, style: Option<u8>, size: Option<&str>) -> Result<()> {
        let mut tag = BytesStart::new("mstyle");
        if let Some(marker) = style {
            let (display, depth) = marker_attrs(marker);
            tag.push_attribute(("displaystyle", display));
            tag.push_attribute(("scriptlevel", depth));
        }
        if let Some(size) = size {
            tag.push_attribute(("mathsize", size));
        }
        self.write_event(Event::Start(tag)).map_err(to_err)
    }
    fn open_mrow(&mut self) -> Result<()> {
        self.write_event(Event::Start(BytesStart::new("mrow")))
            .map_err(to_err)
    }
    fn close(&mut self, tag: &str) -> Result<()> {
        self.write_event(Event::End(BytesEnd::new(tag)))
            .map_err(to_err)
    }
    fn written(&self) -> usize {
        self.get_ref().get_ref().len()
    }
    fn force_infix(&mut self, at: usize) {
        let cursor = self.get_mut();
        if let Some(offset) = infix_insertion(&cursor.get_ref()[at..]) {
            let at = at + offset;
            cursor.get_mut().splice(at..at, INFIX_FORM.bytes());
            cursor.set_position(cursor.get_ref().len() as u64);
        }
    }
}

fn item_size(item: RowItem) -> Option<u8> {
    match item {
        RowItem::Size(value) => Some(value),
        _ => None,
    }
}

/// A part of a sized row: nodes under the same declarations, or a line break
/// between them.
struct SizedRun<'a, T> {
    /// The style switch in force, and which switch of the row it is.
    style: Option<(usize, u8)>,
    size: Option<u8>,
    nodes: &'a [T],
    is_break: bool,
}

/// Write a row holding size declarations. The nodes under one style are an
/// `mstyle`, as in any row, with an `mstyle` in it for each run of one size;
/// a line break under a size is written between those. `carried` is the size
/// in force where the row starts, and is set to the one in force at its end:
/// a size, unlike a style, lasts across a `\middle`.
///
/// The reader takes an `mstyle` that only has a size for a run of the row
/// around it, so that operators beside it keep their spacing; a row of its
/// own has an `mrow` unless it is all under one style.
#[cold]
fn write_sized_sequence<T, W>(
    w: &mut MathOutput<W>,
    nodes: &[T],
    inherited: MathStyle,
    explicit_row: bool,
    carried: &mut Option<u8>,
    classify: impl Fn(&T) -> RowItem,
    emit: impl Fn(&mut MathOutput<W>, &T, MathStyle) -> Result<()>,
) -> Result<()>
where
    MathOutput<W>: RowOutput,
{
    let mut runs: Vec<SizedRun<'_, T>> = Vec::new();
    let (mut style, mut size) = (None, *carried);
    let mut start = 0;
    for (index, node) in nodes.iter().enumerate() {
        let item = classify(node);
        // A repeated size does not end a run.
        if start < index && size.is_some() && item_size(item) == size {
            continue;
        }
        let is_break = size.is_some() && matches!(item, RowItem::Break);
        if !is_break && !matches!(item, RowItem::Style(_) | RowItem::Size(_)) {
            continue;
        }
        if start < index {
            runs.push(SizedRun {
                style,
                size,
                nodes: &nodes[start..index],
                is_break: false,
            });
        }
        match item {
            RowItem::Style(value) => style = Some((index, value)),
            RowItem::Size(value) => size = Some(value),
            _ => runs.push(SizedRun {
                style,
                size,
                nodes: &nodes[index..=index],
                is_break: true,
            }),
        }
        start = index + 1;
    }
    if start < nodes.len() {
        runs.push(SizedRun {
            style,
            size,
            nodes: &nodes[start..],
            is_break: false,
        });
    }
    *carried = size;

    let one_style = match runs.as_slice() {
        [first, rest @ ..] => {
            first.style.is_some() && rest.iter().all(|run| run.style == first.style)
        },
        [] => false,
    };
    let grouped = explicit_row && !one_style;
    if grouped {
        w.open_mrow()?;
    }
    let outer = w.size_per_mille;
    let mut open_style = None;
    for (index, run) in runs.iter().enumerate() {
        if run.style != open_style {
            if open_style.is_some() {
                w.close("mstyle")?;
            }
            if let Some((_, marker)) = run.style {
                w.open_mstyle(Some(marker), None)?;
            }
            open_style = run.style;
        }
        let active = run.style.map_or(inherited, |(_, marker)| {
            MathStyle::branch_from_switch(marker)
        });
        // A size the context already has needs no `mstyle`.
        let size = run.size.map_or(outer, size_per_mille);
        let sized = !run.is_break && size != outer;
        if sized {
            w.open_mstyle(None, Some(&mathsize_percent(outer, size)))?;
        }
        w.size_per_mille = size;
        // Whether the run meets another node of its line at that edge.
        let adjoins = |other: Option<&SizedRun<'_, T>>, node: fn(&[T]) -> Option<&T>| {
            other.is_some_and(|other| {
                // A style after it is an element beside it.
                (run.style.is_none() || other.style == run.style)
                    && node(other.nodes)
                        .is_some_and(|node| !matches!(classify(node), RowItem::Break))
            })
        };
        let leads = sized && adjoins(index.checked_sub(1).map(|index| &runs[index]), <[T]>::last);
        let trails = sized && adjoins(runs.get(index + 1), <[T]>::first);
        for (position, node) in run.nodes.iter().enumerate() {
            let at = w.written();
            emit(w, node, active)?;
            let edge = position == 0 && leads || position + 1 == run.nodes.len() && trails;
            if edge && matches!(classify(node), RowItem::Infix) {
                w.force_infix(at);
            }
        }
        w.size_per_mille = outer;
        if sized {
            w.close("mstyle")?;
        }
    }
    if open_style.is_some() {
        w.close("mstyle")?;
    }
    if grouped {
        w.close("mrow")?;
    }
    Ok(())
}

/// Consecutive declarations replace one another and a trailing declaration
/// contributes no content. Keep the runs borrowed from their original row.
fn style_runs<T>(
    mut nodes: &[T],
    switch: impl Fn(&T) -> Option<u8>,
) -> impl Iterator<Item = (Option<u8>, &[T])> {
    let mut style = None;
    std::iter::from_fn(move || {
        while let Some((node, rest)) = nodes.split_first()
            && let Some(value) = switch(node)
        {
            style = Some(value);
            nodes = rest;
        }
        let (_, tail) = nodes.split_first()?;
        let n = 1 + tail
            .iter()
            .position(|node| switch(node).is_some())
            .unwrap_or(tail.len());
        let (run, rest) = nodes.split_at(n);
        nodes = rest;
        Some((style, run))
    })
}

/// A single styled run supplies its own grouping via mstyle. All other
/// explicit rows still need mrow, especially in fixed-arity elements.
fn needs_mrow<T>(nodes: &[T], switch: impl Fn(&T) -> Option<u8>) -> bool {
    let mut iter = nodes.iter();
    if !iter.next().is_some_and(|node| switch(node).is_some()) {
        return true;
    }
    if !iter.any(|node| switch(node).is_none()) {
        return true;
    }
    iter.any(|node| switch(node).is_some())
}

fn write_sequence<T>(
    w: &mut XmlWriter,
    nodes: &[T],
    inherited: MathStyle,
    explicit_row: bool,
    carried: &mut Option<u8>,
    classify: impl Fn(&T) -> RowItem,
    emit: impl Fn(&mut XmlWriter, &T, MathStyle) -> Result<()>,
) -> Result<()> {
    if carried.is_some()
        || nodes
            .iter()
            .any(|node| matches!(classify(node), RowItem::Size(_)))
    {
        write_sized_sequence(
            w,
            nodes,
            inherited,
            explicit_row,
            carried,
            classify,
            |w, node, style| emit(w, node, style),
        )?;
        return Ok(());
    }
    let switch = |node: &T| match classify(node) {
        RowItem::Style(value) => Some(value),
        _ => None,
    };
    let grouped = explicit_row && needs_mrow(nodes, switch);
    if grouped {
        w.write_event(Event::Start(BytesStart::new("mrow")))
            .map_err(to_err)?;
    }
    for (style, run) in style_runs(nodes, switch) {
        let active_style = if let Some(marker) = style {
            start_math_style_marker(w, marker)?
        } else {
            inherited
        };
        for node in run {
            emit(w, node, active_style)?;
        }
        if style.is_some() {
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
        }
    }
    if grouped {
        w.write_event(Event::End(BytesEnd::new("mrow")))
            .map_err(to_err)?;
    }
    Ok(())
}

fn write_nodes(
    w: &mut XmlWriter,
    nodes: &[MathNode],
    inherited: MathStyle,
    explicit_row: bool,
) -> Result<()> {
    write_sequence(
        w,
        nodes,
        inherited,
        explicit_row,
        &mut None,
        row_item,
        write_node,
    )
}

#[inline(always)]
fn write_nodes_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
) {
    if buf.stopped() {
        return;
    }
    if buf.has_sizes {
        return write_sized_nodes_fast(buf, arena, nodes, inherited, explicit_row);
    }
    write_style_sequence_fast(buf, arena, nodes, inherited, explicit_row, write_node_fast);
}

#[cold]
#[inline(never)]
fn write_sized_nodes_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
) {
    write_sized_fast(
        buf,
        arena,
        nodes,
        inherited,
        explicit_row,
        &mut None,
        write_node_fast,
    );
}

#[cold]
fn write_sized_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
    carried: &mut Option<u8>,
    emit: impl Fn(&mut XmlBuffer, &AstArena, NodeId, MathStyle),
) {
    let written = write_sized_sequence(
        buf,
        nodes,
        inherited,
        explicit_row,
        carried,
        |&id| row_item_fast(arena, id),
        |buf, &id, style| {
            emit(buf, arena, id, style);
            Ok(())
        },
    );
    debug_assert!(written.is_ok(), "writing to a string cannot fail");
}

#[inline(always)]
fn write_style_sequence_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    inherited: MathStyle,
    explicit_row: bool,
    emit: impl Fn(&mut XmlBuffer, &AstArena, NodeId, MathStyle),
) {
    let switch = |&id: &NodeId| {
        if let CompactNode::StyleSwitch(value) = *arena.node(id) {
            Some(value)
        } else {
            None
        }
    };
    let grouped = explicit_row && needs_mrow(nodes, switch);
    if grouped {
        open_tag(buf, "mrow");
    }
    for (style, run) in style_runs(nodes, switch) {
        let active_style = if let Some(marker) = style {
            start_math_style_marker_fast(buf, marker)
        } else {
            inherited
        };
        for &id in run {
            emit(buf, arena, id, active_style);
        }
        if style.is_some() {
            close_tag(buf, "mstyle");
        }
    }
    if grouped {
        close_tag(buf, "mrow");
    }
}

fn write_node(w: &mut XmlWriter, node: &MathNode, display_style: MathStyle) -> Result<()> {
    write_node_in(w, node, false, display_style)
}

/// An n-ary operator with its limits, without its n-aryand.
fn write_nary_head(
    w: &mut XmlWriter,
    op: char,
    sub: Option<&MathNode>,
    sup: Option<&MathNode>,
    limits: LimitStyle,
    display_style: MathStyle,
) -> Result<()> {
    let Some((tag, movable)) = nary_script_tag(op, limits, sub.is_some(), sup.is_some()) else {
        return write_mo_char(w, op, None);
    };
    w.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(to_err)?;
    write_mo_char(w, op, Some(movable))?;
    if let Some(lower) = sub {
        write_argument(w, lower, display_style.sub())?;
    }
    if let Some(upper) = sup {
        write_argument(w, upper, display_style.sup())?;
    }
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(to_err)?;
    Ok(())
}

/// Compact counterpart of [`write_nary_head`].
fn write_nary_head_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    display_style: MathStyle,
) {
    let CompactNode::Nary {
        op,
        sub,
        sup,
        limits,
        ..
    } = *arena.node(id)
    else {
        return;
    };
    let Some((tag, movable)) = nary_script_tag(op, limits, sub.is_some(), sup.is_some()) else {
        write_sign(buf, arena, id, |buf| write_mo_char_fast(buf, op, None));
        return;
    };
    open_tag(buf, tag);
    write_sign(buf, arena, id, |buf| {
        write_mo_char_fast(buf, op, Some(movable));
    });
    if sub.is_some() {
        write_argument_fast(buf, arena, sub, display_style.sub());
    }
    if sup.is_some() {
        write_argument_fast(buf, arena, sup, display_style.sup());
    }
    close_tag(buf, tag);
}

/// A positional MathML child must occupy exactly one element, even when it
/// contains an operator followed by its operand.
fn write_argument(w: &mut XmlWriter, node: &MathNode, display_style: MathStyle) -> Result<()> {
    write_node_in(w, node, true, display_style)
}

fn write_node_in(
    w: &mut XmlWriter,
    node: &MathNode,
    single_argument: bool,
    display_style: MathStyle,
) -> Result<()> {
    match node {
        MathNode::Number(s) => write_simple_element(w, "mn", s),
        MathNode::Identifier(s) => write_mi(w, s),
        MathNode::Operator(attr) => write_named_operator(w, &attr.name, attr.limit_style, false),
        MathNode::Symbol(kind, s) => write_symbol_node(w, *kind, s),
        MathNode::Text(style, s) => write_text(w, *style, s),
        MathNode::Space(r) => write_mspace(w, *r, None),
        MathNode::ExplicitSpace { width, basis } => write_mspace(
            w,
            w.space_width(*width, Some(*basis), display_style),
            (*basis == SpaceBasis::Physical).then_some(*width),
        ),
        MathNode::Rule {
            width,
            height,
            lift,
        } => {
            let mut elem = BytesStart::new("mspace");
            let width_attr = format!("{}em", width.as_f64());
            let height_attr = format!("{}em", (lift.as_f64() + height.as_f64()).max(0.0));
            let depth_attr = format!("{}em", (-lift.as_f64()).max(0.0));
            elem.push_attribute(("width", width_attr.as_str()));
            elem.push_attribute(("height", height_attr.as_str()));
            elem.push_attribute(("depth", depth_attr.as_str()));
            w.write_event(Event::Empty(elem)).map_err(to_err)?;
            Ok(())
        },

        MathNode::Row(children) => write_nodes(w, children, display_style, true),

        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let elem = BytesStart::new("mrow");
            w.write_event(Event::Start(elem)).map_err(to_err)?;

            // Keep the endpoint role explicit when the glyph pair is
            // ambiguous or one side is null. Ordinary matching delimiters
            // retain their compact historical MathML spelling.
            let endpoint_roles = needs_explicit_endpoint_roles(open, close);
            let stretch_endpoints = endpoint_roles
                || needs_stretchy_endpoints(open, close)
                || fenced_content_needs_stretch(content);
            write_mo_fence(
                w,
                open,
                endpoint_roles.then_some("prefix"),
                stretch_endpoints,
            )?;

            // Content. A size declaration lasts across a `\middle`.
            let mut carried = None;
            for segment in content.split_inclusive(|item| matches!(item, FencedItem::Middle(_))) {
                let (elements, middle) = match segment.split_last() {
                    Some((FencedItem::Middle(delim), prefix)) => (prefix, Some(delim)),
                    _ => (segment, None),
                };
                let classify = |item: &FencedItem| match item {
                    FencedItem::Element(node) => row_item(node),
                    FencedItem::Middle(_) => RowItem::Other,
                };
                let emit = |w: &mut XmlWriter, item: &FencedItem, style| match item {
                    FencedItem::Element(MathNode::Symbol(SymbolKind::Fence, text))
                        if is_literal_fence_glyph(text) =>
                    {
                        write_literal_fence(w, text)
                    },
                    FencedItem::Element(MathNode::Symbol(
                        kind @ (SymbolKind::Open | SymbolKind::Close),
                        text,
                    )) => write_literal_delimiter(w, *kind, text),
                    FencedItem::Element(MathNode::Symbol(SymbolKind::Punctuation, text)) => {
                        write_mo_separator(w, text)
                    },
                    FencedItem::Element(node) => write_node(w, node, style),
                    FencedItem::Middle(_) => Ok(()),
                };
                write_sequence(
                    w,
                    elements,
                    display_style,
                    false,
                    &mut carried,
                    classify,
                    emit,
                )?;
                if let Some(delim) = middle {
                    write_mo_middle(w, delim)?;
                }
            }

            // Closing delimiter
            write_mo_fence(
                w,
                close,
                endpoint_roles.then_some("postfix"),
                stretch_endpoints,
            )?;

            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Subscript { base, sub } => {
            write_scripts(w, base, Some(sub), None, None, display_style)
        },
        MathNode::Superscript { base, sup } => {
            write_scripts(w, base, None, Some(sup), None, display_style)
        },
        MathNode::SubSup { base, sub, sup } => {
            write_scripts(w, base, Some(sub), Some(sup), None, display_style)
        },

        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // The scripts are the n-ary operator's, not its n-aryand's.
            let nary = match &**base {
                MathNode::Nary {
                    op,
                    sub,
                    sup,
                    body,
                    limits,
                } if !is_empty_math_node(body) => Some((*op, sub, sup, body, *limits)),
                _ => None,
            };
            let group = single_argument && nary.is_some();
            if group {
                w.write_event(Event::Start(BytesStart::new("mrow")))
                    .map_err(to_err)?;
            }
            w.write_event(Event::Start(BytesStart::new("mmultiscripts")))
                .map_err(to_err)?;
            match nary {
                Some((op, sub, sup, _, limits)) => {
                    write_nary_head(w, op, sub.as_deref(), sup.as_deref(), limits, display_style)?;
                },
                None => write_argument(w, base, display_style)?,
            }
            w.write_event(Event::Empty(BytesStart::new("mprescripts")))
                .map_err(to_err)?;
            match pre_sub {
                Some(s) => write_argument(w, s, display_style.sup())?,
                None => {
                    w.write_event(Event::Empty(BytesStart::new("none")))
                        .map_err(to_err)?;
                },
            }
            match pre_sup {
                Some(s) => write_argument(w, s, display_style.sup())?,
                None => {
                    w.write_event(Event::Empty(BytesStart::new("none")))
                        .map_err(to_err)?;
                },
            }
            w.write_event(Event::End(BytesEnd::new("mmultiscripts")))
                .map_err(to_err)?;
            if let Some((_, _, _, body, _)) = nary {
                write_node(w, body, display_style)?;
            }
            if group {
                w.write_event(Event::End(BytesEnd::new("mrow")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => write_scripts(
            w,
            base,
            None,
            Some(over),
            Some(*movable_limits),
            display_style,
        ),
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => write_scripts(
            w,
            base,
            Some(under),
            None,
            Some(*movable_limits),
            display_style,
        ),
        MathNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => write_scripts(
            w,
            base,
            Some(under),
            Some(over),
            Some(*movable_limits),
            display_style,
        ),

        MathNode::Fraction { kind, num, den } => {
            let forced = kind.forced_style();
            if let Some(style) = forced {
                start_math_style(w, style)?;
            }
            let style = forced.unwrap_or(display_style);
            let mut elem = BytesStart::new("mfrac");
            if *kind == FractionKind::NoBar {
                elem.push_attribute(("linethickness", "0"));
            } else if *kind == FractionKind::Skewed {
                elem.push_attribute(("bevelled", "true"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_argument(w, num, style.fracnum())?;
            write_argument(w, den, style.fracden())?;
            w.write_event(Event::End(BytesEnd::new("mfrac")))
                .map_err(to_err)?;
            if forced.is_some() {
                w.write_event(Event::End(BytesEnd::new("mstyle")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::Sqrt(inner) => {
            w.write_event(Event::Start(BytesStart::new("msqrt")))
                .map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("msqrt")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Root { index, radicand } => {
            w.write_event(Event::Start(BytesStart::new("mroot")))
                .map_err(to_err)?;
            write_argument(w, radicand, display_style)?;
            write_argument(w, index, MathStyle::ScriptScript)?;
            w.write_event(Event::End(BytesEnd::new("mroot")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let has_body = !is_empty_math_node(body);
            let group = single_argument && has_body;
            if group {
                w.write_event(Event::Start(BytesStart::new("mrow")))
                    .map_err(to_err)?;
            }
            write_nary_head(
                w,
                *op,
                sub.as_deref(),
                sup.as_deref(),
                *limits,
                display_style,
            )?;
            if has_body {
                write_node(w, body, display_style)?;
            }
            if group {
                w.write_event(Event::End(BytesEnd::new("mrow")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::Accent {
            base,
            accent,
            position,
            wide,
        } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            let mut elem = BytesStart::new(tag);
            match position {
                AccentPosition::Above => elem.push_attribute(("accent", "true")),
                AccentPosition::Below => elem.push_attribute(("accentunder", "true")),
            }
            if *accent == '\u{0304}' {
                elem.push_attribute(("data-mathema-accent-decoration", "combining-macron"));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_argument(w, base, display_style)?;
            write_accent_char(w, mathml_accent_glyph(*accent), *wide)?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Array { .. } | MathNode::Matrix { .. } | MathNode::NativeArray { .. } => {
            let (columns, rows, arraystretch, add_jot, array_style, col_seps, row_seps, kind) =
                match node {
                    MathNode::Array {
                        columns,
                        rows,
                        arraystretch,
                        add_jot,
                    } => (
                        Cow::Borrowed(columns.as_slice()),
                        rows,
                        *arraystretch,
                        *add_jot,
                        false,
                        0,
                        0,
                        None,
                    ),
                    MathNode::Matrix {
                        kind,
                        alignment,
                        rows,
                    } => (
                        Cow::Owned(vec![*alignment; table_width(rows, 1)?]),
                        rows,
                        None,
                        false,
                        false,
                        0,
                        0,
                        Some(*kind),
                    ),
                    MathNode::NativeArray {
                        columns,
                        rows,
                        arraystretch,
                        add_jot,
                        array_style,
                        col_seps,
                        row_seps,
                        ..
                    } => (
                        Cow::Borrowed(columns.as_slice()),
                        rows,
                        *arraystretch,
                        *add_jot,
                        *array_style,
                        *col_seps,
                        *row_seps,
                        None,
                    ),
                    _ => unreachable!(),
                };
            let promoted = inherited_table_marker(display_style, add_jot, |marker| {
                rows.iter().flatten().all(|cell| {
                    table_cell_parts(cell).1.first().is_none_or(
                        |node| matches!(node, MathNode::StyleSwitch(value) if *value == marker),
                    )
                })
            });
            let reset_to_text = display_style.size_class() > 0 && promoted.is_none();
            let cell_style = promoted.map_or(MathStyle::Text, MathStyle::branch_from_switch);
            if reset_to_text {
                start_math_style(w, MathStyle::Text)?;
            }
            let mut table = BytesStart::new("mtable");
            for &(name, value) in matrix_attributes(kind) {
                table.push_attribute((name, value));
            }
            let pct = arraystretch
                .map(|value| (value * 100.0).round() as u8)
                .unwrap_or(if kind == Some(MatrixKind::SmallMatrix) {
                    50
                } else {
                    100
                });
            if pct != 100 {
                table.push_attribute(("data-mathema-arraystretch", format!("{pct}%").as_str()));
            }
            if promoted.is_some_and(|marker| MathStyle::branch_from_switch(marker).is_display()) {
                table.push_attribute(("displaystyle", "true"));
            }
            let num_cols = if kind.is_some() {
                columns.len()
            } else {
                table_width(rows, columns.len())?
            };
            let dashes = match node {
                MathNode::NativeArray {
                    dashed_col_seps,
                    dashed_row_seps,
                    ..
                } => RuleDashes {
                    cols: dashed_col_seps & col_seps,
                    rows: dashed_row_seps & row_seps,
                },
                _ => RuleDashes::default(),
            };
            table_rule_attributes(
                col_seps,
                row_seps,
                dashes,
                num_cols,
                rows.len(),
                (array_style
                    || (matches!(node, MathNode::NativeArray { .. })
                        && col_seps == 0
                        && row_seps == 0))
                    .then_some(array_style),
                |key, value| {
                    table.push_attribute((key, value));
                },
            );
            w.write_event(Event::Start(table)).map_err(to_err)?;
            for row in rows {
                w.write_event(Event::Start(BytesStart::new("mtr")))
                    .map_err(to_err)?;
                let mut authored = row.iter();
                let mut i = 0;
                while i < num_cols {
                    let cell = authored.next().map(Vec::as_slice).unwrap_or_default();
                    let cell_span = table_cell_span(cell);
                    let (left_boundaries, right_boundaries) = match cell {
                        [
                            MathNode::SpannedCell {
                                left_boundaries,
                                right_boundaries,
                                ..
                            },
                        ] => (left_boundaries.as_slice(), right_boundaries.as_slice()),
                        _ => (&[][..], &[][..]),
                    };
                    let (cell_override, cell) = table_cell_parts(cell);
                    let mut td = BytesStart::new("mtd");
                    if let Some(align) = cell_override.or_else(|| columns.get(i).copied()) {
                        let val = match align {
                            ColumnAlign::Left => "left",
                            ColumnAlign::Center => "center",
                            ColumnAlign::Right => "right",
                        };
                        td.push_attribute(("columnalign", val));
                    }
                    if cell_span > 1 {
                        td.push_attribute(("columnspan", cell_span.to_string().as_str()));
                    }
                    if let Some(value) = boundary_attr(left_boundaries) {
                        td.push_attribute(("data-mathema-left-boundaries", value.as_str()));
                    }
                    if let Some(value) = boundary_attr(right_boundaries) {
                        td.push_attribute(("data-mathema-right-boundaries", value.as_str()));
                    }
                    if left_boundaries.contains(&SpanBoundary::Rule) {
                        td.push_attribute(("data-mathema-left-rule", "true"));
                    }
                    if right_boundaries.contains(&SpanBoundary::Rule) {
                        td.push_attribute(("data-mathema-right-rule", "true"));
                    }
                    if left_boundaries.contains(&SpanBoundary::SuppressGlue) {
                        td.push_attribute(("data-mathema-left-padding", "true"));
                    }
                    if right_boundaries.contains(&SpanBoundary::SuppressGlue) {
                        td.push_attribute(("data-mathema-right-padding", "true"));
                    }
                    w.write_event(Event::Start(td)).map_err(to_err)?;
                    let cell = if promoted.is_some() && !cell.is_empty() {
                        &cell[1..]
                    } else {
                        cell
                    };
                    let small = promoted.is_none()
                        && kind == Some(MatrixKind::SmallMatrix)
                        && cell
                            .first()
                            .is_some_and(|n| !matches!(n, MathNode::StyleSwitch(_)));
                    if small {
                        start_math_style(w, MathStyle::Script)?;
                        write_nodes(w, cell, MathStyle::Script, false)?;
                        w.write_event(Event::End(BytesEnd::new("mstyle")))
                            .map_err(to_err)?;
                    } else {
                        write_nodes(w, cell, cell_style, false)?;
                    }
                    w.write_event(Event::End(BytesEnd::new("mtd")))
                        .map_err(to_err)?;
                    i = i.saturating_add(cell_span);
                }
                w.write_event(Event::End(BytesEnd::new("mtr")))
                    .map_err(to_err)?;
            }
            w.write_event(Event::End(BytesEnd::new("mtable")))
                .map_err(to_err)?;
            if reset_to_text {
                w.write_event(Event::End(BytesEnd::new("mstyle")))
                    .map_err(to_err)?;
            }
            Ok(())
        },

        MathNode::AlignedCell { content, .. } | MathNode::SpannedCell { content, .. } => {
            write_node_in(w, content, single_argument, display_style)
        },
        MathNode::Phantom(inner, _) => {
            w.write_event(Event::Start(BytesStart::new("mphantom")))
                .map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mphantom")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            let mut elem = BytesStart::new("menclose");
            elem.push_attribute(("notation", "box"));
            if let Some(bg) = bg_color {
                let mut style = String::new();
                if let Some(bc) = border_color {
                    style.push_str("border-color: ");
                    style.push_str(bc);
                    style.push_str("; ");
                }
                style.push_str("background-color: ");
                style.push_str(bg);
                elem.push_attribute(("style", style.as_str()));
            } else if let Some(bc) = border_color {
                let style = format!("border-color: {bc}");
                elem.push_attribute(("style", style.as_str()));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, inner, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Actuarial {
            content: inner,
            text_mode,
        } => {
            let mut elem = BytesStart::new("menclose");
            elem.push_attribute(("notation", "actuarial"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_box_child(w, inner, *text_mode, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Reflect { content, text_mode } => {
            let mut elem = BytesStart::new("mrow");
            elem.push_attribute((
                "data-mathema-reflect",
                if *text_mode { "text" } else { "math" },
            ));
            elem.push_attribute(("style", "display:inline-block; transform:scaleX(-1)"));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_box_child(w, content, *text_mode, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Cancel { stroke, content } => {
            let mut elem = BytesStart::new("menclose");
            let notation = match stroke {
                StrokeKind::Forward => "updiagonalstrike",
                StrokeKind::Backward => "downdiagonalstrike",
                StrokeKind::Cross => "updiagonalstrike downdiagonalstrike",
                StrokeKind::Horizontal => "horizontalstrike",
                StrokeKind::Vertical => "verticalstrike",
            };
            elem.push_attribute(("notation", notation));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node(w, content, display_style)?;
            w.write_event(Event::End(BytesEnd::new("menclose")))
                .map_err(to_err)?;
            Ok(())
        },

        MathNode::Scaled { factor, content } => {
            if let Some(delim) = big_delim(*factor, content) {
                write_big_delim(w, "ORD", delim)
            } else {
                write_node_in(w, content, single_argument, display_style)
            }
        },
        MathNode::RaiseBox { content, .. } => {
            write_node_in(w, content, single_argument, display_style)
        },

        MathNode::Styled(style, children) => {
            let variant = style_to_mathvariant(*style);
            let mut elem = BytesStart::new("mstyle");
            elem.push_attribute(("mathvariant", variant));
            if w.operator_font.is_some()
                && let Some(css) = scoped_font_css(*style)
            {
                elem.push_attribute(("style", css));
            }
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            let font = w.operator_font.map(|_| *style);
            w.with_operator_font(font, |w| {
                w.with_letter_style(*style, |w| write_nodes(w, children, display_style, false))
            })?;
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::Color { color, content } => {
            let mut elem = BytesStart::new("mstyle");
            elem.push_attribute(("mathcolor", color.as_str()));
            w.write_event(Event::Start(elem)).map_err(to_err)?;
            write_node_in(w, content, single_argument, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mstyle")))
                .map_err(to_err)?;
            Ok(())
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let branch = display_style.choose(display, text, script, script_script);
            write_node_in(w, branch, single_argument, display_style)
        },
        MathNode::StyleSwitch(_) | MathNode::SizeSwitch(_) => {
            if single_argument {
                w.write_event(Event::Empty(BytesStart::new("mrow")))
                    .map_err(to_err)?;
            }
            Ok(())
        },
        MathNode::Attributed { .. } => {
            w.met_attributes = true;
            Ok(())
        },
        MathNode::LineBreak => {
            let mut tag = BytesStart::new("mspace");
            tag.push_attribute(("linebreak", "newline"));
            w.write_event(Event::Empty(tag)).map_err(to_err)
        },
        MathNode::MathAtom { atom_type, content } => {
            if let Some(class) = big_delim_class(*atom_type)
                && let MathNode::Scaled {
                    factor,
                    content: inner,
                } = content.as_ref()
                && let Some(delim) = big_delim(*factor, inner)
            {
                write_big_delim(w, class, delim)
            } else if let Some(class) = annotated_atom_class(*atom_type) {
                write_atom_group(w, content, class, false, display_style)
            } else if let Some(kind) = atom_symbol_kind(*atom_type)
                && let MathNode::Symbol(_, s) = content.as_ref()
                && let Some(marker) = atom_marker(kind, s)
            {
                write_symbol_marked(w, kind, s, marker, false)
            } else {
                write_node_in(w, content, single_argument, display_style)
            }
        },
        MathNode::StructuredOperator { content, form } => {
            write_structured_operator_group(w, content, *form, false, display_style)
        },
        MathNode::UndefinedCommand { name, arguments } => {
            // MathJax's error element, holding the command and, as one
            // `<mrow>` each, the groups that may be its arguments. A name
            // that may not be a live command is plain text.
            let error = crate::ast::undefined::writes_as_command(name);
            if error {
                w.write_event(Event::Start(BytesStart::new("merror")))
                    .map_err(to_err)?;
            }
            write_simple_element(w, "mtext", name)?;
            for argument in arguments {
                w.write_event(Event::Start(BytesStart::new("mrow")))
                    .map_err(to_err)?;
                write_nodes(w, argument, display_style, false)?;
                w.write_event(Event::End(BytesEnd::new("mrow")))
                    .map_err(to_err)?;
            }
            if error {
                w.write_event(Event::End(BytesEnd::new("merror")))
                    .map_err(to_err)?;
            }
            Ok(())
        },
    }
}

fn is_directional_bar(kind: SymbolKind, text: &str) -> bool {
    matches!(kind, SymbolKind::Open | SymbolKind::Close) && matches!(text, "|" | "‖")
}

/// The `data-mjx-texclass` a symbol's `<mo>` needs so the reader does not
/// give it the glyph's default class: an ordinary use of an operator glyph
/// (`\mathord{-}`, `\mathord{\lneqq}`, `\dag`), or a non-ASCII glyph used
/// with another operator class. ASCII glyphs whose default class already
/// matches remain unmarked (`:` and `!` keep their natural spellings).
fn symbol_marker(kind: SymbolKind, text: &str) -> Option<&'static str> {
    if is_directional_bar(kind, text) {
        return crate::unicode::operators::texclass_name(kind);
    }
    if kind == SymbolKind::Ordinary && text == "'" {
        return Some("ORD");
    }
    crate::unicode::operators::mo_texclass_marker(text, kind)
}

/// The symbol class a `\mathord`/`\mathbin`/`\mathrel`/`\mathpunct` atom
/// gives the single operator it wraps.
fn atom_symbol_kind(atom: MathAtomType) -> Option<SymbolKind> {
    match atom {
        MathAtomType::Ord => Some(SymbolKind::Ordinary),
        MathAtomType::Bin => Some(SymbolKind::BinaryOp),
        MathAtomType::Rel => Some(SymbolKind::Relation),
        MathAtomType::Punct => Some(SymbolKind::Punctuation),
        _ => None,
    }
}

/// How a class atom around a single `<mo>` is written: `Some(None)` when the
/// glyph reads back with `kind` unmarked (`\mathbin{\dag}`), `Some(Some(_))`
/// with an explicit class marker (`\mathrel{\star}`, `\mathord{\lneqq}`),
/// and `None` when no marker can carry the class (listed fences), so the
/// caller keeps writing the content as before.
fn atom_marker(kind: SymbolKind, text: &str) -> Option<Option<&'static str>> {
    match symbol_marker(kind, text) {
        Some(marker) => Some(Some(marker)),
        None if crate::unicode::operators::mo_class(text).kind == kind => Some(None),
        None => None,
    }
}

fn write_symbol_node(w: &mut XmlWriter, kind: SymbolKind, text: &str) -> Result<()> {
    write_symbol_marked(w, kind, text, symbol_marker(kind, text), false)
}

fn write_symbol_marked(
    w: &mut XmlWriter,
    kind: SymbolKind,
    text: &str,
    marker: Option<&str>,
    non_fence: bool,
) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    let ordinary_literal_bar = kind == SymbolKind::Ordinary && matches!(text, "|" | "‖");
    if non_fence || is_directional_bar(kind, text) {
        elem.push_attribute(("fence", "false"));
    } else {
        match kind {
            SymbolKind::LargeOp => elem.push_attribute(("largeop", "true")),
            SymbolKind::Fence => elem.push_attribute(("fence", "true")),
            _ => {},
        }
    }
    if ordinary_literal_bar {
        elem.push_attribute(("fence", "false"));
        elem.push_attribute(("stretchy", "false"));
    } else if let Some(ch) = text.chars().next() {
        let form = symbol_kind_to_form(kind);
        if let Some(entry) = lookup_operator_any(ch, form) {
            if entry.stretchy {
                elem.push_attribute(("stretchy", "true"));
            }
            if entry.form != OperatorForm::Infix {
                elem.push_attribute(("form", crate::unicode::operators::form_str(entry.form)));
            }
        }
    }
    if let Some(marker) = marker {
        elem.push_attribute(("data-mjx-texclass", marker));
        if marker == "ORD" {
            elem.push_attribute(("lspace", "0em"));
            elem.push_attribute(("rspace", "0em"));
        }
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

fn write_box_child(
    w: &mut XmlWriter,
    child: &MathNode,
    text_size: bool,
    display_style: MathStyle,
) -> Result<()> {
    if text_size {
        let mut style = BytesStart::new("mstyle");
        style.push_attribute(("displaystyle", "false"));
        style.push_attribute(("scriptlevel", "0"));
        w.write_event(Event::Start(style)).map_err(to_err)?;
    }
    write_node(
        w,
        child,
        if text_size {
            MathStyle::Text
        } else {
            display_style
        },
    )?;
    if text_size {
        w.write_event(Event::End(BytesEnd::new("mstyle")))
            .map_err(to_err)?;
    }
    Ok(())
}

fn write_simple_element(w: &mut XmlWriter, tag: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(to_err)?;
    Ok(())
}

fn write_text(w: &mut XmlWriter, style: TextStyle, text: &str) -> Result<()> {
    let text = canonical_mathml_text(text);
    let mut elem = BytesStart::new("mtext");
    if style != TextStyle::Normal {
        elem.push_attribute(("mathvariant", style_to_mathvariant(style)));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text.as_ref())))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mtext")))
        .map_err(to_err)?;
    Ok(())
}

/// CSS width is nonnegative. A logical start margin gives a negative TeX
/// space its signed advance in a MathML layout.
/// `physical` is the full width of a space that is a TeX length, kept for the
/// reader in [`KERN_ATTRIBUTE`]: `width` is relative to the current font, the
/// only unit that follows the size a formula is shown at.
fn write_mspace(w: &mut XmlWriter, width: Rational, physical: Option<Rational>) -> Result<()> {
    let mut tag = BytesStart::new("mspace");
    let mut value = String::new();
    if width.num < 0 {
        tag.push_attribute(("width", "0em"));
        value.push_str("margin-inline-start:");
    }
    width.write_scaled_decimal(&mut value, 1);
    value.push_str("em");
    tag.push_attribute((
        if width.num < 0 { "style" } else { "width" },
        value.as_str(),
    ));
    if let Some(physical) = physical {
        let mut kern = String::new();
        physical.write_scaled_decimal(&mut kern, 10);
        kern.push_str("pt");
        tag.push_attribute((KERN_ATTRIBUTE, kern.as_str()));
    }
    w.write_event(Event::Empty(tag)).map_err(to_err)
}

/// The TeX length of a physical space on its `mspace`, in TeX points.
pub(crate) const KERN_ATTRIBUTE: &str = "data-mathema-kern";

/// The `mathvariant` of an `mi`. One of several letters is upright by itself,
/// one of a single letter italic, whatever style an `mstyle` around has
/// (MathML Core 3.2.2, where `mstyle` has no `mathvariant`).
fn identifier_variant(
    s: &str,
    font: Option<TextStyle>,
    around: Option<TextStyle>,
) -> Option<&'static str> {
    if let Some(font) = font {
        return Some(style_to_mathvariant(font));
    }
    if s.len() > 1 && s.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Some(style_to_mathvariant(around.unwrap_or(TextStyle::Normal)));
    }
    (around == Some(TextStyle::Normal)).then_some("normal")
}

fn write_mi(w: &mut XmlWriter, s: &str) -> Result<()> {
    let mut elem = BytesStart::new("mi");
    if let Some(variant) = identifier_variant(s, w.operator_font, w.letter_style) {
        elem.push_attribute(("mathvariant", variant));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(s)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mi")))
        .map_err(to_err)?;
    Ok(())
}

/// Write an endpoint of a `Fenced` node with its standard MathML operator
/// form. Empty endpoints are null delimiters, represented by an empty fenced
/// operator so one-sided `\left.` / `\right.` structures survive reimport.
fn write_mo_fence(w: &mut XmlWriter, text: &str, form: Option<&str>, stretchy: bool) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("fence", "true"));
    if let Some(form) = form {
        elem.push_attribute(("form", form));
    }
    if stretchy {
        elem.push_attribute(("stretchy", "true"));
        elem.push_attribute(("symmetric", "true"));
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

/// Neutral delimiters already have a middle position here. Other glyphs,
/// including bracket-shaped middles, need their infix role to survive import.
fn middle_form(text: &str) -> Option<&'static str> {
    (!matches!(text, "" | "|" | "‖" | "∣" | "∥" | "/")).then_some("infix")
}

fn write_mo_middle(w: &mut XmlWriter, text: &str) -> Result<()> {
    write_mo_fence(w, text, middle_form(text), true)
}

fn write_mo_separator(w: &mut XmlWriter, text: &str) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("separator", "true"));
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn write_literal_fence(w: &mut XmlWriter, text: &str) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("stretchy", "false"));
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

/// A bracket-shaped symbol inside a fence is content, even when its own
/// symbol class is Open or Close. Mark it as a non-fence so a MathML reader
/// does not mistake it for another endpoint of the enclosing row.
fn write_literal_delimiter(w: &mut XmlWriter, kind: SymbolKind, text: &str) -> Result<()> {
    let marker = symbol_marker(kind, text);
    write_symbol_marked(w, kind, text, marker, true)
}

fn needs_stretchy_endpoints(open: &str, close: &str) -> bool {
    matches!((open, close), ("\\", "/") | ("/", "\\"))
}

fn fenced_content_needs_stretch(content: &[FencedItem]) -> bool {
    content.iter().any(|item| {
        matches!(
            item,
            FencedItem::Element(MathNode::Symbol(SymbolKind::Fence, text))
                if is_literal_fence_glyph(text)
        )
    })
}

fn write_mo_char(w: &mut XmlWriter, ch: char, movable: Option<bool>) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    if let Some(movable) = movable {
        elem.push_attribute(("movablelimits", if movable { "true" } else { "false" }));
    }
    let mut glyph = [0; 4];
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(ch.encode_utf8(&mut glyph))))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    Ok(())
}

fn write_accent_char(w: &mut XmlWriter, ch: char, wide: bool) -> Result<()> {
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("stretchy", if wide { "true" } else { "false" }));
    let mut glyph = [0; 4];
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(ch.encode_utf8(&mut glyph))))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

#[inline]
fn mathml_accent_glyph(ch: char) -> char {
    match ch {
        '\u{0304}' => '\u{203E}',
        _ => ch,
    }
}

/// An explicitly movable integral cannot be represented by the default TeX
/// n-ary placement. Its generic under/over node carries the MathML attribute.
fn write_limit_base(
    w: &mut XmlWriter,
    node: &MathNode,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    match node {
        MathNode::Operator(attr) => write_named_operator(
            w,
            &attr.name,
            if movable {
                LimitStyle::Default
            } else {
                LimitStyle::AboveBelow
            },
            true,
        ),
        MathNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => write_atom_group(w, content, "OP", movable, display_style),
        MathNode::StructuredOperator { content, form } => {
            write_structured_operator_group(w, content, *form, movable, display_style)
        },
        MathNode::Symbol(SymbolKind::LargeOp, text) if movable => {
            if let Some(ch) = single_operator_char(text) {
                write_mo_char(w, ch, Some(true))
            } else {
                write_argument(w, node, display_style)
            }
        },
        MathNode::Row(children) if children.len() == 1 => {
            w.write_event(Event::Start(BytesStart::new("mrow")))
                .map_err(to_err)?;
            write_limit_base(w, &children[0], movable, display_style)?;
            w.write_event(Event::End(BytesEnd::new("mrow")))
                .map_err(to_err)
        },
        _ => write_argument(w, node, display_style),
    }
}

/// A `\big`-family delimiter: its MathJax size and its glyph.
struct BigDelim<'a> {
    size: &'static str,
    glyph: &'a str,
}

/// The `minsize`/`maxsize` MathJax writes for a `\big`-family scale factor.
fn big_delim_size(factor: Rational) -> Option<&'static str> {
    BIG_DELIM_SIZES
        .iter()
        .find(|big| big.scale == factor)
        .map(|big| big.mathjax)
}

fn big_delim(factor: Rational, content: &MathNode) -> Option<BigDelim<'_>> {
    let MathNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, glyph) = content
    else {
        return None;
    };
    Some(BigDelim {
        size: big_delim_size(factor)?,
        glyph,
    })
}

fn big_delim_fast(arena: &AstArena, factor: Rational, content: NodeId) -> Option<BigDelim<'_>> {
    let CompactNode::Symbol(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence, sid) =
        *arena.node(content)
    else {
        return None;
    };
    Some(BigDelim {
        size: big_delim_size(factor)?,
        glyph: arena.str_val(sid),
    })
}

/// MathJax's TeX class for `\bigl`, `\bigr` and `\bigm`.
fn big_delim_class(atom: MathAtomType) -> Option<&'static str> {
    match atom {
        MathAtomType::Open => Some("OPEN"),
        MathAtomType::Close => Some("CLOSE"),
        MathAtomType::Rel => Some("REL"),
        _ => None,
    }
}

/// Write a sized delimiter as MathJax does: a fixed-size stretchy `<mo>` in
/// an `<mrow>` that records the TeX class of `\big`/`\bigl`/`\bigr`/`\bigm`.
fn write_big_delim(w: &mut XmlWriter, class: &str, delim: BigDelim<'_>) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", class));
    w.write_event(Event::Start(group)).map_err(to_err)?;
    let mut elem = BytesStart::new("mo");
    elem.push_attribute(("minsize", delim.size));
    elem.push_attribute(("maxsize", delim.size));
    elem.push_attribute(("fence", "true"));
    elem.push_attribute(("stretchy", "true"));
    elem.push_attribute(("symmetric", "true"));
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(delim.glyph)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn write_big_delim_fast(buf: &mut XmlBuffer, class: &str, delim: BigDelim<'_>) {
    buf.push_str(r#"<mrow data-mjx-texclass=""#);
    buf.push_str(class);
    buf.push_str(r#""><mo minsize=""#);
    buf.push_str(delim.size);
    buf.push_str(r#"" maxsize=""#);
    buf.push_str(delim.size);
    buf.push_str(r#"" fence="true" stretchy="true" symmetric="true">"#);
    xml_escape_into(buf, delim.glyph);
    buf.push_str("</mo>");
    close_tag(buf, "mrow");
}

fn annotated_atom_class(atom: MathAtomType) -> Option<&'static str> {
    match atom {
        MathAtomType::Op => Some("OP"),
        MathAtomType::Open => Some("OPEN"),
        MathAtomType::Close => Some("CLOSE"),
        _ => None,
    }
}

fn write_atom_group(
    w: &mut XmlWriter,
    content: &MathNode,
    class: &str,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", class));
    if movable {
        group.push_attribute(("data-mathema-limits", "movable"));
    }
    w.write_event(Event::Start(group)).map_err(to_err)?;
    if let MathNode::Row(children) = content {
        write_nodes(w, children, display_style, false)?;
    } else {
        write_node(w, content, display_style)?;
    }
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn structured_operator_marker(form: OperatorNameForm) -> &'static str {
    match form {
        OperatorNameForm::Unstarred => "unstarred",
        OperatorNameForm::Starred(LimitStyle::Default) => "starred-default",
        OperatorNameForm::Starred(LimitStyle::AboveBelow) => "starred-limits",
        OperatorNameForm::Starred(LimitStyle::SubSup) => "starred-nolimits",
    }
}

fn write_structured_operator_group(
    w: &mut XmlWriter,
    content: &MathNode,
    form: OperatorNameForm,
    movable: bool,
    display_style: MathStyle,
) -> Result<()> {
    let mut group = BytesStart::new("mrow");
    group.push_attribute(("data-mjx-texclass", "OP"));
    group.push_attribute((
        "data-mathema-structured-operator",
        structured_operator_marker(form),
    ));
    if movable {
        group.push_attribute(("data-mathema-limits", "movable"));
    }
    w.write_event(Event::Start(group)).map_err(to_err)?;
    w.with_operator_font(Some(TextStyle::Normal), |w| match content {
        MathNode::Row(children) => write_nodes(w, children, display_style, false),
        _ => write_node(w, content, display_style),
    })?;
    w.write_event(Event::End(BytesEnd::new("mrow")))
        .map_err(to_err)
}

fn write_named_operator(
    w: &mut XmlWriter,
    name: &str,
    limits: LimitStyle,
    scripted: bool,
) -> Result<()> {
    let mut token = BytesStart::new("mo");
    if let Some(value) = named_movable_attr(limits) {
        token.push_attribute(("movablelimits", value));
    }
    if needs_operator_marker(name, limits, scripted) {
        token.push_attribute(("data-mathema-operator", "name"));
    }
    w.write_event(Event::Start(token)).map_err(to_err)?;
    w.write_event(Event::Text(BytesText::new(name)))
        .map_err(to_err)?;
    w.write_event(Event::End(BytesEnd::new("mo")))
        .map_err(to_err)
}

fn single_operator_char(text: &str) -> Option<char> {
    let text = text.trim();
    let ch = text.chars().next()?;
    (text.len() == ch.len_utf8()).then_some(ch)
}

fn style_to_mathvariant(style: TextStyle) -> &'static str {
    match style {
        TextStyle::Normal => "normal",
        TextStyle::Bold => "bold",
        TextStyle::Italic => "italic",
        TextStyle::BoldItalic => "bold-italic",
        TextStyle::Monospace => "monospace",
        TextStyle::SansSerif => "sans-serif",
        TextStyle::SansSerifBold => "bold-sans-serif",
        TextStyle::SansSerifItalic => "sans-serif-italic",
        TextStyle::SansSerifBoldItalic => "sans-serif-bold-italic",
        TextStyle::DoubleStruck => "double-struck",
        TextStyle::Script => "script",
        TextStyle::BoldScript => "bold-script",
        TextStyle::Fraktur => "fraktur",
        TextStyle::BoldFraktur => "bold-fraktur",
    }
}

// ── Fast direct-string compact writer (bypasses quick_xml) ───────────

/// Escape XML special characters in text content.
/// Math content rarely contains these, so we fast-path the common case.
#[inline]
fn xml_escape_into(buf: &mut XmlBuffer, s: &str) {
    // Fast path: if no special chars, push the whole string at once
    if !s
        .bytes()
        .any(|b| matches!(b, b'<' | b'>' | b'&' | b'"' | b'\''))
    {
        buf.push_str(s);
        return;
    }
    for ch in s.chars() {
        match ch {
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '&' => buf.push_str("&amp;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            _ => buf.push(ch),
        }
    }
}

#[inline]
fn open_tag(buf: &mut XmlBuffer, tag: &str) {
    buf.push('<');
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn close_tag(buf: &mut XmlBuffer, tag: &str) {
    buf.push_str("</");
    buf.push_str(tag);
    buf.push('>');
}

#[inline]
fn simple_element(buf: &mut XmlBuffer, tag: &str, text: &str) {
    open_tag(buf, tag);
    xml_escape_into(buf, text);
    close_tag(buf, tag);
}

#[inline]
fn write_text_fast(buf: &mut XmlBuffer, style: TextStyle, text: &str) {
    let text = canonical_mathml_text(text);
    buf.push_str("<mtext");
    if style != TextStyle::Normal {
        buf.push_str(" mathvariant=\"");
        buf.push_str(style_to_mathvariant(style));
        buf.push('"');
    }
    buf.push('>');
    xml_escape_into(buf, text.as_ref());
    buf.push_str("</mtext>");
}

/// Encode AST text in MathML's token whitespace model. XML whitespace is
/// collapsed and trimmed in token content, so leading/trailing spaces and
/// repeated interior spaces use NBSP after the first rendered space. A single
/// ordinary interior space remains ordinary text.
fn canonical_mathml_text(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
        return Cow::Borrowed(text);
    }

    let normalized: Vec<char> = text
        .chars()
        .map(|c| {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < normalized.len() {
        if normalized[index] != ' ' {
            output.push(normalized[index]);
            index += 1;
            continue;
        }

        let start = index;
        while index < normalized.len() && normalized[index] == ' ' {
            index += 1;
        }
        let count = index - start;
        if start == 0 || index == normalized.len() {
            output.extend(std::iter::repeat_n('\u{00A0}', count));
        } else {
            output.push(' ');
            output.extend(std::iter::repeat_n('\u{00A0}', count.saturating_sub(1)));
        }
    }
    Cow::Owned(output)
}

#[inline]
fn write_node_fast(buf: &mut XmlBuffer, arena: &AstArena, id: NodeId, display_style: MathStyle) {
    write_node_fast_in(buf, arena, id, false, display_style);
}

fn write_argument_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    display_style: MathStyle,
) {
    write_node_fast_in(buf, arena, id, true, display_style);
}

fn write_node_fast_in(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    single_argument: bool,
    display_style: MathStyle,
) {
    if buf.stopped() {
        return;
    }
    if arena.has_attrs() {
        return write_attributed_fast(buf, arena, id, single_argument, display_style);
    }
    write_bare_node_fast(buf, arena, id, single_argument, display_style);
}

#[inline(always)]
fn write_bare_node_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    single_argument: bool,
    display_style: MathStyle,
) {
    match *arena.node(id) {
        CompactNode::Number(sid) => simple_element(buf, "mn", arena.str_val(sid)),
        CompactNode::Identifier(sid) => write_mi_fast(buf, arena.str_val(sid)),
        CompactNode::Operator { name, limits } => {
            write_named_operator_fast(buf, arena.str_val(name), limits, false)
        },
        CompactNode::Symbol(kind, sid) => {
            let s = arena.str_val(sid);
            write_mo_symbol_fast(buf, kind, s);
        },
        CompactNode::Text(style, sid) => write_text_fast(buf, style, arena.str_val(sid)),
        CompactNode::Space(r) => {
            let basis = arena.space_basis(id);
            let width = buf.space_width(r, basis, display_style);
            if width.num < 0 {
                buf.push_str("<mspace width=\"0em\" style=\"margin-inline-start:");
            } else {
                buf.push_str("<mspace width=\"");
            }
            width.write_scaled_decimal(buf, 1);
            buf.push_str("em\"");
            if basis == Some(SpaceBasis::Physical) {
                buf.push(' ');
                buf.push_str(KERN_ATTRIBUTE);
                buf.push_str("=\"");
                r.write_scaled_decimal(buf, 10);
                buf.push_str("pt\"");
            }
            buf.push_str("/>");
        },

        CompactNode::Row(span) => {
            write_nodes_fast(buf, arena, arena.children(span), display_style, true)
        },

        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            buf.push_str("<mrow>");

            let open_s = arena.str_val(open);
            let close_s = arena.str_val(close);
            let endpoint_roles = needs_explicit_endpoint_roles(open_s, close_s);
            let stretch_endpoints = endpoint_roles
                || needs_stretchy_endpoints(open_s, close_s)
                || compact_fenced_content_needs_stretch(arena, content);
            write_mo_fence_fast(
                buf,
                open_s,
                endpoint_roles.then_some("prefix"),
                stretch_endpoints,
            );

            // A size declaration lasts across a `\middle`.
            let mut carried = None;
            for segment in arena
                .children(content)
                .split_inclusive(|&id| matches!(arena.node(id), CompactNode::Middle(_)))
            {
                if let Some((&last, prefix)) = segment.split_last()
                    && let CompactNode::Middle(sid) = *arena.node(last)
                {
                    write_nodes_fenced_fast(buf, arena, prefix, display_style, &mut carried);
                    write_mo_middle_fast(buf, arena.str_val(sid));
                } else {
                    write_nodes_fenced_fast(buf, arena, segment, display_style, &mut carried);
                }
            }

            write_mo_fence_fast(
                buf,
                close_s,
                endpoint_roles.then_some("postfix"),
                stretch_endpoints,
            );

            buf.push_str("</mrow>");
        },

        CompactNode::Subscript { base, sub } => {
            write_scripts_fast(buf, arena, base, sub, NodeId::NONE, None, display_style)
        },
        CompactNode::Superscript { base, sup } => {
            write_scripts_fast(buf, arena, base, NodeId::NONE, sup, None, display_style)
        },
        CompactNode::SubSup { base, sub, sup } => {
            write_scripts_fast(buf, arena, base, sub, sup, None, display_style)
        },

        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // The scripts are the n-ary operator's, not its n-aryand's.
            let nary = match *arena.node(base) {
                // One with a size has it as a whole.
                CompactNode::Nary { body, .. }
                    if !is_empty_compact_node(arena, body)
                        && arena.attrs(base).is_none_or(|attrs| attrs.size.is_none()) =>
                {
                    Some(body)
                },
                _ => None,
            };
            let group = single_argument && nary.is_some();
            if group {
                open_tag(buf, "mrow");
            }
            buf.push_str("<mmultiscripts>");
            match nary {
                Some(_) => write_nary_head_fast(buf, arena, base, display_style),
                None => write_argument_fast(buf, arena, base, display_style),
            }
            buf.push_str("<mprescripts/>");
            if pre_sub.is_some() {
                write_argument_fast(buf, arena, pre_sub, display_style.sub());
            } else {
                buf.push_str("<none/>");
            }
            if pre_sup.is_some() {
                write_argument_fast(buf, arena, pre_sup, display_style.sup());
            } else {
                buf.push_str("<none/>");
            }
            buf.push_str("</mmultiscripts>");
            if let Some(body) = nary {
                write_node_fast(buf, arena, body, display_style);
            }
            if group {
                close_tag(buf, "mrow");
            }
        },

        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            NodeId::NONE,
            over,
            Some(movable_limits),
            display_style,
        ),
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            under,
            NodeId::NONE,
            Some(movable_limits),
            display_style,
        ),
        CompactNode::UnderOver {
            base,
            under,
            over,
            movable_limits,
        } => write_scripts_fast(
            buf,
            arena,
            base,
            under,
            over,
            Some(movable_limits),
            display_style,
        ),

        CompactNode::Fraction { kind, num, den } => {
            let forced = kind.forced_style();
            if let Some(style) = forced {
                start_math_style_fast(buf, style);
            }
            let style = forced.unwrap_or(display_style);
            if kind == FractionKind::NoBar {
                buf.push_str("<mfrac linethickness=\"0\">");
            } else if kind == FractionKind::Skewed {
                buf.push_str("<mfrac bevelled=\"true\">");
            } else {
                buf.push_str("<mfrac>");
            }
            write_argument_fast(buf, arena, num, style.fracnum());
            write_argument_fast(buf, arena, den, style.fracden());
            buf.push_str("</mfrac>");
            if forced.is_some() {
                close_tag(buf, "mstyle");
            }
        },

        CompactNode::Sqrt(inner) => {
            buf.push_str("<msqrt>");
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</msqrt>");
        },

        CompactNode::Root { index, radicand } => {
            buf.push_str("<mroot>");
            write_argument_fast(buf, arena, radicand, display_style);
            write_argument_fast(buf, arena, index, MathStyle::ScriptScript);
            buf.push_str("</mroot>");
        },

        CompactNode::Nary { body, .. } => {
            let has_body = !is_empty_compact_node(arena, body);
            let group = single_argument && has_body;
            if group {
                open_tag(buf, "mrow");
            }
            write_nary_head_fast(buf, arena, id, display_style);
            if has_body {
                write_node_fast(buf, arena, body, display_style);
            }
            if group {
                close_tag(buf, "mrow");
            }
        },

        CompactNode::Accent {
            base,
            accent,
            position,
            wide,
        } => {
            let tag = match position {
                AccentPosition::Above => "mover",
                AccentPosition::Below => "munder",
            };
            buf.push('<');
            buf.push_str(tag);
            match position {
                AccentPosition::Above => buf.push_str(" accent=\"true\""),
                AccentPosition::Below => buf.push_str(" accentunder=\"true\""),
            }
            if accent == '\u{0304}' {
                buf.push_str(r#" data-mathema-accent-decoration="combining-macron""#);
            }
            buf.push('>');
            write_argument_fast(buf, arena, base, display_style);
            let glyph = mathml_accent_glyph(accent);
            write_sign(buf, arena, id, |buf| {
                buf.push_str("<mo stretchy=\"");
                buf.push_str(if wide { "true" } else { "false" });
                buf.push_str("\">");
                write_inline_glyph(buf, glyph);
                buf.push_str("</mo>");
            });
            close_tag(buf, tag);
        },

        CompactNode::AlignedCell { content, .. } | CompactNode::SpannedCell { content, .. } => {
            write_node_fast(buf, arena, content, display_style)
        },
        CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct,
            add_jot,
            array_style,
            col_seps,
            row_seps,
        } => {
            let nc = usize::from(num_cols.max(1));
            let arraystretch = arena.arraystretch_value(id, arraystretch_pct);
            let environment = arena
                .valid_matrix_environment(
                    id,
                    col_aligns,
                    cells,
                    num_cols,
                    arraystretch_pct,
                    add_jot,
                    array_style,
                    col_seps,
                    row_seps,
                )
                .map(|(kind, _)| kind);
            let cell_ids = arena.children(cells);
            let promoted = inherited_table_marker(display_style, add_jot, |marker| {
                cell_ids
                    .iter()
                    .all(|cell| super::cell_after_style(arena, cell, marker).is_some())
            });
            let reset_to_text = display_style.size_class() > 0 && promoted.is_none();
            let cell_style = promoted.map_or(MathStyle::Text, MathStyle::branch_from_switch);

            if reset_to_text {
                start_math_style_fast(buf, MathStyle::Text);
            }

            buf.push_str("<mtable");
            for &(name, value) in matrix_attributes(environment) {
                buf.push(' ');
                buf.push_str(name);
                buf.push_str("=\"");
                buf.push_str(value);
                buf.push('"');
            }
            if (arraystretch - 1.0).abs() > f64::EPSILON {
                buf.push_str(" data-mathema-arraystretch=\"");
                buf.push_str(&(arraystretch * 100.0).to_string());
                buf.push_str("%\"");
            }
            if promoted.is_some_and(|marker| MathStyle::branch_from_switch(marker).is_display()) {
                buf.push_str(" displaystyle=\"true\"");
            }
            table_rule_attributes(
                col_seps,
                row_seps,
                arena.rule_dashes(id),
                nc,
                cell_ids.len().div_ceil(nc.max(1)),
                (array_style || (arena.is_native_array(id) && col_seps == 0 && row_seps == 0))
                    .then_some(array_style),
                |key, value| {
                    buf.push(' ');
                    buf.push_str(key);
                    buf.push_str("=\"");
                    buf.push_str(value);
                    buf.push('"');
                },
            );
            buf.push('>');
            let authored_rows = arena
                .array_environment(id)
                .is_none()
                .then(|| {
                    arena.table_metadata(id).and_then(|metadata| {
                        (!metadata.row_lengths.is_empty()).then_some(metadata.row_lengths.as_ref())
                    })
                })
                .flatten();
            for (row_index, row_chunk) in cell_ids.chunks(nc.max(1)).enumerate() {
                buf.push_str("<mtr>");
                let mut i = 0;
                let authored_target = authored_rows
                    .and_then(|rows| rows.get(row_index))
                    .copied()
                    .map_or(usize::MAX, usize::from);
                let mut authored_count = 0usize;
                while i < nc && authored_count < authored_target {
                    let slot = row_chunk.get(i);
                    let align_nodes = arena.children(col_aligns);
                    let (cell_override, mut nodes) = slot
                        .map(|id| arena.table_cell_parts(id))
                        .unwrap_or((None, &[]));
                    let (cell_span, left_boundaries, right_boundaries) =
                        match slot.map(|&id| *arena.node(id)) {
                            Some(CompactNode::SpannedCell {
                                span,
                                left_boundaries,
                                right_boundaries,
                                ..
                            }) => (
                                arena.effective_cell_span_in_row(row_chunk, i, span),
                                left_boundaries,
                                right_boundaries,
                            ),
                            _ => (1, StrId::EMPTY, StrId::EMPTY),
                        };
                    buf.push_str("<mtd");
                    if let Some(a) = cell_override.or_else(|| {
                        align_nodes
                            .get(i)
                            .and_then(|&align_id| match *arena.node(align_id) {
                                CompactNode::ColAlign(a) => Some(a),
                                _ => None,
                            })
                    }) {
                        let val = match a {
                            ColumnAlign::Left => "left",
                            ColumnAlign::Center => "center",
                            ColumnAlign::Right => "right",
                        };
                        buf.push_str(" columnalign=\"");
                        buf.push_str(val);
                        buf.push('"');
                    }
                    if cell_span > 1 {
                        buf.push_str(" columnspan=\"");
                        buf.push_str(&cell_span.to_string());
                        buf.push('"');
                    }
                    if let Some(value) = packed_boundary_attr(arena.str_val(left_boundaries)) {
                        buf.push_str(" data-mathema-left-boundaries=\"");
                        buf.push_str(&value);
                        buf.push('"');
                    }
                    if let Some(value) = packed_boundary_attr(arena.str_val(right_boundaries)) {
                        buf.push_str(" data-mathema-right-boundaries=\"");
                        buf.push_str(&value);
                        buf.push('"');
                    }
                    if span_boundary_has(arena.str_val(left_boundaries), SpanBoundary::Rule) {
                        buf.push_str(" data-mathema-left-rule=\"true\"");
                    }
                    if span_boundary_has(arena.str_val(right_boundaries), SpanBoundary::Rule) {
                        buf.push_str(" data-mathema-right-rule=\"true\"");
                    }
                    if span_boundary_has(arena.str_val(left_boundaries), SpanBoundary::SuppressGlue)
                    {
                        buf.push_str(" data-mathema-left-padding=\"true\"");
                    }
                    if span_boundary_has(
                        arena.str_val(right_boundaries),
                        SpanBoundary::SuppressGlue,
                    ) {
                        buf.push_str(" data-mathema-right-padding=\"true\"");
                    }
                    buf.push('>');
                    if promoted.is_some() && !nodes.is_empty() {
                        nodes = &nodes[1..];
                    }
                    if promoted.is_none()
                        && environment == Some(MatrixKind::SmallMatrix)
                        && nodes
                            .first()
                            .is_some_and(|&n| !matches!(arena.node(n), CompactNode::StyleSwitch(_)))
                    {
                        start_math_style_fast(buf, MathStyle::Script);
                        write_nodes_fast(buf, arena, nodes, MathStyle::Script, false);
                        close_tag(buf, "mstyle");
                    } else {
                        write_nodes_fast(buf, arena, nodes, cell_style, false);
                    }
                    buf.push_str("</mtd>");
                    authored_count += 1;
                    i = i.saturating_add(cell_span);
                }
                buf.push_str("</mtr>");
            }
            buf.push_str("</mtable>");
            if reset_to_text {
                close_tag(buf, "mstyle");
            }
        },

        CompactNode::Phantom(inner, _) => {
            buf.push_str("<mphantom>");
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</mphantom>");
        },

        CompactNode::Boxed {
            content: inner,
            bg_color,
            border_color,
        } => {
            let bg = arena.str_val(bg_color);
            let border = arena.str_val(border_color);
            if !bg.is_empty() || !border.is_empty() {
                buf.push_str("<menclose notation=\"box\" style=\"");
                if !border.is_empty() {
                    buf.push_str("border-color: ");
                    xml_escape_into(buf, border);
                    buf.push_str("; ");
                }
                if !bg.is_empty() {
                    buf.push_str("background-color: ");
                    xml_escape_into(buf, bg);
                }
                buf.push_str("\">");
            } else {
                buf.push_str("<menclose notation=\"box\">");
            }
            write_node_fast(buf, arena, inner, display_style);
            buf.push_str("</menclose>");
        },
        CompactNode::Actuarial {
            content: inner,
            text_mode,
        } => {
            buf.push_str("<menclose notation=\"actuarial\">");
            write_box_child_fast(buf, arena, inner, text_mode, display_style);
            buf.push_str("</menclose>");
        },

        CompactNode::Reflect { content, text_mode } => {
            buf.push_str("<mrow data-mathema-reflect=\"");
            buf.push_str(if text_mode { "text" } else { "math" });
            buf.push_str("\" style=\"display:inline-block; transform:scaleX(-1)\">");
            write_box_child_fast(buf, arena, content, text_mode, display_style);
            buf.push_str("</mrow>");
        },

        CompactNode::Cancel { stroke, content } => {
            let notation = match stroke {
                StrokeKind::Forward => "updiagonalstrike",
                StrokeKind::Backward => "downdiagonalstrike",
                StrokeKind::Cross => "updiagonalstrike downdiagonalstrike",
                StrokeKind::Horizontal => "horizontalstrike",
                StrokeKind::Vertical => "verticalstrike",
            };
            buf.push_str("<menclose notation=\"");
            buf.push_str(notation);
            buf.push_str("\">");
            write_node_fast(buf, arena, content, display_style);
            buf.push_str("</menclose>");
        },

        CompactNode::Scaled { factor, content } => {
            if let Some(delim) = big_delim_fast(arena, factor, content) {
                write_big_delim_fast(buf, "ORD", delim);
            } else {
                write_node_fast_in(buf, arena, content, single_argument, display_style)
            }
        },
        CompactNode::RaiseBox { content, .. } => {
            write_node_fast_in(buf, arena, content, single_argument, display_style)
        },

        CompactNode::Styled(style, span) => {
            let variant = style_to_mathvariant(style);
            buf.push_str("<mstyle mathvariant=\"");
            buf.push_str(variant);
            if buf.operator_font.is_some()
                && let Some(css) = scoped_font_css(style)
            {
                buf.push_str("\" style=\"");
                buf.push_str(css);
            }
            buf.push_str("\">");
            let font = buf.operator_font.map(|_| style);
            buf.with_operator_font(font, |buf| {
                buf.with_letter_style(style, |buf| {
                    write_nodes_fast(buf, arena, arena.children(span), display_style, false)
                })
            });
            buf.push_str("</mstyle>");
        },

        CompactNode::Color { color, content } => {
            buf.push_str("<mstyle mathcolor=\"");
            xml_escape_into(buf, arena.str_val(color));
            buf.push_str("\">");
            write_node_fast(buf, arena, content, display_style);
            buf.push_str("</mstyle>");
        },

        // Helper nodes
        CompactNode::Middle(sid) => write_mo_middle_fast(buf, arena.str_val(sid)),
        CompactNode::ColAlign(_) => {},
        CompactNode::Rule {
            width,
            height,
            lift,
        } => {
            use std::fmt::Write as _;
            buf.push_str("<mspace");
            let _ = write!(buf, " width=\"{}em\"", width.as_f64());
            let _ = write!(
                buf,
                " height=\"{}em\"",
                (lift.as_f64() + height.as_f64()).max(0.0)
            );
            let _ = write!(buf, " depth=\"{}em\"", (-lift.as_f64()).max(0.0));
            buf.push_str("/>");
        },
        CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_) => {
            if !buf.has_sizes && matches!(*arena.node(id), CompactNode::SizeScale(_)) {
                buf.missed_size = true;
            }
            if single_argument {
                buf.push_str("<mrow/>");
            }
        },
        CompactNode::Newline => buf.push_str(r#"<mspace linebreak="newline"/>"#),
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            let branch = display_style.choose(display, text, script, script_script);
            write_node_fast_in(buf, arena, branch, single_argument, display_style);
        },
        CompactNode::MathAtom { atom_type, content } => {
            if let Some(class) = big_delim_class(atom_type)
                && let CompactNode::Scaled {
                    factor,
                    content: inner,
                } = *arena.node(content)
                && let Some(delim) = big_delim_fast(arena, factor, inner)
            {
                write_big_delim_fast(buf, class, delim);
            } else if let Some(class) = annotated_atom_class(atom_type) {
                write_atom_group_fast(buf, arena, content, class, false, display_style);
            } else if let Some(kind) = atom_symbol_kind(atom_type)
                && let CompactNode::Symbol(_, sid) = *arena.node(content)
                && let Some(marker) = atom_marker(kind, arena.str_val(sid))
            {
                write_mo_symbol_marked_fast(buf, kind, arena.str_val(sid), marker, false);
            } else {
                write_node_fast_in(buf, arena, content, single_argument, display_style);
            }
        },
        CompactNode::StructuredOperator { content, form } => {
            write_structured_operator_group_fast(buf, arena, content, form, false, display_style)
        },
        CompactNode::UndefinedCommand { name, arguments } => {
            // MathJax's error element, holding the command and, as one
            // `<mrow>` each, the groups that may be its arguments.
            let name = arena.str_val(name);
            let error = crate::ast::undefined::writes_as_command(name);
            if error {
                buf.push_str("<merror>");
            }
            buf.push_str("<mtext>");
            xml_escape_into(buf, name);
            buf.push_str("</mtext>");
            for &group in arena.children(arguments) {
                open_tag(buf, "mrow");
                match *arena.node(group) {
                    CompactNode::Row(span) => {
                        write_nodes_fast(buf, arena, arena.children(span), display_style, false)
                    },
                    _ => write_node_fast(buf, arena, group, display_style),
                }
                close_tag(buf, "mrow");
            }
            if error {
                buf.push_str("</merror>");
            }
        },
    }
}

/// Chromium treats MathML's `mathvariant` as a glyph-selection hint and may
/// report it as a normal computed CSS weight. The structured operator scope
/// also carries the equivalent standard CSS declaration, which makes the
/// browser's computed style and its selected glyph agree. Ordinary math
/// output does not use this helper.
fn scoped_font_css(style: TextStyle) -> Option<&'static str> {
    match style {
        TextStyle::Normal => Some("font-style:normal;font-weight:400"),
        TextStyle::Bold => Some("font-weight:700"),
        TextStyle::Italic => Some("font-style:italic"),
        TextStyle::BoldItalic => Some("font-style:italic;font-weight:700"),
        TextStyle::SansSerif => Some("font-family:sans-serif"),
        TextStyle::SansSerifBold => Some("font-family:sans-serif;font-weight:700"),
        TextStyle::SansSerifItalic => Some("font-family:sans-serif;font-style:italic"),
        TextStyle::SansSerifBoldItalic => {
            Some("font-family:sans-serif;font-style:italic;font-weight:700")
        },
        TextStyle::Monospace => Some("font-family:monospace"),
        _ => None,
    }
}

/// MathML has cell spacing but no element for a TeX matrix environment.
fn matrix_attributes(kind: Option<MatrixKind>) -> &'static [(&'static str, &'static str)] {
    match kind {
        Some(MatrixKind::SmallMatrix) => &[
            ("data-mathema-array-environment", "smallmatrix"),
            ("scriptlevel", "1"),
            ("rowspacing", "0.1em"),
            ("columnspacing", "0.2778em"),
        ],
        Some(MatrixKind::Matrix) => &[("data-mathema-array-environment", "matrix")],
        None => &[],
    }
}

#[inline]
fn table_rule_bit(mask: u32, boundary: usize) -> bool {
    boundary < u32::BITS as usize && mask & (1u32 << boundary) != 0
}

/// The MathML line style of a native rule boundary.
fn table_rule_style(mask: u32, dashed: u32, boundary: usize) -> &'static str {
    if !table_rule_bit(mask, boundary) {
        "none"
    } else if table_rule_bit(dashed, boundary) {
        "dashed"
    } else {
        "solid"
    }
}

/// Encode an interior rule mask using MathML's repeat-last line-list rule.
/// Trailing `none` is retained when it is needed to prevent a preceding
/// `solid` from repeating across the rest of the table.
fn table_line_attr(mask: u32, dashed: u32, cells: usize) -> Option<String> {
    let limit = cells.saturating_sub(1).min(u32::BITS as usize);
    if mask & !1 == 0 || !(1..=limit).any(|i| table_rule_bit(mask, i)) {
        return None;
    }
    let style = |i| table_rule_style(mask, dashed, i);
    // Boundary 32 can contribute the final "none" after a solid bit 31.
    let end = (2..=limit)
        .rev()
        .find(|&i| style(i) != style(i - 1))
        .unwrap_or(1);
    let mut value = String::new();
    for i in 1..=end {
        if i > 1 {
            value.push(' ');
        }
        value.push_str(style(i));
    }
    Some(value)
}

fn table_rule_attributes(
    col_seps: u32,
    row_seps: u32,
    dashes: RuleDashes,
    columns: usize,
    rows: usize,
    array_style: Option<bool>,
    mut attr: impl FnMut(&'static str, &str),
) {
    if let Some(value) = table_line_attr(col_seps, dashes.cols, columns) {
        attr("columnlines", &value);
    }
    if let Some(value) = table_line_attr(row_seps, dashes.rows, rows) {
        attr("rowlines", &value);
    }
    let edges = [
        (col_seps, dashes.cols, columns),
        (row_seps, dashes.rows, rows),
    ]
    .map(|(mask, dashed, size)| [0, size].map(|boundary| table_rule_style(mask, dashed, boundary)));
    let frame = match edges.as_flattened() {
        [first, rest @ ..] if *first != "none" && rest.iter().all(|style| style == first) => {
            Some(*first)
        },
        _ => None,
    };
    if let Some(frame) = frame {
        attr("frame", frame);
    } else {
        // A MathML frame covers all four sides in one style. The extension
        // encodes just the partial or mixed exterior edges so standard
        // interior lists remain editable.
        for (key, dashed_key, mask, dashed, size) in [
            (
                "data-mathema-col-seps",
                "data-mathema-dashed-col-seps",
                col_seps,
                dashes.cols,
                columns,
            ),
            (
                "data-mathema-row-seps",
                "data-mathema-dashed-row-seps",
                row_seps,
                dashes.rows,
                rows,
            ),
        ] {
            let exterior = 1 | u32::try_from(size)
                .ok()
                .and_then(|n| 1u32.checked_shl(n))
                .unwrap_or(0);
            let edges = mask & exterior;
            if edges != 0 {
                attr(key, &format!("0x{edges:x}"));
            }
            let dashed_edges = edges & dashed;
            if dashed_edges != 0 {
                attr(dashed_key, &format!("0x{dashed_edges:x}"));
            }
        }
    }
    if let Some(padded) = array_style {
        attr(
            "data-mathema-array-style",
            if padded { "true" } else { "false" },
        );
    }
}

// A named <mo> is a MathML core operator and can carry movablelimits.
// A TeX operator made of multiple elements is not a core <mo>, so its
// enclosing script element must reflect the current math style.
enum ScriptBase {
    Named(LimitStyle),
    Structured(LimitStyle),
    OperatorGroup,
    Other,
}

struct ScriptPlacement {
    stacked: bool,
    base_movable: Option<bool>,
}

impl ScriptBase {
    fn for_node(mut node: &MathNode) -> Self {
        while let MathNode::Row(nodes) = node {
            let [only] = nodes.as_slice() else { break };
            node = only;
        }
        match node {
            MathNode::Operator(attr) => Self::Named(attr.limit_style),
            MathNode::StructuredOperator { form, .. } => Self::Structured(form.limit_style()),
            MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => Self::OperatorGroup,
            _ => Self::Other,
        }
    }

    fn for_compact(arena: &AstArena, mut id: NodeId) -> Self {
        while let CompactNode::Row(span) = *arena.node(id) {
            let &[only] = arena.children(span) else { break };
            id = only;
        }
        match *arena.node(id) {
            CompactNode::Operator { limits, .. } => Self::Named(limits),
            CompactNode::StructuredOperator { form, .. } => Self::Structured(form.limit_style()),
            CompactNode::MathAtom {
                atom_type: MathAtomType::Op,
                ..
            } => Self::OperatorGroup,
            _ => Self::Other,
        }
    }

    fn placement(&self, under_over: Option<bool>, display: MathStyle) -> ScriptPlacement {
        match self {
            Self::Named(style) => {
                let movable = under_over.or(match style {
                    LimitStyle::Default => Some(true),
                    LimitStyle::AboveBelow => Some(false),
                    LimitStyle::SubSup => None,
                });
                ScriptPlacement {
                    stacked: movable.is_some(),
                    base_movable: movable,
                }
            },
            Self::Structured(style) => {
                let movable = under_over.or(match style {
                    LimitStyle::Default => Some(true),
                    LimitStyle::AboveBelow => Some(false),
                    LimitStyle::SubSup => None,
                });
                ScriptPlacement {
                    stacked: movable.is_some_and(|moves| !moves || display.is_display()),
                    base_movable: movable,
                }
            },
            Self::OperatorGroup => ScriptPlacement {
                stacked: under_over.is_some_and(|movable| !movable || display.is_display()),
                base_movable: under_over,
            },
            Self::Other => ScriptPlacement {
                stacked: under_over.is_some(),
                base_movable: under_over,
            },
        }
    }

    fn allows_glyph_accent(&self) -> bool {
        match self {
            Self::Named(style) => *style != LimitStyle::AboveBelow,
            Self::Structured(style) => *style != LimitStyle::AboveBelow,
            Self::Other => true,
            // An explicit `\limits` on a compound operator has no style field
            // in MathAtom. Keep its fixed under/over wrapper unaccented.
            Self::OperatorGroup => false,
        }
    }
}

/// Whether a decoration is a brace or line over an operator group, which no
/// limit of it is.
fn stretches_over_operator(decoration: &str) -> bool {
    matches!(
        decoration,
        "overbrace" | "underbrace" | "overline" | "underline"
    )
}

fn named_movable_attr(style: LimitStyle) -> Option<&'static str> {
    match style {
        LimitStyle::Default => Some("true"),
        LimitStyle::AboveBelow => Some("false"),
        LimitStyle::SubSup => None,
    }
}

/// A single letter or punctuation might otherwise be read as a symbol.
fn needs_operator_marker(name: &str, limits: LimitStyle, scripted: bool) -> bool {
    // Without a marker, a bare built-in name such as lim defaults to movable
    // limits on import. Retain an explicit side-script choice as well.
    if !scripted
        && limits == LimitStyle::SubSup
        && crate::unicode::symbols::operator_has_limits(name)
    {
        return true;
    }
    let mut chars = name.chars();
    !chars.next().is_some_and(|first| {
        first.is_alphabetic() && !chars.as_str().is_empty() && chars.all(char::is_alphabetic)
    })
}

#[inline]
fn is_mathml_accent_glyph(node: &MathNode) -> bool {
    match node {
        MathNode::Symbol(_, text) => {
            let mut chars = text.chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && crate::unicode::symbols::is_accent_glyph(ch)
        },
        MathNode::Row(nodes) if nodes.len() == 1 => is_mathml_accent_glyph(&nodes[0]),
        _ => false,
    }
}

#[inline]
fn is_mathml_accent_glyph_fast(arena: &AstArena, id: NodeId) -> bool {
    match *arena.node(id) {
        CompactNode::Symbol(_, sid) => {
            let mut chars = arena.str_val(sid).chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && crate::unicode::symbols::is_accent_glyph(ch)
        },
        CompactNode::Row(span) if arena.children(span).len() == 1 => {
            is_mathml_accent_glyph_fast(arena, arena.children(span)[0])
        },
        _ => false,
    }
}

fn mathml_script_decoration(node: &MathNode, position: AccentPosition) -> Option<&'static str> {
    let MathNode::Symbol(_, text) = node else {
        return None;
    };
    crate::unicode::symbols::fixed_script_decoration(text, position)
}

fn mathml_script_decoration_fast(
    arena: &AstArena,
    id: NodeId,
    position: AccentPosition,
) -> Option<&'static str> {
    let CompactNode::Symbol(_, sid) = *arena.node(id) else {
        return None;
    };
    crate::unicode::symbols::fixed_script_decoration(arena.str_val(sid), position)
}

fn write_scripts(
    w: &mut XmlWriter,
    base: &MathNode,
    lower: Option<&MathNode>,
    upper: Option<&MathNode>,
    under_over: Option<bool>,
    display: MathStyle,
) -> Result<()> {
    let script_base = ScriptBase::for_node(base);
    let place = script_base.placement(under_over, display);
    let stretches = || {
        upper
            .and_then(|node| mathml_script_decoration(node, AccentPosition::Above))
            .or_else(|| {
                lower.and_then(|node| mathml_script_decoration(node, AccentPosition::Below))
            })
            .is_some_and(stretches_over_operator)
    };
    let accent_value =
        if under_over == Some(false) && (script_base.allows_glyph_accent() || stretches()) {
            "true"
        } else {
            "false"
        };
    let Some(tag) = script_tag(place.stacked, lower.is_some(), upper.is_some()) else {
        return write_argument(w, base, display);
    };
    let mut elem = BytesStart::new(tag);
    if under_over == Some(false) && accent_value == "true" && tag != "munderover" {
        if let Some(name) =
            upper.and_then(|node| mathml_script_decoration(node, AccentPosition::Above))
        {
            elem.push_attribute(("data-mathema-accent-decoration", name));
        } else if let Some(name) =
            lower.and_then(|node| mathml_script_decoration(node, AccentPosition::Below))
        {
            elem.push_attribute(("data-mathema-accent-decoration", name));
        }
    }
    if place.stacked {
        if lower.is_some_and(is_mathml_accent_glyph) {
            elem.push_attribute(("accentunder", accent_value));
        }
        if upper.is_some_and(is_mathml_accent_glyph) {
            elem.push_attribute(("accent", accent_value));
        }
    }
    w.write_event(Event::Start(elem)).map_err(to_err)?;
    if let Some(movable) = place.base_movable {
        write_limit_base(w, base, movable, display)?;
    } else if let MathNode::Operator(attr) = base {
        write_named_operator(w, &attr.name, attr.limit_style, true)?;
    } else {
        write_argument(w, base, display)?;
    }
    if let Some(sub) = lower {
        write_argument(w, sub, display.sub())?;
    }
    if let Some(sup) = upper {
        write_argument(w, sup, display.sup())?;
    }
    w.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(to_err)
}

fn write_scripts_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    base: NodeId,
    lower: NodeId,
    upper: NodeId,
    under_over: Option<bool>,
    display: MathStyle,
) {
    let script_base = ScriptBase::for_compact(arena, base);
    let place = script_base.placement(under_over, display);
    let decoration = |id: NodeId, position| {
        id.is_some()
            .then(|| mathml_script_decoration_fast(arena, id, position))
            .flatten()
    };
    let stretches = || {
        decoration(upper, AccentPosition::Above)
            .or_else(|| decoration(lower, AccentPosition::Below))
            .is_some_and(stretches_over_operator)
    };
    let accent_value =
        if under_over == Some(false) && (script_base.allows_glyph_accent() || stretches()) {
            "true"
        } else {
            "false"
        };
    let Some(tag) = script_tag(place.stacked, lower.is_some(), upper.is_some()) else {
        return write_argument_fast(buf, arena, base, display);
    };
    buf.push('<');
    buf.push_str(tag);
    if under_over == Some(false) && accent_value == "true" && tag != "munderover" {
        if let Some(name) = decoration(upper, AccentPosition::Above) {
            buf.push_str(" data-mathema-accent-decoration=\"");
            buf.push_str(name);
            buf.push('"');
        } else if let Some(name) = decoration(lower, AccentPosition::Below) {
            buf.push_str(" data-mathema-accent-decoration=\"");
            buf.push_str(name);
            buf.push('"');
        }
    }
    if place.stacked {
        for (attr, id) in [("accentunder", lower), ("accent", upper)] {
            if id.is_some() && is_mathml_accent_glyph_fast(arena, id) {
                buf.push(' ');
                buf.push_str(attr);
                buf.push_str("=\"");
                buf.push_str(accent_value);
                buf.push('"');
            }
        }
    }
    buf.push('>');
    if let Some(movable) = place.base_movable {
        write_limit_base_fast(buf, arena, base, movable, display);
    } else if let CompactNode::Operator { name, limits } = *arena.node(base) {
        write_sized(buf, arena, base, display, |buf| {
            write_named_operator_fast(buf, arena.str_val(name), limits, true);
        });
    } else {
        write_argument_fast(buf, arena, base, display);
    }
    if lower.is_some() {
        write_argument_fast(buf, arena, lower, display.sub());
    }
    if upper.is_some() {
        write_argument_fast(buf, arena, upper, display.sup());
    }
    close_tag(buf, tag);
}

/// Select the MathML script element and whether its limits move to the side
/// in compact math style. Fixed side scripts use the sub/sup family directly.
fn nary_script_tag(
    op: char,
    limits: LimitStyle,
    sub: bool,
    sup: bool,
) -> Option<(&'static str, bool)> {
    let movable =
        limits == LimitStyle::Default && !crate::unicode::symbols::is_integral_operator(op);
    let stacked = movable || limits == LimitStyle::AboveBelow;
    script_tag(stacked, sub, sup).map(|tag| (tag, movable))
}

fn script_tag(stacked: bool, sub: bool, sup: bool) -> Option<&'static str> {
    Some(match (stacked, sub, sup) {
        (_, false, false) => return None,
        (true, true, true) => "munderover",
        (true, true, false) => "munder",
        (true, false, true) => "mover",
        (false, true, true) => "msubsup",
        (false, true, false) => "msub",
        (false, false, true) => "msup",
    })
}

#[inline]
fn is_empty_math_node(node: &MathNode) -> bool {
    matches!(node, MathNode::Row(children) if children.is_empty())
}

#[inline]
fn is_empty_compact_node(arena: &AstArena, id: NodeId) -> bool {
    id == NodeId::NONE
        || matches!(*arena.node(id), CompactNode::Row(span) if arena.children(span).is_empty())
}

#[inline]
fn write_mi_fast(buf: &mut XmlBuffer, s: &str) {
    if let Some(variant) = identifier_variant(s, buf.operator_font, buf.letter_style) {
        buf.push_str(r#"<mi mathvariant=""#);
        buf.push_str(variant);
        buf.push_str(r#"">"#);
    } else {
        buf.push_str("<mi>");
    }
    xml_escape_into(buf, s);
    buf.push_str("</mi>");
}

#[inline]
fn write_mo_char_fast(buf: &mut XmlBuffer, ch: char, movable: Option<bool>) {
    buf.push_str(match movable {
        Some(true) => "<mo movablelimits=\"true\">",
        Some(false) => "<mo movablelimits=\"false\">",
        None => "<mo>",
    });
    write_inline_glyph(buf, ch);
    buf.push_str("</mo>");
}

#[inline]
fn write_inline_glyph(buf: &mut XmlBuffer, ch: char) {
    buf.invalid_glyph |= !xml_chars::is_xml_char(ch);
    let mut glyph = [0; 4];
    xml_escape_into(buf, ch.encode_utf8(&mut glyph));
}

/// Whether [`write_bare_limit_base`] writes the node's element itself, without
/// the code that adds the node's size.
fn writes_own_tag(arena: &AstArena, id: NodeId, movable: bool) -> bool {
    match *arena.node(id) {
        CompactNode::Operator { .. }
        | CompactNode::StructuredOperator { .. }
        | CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            ..
        } => true,
        CompactNode::Symbol(SymbolKind::LargeOp, sid) => {
            movable && single_operator_char(arena.str_val(sid)).is_some()
        },
        _ => false,
    }
}

fn write_limit_base_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    movable: bool,
    display_style: MathStyle,
) {
    if arena.has_attrs() && writes_own_tag(arena, id, movable) {
        return write_sized(buf, arena, id, display_style, |buf| {
            write_bare_limit_base(buf, arena, id, movable, display_style);
        });
    }
    write_bare_limit_base(buf, arena, id, movable, display_style);
}

fn write_bare_limit_base(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    movable: bool,
    display_style: MathStyle,
) {
    match *arena.node(id) {
        CompactNode::Operator { name, .. } => write_named_operator_fast(
            buf,
            arena.str_val(name),
            if movable {
                LimitStyle::Default
            } else {
                LimitStyle::AboveBelow
            },
            true,
        ),
        CompactNode::MathAtom {
            atom_type: MathAtomType::Op,
            content,
        } => write_atom_group_fast(buf, arena, content, "OP", movable, display_style),
        CompactNode::StructuredOperator { content, form } => {
            write_structured_operator_group_fast(buf, arena, content, form, movable, display_style)
        },
        CompactNode::Symbol(SymbolKind::LargeOp, sid) if movable => {
            if let Some(ch) = single_operator_char(arena.str_val(sid)) {
                write_mo_char_fast(buf, ch, Some(true));
            } else {
                write_argument_fast(buf, arena, id, display_style);
            }
        },
        CompactNode::Row(span) if arena.children(span).len() == 1 => {
            open_tag(buf, "mrow");
            write_limit_base_fast(buf, arena, arena.children(span)[0], movable, display_style);
            close_tag(buf, "mrow");
        },
        _ => write_argument_fast(buf, arena, id, display_style),
    }
}

fn write_atom_group_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    content: NodeId,
    class: &str,
    movable: bool,
    display_style: MathStyle,
) {
    buf.push_str(r#"<mrow data-mjx-texclass=""#);
    buf.push_str(class);
    if movable {
        buf.push_str(r#"" data-mathema-limits="movable">"#);
    } else {
        buf.push_str(r#"">"#);
    }
    if let CompactNode::Row(span) = *arena.node(content) {
        write_nodes_fast(buf, arena, arena.children(span), display_style, false);
    } else {
        write_node_fast(buf, arena, content, display_style);
    }
    close_tag(buf, "mrow");
}

fn write_structured_operator_group_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    content: NodeId,
    form: OperatorNameForm,
    movable: bool,
    display_style: MathStyle,
) {
    buf.push_str(r#"<mrow data-mjx-texclass="OP""#);
    buf.push_str(r#" data-mathema-structured-operator=""#);
    buf.push_str(structured_operator_marker(form));
    buf.push('"');
    if movable {
        buf.push_str(r#" data-mathema-limits="movable""#);
    }
    buf.push('>');
    buf.with_operator_font(Some(TextStyle::Normal), |buf| match *arena.node(content) {
        CompactNode::Row(span) => {
            write_nodes_fast(buf, arena, arena.children(span), display_style, false)
        },
        _ => write_node_fast(buf, arena, content, display_style),
    });
    close_tag(buf, "mrow");
}

fn write_named_operator_fast(buf: &mut XmlBuffer, name: &str, limits: LimitStyle, scripted: bool) {
    buf.push_str("<mo");
    if let Some(value) = named_movable_attr(limits) {
        buf.push_str(" movablelimits=\"");
        buf.push_str(value);
        buf.push('"');
    }
    if needs_operator_marker(name, limits, scripted) {
        buf.push_str(r#" data-mathema-operator="name""#);
    }
    buf.push('>');
    xml_escape_into(buf, name);
    close_tag(buf, "mo");
}

#[inline]
fn write_box_child_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    child: NodeId,
    text_size: bool,
    display_style: MathStyle,
) {
    if text_size {
        buf.push_str(r#"<mstyle displaystyle="false" scriptlevel="0">"#);
    }
    write_node_fast(
        buf,
        arena,
        child,
        if text_size {
            MathStyle::Text
        } else {
            display_style
        },
    );
    if text_size {
        buf.push_str("</mstyle>");
    }
}

fn write_mo_fence_fast(buf: &mut XmlBuffer, text: &str, form: Option<&str>, stretchy: bool) {
    buf.push_str("<mo fence=\"true\"");
    if let Some(form) = form {
        buf.push_str(" form=\"");
        buf.push_str(form);
        buf.push('"');
    }
    if stretchy {
        buf.push_str(" stretchy=\"true\" symmetric=\"true\"");
    }
    buf.push('>');
    xml_escape_into(buf, text);
    buf.push_str("</mo>");
}

fn write_mo_middle_fast(buf: &mut XmlBuffer, text: &str) {
    write_mo_fence_fast(buf, text, middle_form(text), true);
}

fn write_mo_separator_fast(buf: &mut XmlBuffer, text: &str) {
    buf.push_str("<mo separator=\"true\">");
    xml_escape_into(buf, text);
    buf.push_str("</mo>");
}

fn write_literal_fence_fast(buf: &mut XmlBuffer, text: &str) {
    buf.push_str("<mo stretchy=\"false\">");
    xml_escape_into(buf, text);
    buf.push_str("</mo>");
}

fn write_literal_delimiter_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    id: NodeId,
    kind: SymbolKind,
    text: &str,
) {
    let start = buf.len();
    write_mo_symbol_marked_fast(buf, kind, text, symbol_marker(kind, text), true);
    if let Some(size) = arena.attrs(id).and_then(|attrs| attrs.size) {
        let attributes = size_attributes(size, buf.base_pt);
        buf.insert_str(start + "<mo".len(), &attributes);
    }
}

fn is_literal_fence_glyph(text: &str) -> bool {
    matches!(text, "|" | "‖" | "∣" | "∥")
}

fn write_nodes_fenced_fast(
    buf: &mut XmlBuffer,
    arena: &AstArena,
    nodes: &[NodeId],
    display_style: MathStyle,
    carried: &mut Option<u8>,
) {
    if buf.has_sizes {
        return write_sized_fast(
            buf,
            arena,
            nodes,
            display_style,
            false,
            carried,
            write_fenced_node_fast,
        );
    }
    write_style_sequence_fast(
        buf,
        arena,
        nodes,
        display_style,
        false,
        write_fenced_node_fast,
    );
}

#[inline]
fn write_fenced_node_fast(buf: &mut XmlBuffer, arena: &AstArena, id: NodeId, style: MathStyle) {
    if let CompactNode::Symbol(kind, sid) = *arena.node(id) {
        let text = arena.str_val(sid);
        if is_literal_fence_glyph(text) && kind == SymbolKind::Fence {
            write_literal_fence_fast(buf, text);
            return;
        }
        if matches!(kind, SymbolKind::Open | SymbolKind::Close) {
            write_literal_delimiter_fast(buf, arena, id, kind, text);
            return;
        }
        if kind == SymbolKind::Punctuation {
            write_mo_separator_fast(buf, text);
            return;
        }
    }
    write_node_fast(buf, arena, id, style);
}

fn compact_fenced_content_needs_stretch(arena: &AstArena, content: NodeSpan) -> bool {
    arena.children(content).iter().any(|&id| {
        matches!(
            *arena.node(id),
            CompactNode::Symbol(SymbolKind::Fence, sid)
                if is_literal_fence_glyph(arena.str_val(sid))
        )
    })
}

fn needs_explicit_endpoint_roles(open: &str, close: &str) -> bool {
    crate::unicode::operators::closing_fence(open) != Some(close)
}

fn write_mo_symbol_fast(buf: &mut XmlBuffer, kind: SymbolKind, s: &str) {
    write_mo_symbol_marked_fast(buf, kind, s, symbol_marker(kind, s), false);
}

fn write_mo_symbol_marked_fast(
    buf: &mut XmlBuffer,
    kind: SymbolKind,
    s: &str,
    marker: Option<&str>,
    non_fence: bool,
) {
    buf.push_str("<mo");
    let ordinary_literal_bar = kind == SymbolKind::Ordinary && matches!(s, "|" | "‖");
    if non_fence || is_directional_bar(kind, s) {
        buf.push_str(" fence=\"false\"");
    } else {
        match kind {
            SymbolKind::LargeOp => buf.push_str(" largeop=\"true\""),
            SymbolKind::Fence => buf.push_str(" fence=\"true\""),
            _ => {},
        }
    }
    if ordinary_literal_bar {
        buf.push_str(" fence=\"false\" stretchy=\"false\"");
    } else if let Some(ch) = s.chars().next() {
        let form = symbol_kind_to_form(kind);
        if let Some(entry) = lookup_operator_any(ch, form) {
            if entry.stretchy {
                buf.push_str(" stretchy=\"true\"");
            }
            if entry.form != OperatorForm::Infix {
                buf.push_str(" form=\"");
                buf.push_str(crate::unicode::operators::form_str(entry.form));
                buf.push('"');
            }
        }
    }
    if let Some(marker) = marker {
        buf.push_str(r#" data-mjx-texclass=""#);
        buf.push_str(marker);
        buf.push('"');
        if marker == "ORD" {
            buf.push_str(r#" lspace="0em" rspace="0em""#);
        }
    }
    buf.push('>');
    xml_escape_into(buf, s);
    buf.push_str("</mo>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::normalize::{normalize_compact_expr, normalize_expr};
    use crate::reader::Reader;
    use crate::reader::latex::LatexReader;
    use crate::reader::mathml::MathmlReader;

    fn write(node: MathNode) -> String {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        MathmlWriter::write(&expr, &config).unwrap()
    }

    #[test]
    fn number() {
        let output = write(MathNode::Number("42".into()));
        assert!(output.contains("<mn>42</mn>"));
    }

    #[test]
    fn identifier() {
        let output = write(MathNode::Identifier("x".into()));
        assert!(output.contains("<mi>x</mi>"));
    }

    #[test]
    fn a_spanning_cell_keeps_the_following_authored_cell() {
        let xml = r#"<math><mtable><mtr><mtd columnspan="2"><mn>8</mn></mtd><mtd columnalign="right"><mn>9</mn></mtd></mtr><mtr><mn>1</mn><mn>2</mn><mn>3</mn></mtr></mtable></math>"#;
        let parsed = crate::from_mathml(xml).unwrap();
        let heap = parsed.expr();
        let compact = CompactExpr::from_math_expr(&heap).unwrap();
        let cfg = Config::default();
        let result = MathmlWriter::write(&heap, &cfg).unwrap();
        assert!(
            result.contains(r#"<mtd columnalign="right"><mn>9</mn></mtd>"#),
            "{result}"
        );
        assert_eq!(result, MathmlWriter::write_compact(&compact, &cfg).unwrap());
        assert_eq!(result, parsed.to_mathml().unwrap());
    }

    #[test]
    fn mixed_span_wrappers_do_not_change_table_shape() {
        let span = MathNode::SpannedCell {
            span: 6,
            alignment: ColumnAlign::Right,
            left_boundaries: vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
            right_boundaries: vec![SpanBoundary::Rule, SpanBoundary::SuppressGlue],
            content: Box::new(MathNode::Number("1".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::Matrix,
            alignment: ColumnAlign::Left,
            rows: vec![vec![
                vec![span, MathNode::Number("2".into())],
                vec![MathNode::Number("3".into())],
            ]],
        }]);
        let config = Config::default();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let xml = MathmlWriter::write(&expr, &config).unwrap();
        assert_eq!(xml.matches("<mtd ").count(), 2);
        assert!(!xml.contains("columnspan"));
        assert!(!xml.contains("data-mathema-left-rule"));
        assert_eq!(xml, MathmlWriter::write_compact(&compact, &config).unwrap());
        #[cfg(feature = "render")]
        {
            let opts = crate::render::RenderOptions::default();
            assert_eq!(
                crate::render::render_to_svg(&expr, &opts).unwrap(),
                crate::render::render_to_svg_compact(&compact, &opts).unwrap()
            );
        }
    }

    #[test]
    fn ordered_span_boundaries_survive_mathml_roundtrip() {
        let cell = MathNode::SpannedCell {
            span: 1,
            alignment: ColumnAlign::Left,
            left_boundaries: vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
            right_boundaries: vec![SpanBoundary::Rule, SpanBoundary::Rule],
            content: Box::new(MathNode::Identifier("x".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Left],
            rows: vec![vec![vec![cell]]],
            arraystretch: None,
            add_jot: false,
        }]);
        let config = Config::default();
        let xml = MathmlWriter::write(&expr, &config).unwrap();
        assert!(
            xml.contains(r#"data-mathema-left-boundaries="padding|rule""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"data-mathema-right-boundaries="rule|rule""#),
            "{xml}"
        );
        let reparsed = crate::from_mathml(&xml).unwrap();
        assert_eq!(reparsed.expr(), expr);
    }

    #[test]
    fn expansive_heap_spans_respect_the_compact_grid_limit() {
        let cell = MathNode::SpannedCell {
            span: 32768,
            alignment: ColumnAlign::Left,
            left_boundaries: Vec::new(),
            right_boundaries: Vec::new(),
            content: Box::new(MathNode::Number("1".into())),
        };
        let rows = vec![vec![vec![cell]]; 2];
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::Matrix,
            alignment: ColumnAlign::Left,
            rows,
        }]);
        let is_grid_error = |err: crate::Error| {
            matches!(
                err,
                crate::Error::LimitExceeded {
                    kind: crate::error::LimitKind::ListLength,
                    limit: AstArena::MAX_CHILDREN
                }
            )
        };
        assert!(is_grid_error(
            CompactExpr::from_math_expr(&expr).unwrap_err()
        ));
        assert!(is_grid_error(
            MathmlWriter::write(&expr, &Config::default()).unwrap_err()
        ));
        #[cfg(feature = "render")]
        assert!(is_grid_error(
            crate::render::render_to_svg(&expr, &crate::render::RenderOptions::default())
                .unwrap_err()
        ));
    }

    #[test]
    fn minus_symbol_class_survives_mathml_round_trip() {
        for (source, expected_class) in [
            (r"x-y", "binary"),
            (r"-x", "binary"),
            (r"\mathord{-}", "ordinary"),
            (r"\mathord{−}", "ordinary"),
        ] {
            let parsed = crate::from_latex(source).unwrap();
            let mathml = parsed.to_mathml().unwrap();
            assert!(
                mathml.contains(if expected_class == "ordinary" {
                    r#"data-mjx-texclass="ORD" lspace="0em" rspace="0em""#
                } else {
                    "<mo>−</mo>"
                }),
                "{source}: {mathml}"
            );
            let roundtrip = crate::from_mathml(&mathml).unwrap();
            assert_eq!(mathml, roundtrip.to_mathml().unwrap(), "{source}");
            #[cfg(feature = "render")]
            assert_eq!(
                parsed.to_svg().unwrap(),
                roundtrip.to_svg().unwrap(),
                "{source}"
            );
        }

        let unmarked =
            crate::from_mathml(r#"<math><mi>x</mi><mo>−</mo><mi>y</mi></math>"#).unwrap();
        let marked = unmarked.to_mathml().unwrap();
        assert!(marked.contains("<mo>−</mo>"));
        assert_eq!(
            marked,
            crate::from_latex("x-y").unwrap().to_mathml().unwrap()
        );
    }

    #[test]
    fn explicit_nondefault_ascii_classes_survive_heap_and_compact_mathml() {
        let config = Config::default();
        for input in [
            r#"<math><mo data-mjx-texclass="REL">@</mo></math>"#,
            r#"<math><mo data-mjx-texclass="REL">?</mo></math>"#,
            r#"<math><mfenced><mo data-mjx-texclass="REL">@</mo></mfenced></math>"#,
        ] {
            let heap = MathmlReader::parse(input, &config).unwrap().expr;
            let compact = MathmlReader::parse_compact(input, &config).unwrap().expr;
            let heap_xml = MathmlWriter::write(&heap, &config).unwrap();
            let compact_xml = MathmlWriter::write_compact(&compact, &config).unwrap();
            assert_eq!(heap_xml, compact_xml, "{input}");
            assert!(
                heap_xml.contains(r#"data-mjx-texclass="REL""#),
                "{heap_xml}"
            );

            let reread_heap = MathmlReader::parse(&heap_xml, &config).unwrap().expr;
            let reread_compact = MathmlReader::parse_compact(&heap_xml, &config)
                .unwrap()
                .expr;
            assert_eq!(
                MathmlWriter::write(&reread_heap, &config).unwrap(),
                heap_xml
            );
            assert_eq!(
                MathmlWriter::write_compact(&reread_compact, &config).unwrap(),
                heap_xml
            );
        }

        let natural = crate::from_latex(r"a:b! a?b").unwrap().to_mathml().unwrap();
        assert!(!natural.contains("data-mjx-texclass"), "{natural}");

        for source in [r"\mathrel{@}", r"\mathrel{?}"] {
            let mathml = crate::from_latex(source).unwrap().to_mathml().unwrap();
            assert!(
                mathml.contains(r#"data-mjx-texclass="REL""#),
                "{source}: {mathml}"
            );
            assert_eq!(
                crate::from_mathml(&mathml).unwrap().to_mathml().unwrap(),
                mathml,
                "{source}"
            );
        }
    }

    #[test]
    fn text_style_and_whitespace_round_trip_on_heap_and_compact_paths() {
        let styles = [
            TextStyle::Normal,
            TextStyle::Bold,
            TextStyle::Italic,
            TextStyle::BoldItalic,
            TextStyle::Monospace,
            TextStyle::SansSerif,
            TextStyle::SansSerifBold,
            TextStyle::SansSerifItalic,
            TextStyle::SansSerifBoldItalic,
            TextStyle::DoubleStruck,
            TextStyle::Script,
            TextStyle::BoldScript,
            TextStyle::Fraktur,
            TextStyle::BoldFraktur,
        ];
        let config = Config::default();
        let texts = [
            (
                "  a  < b  ",
                "\u{00a0}\u{00a0}a \u{00a0}< b\u{00a0}\u{00a0}",
            ),
            ("\ta\nb\r", "\u{00a0}a b\u{00a0}"),
            ("\u{00a0} a\u{00a0}", "\u{00a0} a\u{00a0}"),
        ];
        for style in styles {
            for &(source, canonical) in &texts {
                let expr = MathExpr::inline(vec![MathNode::Text(style, source.into())]);
                let heap = MathmlWriter::write(&expr, &config).unwrap();
                let compact_expr = CompactExpr::from_math_expr(&expr).unwrap();
                let compact = MathmlWriter::write_compact(&compact_expr, &config).unwrap();
                assert_eq!(heap, compact, "style: {style:?}, source: {source:?}");
                if style == TextStyle::Normal {
                    assert!(heap.contains("<mtext>"), "{heap}");
                } else {
                    assert!(
                        heap.contains(&format!("mathvariant=\"{}\"", style_to_mathvariant(style))),
                        "style: {style:?}, output: {heap}"
                    );
                }

                let parsed = crate::reader::mathml::MathmlReader::parse(
                    &heap,
                    &config.with_display(DisplayStyle::Inline),
                )
                .unwrap()
                .expr;
                assert_eq!(
                    parsed.nodes,
                    vec![MathNode::Text(style, canonical.into())],
                    "style: {style:?}, source: {source:?}"
                );
                let rewritten = MathmlWriter::write(&parsed, &config).unwrap();
                assert_eq!(rewritten, heap, "style: {style:?}, source: {source:?}");
            }
        }
    }

    #[test]
    fn ordinary_vertical_bars_keep_their_mathml_class() {
        let config = Config::default();
        for source in [
            r"\vert",
            r"\Vert",
            r"\left(\vert x\vert\right)",
            r"\Set{\vert x\vert|P}",
        ] {
            let original = crate::from_latex(source).unwrap();
            let mathml = original.to_mathml().unwrap();
            assert!(mathml.contains("fence=\"false\""), "{source}: {mathml}");
            assert!(mathml.contains("stretchy=\"false\""), "{source}: {mathml}");
            let imported = crate::from_mathml(&mathml).unwrap();
            assert_eq!(imported.to_mathml().unwrap(), mathml, "{source}");
            #[cfg(feature = "render")]
            assert_eq!(
                imported.to_svg_text().unwrap(),
                original.to_svg_text().unwrap()
            );

            let raw_heap = LatexReader::parse(source, &config).unwrap().expr;
            let raw_compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            assert_eq!(
                MathmlWriter::write(&raw_heap, &config).unwrap(),
                MathmlWriter::write_compact(&raw_compact, &config).unwrap(),
                "raw heap/compact: {source}"
            );

            let mut normalized_heap = raw_heap.clone();
            normalize_expr(&mut normalized_heap);
            let mut normalized_compact = raw_compact.clone();
            normalize_compact_expr(&mut normalized_compact).unwrap();
            assert_eq!(
                MathmlWriter::write(&normalized_heap, &config).unwrap(),
                MathmlWriter::write_compact(&normalized_compact, &config).unwrap(),
                "normalized heap/compact: {source}"
            );
        }
    }

    #[test]
    fn text_without_whitespace_keeps_style() {
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Bold, "hello".into())]);
        let output = MathmlWriter::write(&expr, &Config::default()).unwrap();
        assert!(output.contains("<mtext mathvariant=\"bold\">hello</mtext>"));
        assert!(!output.contains("xml:space"));
    }

    #[test]
    fn middle_and_slash_delimiters_request_symmetric_stretching() {
        for (source, operator) in [
            (
                r"\left(\frac{1}{x}\middle|y\right)",
                r#"stretchy="true" symmetric="true">|</mo>"#,
            ),
            (
                r"\left\backslash\frac{1}{x}\right/",
                r#"fence="true" stretchy="true" symmetric="true">\</mo>"#,
            ),
        ] {
            let result = crate::from_latex(source).unwrap();
            let xml = result.to_mathml().unwrap();
            assert_eq!(
                MathmlWriter::write(&result.expr(), &Config::default()).unwrap(),
                xml
            );
            assert!(xml.contains(operator), "{source}: {xml}");
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn negative_mspace_has_a_signed_browser_advance_and_round_trips() {
        let formula = crate::from_latex(r"a\hskip-.25em b").unwrap();
        let xml = formula.to_mathml().unwrap();
        assert!(
            xml.contains(r#"<mspace width="0em" style="margin-inline-start:-0.25em"/>"#),
            "{xml}"
        );
        assert_eq!(
            xml,
            MathmlWriter::write(&formula.expr(), &Config::default()).unwrap()
        );
        let again = crate::from_mathml(&xml).unwrap();
        assert_eq!(again.to_mathml().unwrap(), xml);
        assert_eq!(
            crate::from_mathml(r#"<math><mspace width="-.25em"/></math>"#)
                .unwrap()
                .expr()
                .nodes,
            vec![MathNode::Space(Rational::new(-1, 4))]
        );
    }

    #[test]
    fn fraction() {
        let output = write(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<mfrac>"));
        assert!(output.contains("<mn>1</mn>"));
        assert!(output.contains("<mn>2</mn>"));
    }

    #[test]
    fn operator_accents_keep_true_accents_separate_from_limit_labels() {
        for (latex, accent) in [
            (r"\overset{¯}{\lim}", true),
            (r"\overset{¯}{\operatorname{rate}}", true),
            (r"\overset{¯}{\mathop{lim}}", true),
            (r"\overline{\lim}", true),
            (r"\overset{!}{\lim}", false),
            (r"\lim\limits^¯", false),
            (r"\overset{!}{=}", false),
        ] {
            let result = crate::from_latex(latex).unwrap();
            let heap = MathmlWriter::write(&result.expr(), &Config::default()).unwrap();
            let compact =
                MathmlWriter::write_compact(result.compact_expr(), &Config::default()).unwrap();
            assert_eq!(heap, compact, "heap/compact mismatch for {latex}");
            assert_eq!(heap.contains("accent=\"true\""), accent, "{latex}: {heap}");
        }

        let authored = r#"<math><mover accent="true"><mo>lim</mo><mo>¯</mo></mover></math>"#;
        let output =
            crate::convert(authored, crate::Format::MathML, crate::Format::MathML).unwrap();
        assert!(output.contains(r#"<mover accent="true"><mo movablelimits="false">lim</mo>"#));
        let movable = authored.replace("<mo>lim", r#"<mo movablelimits="true">lim"#);
        let output = crate::from_mathml(&movable).unwrap().to_mathml().unwrap();
        assert!(output.contains(r#"<mover accent="true"><mo movablelimits="true">lim</mo>"#));
        for source in [r"\lim\limits^¯", r"\mathop{x}\limits_¯"] {
            let xml = crate::from_latex(source).unwrap().to_mathml().unwrap();
            assert!(
                xml.contains(r#"accent="false""#) || xml.contains(r#"accentunder="false""#),
                "{xml}"
            );
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn text_sizing_is_separate_from_an_upright_font() {
        let xml = r#"<math><menclose notation="actuarial"><mi mathvariant="normal">x</mi></menclose></math>"#;
        let parsed = crate::from_mathml(xml).unwrap();
        let normalized = parsed.to_mathml().unwrap();
        assert!(
            normalized.contains(r#"mathvariant="normal""#),
            "{normalized}"
        );
        assert!(!normalized.contains(r#"scriptlevel="0""#), "{normalized}");
        for mode in [true, false] {
            for node in [
                MathNode::Actuarial {
                    content: Box::new(MathNode::Identifier("x".into())),
                    text_mode: mode,
                },
                MathNode::Reflect {
                    content: Box::new(MathNode::Identifier("x".into())),
                    text_mode: mode,
                },
            ] {
                let expr = MathExpr::block(vec![node]);
                let compact = CompactExpr::from_math_expr(&expr).unwrap();
                for output in [
                    MathmlWriter::write(&expr, &Config::default()).unwrap(),
                    MathmlWriter::write_compact(&compact, &Config::default()).unwrap(),
                ] {
                    assert_eq!(output.matches("<mi>x</mi>").count(), 1, "{output}");
                    assert_eq!(output.contains(r#"scriptlevel="0""#), mode);
                    let round = crate::from_mathml(&output).unwrap().to_mathml().unwrap();
                    assert_eq!(round, output);
                }
            }
        }
    }

    #[test]
    fn actuarial_text_mode_emits_standard_size_hints() {
        let text = write(MathNode::Actuarial {
            content: Box::new(MathNode::Identifier("x".into())),
            text_mode: true,
        });
        assert!(text.contains("notation=\"actuarial\""));
        assert!(text.contains("displaystyle=\"false\""));
        assert!(text.contains("scriptlevel=\"0\""));

        let inherited = write(MathNode::Actuarial {
            content: Box::new(MathNode::Identifier("x".into())),
            text_mode: false,
        });
        assert!(inherited.contains("notation=\"actuarial\""));
        assert!(!inherited.contains("scriptlevel=\"0\""));
    }

    #[test]
    fn native_actuarial_mathml_keeps_inherited_style() {
        let config = Config::default();
        let parsed = crate::reader::mathml::MathmlReader::parse(
            r#"<math><msub><mi>x</mi><menclose notation="actuarial"><mi>y</mi></menclose></msub></math>"#,
            &config,
        )
        .unwrap()
        .expr;
        let MathNode::Subscript { sub, .. } = &parsed.nodes[0] else {
            panic!("expected subscript: {:?}", parsed.nodes);
        };
        assert!(matches!(
            &**sub,
            MathNode::Actuarial {
                text_mode: false,
                ..
            }
        ));
    }

    #[test]
    fn latex_hbox_actuarial_style_hints_survive_mathml_round_trip() {
        let mathml =
            crate::convert(r"\angl{$x_i$}", crate::Format::LaTeX, crate::Format::MathML).unwrap();
        assert!(mathml.contains("displaystyle=\"false\""));
        assert!(mathml.contains("scriptlevel=\"0\""));
        let latex = crate::convert(&mathml, crate::Format::MathML, crate::Format::LaTeX).unwrap();
        assert!(latex.starts_with(r"\angl{"), "{latex:?}");
    }

    #[test]
    fn subscript() {
        let output = write(MathNode::Subscript {
            base: Box::new(MathNode::Identifier("x".into())),
            sub: Box::new(MathNode::Number("2".into())),
        });
        assert!(output.contains("<msub>"));
    }

    #[test]
    fn sqrt() {
        let output = write(MathNode::Sqrt(Box::new(MathNode::Number("2".into()))));
        assert!(output.contains("<msqrt>"));
    }

    #[test]
    fn display_block() {
        let expr = MathExpr::block(vec![MathNode::Number("1".into())]);
        let config = Config::default();
        let output = MathmlWriter::write(&expr, &config).unwrap();
        assert!(output.contains("display=\"block\""));
    }

    #[test]
    fn phantom() {
        let output = write(MathNode::Phantom(
            Box::new(MathNode::Identifier("x".into())),
            PhantomKind::Full,
        ));
        assert!(output.contains("<mphantom>"));
    }

    #[test]
    fn boxed() {
        let output = write(MathNode::Boxed {
            content: Box::new(MathNode::Number("1".into())),
            bg_color: None,
            border_color: None,
        });
        assert!(output.contains("<menclose"));
        assert!(output.contains("notation=\"box\""));
    }

    #[test]
    fn below_accent_uses_accentunder_attribute() {
        let node = MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '_',
            position: AccentPosition::Below,
            wide: false,
        };
        let output = write(node.clone());
        assert!(output.contains("<munder accentunder=\"true\">"));
        assert!(!output.contains("<munder accent=\"true\">"));

        let expr = MathExpr::inline(vec![node]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(output.contains("<munder accentunder=\"true\">"));
        assert!(!output.contains("<munder accent=\"true\">"));
    }

    #[test]
    #[cfg(feature = "render")]
    fn builtin_accent_identity_survives_mathml_roundtrip_on_both_paths() {
        use crate::reader::Reader;
        use crate::reader::latex::LatexReader;
        use crate::render::{RenderOptions, render_to_svg_compact};

        let config = Config::default();
        for source in [
            r"\underline{x}",
            r"\overline{lim}",
            r"\underbracket{end}",
            r"\overbracket{rate}",
            r"\overbrace{rate}",
            r"\underbrace{rate}",
            r"\underleftarrow{lim}",
            r"\overlinesegment{ab}",
            r"\underlinesegment{ab}",
            r"\widehat{AB}",
            r"\widetilde{AB}",
            r"\hat{x}",
            r"\bar{x}",
        ] {
            let heap = LatexReader::parse(source, &config).unwrap().expr;
            let compact = LatexReader::parse_compact(source, &config).unwrap().expr;
            let heap_xml = MathmlWriter::write(&heap, &config).unwrap();
            let compact_xml = MathmlWriter::write_compact(&compact, &config).unwrap();
            assert_eq!(
                heap_xml, compact_xml,
                "heap/compact MathML mismatch: {source}"
            );
            let imported = crate::from_mathml(&heap_xml).unwrap();
            assert_eq!(
                heap_xml,
                imported.to_mathml().unwrap(),
                "MathML instability: {source}"
            );
            assert_eq!(
                svg_dimensions(
                    &render_to_svg_compact(&compact, &RenderOptions::default()).unwrap()
                ),
                svg_dimensions(&imported.to_svg().unwrap()),
                "compact geometry changed on reimport: {source}"
            );
        }
    }

    #[cfg(feature = "render")]
    fn svg_dimensions(svg: &str) -> (&str, &str, &str) {
        let root = svg.split('>').next().expect("SVG root");
        let attr = |name: &str| {
            root.split_once(&format!(" {name}=\""))
                .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
                .unwrap_or("")
        };
        (attr("width"), attr("height"), attr("viewBox"))
    }

    fn assert_rule_outputs_match(
        width: Rational,
        height: Rational,
        lift: Rational,
        expected: &str,
    ) {
        let node = MathNode::Rule {
            width,
            height,
            lift,
        };
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        let output = MathmlWriter::write(&expr, &config).unwrap();
        assert!(output.contains(expected), "non-compact output: {output}");

        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(output.contains(expected), "compact output: {output}");
    }

    #[test]
    fn rule_positive_lift_writes_height_and_depth_on_both_paths() {
        assert_rule_outputs_match(
            Rational::new(2, 1),
            Rational::new(3, 1),
            Rational::new(1, 1),
            "<mspace width=\"2em\" height=\"4em\" depth=\"0em\"/>",
        );
    }

    #[test]
    fn rule_negative_lift_writes_height_and_depth_on_both_paths() {
        assert_rule_outputs_match(
            Rational::new(2, 1),
            Rational::new(3, 1),
            Rational::new(-1, 1),
            "<mspace width=\"2em\" height=\"2em\" depth=\"1em\"/>",
        );
    }

    #[test]
    fn compact_accent_escapes_xml_special_operator_chars() {
        let expr = MathExpr::inline(vec![MathNode::Accent {
            base: Box::new(MathNode::Identifier("x".into())),
            accent: '<',
            position: AccentPosition::Below,
            wide: false,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains(r#"<mo stretchy="false">&lt;</mo>"#),
            "compact output: {output}"
        );
        assert!(!output.contains("<mo><</mo>"), "compact output: {output}");
    }

    #[test]
    fn compact_nary_escapes_xml_special_operator_chars() {
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '<',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: None,
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("<mo movablelimits=\"false\">&lt;</mo>"),
            "compact output: {output}"
        );
        assert!(!output.contains("><<"), "compact output: {output}");
    }

    #[test]
    fn bounded_nary_replacement_is_counted_and_reported() {
        let expr = MathExpr::inline(vec![MathNode::Nary {
            op: '\u{13}',
            sub: None,
            sup: None,
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let (unlimited, diagnostics) =
            MathmlWriter::write_compact_with_diagnostics(&compact, &config).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message == xml_chars::REPLACED })
        );
        let exact = MathmlWriter::write_compact_with_diagnostics_and_limits(
            &compact,
            &config,
            &OutputLimits::with_max_bytes(unlimited.len()),
        )
        .unwrap();
        assert_eq!(exact.0, unlimited);
        assert!(matches!(
            MathmlWriter::write_compact_with_diagnostics_and_limits(
                &compact,
                &config,
                &OutputLimits::with_max_bytes(unlimited.len() - 1),
            ),
            Err(crate::Error::OutputLimit { .. })
        ));
    }

    #[test]
    fn bounded_text_replacement_is_counted_and_reported() {
        let expr = MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, "a\u{13}b".into())]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let (unlimited, diagnostics) =
            MathmlWriter::write_compact_with_diagnostics(&compact, &config).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message == xml_chars::REPLACED })
        );
        let exact = MathmlWriter::write_compact_with_diagnostics_and_limits(
            &compact,
            &config,
            &OutputLimits::with_max_bytes(unlimited.len()),
        )
        .unwrap();
        assert_eq!(exact.0, unlimited);
        assert!(matches!(
            MathmlWriter::write_compact_with_diagnostics_and_limits(
                &compact,
                &config,
                &OutputLimits::with_max_bytes(unlimited.len() - 1),
            ),
            Err(crate::Error::OutputLimit { .. })
        ));
    }

    #[test]
    fn mathchoice_uses_its_local_style_in_both_writers() {
        fn xml(latex: &str, display: DisplayStyle) -> String {
            let config = Config::default().with_display(display);
            let parsed = crate::from_latex_with(latex, &config).unwrap();
            let xml = parsed.to_mathml().unwrap();
            assert_eq!(
                xml,
                MathmlWriter::write(&parsed.expr(), &config).unwrap(),
                "{latex}"
            );
            xml
        }
        let choice = r"\mathchoice{D}{T}{S}{Q}";
        for (source, mode, expected) in [
            (choice, DisplayStyle::Block, "<mi>D</mi>"),
            (choice, DisplayStyle::Inline, "<mi>T</mi>"),
            (
                r"x^{\mathchoice{D}{T}{S}{Q}}",
                DisplayStyle::Block,
                "<msup><mi>x</mi><mi>S</mi></msup>",
            ),
            (
                r"x_{y^{\mathchoice{D}{T}{S}{Q}}}",
                DisplayStyle::Inline,
                "<msup><mi>y</mi><mi>Q</mi></msup>",
            ),
            (
                r"\frac{\mathchoice{D}{T}{S}{Q}}{y}",
                DisplayStyle::Block,
                "<mfrac><mi>T</mi><mi>y</mi>",
            ),
            (
                r"\sqrt[\mathchoice{D}{T}{S}{Q}]{z}",
                DisplayStyle::Block,
                "<mroot><mi>z</mi><mi>Q</mi></mroot>",
            ),
            (
                r"{\scriptstyle\mathchoice{D}{T}{S}{Q}}\mathchoice{D}{T}{S}{Q}",
                DisplayStyle::Block,
                r#"<mstyle displaystyle="false" scriptlevel="1"><mi>S</mi></mstyle><mi>D</mi>"#,
            ),
        ] {
            let written = xml(source, mode);
            assert!(written.contains(expected), "{source}: {written}");
        }
        let inline = xml(
            r"\mathchoice{D}{T}{S}{Q}\displaystyle\mathchoice{D}{T}{S}{Q}",
            DisplayStyle::Inline,
        );
        assert!(inline.contains(r#"display="inline"><mi>T</mi><mstyle displaystyle="true" scriptlevel="0"><mi>D</mi></mstyle>"#), "{inline}");
    }

    #[test]
    fn a_single_styled_cell_does_not_override_the_rest_of_a_table() {
        let parsed =
            crate::from_latex(r"x^{\begin{matrix}\scriptscriptstyle a & b\end{matrix}}").unwrap();
        let xml = parsed.to_mathml().unwrap();
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="0"><mtable data-mathema-array-environment="matrix">"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="2"><mi>a</mi></mstyle>"#),
            "{xml}"
        );
        assert_eq!(
            xml,
            MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap()
        );

        let xml = r#"<math><mtable scriptlevel="2"><mtr scriptlevel="-1"><mtd scriptlevel="+1"><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr></mtable><mi>z</mi></math>"#;
        let canonical = crate::from_mathml(xml).unwrap().to_mathml().unwrap();
        for (level, text) in [(2, 'a'), (1, 'b')] {
            assert!(
                canonical.contains(&format!(
                    r#"<mstyle displaystyle="false" scriptlevel="{level}"><mi>{text}</mi></mstyle>"#
                )),
                "{canonical}"
            );
        }
        assert!(
            canonical.ends_with("</mtable><mi>z</mi></math>"),
            "{canonical}"
        );
    }

    #[test]
    fn tables_reset_matrix_cells_to_text_size_inside_scripts() {
        let config = Config::default();
        for source in [
            r"x^{\begin{matrix}a&b\\c&d\end{matrix}}",
            r"x^{\begin{array}{c}a\end{array}}",
        ] {
            let result = crate::from_latex(source).unwrap();
            let heap = MathmlWriter::write(&result.expr(), &config).unwrap();
            let compact = MathmlWriter::write_compact(result.compact_expr(), &config).unwrap();
            assert_eq!(heap, compact, "heap/compact mismatch for {source}");
            assert!(
                heap.contains("<mstyle displaystyle=\"false\" scriptlevel=\"0\"><mtable"),
                "nested table did not reset to text size for {source}: {heap}"
            );
        }

        let aligned = crate::from_latex(
            r"x^{\begin{aligned}\mathchoice{D}{T}{S}{Q}&=\mathchoice{D}{T}{S}{Q}\end{aligned}}",
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            aligned.contains("<mtable displaystyle=\"true\">")
                && aligned.contains("<mi>D</mi>")
                && !aligned.contains("scriptlevel=\"0\""),
            "aligned table did not retain inherited script size: {aligned}"
        );
        let aligned_fraction =
            crate::from_latex(r"x^{\begin{aligned}\frac{a}{b}\end{aligned}}").unwrap();
        let aligned_fraction_xml = aligned_fraction.to_mathml().unwrap();
        let aligned_fraction_again = crate::from_mathml(&aligned_fraction_xml)
            .unwrap()
            .to_mathml()
            .unwrap();
        assert_eq!(aligned_fraction_xml, aligned_fraction_again);

        let top = crate::from_latex(r"\begin{matrix}a&b\end{matrix}")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(
            !top.contains("scriptlevel=\"0\""),
            "top-level matrix gained an unnecessary style wrapper: {top}"
        );

        let authored = crate::from_mathml(
            r#"<math><msup><mi>x</mi><mtable><mtr><mtd><mi>a</mi></mtd></mtr></mtable></msup></math>"#,
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            authored.contains("<msup><mi>x</mi><mtable>")
                && !authored.contains("scriptlevel=\"0\""),
            "an unstyled MathML table inherits its surrounding script level: {authored}"
        );

        let explicit = crate::from_mathml(
            r#"<math><msup><mi>x</mi><mstyle scriptlevel="0"><mtable><mtr><mtd><mi>a</mi></mtd></mtr></mtable></mstyle></msup></math>"#,
        )
        .unwrap()
        .to_mathml()
        .unwrap();
        assert!(
            explicit.contains("<mstyle displaystyle=\"false\" scriptlevel=\"0\"><mtable>"),
            "explicit table scriptlevel was not preserved: {explicit}"
        );
    }

    #[test]
    fn forced_fraction_styles_match_explicit_switches_in_scripts() {
        for (command, declaration, child) in
            [("dfrac", "displaystyle", "T"), ("tfrac", "textstyle", "S")]
        {
            let choice = r"\mathchoice{D}{T}{S}{Q}";
            let fraction = format!(r"x_{{\{command}{{{choice}}}{{{choice}}}}}");
            let declared = format!(r"x_{{\{declaration}\frac{{{choice}}}{{{choice}}}}}");
            let expected = crate::from_latex(&fraction).unwrap().to_mathml().unwrap();
            let actual = crate::from_latex(&declared).unwrap().to_mathml().unwrap();
            assert_eq!(expected, actual);
            assert!(actual.contains(&format!("<mfrac><mi>{child}</mi><mi>{child}</mi></mfrac>")));
            assert_eq!(
                crate::from_mathml(&actual).unwrap().to_mathml().unwrap(),
                actual
            );
        }
    }

    #[test]
    fn named_operators_employ_the_requested_mathml_limit_style() {
        for (style, start) in [
            (
                LimitStyle::Default,
                r#"<munder><mo movablelimits="true">lim</mo>"#,
            ),
            (
                LimitStyle::AboveBelow,
                r#"<munder><mo movablelimits="false">lim</mo>"#,
            ),
            (LimitStyle::SubSup, "<msub><mo>lim</mo>"),
        ] {
            let expr = MathExpr::block(vec![MathNode::Subscript {
                base: Box::new(MathNode::Operator(OperatorAttr {
                    name: "lim".into(),
                    limit_style: style,
                })),
                sub: Box::new(MathNode::Identifier("i".into())),
            }]);
            let compact = CompactExpr::from_math_expr(&expr).unwrap();
            let xml = MathmlWriter::write(&expr, &Config::default()).unwrap();
            assert!(xml.contains(start), "{xml}");
            assert_eq!(
                MathmlWriter::write_compact(&compact, &Config::default()).unwrap(),
                xml
            );
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn compound_operator_limits_follow_their_local_math_style() {
        let operator = || MathNode::Underscript {
            base: Box::new(MathNode::MathAtom {
                atom_type: MathAtomType::Op,
                content: Box::new(MathNode::Row(vec![
                    MathNode::Identifier("a".into()),
                    MathNode::Symbol(SymbolKind::BinaryOp, "+".into()),
                    MathNode::Identifier("b".into()),
                ])),
            }),
            under: Box::new(MathNode::Identifier("i".into())),
            movable_limits: true,
        };
        let nested = MathNode::Superscript {
            base: Box::new(MathNode::Identifier("z".into())),
            sup: Box::new(operator()),
        };
        for (expr, tag) in [
            (MathExpr::block(vec![operator()]), "munder"),
            (MathExpr::inline(vec![operator()]), "msub"),
            (MathExpr::block(vec![nested]), "msub"),
        ] {
            let config = Config::default();
            let xml = MathmlWriter::write(&expr, &config).unwrap();
            let fragment = format!(
                r#"<{tag}><mrow data-mjx-texclass="OP" data-mathema-limits="movable"><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mi>i</mi></{tag}>"#
            );
            assert!(xml.contains(&fragment), "{xml}");
            let packed = CompactExpr::from_math_expr(&expr).unwrap();
            assert_eq!(MathmlWriter::write_compact(&packed, &config).unwrap(), xml);
            assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
        }
    }

    #[test]
    fn mathml_retains_open_and_close_spacing_classes_around_a_fence() {
        use crate::reader::{Reader, latex::LatexReader};

        let source = r"x+\mathopen{}\mathclose{\left(a+b\right)}_i y";
        let cfg = Config::default();
        let heap = LatexReader::parse(source, &cfg).unwrap().expr;
        let compact = LatexReader::parse_compact(source, &cfg).unwrap().expr;
        let xml = MathmlWriter::write_compact(&compact, &cfg).unwrap();
        assert_eq!(MathmlWriter::write(&heap, &cfg).unwrap(), xml);
        assert!(xml.contains(r#"<mrow data-mjx-texclass="OPEN"></mrow>"#));
        assert!(xml.contains(r#"<mrow data-mjx-texclass="CLOSE">"#));
        let short = xml.replace(
            r#"<mrow data-mjx-texclass="OPEN"></mrow>"#,
            r#"<mrow data-mjx-texclass="OPEN"/>"#,
        );
        for input in [&xml, &short] {
            let again = crate::from_mathml(input).unwrap();
            assert_eq!(again.to_mathml().unwrap(), xml);
            assert_eq!(
                again.to_latex().unwrap(),
                crate::from_latex(source).unwrap().to_latex().unwrap()
            );
            #[cfg(feature = "render")]
            assert_eq!(
                again.to_svg().unwrap(),
                crate::from_latex(source).unwrap().to_svg().unwrap()
            );
        }
    }

    #[test]
    fn constructed_smallmatrix_selects_scriptstyle_without_an_explicit_switch() {
        let leaf = |name: &str| Box::new(MathNode::Identifier(name.into()));
        let cell = MathNode::MathChoice {
            display: leaf("D"),
            text: leaf("T"),
            script: leaf("S"),
            script_script: leaf("Q"),
        };
        let expr = MathExpr::inline(vec![MathNode::Matrix {
            kind: MatrixKind::SmallMatrix,
            alignment: ColumnAlign::Right,
            rows: vec![vec![vec![cell]]],
        }]);
        let cfg = Config::default();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let xml = MathmlWriter::write(&expr, &cfg).unwrap();
        assert!(
            xml.contains(r#"<mstyle displaystyle="false" scriptlevel="1"><mi>S</mi></mstyle>"#),
            "{xml}"
        );
        assert_eq!(MathmlWriter::write_compact(&compact, &cfg).unwrap(), xml);
        assert_eq!(crate::from_mathml(&xml).unwrap().to_mathml().unwrap(), xml);
    }

    #[test]
    fn nary_operands_stay_inside_fixed_arity_mathml_arguments() {
        for (source, before, after) in [
            (
                r"\frac{\sum_i^n x}{2}",
                "<mfrac><mrow><munderover>",
                "</munderover><mi>x</mi></mrow><mn>2</mn></mfrac>",
            ),
            (
                r"y^{\prod_k^n k}",
                "<msup><mi>y</mi><mrow><munderover>",
                "</munderover><mi>k</mi></mrow></msup>",
            ),
            (
                r"\sqrt[\sum_i i]{z}",
                "<mroot><mi>z</mi><mrow><munder>",
                "</munder><mi>i</mi></mrow></mroot>",
            ),
            (
                r"\overset{2}{\sum_i x}",
                "<mrow><munder>",
                "</munder><mi>x</mi></mrow><mn>2</mn></mover>",
            ),
            (
                r"\frac{a}{\int_0^1 x}",
                "<mfrac><mi>a</mi><mrow><msubsup>",
                "</msubsup><mi>x</mi></mrow></mfrac>",
            ),
        ] {
            let parsed = crate::from_latex(source).unwrap();
            for xml in [
                parsed.to_mathml().unwrap(),
                MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap(),
            ] {
                assert!(
                    xml.contains(before) && xml.contains(after),
                    "{source}: {xml}"
                );
                assert_eq!(
                    crate::from_mathml(&xml).unwrap().to_mathml().unwrap(),
                    xml,
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn default_nary_limits_use_movable_under_over_mathml() {
        let config = Config::default();
        let default_sum = MathExpr::block(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let compact = CompactExpr::from_math_expr(&default_sum).unwrap();
        for output in [
            MathmlWriter::write(&default_sum, &config).unwrap(),
            MathmlWriter::write_compact(&compact, &config).unwrap(),
        ] {
            assert!(
                output.contains("<munderover><mo movablelimits=\"true\">∑</mo>"),
                "default sum limits were not encoded as movable under/over: {output}"
            );
            assert!(
                !output.contains("<msubsup>"),
                "default sum used fixed scripts: {output}"
            );
            assert_eq!(
                crate::convert(&output, crate::Format::MathML, crate::Format::MathML).unwrap(),
                output,
                "movable default limits lost on MathML import"
            );
        }

        let default_integral = MathExpr::block(vec![MathNode::Nary {
            op: '∫',
            sub: Some(Box::new(MathNode::Number("0".into()))),
            sup: Some(Box::new(MathNode::Number("1".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::Default,
        }]);
        let output = MathmlWriter::write(&default_integral, &config).unwrap();
        assert!(output.contains("<msubsup><mo movablelimits=\"false\">∫</mo>"));

        let forced = MathExpr::inline(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::SubSup,
        }]);
        let forced_output = MathmlWriter::write(&forced, &config).unwrap();
        assert!(forced_output.contains("<msubsup><mo movablelimits=\"false\">∑</mo>"));

        let above_below = MathExpr::block(vec![MathNode::Nary {
            op: '∑',
            sub: Some(Box::new(MathNode::Identifier("i".into()))),
            sup: Some(Box::new(MathNode::Identifier("n".into()))),
            body: Box::new(MathNode::Identifier("x".into())),
            limits: LimitStyle::AboveBelow,
        }]);
        let above_below_output = MathmlWriter::write(&above_below, &config).unwrap();
        assert!(above_below_output.contains("<munderover><mo movablelimits=\"false\">∑</mo>"));
        assert_eq!(
            crate::convert(
                &above_below_output,
                crate::Format::MathML,
                crate::Format::MathML
            )
            .unwrap(),
            above_below_output,
            "forced limits lost on MathML import"
        );
    }

    #[test]
    fn compact_boxed_style_escapes_attribute_values() {
        let expr = MathExpr::inline(vec![MathNode::Boxed {
            content: Box::new(MathNode::Identifier("x".into())),
            bg_color: Some("red&blue\"".into()),
            border_color: Some("black&white\"".into()),
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("black&amp;white&quot;"),
            "compact output: {output}"
        );
        assert!(
            output.contains("red&amp;blue&quot;"),
            "compact output: {output}"
        );
        assert!(
            !output.contains("black&white\""),
            "compact output: {output}"
        );
    }

    #[test]
    fn compact_mathcolor_escapes_attribute_values() {
        let mut arena = AstArena::new();
        let color = arena.push_str("red&blue\"");
        let x = arena.push_str("x");
        let content = arena.push_node(CompactNode::Identifier(x));
        let root = arena.push_node(CompactNode::Color { color, content });
        let roots = arena.push_children(&[root]).unwrap();
        let compact = CompactExpr::new(arena, roots, DisplayStyle::Inline);
        let config = Config::default();
        let output = MathmlWriter::write_compact(&compact, &config).unwrap();
        assert!(
            output.contains("mathcolor=\"red&amp;blue&quot;\""),
            "compact output: {output}"
        );
        assert!(
            !output.contains("mathcolor=\"red&blue\"\""),
            "compact output: {output}"
        );
    }

    #[test]
    fn big_delimiters_are_written_as_mathjax_sized_operators() {
        let mathml = crate::convert(
            r"a\bigl( b\bigr) \Bigm| \bigg\| \Biggl.",
            crate::Format::LaTeX,
            crate::Format::MathML,
        )
        .unwrap();
        for expected in [
            r#"<mrow data-mjx-texclass="OPEN"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">(</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="CLOSE"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">)</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="REL"><mo minsize="1.623em" maxsize="1.623em" fence="true" stretchy="true" symmetric="true">|</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="ORD"><mo minsize="2.047em" maxsize="2.047em" fence="true" stretchy="true" symmetric="true">‖</mo></mrow>"#,
            r#"<mrow data-mjx-texclass="OPEN"><mo minsize="2.470em" maxsize="2.470em" fence="true" stretchy="true" symmetric="true"></mo></mrow>"#,
        ] {
            assert!(mathml.contains(expected), "missing {expected} in {mathml}");
        }
        // The heap writer agrees with the compact writer.
        let parsed = crate::from_latex(r"a\bigl( b\bigr) \Bigm| \bigg\| \Biggl.").unwrap();
        let heap = MathmlWriter::write(&parsed.expr(), &Config::default()).unwrap();
        assert_eq!(heap, mathml);
        // Sized delimiters and their classes round-trip without a loss diagnostic.
        let (_, diagnostics) = parsed.to_mathml_with_diagnostics().unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
