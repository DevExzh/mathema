# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **Fixed a stack-overflow denial-of-service in the MTEF reader.** Deeply
  nested MTEF input (PILE/template/line recursion) had no parse-depth limit and
  could abort the process with a stack overflow before the post-parse
  `validate_compact_depth` ran — a ~70 KB input (well inside the 4 MiB budget)
  was enough. The reader now enforces `MAX_PARSE_DEPTH = 128` during parsing,
  matching the other four readers, and rejects deeper input with an error.
- **`MtefReader::parse_bytes` now honors `Config::max_input_bytes`.** The public
  raw-bytes entry point previously bypassed the input-size limit; oversized byte
  slices are now rejected with `Error::InputTooLarge`.
- **Fixed a remotely-triggerable denial-of-service in macro expansion.**
  `\newcommand`/`\def` bodies that expand to a self-replicating macro (e.g.
  `\newcommand{\a}{\a\a}\a`) previously grew the working string exponentially —
  the 64-iteration cap did not bound output size, so a ~30-byte input could
  exhaust memory and CPU. Expansion output is now capped (per input, at least
  1 MiB) and exceeding it returns a `LatexParse` error instead of hanging.

### Added

- **Configurable input-size limit at all reader entry points.** `Config` gains
  a `max_input_bytes` field (default 4 MiB, `None` to disable). Inputs larger
  than the limit are rejected with `Error::InputTooLarge` before any
  tokenization or parsing work is done.
- **`Error::InputTooLarge` and `LatexParseError::ExpansionLimit`** error
  variants.
- **MSRV policy:** the crate now declares `rust-version = "1.88"` (required by
  the `let`-chains it uses) in `Cargo.toml`.
- **Lint gates:** a `[lints]` table plus a CI workflow that runs
  `cargo fmt --check`, `cargo clippy --all-features -- -D warnings`, the test
  suite, and a fuzz smoke job.
- **Publish hygiene:** `Cargo.toml` now declares `repository`, `homepage`,
  `readme`, `keywords`, and `categories`, plus an `exclude` list that drops the
  multi-megabyte format specs, internal planning docs, demo web/WASM apps,
  debug examples, and test fixtures from the published tarball (172 → 111
  files).
- **CI coverage for the library-only and docs paths:** new jobs run
  `cargo build`/`cargo test --no-default-features` (the common
  `default-features = false` consumer path) and
  `cargo doc --no-deps --all-features` with `-D warnings`.
- **Crate-level `//!` documentation** on `lib.rs` with a runnable quick-start
  example and a feature-flag overview.
- **`wasm32` and `cargo audit` CI jobs:** a new job builds
  `--target wasm32-unknown-unknown --no-default-features --features wasm`, and
  another runs `cargo audit` against the dependency tree.
- **MTEF `tmBOX` and `tmSTRIKE` support (reader + writer).** The reader now maps
  `tmBOX` → `Boxed` and `tmSTRIKE` → `Cancel` (with stroke direction from the
  variation bits) instead of warning-and-flattening, and the writer emits both
  templates for `Boxed`/`Cancel` nodes instead of silently dropping the box /
  cross-out. Reader coverage is now 35 of 38 selectors.
- **Idempotent round-trip proptests for OMML and MTEF**, plus a `fuzz_omml_roundtrip`
  target; the proptest AST generator now covers all 30 `MathNode` variants and
  all 14 text styles, so the "writers never panic" properties exercise the full
  AST. (UnicodeMath remains a deliberately lossy linear format and is not
  asserted idempotent — see the test note.)

### Changed

- **The SVG compact render path uses the arena layout engine.**
  `render_to_svg_compact` (and therefore `convert(.., Svg)`) builds layout
  through the arena `LayoutBuilder`. The `~1.25–1.77×` figure reported by the
  `layout` benchmark is **layout-only with a warm, reused arena**; the default
  public API allocates a fresh `LayoutArena` per call (a cold arena), so the
  end-to-end SVG speedup is small. Output is unchanged (guarded by an existing
  byte-equality test against the heap path).
- **Note on raster:** PNG/JPEG (`to_png()` / `to_jpeg()`,
  `render_to_image_compact`) currently lay out through the heap `LayoutBox`
  path (`layout_expr_c`) and do **not** yet go through the arena, so they do not
  receive the arena layout's reuse benefit. The parse/convert pipeline's compact
  speedups (parse `~1.88×`, writers `2.5–4.7×`) are unaffected and measured
  separately.

### Fixed

- **AST variant count corrected from 27 to 30.** The `MathNode` doc comment,
  `README`, and the constructibility test (`all_27_variants_constructible`) all
  understated the enum, omitting `Rule`, `RaiseBox`, and `MathAtom`. The count
  is now 30 and the renamed `all_30_variants_constructible` test constructs all
  of them.
- **UnicodeMath now absorbs parenthesized arguments after `√`/`∛`/scripts.** Per
  UTN#28, `√(x+y)` and `x^(a+b)` use the parentheses as a grouping delimiter;
  the reader previously kept them as a literal `Fenced`, so re-serializing
  double-wrapped them (`√(x)` → `√((x))`). The parentheses are now absorbed.
- **`fuzz_mtef` feeds raw bytes** through `parse_bytes` instead of requiring
  valid UTF-8, so the fuzzer exercises the binary parser rather than mostly the
  hex-decode error path.
- **`render_to_svg_compact` rustdoc** no longer advertises the bare
  `~1.25–1.77×` figure; it now matches the CHANGELOG's corrected, scoped wording
  (layout-only, warm-arena-only; the default cold-arena path gains little).
- **The infallible `preprocess` is now `pub(crate)`** (removed from the public
  API) with a doc note that it is not DoS-safe and that untrusted input must use
  the fallible `preprocess_cow`.
- **MTEF reader no longer silently drops template-based accents and limits.**
  The `tmHAT`, `tmTILDE`, `tmVEC` (vector, incl. direction/harpoon/under
  variations), `tmARC`, `tmLIM` (lim with lower/upper limit slots), and
  `tmDIRAC` (bra/ket/bra-ket) templates now map to the corresponding AST nodes
  instead of being flattened into a bare row. Any *still*-unsupported template
  selector now emits a `DiagnosticKind::Warning` (content is still preserved by
  flattening) so structural loss is observable rather than silent.
- **Clippy lint gate is green again.** The
  `#[allow(clippy::too_many_arguments)]` was attached to the zero-argument
  `partial_deslant_skew()` instead of the 8-argument `draw_glyph`, so
  `cargo clippy --all-features --all-targets -- -D warnings` failed; the
  annotation is now on the function that actually needs it.
- **OMML phantom subtypes are no longer collapsed to `Full`.** `<m:phant>`
  honors `m:show`/`m:zeroAsc`/`m:zeroDesc`/`m:zeroWid` to round-trip
  `Horizontal`/`Vertical`/`Full` phantoms.
- **MTEF writer no longer flattens `PreScripts` into full-size inline nodes;**
  pre-subscripts/superscripts are now emitted through the MTEF sub/sup script
  template on an empty base, preserving their positioning. `Phantom` and
  `MathChoice` (which have no MTEF template) keep their content/display branch
  instead of being dropped.
- **UnicodeMath/MathML style round-trips recover all 13 text styles.**
  `unstyled_char` now reverses BoldItalic, Script, BoldScript, Fraktur,
  BoldFraktur, and every SansSerif variant in addition to the original four.
