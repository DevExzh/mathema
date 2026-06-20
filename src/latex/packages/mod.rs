//! LaTeX package support registry.
//!
//! This module aggregates per-package "support files", one Rust file per LaTeX
//! package. The goal is that adding support for a new package (or filling in an
//! existing stub) only ever requires editing **one** file — the package's own
//! file — never this `mod.rs` and never the parser/preprocessor. Every package
//! file is already registered below, so Wave-2 work is conflict-free.
//!
//! # Per-package file contract
//!
//! Each package file (e.g. `gensymb.rs`) exposes exactly two public constants:
//!
//! ```ignore
//! use super::SymbolClass;
//!
//! /// Builtin macros contributed by this package: `(name, param_count, body)`.
//! ///
//! /// * `name`        — command name **without** the leading backslash.
//! /// * `param_count` — number of `#1..#9` parameters the body consumes.
//! /// * `body`        — replacement text; may contain LaTeX and/or literal
//! ///                   Unicode characters. `#1`..`#9` are substituted with the
//! ///                   captured arguments. These are seeded into the
//! ///                   preprocessor and expand exactly like a user
//! ///                   `\newcommand`, **except** that a user `\newcommand` /
//! ///                   `\def` of the same name overrides the builtin.
//! pub const MACROS: &[(&str, u8, &str)] = &[];
//!
//! /// Symbol entries consulted by the parser's unknown-command fallback:
//! /// `(command_name, unicode_str, class)`.
//! ///
//! /// * `command_name` — command name **without** the leading backslash.
//! /// * `unicode_str`  — the literal Unicode string the command renders as
//! ///                    (usually one char, but multi-char is allowed).
//! /// * `class`        — the [`SymbolClass`] used to classify the symbol.
//! ///
//! /// These are only consulted *after* the built-in PHF symbol tables miss, so
//! /// a package can never shadow a core symbol.
//! pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];
//! ```
//!
//! Prefer `MACROS` for anything that is a rewrite (e.g. `\coloneqq` ->
//! `\mathrel{:=}`) or that must work in both text and math mode (preprocessor
//! expansion happens before parsing, so it is mode-agnostic). Use `SYMBOLS` for
//! commands that are a single glyph best emitted directly by the parser with a
//! specific symbol class.

/// Symbol classification re-exported under a stable, package-facing name.
///
/// Package files refer to this as `SymbolClass`; it is the parser's
/// [`crate::ast::SymbolKind`].
pub use crate::ast::SymbolKind as SymbolClass;

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

// --- Registered package files (each owns its own file) ---------------------
pub mod amsbsy;
pub mod amsfonts;
pub mod amsmath;
pub mod amssymb;
pub mod bbm;
pub mod bboldx;
pub mod braket;
pub mod centernot;
pub mod dsfont;
pub mod extpfeil;
pub mod gensymb;
pub mod mathtools;
pub mod physics;
pub mod textcomp;
pub mod units;
pub mod upgreek;

/// All package macro tables, in registration order.
const PACKAGE_MACROS: &[&[(&str, u8, &str)]] = &[
    amsbsy::MACROS,
    amsfonts::MACROS,
    amsmath::MACROS,
    amssymb::MACROS,
    bbm::MACROS,
    bboldx::MACROS,
    braket::MACROS,
    centernot::MACROS,
    dsfont::MACROS,
    extpfeil::MACROS,
    gensymb::MACROS,
    mathtools::MACROS,
    physics::MACROS,
    textcomp::MACROS,
    units::MACROS,
    upgreek::MACROS,
];

/// All package symbol tables, in registration order.
const PACKAGE_SYMBOLS: &[&[(&str, &str, SymbolClass)]] = &[
    amsbsy::SYMBOLS,
    amsfonts::SYMBOLS,
    amsmath::SYMBOLS,
    amssymb::SYMBOLS,
    bbm::SYMBOLS,
    bboldx::SYMBOLS,
    braket::SYMBOLS,
    centernot::SYMBOLS,
    dsfont::SYMBOLS,
    extpfeil::SYMBOLS,
    gensymb::SYMBOLS,
    mathtools::SYMBOLS,
    physics::SYMBOLS,
    textcomp::SYMBOLS,
    units::SYMBOLS,
    upgreek::SYMBOLS,
];

/// Iterator over every builtin macro `(name, param_count, body)` contributed by
/// all registered packages. Used to seed the preprocessor.
pub fn builtin_macros() -> impl Iterator<Item = (&'static str, u8, &'static str)> {
    PACKAGE_MACROS
        .iter()
        .flat_map(|table| table.iter().copied())
}

/// Cached set of all builtin macro names for cheap membership checks.
fn builtin_name_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| builtin_macros().map(|(name, _, _)| name).collect())
}

/// Cheap membership check: is `name` (without backslash) a builtin macro?
///
/// Used by `needs_preprocessing` so plain input keeps the zero-copy fast path.
#[inline]
pub fn is_builtin_macro_name(name: &str) -> bool {
    builtin_name_set().contains(name)
}

