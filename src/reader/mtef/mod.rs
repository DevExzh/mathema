//! MTEF reader: MathType's binary equation format, versions 3 (Equation
//! Editor 3.x) and 5 (MathType 4 and later), as specified in `MTEFv3.md` and
//! `MTEFv5.md`.
//!
//! Input is either bare MTEF data or an OLE `Equation Native` stream, which
//! starts with a 28-byte `EQNOLEFILEHDR`. The [`Reader`] implementation takes
//! the bytes hex-encoded; [`crate::from_mtef_bytes`] takes them as they are.
//!
//! # What the AST keeps
//!
//! Structure is mapped faithfully: lines, piles and matrices, fractions,
//! roots, scripts and leading scripts, fences (including mixed intervals),
//! big operators with their limits, limits under/over a base, horizontal
//! braces, arrows with labels, embellishments (accents, primes, negation
//! slashes), function names as one operator and text runs as one text node.
//!
//! The AST has no place for MathType's typesetting data, which is read and
//! dropped without a diagnostic: nudges, type sizes, explicit fonts (only
//! their bold/italic style is kept), line spacing, tab stops (rulers), the
//! equation preferences, template options (fence alignment, expanding
//! integrals) and partition line styles (dashed and dotted rules become solid
//! ones). Colours are dropped with an informational diagnostic. Operator and
//! decimal-point column alignment become left and right alignment.
//!
//! # Strictness
//!
//! In [`Strictness::Strict`] mode the reader refuses anything it cannot decode
//! exactly (an unknown record, template or embellishment, a malformed or
//! truncated record, data after the final END). In [`Strictness::Lenient`]
//! mode it reports such problems as warnings and keeps going where the format
//! allows; future-expansion records (type 100 and up) are skipped by their
//! length in both modes. A stream that cannot be decoded at all yields an
//! empty expression and a warning in Lenient mode.

mod chars;
mod decode;
mod info;
mod templates;
#[cfg(test)]
mod tests;

pub use info::{MtefInfo, MtefPlatform, MtefProduct, MtefVersion, OleHeader};

use crate::ast::DisplayStyle;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, NodeSpan};
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader;
use decode::Decoder;

/// Reader for MTEF equations.
pub struct MtefReader;

impl Reader for MtefReader {
    /// Parse hex-encoded MTEF data (whitespace between digits is ignored).
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        Self::parse_hex_with_info(input, config).map(|(result, _)| result)
    }
}

impl MtefReader {
    /// Parse raw MTEF bytes (bare MTEF or an `Equation Native` stream).
    ///
    /// This is the reader-level entry point; it does not normalize the AST.
    /// Prefer [`crate::from_mtef_bytes`].
    pub fn parse_bytes(bytes: &[u8], config: &Config) -> Result<ParseResult> {
        Self::parse_bytes_compact(bytes, config).map(|r| r.to_parse_result())
    }

    /// Parse raw MTEF bytes into the compact representation.
    pub fn parse_bytes_compact(bytes: &[u8], config: &Config) -> Result<CompactParseResult> {
        Self::parse_bytes_with_info(bytes, config).map(|(result, _)| result)
    }

    /// Parse hex-encoded MTEF data, also returning its header information.
    pub(crate) fn parse_hex_with_info(
        input: &str,
        config: &Config,
    ) -> Result<(CompactParseResult, Option<MtefInfo>)> {
        config.check_input_size(input)?;
        match hex_decode(input) {
            Ok(bytes) => Self::parse_bytes_with_info(&bytes, config),
            Err(e) if config.strictness == Strictness::Lenient => Ok((
                empty_result(
                    DisplayStyle::Inline,
                    vec![warning(format!("MTEF hex decode failed (recovered): {e}"))],
                ),
                None,
            )),
            Err(e) => Err(e),
        }
    }

    /// Parse raw MTEF bytes, also returning their header information.
    pub(crate) fn parse_bytes_with_info(
        bytes: &[u8],
        config: &Config,
    ) -> Result<(CompactParseResult, Option<MtefInfo>)> {
        config.check_input_len(bytes.len())?;
        if bytes.is_empty() {
            return Ok((empty_result(DisplayStyle::Inline, Vec::new()), None));
        }
        let mut decoder = Decoder::new(bytes, config);
        let outcome = decoder.run();
        let info = decoder.info.take();
        let display = match info.as_ref().and_then(|i| i.inline) {
            Some(true) => DisplayStyle::Inline,
            Some(false) => DisplayStyle::Block,
            None => config.display,
        };
        match outcome {
            Ok(roots) => {
                let mut arena = decoder.arena;
                let roots = arena.try_push_children(&roots)?;
                let expr = CompactExpr {
                    arena,
                    roots,
                    display,
                };
                Ok((
                    CompactParseResult::with_diagnostics(expr, decoder.diagnostics),
                    info,
                ))
            },
            // Resource limits are enforced in every mode.
            Err(Error::MtefParse { message }) if config.strictness == Strictness::Lenient => {
                let mut diagnostics = decoder.diagnostics;
                diagnostics.push(warning(format!(
                    "MTEF data could not be decoded; nothing was kept: {message}"
                )));
                Ok((empty_result(display, diagnostics), info))
            },
            Err(e) => Err(e),
        }
    }
}

fn warning(message: String) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Warning,
        message,
        span: None,
    }
}

fn empty_result(display: DisplayStyle, diagnostics: Vec<Diagnostic>) -> CompactParseResult {
    CompactParseResult::with_diagnostics(
        CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, display),
        diagnostics,
    )
}

/// Decode a hex string to bytes, skipping whitespace.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return Err(Error::MtefParse {
            message: "MTEF hex string has odd length".into(),
        });
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_chunks::<2>().0 {
        out.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Ok(out)
}

fn hex_value(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::MtefParse {
            message: format!("invalid hex character {:?} in MTEF input", b as char),
        }),
    }
}
