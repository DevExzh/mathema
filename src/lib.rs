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
//!
//! // Ask a one-shot conversion to report both reader and writer diagnostics.
//! let (_unicode_math, diagnostics) = mathema::convert_with_diagnostics(
//!     r"\color{red}{x}",
//!     Format::LaTeX,
//!     Format::UnicodeMath,
//! ).unwrap();
//! assert!(!diagnostics.is_empty());
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
//! limits, so adversarial input is rejected rather than exhausting memory.
//! Every reader enforces [`Config::max_nodes`] while constructing its AST,
//! including generated MathML and OMML table nodes. MTEF and UnicodeMath also
//! enforce [`Config::max_depth`] while constructing trees.
//! `convert_with` and the `from_*` functions validate the configured depth on
//! every parsed tree. Limit failures use [`Error::LimitExceeded`].

#![deny(missing_docs)]

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
    ConvertResult, convert, convert_with, convert_with_config_and_diagnostics,
    convert_with_diagnostics, from_latex, from_latex_with, from_mathml, from_mathml_with,
    from_mtef, from_mtef_bytes, from_mtef_bytes_with, from_mtef_with, from_omml, from_omml_with,
    from_unicode_math, from_unicode_math_with, write_compact_expr,
    write_compact_expr_with_diagnostics, write_expr, write_expr_with_diagnostics,
};
#[cfg(feature = "render")]
pub use convert::{
    convert_with_config_and_svg_options, convert_with_config_and_svg_options_and_diagnostics,
    convert_with_svg_options, convert_with_svg_options_and_diagnostics,
};
pub use error::{
    Config, Diagnostic, DiagnosticKind, Error, Format, LatexProfile, LimitKind, ParseResult,
    Result, Strictness, TypesettingPolicy, UndefinedCommands,
};
pub use reader::mtef::{MtefInfo, MtefPlatform, MtefProduct, MtefVersion, OleHeader};
pub use reader::omml::{OmmlJustification, OmmlParagraph};
#[cfg(feature = "raster")]
pub use render::raster::{
    ImageFormat, RasterLimits, RasterOptions, render_to_image, render_to_image_with_diagnostics,
    render_to_image_with_diagnostics_and_limits, render_to_image_with_limits,
};
#[cfg(feature = "render")]
pub use render::{
    PreparedRender, RenderGeometry, RenderOptions, SvgLimits, SvgMode, SvgOptions, prepare_render,
    prepare_render_compact, render_to_svg, render_to_svg_compact_with_options,
    render_to_svg_text_compact_with_options, render_to_svg_text_with_options,
    render_to_svg_with_diagnostics, render_to_svg_with_options,
    render_to_svg_with_options_and_limits,
};
#[cfg(feature = "render")]
pub use render::{
    render_to_svg_standalone, render_to_svg_standalone_compact,
    render_to_svg_standalone_compact_arena, render_to_svg_standalone_with_diagnostics,
};
pub use writer::mathml::{MathmlOptions, MathmlProfile};
pub use writer::mtef::MtefWriteOptions;
pub use writer::omml::{OmmlOptions, OmmlRunProperties};
