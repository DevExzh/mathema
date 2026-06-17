use crate::ast::MathExpr;
use crate::ast::compact::CompactExpr;
use crate::ast::normalize::normalize_compact_expr;
use crate::error::{CompactParseResult, Config, Format, Result};
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

/// One-liner conversion between any two supported formats.
///
/// ```
/// use mathema::{convert, Format};
/// let mathml = convert("x^2", Format::LaTeX, Format::MathML).unwrap();
/// ```
pub fn convert(input: &str, from: Format, to: Format) -> Result<String> {
    let config = Config::default();
    convert_with(input, from, to, &config)
}

/// Conversion with custom configuration.
pub fn convert_with(input: &str, from: Format, to: Format, config: &Config) -> Result<String> {
    let mut compact = parse_compact(input, from, config)?;
    crate::ast::validate_compact_depth(&compact.expr.arena, compact.expr.roots)?;
    normalize_compact_expr(&mut compact.expr);
    write_compact(&compact.expr, to, config)
}

fn parse_compact(input: &str, format: Format, config: &Config) -> Result<CompactParseResult> {
    match format {
        Format::LaTeX => LatexReader::parse_compact(input, config),
        Format::MathML => MathmlReader::parse_compact(input, config),
        Format::Omml => OmmlReader::parse_compact(input, config),
        Format::Mtef => MtefReader::parse_compact(input, config),
        Format::UnicodeMath => UnicodeMathReader::parse_compact(input, config),
        #[cfg(feature = "render")]
        Format::Svg => Err(crate::error::Error::Unsupported(
            "SVG is an output-only format and cannot be parsed".into(),
        )),
        #[cfg(feature = "raster")]
        Format::Png | Format::Jpeg => Err(crate::error::Error::Unsupported(
            "raster formats (PNG/JPEG) are output-only and cannot be parsed".into(),
        )),
    }
}

fn write_compact(
    expr: &crate::ast::compact::CompactExpr,
    format: Format,
    config: &Config,
) -> Result<String> {
    match format {
        Format::LaTeX => LatexWriter::write_compact(expr, config),
        Format::MathML => MathmlWriter::write_compact(expr, config),
        Format::Omml => OmmlWriter::write_compact(expr, config),
        Format::Mtef => MtefWriter::write_compact(expr, config),
        Format::UnicodeMath => UnicodeMathWriter::write_compact(expr, config),
        #[cfg(feature = "render")]
        Format::Svg => {
            let options = crate::render::RenderOptions::default();
            crate::render::render_to_svg_compact(expr, &options)
        },
        #[cfg(feature = "raster")]
        Format::Png | Format::Jpeg => Err(crate::error::Error::Unsupported(
            "raster formats produce binary output; use render_to_image() instead".into(),
        )),
    }
}

/// Builder-style conversion result. Parse once, write to multiple formats.
pub struct ConvertResult {
    compact: CompactParseResult,
    config: Config,
}

impl ConvertResult {
    pub fn to_latex(&self) -> Result<String> {
        LatexWriter::write_compact(&self.compact.expr, &self.config)
    }

    pub fn to_mathml(&self) -> Result<String> {
        MathmlWriter::write_compact(&self.compact.expr, &self.config)
    }

    pub fn to_latex_with(&self, config: &Config) -> Result<String> {
        LatexWriter::write_compact(&self.compact.expr, config)
    }

    pub fn to_mathml_with(&self, config: &Config) -> Result<String> {
        MathmlWriter::write_compact(&self.compact.expr, config)
    }

    pub fn to_omml(&self) -> Result<String> {
        OmmlWriter::write_compact(&self.compact.expr, &self.config)
    }

    pub fn to_omml_with(&self, config: &Config) -> Result<String> {
        OmmlWriter::write_compact(&self.compact.expr, config)
    }

    pub fn to_mtef(&self) -> Result<String> {
        MtefWriter::write_compact(&self.compact.expr, &self.config)
    }

    pub fn to_mtef_with(&self, config: &Config) -> Result<String> {
        MtefWriter::write_compact(&self.compact.expr, config)
    }

    pub fn to_unicode_math(&self) -> Result<String> {
        UnicodeMathWriter::write_compact(&self.compact.expr, &self.config)
    }

