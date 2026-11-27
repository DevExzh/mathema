//! MathemaConverter builder class for WASM.
//!
//! Usage from JavaScript:
//! ```js
//! import { init, MathemaConverter } from './mathema_bg.wasm';
//! init();
//! const conv = new MathemaConverter('latex');
//! const result = conv.parse('x^2');
//! console.log(result.toMathml());
//! console.log(result.toOmml());
//! result.free(); // release WASM memory
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
unsafe extern "C" {
    /// Native JavaScript representation of the serialized heap expression.
    #[wasm_bindgen(typescript_type = "MathExpr")]
    pub type MathExprJs;
    /// Native JavaScript representation of parsed diagnostics.
    #[wasm_bindgen(typescript_type = "Diagnostic[]")]
    pub type DiagnosticArray;
}

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_TYPES: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/wasm/mathema.d.ts"));

use crate::ast::compact::CompactExpr;
use crate::ast::normalize::normalize_compact_expr_trusted;
use crate::error::{CompactParseResult, Config, DiagnosticKind, Format, Strictness};
use crate::reader::Reader;
use crate::reader::latex::LatexReader;
use crate::reader::mathml::MathmlReader;
use crate::reader::mtef::MtefReader;
use crate::reader::omml::OmmlReader;
use crate::reader::unicode_math::UnicodeMathReader;
use crate::writer::Writer;
use crate::writer::latex::LatexWriter;
use crate::writer::mathml::MathmlWriter;
use crate::writer::mtef::MtefWriter;
use crate::writer::omml::OmmlWriter;
use crate::writer::unicode_math::UnicodeMathWriter;

use super::to_js_err;

/// A configured converter that parses input and produces multi-format output.
#[wasm_bindgen]
pub struct MathemaConverter {
    format: Format,
    config: Config,
}

#[wasm_bindgen]
impl MathemaConverter {
    /// Create a new converter for the given input format.
    ///
    /// Formats: "latex", "mathml", "omml", "mtef", "unicodemath"
    #[wasm_bindgen(constructor)]
    pub fn new(format: &str) -> Result<MathemaConverter, JsValue> {
        let format = super::parse_input_format(format)?;
        Ok(MathemaConverter {
            format,
            config: Config::default(),
        })
    }

    /// Create a converter with explicit options.
    ///
    /// - `format`: input format string
    /// - `strict`: if true, reject malformed input; if false, recover where possible
    /// - `display`: if true, use block/display style; if false, use inline style
    #[wasm_bindgen(js_name = "withOptions")]
    pub fn with_options(
        format: &str,
        strict: bool,
        display: bool,
    ) -> Result<MathemaConverter, JsValue> {
        let format = super::parse_input_format(format)?;
        let strictness = if strict {
            Strictness::Strict
        } else {
            Strictness::Lenient
        };
        let display_style = if display {
            crate::ast::DisplayStyle::Block
        } else {
            crate::ast::DisplayStyle::Inline
        };
        Ok(MathemaConverter {
            format,
            config: Config {
                strictness,
                display: display_style,
                ..Config::default()
            },
        })
    }

    /// Parse input and return a reusable result for multi-format output.
    pub fn parse(&self, input: &str) -> Result<ParsedMath, JsValue> {
        let mut compact = self.parse_compact(input)?;
        crate::ast::validate_compact_depth(&compact.expr.arena, compact.expr.roots)
            .map_err(to_js_err)?;
        normalize_compact_expr_trusted(&mut compact.expr).map_err(to_js_err)?;
        Ok(ParsedMath {
            compact,
            config: self.config,
        })
    }

    /// Parse input asynchronously, returning a `Promise<ParsedMath>`.
    ///
    /// Parsing still runs on the WebAssembly thread. The asynchronous wrapper
    /// lets a caller compose this operation with other Promise-based work, but
    /// it does not move parsing to a worker. Applications that must keep the
    /// main thread responsive should call this method from a dedicated Worker.
    #[wasm_bindgen(js_name = "parseAsync")]
    pub async fn parse_async(&self, input: String) -> Result<ParsedMath, JsValue> {
        self.parse(&input)
    }
}

