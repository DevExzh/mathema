# UnicodeMath fixtures

Inputs found by fuzzing the UnicodeMath reader (`fuzz/fuzz_targets/fuzz_unicode_math.rs`)
with libFuzzer; generated data, no external source, no licence restrictions. Each one
made the reader backtrack exponentially (unclosed brackets nested under scripts,
n-ary operators, radicals and matrices), taking seconds and gigabytes of memory.
`tests/unicode_math_limits.rs` parses each one and requires a quick, bounded result.

The `oom-*` files ran out of memory under the fuzzer; the `slow-unit-*` files exceeded
its per-input time limit. The suffix is the start of libFuzzer's SHA-1 name for the input.
