# mathema

Convert math formulas between **LaTeX**, **MathML**, **OMML**, **MTEF**, and **UnicodeMath** via a shared AST. Optionally render to **SVG**.

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
| SVG | -- | Yes (feature `render`) |

## CLI

```sh
cargo build --release --features render

# Positional input
mathema -f latex -t mathml "x^2"

# Pipe from stdin
echo "x^2" | mathema -f latex -t svg
```

Format names: `latex` (`tex`), `mathml` (`mml`), `omml`, `mtef`, `unicodemath` (`um`), `svg` (output only, requires `render` feature).

## Features

| Feature | Description |
|---|---|
| `render` | SVG output via a built-in TeX-style layout engine |
| `serde` | Serialize / deserialize the AST |
| `wasm` | WebAssembly bindings |

```sh
# Enable SVG rendering
cargo build --features render

# Enable all optional features
cargo build --features "render,serde,wasm"
```

## Architecture

```
Input string ──Reader──▸ MathExpr (AST) ──Writer──▸ Output string
                              │
                         LayoutBox tree ──emit──▸ SVG  (feature: render)
```

The shared AST (`MathExpr` / `MathNode`) has 26 node variants covering the union of all supported formats. Readers parse format-specific input into this AST; writers serialize it back out.

The optional render pipeline applies TeX typesetting rules (TeXBook Rules 11, 12, 13, 15, 18) to produce positioned layout boxes, then emits them as SVG.

## License

MIT
