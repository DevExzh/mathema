//! `amsbsy` package support.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! `amsbsy` provides `\boldsymbol` and `\pmb` ("poor man's bold").
//!
//! NOTE: Both `\boldsymbol` and `\pmb` are already handled directly in
//! `src/latex/parse.rs` (`\pmb` maps to a bold math font, `\boldsymbol` is in
//! the math-font command list). To avoid duplicating that behavior, this
//! package contributes no macros or symbols for them. The reference test in
//! `packages/mod.rs` (`amsbsy_pmb_renders_bold`) pins the existing behavior.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
///
/// Intentionally empty: `\pmb` and `\boldsymbol` are handled in `parse.rs`.
pub const MACROS: &[(&str, u8, &str)] = &[];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];
