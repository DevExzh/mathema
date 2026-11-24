pub mod entities;
pub mod fonts;
pub(crate) mod math_class;
pub mod operators;
mod scripts;
pub mod symbols;

pub use entities::MATHML_ENTITIES;
pub use fonts::*;
pub(crate) use scripts::decode_script_scalar;
pub use symbols::*;

/// Width in em of a fixed-width Unicode math space.
#[inline]
pub(crate) fn math_space_width(ch: char) -> Option<crate::ast::Rational> {
    let (num, den) = match ch {
        '\u{00A0}' | '\u{2005}' => (1, 4),
        '\u{2000}' | '\u{2002}' => (1, 2),
        '\u{2001}' | '\u{2003}' | '\u{3000}' => (1, 1),
        '\u{2004}' => (1, 3),
        '\u{2006}' | '\u{2009}' | '\u{202F}' => (1, 6),
        '\u{200A}' => (1, 18),
        '\u{205F}' => (2, 9),
        _ => return None,
    };
    Some(crate::ast::Rational::new(num, den))
}

/// Resolve a MathML/HTML named character reference (without the leading `&`
/// or trailing `;`) to its replacement string.
///
/// Handles the three forms found in MathML test data:
/// - named entities (`alpha`, `InvisibleTimes`, `ApplyFunction`, …)
/// - decimal numeric references (`#8721`)
/// - hexadecimal numeric references (`#x2211`, `#X2211`)
#[inline]
pub fn resolve_entity(name: &str) -> Option<std::borrow::Cow<'static, str>> {
    use std::borrow::Cow;
    if let Some(rest) = name.strip_prefix('#') {
        let cp = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        let ch = char::from_u32(cp)?;
        return Some(Cow::Owned(ch.to_string()));
    }
    MATHML_ENTITIES.get(name).map(|s| Cow::Borrowed(*s))
}

#[cfg(test)]
mod entity_tests {
    use super::resolve_entity;

    #[test]
    fn resolves_named() {
        assert_eq!(resolve_entity("alpha").as_deref(), Some("\u{03B1}"));
        assert_eq!(
            resolve_entity("InvisibleTimes").as_deref(),
            Some("\u{2062}")
        );
        assert_eq!(resolve_entity("ApplyFunction").as_deref(), Some("\u{2061}"));
        assert_eq!(resolve_entity("amp").as_deref(), Some("&"));
    }

    #[test]
    fn resolves_numeric() {
        assert_eq!(resolve_entity("#8721").as_deref(), Some("\u{2211}"));
        assert_eq!(resolve_entity("#x2211").as_deref(), Some("\u{2211}"));
        assert_eq!(resolve_entity("#X41").as_deref(), Some("A"));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(resolve_entity("definitelynotanentity"), None);
    }
}
