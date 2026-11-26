//! MTEF writer: MathType's binary equation format, versions 5 (MathType 4
//! and later, OLE ProgID `Equation.DSMT4`) and 3 (Equation Editor 3.x, ProgID
//! `Equation.3`), as specified in `MTEFv5.md` and `MTEFv3.md`.
//!
//! [`MtefWriter::write_bytes`] returns the binary data, optionally preceded
//! by the 28-byte `EQNOLEFILEHDR` of an OLE `Equation Native` stream; the
//! [`Writer`](crate::writer::Writer) implementation returns the same bytes
//! hex-encoded (MTEF 5, no OLE header).
//!
//! # What is written
//!
//! The stream has the layout the applications write: the header, then (MTEF
//! 5) the encoding and font definitions and equation preferences of
//! MathType's default settings, an initial size record, the equation's LINE
//! or PILE and the final END. Each AST node becomes the records the MTEF
//! reader turns back into that node; see the encoder for the mapping.
//! Fraktur letters and script capitals are written in MTEF 5 as MathType writes them,
//! in the Euclid Fraktur and Euclid Math One fonts with MTCode's private-use
//! values; MTEF 3 has no math alphabets.
//!
//! The AST holds no typesetting data, so none is written: no nudges, rulers,
//! line spacing, or explicit point sizes. Typesize records are
//! written where scripts, limits and operator signs change the size, as the
//! applications do. Nodes MTEF cannot represent (phantoms, rules,
//! scaling, raised boxes, style switches and math choices) are written as their content or
//! dropped, and each such loss is reported by
//! [`MtefWriter::write_bytes_with_diagnostics`]. MTEF 5 colours are written as
//! COLOR_DEF and COLOR records; MTEF 3 has none.

mod chars;
mod encode;
#[cfg(test)]
mod tests;

use crate::ast::compact::CompactExpr;
use crate::ast::{DisplayStyle, MathExpr, Rational};
use crate::error::{Config, Diagnostic, DiagnosticKind, Result};
use crate::reader::mtef::{MtefInfo, MtefProduct, MtefVersion};
use crate::writer::Writer as MathWriter;
use encode::Encoder;

/// Writer for MTEF equations.
pub struct MtefWriter;

/// How [`MtefWriter::write_bytes`] writes an equation: the MTEF version, the
/// product identity in the header, and whether to add the OLE header.
///
/// The default writes MTEF 5 as MathType 6 (`DSMT6`) does, which is what a
/// new `Equation.DSMT4` object holds. [`MtefWriteOptions::new`] with
/// [`MtefVersion::V3`] writes an Equation Editor 3.x (`Equation.3`)
/// equation. [`MtefWriteOptions::from_info`] keeps the version and product of
/// an equation that was read from MTEF, so that an updated OLE object keeps
/// its kind.
///
/// Only MTEF 3 and 5 are written. MTEF 2 and 4 are read only: options for
/// either write MTEF 5.
///
/// ```
/// use mathema::{MtefVersion, MtefWriteOptions};
///
/// let result = mathema::from_latex(r"\frac{1}{2}").unwrap();
/// let mtef5 = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
/// assert_eq!(&mtef5[..5], &[5, 1, 0, 6, 0]);
///
/// let options = MtefWriteOptions::new(MtefVersion::V3).with_ole_header(true);
/// let native = result.to_mtef_bytes(&options).unwrap();
/// assert_eq!(&native[..2], &[28, 0]);
/// assert_eq!(&native[28..33], &[3, 1, 1, 3, 10]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MtefWriteOptions {
    /// The MTEF version to write: [`MtefVersion::V3`] writes MTEF 3, any
    /// other version MTEF 5.
    pub version: MtefVersion,
    /// Whether to precede the MTEF data with the `EQNOLEFILEHDR` of an OLE
    /// `Equation Native` stream.
    pub ole_header: bool,
    /// The generating product recorded in the header.
    pub product: MtefProduct,
    /// The product's major version recorded in the header.
    pub product_version: u8,
    /// The product's minor version recorded in the header.
    pub product_subversion: u8,
    /// The MTEF 5 application key (ignored for MTEF 3).
    pub application_key: String,
}

impl Default for MtefWriteOptions {
    fn default() -> Self {
        Self::new(MtefVersion::V5)
    }
}

