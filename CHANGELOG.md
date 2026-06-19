# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-19

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

### Performance

- **The default compact SVG path now reuses a pooled layout arena.**
  `render_to_svg_compact` / `render_to_svg_standalone_compact` (and therefore
  `convert(.., Svg)`) borrow a per-thread `LayoutArena` instead of allocating a
  fresh cold arena per call, so the default public API captures the
  allocation-reuse benefit that previously required callers to manage their own
  arena via `render_to_svg_compact_arena`. Output is byte-identical (guarded by
  `render_to_svg_compact_arena_matches_heap`).
- **PNG/JPEG rasterization now uses the arena layout engine.**
  `render_to_image_compact` (and therefore `to_png()` / `to_jpeg()`) lays out
  through the compact `LayoutBuilder` + `LayoutArena` and rasterizes directly
  from the arena (`rasterize_layout_arena` / `emit_raster_box_arena`), instead
  of the separate compact→heap `layout_expr_c` engine. Raster and SVG now share
  a single layout pass, eliminating the SVG/raster layout-divergence risk; the
  compact→heap layout engine is now production-dead (retained only for
  `#[cfg(test)]` heap-vs-arena equality guards). Output is byte-identical to the
  old heap raster path (guarded by `arena_raster_matches_heap_quadratic`).
- **Discoverability of the zero-copy fast path.** `ConvertResult::expr()` /
  `into_expr()` now document that they materialize a full legacy `MathExpr`
  heap tree (~60% of a parse) and steer throughput-sensitive callers to
  `compact_expr()` + the `*_compact` writers/renderers.
- **Removed the dead compact→heap layout engine.** Now that raster lays out
  through the arena, the compact→heap layout engine (`layout_expr_c` /
  `layout_node_c` and ~30 `*_c` helpers) had no production callers, so it was
  deleted — shrinking `src/render/layout.rs` from 13,035 to ~10,324 lines and
  removing one of the three near-isomorphic layout families (a maintenance and
  binary-size liability flagged by the assessment). The `#[cfg(test)]`
  heap-vs-arena byte-equality guards now use the legacy `MathExpr→heap` engine
  (`layout_expr`) as the reference, which is what real `&MathExpr` callers use.
  No production layout/emit/raster behavior changed (test counts unchanged:
  default 736, all-features 1532, no-default 711).
- **Known render shortcomings (documented, not yet fixed):** end-to-end SVG is
  *emit-bound* (SVG emit is ~73% of render time, so the arena layout win is
  small end-to-end); and the compact UnicodeMath writer is ~30% slower than its
  (trivial) legacy counterpart due to the fixed arena-indirection floor.
  Profiling/optimizing SVG emit is tracked as a dedicated follow-up.
- **Legacy and arena layout engines reconciled (retirement now unblocked).** A
  new gate, `legacy_vs_arena_render_divergence_baseline` (`tests/render_svg.rs`),
  renders every formula in the KaTeX + small corpora through both the legacy
  `MathExpr→heap` engine and the arena engine (via `from_math_expr`). It began at
  **28 of 135** diverging *complex* formulas (a pre-existing gap between the two
  production engines, not introduced by the compact→heap removal); all 28 were
  reconciled to **0** by the arena-engine fixes listed under **Fixed**. The two
  engines now produce byte-identical SVG everywhere, so `render_to_svg`/
  `render_to_svg_standalone` can be re-implemented on the arena engine and the
  legacy `layout_node` engine retired (the remaining step toward a single layout
  engine); the gate guards against any regression.

### Changed

- **The SVG compact render path uses the arena layout engine.**
  `render_to_svg_compact` (and therefore `convert(.., Svg)`) builds layout
  through the arena `LayoutBuilder` (now over a pooled, reused arena — see
  **Performance**). The `~1.25–1.77×` figure reported by the `layout` benchmark
  is **layout-only**; because SVG emit dominates end-to-end render time, the
  full-pipeline speedup is small regardless. Output is unchanged (guarded by a
  byte-equality test against the heap path).
