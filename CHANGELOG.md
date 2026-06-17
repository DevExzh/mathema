# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

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

### Changed

- **Default render paths now use the faster arena layout engine.**
  `render_to_svg_compact` and `render_to_image_compact` (and therefore
  `convert(.., Svg)`, `to_png()`, `to_jpeg()`) route through the arena
  `LayoutBuilder` path, which the benchmarks measure as ~1.25–1.77× faster than
  the heap path. Output is unchanged (guarded by an existing equality test).

### Fixed

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
