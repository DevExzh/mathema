//! Top-level conversion entry points: the one-shot [`convert`] function and the
//! parse-once [`from_latex`]/[`from_mathml`]/… builders returning [`ConvertResult`].

use crate::ast::MathExpr;
use crate::ast::compact::CompactExpr;
use crate::ast::normalize::{
    normalize_compact_expr, normalize_compact_expr_keeping_identifiers,
    normalize_compact_expr_opening_arguments,
};
use crate::error::{CompactParseResult, Config, Format, Result};
use crate::reader::Reader;
use crate::reader::latex::LatexReader;
use crate::reader::mathml::MathmlReader;
use crate::reader::mtef::{MtefInfo, MtefReader};
use crate::reader::omml::OmmlReader;
use crate::reader::unicode_math::UnicodeMathReader;
use crate::writer::Writer;
use crate::writer::latex::LatexWriter;
use crate::writer::mathml::MathmlWriter;
use crate::writer::mtef::{MtefWriteOptions, MtefWriter};
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
    crate::ast::validate_compact_depth_with(
        &compact.expr.arena,
        compact.expr.roots,
        config.effective_max_depth(),
    )?;
    normalize_for(&mut compact.expr, from, to)?;
    write_compact(&compact.expr, to, config)
}

/// Normalize a freshly read expression. The letters of an operator's name are
/// merged neither on the way from MTEF, whose characters are atoms of their own,
/// nor on the way to it.
///
/// What the target does not write goes first: it must not keep apart what
/// normalization joins.
fn normalize_for(expr: &mut CompactExpr, from: Format, to: Format) -> Result<()> {
    let normalize = if from == Format::Mtef || to == Format::Mtef {
        normalize_compact_expr_keeping_identifiers
    } else if to == Format::LaTeX {
        normalize_compact_expr_opening_arguments
    } else {
        normalize_compact_expr
    };
    if from == Format::Mtef && to == Format::LaTeX && expr.arena.has_attrs() {
        // As it is normalized when it is read for a `ConvertResult`: sizes are
        // declared for the nodes of a list, not of groups in it.
        // (The colors of MTEF can all be written.)
        normalize(expr)?;
    }
    drop_unwritten(expr, to)?;
    normalize(expr)
}

/// Drop what `target` does not write, before normalization: the font sizes and
/// colors of pure OMML, the sizes of UnicodeMath, and in LaTeX those that no
/// declaration draws and the colors it cannot hold.
pub(crate) fn drop_unwritten(expr: &mut CompactExpr, target: Format) -> Result<()> {
    match target {
        Format::LaTeX => {
            crate::writer::latex::colors::to_groups(expr);
            if expr.arena.has_attrs() {
                *expr = crate::writer::latex::lower_sizes(expr)?;
            }
        },
        Format::Omml => {
            crate::writer::omml::drop_colors(expr);
            expr.arena.clear_attrs();
        },
        Format::UnicodeMath => expr.arena.clear_attrs(),
        _ => {},
    }
    Ok(())
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
    mtef: Option<MtefInfo>,
}

impl ConvertResult {
    /// How the expression was normalized when it was read, if it was.
    fn normalization(&self) -> Option<crate::writer::latex::Normalize> {
        self.mtef
            .is_some()
            .then_some(normalize_compact_expr_keeping_identifiers as _)
    }

    /// The expression without its font sizes, for a format that writes none,
    /// as it would have been read without them. `None` when it has none.
    fn without_sizes(&self) -> Result<Option<CompactExpr>> {
        if !self.compact.expr.arena.has_attrs() {
            return Ok(None);
        }
        let mut plain = self.compact.expr.clone();
        plain.arena.clear_attrs();
        if let Some(normalize) = self.normalization() {
            normalize(&mut plain)?;
        }
        Ok(Some(plain))
    }

    /// The expression without the sizes and colors that pure OMML does not
    /// write, if it has some.
    fn in_pure_omml(&self) -> Result<Option<CompactExpr>> {
        let expr = &self.compact.expr;
        if !expr.arena.has_attrs() && !crate::writer::omml::has_colors(expr) {
            return Ok(None);
        }
        let mut plain = expr.clone();
        drop_unwritten(&mut plain, Format::Omml)?;
        if let Some(normalize) = self.normalization() {
            normalize(&mut plain)?;
        }
        Ok(Some(plain))
    }

