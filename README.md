# mathema

Convert math formulas between **LaTeX**, **MathML**, **OMML**, **MTEF**, and **UnicodeMath** via a shared AST. Optionally render to **SVG**, **PNG**, or **JPEG**.

## Quick start

```rust
use mathema::{convert, Format};

// One-liner conversion
let mathml = convert("x^2 + y^2 = z^2", Format::LaTeX, Format::MathML).unwrap();

// Builder API — parse once, write to multiple formats
let result = mathema::from_latex(r"\frac{1}{2}").unwrap();
let mathml = result.to_mathml().unwrap();
let omml   = result.to_omml().unwrap();
let umath  = result.to_unicode_math().unwrap();
```

## Supported formats

| Format | Read | Write |
|---|---|---|
| LaTeX | Yes | Yes |
| MathML | Yes | Yes |
| Office MathML (OMML) | Yes | Yes |
| UnicodeMath | Yes | Yes |
| MathType (MTEF) | Yes | Yes |
| SVG | -- | Yes (`render`) |
| PNG | -- | Yes (`raster`) |
| JPEG | -- | Yes (`raster`) |

## CLI

```sh
cargo build --release --features raster   # or just --features render for SVG only

# Convert between text formats
mathema convert -f latex -t mathml "x^2"
echo "x^2" | mathema convert -f latex -t omml

# Render to SVG, PNG, or JPEG
mathema render "x^2"                              # SVG to stdout
mathema render --format png -o out.png "x^2"      # PNG (requires raster)
mathema render --format jpeg -o out.jpeg "x^2"    # JPEG (requires raster)
mathema render --font-size 24 --color black "x^2" # custom options

# Validate input
mathema validate -f latex "x^2"

# Batch processing (pass multiple files)
mathema convert -t mathml -o out/ *.tex
mathema render --format png -o out/ *.tex
```

The `--from` / `-f` flag is optional; the format is auto-detected from file extension or content when omitted.

SVG output is **self-contained** by default: glyph outlines from Latin Modern Math are embedded as `<path>`/`<use>` elements, so the SVG renders identically everywhere (no fonts to install) and **looks the same as the PNG/JPEG output**, which is rasterized from the same outlines.

Format names: `latex` (`tex`), `mathml` (`mml`), `omml`, `mtef`, `unicodemath` (`um`), `svg`, `png`, `jpeg` (`jpg`).

## Features

| Feature | Description |
|---|---|
| `render` | SVG output via a built-in TeX-style layout engine |
| `raster` | PNG/JPEG output (implies `render`) |
| `font-embed` | Standalone SVG with embedded glyph outlines (implies `render`) |
| `serde` | Serialize / deserialize the AST |
| `wasm` | WebAssembly bindings (implies `serde`) |

```sh
cargo build --features render          # SVG rendering
cargo build --features raster          # PNG/JPEG rendering
cargo build --features font-embed      # standalone SVG with embedded fonts
cargo build --features "render,serde,wasm"  # all optional features
```

## WASM

Pre-built bindings and an example page live in `wasm/`. See [`wasm/README.md`](wasm/README.md) for build instructions and usage.

## Architecture

```
Input string ──Reader──▸ MathExpr (AST) ──Writer──▸ Output string
                              │
                         LayoutBox tree ──emit──▸ SVG       (render)
                              │
                         rasterize ──▸ PNG/JPEG             (raster)
```

The shared AST (`MathExpr` / `MathNode`) has 26 node variants covering the union of all supported formats. Readers parse format-specific input into this AST; writers serialize it back out.

The optional render pipeline applies TeX typesetting rules (TeXBook Rules 11–18) to produce positioned layout boxes, then emits them as SVG. The `raster` feature adds PNG/JPEG rasterization via `fontdue` + `tiny-skia`. The `font-embed` feature embeds glyph `<path>` outlines into the SVG for standalone rendering without external fonts.

## License

MIT
