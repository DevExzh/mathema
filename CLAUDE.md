# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```sh
cargo build                          # core library (no optional features)
cargo build --features render        # with SVG rendering
cargo build --features raster        # with PNG/JPEG rendering (implies render)
cargo build --features font-embed     # SVG with embedded font paths (standalone)
cargo build --features "render,serde,wasm"  # all optional features

cargo test                           # unit + integration tests (core only)
cargo test --features render         # include SVG render tests
cargo test --features raster         # include raster render tests
cargo test --features font-embed     # include font embedding tests
cargo test --test round_trip         # single integration test file
cargo test convert::tests::latex_to_mathml  # single test by name

cargo clippy --all-targets --features raster,serde  # lint with all compilable features
cargo bench                          # criterion benchmarks
```

Fuzz targets live in `fuzz/` (one per reader): `cargo +nightly fuzz run fuzz_latex`, etc.

## Architecture

```
Input string ──Reader──▶ MathExpr (AST) ──Writer──▶ Output string
                              │
                         LayoutBox tree ──emit──▶ SVG   (feature: render)
                              │
                         rasterize ──▶ PNG/JPEG         (feature: raster)
```

**AST** (`src/ast/mod.rs`): `MathExpr` holds `Vec<MathNode>` + `DisplayStyle`. `MathNode` is a 26-variant enum covering the union of all formats. `normalize.rs` canonicalizes the AST between parse and write. `visit.rs` provides a visitor trait.

**Reader trait** (`src/reader/mod.rs`): `fn parse(input: &str, config: &Config) -> Result<ParseResult>`. One implementation per format in `src/reader/{latex,mathml,omml,mtef,unicode_math}.rs`. LaTeX and UnicodeMath use `nom` combinators; MathML and OMML use `quick-xml`.

**Writer trait** (`src/writer/mod.rs`): `fn write(expr: &MathExpr, config: &Config) -> Result<String>`. Mirror structure in `src/writer/`.

**Render pipeline** (`src/render/`, feature-gated):
- `style.rs` — TeX math styles (Display/Text/Script/ScriptScript)
- `params.rs` — TeX typesetting constants from the TeXBook
- `metrics.rs` — glyph metrics from embedded Latin Modern Math font data
- `layout.rs` — recursive layout producing a `LayoutBox` tree (TeXBook Rules 11-18)
- `spacing.rs` — inter-atom spacing per Knuth's spacing table
- `emit.rs` — SVG serialization from positioned layout boxes
- `font_embed.rs` — glyph outline extraction via `ttf-parser` (feature `font-embed`)
- `subset.rs` — font subsetting (only embed used glyphs)
- `svg_writer.rs` — standalone SVG with embedded `<path>` glyph outlines (feature `font-embed`)
- `raster.rs` — PNG/JPEG rasterization via `fontdue` + `tiny-skia` (feature `raster`)

**Unicode tables** (`src/unicode/`): `symbols.rs` and `operators.rs` are `phf` perfect-hash maps for symbol/operator lookup. `fonts.rs` has embedded font metrics.

**Config** (`src/error.rs`): `Strictness::Strict` (default) rejects malformed input; `Strictness::Lenient` recovers and collects diagnostics.

**Public API** (`src/convert.rs`): `convert()` one-liner and `from_latex()` / `from_mathml()` / etc. builder pattern returning `ConvertResult` for multi-format output.

## Feature Flags

| Feature  | Gate                | Deps added                            |
|----------|---------------------|---------------------------------------|
| `render` | SVG output          | none                                  |
| `raster` | PNG/JPEG output     | `fontdue`, `tiny-skia`, `jpeg-encoder` (implies `render`) |
| `font-embed` | Standalone SVG | `ttf-parser` (implies `render`)           |
| `serde`  | AST serialization   | `serde`                               |
| `wasm`   | WASM bindings       | `wasm-bindgen`                        |

## Rust Edition

Edition 2024 — requires Rust nightly or recent stable (1.85+).