    fn write_omml(
        &self,
        config: &Config,
        options: &crate::writer::omml::OmmlOptions,
    ) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        let expr = &self.compact.expr;
        let written = OmmlWriter::write_with_options(expr, config, options)?;
        let plain = match options.run_properties {
            crate::writer::omml::OmmlRunProperties::WordprocessingMl => None,
            crate::writer::omml::OmmlRunProperties::DrawingMl => self.without_sizes()?,
            crate::writer::omml::OmmlRunProperties::None => self.in_pure_omml()?,
        };
        if let Some(plain) = plain {
            let (omml, _) = OmmlWriter::write_with_options(&plain, config, options)?;
            return Ok((omml, written.1));
        }
        Ok(written)
    }

    /// Serialize the parsed expression to LaTeX.
    pub fn to_latex(&self) -> Result<String> {
        LatexWriter::write_read(&self.compact.expr, &self.config, self.normalization())
    }

    /// Serialize the parsed expression to Presentation MathML.
    pub fn to_mathml(&self) -> Result<String> {
        MathmlWriter::write_compact(&self.compact.expr, &self.config)
    }

    /// Serialize to LaTeX, also returning writer-side loss diagnostics: text
    /// that is no color in a color, control characters in text, font sizes.
    /// See [`to_mathml_with_diagnostics`](Self::to_mathml_with_diagnostics).
    pub fn to_latex_with_diagnostics(&self) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        LatexWriter::write_read_with_diagnostics(
            &self.compact.expr,
            &self.config,
            self.normalization(),
        )
    }

    /// Serialize to LaTeX using the given configuration.
    pub fn to_latex_with(&self, config: &Config) -> Result<String> {
        LatexWriter::write_read(&self.compact.expr, config, self.normalization())
    }

    /// Serialize to Presentation MathML using the given configuration.
    pub fn to_mathml_with(&self, config: &Config) -> Result<String> {
        MathmlWriter::write_compact(&self.compact.expr, config)
    }

    /// Serialize the parsed expression to OMML (Office MathML).
    pub fn to_omml(&self) -> Result<String> {
        self.to_omml_with(&self.config)
    }

    /// Serialize to OMML using the given configuration.
    pub fn to_omml_with(&self, config: &Config) -> Result<String> {
        let plain = self.in_pure_omml()?;
        OmmlWriter::write_compact(plain.as_ref().unwrap_or(&self.compact.expr), config)
    }

    /// Serialize to OMML with writer options (host run-property dialect,
    /// display block and justification), also returning writer-side loss
    /// diagnostics.
    ///
    /// ```
    /// use mathema::{OmmlOptions, OmmlRunProperties};
    /// let result = mathema::from_latex(r"\textcolor{blue}{y}").unwrap();
    /// let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::DrawingMl);
    /// let (omml, _) = result.to_omml_with_options(&options).unwrap();
    /// assert!(omml.contains(r#"<a:srgbClr val="0000FF"/>"#));
    /// ```
    pub fn to_omml_with_options(
        &self,
        options: &crate::writer::omml::OmmlOptions,
    ) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        self.write_omml(&self.config, options)
    }

    /// Serialize the parsed expression to MTEF 5, hex-encoded: the bytes
    /// [`to_mtef_bytes`](Self::to_mtef_bytes) returns with the default
    /// [`MtefWriteOptions`].
    pub fn to_mtef(&self) -> Result<String> {
        MtefWriter::write_compact(&self.compact.expr, &self.config)
    }

    /// Serialize the parsed expression to binary MTEF.
    ///
    /// `options` selects MTEF 5 (MathType, the default) or MTEF 3 (Equation
    /// Editor 3.x) and whether to add the OLE `Equation Native` header. To
    /// write an equation read from MTEF back as the same kind of object, use
    /// [`MtefWriteOptions::from_info`] with [`mtef_info`](Self::mtef_info).
    ///
    /// ```
    /// use mathema::MtefWriteOptions;
    ///
    /// let result = mathema::from_latex(r"\sqrt{x}").unwrap();
    /// let bytes = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    /// let back = mathema::from_mtef_bytes(&bytes).unwrap();
    /// assert_eq!(back.to_latex().unwrap(), r"\sqrt{x}");
    ///
    /// // Keep the version and product of an equation read from MTEF.
    /// let options = back.mtef_info().map(MtefWriteOptions::from_info).unwrap_or_default();
    /// assert_eq!(back.to_mtef_bytes(&options).unwrap(), bytes);
    /// ```
    ///
    /// # Errors
    ///
    /// See [`MtefWriter::write_bytes`].
    pub fn to_mtef_bytes(&self, options: &MtefWriteOptions) -> Result<Vec<u8>> {
        MtefWriter::write_bytes(&self.compact.expr, &self.config, options)
    }

    /// Serialize to binary MTEF, also returning writer-side loss
    /// diagnostics. See [`to_mtef_bytes`](Self::to_mtef_bytes) and
    /// [`to_mathml_with_diagnostics`](Self::to_mathml_with_diagnostics).
    pub fn to_mtef_bytes_with_diagnostics(
        &self,
        options: &MtefWriteOptions,
    ) -> Result<(Vec<u8>, Vec<crate::error::Diagnostic>)> {
        MtefWriter::write_bytes_with_diagnostics(&self.compact.expr, &self.config, options)
    }

    /// Serialize to MTEF using the given configuration.
    pub fn to_mtef_with(&self, config: &Config) -> Result<String> {
        MtefWriter::write_compact(&self.compact.expr, config)
    }

    /// Serialize the parsed expression to UnicodeMath linear notation.
    pub fn to_unicode_math(&self) -> Result<String> {
        self.to_unicode_math_with(&self.config)
    }

    /// Serialize to UnicodeMath using the given configuration.
    pub fn to_unicode_math_with(&self, config: &Config) -> Result<String> {
        if !self.compact.expr.arena.has_attrs() {
            return UnicodeMathWriter::write_compact(&self.compact.expr, config);
        }
        let plain = self.without_sizes()?;
        UnicodeMathWriter::write_compact(plain.as_ref().unwrap_or(&self.compact.expr), config)
    }

    /// Serialize to Presentation MathML, also returning diagnostics for any
    /// information that the target format cannot represent (writer-side loss).
    ///
    /// Unlike [`diagnostics`](Self::diagnostics) — which reports *reader-side*
    /// warnings from parsing — this surfaces *writer-side* loss (e.g. a node
    /// kind the target cannot express). Use it when round-trip fidelity matters.
    pub fn to_mathml_with_diagnostics(&self) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        MathmlWriter::write_compact_with_diagnostics(&self.compact.expr, &self.config)
    }

    /// Serialize to OMML, also returning writer-side loss diagnostics.
    /// See [`to_mathml_with_diagnostics`](Self::to_mathml_with_diagnostics).
    pub fn to_omml_with_diagnostics(&self) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        self.write_omml(&self.config, &Default::default())
    }

    /// Serialize to MTEF, also returning writer-side loss diagnostics. MTEF
    /// silently drops several decorations (rules, colors, scale, phantoms), so
    /// this is the recommended entry point when targeting MathType.
    /// See [`to_mathml_with_diagnostics`](Self::to_mathml_with_diagnostics).
    pub fn to_mtef_with_diagnostics(&self) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        MtefWriter::write_compact_with_diagnostics(&self.compact.expr, &self.config)
    }

    /// Serialize to UnicodeMath, also returning writer-side loss diagnostics.
    /// UnicodeMath is an intentionally lossy linear notation; this reports the
    /// node kinds that could not be represented faithfully.
    /// See [`to_mathml_with_diagnostics`](Self::to_mathml_with_diagnostics).
    pub fn to_unicode_math_with_diagnostics(
        &self,
    ) -> Result<(String, Vec<crate::error::Diagnostic>)> {
        let output = self.to_unicode_math()?;
        let diagnostics = UnicodeMathWriter::compact_diagnostics(&self.compact.expr, &self.config);
        Ok((output, diagnostics))
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

    /// Render a standalone SVG using custom [`RenderOptions`](crate::render::RenderOptions).
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

    /// Render a text-based (client-font) SVG using custom [`RenderOptions`](crate::render::RenderOptions).
    #[cfg(feature = "render")]
    pub fn to_svg_text_with(&self, options: &crate::render::RenderOptions) -> Result<String> {
        crate::render::render_to_svg_compact(&self.compact.expr, options)
    }

    /// Render the expression to PNG image bytes.
    #[cfg(feature = "raster")]
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let options = crate::render::raster::RasterOptions::default();
        crate::render::raster::render_to_image_compact(&self.compact.expr, &options)
    }

    /// Render to PNG image bytes using custom [`RasterOptions`](crate::render::raster::RasterOptions).
    #[cfg(feature = "raster")]
    pub fn to_png_with(&self, options: &crate::render::raster::RasterOptions) -> Result<Vec<u8>> {
        crate::render::raster::render_to_image_compact(&self.compact.expr, options)
    }

    /// Render the expression to JPEG image bytes.
    #[cfg(feature = "raster")]
    pub fn to_jpeg(&self) -> Result<Vec<u8>> {
        let options = crate::render::raster::RasterOptions {
            format: crate::render::raster::ImageFormat::Jpeg,
            ..crate::render::raster::RasterOptions::default()
        };
        crate::render::raster::render_to_image_compact(&self.compact.expr, &options)
    }

    /// Render to JPEG image bytes using custom [`RasterOptions`](crate::render::raster::RasterOptions).
    #[cfg(feature = "raster")]
    pub fn to_jpeg_with(&self, options: &crate::render::raster::RasterOptions) -> Result<Vec<u8>> {
        crate::render::raster::render_to_image_compact(&self.compact.expr, options)
    }

    /// Access the parsed AST as a legacy [`MathExpr`].
    ///
    /// **Cost:** this *materializes* a full heap `Box`/`Vec`/`String` tree from
    /// the compact arena representation and is measurably expensive — roughly
    /// 60% of a full parse for a typical formula. Throughput-sensitive callers
    /// should prefer [`compact_expr`](Self::compact_expr) together with the
    /// `*_compact` writers/renderers (e.g. [`to_mathml`](Self::to_mathml), or
    /// `to_svg` with the `render` feature), which operate on the compact tree
    /// with no legacy materialization.
    #[must_use]
    pub fn expr(&self) -> MathExpr {
        self.compact.expr.to_math_expr()
    }

    /// Take ownership of the parsed AST as a legacy [`MathExpr`].
    ///
    /// Like [`expr`](Self::expr), this materializes a full heap tree; see that
    /// method's cost note and prefer [`compact_expr`](Self::compact_expr) on hot
    /// paths.
    #[must_use]
    pub fn into_expr(self) -> MathExpr {
        self.compact.expr.to_math_expr()
    }

    /// Access the compact arena-based expression directly (zero-copy).
    ///
    /// This is the fast path: it borrows the already-parsed compact tree with no
    /// allocation. Pair it with the `*_compact` writers/renderers for the lowest
    /// per-conversion cost.
    #[must_use]
    pub fn compact_expr(&self) -> &CompactExpr {
        &self.compact.expr
    }

    /// Header information of MTEF input: its format version, generating
    /// product and platform, and the OLE header of an `Equation Native`
    /// stream. `None` for other input formats.
    #[must_use]
    pub fn mtef_info(&self) -> Option<&MtefInfo> {
        self.mtef.as_ref()
    }

    /// Diagnostics collected while *parsing* the input (e.g. lenient-mode
    /// recovery warnings, and metadata-dropping macros such as `\href` /
    /// `\includegraphics`).
    ///
    /// Note: these are **reader-side** diagnostics. Writer-side loss (e.g. a
    /// format that cannot represent color) is reported separately by
    /// [`crate::writer::Writer::write_compact_with_diagnostics`].
    #[must_use]
    pub fn diagnostics(&self) -> &[crate::error::Diagnostic] {
        &self.compact.diagnostics
    }
}

