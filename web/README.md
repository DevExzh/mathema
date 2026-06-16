# Mathema Web

A [SvelteKit](https://kit.svelte.dev/) single-page app that serves the
**Mathema** math-formula **renderer** and **format converter** entirely in the
browser via **WebAssembly**. There is no backend — the `mathema` Rust crate is
compiled to `wasm32-unknown-unknown` and runs client-side.

## Features

- **Render** any supported input to a self-contained SVG (Latin Modern Math
  glyph outlines are embedded, so it looks identical everywhere — no web fonts
  required).
- **Convert** between LaTeX, MathML (Presentation **and** Content), OMML,
  UnicodeMath and MTEF.
- Live preview, display/inline toggle, strict/lenient parsing, adjustable font
  size, parse diagnostics, AST node count, and one-click copy of every output.

## Prerequisites

- Rust toolchain with the `wasm32-unknown-unknown` target
  (`rustup target add wasm32-unknown-unknown`)
- [`wasm-pack`](https://rustwasm.github.io/wasm-pack/)
- [Bun](https://bun.sh/) 1.3+ (the preferred package manager / runtime)

## Build & run

```sh
cd web
bun install           # install JS deps

# 1. Build the WebAssembly package (outputs to src/lib/wasm/).
#    Re-run this whenever the Rust crate changes.
bun run wasm

# 2a. Dev server with hot reload
bun run dev

# 2b. Production build (static site in ./build) + local preview
bun run build
bun run preview
```

`bun run wasm` is a convenience wrapper for:

```sh
wasm-pack build .. --target web --out-dir web/src/lib/wasm --release \
  -- --no-default-features --features "wasm render"
```

> The generated `src/lib/wasm/` directory is a build artifact (git-ignored).
> Run `bun run wasm` after cloning before `bun run dev`/`bun run build`.

## How it works

| Layer | File |
|-------|------|
| WASM bindings | `../src/wasm/converter.rs` (`MathemaConverter`, `ParsedMath`) |
| WASM loader   | `src/lib/mathema.js` — loads the module once and exposes `process()` / `convert()` |
| UI            | `src/routes/+page.svelte` |

`ParsedMath.toSvg(fontSizePx, display)` produces the embedded-outline SVG; the
other `to*` methods produce the textual conversions. Everything is parsed once
per input change and re-serialized to every format without re-parsing.

The app is configured as a fully static site (`adapter-static`,
`ssr = false`), so the output of `bun run build` can be hosted on any static file
server or CDN.
