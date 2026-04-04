//! TypeScript type documentation for the WASM API.
//!
//! This module contains no runtime code. It documents the shape of
//! JavaScript objects produced by `serde-wasm-bindgen` serialization
//! of Rust types, for reference when maintaining `wasm/mathema.d.ts`.
//!
//! ## AST Serialization Schema
//!
//! The serde representation uses **externally-tagged** enums:
//! - `MathNode::Number("42")` -> `{ "Number": "42" }`
//! - `MathNode::Superscript { base, sup }` -> `{ "Superscript": { "base": ..., "sup": ... } }`
//! - `MathNode::Symbol(kind, text)` -> `{ "Symbol": ["Ordinary", "+"] }`
//!
//! `Option<T>` maps to `T | null` in JavaScript.
//! `Box<T>` is transparent (serializes as `T`).
//! `char` serializes as a single-character string.
//!
//! ## Keeping types in sync
//!
//! When modifying `MathNode` variants in `src/ast/mod.rs`:
//! 1. Update `wasm/mathema.d.ts` to match
//! 2. Run `cargo test --features wasm` to verify serde roundtrip