/// Parse LaTeX input into a reusable conversion result.
pub fn from_latex(input: &str) -> Result<ConvertResult> {
    from_latex_with(input, &Config::default())
}

/// Parse LaTeX with custom configuration.
pub fn from_latex_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = LatexReader::parse_compact(input, config)?;
    check_depth(&compact, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
        mtef: None,
    })
}

/// Parse MathML input into a reusable conversion result.
pub fn from_mathml(input: &str) -> Result<ConvertResult> {
    from_mathml_with(input, &Config::default())
}

/// Parse MathML with custom configuration.
pub fn from_mathml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = MathmlReader::parse_compact(input, config)?;
    check_depth(&compact, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
        mtef: None,
    })
}

/// Parse OMML input into a reusable conversion result.
pub fn from_omml(input: &str) -> Result<ConvertResult> {
    from_omml_with(input, &Config::default())
}

/// Parse OMML with custom configuration.
pub fn from_omml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = OmmlReader::parse_compact(input, config)?;
    check_depth(&compact, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
        mtef: None,
    })
}

/// Parse MTEF input (hex-encoded binary) into a reusable conversion result.
///
/// The bytes the hex string encodes are read like [`from_mtef_bytes`] reads
/// them, including an `Equation Native` stream's OLE header.
pub fn from_mtef(input: &str) -> Result<ConvertResult> {
    from_mtef_with(input, &Config::default())
}