/// Cached lookup map of every package-contributed symbol command name to its
/// `(unicode_str, class)`. Built once on first use, mirroring
/// [`builtin_name_set`]. Replaces an O(n) linear scan over all package tables.
///
/// Insertion preserves first-table-wins semantics (`or_insert`), matching the
/// previous linear scan that returned the first matching entry.
fn package_symbol_map() -> &'static HashMap<&'static str, (&'static str, SymbolClass)> {
    static MAP: OnceLock<HashMap<&'static str, (&'static str, SymbolClass)>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for table in PACKAGE_SYMBOLS {
            for &(cmd, unicode, class) in *table {
                m.entry(cmd).or_insert((unicode, class));
            }
        }
        m
    })
}

/// Parser fallback: look up a package-contributed symbol by command name.
///
/// Returns `(unicode_str, class)`. Consulted only after the core PHF tables
/// miss, so it can never shadow a core symbol.
pub fn lookup_package_symbol(name: &str) -> Option<(&'static str, SymbolClass)> {
    package_symbol_map().get(name).copied()
}

/// Detect cross-package builtin-macro shadows that would silently change
/// behavior. Returns the sorted list of macro names that are registered by more
/// than one package with **differing** bodies (identical re-definitions, e.g.
/// `\abs` in both `mathtools` and `physics`, are harmless and excluded).
///
/// Builtin macros are seeded into a `HashMap` with last-writer-wins semantics,
/// so a name defined by two packages with different bodies is a silent shadow
/// (this was the `physics`-over-`braket` `\bra`/`\ket` bug). The
/// `no_conflicting_builtin_macro_shadows` test asserts this list is empty.
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub fn conflicting_macro_shadows() -> Vec<&'static str> {
    let mut bodies: HashMap<&'static str, &'static str> = HashMap::new();
    let mut conflicts: HashSet<&'static str> = HashSet::new();
    for table in PACKAGE_MACROS {
        for &(name, _, body) in *table {
            match bodies.get(name) {
                Some(&prev) if prev != body => {
                    conflicts.insert(name);
                },
                Some(_) => {},
                None => {
                    bodies.insert(name, body);
                },
            }
        }
    }
    let mut v: Vec<&'static str> = conflicts.into_iter().collect();
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use crate::{Format, convert};

    fn to_mathml(latex: &str) -> String {
        convert(latex, Format::LaTeX, Format::MathML).expect("convert should succeed")
    }

    // --- gensymb reference package -----------------------------------------

    #[test]
    fn gensymb_degree_math() {
        assert!(to_mathml("12\\degree").contains('\u{00B0}'));
    }

    #[test]
    fn gensymb_degree_text() {
        assert!(to_mathml("\\text{room \\degree}").contains('\u{00B0}'));
    }

    #[test]
    fn gensymb_celsius_micro_ohm_perthousand() {
        assert!(to_mathml("\\celsius").contains('\u{2103}'));
        assert!(to_mathml("\\micro").contains('\u{00B5}'));
        assert!(to_mathml("\\ohm").contains('\u{03A9}'));
        assert!(to_mathml("\\perthousand").contains('\u{2030}'));
    }

    // --- amsbsy reference package ------------------------------------------

    #[test]
    fn amsbsy_pmb_renders_bold() {
        // `\pmb` is handled directly in `parse.rs` (mapped to a bold math font),
        // so amsbsy contributes no macro for it — see `amsbsy.rs`. This test
        // pins the existing behavior: the argument is emitted in a bold style.
        let mathml = to_mathml("\\pmb{x}");
        assert!(
            mathml.contains("bold") || mathml.contains("mathvariant=\"bold\""),
            "expected bold styling, got: {mathml}"
        );
    }

    // --- registry plumbing --------------------------------------------------

    #[test]
    fn user_macro_overrides_builtin() {
        // gensymb seeds `\degree` -> `°`. A user redefinition must win.
        let mathml = to_mathml("\\newcommand{\\degree}{X}12\\degree");
        assert!(mathml.contains('X'));
        assert!(!mathml.contains('\u{00B0}'));
    }

    #[test]
    fn builtin_name_set_contains_gensymb() {
        assert!(super::is_builtin_macro_name("degree"));
        assert!(!super::is_builtin_macro_name("frac"));
    }

    #[test]
    fn no_conflicting_builtin_macro_shadows() {
        // No two packages may register the same macro name with differing
        // bodies (that would be a silent last-writer-wins shadow, as the
        // physics-over-braket `\bra`/`\ket` bug was). Identical re-definitions
        // (e.g. `\abs` in both mathtools and physics) are harmless.
        let conflicts = super::conflicting_macro_shadows();
        assert!(
            conflicts.is_empty(),
            "conflicting builtin macro shadows: {conflicts:?}"
        );
    }
}