- **Raster shares the SVG arena layout.** PNG/JPEG (`to_png()` / `to_jpeg()`,
  `render_to_image_compact`) now lay out through the same compact arena
  `LayoutBuilder` as SVG (see **Performance**), rather than a separate
  compact→heap engine. The parse/convert pipeline's compact speedups (parse
  `~1.88×`, writers `2.5–4.7×`) are unaffected and measured
  separately.

### Fixed

- **Silent corruption: unbraced infix `\over`/`\atop`/`\above`/`\choose` leaked
  a `__infix_*__` sentinel into output.** `check_infix_fraction` was only called
  inside braced groups, so top-level plain-TeX like `a \over b` emitted
  `<mi>__infix_over__</mi>` instead of an `<mfrac>`. It is now applied to the
  top-level node list and recurses into numerator/denominator, so chained
  operators (`1 \over 2 \over 3`) also resolve without leaking. Regression tests
  added (`reader/latex.rs`, `tests/find_unsupported.rs`).
- **`AstArena` unchecked `unsafe fn` accessors are no longer public API.**
  `node_unchecked`/`children_unchecked`/`str_unchecked` are now `pub(crate)`
  (they had no external callers); a `miri` CI job validates the arena's
  `unsafe` blocks for UB.
- **MTEF parse errors are distinguishable.** New `Error::MtefParse` /
  `Error::UnicodeMathParse` variants replace the previous generic
  `Error::Unsupported` for malformed MTEF/UnicodeMath input.
- **MTEF writer data loss is now observable.** `Writer::write_compact_with_diagnostics`
  reports `DiagnosticKind::Warning` for nodes the MTEF writer drops (Rule, color,
  `Scaled`/`RaiseBox`/`MathChoice`/`MathAtom`, box colors), matching the reader.
- **`find_unsupported` gained honest output-level gates** that the AST heuristic
  cannot provide (`__infix_*__` sentinel detection; wrapper commands preserve
  visible content).
- **`cargo fmt --check` is clean** (cleared the `glyph_variants.rs` drift); stale
  "26 AST variants" corrected to 30 in `tests/round_trip.rs`.
- **Legacy and arena layout engines now render every corpus formula
  byte-identically (28 → 0 divergences).** A regression gate
  (`legacy_vs_arena_render_divergence_baseline`) renders the KaTeX + small
  corpora through both engines; reconciling them fixed a series of real
  arena-engine (default `convert(.., Svg)`/compact path) bugs:
  - **Math-alphabet double-italic:** `\mathit{x}` / `\mathnormal{x}` (and
    italic-fallback letters in `\mathbb`/`\mathcal`/… groups) were remapped to
    the italic Unicode codepoint (𝑥, U+1D465) *and* given `font-style="italic"`,
    double-slanting an already-italic glyph; the styled path now decides
    faux-italic like the normal identifier path.
  - **Styled-group child duplication:** `layout_styled_arena` held the shared
    children buffer open across composite sub-layouts, so e.g. `\mathbb{\breve a}`
    rendered its content twice; switched to the normal HBox two-phase pattern.
  - **Missing inter-atom spacing** in styled groups (e.g. the medium space
    around the binary `+` in `\mathbb{\imath+}`).
  - **Over/under decoration heights:** `\underline` rules were too thick, and
    `\overbrace`/`\underbrace`/`\overbracket` used wrong heights; unified all
    over/under-script sites on a shared `stretchy_overunder_deco_h` table
    (brace 0.548, bracket 0.44, overline 0.08).
  - **Invisible struts:** zero-area rules (e.g. `\\[1em]` row struts) are no
    longer emitted as `width="0.000"` rects.
  - **`\cr` row separator:** array/matrix cells now stop at `\cr` (previously it
    was swallowed as a `Space`, collapsing rows/columns).
  - **Stretchy-delimiter target height** is quantized to a 1e-9 grid so the two
    engines' f64 accumulation order cannot flip a 4-decimal path coordinate.
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