impl MathemaConverter {
    fn parse_compact(&self, input: &str) -> Result<CompactParseResult, JsValue> {
        match self.format {
            Format::LaTeX => LatexReader::parse_compact(input, &self.config).map_err(to_js_err),
            Format::MathML => MathmlReader::parse_compact(input, &self.config).map_err(to_js_err),
            Format::Omml => OmmlReader::parse_compact(input, &self.config).map_err(to_js_err),
            Format::Mtef => MtefReader::parse_compact(input, &self.config).map_err(to_js_err),
            Format::UnicodeMath => {
                UnicodeMathReader::parse_compact(input, &self.config).map_err(to_js_err)
            },
            #[cfg(feature = "render")]
            Format::Svg => Err(JsValue::from_str("SVG is output-only")),
            #[cfg(feature = "raster")]
            Format::Png | Format::Jpeg => Err(JsValue::from_str("raster formats are output-only")),
        }
    }
}

/// The result of parsing a math expression. Holds the parsed AST and can
/// produce output in any supported format without re-parsing.
#[wasm_bindgen]
pub struct ParsedMath {
    compact: CompactParseResult,
    config: Config,
}

impl ParsedMath {
    /// The expression without the font sizes and colors that `format` does not write, as
    /// it would have been normalized without them.
    fn written_in(&self, format: Format) -> Result<Cow<'_, CompactExpr>, JsValue> {
        let arena = &self.compact.expr.arena;
        if !arena.has_attrs() && !crate::writer::omml::has_colors(&self.compact.expr) {
            return Ok(Cow::Borrowed(&self.compact.expr));
        }
        let mut expr = self.compact.expr.clone();
        crate::convert::drop_unwritten(&mut expr, format).map_err(to_js_err)?;
        normalize_compact_expr_trusted(&mut expr).map_err(to_js_err)?;
        Ok(Cow::Owned(expr))
    }
}

#[wasm_bindgen]
impl ParsedMath {
    /// Write as LaTeX.
    #[wasm_bindgen(js_name = "toLatex")]
    pub fn to_latex(&self) -> Result<String, JsValue> {
        let expr = self.written_in(Format::LaTeX)?;
        LatexWriter::write_compact(&expr, &self.config).map_err(to_js_err)
    }

    /// Write as MathML.
    #[wasm_bindgen(js_name = "toMathml")]
    pub fn to_mathml(&self) -> Result<String, JsValue> {
        MathmlWriter::write_compact(&self.compact.expr, &self.config).map_err(to_js_err)
    }

    /// Write as Office Math Markup Language (OMML).
    #[wasm_bindgen(js_name = "toOmml")]
    pub fn to_omml(&self) -> Result<String, JsValue> {
        let expr = self.written_in(Format::Omml)?;
        OmmlWriter::write_compact(&expr, &self.config).map_err(to_js_err)
    }

    /// Write as MTEF (hex-encoded binary).
    #[wasm_bindgen(js_name = "toMtef")]
    pub fn to_mtef(&self) -> Result<String, JsValue> {
        MtefWriter::write_compact(&self.compact.expr, &self.config).map_err(to_js_err)
    }

    /// Write as UnicodeMath.
    #[wasm_bindgen(js_name = "toUnicodeMath")]
    pub fn to_unicode_math(&self) -> Result<String, JsValue> {
        let expr = self.written_in(Format::UnicodeMath)?;
        UnicodeMathWriter::write_compact(&expr, &self.config).map_err(to_js_err)
    }

    /// Render as a standalone SVG with embedded Latin Modern Math glyph
    /// outlines (no client-side fonts required — renders identically
    /// everywhere). `font_size_px` defaults to 16 when `<= 0`; `display`
    /// overrides display/inline style (pass `None`/undefined to use the
    /// parsed expression's own style).
    #[cfg(feature = "render")]
    #[wasm_bindgen(js_name = "toSvg")]
    pub fn to_svg(
        &self,
        font_size_px: Option<f64>,
        display: Option<bool>,
    ) -> Result<String, JsValue> {
        let mut opts = crate::render::RenderOptions::default();
        if let Some(px) = font_size_px
            && px > 0.0
        {
            opts.font_size_px = px;
        }
        opts.display_mode = display;
        crate::render::render_to_svg_standalone_compact(&self.compact.expr, &opts)
            .map_err(to_js_err)
    }