impl MtefWriteOptions {
    /// Options for `version` with the product identity the applications
    /// write: MathType 6.0 (`DSMT6`) for MTEF 5, Equation Editor 3.0 (header
    /// version 3, subversion 10) for MTEF 3. No OLE header. MTEF 2 and 4,
    /// which are not written, get the MTEF 5 options.
    #[must_use]
    pub fn new(version: MtefVersion) -> Self {
        match version {
            MtefVersion::V3 => Self {
                version,
                ole_header: false,
                product: MtefProduct::EquationEditor,
                product_version: 3,
                product_subversion: 10,
                application_key: String::new(),
            },
            _ => Self {
                version: MtefVersion::V5,
                ole_header: false,
                product: MtefProduct::MathType,
                product_version: 6,
                product_subversion: 0,
                application_key: "DSMT6".to_owned(),
            },
        }
    }

    /// Options that write an equation of the same kind as the one `info`
    /// describes: its MTEF version, product, product version and application
    /// key, and an OLE header when the input had one. The platform is always
    /// written as Windows, whose character encoding the writer uses.
    ///
    /// MTEF 2 and 4 are not written: an equation read from either is written
    /// as an Equation Editor 3.0 equation (MTEF 3) when Equation Editor
    /// generated it, else as a MathType 6 equation (MTEF 5), with the OLE
    /// header kept as above.
    ///
    /// ```
    /// use mathema::{MtefVersion, MtefWriteOptions};
    ///
    /// // An Equation Editor 3 equation holding the digit 2.
    /// let mtef = [3, 1, 1, 3, 0, 0x0A, 0x01, 0x02, 0x88, 0x32, 0x00, 0x00, 0x00];
    /// let result = mathema::from_mtef_bytes(&mtef).unwrap();
    /// let options = MtefWriteOptions::from_info(result.mtef_info().unwrap());
    /// assert_eq!(options.version, MtefVersion::V3);
    /// let rewritten = result.to_mtef_bytes(&options).unwrap();
    /// assert_eq!(&rewritten[..5], &[3, 1, 1, 3, 0]);
    /// ```
    #[must_use]
    pub fn from_info(info: &MtefInfo) -> Self {
        if matches!(info.version, MtefVersion::V2 | MtefVersion::V4) {
            let version = if info.product == MtefProduct::EquationEditor {
                MtefVersion::V3
            } else {
                MtefVersion::V5
            };
            return Self::new(version).with_ole_header(info.ole_header.is_some());
        }
        let defaults = Self::new(info.version);
        Self {
            version: defaults.version,
            ole_header: info.ole_header.is_some(),
            product: info.product,
            product_version: info.product_version,
            product_subversion: info.product_subversion,
            application_key: info
                .application_key
                .clone()
                .unwrap_or(defaults.application_key),
        }
    }

    /// Builder: set whether to add the OLE `EQNOLEFILEHDR`.
    #[must_use]
    pub fn with_ole_header(mut self, ole_header: bool) -> Self {
        self.ole_header = ole_header;
        self
    }

    fn v5(&self) -> bool {
        self.version != MtefVersion::V3
    }
}

/// Length of the `EQNOLEFILEHDR`.
const OLE_HEADER_LEN: usize = 28;
/// `EQNOLEFILEHDR` version: hiword 2, loword 0.
const OLE_HEADER_VERSION: u32 = 0x0002_0000;
/// The clipboard format field of `EQNOLEFILEHDR`. It holds the value of the
/// registered "MathType EF" clipboard format in the session that saved the
/// object, which differs between sessions; these are values found in real
/// Equation Editor 3 and MathType 6 objects.
const OLE_CLIPBOARD_FORMAT_V3: u16 = 0xC16D;
const OLE_CLIPBOARD_FORMAT_V5: u16 = 0xC295;

/// The fnTEXT_FE style definition: unused (0), or the Far East font plain.
fn far_east_style(far_east: bool, alphabets: bool) -> &'static [u8] {
    match (far_east, alphabets) {
        (false, _) => &[0],
        (true, false) => &[5, 0],
        (true, true) => &[7, 0],
    }
}

