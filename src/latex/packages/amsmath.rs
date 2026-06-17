//! `amsmath` package support.
//!
//! Currently provides the variable-limit operators (`\varlimsup`, `\varliminf`,
//! `\varinjlim`, `\varprojlim`). These expand to a named limit operator (`\lim`)
//! decorated with an over/under line or arrow, matching amsmath's definitions:
//!
//! ```text
//! \varlimsup → \overline{\lim}        \varliminf → \underline{\lim}
//! \varinjlim → \underrightarrow{\lim} \varprojlim → \underleftarrow{\lim}
//! ```
//!
//! Scripts attached to these (e.g. `\varlimsup_{n\to\infty}`) are placed
//! above/below the decorated stack, because the layout treats a decorated limit
//! operator as a limit operator.

use super::SymbolClass;

/// Builtin macros contributed by `amsmath`: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("varlimsup", 0, r"\overline{\lim}"),
    ("varliminf", 0, r"\underline{\lim}"),
    ("varinjlim", 0, r"\underrightarrow{\lim}"),
    ("varprojlim", 0, r"\underleftarrow{\lim}"),
];

/// Symbol entries contributed by `amsmath`: none.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];