    /// Render a standalone SVG with optional accessibility metadata and an ID
    /// prefix for composing several inline SVG documents.
    #[cfg(feature = "render")]
    #[wasm_bindgen(js_name = "toSvgWithOptions")]
    pub fn to_svg_with_options(
        &self,
        font_size_px: Option<f64>,
        display: Option<bool>,
        title: Option<String>,
        aria_label: Option<String>,
        id_prefix: Option<String>,
    ) -> Result<String, JsValue> {
        let mut render = crate::render::RenderOptions::default();
        if let Some(px) = font_size_px
            && px > 0.0
        {
            render.font_size_px = px;
        }
        render.display_mode = display;
        let mut options = crate::render::SvgOptions::new(render);
        if let Some(title) = title {
            options = options.with_title(title);
        }
        if let Some(label) = aria_label {
            options = options.with_aria_label(label);
        }
        if let Some(prefix) = id_prefix {
            options = options.with_id_prefix(prefix).map_err(to_js_err)?;
        }
        crate::render::render_to_svg_compact_with_options(&self.compact.expr, &options)
            .map_err(to_js_err)
    }

    /// Render as a `<text>`-based SVG that relies on client-side math fonts.
    /// Smaller output, but appearance depends on installed fonts.
    #[cfg(feature = "render")]
    #[wasm_bindgen(js_name = "toSvgText")]
    pub fn to_svg_text(
        &self,
        font_size_px: Option<f64>,
        display: Option<bool>,
    ) -> Result<String, JsValue> {
        let mut opts = crate::render::RenderOptions::default();
        if let Some(px) = font_size_px
            && px > 0.0
        {
            opts.font_size_px = px;
        }
        opts.display_mode = display;
        crate::render::render_to_svg_compact(&self.compact.expr, &opts).map_err(to_js_err)
    }

    /// Get the AST as a JavaScript object (via serde-wasm-bindgen).
    #[wasm_bindgen(js_name = "toAst")]
    pub fn to_ast(&self) -> Result<MathExprJs, JsValue> {
        let expr = self.compact.expr.to_math_expr();
        crate::wasm::serde::to_js_value(&expr).map(JsCast::unchecked_into)
    }

    /// Get the number of AST nodes (compact arena size).
    #[wasm_bindgen(js_name = "nodeCount")]
    pub fn node_count(&self) -> usize {
        self.compact.expr.arena.node_count()
    }

    /// Get any parse diagnostics as a JSON string.
    /// Returns "[]" if no diagnostics. Each entry includes `kind`, `message`,
    /// and, when known, a byte-offset `span` object with `start` and `end`.
    /// Use [`diagnosticsStructured`](Self::diagnostics_structured) when a
    /// native JavaScript object is preferable to parsing JSON.
    #[wasm_bindgen(js_name = "diagnostics")]
    pub fn diagnostics(&self) -> String {
        if self.compact.diagnostics.is_empty() {
            return "[]".to_string();
        }
        let diags: Vec<String> = self
            .compact
            .diagnostics
            .iter()
            .map(|d| {
                let kind = match d.kind {
                    DiagnosticKind::Warning => "Warning",
                    DiagnosticKind::Info => "Info",
                };
                let span = d.span.map_or_else(
                    || "null".to_string(),
                    |span| format!("{{\"start\":{},\"end\":{}}}", span.start, span.end),
                );
                format!(
                    "{{\"kind\":{},\"message\":{},\"span\":{span}}}",
                    json_quote(kind),
                    json_quote(&d.message),
                )
            })
            .collect();
        format!("[{}]", diags.join(","))
    }

    /// Get parse diagnostics as native JavaScript objects.
    ///
    /// The returned array preserves source byte spans when the reader knows
    /// them. Its shape is the generated `Diagnostic[]` declaration.
    #[wasm_bindgen(js_name = "diagnosticsStructured")]
    pub fn diagnostics_structured(&self) -> Result<DiagnosticArray, JsValue> {
        let diagnostics = self
            .compact
            .diagnostics
            .iter()
            .map(WasmDiagnostic::from)
            .collect::<Vec<_>>();
        crate::wasm::serde::to_js_value(&diagnostics).map(JsCast::unchecked_into)
    }
}

/// Quote one Rust string using JSON's complete string-escape grammar. The
/// legacy `diagnostics()` API is kept as a string for compatibility, so it
/// must remain valid JSON even when a reader includes a control character in a
/// message.
fn json_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{08}' => quoted.push_str("\\b"),
            '\u{0C}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character <= '\u{1F}' => {
                write!(quoted, "\\u{:04x}", character as u32).expect("String write cannot fail");
            },
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[derive(::serde::Serialize)]
struct WasmDiagnostic {
    kind: &'static str,
    message: String,
    span: Option<WasmSpan>,
}