/// Parse hex-encoded MTEF with custom configuration. See [`from_mtef`].
pub fn from_mtef_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let (compact, info) = MtefReader::parse_hex_with_info(input, config)?;
    finish_mtef(compact, info, config)
}

/// Parse binary MTEF data into a reusable conversion result.
///
/// Accepts bare MTEF 2 (Equation Editor 1.x and 2.x, MathType 3.0), MTEF 3
/// (Equation Editor 3.x), MTEF 4 (MathType 3.5) or MTEF 5 (MathType 4 and
/// later) data, or the contents of an OLE `Equation Native` stream, whose 28-byte
/// `EQNOLEFILEHDR` is detected and validated. The result is normalized and
/// depth-checked exactly as [`convert`] does, and
/// [`ConvertResult::mtef_info`] reports the version and product the input
/// declared.
///
/// ```
/// // MTEF 5 written by MathType 6: the equation `x`.
/// let mut mtef = vec![5, 1, 0, 6, 0];
/// mtef.extend_from_slice(b"DSMT6\0");
/// mtef.extend_from_slice(&[0x01, 0x0A, 0x01, 0x00, 0x02, 0x00, 0x83, b'x', 0x00, 0x00, 0x00]);
/// let result = mathema::from_mtef_bytes(&mtef).unwrap();
/// assert_eq!(result.to_latex().unwrap(), "x");
/// assert_eq!(result.mtef_info().unwrap().application_key.as_deref(), Some("DSMT6"));
/// ```
pub fn from_mtef_bytes(bytes: &[u8]) -> Result<ConvertResult> {
    from_mtef_bytes_with(bytes, &Config::default())
}

/// Parse binary MTEF data with custom configuration. See [`from_mtef_bytes`].
pub fn from_mtef_bytes_with(bytes: &[u8], config: &Config) -> Result<ConvertResult> {
    let (compact, info) = MtefReader::parse_bytes_with_info(bytes, config)?;
    finish_mtef(compact, info, config)
}

/// Refuse trees deeper than [`Config::max_depth`] before handing them out.
fn check_depth(compact: &CompactParseResult, config: &Config) -> Result<()> {
    crate::ast::validate_compact_depth_with(
        &compact.expr.arena,
        compact.expr.roots,
        config.effective_max_depth(),
    )
}

/// Apply the checks and normalization [`convert_with`] applies.
fn finish_mtef(
    mut compact: CompactParseResult,
    mtef: Option<MtefInfo>,
    config: &Config,
) -> Result<ConvertResult> {
    check_depth(&compact, config)?;
    normalize_for(&mut compact.expr, Format::Mtef, Format::Mtef)?;
    Ok(ConvertResult {
        compact,
        config: *config,
        mtef,
    })
}

/// Parse UnicodeMath input into a reusable conversion result.
pub fn from_unicode_math(input: &str) -> Result<ConvertResult> {
    from_unicode_math_with(input, &Config::default())
}

