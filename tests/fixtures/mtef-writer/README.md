# MTEF writer fixtures

Equations with leading scripts (`tmSUB` / `tmSUP` / `tmSUBSUP` with
`tvSU_PRECEDES`) used by the MTEF writer tests. Each file is the
`Equation Native` stream (OLE header removed) of an OLE equation object
written by MathType 6, from the test suites of two Ruby projects, both
"Copyright (c) 2015 Jure Triglav", MIT License (licence text in
`../mtef/README.md`):

| File | Project | Path in the project |
| --- | --- | --- |
| `upstream-v5-leading-scripts.bin` | `mathtype` | `spec/fixtures/input/mathtype5/equation13.bin` |
| `upstream-v5-leading-scripts-sums.bin` | `mathtype_to_mathml` | `spec/fixtures/input/424.bin` |

`mathtype_to_mathml` renders both with `mmultiscripts` and `mprescripts`
(its `spec/fixtures/expected/equation13.xml` and `424.xml`): the scripts
belong to the item after them.