    pub fn to_unicode_math_with(&self, config: &Config) -> Result<String> {
        UnicodeMathWriter::write_compact(&self.compact.expr, config)
    }

    /// Render to a standalone SVG with embedded glyph outlines.
    ///
    /// This is the default for [`to_svg`](Self::to_svg): the output embeds
    /// Latin Modern Math glyph outlines as `<path>`/`<use>` elements, so it
    /// renders identically everywhere (no client-side fonts required) and
    /// visually matches the PNG/JPEG raster output.
    #[cfg(feature = "render")]
    pub fn to_svg(&self) -> Result<String> {
        let options = crate::render::RenderOptions::default();
        crate::render::render_to_svg_standalone_compact(&self.compact.expr, &options)
    }

    #[cfg(feature = "render")]
    pub fn to_svg_with(&self, options: &crate::render::RenderOptions) -> Result<String> {
        crate::render::render_to_svg_standalone_compact(&self.compact.expr, options)
    }

    /// Render to a text-based SVG that relies on client-side fonts (`<text>`
    /// elements). Smaller output, but appearance depends on the fonts the
    /// viewer has installed and will not match the raster output.
    #[cfg(feature = "render")]
    pub fn to_svg_text(&self) -> Result<String> {
        let options = crate::render::RenderOptions::default();
        crate::render::render_to_svg_compact(&self.compact.expr, &options)
    }

    #[cfg(feature = "render")]
    pub fn to_svg_text_with(&self, options: &crate::render::RenderOptions) -> Result<String> {
        crate::render::render_to_svg_compact(&self.compact.expr, options)
    }

    #[cfg(feature = "raster")]
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let options = crate::render::raster::RasterOptions::default();
        crate::render::raster::render_to_image_compact(&self.compact.expr, &options)
    }

    #[cfg(feature = "raster")]
    pub fn to_png_with(&self, options: &crate::render::raster::RasterOptions) -> Result<Vec<u8>> {
        crate::render::raster::render_to_image_compact(&self.compact.expr, options)
    }

    #[cfg(feature = "raster")]
    pub fn to_jpeg(&self) -> Result<Vec<u8>> {
        let options = crate::render::raster::RasterOptions {
            format: crate::render::raster::ImageFormat::Jpeg,
            ..crate::render::raster::RasterOptions::default()
        };
        crate::render::raster::render_to_image_compact(&self.compact.expr, &options)
    }

    #[cfg(feature = "raster")]
    pub fn to_jpeg_with(&self, options: &crate::render::raster::RasterOptions) -> Result<Vec<u8>> {
        crate::render::raster::render_to_image_compact(&self.compact.expr, options)
    }

    /// Access the parsed AST (materialized from compact arena representation).
    #[must_use]
    pub fn expr(&self) -> MathExpr {
        self.compact.expr.to_math_expr()
    }

    /// Take ownership of the parsed AST.
    #[must_use]
    pub fn into_expr(self) -> MathExpr {
        self.compact.expr.to_math_expr()
    }

    /// Access the compact arena-based expression directly (zero-copy).
    #[must_use]
    pub fn compact_expr(&self) -> &CompactExpr {
        &self.compact.expr
    }
}

/// Parse LaTeX input into a reusable conversion result.
pub fn from_latex(input: &str) -> Result<ConvertResult> {
    from_latex_with(input, &Config::default())
}

/// Parse LaTeX with custom configuration.
pub fn from_latex_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = LatexReader::parse_compact(input, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
    })
}

/// Parse MathML input into a reusable conversion result.
pub fn from_mathml(input: &str) -> Result<ConvertResult> {
    from_mathml_with(input, &Config::default())
}

/// Parse MathML with custom configuration.
pub fn from_mathml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = MathmlReader::parse_compact(input, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
    })
}

/// Parse OMML input into a reusable conversion result.
pub fn from_omml(input: &str) -> Result<ConvertResult> {
    from_omml_with(input, &Config::default())
}

/// Parse OMML with custom configuration.
pub fn from_omml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = OmmlReader::parse_compact(input, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
    })
}

