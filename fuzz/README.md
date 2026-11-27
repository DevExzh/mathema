# Fuzzing

One target per reader, plus round trips and rendering (`cargo +nightly fuzz list`).
The corpora (`fuzz/corpus/`) and artifacts are not committed.

The LaTeX targets (`fuzz_latex`, `fuzz_latex_roundtrip`, `fuzz_layout`,
`fuzz_svg_embed`, `fuzz_render_paths`) have a dictionary and seeds, which
libFuzzer needs to reach
environments and packages it would not spell by chance:

```sh
mkdir -p fuzz/corpus/fuzz_latex
cargo +nightly fuzz run fuzz_latex fuzz/corpus/fuzz_latex fuzz/seeds/latex -- \
    -dict=fuzz/dict/latex.dict -max_len=2048
```

New finds go to the first directory. With `-fork=N`, run the built binary with
one corpus directory: merge the seeds into it first (`-merge=1 corpus
fuzz/seeds/latex`).

- `fuzz/dict/latex.dict` is generated from the reader's sources:
  `python3 scripts/gen_latex_dict.py`, after adding commands or environments.
- `fuzz/seeds/latex/` is written by hand, one construct or package per file;
  `tests/latex_fuzz_seeds.rs` checks that every environment of the dictionary is
  in one, and that each is read and stable.
- `fuzz/seeds/mathml/` and `fuzz/seeds/omml/` (for `fuzz_mathml`, `fuzz_omml` and
  `fuzz_omml_roundtrip`) have operators next to letters and to nodes that draw
  nothing: what the LaTeX writer must set apart from a control word.