#[derive(::serde::Serialize)]
struct WasmSpan {
    start: usize,
    end: usize,
}

impl From<&crate::error::Diagnostic> for WasmDiagnostic {
    fn from(diagnostic: &crate::error::Diagnostic) -> Self {
        Self {
            kind: match diagnostic.kind {
                DiagnosticKind::Warning => "Warning",
                DiagnosticKind::Info => "Info",
            },
            message: diagnostic.message.clone(),
            span: diagnostic.span.map(|span| WasmSpan {
                start: span.start,
                end: span.end,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_parse_latex_to_mathml() {
        let converter = MathemaConverter::new("latex").expect("should create converter");
        let result = converter.parse("x^2").expect("should parse");
        let mathml = result.to_mathml().expect("should write mathml");
        assert!(mathml.contains("<math"));
    }

    #[test]
    fn converter_parse_with_strict() {
        let converter =
            MathemaConverter::with_options("latex", true, false).expect("should create converter");
        let result = converter.parse("x^2").expect("should parse");
        let latex = result.to_latex().expect("should write latex");
        assert!(latex.contains("x"));
    }

    #[test]
    fn converter_multi_output() {
        let converter = MathemaConverter::new("latex").expect("should create converter");
        let result = converter.parse("\\frac{1}{2}").expect("should parse");
        let mathml = result.to_mathml().expect("mathml");
        let omml = result.to_omml().expect("omml");
        let um = result.to_unicode_math().expect("unicode_math");
        assert!(mathml.contains("<mfrac"));
        assert!(omml.contains("<m:f>"));
        assert!(!um.is_empty());
    }

    #[test]
    fn converter_unknown_format_errors() {
        let result = MathemaConverter::new("pdf");
        assert!(result.is_err());
    }

    #[test]
    fn converter_all_input_formats() {
        for fmt in &["latex", "mathml", "omml", "mtef", "unicodemath"] {
            let converter = MathemaConverter::new(fmt);
            assert!(converter.is_ok(), "failed to create converter for {fmt}");
        }
    }

    #[test]
    fn converter_node_count() {
        let converter = MathemaConverter::new("latex").expect("create");
        let result = converter.parse("x^2").expect("parse");
        assert!(result.node_count() > 0);
    }

    #[test]
    fn converter_diagnostics_empty() {
        let converter = MathemaConverter::new("latex").expect("create");
        let result = converter.parse("x").expect("parse");
        assert_eq!(result.diagnostics(), "[]");
    }

    #[test]
    fn converter_diagnostics_json_preserves_source_span() {
        let converter =
            MathemaConverter::with_options("latex", false, false).expect("create converter");
        let result = converter.parse(r"\foo{x}").expect("lenient parse");
        let diagnostics = result.diagnostics();
        assert!(diagnostics.contains(r#""kind":"Warning""#));
        assert!(diagnostics.contains(r#""span":{"start":0,"end":4}"#));
    }

    #[test]
    fn diagnostics_json_escapes_every_control_character() {
        let value: String = (0..=0x1f).filter_map(char::from_u32).collect();
        let encoded = json_quote(&value);
        let decoded: String = serde_json::from_str(&encoded).expect("valid JSON string");
        assert_eq!(decoded, value);
    }

    #[cfg(feature = "render")]
    #[test]
    fn converter_renders_svg() {
        let converter = MathemaConverter::new("latex").expect("create");
        let result = converter.parse("\\frac{1}{2}").expect("parse");
        let svg = result.to_svg(Some(28.0), Some(true)).expect("svg");
        assert!(svg.contains("<svg"), "expected an <svg> root: {svg}");
        // Embedded-outline SVG contains drawn glyph geometry.
        assert!(svg.contains("<path") || svg.contains("<use"));
    }

    #[cfg(feature = "render")]
    #[test]
    fn converter_renders_content_mathml_svg() {
        let converter = MathemaConverter::new("mathml").expect("create");
        let result = converter
            .parse("<math><apply><ln/><ci>a</ci></apply></math>")
            .expect("parse");
        let svg = result.to_svg(None, None).expect("svg");
        assert!(svg.contains("<path") || svg.contains("<use"));
    }

    #[test]
    fn parse_async_compiles() {
        // Verify sync parse works (async wraps it in a Promise;
        // actual async behavior requires a JS runtime).
        let converter = MathemaConverter::new("latex").expect("create");
        let result = converter.parse("x^2").expect("parse");
        assert!(result.to_mathml().is_ok());
    }
}
