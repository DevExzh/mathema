pub mod ast;
pub mod convert;
pub mod error;
pub mod reader;
#[cfg(feature = "render")]
pub mod render;
pub mod unicode;
#[cfg(feature = "wasm")]
pub mod wasm;
pub mod writer;

// Re-export the public API
pub use ast::{MathExpr, MathNode};
pub use convert::{
    ConvertResult, convert, convert_with, from_latex, from_latex_with, from_mathml,
    from_mathml_with, from_mtef, from_mtef_with, from_omml, from_omml_with, from_unicode_math,
    from_unicode_math_with,
};
pub use error::{Config, Error, Format, ParseResult, Result, Strictness};
#[cfg(feature = "raster")]
pub use render::raster::{ImageFormat, RasterOptions, render_to_image};
#[cfg(feature = "render")]
pub use render::{RenderOptions, render_to_svg};
