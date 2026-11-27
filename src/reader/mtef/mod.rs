//! MTEF reader: MathType's binary equation format, versions 3 (Equation
//! Editor 3.x) and 5 (MathType 4 and later), as specified in `MTEFv3.md` and
//! `MTEFv5.md`, and versions 2 (Equation Editor 1.x and 2.x, MathType 3.0)
//! and 4 (MathType 3.5), which are MTEF 3 with 8-bit characters (`docs/mtef-legacy.md`).
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
//! The AST has no place for MathType's typesetting data. Nudge offsets,
//! nonzero line spacing, and nonempty tab stops (rulers) are read and dropped
//! with one informational diagnostic per category. Explicit fonts retain their
//! bold/italic and math-alphabet effects; equation preferences are used for
//! sizes where possible. Template options without an AST equivalent (fence
//! alignment, expanding integrals) are dropped with an informational
//! diagnostic. Dotted partition lines become dashed ones. Operator and
//! decimal-point column alignment become left and right alignment.
//!
//! Font sizes become attributes ([`crate::ast::attrs`]) where they are not the
//! size a character has by its place in the equation: explicit point sizes,
//! sizes made larger or smaller, and typesizes the user chose.
//!
//! # Conventions
//!
//! Colours become colour nodes. Every variable-style letter is an identifier
//! of its own. Outside the text styles the ASCII hyphen, tilde, apostrophe
//! and backslash are read as `−`, `∼`, a prime and `∖`, a point between
//! digits is part of the number, and empty script and limit lines, which
//! draw nothing, are dropped. A leading-script template (`tvSU_PRECEDES`)
//! before an operator, punctuation, a fence or the end of its line cannot
//! lead anything there; it becomes a script of the item before it, where it
//! is drawn.
//!
//! # Strictness
//!
//! In [`Strictness::Strict`] mode the reader refuses anything it cannot decode
//! exactly (an unknown record, template or embellishment, a malformed or
//! truncated record, data after the final END). The short slack real writers
//! leave between the final END and the size an `Equation Native` header
//! declares is ignored with an informational diagnostic in both modes. In
//! [`Strictness::Lenient`] mode it reports such problems as warnings and
//! keeps going where the format allows; future-expansion records (type 100
//! and up) are skipped by their length in both modes. A stream that cannot be
//! decoded at all yields an empty expression and a warning in Lenient mode.

/// MTEF MATRIX has at most 255 cells on each axis and one more boundary.
#[derive(Clone, Copy, Default)]
pub(crate) struct PartitionMask([u64; 4]);

impl PartitionMask {
    pub(crate) fn from_low(mask: u32) -> Self {
        Self([u64::from(mask), 0, 0, 0])
    }
    pub(crate) fn low(self) -> u32 {
        self.0[0] as u32
    }
    pub(crate) fn get(self, boundary: usize) -> bool {
        self.0[boundary / 64] & (1 << (boundary % 64)) != 0
    }
    pub(crate) fn set(&mut self, boundary: usize, active: bool) {
        let bit = 1 << (boundary % 64);
        let word = &mut self.0[boundary / 64];
        if active {
            *word |= bit;
        } else {
            *word &= !bit;
        }
    }
    pub(crate) fn any(self) -> bool {
        self.0 != [0; 4]
    }
    pub(crate) fn extended_count(self) -> u32 {
        self.0.iter().map(|v| v.count_ones()).sum::<u32>() - self.low().count_ones()
    }
    pub(crate) fn intersection(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] & other.0[i]))
    }
    pub(crate) fn union(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] | other.0[i]))
    }
    pub(crate) fn differing(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] ^ other.0[i]))
    }
}

