# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- LaTeX macro expansion now charges queued replacements, discarded selector
  branches, macro-body evaluation and token processing against a work budget.
  Recursive expansion depth is a typed expansion error, and intermediate token
  vectors avoid reserving their full source-byte length. Ordinary reusable
  macros retain their existing behavior within the bounded budget.

### Added

- LaTeX `\@binrel` applies the first argument's binary, relation, or ordinary
  spacing class to the second argument, including when the sample is a group.
- **OMML paragraphs.** `OmmlReader::parse_paragraph` reads an `m:oMathPara`
  as separate equations plus its `m:jc` justification (`OmmlParagraph`,
  `OmmlJustification`); `OmmlWriter::write_paragraph` writes them back.
- **OMML writer options.** `OmmlOptions` selects the host run-property
  dialect (`OmmlRunProperties::None`, the default, `WordprocessingMl` or
  `DrawingMl`) and an optional display block (`m:oMathPara` with `m:jc`).
  Available through `OmmlWriter::write_with_options` and
  `ConvertResult::to_omml_with_options`.

- `Error::MtefWrite`: the MTEF writer refuses output its reader would not
  read back (a writer defect) instead of returning it.
- **Binary MTEF output:** `ConvertResult::to_mtef_bytes` /
  `to_mtef_bytes_with_diagnostics` and `MtefWriter::write_bytes` write MTEF 5
  (MathType, `Equation.DSMT4`) or MTEF 3 (Equation Editor 3.x,
  `Equation.3`), optionally with the 28-byte OLE `EQNOLEFILEHDR`.
  `MtefWriteOptions` selects the version and the header's product identity;
  `MtefWriteOptions::from_info` keeps the kind of an equation read from MTEF.
- **Binary MTEF input:** `from_mtef_bytes` / `from_mtef_bytes_with` read bare
  MTEF data or an OLE `Equation Native` stream (the 28-byte `EQNOLEFILEHDR` is
  detected and validated) and apply the same depth check and normalization as
  `convert`. `ConvertResult::mtef_info` returns an `MtefInfo` with the MTEF
  version, platform, product and version, MTEF 5 application key and inline
  flag, and the OLE header.
- **Resource limits:** `Config::max_nodes` (AST nodes, default 1 000 000) and
  `Config::max_depth` (AST depth, default and ceiling 128), with
  `Error::LimitExceeded` and `LimitKind` naming the limit that was hit
  (`Nodes`, `Depth`, or `ListLength` for a node with more than 65 535
  children). The LaTeX, MTEF and UnicodeMath readers enforce the node limit
  while they build nodes. MTEF and UnicodeMath enforce the depth limit while
  they build nodes, and the depth validators now report `LimitExceeded` instead
  of `Unsupported`. LaTeX proof-tree, XY-diagram and `numcases` builders stop
  promptly when the node limit is reached instead of recovering into more
  nodes.
- Actuarial angles (`\angl` and `\angln`) retain their top and right rules in
  LaTeX, MathML, OMML, MTEF v5, UnicodeMath, and both SVG and raster rendering.
  Text-box arguments preserve upright text and inline math.
- LaTeX `\reflectbox` and `\mathreflectbox` preserve an explicit horizontal
  reflection through the AST and SVG/raster renderers. MathML uses a
  `data-mathema-reflect` marker with a CSS `scaleX(-1)` style so mathema can
  read its own output back.

### Changed

- LaTeX script arguments reuse canonical token grouping for mapped Unicode
  symbols, keeping preferred control words such as `\Re` stable across writes.

- CLI parsing keeps LaTeX source whitespace intact, including a trailing `\ `
  control-space command, while retaining trimming for other input formats.

- SVG and raster rendering reject non-finite or non-positive `font_size_px`
  values; raster rendering applies the same validation to its `scale` factor.

- MTEF writer rewritten to follow `MTEFv5.md` and `MTEFv3.md` and the layout
  MathType and Equation Editor write: header, MathType's default encoding,
  font and EQN_PREFS definitions (MTEF 5), typesize records where the size
  changes, script templates after their base, roots with the radicand before
  the index, big operators with main, lower and upper slots, the 0x10 / 0x20
  limit bits and their operator characters, fence, arrow and accent
  characters after template slots, piles and matrices, embellishments, and
  the final END. Leading scripts are written before their base, Far East
  text in the Far East text style with its font, and MT Extra font
  positions only where real files confirm them. Its output reads back in
  Strict mode; `to_mtef` still returns MTEF 5 as hex.

- **Breaking: list builders are fallible.** `AstArena::push_children`,
  `CompactExpr::from_math_expr`, `CompactParseResult::from_parse_result` and
  `normalize_compact_expr` return `Result` and refuse a node with more than
  `AstArena::MAX_CHILDREN` (65 535) children with `LimitKind::ListLength`.
  Parsers that cannot return an error where they build a list use
  `AstArena::push_children_deferred` and report it through
  `AstArena::check_limits`, which also reports `LimitKind::ArenaSize` when
  nodes, child references or string bytes would outgrow 32-bit indices.