/// The definitions and preferences MathType 5 and 6 write in front of every
/// equation with their default settings ("Define Sizes", "Define Spacing",
/// "Define Styles"): the WinAllBasicCodePages encoding, the fonts Times New
/// Roman, Symbol, Courier New and MT Extra, and the EQN_PREFS record with
/// the default sizes (12 pt, 58 %, 42 %, 150 %, 100 %, 75 %, 150 %, 1 pt),
/// the 30 default spacing values and the default style-to-font table
/// (MTEFv5.md "ENCODING_DEF", "FONT_DEF", "EQN_PREFS", "Dimension arrays").
#[rustfmt::skip]
const V5_DEFINITIONS: &[u8] = &[
    // ENCODING_DEF 5: "WinAllBasicCodePages".
    0x13, b'W', b'i', b'n', b'A', b'l', b'l', b'B', b'a', b's', b'i', b'c',
    b'C', b'o', b'd', b'e', b'P', b'a', b'g', b'e', b's', 0,
    // FONT_DEF 1: encoding 5, "Times New Roman".
    0x11, 5, b'T', b'i', b'm', b'e', b's', b' ', b'N', b'e', b'w', b' ', b'R',
    b'o', b'm', b'a', b'n', 0,
    // FONT_DEF 2: encoding 3 (Symbol), "Symbol".
    0x11, 3, b'S', b'y', b'm', b'b', b'o', b'l', 0,
    // FONT_DEF 3: encoding 5, "Courier New".
    0x11, 5, b'C', b'o', b'u', b'r', b'i', b'e', b'r', b' ', b'N', b'e', b'w', 0,
    // FONT_DEF 4: encoding 4 (MTExtra), "MT Extra".
    0x11, 4, b'M', b'T', b' ', b'E', b'x', b't', b'r', b'a', 0,
];

/// The definitions MathType adds for Fraktur and script letters, as it
/// writes them: an encoding named after each font and the font itself
/// (ENCODING_DEF 6 and FONT_DEF 5 for Euclid Fraktur, ENCODING_DEF 7 and
/// FONT_DEF 6 for Euclid Math One). Both are defined when either is used, so
/// that their indices are known while the body is written.
#[rustfmt::skip]
const V5_ALPHABET_DEFINITIONS: &[u8] = &[
    0x13, b'E', b'u', b'c', b'l', b'i', b'd', b'F', b'r', b'a', b'k', b't',
    b'u', b'r', 0,
    0x11, 6, b'E', b'u', b'c', b'l', b'i', b'd', b' ', b'F', b'r', b'a', b'k',
    b't', b'u', b'r', 0,
    0x13, b'E', b'u', b'c', b'l', b'i', b'd', b'M', b'a', b't', b'h', b'1', 0,
    0x11, 7, b'E', b'u', b'c', b'l', b'i', b'd', b' ', b'M', b'a', b't', b'h',
    b' ', b'O', b'n', b'e', 0,
];

/// FONT_DEF indices of Euclid Fraktur and Euclid Math One in
/// [`V5_ALPHABET_DEFINITIONS`].
pub(super) const ALPHABET_FONT_DEFS: [u8; 2] = [5, 6];

/// The definitions MathType adds when an equation holds Far East text: the
/// WinAllCodePages encoding (ENCODING_DEF 6) and a Far East font (FONT_DEF
/// 5). MathType writes the font's name in the system code page (宋体 on a
/// Chinese system); the writer uses its code-page-independent name.
#[rustfmt::skip]
const V5_FAR_EAST_DEFINITIONS: &[u8] = &[
    0x13, b'W', b'i', b'n', b'A', b'l', b'l', b'C', b'o', b'd', b'e', b'P',
    b'a', b'g', b'e', b's', 0,
    0x11, 6, b'S', b'i', b'm', b'S', b'u', b'n', 0,
];

/// Index of the FONT_DEF's encoding in [`V5_FAR_EAST_DEFINITIONS`], which
/// moves past the alphabet encodings when those are defined first.
const FAR_EAST_ENCODING_AT: usize = 18;

