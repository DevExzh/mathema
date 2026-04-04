//! WASM bindings for mathema.
//!
//! Provides both free functions for simple conversions and a
//! `MathemaConverter` builder for parse-once/write-many workflows.

pub mod converter;
pub mod serde;
mod types;

use wasm_bindgen::prelude::*;

use crate::reader::Reader;
use crate::reader::latex::LatexReader;
use crate::writer::Writer;
use crate::writer::latex::LatexWriter;
use crate::writer::mathml::MathmlWriter;
use crate::writer::omml::OmmlWriter;
use crate::writer::unicode_math::UnicodeMathWriter;
use crate::{Config, Format, convert as do_convert};

/// Initialize the WASM module. Call once before any other function.
/// Sets up the panic hook so Rust panics produce readable JS console errors.
#[wasm_bindgen(js_name = "init")]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Convert any `Display` error into a `JsValue` string.
pub(crate) fn to_js_err(e: impl std::fmt::Display) -> JsValue {
    js_string(&e.to_string())
}

/// Create a `JsValue` string that works on both wasm32 and native targets.
///
/// On native targets `JsValue::from_str` panics because the wasm-bindgen
/// runtime is unavailable.  For test builds we fall back to `JsValue::NULL`
/// so error-path tests can exercise `Result::is_err()` without panicking.
#[cfg(target_family = "wasm")]
pub(crate) fn js_string(s: &str) -> JsValue {
    JsValue::from_str(s)
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn js_string(_s: &str) -> JsValue {
    // Return a sentinel value; the string content is lost, but callers only
    // need a non-Ok JsValue when running on native (test) targets.
    JsValue::NULL
}

pub(crate) fn parse_format(s: &str) -> Result<Format, JsValue> {
    match s.to_lowercase().as_str() {
        "latex" | "tex" => Ok(Format::LaTeX),
        "mathml" | "mml" => Ok(Format::MathML),
        "omml" => Ok(Format::Omml),
        "mtef" => Ok(Format::Mtef),
        "unicodemath" | "unicode-math" | "um" => Ok(Format::UnicodeMath),
        _ => Err(js_string(&format!("unknown format: {s}"))),
    }
}

/// Convert a math expression between formats.
///
/// Formats: "latex", "mathml", "omml", "mtef", "unicodemath"
#[wasm_bindgen]
pub fn convert(input: &str, from: &str, to: &str) -> Result<String, JsValue> {
    let from_fmt = parse_format(from)?;
    let to_fmt = parse_format(to)?;
    do_convert(input, from_fmt, to_fmt).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return MathML.
#[wasm_bindgen]
pub fn latex_to_mathml(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut compact = LatexReader::parse_compact(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_compact_expr(&mut compact.expr);
    MathmlWriter::write_compact(&compact.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return OMML.
#[wasm_bindgen]
pub fn latex_to_omml(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut compact = LatexReader::parse_compact(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_compact_expr(&mut compact.expr);
    OmmlWriter::write_compact(&compact.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize the AST, and return UnicodeMath.
#[wasm_bindgen]
pub fn latex_to_unicode_math(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut compact = LatexReader::parse_compact(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_compact_expr(&mut compact.expr);
    UnicodeMathWriter::write_compact(&compact.expr, &config).map_err(to_js_err)
}

/// Parse LaTeX, normalize, and return the re-serialized (normalized) LaTeX.
#[wasm_bindgen]
pub fn normalize_latex(input: &str) -> Result<String, JsValue> {
    let config = Config::default();
    let mut compact = LatexReader::parse_compact(input, &config).map_err(to_js_err)?;
    crate::ast::normalize::normalize_compact_expr(&mut compact.expr);
    LatexWriter::write_compact(&compact.expr, &config).map_err(to_js_err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // If this test runs, the module structure is correct.
    }
}
