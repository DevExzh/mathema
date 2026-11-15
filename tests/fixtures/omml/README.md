# OMML fixtures

Equations used by `tests/omml_round_trip.rs`.

## Hand-written

- `word_paragraph.omml`: a display-math paragraph in the form Word writes
  (`m:oMathPara` with `m:jc`, `m:ctrlPr` holding `w:rPr`, `w:rFonts` on runs,
  a `w:color` run, normal text with significant spaces, an equation array in
  braces, `m:func` around `m:limLow`, a skewed fraction, an integral with a
  differential box, a radical with a degree, an accent, an overbar,
  double-struck script and a bracketed matrix).
- `powerpoint_paragraph.omml`: a display-math paragraph in the form PowerPoint
  writes inside `a14:m` (DrawingML `a:rPr` run properties, math italic and
  bold italic letters stored as Mathematical Alphanumeric Symbols, an
  `a:solidFill` run color, `m:func` and normal text).

Both were written by hand from ECMA-376 Part 1, §22.1 and its schema; they
are not copied from any document.

## From plurimath

Source: plurimath (https://github.com/plurimath/plurimath), commit
`24732092bbb0cd9c26dead65eafdd57a8029757d`, directory
`spec/plurimath/fixtures/omml/`. Copied unchanged.

| File here                          | Upstream file |
|------------------------------------|---------------|
| `plurimath_047_nary_eqarr.omml`     | `047.omml`    |
| `plurimath_066_delimited_eqarr.omml`| `066.omml`    |
| `plurimath_075_func.omml`           | `075.omml`    |

License: BSD 2-Clause.

Copyright Ribose.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
