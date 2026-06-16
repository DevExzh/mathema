pub mod entities;
pub mod fonts;
pub mod operators;
pub mod symbols;

pub use entities::MATHML_ENTITIES;
pub use fonts::*;
pub use symbols::*;

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
