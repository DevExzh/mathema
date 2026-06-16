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

The MathML reader handles **both Presentation and Content MathML**. Content forms (`<apply>`, `<ci>`, `<cn>`, `<plus/>`, `<sin/>`, `<int/>`, `<matrix>`, `<piecewise>`, …) are lowered to the same presentation AST by `src/reader/mathml/content.rs` (a submodule of `mathml`, so it reuses the parser's private arena/helpers). Named character references (`&alpha;`, `&InvisibleTimes;`, the full ~2100-entry W3C set) resolve via `src/unicode/entities.rs` (auto-generated `phf` map) through `unicode::resolve_entity`; the reader's `unescape_text`/`unescape_attr` apply it to text and attributes. Math "invisible" operators (U+2061–2064, zero-width controls) render as zero-width kerns — see `render::layout::is_invisible_char`. Use `cargo run --example mathml_audit --features render -- <dir>` to audit a corpus (e.g. the W3C `testsuite`).

**Writer trait** (`src/writer/mod.rs`): `fn write(expr: &MathExpr, config: &Config) -> Result<String>`. Mirror structure in `src/writer/`.

**Render pipeline** (`src/render/`, feature-gated):
- `style.rs` — TeX math styles (Display/Text/Script/ScriptScript)
- `params.rs` — TeX typesetting constants from the TeXBook
- `metrics.rs` — glyph metrics from embedded Latin Modern Math font data (font-aware via the font registry)
- `fonts_registry.rs` — **multi-font registry**: a `FontId` enum (Math + Latin Modern text faces — roman/sans/mono in regular/bold/italic/oblique, extracted from `fonts/lm.zip` into `fonts/lm*10-*.otf`) with lazily-parsed `ttf_parser::Face`s, per-`(FontId,char)` metrics cache, and `TextStyle -> FontId` selection. `\text`/`\mathrm`→roman, `\mathbf`→roman-bold, `\mathsf`→sans, `\mathtt`→mono; `\mathbb`/`\mathcal`/`\mathfrak` (DoubleStruck/Script/Fraktur) keep using the Math font's Unicode math-alphanumeric glyphs. MATH-table features (constants, stretchy delimiters, big-operator variants, italic corrections) always come from the Math font.
- `layout.rs` — recursive layout producing a `LayoutBox` tree (TeXBook Rules 11-18)
- `spacing.rs` — inter-atom spacing per Knuth's spacing table
- `emit.rs` — SVG serialization from positioned layout boxes
- `glyph_variants.rs` — OpenType MATH-table size variants (stretchy delimiters, display large operators); shared by the raster and embedded-SVG emitters so both draw the same glyphs
- `font_embed.rs` / `subset.rs` — glyph outline extraction via `ttf-parser` (now under the `render` feature)
- `svg_writer.rs` — standalone SVG with embedded `<path>` glyph outlines (under `render`)
- `raster.rs` — PNG/JPEG rasterization via `fontdue` + `tiny-skia` (feature `raster`)

**SVG output modes:** `ConvertResult::to_svg()` produces a **standalone, embedded-outline SVG** (Latin Modern Math glyph outlines as `<path>`/`<use>`), so it renders identically everywhere and **visually matches the PNG/JPEG output** (the shared layout pass + identical outlines + identical MATH-table variant glyphs). `ConvertResult::to_svg_text()` produces the older `<text>`-based SVG that depends on client-side fonts. SVG↔PNG parity is guarded by `tests/svg_png_parity.rs` (rasterizes the SVG with `resvg` and pixel-compares against the PNG).

**Render performance notes:**
- SVG path strings (radical, scaled KaTeX accents) are built during layout with `layout::fmt_fixed4` / `fmt_fixed4_trim` — manual fixed-point formatters that avoid `core::fmt`'s `{:.4}` float path (~300 ns/value). This cut `\sqrt` layout ~12× and wide accents (`\widehat`/`\widetilde`) ~6×.
- The embedded font's glyph outlines are cached in `font_embed.rs` (`GLYPH_CACHE` / `VARIANT_CACHE`, keyed by char / glyph id), so repeated renders reuse extracted `<path>` data instead of re-interpreting CFF charstrings each time (`to_svg()` ~2.7× faster). The cache only applies to the embedded `FONT_DATA` (gated on `ptr::eq`).
- The LaTeX parser pre-sizes its arena from the input length and borrows the input on the common no-macro path (`preprocess_cow`).

**Unicode tables** (`src/unicode/`): `symbols.rs` and `operators.rs` are `phf` perfect-hash maps for symbol/operator lookup. `fonts.rs` has embedded font metrics.

**LaTeX package support** (`src/latex/`): ~26 LaTeX packages (MathJax-style) are supported via three seams:
- `packages/` — a per-package registry (`mod.rs` aggregates one file per package). Each file exports `MACROS: &[(name, nargs, body)]` (builtin macros seeded into the preprocessor before parsing; user `\newcommand` overrides them) and `SYMBOLS: &[(name, unicode, SymbolClass)]` (consulted by the parser's unknown-command fallback after the core PHF tables miss). Macro/symbol packages live entirely here: amsbsy, amsfonts, amssymb, bbm, bboldx, braket, centernot, dsfont, extpfeil, gensymb, mathtools, physics, textcomp, units, upgreek. Add a package by filling one stub file.
- `mhchem.rs` / `siunitx.rs` — mini-language translators (`\ce`/`\pu`, `\SI`/`\num`/`\si`/`\ang`/`\qty`/`\unit`) that rewrite their argument into standard LaTeX and re-parse it. One dispatch arm each in `parse.rs`.
- New environments/commands in `parse.rs` that **compose existing `CompactNode` variants** (no new variant): amscd `CD`, cases `numcases`/`subnumcases`, `empheq`, colortbl (`\cellcolor`/`\rowcolor`/`\columncolor` → `Boxed` cell backgrounds), bussproofs `prooftree` (→ `Array`+`Rule`), xypic `\xymatrix`, and real `color`/`xcolor` (`\color` switch, `\definecolor`, `red!50!blue` mixing → resolved into the existing `Color`/`Boxed` nodes; resolution helpers in `render/css_colors.rs`).

**Config** (`src/error.rs`): `Strictness::Strict` (default) rejects malformed input; `Strictness::Lenient` recovers and collects diagnostics.

**Public API** (`src/convert.rs`): `convert()` one-liner and `from_latex()` / `from_mathml()` / etc. builder pattern returning `ConvertResult` for multi-format output.

**WASM bindings** (`src/wasm/`, feature `wasm`): `MathemaConverter`/`ParsedMath` expose `toLatex`/`toMathml`/`toOmml`/`toUnicodeMath`/`toMtef` plus, under `wasm,render`, `toSvg(fontSizePx, display)` (standalone embedded-outline SVG) and `toSvgText(...)`. Build with `wasm-pack build --target web -- --no-default-features --features "wasm render"`.

**Web app** (`web/`): a static, **TypeScript** SvelteKit single-page app (`adapter-static`, `ssr=false`) that serves the renderer + converter entirely client-side via the WASM package. `web/src/lib/mathema.ts` loads the module once; `web/src/routes/+page.svelte` (`<script lang="ts">`) is the UI. See `web/README.md` for build/run steps (`bun install && bun run wasm && bun run dev`). The generated `web/src/lib/wasm/` is a build artifact (git-ignored).

## Feature Flags

| Feature  | Gate                | Deps added                            |
|----------|---------------------|---------------------------------------|
| `render` | SVG output                            | `ttf-parser` (embedded-outline SVG)   |
| `raster` | PNG/JPEG output     | `fontdue`, `tiny-skia`, `jpeg-encoder` (implies `render`) |
| `font-embed` | Standalone SVG (alias of `render`; outline embedding now lives under `render`) | — |
| `serde`  | AST serialization   | `serde`                               |
| `wasm`   | WASM bindings       | `wasm-bindgen`                        |

## Rust Edition

Edition 2024 — requires Rust nightly or recent stable (1.85+).
