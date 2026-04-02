#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

#[cfg(feature = "wasm")]
use crate::reader::Reader;
#[cfg(feature = "wasm")]
use crate::reader::latex::LatexReader;
#[cfg(feature = "wasm")]
use crate::writer::Writer;
#[cfg(feature = "wasm")]
use crate::writer::latex::LatexWriter;
#[cfg(feature = "wasm")]
use crate::writer::mathml::MathmlWriter;
#[cfg(feature = "wasm")]
use crate::writer::omml::OmmlWriter;
#[cfg(feature = "wasm")]
use crate::writer::unicode_math::UnicodeMathWriter;
#[cfg(feature = "wasm")]
use crate::{Config, Format, convert as do_convert};

/// Convert any `Display` error into a `JsValue` string.
#[cfg(feature = "wasm")]
fn to_js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[cfg(feature = "wasm")]
fn parse_format(s: &str) -> Result<Format, JsValue> {
    match s.to_lowercase().as_str() {
        "latex" | "tex" => Ok(Format::LaTeX),
        "mathml" | "mml" => Ok(Format::MathML),
        "omml" => Ok(Format::Omml),
        "mtef" => Ok(Format::Mtef),
        "unicodemath" | "unicode-math" | "um" => Ok(Format::UnicodeMath),
        _ => Err(JsValue::from_str(&format!("unknown format: {s}"))),
    }
}

/// Convert a math expression between formats.
///
/// Formats: "latex", "mathml", "omml", "mtef", "unicodemath"
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn convert(input: &str, from: &str, to: &str) -> Result<String, JsValue> {
    let from_fmt = parse_format(from)?;
    let to_fmt = parse_format(to)?;
    do_convert(input, from_fmt, to_fmt).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return MathML.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn latex_to_mathml(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut parsed = LatexReader::parse(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_expr(&mut parsed.expr);
    MathmlWriter::write(&parsed.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return OMML.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn latex_to_omml(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut parsed = LatexReader::parse(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_expr(&mut parsed.expr);
    OmmlWriter::write(&parsed.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return UnicodeMath.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn latex_to_unicode_math(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut parsed = LatexReader::parse(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_expr(&mut parsed.expr);
    UnicodeMathWriter::write(&parsed.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize, and return the re-serialized (normalized) LaTeX.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn normalize_latex(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut parsed = LatexReader::parse(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_expr(&mut parsed.expr);
    LatexWriter::write(&parsed.expr, &config).map_err(to_js_err)
}