#[rustfmt::skip]
const V5_PREFERENCES: &[u8] = &[
    // EQN_PREFS, options 0.
    0x12, 0x00,
    // Sizes: 8 dimensions.
    0x08, 0x21, 0x2f, 0x45, 0x8f, 0x44, 0x2f, 0x41, 0x50, 0xf4, 0x10, 0x0f,
    0x47, 0x5f, 0x41, 0x50, 0xf2, 0x1f,
    // Spacing: 30 dimensions.
    0x1e, 0x41, 0x50, 0xf4, 0x15, 0x0f, 0x41, 0x00, 0xf4, 0x45, 0xf4, 0x25,
    0xf4, 0x8f, 0x42, 0x5f, 0x41, 0x00, 0xf4, 0x10, 0x0f, 0x43, 0x5f, 0x41,
    0x00, 0xf4, 0x8f, 0x45, 0xf4, 0x2a, 0x5f, 0x48, 0xf4, 0x8f, 0x41, 0x00,
    0xf4, 0x10, 0x0f, 0x40, 0xf4, 0x8f, 0x41, 0x7f, 0x48, 0xf4, 0x10, 0x0f,
    0x41, 0x2a, 0x5f, 0x44, 0x5f, 0x45, 0xf4, 0x5f, 0x45, 0xf4, 0x5f, 0x41,
    0x0f,
    // Styles: 12 definitions (font, character style), fnTEXT to fnMTEXTRA;
    // the twelfth, fnTEXT_FE, follows (see `far_east_style`).
    0x0c, 1, 0, 1, 0, 1, 2, 2, 2, 2, 0, 2, 0, 1, 1, 1, 0, 3, 0, 1, 0, 4, 0,
];

/// The character styles of fnTEXT, fnFUNCTION and fnUCGREEK in
/// [`V5_PREFERENCES`], counted from its end.
const V5_DEFINED_STYLES: [usize; 3] = [21, 19, 13];

/// The sizes in [`V5_PREFERENCES`], with their count.
const V5_SIZES: std::ops::Range<usize> = 2..20;

/// The default sizes with another full size (MTEFv5.md "Dimension arrays").
fn size_preferences(full: Rational) -> Vec<u8> {
    let mut points = String::new();
    full.write_scaled_decimal(&mut points, 1);
    // A dimension has no more digits than it needs.
    points.truncate(12);
    let mut nibbles = Vec::new();
    for (unit, value) in [
        (2, points.as_str()),
        (4, "58"),
        (4, "42"),
        (4, "150"),
        (4, "100"),
        (4, "75"),
        (4, "150"),
        (2, "1"),
    ] {
        nibbles.push(unit);
        nibbles.extend(value.bytes().map(|digit| match digit {
            b'.' => 10,
            b'-' => 11,
            digit => digit - b'0',
        }));
        nibbles.push(15);
    }
    let mut out = vec![8];
    out.extend(
        nibbles
            .chunks(2)
            .map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0)),
    );
    out
}