/// Parse UnicodeMath with custom configuration.
pub fn from_unicode_math_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let compact = UnicodeMathReader::parse_compact(input, config)?;
    check_depth(&compact, config)?;
    Ok(ConvertResult {
        compact,
        config: *config,
        mtef: None,
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

    #[cfg(feature = "render")]
    #[test]
    fn public_span_mutations_keep_physical_slots_across_all_routes() {
        use crate::ast::{ColumnAlign, MathExpr, MathNode};
        use crate::{CompactNode, StrId};

        fn assert_routes(compact: &CompactExpr, heap: &MathExpr, cells: usize) {
            let config = Config::default();
            let heap_compact = CompactExpr::from_math_expr(heap).unwrap();
            assert_eq!(
                LatexWriter::write_compact(compact, &config).unwrap(),
                LatexWriter::write(heap, &config).unwrap()
            );
            assert_eq!(
                MathmlWriter::write_compact(compact, &config).unwrap(),
                MathmlWriter::write(heap, &config).unwrap()
            );
            assert_eq!(
                UnicodeMathWriter::write_compact(compact, &config).unwrap(),
                UnicodeMathWriter::write(heap, &config).unwrap()
            );
            assert_eq!(
                OmmlWriter::write_compact(compact, &config).unwrap(),
                OmmlWriter::write(heap, &config).unwrap()
            );
            assert_eq!(
                crate::render::render_to_svg_compact(
                    compact,
                    &crate::render::RenderOptions::default()
                )
                .unwrap(),
                crate::render::render_to_svg(heap, &crate::render::RenderOptions::default())
                    .unwrap()
            );
            for version in [
                crate::reader::mtef::MtefVersion::V3,
                crate::reader::mtef::MtefVersion::V5,
            ] {
                let options = MtefWriteOptions::new(version);
                let compact_bytes = MtefWriter::write_bytes(compact, &config, &options).unwrap();
                let heap_bytes = MtefWriter::write_bytes(&heap_compact, &config, &options).unwrap();
                assert_eq!(compact_bytes, heap_bytes);
                let back = from_mtef_bytes(&compact_bytes).unwrap();
                assert_eq!(back.to_mathml().unwrap().matches("<mtd").count(), cells);
            }
        }

        let source = from_latex(r"\begin{array}{lll}\multicolumn{2}{r}{7}&9\end{array}").unwrap();
        let mut shrunk = source.compact_expr().clone();
        let root = shrunk.arena.children(shrunk.roots)[0];
        let cell = match *shrunk.arena.node(root) {
            CompactNode::Array { cells, .. } => shrunk.arena.children(cells)[0],
            _ => panic!("expected array"),
        };
        let CompactNode::SpannedCell {
            alignment, content, ..
        } = *shrunk.arena.node(cell)
        else {
            panic!("expected span")
        };
        *shrunk.arena.node_mut_in_place(cell) = CompactNode::SpannedCell {
            alignment,
            span: 1,
            left_boundaries: StrId::EMPTY,
            right_boundaries: StrId::EMPTY,
            content,
        };
        let shrunk_heap = shrunk.to_math_expr();
        let rows = match &shrunk_heap.nodes[0] {
            MathNode::Array { rows, .. } | MathNode::NativeArray { rows, .. } => rows,
            _ => panic!("expected heap array"),
        };
        assert_eq!(rows[0].len(), 3);
        assert!(matches!(&rows[0][1][..], []));
        assert!(matches!(&rows[0][2][..], [MathNode::Number(value)] if value == "9"));
        assert_routes(&shrunk, &shrunk_heap, 3);

        let content_span = shrunk.arena.push_children(&[content]).unwrap();
        *shrunk.arena.node_mut_in_place(cell) = CompactNode::Row(content_span);
        let ordinary_heap = shrunk.to_math_expr();
        assert_routes(&shrunk, &ordinary_heap, 3);
        assert!(
            LatexWriter::write_compact(&shrunk, &Config::default())
                .unwrap()
                .contains("7 &  & 9")
        );

        let partial = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Left; 3],
            rows: vec![
                vec![
                    vec![MathNode::Identifier("a".into())],
                    vec![MathNode::Identifier("b".into())],
                    vec![MathNode::Identifier("c".into())],
                ],
                vec![vec![MathNode::SpannedCell {
                    alignment: ColumnAlign::Left,
                    span: 1,
                    left_boundaries: Vec::new(),
                    right_boundaries: Vec::new(),
                    content: Box::new(MathNode::Identifier("7".into())),
                }]],
            ],
            arraystretch: None,
            add_jot: false,
        }]);
        let mut partial_compact = CompactExpr::from_math_expr(&partial).unwrap();
        let root = partial_compact.arena.children(partial_compact.roots)[0];
        let cell = match *partial_compact.arena.node(root) {
            CompactNode::Array {
                cells, num_cols, ..
            } => partial_compact.arena.children(cells)[num_cols as usize],
            _ => panic!("expected array"),
        };
        let CompactNode::SpannedCell {
            alignment, content, ..
        } = *partial_compact.arena.node(cell)
        else {
            panic!("expected span")
        };
        *partial_compact.arena.node_mut_in_place(cell) = CompactNode::SpannedCell {
            alignment,
            span: 3,
            left_boundaries: StrId::EMPTY,
            right_boundaries: StrId::EMPTY,
            content,
        };
        let partial_heap = partial_compact.to_math_expr();
        let rows = match &partial_heap.nodes[0] {
            MathNode::Array { rows, .. } | MathNode::NativeArray { rows, .. } => rows,
            _ => panic!("expected heap array"),
        };
        assert!(matches!(
            &rows[1][0][..],
            [MathNode::SpannedCell { span: 3, .. }]
        ));
        let partial_mathml =
            MathmlWriter::write_compact(&partial_compact, &Config::default()).unwrap();
        assert!(partial_mathml.contains(r#"columnspan="3""#));
        assert_routes(&partial_compact, &partial_heap, 6);

        let contradictory_source =
            from_latex(r"\begin{array}{ll}\multicolumn{1}{r}{7}&9\end{array}").unwrap();
        let mut contradictory = contradictory_source.compact_expr().clone();
        let root = contradictory.arena.children(contradictory.roots)[0];
        let cell = match *contradictory.arena.node(root) {
            CompactNode::Array { cells, .. } => contradictory.arena.children(cells)[0],
            _ => panic!("expected array"),
        };
        let (alignment, content) = match *contradictory.arena.node(cell) {
            CompactNode::AlignedCell { alignment, content }
            | CompactNode::SpannedCell {
                alignment, content, ..
            } => (alignment, content),
            _ => panic!("expected span wrapper"),
        };
        *contradictory.arena.node_mut_in_place(cell) = CompactNode::SpannedCell {
            alignment,
            span: 2,
            left_boundaries: StrId::EMPTY,
            right_boundaries: StrId::EMPTY,
            content,
        };
        let contradictory_heap = contradictory.to_math_expr();
        let rows = match &contradictory_heap.nodes[0] {
            MathNode::Array { rows, .. } | MathNode::NativeArray { rows, .. } => rows,
            _ => panic!("expected heap array"),
        };
        assert!(
            matches!(&rows[0][0][..], [MathNode::SpannedCell { span: 1, .. }]),
            "{rows:?}"
        );
        let contradictory_latex =
            LatexWriter::write_compact(&contradictory, &Config::default()).unwrap();
        assert!(contradictory_latex.contains(r"\multicolumn{1}"));
        assert!(!contradictory_latex.contains(r"\multicolumn{2}"));
        assert_routes(&contradictory, &contradictory_heap, 2);

        // A public caller can construct a compact grid whose final row is
        // short. The layout path must retain that row and cap its span to the
        // actual available slots.
        let mut arena = crate::AstArena::new();
        let columns: Vec<_> = (0..3)
            .map(|_| arena.push_node(CompactNode::ColAlign(ColumnAlign::Left)))
            .collect();
        let col_aligns = arena.push_children(&columns).unwrap();
        let mut identifier = |value: &str| {
            let sid = arena.push_str(value);
            arena.push_node(CompactNode::Identifier(sid))
        };
        let a = identifier("a");
        let b = identifier("b");
        let c = identifier("c");
        let seven = identifier("7");
        let span = arena.push_node(CompactNode::SpannedCell {
            alignment: ColumnAlign::Left,
            span: 3,
            left_boundaries: StrId::EMPTY,
            right_boundaries: StrId::EMPTY,
            content: seven,
        });
        let cells = arena.push_children(&[a, b, c, span]).unwrap();
        let root = arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: 3,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        let roots = arena.push_children(&[root]).unwrap();
        let partial_compact = CompactExpr::new(arena, roots, crate::ast::DisplayStyle::Inline);
        let partial_heap = partial_compact.to_math_expr();
        let rows = match &partial_heap.nodes[0] {
            MathNode::Array { rows, .. } | MathNode::NativeArray { rows, .. } => rows,
            _ => panic!("expected heap array"),
        };
        assert!(matches!(
            &rows[1][0][..],
            [MathNode::SpannedCell { span: 1, .. }]
        ));
        assert_routes(&partial_compact, &partial_heap, 6);
    }

    #[test]
    fn unicode_math_projects_a_spanning_cell_to_its_logical_columns() {
        use crate::ast::{ColumnAlign, MathExpr, MathNode};
        let cell = MathNode::SpannedCell {
            alignment: ColumnAlign::Center,
            span: 2,
            left_boundaries: vec![],
            right_boundaries: vec![],
            content: Box::new(MathNode::Number("7".into())),
        };
        let expr = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Center; 4],
            rows: vec![
                vec![
                    vec![MathNode::Number("1".into())],
                    vec![cell],
                    vec![MathNode::Number("9".into())],
                ],
                vec![vec![MathNode::Number("2".into())]],
                vec![],
            ],
            arraystretch: None,
            add_jot: false,
        }]);
        let cfg = Config::default();
        let heap = UnicodeMathWriter::write(&expr, &cfg).unwrap();
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        assert_eq!(heap, "■(1&7&&9@2&&&@&&&)");
        assert_eq!(
            heap,
            UnicodeMathWriter::write_compact(&compact, &cfg).unwrap()
        );
    }

    #[test]
    fn omml_column_properties_retain_unanimous_cell_overrides() {
        for (src, aligned_cells, loses_alignment) in [
            (
                r"\begin{array}{ll}\multicolumn{1}{r}{a}&1\\\multicolumn{1}{r}{bb}&2\end{array}",
                2,
                false,
            ),
            (
                r"\begin{array}{ll}\multicolumn{1}{r}{a}&1\\b&2\end{array}",
                0,
                true,
            ),
        ] {
            let (omml, diags) = from_latex(src).unwrap().to_omml_with_diagnostics().unwrap();
            let readback = from_omml(&omml).unwrap().to_mathml().unwrap();
            assert_eq!(
                readback.matches(r#"columnalign="right""#).count(),
                aligned_cells,
                "{readback}"
            );
            assert_eq!(
                diags.iter().any(|d| d.message.contains("cell alignment")),
                loses_alignment,
                "{diags:?}"
            );
        }
    }

    #[test]
    fn mtef_uses_an_effective_alignment_when_every_cell_agrees() {
        let source = r"\begin{array}{lc}\multicolumn{1}{r}{a}&\multicolumn{1}{r}{1}\\\multicolumn{1}{r}{bb}&\multicolumn{1}{r}{2}\end{array}";
        for v in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
            let (bytes, diags) = from_latex(source)
                .unwrap()
                .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(v))
                .unwrap();
            assert!(
                !diags.iter().any(|d| d.message.contains("alignment")),
                "{diags:?}"
            );
            let xml = from_mtef_bytes(&bytes).unwrap().to_mathml().unwrap();
            assert_eq!(xml.matches(r#"columnalign="right""#).count(), 4, "{xml}");
        }
    }

    #[test]
    fn spanned_cell_interchange_reports_only_unrepresentable_table_metadata() {
        use crate::ast::{ColumnAlign, MathExpr, MathNode, SpanBoundary};

        let expr = MathExpr::inline(vec![MathNode::Array {
            columns: vec![
                ColumnAlign::Left,
                ColumnAlign::Left,
                ColumnAlign::Left,
                ColumnAlign::Left,
            ],
            rows: vec![vec![
                vec![MathNode::Identifier("a".into())],
                vec![MathNode::SpannedCell {
                    alignment: ColumnAlign::Right,
                    span: 2,
                    left_boundaries: vec![SpanBoundary::SuppressGlue, SpanBoundary::Rule],
                    right_boundaries: vec![SpanBoundary::Rule, SpanBoundary::SuppressGlue],
                    content: Box::new(MathNode::Identifier("LONG".into())),
                }],
                vec![MathNode::Identifier("z".into())],
            ]],
            arraystretch: None,
            add_jot: false,
        }]);
        let compact = CompactExpr::from_math_expr(&expr).unwrap();
        let config = Config::default();

        let (omml, omml_diagnostics) =
            OmmlWriter::write_compact_with_diagnostics(&compact, &config).unwrap();
        assert!(
            omml_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("cell-span construct"))
        );
        assert!(
            omml_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("boundary material"))
        );

        let (mtef5, mtef5_diagnostics) = MtefWriter::write_bytes_with_diagnostics(
            &compact,
            &config,
            &MtefWriteOptions::default(),
        )
        .unwrap();
        assert!(
            mtef5_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("cell-span construct"))
        );
        assert!(
            mtef5_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("boundary")
                    || diagnostic.message.contains("padding"))
        );

        let (mtef3, mtef3_diagnostics) = MtefWriter::write_bytes_with_diagnostics(
            &compact,
            &config,
            &MtefWriteOptions::new(crate::reader::mtef::MtefVersion::V3),
        )
        .unwrap();
        assert!(
            mtef3_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("cell-span construct"))
        );
        assert!(
            mtef3_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("boundary")
                    || diagnostic.message.contains("padding"))
        );

        let (unicode, unicode_diagnostics) =
            UnicodeMathWriter::write_compact_with_diagnostics(&compact, &config).unwrap();
        assert!(
            unicode_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("separate matrix entries"))
        );
        assert!(
            unicode_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("boundary material"))
        );

        let omml_back = from_omml(&omml).unwrap().to_mathml().unwrap();
        let mtef5_back = from_mtef_bytes(&mtef5).unwrap().to_mathml().unwrap();
        let mtef3_back = from_mtef_bytes(&mtef3).unwrap().to_mathml().unwrap();
        let unicode_back = from_unicode_math(&unicode).unwrap().to_mathml().unwrap();
        for output in [omml_back, mtef5_back, mtef3_back, unicode_back] {
            let long = output.contains(">LONG</")
                || ["L", "O", "N", "G"]
                    .into_iter()
                    .all(|part| output.contains(&format!(">{part}</")));
            assert!(
                output.contains(">a</") && long && output.contains(">z</"),
                "{output}"
            );
            assert!(!output.contains("columnspan"));
        }

        let standalone = MathExpr::inline(vec![MathNode::SpannedCell {
            alignment: ColumnAlign::Right,
            span: 2,
            left_boundaries: vec![SpanBoundary::Rule],
            right_boundaries: vec![SpanBoundary::SuppressGlue],
            content: Box::new(MathNode::Identifier("x".into())),
        }]);
        let mixed = MathExpr::inline(vec![MathNode::Row(vec![
            MathNode::SpannedCell {
                alignment: ColumnAlign::Right,
                span: 2,
                left_boundaries: Vec::new(),
                right_boundaries: Vec::new(),
                content: Box::new(MathNode::Identifier("x".into())),
            },
            MathNode::Identifier("y".into()),
        ])]);
        for value in [standalone, mixed] {
            let compact = CompactExpr::from_math_expr(&value).unwrap();
            assert!(
                OmmlWriter::write_compact_with_diagnostics(&compact, &config)
                    .unwrap()
                    .1
                    .is_empty()
            );
            assert!(
                MtefWriter::write_compact_with_diagnostics(&compact, &config)
                    .unwrap()
                    .1
                    .is_empty()
            );
            assert!(
                UnicodeMathWriter::write_compact_with_diagnostics(&compact, &config)
                    .unwrap()
                    .1
                    .is_empty()
            );
        }

        let control = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Left, ColumnAlign::Left],
            rows: vec![vec![
                vec![MathNode::Identifier("a".into())],
                vec![MathNode::Identifier("b".into())],
            ]],
            arraystretch: None,
            add_jot: false,
        }]);
        let compact = CompactExpr::from_math_expr(&control).unwrap();
        assert!(
            OmmlWriter::write_compact_with_diagnostics(&compact, &config)
                .unwrap()
                .1
                .is_empty()
        );
        assert!(
            MtefWriter::write_compact_with_diagnostics(&compact, &config)
                .unwrap()
                .1
                .is_empty()
        );
        assert!(
            UnicodeMathWriter::write_compact_with_diagnostics(&compact, &config)
                .unwrap()
                .1
                .iter()
                .any(|diagnostic| diagnostic.message.contains("centers table columns"))
        );
    }

    #[test]
    fn mtef_preserves_uniform_cell_rules_for_v3_and_v5() {
        let source =
            r"\begin{array}{ll}\multicolumn{1}{|l|}{a}&b\\\multicolumn{1}{|l|}{c}&d\end{array}";
        let result = from_latex(source).unwrap();
        for version in [
            crate::reader::mtef::MtefVersion::V5,
            crate::reader::mtef::MtefVersion::V3,
        ] {
            let (bytes, diagnostics) = result
                .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(version))
                .unwrap();
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let back = from_mtef_bytes(&bytes).unwrap();
            let latex = back.to_latex().unwrap();
            assert!(latex.contains("{|l|l}"), "{latex}");
            assert!(
                latex.contains("a")
                    && latex.contains("b")
                    && latex.contains("c")
                    && latex.contains("d")
            );
            assert!(back.diagnostics().is_empty(), "{:?}", back.diagnostics());
        }
    }

    #[test]
    fn mtef_reports_nonuniform_double_and_trimmed_cell_rules() {
        for (source, expected) in [
            (
                r"\begin{array}{ll}\multicolumn{1}{|l|}{a}&b\\c&d\end{array}",
                "row-varying cell rules",
            ),
            (
                r"\begin{array}{ll}\multicolumn{1}{||l||}{a}&b\\\multicolumn{1}{||l||}{c}&d\end{array}",
                "repeated cell rules",
            ),
            (
                r"\begin{array}{ll}\multicolumn{1}{@{}l@{}}{a}&b\\\multicolumn{1}{@{}l@{}}{c}&d\end{array}",
                "glue suppression",
            ),
        ] {
            let result = from_latex(source).unwrap();
            let (_, diagnostics) = result.to_mtef_with_diagnostics().unwrap();
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains(expected)),
                "{source}: {diagnostics:?}"
            );
        }

        let nested = from_latex(
            r"\begin{array}{l}\begin{array}{l}\multicolumn{1}{|l|}{a}\\\multicolumn{1}{|l|}{b}\end{array}\end{array}",
        ).unwrap();
        for version in [crate::MtefVersion::V3, crate::MtefVersion::V5] {
            let (bytes, diagnostics) = nested
                .to_mtef_bytes_with_diagnostics(&MtefWriteOptions::new(version))
                .unwrap();
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.message.contains("boundary material")),
                "{diagnostics:?}"
            );
            let tex = from_mtef_bytes(&bytes).unwrap().to_latex().unwrap();
            assert!(tex.contains(r"\begin{array}{|l|}"), "{tex}");
        }
    }

    #[test]
    fn authored_cell_alignment_reports_loss_in_other_table_formats() {
        let source = r#"<math><mtable columnalign="left left"><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr columnalign="right center"><mtd><mi>LONG</mi></mtd><mtd><mi>c</mi></mtd></mtr></mtable></math>"#;
        let result = from_mathml(source).unwrap();

        let (_, omml) = result.to_omml_with_diagnostics().unwrap();
        assert!(
            omml.iter()
                .any(|diagnostic| diagnostic.message.contains("cell alignment"))
        );

        let (_, mtef) = result.to_mtef_with_diagnostics().unwrap();
        assert!(mtef.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("MTEF exports one matrix alignment")
        }));

        let (_, unicode_math) = result.to_unicode_math_with_diagnostics().unwrap();
        assert!(
            unicode_math
                .iter()
                .any(|diagnostic| diagnostic.message.contains("table columns"))
        );

        let uniform = from_mathml(
            r#"<math><mtable columnalign="left"><mtr><mtd><mi>a</mi></mtd></mtr></mtable></math>"#,
        )
        .unwrap();
        assert!(uniform.to_omml_with_diagnostics().unwrap().1.is_empty());
        assert!(uniform.to_mtef_with_diagnostics().unwrap().1.is_empty());
        assert!(
            uniform
                .to_unicode_math_with_diagnostics()
                .unwrap()
                .1
                .iter()
                .any(|diagnostic| diagnostic.message.contains("centers table columns"))
        );
        let unicode_back = from_unicode_math(&uniform.to_unicode_math().unwrap())
            .unwrap()
            .to_mathml()
            .unwrap();
        assert!(unicode_back.contains(r#"columnalign="center""#));
    }

    #[test]
    fn alignment_diagnostics_require_a_whole_cell_and_actual_loss() {
        use crate::ast::{ColumnAlign, MathExpr, MathNode};

        let standalone = MathExpr::inline(vec![MathNode::AlignedCell {
            alignment: ColumnAlign::Right,
            content: Box::new(MathNode::Identifier("x".into())),
        }]);
        let mixed = MathExpr::inline(vec![MathNode::Row(vec![
            MathNode::AlignedCell {
                alignment: ColumnAlign::Right,
                content: Box::new(MathNode::Identifier("x".into())),
            },
            MathNode::Identifier("y".into()),
        ])]);
        for expr in [standalone, mixed] {
            let compact = CompactExpr::from_math_expr(&expr).unwrap();
            assert!(
                OmmlWriter::write_compact_with_diagnostics(&compact, &Config::default())
                    .unwrap()
                    .1
                    .is_empty()
            );
            assert!(
                MtefWriter::write_compact_with_diagnostics(&compact, &Config::default())
                    .unwrap()
                    .1
                    .is_empty()
            );
            assert!(
                UnicodeMathWriter::write_compact_with_diagnostics(&compact, &Config::default())
                    .unwrap()
                    .1
                    .is_empty()
            );
        }

        let omml_redundant = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Right],
            rows: vec![vec![vec![MathNode::AlignedCell {
                alignment: ColumnAlign::Right,
                content: Box::new(MathNode::Identifier("x".into())),
            }]]],
            arraystretch: None,
            add_jot: false,
        }]);
        let unicode_redundant = MathExpr::inline(vec![MathNode::Array {
            columns: vec![ColumnAlign::Center],
            rows: vec![vec![vec![MathNode::AlignedCell {
                alignment: ColumnAlign::Center,
                content: Box::new(MathNode::Identifier("x".into())),
            }]]],
            arraystretch: None,
            add_jot: false,
        }]);
        let compact = CompactExpr::from_math_expr(&omml_redundant).unwrap();
        assert!(
            OmmlWriter::write_compact_with_diagnostics(&compact, &Config::default())
                .unwrap()
                .1
                .is_empty()
        );
        assert!(
            MtefWriter::write_compact_with_diagnostics(&compact, &Config::default())
                .unwrap()
                .1
                .is_empty()
        );
        let compact = CompactExpr::from_math_expr(&unicode_redundant).unwrap();
        assert!(
            UnicodeMathWriter::write_compact_with_diagnostics(&compact, &Config::default())
                .unwrap()
                .1
                .is_empty()
        );
    }

    #[test]
    fn nested_authored_cell_content_survives_lossy_table_formats() {
        let source = r#"<math><mtable columnalign="left"><mtr><mtd><mi>a</mi></mtd></mtr><mtr><mtd columnalign="right"><mtable><mtr><mtd><mi>x</mi></mtd><mtd><mi>y</mi></mtd></mtr></mtable></mtd></mtr></mtable></math>"#;
        let result = from_mathml(source).unwrap();

        let omml = result.to_omml().unwrap();
        let omml_back = from_omml(&omml).unwrap().to_mathml().unwrap();
        assert!(omml_back.contains(">x<") && omml_back.contains(">y<"));

        let mtef = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
        let mtef_back = from_mtef_bytes(&mtef).unwrap().to_mathml().unwrap();
        assert!(mtef_back.contains(">x<") && mtef_back.contains(">y<"));

        let unicode = result.to_unicode_math().unwrap();
        let unicode_back = from_unicode_math(&unicode).unwrap().to_mathml().unwrap();
        assert!(unicode_back.contains(">x<") && unicode_back.contains(">y<"));
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
