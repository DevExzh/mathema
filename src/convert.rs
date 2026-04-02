use crate::ast::MathExpr;
use crate::error::{Config, Format, ParseResult, Result};
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
    let mut parsed = parse(input, from, config)?;
    crate::ast::normalize::normalize_expr(&mut parsed.expr);
    write(&parsed.expr, to, config)
}

fn parse(input: &str, format: Format, config: &Config) -> Result<ParseResult> {
    match format {
        Format::LaTeX => LatexReader::parse(input, config),
        Format::MathML => MathmlReader::parse(input, config),
        Format::Omml => OmmlReader::parse(input, config),
        Format::Mtef => MtefReader::parse(input, config),
        Format::UnicodeMath => UnicodeMathReader::parse(input, config),
    }
}

fn write(expr: &MathExpr, format: Format, config: &Config) -> Result<String> {
    match format {
        Format::LaTeX => LatexWriter::write(expr, config),
        Format::MathML => MathmlWriter::write(expr, config),
        Format::Omml => OmmlWriter::write(expr, config),
        Format::Mtef => MtefWriter::write(expr, config),
        Format::UnicodeMath => UnicodeMathWriter::write(expr, config),
    }
}

/// Builder-style conversion result. Parse once, write to multiple formats.
pub struct ConvertResult {
    pub parsed: ParseResult,
    config: Config,
}

impl ConvertResult {
    pub fn to_latex(&self) -> Result<String> {
        LatexWriter::write(&self.parsed.expr, &self.config)
    }

    pub fn to_mathml(&self) -> Result<String> {
        MathmlWriter::write(&self.parsed.expr, &self.config)
    }

    pub fn to_latex_with(&self, config: &Config) -> Result<String> {
        LatexWriter::write(&self.parsed.expr, config)
    }

    pub fn to_mathml_with(&self, config: &Config) -> Result<String> {
        MathmlWriter::write(&self.parsed.expr, config)
    }

    pub fn to_omml(&self) -> Result<String> {
        OmmlWriter::write(&self.parsed.expr, &self.config)
    }

    pub fn to_omml_with(&self, config: &Config) -> Result<String> {
        OmmlWriter::write(&self.parsed.expr, config)
    }

    pub fn to_mtef(&self) -> Result<String> {
        MtefWriter::write(&self.parsed.expr, &self.config)
    }

    pub fn to_mtef_with(&self, config: &Config) -> Result<String> {
        MtefWriter::write(&self.parsed.expr, config)
    }

    pub fn to_unicode_math(&self) -> Result<String> {
        UnicodeMathWriter::write(&self.parsed.expr, &self.config)
    }

    pub fn to_unicode_math_with(&self, config: &Config) -> Result<String> {
        UnicodeMathWriter::write(&self.parsed.expr, config)
    }

    /// Access the parsed AST for inspection or transformation.
    pub fn expr(&self) -> &MathExpr {
        &self.parsed.expr
    }

    /// Take ownership of the parsed AST.
    pub fn into_expr(self) -> MathExpr {
        self.parsed.expr
    }
}

/// Parse LaTeX input into a reusable conversion result.
pub fn from_latex(input: &str) -> Result<ConvertResult> {
    from_latex_with(input, &Config::default())
}

/// Parse LaTeX with custom configuration.
pub fn from_latex_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let parsed = LatexReader::parse(input, config)?;
    Ok(ConvertResult {
        parsed,
        config: config.clone(),
    })
}

/// Parse MathML input into a reusable conversion result.
pub fn from_mathml(input: &str) -> Result<ConvertResult> {
    from_mathml_with(input, &Config::default())
}

/// Parse MathML with custom configuration.
pub fn from_mathml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let parsed = MathmlReader::parse(input, config)?;
    Ok(ConvertResult {
        parsed,
        config: config.clone(),
    })
}

/// Parse OMML input into a reusable conversion result.
pub fn from_omml(input: &str) -> Result<ConvertResult> {
    from_omml_with(input, &Config::default())
}

/// Parse OMML with custom configuration.
pub fn from_omml_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let parsed = OmmlReader::parse(input, config)?;
    Ok(ConvertResult {
        parsed,
        config: config.clone(),
    })
}

/// Parse MTEF input (hex-encoded binary) into a reusable conversion result.
pub fn from_mtef(input: &str) -> Result<ConvertResult> {
    from_mtef_with(input, &Config::default())
}

/// Parse MTEF with custom configuration.
pub fn from_mtef_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let parsed = MtefReader::parse(input, config)?;
    Ok(ConvertResult {
        parsed,
        config: config.clone(),
    })
}

/// Parse UnicodeMath input into a reusable conversion result.
pub fn from_unicode_math(input: &str) -> Result<ConvertResult> {
    from_unicode_math_with(input, &Config::default())
}

/// Parse UnicodeMath with custom configuration.
pub fn from_unicode_math_with(input: &str, config: &Config) -> Result<ConvertResult> {
    let parsed = UnicodeMathReader::parse(input, config)?;
    Ok(ConvertResult {
        parsed,
        config: config.clone(),
    })
}
