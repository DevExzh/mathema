//! KaTeX built-in convenience macros.
//!
//! See [`super`] (`packages/mod.rs`) for the per-package file contract.
//!
//! KaTeX predefines a few shorthands that belong to no LaTeX package: the
//! blackboard-bold number sets (`\R`, `\N`, `\Z` and their HTML-entity style
//! spellings) and the section/paragraph signs `\S` and `\P`, which KaTeX
//! accepts in math as well as text mode. They expand to the same input the
//! author would otherwise write, so every writer and the renderer see the
//! usual `\mathbb` letter.

use super::SymbolClass;

/// Builtin macros contributed by this package: `(name, param_count, body)`.
pub const MACROS: &[(&str, u8, &str)] = &[
    ("N", 0, r"\mathbb{N}"),
    ("R", 0, r"\mathbb{R}"),
    ("Z", 0, r"\mathbb{Z}"),
    ("natnums", 0, r"\mathbb{N}"),
    ("Reals", 0, r"\mathbb{R}"),
    ("reals", 0, r"\mathbb{R}"),
    ("Complex", 0, r"\mathbb{C}"),
    ("cnums", 0, r"\mathbb{C}"),
    ("S", 0, "\u{00A7}"), // § SECTION SIGN
    ("P", 0, "\u{00B6}"), // ¶ PILCROW SIGN
];

/// Symbol entries for the parser fallback: `(command_name, unicode_str, class)`.
pub const SYMBOLS: &[(&str, &str, SymbolClass)] = &[];

#[cfg(test)]
mod tests {
    use crate::convert;

    fn mathml(latex: &str) -> String {
        convert::from_latex(latex)
            .expect("parse should succeed")
            .to_mathml()
            .expect("mathml should succeed")
    }

    #[test]
    fn number_sets_are_blackboard_letters() {
        for (alias, canonical) in [
            (r"\R", r"\mathbb{R}"),
            (r"\Reals", r"\mathbb{R}"),
            (r"\reals", r"\mathbb{R}"),
            (r"\N", r"\mathbb{N}"),
            (r"\natnums", r"\mathbb{N}"),
            (r"\Z", r"\mathbb{Z}"),
            (r"\Complex", r"\mathbb{C}"),
            (r"\cnums", r"\mathbb{C}"),
        ] {
            assert_eq!(mathml(alias), mathml(canonical), "{alias}");
        }
    }

    #[test]
    fn section_and_paragraph_signs() {
        assert!(mathml(r"\S 3").contains('\u{00A7}'));
        assert!(mathml(r"\P").contains('\u{00B6}'));
    }

    #[test]
    fn user_definitions_override_the_shorthands() {
        assert_eq!(mathml(r"\renewcommand{\R}{x}\R"), mathml("x"));
    }
}