- `from_latex_with`, `from_mathml_with`, `from_omml_with` and
  `from_unicode_math_with` now check `Config::max_depth`.
- **The OMML reader is strict.** In Strict mode malformed XML (mismatched or
  missing end tags, unclosed comments, truncated input, undeclared entities,
  duplicate attributes), unknown math elements, invalid property values and
  duplicated arguments are errors naming the element. Lenient mode skips them
  with a warning. Valid properties without an AST equivalent, including host
  run properties such as `w:sz` or `a:latin`, are dropped with an `Info`
  diagnostic naming them. Element nesting, child counts and matrix sizes are
  bounded, and the padding of short matrix rows and declared columns beyond
  the widest row are limited across the whole input in proportion to its
  length.
- **The OMML reader keeps more structure.** `m:eqArr` reads as an `Array`
  with `add_jot`: `&` alignment points in its rows become right/left
  alternating columns, rows without them a single centred column; `m:box` as a `MathAtom` (`Op` with `m:opEmu`);
  `m:nor` runs as `Text` with their whitespace; `m:func` as
  `Row[name, U+2061, argument]` (a plain `sin` run stays an operator);
  explicit `m:sty="p"` as upright `Styled`; `w:color` and DrawingML
  `a:solidFill` as color; math alphanumeric letters (as PowerPoint stores
  them) as base letters with a style. On/off properties without a value now
  mean "on", `m:phant` content is shown unless `m:show` is off (so Word's
  `\smash` stays visible), an empty `m:nary` character is refused instead of
  becoming an integral, and the `m:bar` and `m:d` defaults follow the schema.
- **The OMML writer output follows the schema.** `m:scr` precedes `m:sty`,
  `m:dPr` children are in schema order, required arguments are always
  present, `m:eqArr` (with `&` alignment points for aligned columns) and
  `m:box` are written back, `m:show` is written only to hide phantom content, operators are plain
  upright runs, text keeps its whitespace (`xml:space="preserve"`), and
  cancellations hide the box sides. Color is written only into the host run
  properties and never into `m:rPr`; without a host dialect it is reported as
  lost. Loss diagnostics now come from the writer itself.
- **`#RRGGBB` colors reach LaTeX** as `\textcolor[HTML]{RRGGBB}` instead of
  being dropped during normalization.
- **UnicodeMath matrices no longer drop content silently.** In Strict mode a
  `■(…)` matrix with content that is not an element (for example the `#` in
  `■(a#b)` or an unclosed `(`) is not read as a matrix, so the input from
  there on is kept as text, like other content the reader cannot parse.
  Lenient mode still skips such content and reports it with one warning per
  matrix (the number of skipped runs and their span), at most 32 warnings
  plus a summary. Whitespace between cells is still ignored. Matrix cells
  count towards `Config::max_nodes` as they are read, and a matrix whose
  padded rows would exceed 65 535 cells is refused with
  `LimitKind::ListLength`.
- **Invisible operators** (U+2061–U+2064) are written as zero-width
  `\\mathord{}` atoms in LaTeX and dropped in MTEF instead of being emitted
  as raw characters.

- `from_mtef` / `from_mtef_with` (hex input) now go through the binary path
  above, so they also accept `Equation Native` streams, normalize the AST and
  report `mtef_info`.
- MTEF reader rewritten to follow `MTEFv3.md` and `MTEFv5.md`: MTEF 3 and
  MTEF 5 template selectors are decoded with their own tables, script
  templates attach to the preceding item, function names become one operator,
  text and digit runs become one node, and fences, roots, big operators with
  limits (stored main, lower, upper), limit templates, matrices, piles,
  embellishments, arrows, braces, intervals, Dirac brackets, strikes and boxes
  map onto the AST. Fonts, sizes, nudges, rulers and colours are read and not
  kept.

### Fixed

- MathML text preserves supported `mathvariant` values. Both writers encode
  significant leading, trailing, and repeated spaces using nonbreaking spaces.
  The reader collapses ordinary XML whitespace in tokens.
- LaTeX text commands accept a single token, including in radicals and
  scripts. A bare macro's replacement stays in the text argument. Literal
  text characters and repeated hyphens or quotes survive LaTeX conversion.
- Operator names escape LaTeX comment and grouping characters, including
  percent signs, braces, and literal backslashes. Unclosed operator-name
  arguments now fail instead of consuming the remainder of the input.

- Malformed old-style LaTeX font switches (`\\rm`, `\\sf`, and related
  commands) no longer retry an unconsumed scoped argument until the AST grows
  without bound; recoverable failures now reach lenient recovery.
- LaTeX array preambles no longer expand an unbounded `*{n}{pattern}` repeat;
  oversized, overflowing, or deeply nested specifications fail promptly with
  a typed resource-limit error in both strict and lenient modes.
