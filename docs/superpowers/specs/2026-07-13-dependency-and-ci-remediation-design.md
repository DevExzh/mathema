# Dependency Upgrade and CI Remediation Design

## Objective

Upgrade Mathema's Rust, Bun, and GitHub Actions dependencies while preserving
the declared Rust 1.88 minimum supported Rust version (MSRV), then fix every
GitHub Actions failure without weakening the repository's validation gates.
All work will be developed on the standalone
`chore/deps-ci-2026-07` branch and verified both locally and on GitHub.

## Current Evidence

The branch starts at `origin/main` commit `611e7df`. The latest CI run for
that commit is GitHub Actions run `27909989785`, which fails in four areas:

- `test`: `katex_corpus_no_new_unsupported_commands` assumes the ignored
  `tests/data/katex_corpus.json` file exists in a clean checkout.
- `clippy`: Criterion 0.8 deprecates `criterion::black_box`, and
  `-D warnings` promotes all occurrences in the benchmark targets to errors.
- `build (library-only, no default features)`: `tests/cli.rs` executes even
  when the optional `cli` feature and binary are absent.
- `fuzz smoke`: `fuzz_latex_roundtrip` finds writer output that cannot be
  parsed again, initially demonstrated by input `~` producing `\\ `.

The open dependency PRs do not introduce these failures; they inherit them
from `main`. PR #3 updates part of the Bun stack, and PR #4 updates
`quick-xml` and `indicatif`. Both are already behind newer compatible
versions, so the standalone branch will supersede their version sets instead
of merging the stale branches.

The primary checkout also contains an unpushed commit and unstaged parser and
writer changes created while investigating these same CI and fuzz failures.
Those changes are evidence and reusable work, but they will be transferred
only after their relevance is verified. The primary checkout and its dirty
state will not be modified.

## Dependency Policy

### Rust

- Keep `package.rust-version = "1.88"` and edition 2024.
- Upgrade every direct Cargo dependency to the newest stable release that is
  compatible with Rust 1.88 and the current public API.
- Accept semver-compatible transitive upgrades in `Cargo.lock`, using the
  Rust 1.88 toolchain/resolver so the lockfile cannot silently require a newer
  compiler.
- Include the security-relevant `quick-xml` 0.41 upgrade and the latest
  compatible `indicatif` 0.18 release.
- Do not adopt a newer major merely to make the version number larger when it
  requires abandoning the declared MSRV or changing Mathema's behavior.

### Web

- Upgrade the SvelteKit 2, Svelte 5, Vite 8, Svelte Vite plugin 7,
  `svelte-check` 4, and Node 26 type packages to their latest stable
  releases.
- Refresh `web/bun.lock` with the repository's declared Bun package manager
  and verify frozen installation.
- Keep TypeScript 6 unless the current Svelte/Vite toolchain explicitly
  supports TypeScript 7 during implementation. TypeScript 7 is a separate
  major migration, not a lockfile refresh.

### GitHub Actions

- Enumerate every `uses:` reference in `.github/workflows`.
- Use `gh api` to confirm current stable action releases/tags.
- Upgrade obsolete action majors where the new major is compatible with the
  existing workflow contract, particularly actions still running on the
  deprecated Node 20 runtime.
- Preserve workflow permissions, triggers, environments, cache semantics, and
  artifact paths.

## CI Remediation

### Hermetic Corpus Tests

Corpus-dependent tests will detect whether their intentionally untracked input
exists. A clean checkout will skip only the external-corpus assertion with an
explicit message; checked-in unit and regression tests will continue to run.
The workflow will not download mutable test data or mark the entire test
target ignored.

### Feature-Correct CLI Tests

`tests/cli.rs` will be compiled and executed only when the `cli` feature is
enabled. Default-feature CI continues to exercise every CLI test, while
`--no-default-features` validates the library without trying to locate a
binary that Cargo was instructed not to build.

### Criterion Compatibility

Benchmark targets will import `std::hint::black_box` and retain Criterion
only for benchmark registration and measurement. No warning suppression will
be added.

### Fuzz Round-Trip Correctness

Each reproducible fuzz failure will become a deterministic regression case.
The parser/writer contract is:

1. Parsing valid or recoverable input must not panic or exhibit unbounded
   work.
2. Writer output claimed to be LaTeX, OMML, or UnicodeMath must parse again.
3. A parse-write-parse-write cycle must stabilize under the fuzz target's
   equivalence rules.

Existing local fixes for whitespace escaping, script arguments, delimiters,
font switches, symbol classification, OMML text, and bounded parenthesis
lookahead will be evaluated against individual reproductions. Only changes
that address a demonstrated failure and preserve existing semantics will be
retained. Parser limits will bound pathological inputs rather than merely
increasing CI timeouts.

## Implementation Sequence

1. Transfer the already committed CI/fuzz remediation from the primary
   checkout and verify its exact diff on the standalone branch.
2. Reproduce every known baseline failure, add or identify its regression
   coverage, and verify the scoped fix.
3. Transfer the remaining uncommitted fuzz-hardening changes one behavior at
   a time, adding deterministic regressions before retaining each fix.
4. Update Cargo manifests and lockfile under Rust 1.88, resolving API or lint
   fallout without changing the MSRV.
5. Update the Bun manifest and lockfile, then verify type checking, WASM
   integration, and the production static build.
6. Update compatible GitHub Actions pins after confirming releases with
   `gh api`.
7. Run the complete local verification matrix.
8. Push the standalone branch, open a draft pull request, and inspect checks
   and logs with `gh`. Repeat root-cause diagnosis and focused fixes until
   every GitHub Actions check passes.

## Error Handling and Scope Control

- Dependency incompatibilities will be handled at their source: API
  adaptation, feature selection, or a documented compatible version. Warning
  suppression and removal of validation jobs are not acceptable fixes.
- Fuzz failures will be minimized and converted to regressions before parser
  changes are accepted.
- CI-only environmental differences will be reproduced locally where
  possible. When they cannot be reproduced, job logs and workflow inputs will
  be used to isolate the failing boundary.
- Unrelated changes in the primary checkout remain untouched.
- No release, merge, or modification of the existing Dependabot PRs is part
  of this task. The deliverable is a separate branch and draft PR.

## Verification Matrix

The local gate consists of:

- `cargo fmt --all --check`
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo test`
- `cargo test --all-features`
- `cargo build --no-default-features`
- `cargo test --no-default-features`
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm`
- `cargo +1.88.0 check --all-features`
- `cargo audit`
- `cargo +nightly miri test --lib ast`
- a bounded run of every target returned by `cargo +nightly fuzz list`
- `bun install --frozen-lockfile`, `bun run check`, `bun run wasm`, and
  `bun run build` in `web/`

If a required local tool is unavailable, it will be installed or the exact
unverified gate will be reported before pushing. Local success is necessary
but not sufficient: final acceptance requires the draft PR's GitHub Actions
checks to complete successfully and `gh pr checks` to show no failures.

## Acceptance Criteria

- The standalone branch contains the dependency updates and scoped CI fixes.
- Cargo resolves and builds with Rust 1.88.
- Rust, Bun, and compatible GitHub Actions dependencies are current under the
  policies above.
- Every deterministic regression and local verification command passes.
- Every GitHub Actions check on the draft PR is green.
- The primary checkout's existing commit and unstaged changes remain intact.
