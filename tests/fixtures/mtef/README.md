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

## From govdocs1

The `govdocs-*` files are complete `Equation Native` streams (*OLE*, with the
28-byte `EQNOLEFILEHDR`) of equations in documents of the govdocs1 corpus,
which consists of public US government documents. Each covers a MathType or
Equation Editor habit that Strict mode now accepts.

| File | Source zip | Member | Stream | MTEF | Exercises |
| --- | --- | --- | --- | --- | --- |
| `govdocs-v3-buffer-slack.bin` (OLE) | `zipfiles/231.zip` | `231/231818.doc` | `ObjectPool/_988461364/Equation Native` | 3 | 3 bytes of heap fill (`EF EF EF`) after the final END, inside the size the header declares |
| `govdocs-v5-buffer-slack.bin` (OLE) | `zipfiles/254.zip` | `254/254031.doc` | `ObjectPool/_1085980190/Equation Native` | 5 | non-zero slack after the final END (MathType 4.0) |
| `govdocs-v5-line-ruler.bin` (OLE) | `zipfiles/816.zip` | `816/816303.ppt` | PowerPoint OLE object at offset 272930 | 5 | LINE with `mtefOPT_LP_RULER`, its tab stops written without a RULER tag (MathType 5.2) |
| `govdocs-v5-bare-space-codes.bin` (OLE) | `zipfiles/802.zip` | `802/802059.ppt` | PowerPoint OLE object at offset 518834 | 5 | Spacing-style characters written as bare codes (MathType 5.0) |
| `govdocs-v5-fraktur.bin` (OLE) | `zipfiles/302.zip` | `302/302170.ppt` | PowerPoint OLE object at offset 2309673 | 5 | Euclid Fraktur letter as MTCode U+F001 with font position `B` |
| `govdocs-v5-script.bin` (OLE) | `zipfiles/456.zip` | `456/456574.doc` | `ObjectPool/_1140353549/Equation Native` | 5 | Euclid Math One letter as MTCode U+F10D with font position `N` |
| `govdocs-v5-symbol-arrow-extension.bin` (OLE) | `zipfiles/871.zip` | `871/871082.doc` | `ObjectPool/_1200312916/Equation Native` | 5 | Symbol-font arrow extension written as Adobe's U+F8E6 |
`Equation Native` streams of documents in the govdocs1 corpus (Digital
Corpora; documents of the U.S. government, public domain). Each file is one
unmodified stream; the table names the corpus archive and its member.

| File | Archive | Member | MTEF | Exercises |
| --- | --- | --- | --- | --- |
| `v3-govdocs-decimal.bin` (OLE) | `zipfiles/424.zip` | `424/424693.ppt` | 3 | number with a function-style decimal point |
| `v3-govdocs-tilde.bin` (OLE) | `zipfiles/868.zip` | `868/868817.doc` | 3 | ASCII tilde as the similarity relation |
| `v3-govdocs-prime.bin` (OLE) | `zipfiles/090.zip` | `090/090099.doc` | 3 | apostrophe as a prime |
| `v3-govdocs-leading-script.bin` (OLE) | `zipfiles/321.zip` | `321/321609.ppt` | 3 | leading-script template with nothing to lead |
| `v5-govdocs-colour.bin` (OLE) | `zipfiles/700.zip` | `700/700515.ppt` | 5 | COLOR_DEF and COLOR records |

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
