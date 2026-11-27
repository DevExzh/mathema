# mathema

Convert math formulas between **LaTeX**, **MathML**, **OMML**, **MTEF**, and **UnicodeMath** via a shared AST. Optionally render to **SVG**, **PNG**, or **JPEG**.

## Quick start

```rust
use mathema::{convert, Format};

// One-liner conversion
let mathml = convert("x^2 + y^2 = z^2", Format::LaTeX, Format::MathML).unwrap();

// One-liner conversion with reader and writer diagnostics
let (umath, diagnostics) = mathema::convert_with_diagnostics(
    r"\color{red}{x}",
    Format::LaTeX,
    Format::UnicodeMath,
).unwrap();
assert!(!umath.is_empty());
assert!(!diagnostics.is_empty());

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
| MathML | Yes (Presentation + Content) | Yes (Presentation only) |
| Office MathML (OMML) | Yes | Yes |
| UnicodeMath | Yes | Yes |
| MathType (MTEF 2 to 5) | Yes (MTEF 2, 3, 4 or 5; binary or hex; bare or OLE `Equation Native`) | Yes (MTEF 3 or 5; binary or hex; bare or OLE `Equation Native`) |
| SVG | -- | Yes (`render`) |
| PNG | -- | Yes (`raster`) |
| JPEG | -- | Yes (`raster`) |

> **MathML note:** input accepts both Presentation and Content MathML (Content
> is lowered to Presentation on read); output emits **Presentation MathML only**.

> **MTEF note:** `from_mtef_bytes` reads Equation Editor 3 (MTEF 3) and MathType 4+
> (MTEF 5) equations, with or without the OLE `Equation Native` header, and
> `ConvertResult::mtef_info` reports which one the input was. MathType's
> typesetting data (fonts, sizes, nudges, tab stops) has no place in
> the AST and is not kept. `ConvertResult::to_mtef_bytes` writes MTEF 5 or
> MTEF 3; `MtefWriteOptions::from_info(result.mtef_info())` writes an
> equation back as the same kind of object it was read from.

## CLI

```sh
cargo build --release --features raster   # or just --features render for SVG only

# Convert between text formats
mathema convert -f latex -t mathml "x^2"
echo "x^2" | mathema convert -f latex -t omml

# Read a raw MTEF or Equation Native stream from a file or stdin
mathema convert -f mtef --mtef-binary -t latex equation.bin
cat equation.bin | mathema convert -f mtef --mtef-binary -t mathml

# Keep conversion output on stdout; conversion warnings are shown on stderr
mathema convert -f latex -t unicodemath "\\color{red}{x}"
# Select the host dialect when writing OMML (`pure`, `word`, or `drawing`)
mathema convert -f latex -t omml --omml-dialect word "\\color{red}{x}"
# Add informational diagnostics too (and recover where possible)
mathema convert --lenient --diagnostics -f latex -t unicodemath "\\color{red}{x}"

# Render to SVG, PNG, or JPEG
mathema render -f latex "x^2"                              # SVG to stdout
mathema render -f latex --format png -o out.png "x^2"      # PNG (requires raster)
mathema render -f latex --format jpeg -o out.jpeg "x^2"    # JPEG (requires raster)
mathema render -f latex --font-size 24 --color black "x^2" # custom options

# Validate input
mathema validate -f latex "x^2"
mathema validate --lenient -f latex "\\unknowncommand{x}"

# Batch processing (pass multiple files)
mathema convert -t mathml -o out/ *.tex
mathema render --format png -o out/ *.tex
```

The `--from` / `-f` flag is optional; the format is auto-detected from file extension or content when omitted.
MTEF file arguments are treated as UTF-8 hex text by default. Pass
`--mtef-binary` (or its `--binary` alias) to read raw MTEF bytes, including an
OLE `Equation Native` header. The flag implies `--from mtef`, though an
explicit `--from` may still be used. The binary mode accepts files and stdin,
including streams that are not valid UTF-8. File and stdin reads are bounded
by the library's configured input-size limit before parsing starts.

SVG output is **self-contained** by default: glyph outlines from Latin Modern Math are embedded as `<path>`/`<use>` elements, so the SVG renders identically everywhere (no fonts to install). It is rasterized from the same glyph outlines as the PNG/JPEG output and uses the same layout, so the two match closely (pixel-level luminance parity within ~8%, since SVG is rasterized via `resvg` and PNG/JPEG via `tiny-skia`).

Format names: `latex` (`tex`), `mathml` (`mml`), `omml`, `mtef`, `unicodemath` (`um`), `svg`, `png`, `jpeg` (`jpg`).

### LaTeX output profile

LaTeX output is a math fragment. The default profile keeps the existing
KaTeX-oriented spelling. Select `LatexProfile::PdfLaTeX` with
`Config::with_latex_profile` when compiling with pdfLaTeX. It protects colored
box bodies with `\ensuremath` and emits hexadecimal colors with xcolor models.
The portable baseline for ordinary formulas, colors, and the writer's common
structures is:

```tex
\documentclass{article}
\usepackage{amsmath,amssymb,xcolor}
\begin{document}
$ x^2 + y^2 = z^2 $
\end{document}
```

Replace the sample expression between the dollar signs with the generated
fragment.

Some optional writer features require additional packages. `cancel` enables
cancel marks, `graphicx` enables reflection, `mathtools` enables selected
bracket accents, and `arydshln` enables dashed table rules. Add those packages
only when the corresponding constructs are present. Hexadecimal box colors
are emitted with xcolor's `HTML` model, so they remain valid in this profile.

## Features

| Feature | Description |
|---|---|
| `render` | SVG output via a built-in TeX-style layout engine, including standalone SVG with embedded Latin Modern glyph outlines |
| `raster` | PNG/JPEG output (implies `render`) |
| `font-embed` | Alias of `render` (kept for forward compatibility); standalone embedded-outline SVG is already available under `render` |
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

The shared AST (`MathExpr` / `MathNode`) models the union of all supported formats. Readers parse format-specific input into the heap AST; writers serialize it back out. `CompactExpr` stores the same expression in an arena, with compact-only helper nodes for table alignment and fenced separators plus side metadata for typesetting attributes and source provenance.

The optional render pipeline applies core TeXBook typesetting rules (radicals — Rule 11; accent skew — Rule 12; axis centering — Rule 13a; fractions — Rule 15; sub/superscript positioning — Rules 18a/c–f) to produce positioned layout boxes, then emits them as SVG. The `raster` feature adds PNG/JPEG rasterization via `fontdue` + `tiny-skia`. Standalone SVG embeds glyph `<path>` outlines for rendering without external fonts; this is available under the `render` feature (the `font-embed` feature is currently an alias of `render`).

## License

The Mathema source code is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details. `src/unicode/math_class.rs` is generated from Unicode data, used under the Unicode License V3; see [LICENSE-UNICODE](LICENSE-UNICODE).

The optional render feature embeds Latin Modern text and math fonts. Those third-party fonts are released under the GUST Font License. The exact upstream license texts are included in [LICENSE-LATIN-MODERN.txt](LICENSE-LATIN-MODERN.txt) and [LICENSE-LATIN-MODERN-MATH.txt](LICENSE-LATIN-MODERN-MATH.txt), with the referenced [LPPL 1.3c text](LICENSE-LPPL-1.3c.txt). The project manifests and byte-level provenance are documented in [NOTICE-FONTS.txt](NOTICE-FONTS.txt).
