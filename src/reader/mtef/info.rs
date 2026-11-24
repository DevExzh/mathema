//! Header information an MTEF stream declares about itself.

/// The MTEF format version of a decoded equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MtefVersion {
    /// MTEF 2, written by Equation Editor 1.x and 2.x and by MathType 3.0.
    /// Read only: the writer writes MTEF 3 or 5 instead.
    V2,
    /// MTEF 3, written by Equation Editor 3.x.
    V3,
    /// MTEF 4, written by MathType 3.5. Read only: the writer writes MTEF 3
    /// or 5 instead.
    V4,
    /// MTEF 5, written by MathType 4.0 and later.
    V5,
}

impl MtefVersion {
    /// The version number as written in the MTEF header (2 to 5).
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            MtefVersion::V2 => 2,
            MtefVersion::V3 => 3,
            MtefVersion::V4 => 4,
            MtefVersion::V5 => 5,
        }
    }
}

/// The platform an MTEF stream was generated on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MtefPlatform {
    /// Macintosh (header value 0).
    Mac,
    /// Windows (header value 1).
    Windows,
    /// Any other header value.
    Other(u8),
}

impl MtefPlatform {
    pub(super) fn from_byte(b: u8) -> Self {
        match b {
            0 => MtefPlatform::Mac,
            1 => MtefPlatform::Windows,
            other => MtefPlatform::Other(other),
        }
    }
}

/// The product that generated an MTEF stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MtefProduct {
    /// MathType (header value 0).
    MathType,
    /// Equation Editor (header value 1).
    EquationEditor,
    /// Any other header value.
    Other(u8),
}

impl MtefProduct {
    pub(super) fn from_byte(b: u8) -> Self {
        match b {
            0 => MtefProduct::MathType,
            1 => MtefProduct::EquationEditor,
            other => MtefProduct::Other(other),
        }
    }
}

/// The `EQNOLEFILEHDR` that precedes MTEF data in an OLE `Equation Native`
/// stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OleHeader {
    /// The header's version field (`0x0002_0000` for every known writer).
    pub version: u32,
    /// The registered clipboard format of the equation data.
    pub clipboard_format: u16,
    /// The declared length of the MTEF data that follows the header.
    pub object_size: u32,
}

/// What an MTEF input declared about itself: format version, generating
/// platform and product, and the OLE header when the input was an
/// `Equation Native` stream.
///
/// A caller that rewrites an embedded equation can use this to keep the
/// object's kind (Equation Editor 3 versus MathType) unchanged.
///
/// ```
/// use mathema::{MtefProduct, MtefVersion};
///
/// // An Equation Editor 3 equation holding the single digit 2.
/// let mtef = [3, 1, 1, 3, 0, 0x0A, 0x01, 0x02, 0x88, 0x32, 0x00, 0x00, 0x00];
/// let result = mathema::from_mtef_bytes(&mtef).unwrap();
/// let info = result.mtef_info().unwrap();
/// assert_eq!(info.version, MtefVersion::V3);
/// assert_eq!(info.product, MtefProduct::EquationEditor);
/// assert!(info.ole_header.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MtefInfo {
    /// The MTEF format version.
    pub version: MtefVersion,
    /// The generating platform.
    pub platform: MtefPlatform,
    /// The generating product.
    pub product: MtefProduct,
    /// The generating product's major version.
    pub product_version: u8,
    /// The generating product's minor version.
    pub product_subversion: u8,
    /// The application key of an MTEF 5 stream (for example `DSMT6`).
    pub application_key: Option<String>,
    /// Whether an MTEF 5 stream marks the equation as inline (`Some(true)`)
    /// or display (`Some(false)`). MTEF 2 to 4 do not record this.
    pub inline: Option<bool>,
    /// The OLE header, when the input was an `Equation Native` stream.
    pub ole_header: Option<OleHeader>,
}