pub(crate) mod chars;
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
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, Format, LocatedError,
    ParseResult, Result, Strictness,
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

    /// Parse raw bytes with a best-effort wire offset on malformed input.
    pub fn parse_bytes_with_location(
        bytes: &[u8],
        config: &Config,
    ) -> std::result::Result<ParseResult, LocatedError> {
        Self::parse_bytes_with_info_located(bytes, config)
            .map(|(result, _)| result.to_parse_result())
    }

    /// Parse hex-encoded MTEF data, also returning its header information.
    pub(crate) fn parse_hex_with_info(
        input: &str,
        config: &Config,
    ) -> Result<(CompactParseResult, Option<MtefInfo>)> {
        Self::parse_hex_with_info_located(input, config).map_err(LocatedError::into_error)
    }

    pub(crate) fn parse_hex_with_info_located(
        input: &str,
        config: &Config,
    ) -> std::result::Result<(CompactParseResult, Option<MtefInfo>), LocatedError> {
        config.check_input_size(input)?;
        match hex_decode(input) {
            Ok(bytes) => Self::parse_bytes_with_info_located(&bytes, config),
            Err(e)
                if config.strictness == Strictness::Lenient
                    && !matches!(&e, Error::RequestCancelled | Error::LimitExceeded { .. }) =>
            {
                Ok((
                    empty_result(
                        DisplayStyle::Inline,
                        vec![warning(format!("MTEF hex decode failed (recovered): {e}"))],
                    ),
                    None,
                ))
            },
            Err(e) => Err(LocatedError::with_span(e, Some(Format::Mtef), None)),
        }
    }

    /// Parse raw MTEF bytes, also returning their header information.
    pub(crate) fn parse_bytes_with_info(
        bytes: &[u8],
        config: &Config,
    ) -> Result<(CompactParseResult, Option<MtefInfo>)> {
        Self::parse_bytes_with_info_located(bytes, config).map_err(LocatedError::into_error)
    }

    pub(crate) fn parse_bytes_with_info_located(
        bytes: &[u8],
        config: &Config,
    ) -> std::result::Result<(CompactParseResult, Option<MtefInfo>), LocatedError> {
        config.check_input_len(bytes.len())?;
        if bytes.is_empty() {
            return Ok((empty_result(DisplayStyle::Inline, Vec::new()), None));
        }
        let mut decoder = Decoder::new(bytes, config);
        let mut outcome = decoder.run();
        if outcome.is_ok() && decoder.decodes_again() {
            decoder = Decoder::new(bytes, config);
            decoder.keep_full_sizes();
            outcome = decoder.run();
        }
        let info = decoder.info.take();
        let display = match info.as_ref().and_then(|i| i.inline) {
            Some(true) => DisplayStyle::Inline,
            Some(false) => DisplayStyle::Block,
            None => config.display,
        };
        match outcome {
            Ok(roots) => {
                decoder.settle_leading_scripts()?;
                let full_size = decoder
                    .arena
                    .has_attrs()
                    .then(|| decoder.full_size())
                    .flatten();
                let mut arena = decoder.arena;
                let roots = arena.push_children(&roots)?;
                arena.check_limits()?;
                let mut expr = CompactExpr::new(arena, roots, display);
                expr.drop_unreachable_attrs();
                // The preferences matter only to sizes seen against them.
                let base = full_size.filter(|_| expr.arena.has_attrs());
                expr.set_base_size(base);
                Ok((
                    CompactParseResult::with_diagnostic_metadata(
                        expr,
                        decoder.diagnostics,
                        decoder.diagnostic_metadata,
                    ),
                    info,
                ))
            },
            // Resource limits are enforced in every mode.
            Err(Error::MtefParse { message }) if config.strictness == Strictness::Lenient => {
                let span = decoder
                    .last_error_offset
                    .map(|offset| decoder.wire_span(offset));
                let mut diagnostics = decoder.diagnostics;
                diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::Warning,
                    message: format!("MTEF data could not be decoded; nothing was kept: {message}"),
                    span,
                });
                Ok((
                    CompactParseResult::with_diagnostic_metadata(
                        CompactExpr::new(AstArena::new(), NodeSpan::EMPTY, display),
                        diagnostics,
                        decoder.diagnostic_metadata,
                    ),
                    info,
                ))
            },
            Err(e) => Err(LocatedError::with_span(
                e,
                Some(Format::Mtef),
                decoder
                    .last_error_offset
                    .map(|offset| decoder.wire_span(offset)),
            )),
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

const HEX_WORK_CHUNK: usize = 4096;

/// Charge one unit after each chunk of the original hex spelling. Keeping the
/// counter in bytes, rather than decoded digits, makes runs of whitespace
/// cancellable as well.
fn checkpoint_hex_chunk(remaining: &mut usize) -> Result<()> {
    if *remaining == 0 {
        crate::error::request_checkpoint(1)?;
        *remaining = HEX_WORK_CHUNK;
    }
    *remaining -= 1;
    Ok(())
}

/// Decode a hex string to bytes, skipping whitespace.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    // Count first so an odd-length input keeps the historical error precedence
    // over invalid hex characters. This pass also charges whitespace and
    // permits a work-limited request to stop before any output allocation.
    let mut digits_len = 0usize;
    let mut remaining = 0;
    for byte in s.bytes() {
        checkpoint_hex_chunk(&mut remaining)?;
        if !byte.is_ascii_whitespace() {
            digits_len += 1;
        }
    }
    if !digits_len.is_multiple_of(2) {
        return Err(Error::MtefParse {
            message: "MTEF hex string has odd length".into(),
        });
    }

    // Decode directly into the result instead of retaining a second full-size
    // digit buffer. Capacity grows only as checked input is consumed.
    let mut out = Vec::new();
    let mut high = None;
    let mut remaining = 0;
    for byte in s.bytes() {
        checkpoint_hex_chunk(&mut remaining)?;
        if byte.is_ascii_whitespace() {
            continue;
        }
        let value = hex_value(byte)?;
        if let Some(high) = high.take() {
            out.push((high << 4) | value);
        } else {
            high = Some(value);
        }
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
