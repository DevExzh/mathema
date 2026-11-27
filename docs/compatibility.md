# External compatibility checks

The normal test suite checks Mathema's internal readers and writers. The
external subset in `scripts/external_compat.sh` adds checks against the tools
that consume generated output.

The script expects these programs to be installed:

- `pdflatex`, which compiles the public `LatexProfile::PdfLaTeX` test;
- `xmllint`, which performs XML Schema validation; and
- `cargo`, to build and run the checked-in writer tests.

The LaTeX profile test uses the public `Config::with_latex_profile` API and
declares `amsmath`, `amssymb`, and `xcolor`, matching `README.md`. Optional
writer constructs require additional packages and are kept outside this gate.

The MathML 3 and ECMA-376 schemas are intentionally kept outside the source
tree. Set `MATHEMA_MATHML_XSD` to the local MathML 3 schema and
`MATHEMA_OMML_XSD` to `shared-math.xsd`. The directory containing that file
must contain the complete imported ECMA XSD set. The review workspace contains
reference copies in
`target/review/parent/w3c-mathml3` and
`target/review/parent/official-ecma-transitional`.
Set `MATHEMA_XML_XSD` to W3C's XML namespace schema so the ECMA XML import is
validated against the real namespace declaration. The checked-in review copy
is `target/review/parent/xml.xsd`.
The CI job downloads the W3C MathML 3 archive
`mathml3-xsd.zip` (SHA-256
`921cba0eaf1ec3c2756849425d0ca4c7b2d5f549eda7dd1e0d7f1351ba173a8d`) and
the ECMA-376 Part 4, fifth-edition archive (SHA-256
`bd25da1109f73762356596918bf5ff8b74a1331642dba5f1c1d1dfc6bed34ecd`).

Run the full subset from the repository root:

```sh
MATHEMA_MATHML_XSD=/path/to/mathml3.xsd \
MATHEMA_OMML_XSD=/path/to/shared-math.xsd \
MATHEMA_XML_XSD=/path/to/xml.xsd \
  scripts/external_compat.sh
```

The command compiles representative LaTeX output with `-halt-on-error` and
`-no-shell-escape`, validates generated Presentation MathML against MathML 3,
and runs the pure OMML, WordprocessingML, and DrawingML writer paths. Pure
OMML and WordprocessingML are checked as complete schema documents. The
DrawingML host run properties are removed before the shared-math schema check,
because that schema does not contain the host extension. A separate assertion
rejects Word-only `m:mcJc` in DrawingML output. The command returns a failure
when an external consumer rejects output. This keeps the check suitable for a
release gate while allowing ordinary library builds to run without TeX or XML
schema packages.
