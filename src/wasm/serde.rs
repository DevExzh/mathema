//! Zero-copy AST serialization for the JS/WASM boundary.
//!
//! Uses `serde-wasm-bindgen` to construct JavaScript objects directly from
//! the serde data model, avoiding JSON string intermediaries. This is
//! faster than JSON round-tripping and produces native JS objects that
//! can be traversed without parsing.

use wasm_bindgen::JsValue;

/// Serialize any serde-compatible value to a `JsValue` using `serde-wasm-bindgen`.
///
/// This constructs JS objects directly via the WASM/JS interface, avoiding:
/// 1. JSON string serialization/deserialization overhead
/// 2. Intermediate string allocation
///
/// The resulting `JsValue` is a native JS object tree matching the serde schema.
/// For `MathExpr`, this produces:
/// ```js
/// {
///   nodes: [{ Identifier: "x" }, { Superscript: { base: ..., sup: ... } }],
///   display: "Inline"
/// }
/// ```
pub fn to_js_value<T: ::serde::Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_large_number_types_as_bigints(false);
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Deserialize a `JsValue` back into a Rust type.
///
/// Useful for accepting AST modifications from JavaScript.
pub fn from_js_value<T: ::serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::ast::{DisplayStyle, MathExpr, MathNode};

    #[test]
    fn serialize_simple_expr() {
        let expr = MathExpr::new(
            vec![MathNode::Identifier("x".into())],
            DisplayStyle::Inline,
        );
        let json = serde_json::to_string(&expr).unwrap();
        assert!(json.contains("Identifier"));
        assert!(json.contains("Inline"));
    }

    #[test]
    fn serialize_complex_expr() {
        let expr = MathExpr::new(
            vec![MathNode::Superscript {
                base: Box::new(MathNode::Identifier("x".into())),
                sup: Box::new(MathNode::Number("2".into())),
            }],
            DisplayStyle::Block,
        );
        let json = serde_json::to_string(&expr).unwrap();
        assert!(json.contains("Superscript"));
        assert!(json.contains("Block"));
    }

    #[test]
    fn roundtrip_via_json() {
        let expr = MathExpr::new(
            vec![MathNode::Number("42".into())],
            DisplayStyle::Inline,
        );
        let json = serde_json::to_string(&expr).unwrap();
        assert!(json.contains("\"Number\""));
        assert!(json.contains("42"));
        let roundtrip: MathExpr = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, expr);
    }
}
