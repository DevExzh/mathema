//! # mathema
//!
//! Convert math formulas between **LaTeX**, **MathML**, **OMML**, **MTEF**, and
//! **UnicodeMath** through a shared abstract syntax tree, and optionally render
//! them to **SVG**, **PNG**, or **JPEG**.
//!
//! Every format is parsed by a *reader* into the shared [`MathExpr`] /
//! [`MathNode`] AST, then serialized by a *writer* back out to any other text
//! format. The optional render pipeline (feature `render`) applies TeXBook-grade
//! typesetting to produce positioned layout boxes and emits them as SVG; the
//! `raster` feature adds PNG/JPEG output.
//!
//! ## Quick start
//!
//! ```
//! use mathema::{convert, Format};
//!
//! // One-shot conversion between two text formats.
//! let mathml = convert("x^2 + y^2 = z^2", Format::LaTeX, Format::MathML).unwrap();
//! assert!(mathml.contains("<math"));
//!
//! // Parse once, write to several formats.
//! let result = mathema::from_latex(r"\frac{1}{2}").unwrap();
//! let _mathml = result.to_mathml().unwrap();
//! let _omml = result.to_omml().unwrap();
//! ```
//!
//! ## Feature flags
//!
//! - `cli` *(default)* — the `mathema` command-line binary.
//! - `render` — SVG output via the built-in TeX-style layout engine.
//! - `raster` — PNG/JPEG output (implies `render`).
//! - `serde` — `Serialize`/`Deserialize` for the AST.
//! - `wasm` — WebAssembly bindings (implies `serde`).
//!
//! ## Hardening
//!
//! Readers enforce [`Config::max_input_bytes`] (default 4 MiB) and layered parse
//! and AST depth limits, so adversarial input is rejected rather than exhausting
//! memory. See [`Error`] for the failure modes.

#![deny(missing_docs)]

#[doc(hidden)]
pub mod ast;
pub mod convert;
#[path = "render/css_colors.rs"]
pub(crate) mod css_colors;
pub mod error;
pub(crate) mod latex;
#[doc(hidden)]
pub mod reader;
#[cfg(feature = "render")]
pub mod render;
#[doc(hidden)]
pub mod unicode;
#[cfg(feature = "wasm")]
pub mod wasm;
#[doc(hidden)]
pub mod writer;

// Re-export the public API
pub use ast::arena::AstArena;
pub use ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
pub use ast::{MathExpr, MathNode};
pub use convert::{
    ConvertResult, convert, convert_with, from_latex, from_latex_with, from_mathml,
    from_mathml_with, from_mtef, from_mtef_with, from_omml, from_omml_with, from_unicode_math,
    from_unicode_math_with,
};
pub use error::{
    Config, Diagnostic, DiagnosticKind, Error, Format, ParseResult, Result, Strictness,
};
#[cfg(feature = "raster")]
pub use render::raster::{ImageFormat, RasterOptions, render_to_image};
#[cfg(feature = "render")]
pub use render::{RenderOptions, render_to_svg};
#[cfg(feature = "render")]
pub use render::{
    render_to_svg_standalone, render_to_svg_standalone_compact,
    render_to_svg_standalone_compact_arena,
};
