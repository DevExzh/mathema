# MTEF fixtures

Binary MathType equations used by `tests/mtef.rs`. `golden.txt` holds the
LaTeX each file converts to (`name: latex`, one line per file).

Files are bare MTEF data unless marked *OLE*, in which case they are complete
`Equation Native` streams that start with the 28-byte `EQNOLEFILEHDR`.

## From `trigonometric_ratios.doc`

Extracted from `trigonometric_ratios.doc`, a document authored by the project
owner; no external source, no licence restrictions. Each file is the
`Equation Native` stream of the named OLE storage. Goldens were checked against
a PDF rendering of the same document.

| File | Storage | MTEF | Exercises |
| --- | --- | --- | --- |
| `v3-function-names.bin` (OLE) | `ObjectPool/_1234567890` | 3 | function names, function-style punctuation |
| `v3-fraction-root-script.bin` | `ObjectPool/_1234567964` | 3 | fractions, root with null index, superscripts, MT Extra symbols |
| `v3-brace-matrix.bin` | `ObjectPool/_1234567944` | 3 | left brace with its glyph, matrix, content after the matrix |
| `v3-pile.bin` | `ObjectPool/_1234568171` | 3 | pile as the whole equation |
| `v3-subscripts.bin` | `ObjectPool/_1234568236` | 3 | subscript templates, function-style braces |
| `v3-bars.bin` | `ObjectPool/_1234568347` | 3 | absolute-value bars with private-use glyphs |
| `v3-brace-bar-text.bin` (OLE) | `ObjectPool/_1234568554` | 3 | nested fences, left bar only, Far East text, zero padding after END |
| `v3-over-arrow.bin` | `ObjectPool/_1234568556` | 3 | over-arrow template |
| `v3-function-powers.bin` | `ObjectPool/_1234568093` | 3 | superscript on a function name |
| `v3-equation-system.bin` | `ObjectPool/_1234568099` | 3 | two brace-and-matrix systems |
| `v5-fraction.bin` (OLE) | `ObjectPool/_1234567891` | 5 | definitions, equation preferences, fraction |
| `v5-bars-sqrt.bin` | `ObjectPool/_1234567921` | 5 | bar fence, square root, superscripts |
| `v5-interval.bin` | `ObjectPool/_1234568000` | 5 | interval template `(…]` |
| `v5-parens-functions.bin` | `ObjectPool/_1234568127` | 5 | paren templates, function-start flags, powers of functions |
| `v5-subscript.bin` | `ObjectPool/_1234568366` | 5 | subscript template |
| `v5-explicit-font.bin` | `ObjectPool/_1234568476` | 5 | FONT_STYLE_DEF and explicit-font characters |
| `v5-text-and-fonts.bin` (OLE) | `ObjectPool/_1234568477` | 5 | text and Far East text runs, explicit fonts |
| `v5-degrees-text.bin` | `ObjectPool/_1234568481` | 5 | degree signs, Far East punctuation |
| `v5-nested-fractions.bin` | `ObjectPool/_1234567972` | 5 | fractions inside fractions |
| `v5-plus-minus.bin` | `ObjectPool/_1234567915` | 5 | Symbol-style operators |

## From MIT-licensed test suites

The `upstream-*` files cover structures the document above does not use. They
are the `Equation Native` streams (OLE header removed) of OLE equation objects
from the test suites of two Ruby projects, both
"Copyright (c) 2015 Jure Triglav", MIT License:

| File | Project | Path in the project |
| --- | --- | --- |
| `upstream-v3-limit-sum-embellishment.bin` | `mathtype` | `spec/fixtures/input/mathtype3/lim_embell.bin` |
| `upstream-v5-integrals.bin` | `mathtype_to_mathml` | `spec/fixtures/input/integrals.bin` |
| `upstream-v5-fences.bin` | `mathtype_to_mathml` | `spec/fixtures/input/fences.bin` |
| `upstream-v5-embellishments.bin` | `mathtype_to_mathml` | `spec/fixtures/input/embellishments_roots_long_divisions.bin` |
| `upstream-v5-sumop-limits.bin` | `mathtype_to_mathml` | `spec/fixtures/input/299.bin` |
| `upstream-v5-matrices.bin` | `mathtype_to_mathml` | `spec/fixtures/input/matrices.bin` |

Their goldens were checked against what those projects expect for the same
files: the decoded records (`mathtype`) or the MathML (`mathtype_to_mathml`).

Licence of the `upstream-*` files (identical in both projects):

```text
Copyright (c) 2015 Jure Triglav

MIT License

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```