- Leading scripts kept their base only until normalization, which attached
  them to the atom before (`x={}^{14}_{6}C` became `x=^{14}_{6}`-style
  scripts on `=`, and MathML `mprescripts`, OMML `m:sPre` and MTEF leading
  scripts moved the same way). An empty-base script now becomes a leading
  script of the node after it; a script the LaTeX parser finds without a base
  after white space (`x '`) still attaches to the atom before it, and one
  after a big operator or a limit function (`<mo>∫</mo><msub><mrow/><mi>C</mi></msub>`)
  stays its limit, as one after a function name (`\sin {}^2 x`) stays its
  script.
- The LaTeX writer wrote a word symbol such as MathML `<mo>lim</mo>` as its
  first letter; it now writes the LaTeX command for the names LaTeX and
  amsmath define (`\lim`) and `\operatorname{…}` (`\operatorname*{…}` for
  limit-taking names) otherwise, also for names such as lcm that have no
  command.
- LaTeX invisible operators (U+2061–U+2064) no longer leave trailing
  whitespace or merge adjacent identifiers on a second write; the writer uses
  a stable zero-width `\mathord{}` atom to preserve their ordinary-token
  boundary.
- Literal Unicode minus signs retain their ordinary class when adjacent to
  ASCII minus operators, so LaTeX round-trips do not change `−-B` into a
  different binary-operator sequence.
- Text daggers in math expressions keep their ordinary spacing through LaTeX
  conversion. The writer uses `\dag` and `\ddag` for the ordinary forms and
  `\dagger` and `\ddagger` for the binary operators.
- LaTeX math accepts `\>`, `\enskip`, and the positive and negative named
  thin, medium, and thick spaces. Their widths survive conversion back to LaTeX.
- Literal Unicode math spaces survive LaTeX conversion at either end of a
  formula. U+2004 and U+2005 keep their one-third and one-quarter em widths.
- A script on `ℜ` or `ℓ` keeps the same grouping when its glyph is written
  as `\Re` or `\ell` and parsed again.
- Literal backslash symbols are written as `\textbackslash` rather than a raw
  backslash that could become a newline or control space when followed by
  another command.
- Repeated LaTeX limits and nolimits modifiers are consumed in order, with the
  last modifier determining placement for n-ary and named operators. Explicit
  placement is preserved when writing named operators back to LaTeX.
- `\text{\int}` lost its content; symbol commands in text keep their
  characters.
- UnicodeMath writes leading scripts as `(_sub^sup)base` (UTN #28 section
  3.3) and reads that form back; they were written as postscripts of the
  item before.

- Normalization merged runs of letters, digits or text quadratically (65 000
  letters used about 2 GB) and 32-bit string offsets could wrap; runs are now
  merged in linear time and arena growth past 32 bits is refused.
- Lenient OMML reported resource limits as an empty result with a warning;
  limits now fail in every mode.
- Lists longer than 65 535 nodes were silently truncated by every reader
  and by normalization (for example two OMML `m:e` of 40 000 runs flattened
  into one delimiter): they are now refused with `LimitKind::ListLength`.
- `CompactExpr::from_math_expr` no longer splits array rows that are wider
  than the column-alignment list.

- MTEF 5 equations decoded to an empty expression: EQN_PREFS was read with
  the wrong layout and definition records were given an options byte they do
  not have.
- MTEF 3 templates lost their structure: fence and operator characters after a
  template's slots ended the parent line early, and MTEF 5 selector numbers
  were applied to MTEF 3 data.
- MTEF record layouts: SIZE, PILE, MATRIX, RULER, FONT, COLOR_DEF, CHAR and
  MTEF 3 EMBELL (which carries a padding byte) are now read in full.
- Strict mode refuses unknown records, templates, embellishments and
  typefaces, truncated data, a missing final END and data after it instead of
  skipping them; Lenient mode reports each recovery as a warning. The MTEF
  writer does not yet write the final END, so its output reads in Lenient
  mode only.

### Security

- A MATRIX record could make the MTEF reader allocate every declared cell
  without reading input (a 128 KiB input used about 1.9 GB). Cells are now
  only created for records that are present, and node count and nesting depth
  are bounded by `Config`.
- The UnicodeMath reader backtracked exponentially: an unclosed bracket under
  a script, n-ary operator, radical or matrix was parsed again by every
  fallback at every nesting level (an 830-byte input used 10 GB and nested
  box operators could overflow the stack), and every enclosing unclosed
  matrix re-read the rest of the input (126 nested `■(` before 2 MB took
  18 s). It now parses in linear time and
  enforces `Config::max_nodes` and `Config::max_depth`, failing with
  `Error::LimitExceeded` in Strict and Lenient mode; nesting that used to fail
  with `Error::UnicodeMathParse` now reports `LimitKind::Depth`.

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