impl MtefWriter {
    /// Fast internal bridge for an expression returned by a parser or checked
    /// conversion result. Public byte calls retain strict arena validation.
    pub(crate) fn write_bytes_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        super::validate_compact_trusted(expr, config)?;
        Self::write_bytes_with_diagnostics_validated(expr, config, options)
    }

    /// Trusted compact writer used by the parse-once conversion result.
    pub(crate) fn write_compact_trusted(expr: &CompactExpr, config: &Config) -> Result<String> {
        let (bytes, _) =
            Self::write_bytes_with_diagnostics_trusted(expr, config, &MtefWriteOptions::default())?;
        Ok(hex_encode(&bytes))
    }

    /// Trusted diagnostic compact writer used by the parse-once conversion
    /// result.
    pub(crate) fn write_compact_with_diagnostics_trusted(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let (bytes, diagnostics) =
            Self::write_bytes_with_diagnostics_trusted(expr, config, &MtefWriteOptions::default())?;
        Ok((hex_encode(&bytes), diagnostics))
    }

    /// Write `expr` as binary MTEF.
    ///
    /// ```
    /// use mathema::{Config, MtefWriteOptions};
    /// use mathema::writer::mtef::MtefWriter;
    ///
    /// let result = mathema::from_latex("x^2").unwrap();
    /// let bytes = MtefWriter::write_bytes(
    ///     result.compact_expr(),
    ///     &Config::default(),
    ///     &MtefWriteOptions::default(),
    /// )
    /// .unwrap();
    /// assert_eq!(bytes[0], 5);
    /// assert_eq!(mathema::from_mtef_bytes(&bytes).unwrap().to_latex().unwrap(), "x^{2}");
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`](crate::Error::LimitExceeded) when the AST, or
    /// the AST the reader rebuilds from the output, is deeper than
    /// [`Config::max_depth`] or larger than [`Config::max_nodes`];
    /// [`Error::MtefWrite`](crate::Error::MtefWrite) if the output would not
    /// read back for any other reason (a writer defect);
    /// [`Error::Unsupported`](crate::Error::Unsupported) for an array with
    /// more than 255 rows or columns, and in Strict mode for a character
    /// outside the Basic Multilingual Plane (Lenient mode writes U+FFFD and
    /// reports it).
    pub fn write_bytes(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
    ) -> Result<Vec<u8>> {
        Self::write_bytes_with_diagnostics(expr, config, options).map(|(bytes, _)| bytes)
    }

    /// Write `expr` as binary MTEF, also returning a warning for each kind of
    /// information MTEF cannot hold that the AST had.
    ///
    /// # Errors
    ///
    /// As [`write_bytes`](Self::write_bytes).
    pub fn write_bytes_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        super::validate_compact_for_write(expr, config)?;
        Self::write_bytes_with_diagnostics_validated(expr, config, options)
    }

    fn write_bytes_with_diagnostics_validated(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        let (mtef, diagnostics, reading) = match Self::encode(expr, config, options) {
            Ok(written) => written,
            Err(error) => return Self::write_unsized(expr, config, options, error),
        };
        // The reading can nest deeper than the tree written. Where that could
        // matter, refuse a tree whose reading cannot be written again.
        if let Some(mut reading) = reading {
            crate::ast::normalize::normalize_compact_expr(&mut reading)?;
            // Any other error is reported when that tree is written.
            if let Err(limit @ crate::error::Error::LimitExceeded { .. }) =
                Self::encode(&reading, config, options)
            {
                return Self::write_unsized(expr, config, options, limit);
            }
        }
        Self::finish(mtef, diagnostics, options, options.v5())
    }

    /// Write without the font sizes what exceeds a limit with them: the
    /// separators between sizes make lines longer.
    #[cold]
    #[inline(never)]
    fn write_unsized(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
        error: crate::error::Error,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        if !matches!(error, crate::error::Error::LimitExceeded { .. }) || !expr.arena.has_attrs() {
            return Err(error);
        }
        let mut plain = expr.clone();
        plain.arena.clear_attrs();
        let (bytes, mut diagnostics) = Self::write_bytes_with_diagnostics(&plain, config, options)?;
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Warning,
            message: "the font sizes were dropped: with them the equation exceeds the limits"
                .to_owned(),
            span: None,
        });
        Ok((bytes, diagnostics))
    }

    /// The MTEF data without the OLE header, and its reading when the tree
    /// is near a limit.
    fn encode(
        expr: &CompactExpr,
        config: &Config,
        options: &MtefWriteOptions,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>, Option<CompactExpr>)> {
        let v5 = options.v5();
        let new_encoder = || {
            Encoder::new(
                &expr.arena,
                config,
                v5,
                expr.display,
                crate::ast::attrs::base_size_pt(expr.meta.as_deref()),
            )
        };
        let mut encoder = new_encoder();
        encoder.equation(expr.roots)?;
        if encoder.punctuation_needs_font() {
            encoder = new_encoder();
            encoder.punctuation_font = true;
            encoder.equation(expr.roots)?;
        }
        let styles = encoder.defined_styles();
        let body = std::mem::take(&mut encoder.out);
        let diagnostics = std::mem::take(&mut encoder.diagnostics);

        let far_east = encoder.far_east;
        let alphabets = encoder.alphabets;
        let mut mtef = Vec::with_capacity(body.len() + 256);
        // Header (MTEFv5.md "Header record", MTEFv3.md "MTEF header").
        mtef.push(if v5 { 5 } else { 3 });
        // Platform: Windows, whose encodings the writer uses.
        mtef.push(1);
        mtef.push(match options.product {
            MtefProduct::MathType => 0,
            MtefProduct::EquationEditor => 1,
            MtefProduct::Other(b) => b,
        });
        mtef.push(options.product_version);
        mtef.push(options.product_subversion);
        if v5 {
            mtef.extend(options.application_key.bytes().filter(|&b| b != 0));
            mtef.push(0);
            mtef.push(u8::from(expr.display == DisplayStyle::Inline));
            mtef.extend_from_slice(V5_DEFINITIONS);
            if alphabets {
                mtef.extend_from_slice(V5_ALPHABET_DEFINITIONS);
            }
            if far_east {
                let start = mtef.len();
                mtef.extend_from_slice(V5_FAR_EAST_DEFINITIONS);
                if alphabets {
                    mtef[start + FAR_EAST_ENCODING_AT] += 2;
                }
            }
            let base = expr.meta.as_deref().and_then(crate::ast::ExprMeta::base);
            match base {
                Some(base) => {
                    mtef.extend_from_slice(&V5_PREFERENCES[..V5_SIZES.start]);
                    mtef.extend(size_preferences(base));
                    mtef.extend_from_slice(&V5_PREFERENCES[V5_SIZES.end..]);
                },
                None => mtef.extend_from_slice(V5_PREFERENCES),
            }
            for (at, style) in V5_DEFINED_STYLES.into_iter().zip(styles) {
                let at = mtef.len() - at;
                mtef[at] = style;
            }
            mtef.extend_from_slice(far_east_style(far_east, alphabets));
        }
        mtef.extend_from_slice(&body);

        // The reader rebuilds the AST with rows around slots and cells, so it
        // can nest deeper than the AST written. Read the output back with the
        // same limits so that a successful write is always readable.
        // The input-size limit is for what a caller reads, not for what is
        // written, so the check reads without it.
        let mut check = config.with_strictness(crate::error::Strictness::Strict);
        check.max_input_bytes = None;
        match crate::reader::mtef::MtefReader::parse_bytes_compact(&mtef, &check) {
            Ok(reading) => {
                let reading = encoder.is_near_limits().then_some(reading.expr);
                Ok((mtef, diagnostics, reading))
            },
            Err(limit @ crate::error::Error::LimitExceeded { .. }) => Err(limit),
            Err(other) => Err(crate::error::Error::MtefWrite {
                message: format!(
                    "the MTEF written for this expression does not read back: {other}"
                ),
            }),
        }
    }

    /// Add the OLE header when asked for.
    fn finish(
        mtef: Vec<u8>,
        diagnostics: Vec<Diagnostic>,
        options: &MtefWriteOptions,
        v5: bool,
    ) -> Result<(Vec<u8>, Vec<Diagnostic>)> {
        if !options.ole_header {
            return Ok((mtef, diagnostics));
        }
        let object_size = u32::try_from(mtef.len()).map_err(|_| {
            crate::error::Error::Unsupported(
                "the MTEF data is too large for an Equation Native stream".into(),
            )
        })?;
        let mut out = Vec::with_capacity(OLE_HEADER_LEN + mtef.len());
        out.extend_from_slice(&(OLE_HEADER_LEN as u16).to_le_bytes());
        out.extend_from_slice(&OLE_HEADER_VERSION.to_le_bytes());
        let clipboard_format = if v5 {
            OLE_CLIPBOARD_FORMAT_V5
        } else {
            OLE_CLIPBOARD_FORMAT_V3
        };
        out.extend_from_slice(&clipboard_format.to_le_bytes());
        out.extend_from_slice(&object_size.to_le_bytes());
        // Four reserved 32-bit fields, "not used".
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&mtef);
        Ok((out, diagnostics))
    }
}

impl MathWriter for MtefWriter {
    /// Write the AST as MTEF 5, returned hex-encoded.
    fn write(expr: &MathExpr, config: &Config) -> Result<String> {
        super::validate_heap_for_write(expr, config)?;
        let compact = CompactExpr::from_math_expr_after_validation(expr)?;
        let (bytes, _) = Self::write_bytes_with_diagnostics_validated(
            &compact,
            config,
            &MtefWriteOptions::default(),
        )?;
        Ok(hex_encode(&bytes))
    }

    fn write_compact(expr: &CompactExpr, config: &Config) -> Result<String> {
        let (bytes, _) =
            Self::write_bytes_with_diagnostics(expr, config, &MtefWriteOptions::default())?;
        Ok(hex_encode(&bytes))
    }

    fn write_compact_with_diagnostics(
        expr: &CompactExpr,
        config: &Config,
    ) -> Result<(String, Vec<Diagnostic>)> {
        let (bytes, diagnostics) =
            Self::write_bytes_with_diagnostics(expr, config, &MtefWriteOptions::default())?;
        Ok((hex_encode(&bytes), diagnostics))
    }
}

/// Encode bytes as a lowercase hex string.
fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(data.len() * 2);
    for &b in data {
        hex.push(char::from(HEX[usize::from(b >> 4)]));
        hex.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    hex
}