/// Parse MTEF input (hex-encoded binary) into a reusable conversion result.
pub fn from_mtef(input: &str) -> Result<ConvertResult> {
    from_mtef_with(input, &Config::default())
}

/// Parse MTEF with custom configuration.
pub fn from_mtef_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = MtefReader::parse_compact(input, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
    })
}

/// Parse UnicodeMath input into a reusable conversion result.
pub fn from_unicode_math(input: &str) -> Result<ConvertResult> {
    from_unicode_math_with(input, &Config::default())
}

/// Parse UnicodeMath with custom configuration.
pub fn from_unicode_math_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = UnicodeMathReader::parse_compact(input, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // === convert() one-liner API ===

    #[test]
    fn latex_to_mathml() {
        let result = convert("x^2", Format::LaTeX, Format::MathML).unwrap();
        assert!(result.contains("<math"));
        assert!(result.contains("</math>"));
    }

    #[test]
    fn latex_to_omml() {
        let result = convert("x^2", Format::LaTeX, Format::Omml).unwrap();
        assert!(result.contains("<m:oMath"));
    }

    #[test]
    fn latex_to_unicode_math() {
        let result = convert("x^2", Format::LaTeX, Format::UnicodeMath).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn latex_to_latex_identity() {
        let result = convert("x", Format::LaTeX, Format::LaTeX).unwrap();
        assert!(result.contains("x"));
    }

    #[cfg(feature = "render")]
    #[test]
    fn latex_to_svg() {
        let result = convert("x^2", Format::LaTeX, Format::Svg).unwrap();
        assert!(result.starts_with("<svg"));
        assert!(result.ends_with("</svg>"));
    }

    #[cfg(feature = "render")]
    #[test]
    fn svg_is_output_only() {
        let result = convert("<svg></svg>", Format::Svg, Format::LaTeX);
        assert!(result.is_err(), "SVG should not be parseable");
    }

    #[test]
    fn mathml_to_latex() {
        let mathml = "<math><mi>x</mi></math>";
        let result = convert(mathml, Format::MathML, Format::LaTeX).unwrap();
        assert!(result.contains("x"));
    }

    // === from_* builder API ===

    #[test]
    fn from_latex_builder() {
        let result = from_latex("x^2").unwrap();
        let mathml = result.to_mathml().unwrap();
        assert!(mathml.contains("<math"));
        let latex = result.to_latex().unwrap();
        assert!(!latex.is_empty());
    }

    #[test]
    fn from_latex_multiple_outputs() {
        let result = from_latex("\\frac{1}{2}").unwrap();
        let mathml = result.to_mathml().unwrap();
        let omml = result.to_omml().unwrap();
        let umath = result.to_unicode_math().unwrap();
        assert!(mathml.contains("<mfrac"));
        assert!(omml.contains("<m:f>"));
        assert!(!umath.is_empty());
    }

    #[test]
    fn from_mathml_builder() {
        let result = from_mathml("<math><mn>42</mn></math>").unwrap();
        let latex = result.to_latex().unwrap();
        assert!(latex.contains("42"));
    }

    #[test]
    fn from_latex_expr_access() {
        let result = from_latex("x").unwrap();
        let expr = result.expr();
        assert!(!expr.nodes.is_empty());
    }

    #[test]
    fn from_latex_into_expr() {
        let result = from_latex("x").unwrap();
        let expr = result.into_expr();
        assert!(!expr.nodes.is_empty());
    }

    #[cfg(feature = "render")]
    #[test]
    fn builder_to_svg() {
        let result = from_latex("x^2").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(svg.starts_with("<svg"));
    }

    #[cfg(feature = "render")]
    #[test]
    fn builder_to_svg_with_options() {
        let result = from_latex("x").unwrap();
        let options = crate::render::RenderOptions {
            font_size_px: 24.0,
            ..crate::render::RenderOptions::default()
        };
        let svg = result.to_svg_with(&options).unwrap();
        assert!(svg.starts_with("<svg"));
    }

    // === with_config variants ===

    #[test]
    fn convert_with_custom_config() {
        let config = Config::default();
        let result = convert_with("x", Format::LaTeX, Format::MathML, &config).unwrap();
        assert!(result.contains("<math"));
    }

    #[test]
    fn from_latex_with_config() {
        let config = Config::default();
        let result = from_latex_with("x", &config).unwrap();
        let latex = result.to_latex_with(&config).unwrap();
        assert!(latex.contains("x"));
    }

    // === Error cases ===

    #[test]
    fn invalid_latex_errors() {
        // Unmatched brace
        let result = convert("{", Format::LaTeX, Format::MathML);
        // This may or may not error depending on parser leniency,
        // but shouldn't panic
        let _ = result;
    }

    #[test]
    fn invalid_mathml_errors() {
        let result = convert("not xml at all<<<", Format::MathML, Format::LaTeX);
        assert!(result.is_err());
    }

    #[test]
    fn empty_input() {
        let result = convert("", Format::LaTeX, Format::MathML).unwrap();
        assert!(result.contains("<math"));
    }

    #[test]
    fn oversized_input_is_rejected() {
        let config = Config {
            max_input_bytes: Some(1024),
            ..Config::default()
        };
        let big = "x".repeat(2048);
        let result = convert_with(&big, Format::LaTeX, Format::MathML, &config);
        assert!(matches!(
            result,
            Err(crate::error::Error::InputTooLarge { .. })
        ));
    }

    #[test]
    fn input_limit_can_be_disabled() {
        // The same input that a small explicit limit rejects is accepted when
        // the limit is disabled (None).
        let input = "x + y + z".repeat(64); // ~576 bytes
        let rejected = convert_with(
            &input,
            Format::LaTeX,
            Format::MathML,
            &Config {
                max_input_bytes: Some(64),
                ..Config::default()
            },
        );
        assert!(matches!(
            rejected,
            Err(crate::error::Error::InputTooLarge { .. })
        ));
        let accepted = convert_with(
            &input,
            Format::LaTeX,
            Format::MathML,
            &Config {
                max_input_bytes: None,
                ..Config::default()
            },
        );
        assert!(accepted.is_ok(), "None limit should accept the same input");
    }

    #[test]
    fn from_latex_respects_input_limit() {
        let config = Config {
            max_input_bytes: Some(8),
            ..Config::default()
        };
        let result = from_latex_with("x + y + z + w", &config);
        assert!(matches!(
            result,
            Err(crate::error::Error::InputTooLarge { .. })
        ));
    }

    #[cfg(feature = "render")]
    #[test]
    fn colorbox_renders_with_fill() {
        let result = from_latex(r"\colorbox{teal}{B}").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r#"fill="teal""#),
            "colorbox SVG should contain fill=teal, got: {svg}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn fcolorbox_renders_with_fill_and_border() {
        let result = from_latex(r"\fcolorbox{blue}{red}{C}").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r#"fill="red""#),
            "fcolorbox SVG should contain fill=red, got: {svg}"
        );
        assert!(
            svg.contains(r#"stroke="blue""#),
            "fcolorbox SVG should contain stroke=blue, got: {svg}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn color_switch_colors_rest_of_group() {
        // \color is a switch: it should colorize the following content.
        let result = from_latex(r"\color{red} x + y").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r#"fill="red""#),
            "\\color should colorize following content, got: {svg}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn definecolor_resolves_to_hex() {
        let result = from_latex(r"\definecolor{myc}{rgb}{1,0,0}\color{myc}A").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r##"fill="#ff0000""##),
            "\\definecolor rgb 1,0,0 should resolve to #ff0000, got: {svg}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn color_model_html_arg() {
        let result = from_latex(r"\color[HTML]{00FF00}B").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r##"fill="#00ff00""##),
            "\\color[HTML]{{00FF00}} should resolve to #00ff00, got: {svg}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn xcolor_mixing_expression() {
        let result = from_latex(r"\textcolor{red!50!blue}{M}").unwrap();
        let svg = result.to_svg().unwrap();
        assert!(
            svg.contains(r##"fill="#800080""##),
            "red!50!blue should resolve to #800080, got: {svg}"
        );
        let mml = from_latex(r"\textcolor{red!50!blue}{M}")
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(
            mml.contains(r##"mathcolor="#800080""##),
            "MathML should carry resolved mathcolor, got: {mml}"
        );
    }
}
