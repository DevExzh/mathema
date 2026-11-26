# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking (behavior):** letters that are separate in the source stay
  separate. `convert`, `normalize_expr` and `normalize_compact_expr` joined
  neighbouring ASCII letters into one identifier, which MathML draws upright:
  `ab` became `<mi mathvariant="normal">ab</mi>` and is now
  `<mi>a</mi><mi>b</mi>`, as TeX and `from_latex(…).to_mathml()` mean it. OMML
  gets a run per letter and UnicodeMath parentheses where a row of letters is an
  operand (`(ab)/c`). Only the letters of operator names (`\varinjlim`) are
  joined. A list of more than 65,535 letters is refused as any other list of
  that length is, also where groups are opened into one.
- The argument of a function (OMML `m:func`, a MathML `mrow` after U+2061) stays
  one node through normalization: `sin⁡(2x)` written as OMML no longer has `x`
  outside the function, MathML keeps the `mrow`, and UnicodeMath writes
  `sin⁡〖2x〗`. Conversion to LaTeX writes it without a group, as before.
- OMML reader and writer: a function name in several runs (`er`, `f`) is one; what
  a name has above, below or beside it is not joined to a word.
- **Breaking (behavior):** the letters of an italic OMML run (`sty` i, or none,
  which is Word's default) are identifiers of their own; a run of `sty` p is one
  as before, and so is the name of an `m:func`.
- The OMML writer writes an identifier of several letters upright (`sty` p), as
  MathML draws `<mi>kr</mi>`; it wrote it italic. One in an italic style is
  written as an italic run, whose letters come back as identifiers of their own.
- The MathML writer spells the style on the letters of `\mathrm`
  (`<mi mathvariant="normal">`), as MathML Core has no `mathvariant` on
  `mstyle`, and that of a styled word on its `mi`, which was upright.
- The heap normalizer normalizes trees with typesetting attributes as their
  compact form. MTEF SIZE records with nothing added are read as the typesize
  records they spell out.
- The scripts of `\underbrace`, `\overbrace`, `\underbracket` and
  `\overbracket` are their labels in every format (`munder`, `m:limLow`, LaTeX
  `\underbrace{x}_n`, MTEF brace templates); they were subscripts of a group,
  `{\underbrace{x}}_n`, which TeX and KaTeX set beside the brace. A label on
  the other side is written `\overset{n}{\underbrace{x}}`. The scripts of a
  braced `{\underbrace{x}}` are now set beside it.
- A brace, overline or underline over `\mathop{…}` keeps its command, and a
  lone `⏞`/`⏟` over or under something is written `\overbrace`/`\underbrace`.
- Rendering: an operator without an operand has no thin space after it.
- A `/` is an ordinary symbol and a `:` a relation, as in TeX and KaTeX, in
  every reader; they were a binary operator and an ordinary symbol. `a/b` is
  written without a space, an ordinary colon `{:}` and a punctuation one `\colon`.
- Rendering: `{…}`, `\frac`, `\boxed` and accents are ordinary atoms and the dots
  on the line inner ones, spaces do not hide the atoms on their sides from each
  other, what follows a big operator is spaced against its operand, `\middle`
  has no space around it, `\colon` has amsmath's, `!` and `?` close,
  `\underbrace` and its like are operators, and a binary operator at the end of a
  list or before a relation or a closing is ordinary.
- Rendering: scripts are drawn with the script designs of Latin Modern Math (OpenType
  `ssty`), in the sizes and widths of XeTeX; TeX's space after scripts and beside
  fractions does not shrink in scripts.
- Rendering: fractions in text are spaced as scripts are, without spaces around
  operators and relations.
- Rendering: the invisible operators U+2061 to U+2064 take no space, so a function
  applied to a parenthesis has none.
- Rendering: labels of `\overset`, `\underset`, `\stackrel` and the arrows are as far
  away as TeX's, with its space outside of them.
- LaTeX: a relation, binary operator or punctuation in braces among other symbols
  (`1{,}000`, `a{=}b`) stays an ordinary atom, without spaces; the output spells it
  `\mathord{=}`.
- Rendering: a displayed large operator is the font's variant at the size of the text;
  it was scaled up to 1.6 or 2 times, wider than its box, and an operator in text
  larger than 1.2 used it too.
- Rendering: the box of `\sum` and the other displayed operators is that of the drawn
  glyph, so their limits are as close as TeX sets them with the font; scripts at
  their side stay where pdfTeX puts them.
- Rendering: the delimiters of `\binom` and `\genfrac` have TeX's two sizes and do not
  grow with what is inside.
- LaTeX: `\pod{6}` is `(6)`; it was read as `\pmod{6}`.
- Rendering: the left-aligned columns of `aligned`, `align`, `split` and the like
  start with an ordinary atom, as in amsmath: `a &= b` has a space before `=`.
- Rendering: an arrow or equals sign is drawn at its natural width under or over
  a label that fits (`\overset{!}{=}`, `\xrightarrow{f}`), not 2.25em wide; an
  equals sign under one up to twice as wide (`\overset{\text{def}}{=}`).
  Angle brackets past the largest glyph are that glyph.
- Rendering: `\boxed`, `\fbox`, `\colorbox` and `\fcolorbox` have the padding of
  TeX (3pt, and 0.4pt of frame, drawn inside the box), not 1pt, and a
  `\colorbox` or cell colour has no frame.
- Rendering: the delimiters of `\left…\right`, `\binom` and matrices and the
  radical sign are the font's glyphs at their own size, as TeX takes them, not
  the next ones scaled to fit; there is no padding inside the delimiters, a null
  one has its space, and matrix columns are 1em apart. A superscript in a
  cramped style (`\sqrt{x^2}`) is as low as in TeX.
- Rendering: a letter's box includes its italic correction, as in TeX, so `T)`,
  `f_1` and `\frac{V}{Y}` no longer collide. A superscript (primes too) follows
  the correction, a subscript does not, and that of an integral sits under its
  slope. Advances are those of the drawn Latin Modern Math glyph (most Greek
  letters, `\infty`, `\forall`, the dots and the integrals were too narrow), and
  an accent is centred on the font's attachment point (`\hat d`, `\vec V_1`) and
  as narrow over a wide letter. The bar of a fraction is inside its null
  delimiters.
- `\textcolor{red}{\sum}`, `\colorbox` and `\fcolorbox` with just an operator
  in braces are read like the operator token (`\textcolor{red}\sum`).
- LaTeX output spells more combining accents as commands (`\overline`,
  `\underline`, `\overleftarrow`, `\overrightharpoon`, …), and the rest of
  U+0300–U+036F by code (`\overset{\char779 }{a}`), which KaTeX accepts. The
  UnicodeMath reader sets the marks that belong below (U+0323, U+0327, U+0331,
  …) below, and the OMML reader reads them back.
- UnicodeMath: the label of `\overbracket{x}^n` is written `〖x┴⎴〗┴n`, and the
  reader accepts quoted text as a script or bound (`x_"max"`).
- An operator token in a fence is written with its body, as in a row.
- `\sideset{_1}{}\sum x` and other operators with leading scripts are written
  with an empty operand, `{{}_{1}\sum}{}x`, where the next node would be read as
  theirs.
- `{\sum}x` is read as the ordinary atom braces make (`\mathord{\sum}x`), without
  the space an operator has.
- UnicodeMath groups an accented base under a bracket or accent
  (`〖x┴⎴〗┴⎴`).
- Normalization reaches every nesting level the depth limit accepts; converted
  output of deep tables and proofs no longer changes on a second pass, where the
  reader takes it back (nearly 64 nested tables are at its limit). `normalize_expr`
  also drops zero-width spaces.
- Text with many style groups is read in linear time.
- A character other than ASCII after `@` in `CD` no longer panics.
- Deeply nested tables and scripts of sizing commands are refused before they
  overflow the stack.
- LaTeX output keeps a script without a base before a literal closing
  delimiter where it is (``\left/ {}^{\prime} \middle. \mathclose{\char`\~}
  \right.``), and dropped or empty text in a text box no longer splits or loses
  what is around it.
- An empty `array` preamble is read as the preamble, so a rule after it
  (`\begin{array}{}\hline …`) is kept; it was read as the first cell.
- A sizing command before something that is no delimiter (`\big\alpha`) leaves
  it in its own size in every output, as TeX does; LaTeX output already did.
- Scripts on both sides of a `\middle` delimiter stay beside it
  (`\left( {}_{a} \middle| {}_{b} \right)`); they moved the delimiter into a
  group, where TeX rejects it.
- In a text box, text in one face around dropped characters is written as one,
  and a combining mark no longer joins a math shift or brace of inline math.
- A space after `\reflectbox{…}` or `\verb|…|` in a text box is kept, as in TeX.
- LaTeX output of a lone combining mark set over or under something that is no
  accent (`\xrightarrow{…}`), of `\sideset` without an operator, of
  `\overset{}{\mathop{…}}` and of scripts beside `\mathop{\max}` reads back as
  written.
- `normalize_expr` flattens groups in the content of a fence, as the compact
  normalizer does. It keeps `\idotsint` in one piece, and a `\middle` below
  the fence's own list becomes the symbol it names.
- A chain of commands that take each other without braces (`\hat\hat…`,
  `\big\big…`, `\sum\sum…`) has a depth limit in the reader; a long one
  overflowed the stack.
- MTEF 2 to 4 do not record the full size of an equation: sizes in points are
  kept and drawn relative to an assumed 12 pt.
- Rendering: a leading script is placed by its nucleus alone, as TeX sets
  `{}_a t_b`; trailing scripts pushed it further down or up.
- MTEF 5 reader: the bold and italic an equation's preferences define for the
  text, function, variable, Greek, symbol, number and first user styles are
  read, as the pictures of equations draw them: `\mathbf{x}`, `\boldsymbol{x}`,
  `\mathrm{x}`, `\textbf{…}`, `\mathbf{\sin}`, `\mathbf{12}`, `\boldsymbol{+}`.
  Function names with limits and other symbols stay plain. The MTEF 5 writer
  defines bold or italic text, function names and upper-case Greek where all of
  an equation have it (punctuation, in the Function style otherwise, then has a
  font of its own), and reports a loss otherwise; it had written them plain,
  function names without a word.
- MTEF reader and writer: digits, and the symbols `+ - = < > ( ) [ ] / , . : ; ! * | %`,
  in a bold explicit font are bold (`\mathbf{1}`, `\boldsymbol{+}`), as the
  pictures of equations draw them. MTEF is written so for `\mathbf` around a
  number of several digits and for `\boldsymbol` around these symbols.
- MTEF writer: smashed content (`\smash`, `\smash[b]`, `\smash[t]`) is written,
  with an `Info` diagnostic that its height is not kept; it was dropped. A
  dropped `\vphantom`, which draws nothing, is an `Info` and no longer a warning.
- MTEF 2 and 3 reader: bold and italic of explicit fonts were read the wrong way
  round. The description of MTEF 3 has the two bits swapped; the pictures stored
  with equations show it (`docs/mtef-legacy.md`). MTEF 4 and 5 are unchanged, and
  so is MTEF 3 this crate wrote, which has only upright and bold italic fonts.
- MTEF reader and writer: rewriting settles after one pass in more equations.
  A limit template without limits, and coloured parts of a template, join
  their line; a prime belongs to the nucleus in its colour whatever the colours
  of the scripts between (`x_a'` with a black `a`); an embellished prime is the
  glyph and as many primes; leading scripts of nothing are scripts on nothing.
  Trees as read, before normalization, have fewer zero spaces and groups.
- MTEF reader: leading scripts and scripts on scripts in other colours keep
  them; primes drawn by many templates are joined in linear time.
- MTEF writer: `'s` before a letter is written with a zero space that keeps the
  apostrophe.
- Pure OMML: colors are dropped before runs are joined, by `convert` and
  `ConvertResult` alike, so text and numbers that only a color kept apart are
  one run, as they read back: neighbouring digit groups told apart only by
  their colors become one number (`16`, `10`, `16` are written `161016`).
  WordprocessingML and DrawingML output keeps the colors and keeps them apart.
- MTEF writer: stretchy bar accents over several letters no longer warn as
  narrow accents, and MTEF 5 writes a strike under an accent in its own
  direction.
- The MTEF writer writes an accent on one styled letter (`\tilde{\mathbf A}`,
  also over `\mathfrak`, `\mathcal` capitals and `\boldsymbol`) or on a struck
  letter as the character's embellishment, as on a plain letter; it came back
  as the wide accent in MTEF 5 and as a limit in MTEF 3, mostly silently. A
  narrow accent over more than one character, which MTEF has only as a wide
  one, is now a reported loss.
- Rewriting MTEF settles after one pass in more cases: an INTOP or SUMOP
  template without limits is read as loose items of its line (so a prime it
  draws is the prime of what precedes it), slots that hold only a zero space
  are empty, and a lone point after a number is kept apart from the digits
  after it. MTEF 5 colour is no longer lost on a base with both leading and
  trailing scripts.
  A prime or script after such a template is its last item's, a prime is not
  the script of an empty leading script, a zero space before it does not come
  between, and scripts on nothing lead the first item of a coloured run. Trees
  nested too deeply for normalization, and some scripts on nothing next to
  another colour, can still take a second pass.
  Near the depth limit, the MTEF writer refuses a tree whose reading could
  not be written again.
- The MTEF reader accepts more real MathType output in `Strictness::Strict`:
  the short buffer slack after the final END of an `Equation Native` stream
  (an error before; unless it decodes as equation content), MTEF 5 tab stops
  written without a RULER tag, and Spacing-style characters written as bare
  codes. In Lenient mode that slack is now an Info diagnostic instead of a
  Warning.
- The MTEF reader reads Fraktur letters and script capitals in Euclid Fraktur
  and Euclid Math One, and Adobe's private-use Symbol pieces, as what they
  are (kept as private-use characters with a warning before). The MTEF 5
  writer writes `\mathfrak` letters and `\mathcal` capitals in those fonts
  (written as plain variables before); small script letters and MTEF 3 stay
  a reported loss.
- In MTEF, leading scripts stay outside the trailing scripts of the same base
  (`{}_a b_c`), and limits that draw nothing are dropped at once, with a
  writer warning, instead of one nesting level per write.
- MTEF labelled double arrows (`⇌`, `⇄`) keep both arrows; the reader kept
  only the first. Rewriting MTEF no longer adds zero-width spaces before some
  leading scripts.

- UnicodeMath: a bar pair, bra, ket or `⟨⟩ ⌈⌉ ⌊⌋` group is a whole operand
  (`|x|/|y|`, `√|x|`, `x_|a|`, `∑|a+b|/c`); a bar that pairs with nothing is
  a delimiter (`P(A|B/C)` is `A|\frac{B}{C}`); Unicode script characters are
  scripts (`x²`, `a₁`, `10⁻³`), `'` is a prime, U+2061 binds a function's
  argument, `½` is an operand, `√abc` takes the whole operand, and a point that is
  not a decimal point is punctuation (`a.b`). `x_(a)b`
  no longer takes `b` into the subscript. `■(@)` keeps an empty last row.
- MathML: the leading scripts of a big operator are on the operator, not on
  its operand.
- UnicodeMath: every math character has its Unicode math class where no
  LaTeX command gives one (`a≉b/c` is `a≉\frac{b}{c}`); `\not` writes the
  struck character (`≉`, `≐̸`); fence content is separated like any list
  (`\left(a\frac{b}{c}\right)` read back as `(\frac{ab}{c})`); `|` and `‖`
  open, close, divide or are script bases by UTN 28 section 3.1 (`|x/y|` is
  `|\frac{x}{y}|`), and `|ψ⟩` and `⟨ψ|` are kets and bras; wide accents are
  written as combining marks in every context; `√(n&x)` no longer turns what follows into
  text, and `∜x` is read.

- **Security:** LaTeX output writes a color argument only for text that is a
  color (a name, `#hex`, an xcolor expression). `\colorbox{\input{a}}{x}` was
  written back verbatim.
- **Security:** `UndefinedCommands::Keep` no longer writes any engine primitive
  (except those that only typeset in place), any kernel command that defines,
  hooks, loads or ends something, `\begin` and `\end`, or more package
  commands live. It is still not a sandbox.
- **Breaking (behavior):** a color that is none is dropped from LaTeX output: a
  `\textcolor` or a box that only had that background becomes its content.
  Braced names (`\colorbox{{red}}{x}`) count as none. The other formats keep
  such colors: normalization no longer drops `#f00`, `#rrggbbaa`, `hsl(…%)`, ….
  `LatexWriter::write*` given an expression directly writes such a node as the
  atom `\mathord{…}`.
- **Breaking (behavior):** `\verb` is written as `\texttt` with character codes,
  since it is unsafe in an argument.
- **Breaking (behavior):** characters that are LaTeX syntax are escaped
  everywhere in LaTeX output (large operators, numbers, delimiters, names).
  Control characters in text and function names are dropped, and
  `ConvertResult::to_latex_with_diagnostics` reports them. A raw backtick in
  LaTeX math is the opening quote `‘`, as in TeX and KaTeX, and a raw `"` is
  read and written as itself (under babel's german, where `"` is active, use
  `\shorthandoff{"}`).
- **Breaking (behavior):** `\hbar`, `\hslash`, `\ell`, `\Re`, `\Im`, the
  Hebrew letters, `\imath`, `\jmath` and `\eth` are identifiers (MathML
  `<mi>`, as KaTeX and MathJax write them) instead of ordinary symbols.
- **Breaking (behavior):** MTEF equations convert more faithfully. Each
  variable-style letter is its own identifier (`dt` was one upright `<mi>`);
  decimals are one number; the ASCII hyphen, tilde, apostrophe and backslash
  become `−`, `∼`, a prime and `∖` outside text; empty scripts and limits are
  dropped; a leading script with nothing to lead becomes a script of the item
  before it, also before a fence template or function name. Colours (MTEF 5) and dashed partition lines are read and
  written; dotted lines are read as dashed. Colours are kept as they are:
  equations from dark slides may be pure white, which is invisible on a white
  page. OMML output drops colour by default, as before, with a diagnostic;
  `OmmlRunProperties` keeps it on text runs. The diagnostic about colour
  moved from reading to the writers that drop it. An apostrophe before a lone
  `s` (`ν_e's`) stays an apostrophe; the hyphen of a compound typed as math
  (`S-wave`) is read as a minus sign.
- The MTEF writer reports identifiers of several characters (MathML
  `<mi>rate</mi>`), which it writes as separate variables, and `convert` no
  longer joins letters (`dx`) on the way to MTEF. OMML output reports colour
  it drops from delimiters, operators, rules and accents.
- MathML `<mo>` holding one of those letter-like symbols (`<mo>ℜ</mo>`) is
  read as the letter.
- The LaTeX writer no longer braces a font command that carries a script
  (`\mathbf{F}_{x}`).

- **Breaking (behavior):** the UnicodeMath reader follows more of Unicode
  Technical Note 28. A script operand is the whole run of letters and
  digits (section 2.2): `a_ij` is `a_{ij}` (was `a_i j`) and `x^2y` is
  `x^{2y}`, ending at a function name (`x^2sin x` is `x^{2}\sin x`, and
  `x_max` is `x_{\max}`); a sign starts one (appendix A `soperand`): `e^-x` is
  `e^{-x}` (was `e^{-}x`) and `x^-1` is `x^{-1}`. `·` (U+00B7), `•` and
  `*` are binary operators (table 3.1), so `a·b/c` and `a*b/c` no longer
  fall back to text. `/` before an operator negates it (section 4.1):
  `a/=b` is `a≠b`, `a/∈B` is `a∉B` (was a fraction). In an n-aryand a
  function name takes its argument after a space (section 3.5): `∑ log k`
  sums `log k` (was `log`), and a space before the differential ⅆ does
  not end an integrand (`∫ sin x ⅆx`). The space characters of section
  3.16 (U+2009, U+2005, U+2003, …) are kept as spaces everywhere (they were
  dropped outside brackets, so `x`, U+2009, `dx` read as the word `xdx`). The
  UnicodeMath writer writes such script operands without parentheses
  (`a_ij`, `e^-x`), writes LaTeX spaces with the character of their width
  (`\;` is U+2005, `\:` U+205F; the negative `\!` is the zero-width space
  U+200B, read back as an empty atom), keeps a sign-only script apart from
  an operand after it (`x^- y`), and breaks a word that would read as a
  function name (`i nf`). A script run goes on through a span of letters
  that only ends in a function name (`x_kmax`, `x_max1`). The minus
  sign U+2212 is a binary operator, as `-` is: `a−b/c` is `a-\frac{b}{c}`
  (was `\frac{a-b}{c}`, which LaTeX `a-\frac{b}{c}` became). Every
  operator, relation, arrow and delimiter a LaTeX command spells has its
  class (`a≲b/c` was `(a≲b)/c`, `⟹` was text). Accents and
  scripts on words, fractions, scripted atoms and n-ary operators group
  them, and a script that is a radical or is parenthesized is written in
  parentheses.

- **Breaking:** the public AST enums `CompactNode`, `ColumnAlign`,
  `FractionKind`, `StrokeKind`, `PhantomKind`, `SpaceBasis`, `MatrixKind`,
  `TextStyle` and `SymbolKind` and the structs `MathExpr` and `CompactExpr`
  are now `#[non_exhaustive]`, so later releases can add variants to these
  enums and fields to these structs. Code outside the crate must add a
  wildcard arm (`_ => …`) to a `match` on these enums, and build
  `MathExpr` / `CompactExpr` with their constructors (`MathExpr::new`,
  `MathExpr::inline`, `MathExpr::block`, `CompactExpr::new`,
  `CompactExpr::from_math_expr`) instead of a struct literal; their fields
  stay public to read, and a struct pattern needs `..`. The fields of each
  variant, `FencedItem`, `OperatorAttr`, `OperatorNameForm`,
  `MathAtomType`, `LimitStyle`, `DisplayStyle` and `AccentPosition` stay
  exhaustive: a change to them is still breaking.
- **Breaking (behavior):** `MathNode` has two new variants,
  `MathNode::SizeSwitch(u8)` (`\tiny` … `\Huge`, MathML `mathsize`) and
  `MathNode::LineBreak` (`\\`, `<mspace linebreak="newline"/>`).
  `ConvertResult::expr()`, `into_expr()` and serde output now carry them
  where they produced an empty `Row` and a zero-width `Space` before, so
  the heap AST no longer loses text sizes and line breaks
  (`CompactExpr::from_math_expr` and `to_math_expr` convert them both
  ways).
- **Breaking (behavior):** `\above` and `\abovewithdelims` need a
  dimension, as in TeX ("Missing number") and MathJax ("Missing
  dimension"): `{a \above b}` is now an error under `Strictness::Strict`
  ("missing dimension after \above"); `Strictness::Lenient` warns and draws
  the default rule. As in KaTeX and MathJax the dimension may be braced
  (`\above{1pt}`); a number without a unit (`\above 1`) is an error like
  TeX's "Illegal unit of measure" (lenient: read as points). A length
  register named like one (`\fboxrule`, `\arrayrulewidth`, `\jot`, …) is a
  dimension: drawn at the default thickness with a warning, where it was
  left in the denominator; a structural command (`\end`, `\right`, `\\`)
  never is. A missing `\…withdelims` delimiter is a specific strict error
  ("missing delimiter after \overwithdelims"); lenient, it is the null
  delimiter, as TeX inserts `.`. An infix fraction command where one atom
  is required (`x^\over y`, `\sqrt\over`, `\frac\over b`) is a strict error
  ("misplaced \over") and a lenient warning; it was dropped silently. This
  follows TeX: KaTeX accepts `\frac\over b`, reading its argument as a
  group.
- **Breaking (behavior):** a second infix fraction command in one list
  (`a \over b \over c`) is a strict error ("ambiguous \over after \over:
  you need another { and }"), as in TeX and KaTeX. Lenient warns and, as
  before, lets it divide the denominator (`\frac{a}{\frac{b}{c}}`).
- **Breaking (behavior):** a second infix fraction command after a
  `\color` switch read inside another is now also that error
  (`\color{red} a \over b \color{blue} c \over d`, or one on each side
  of a `\middle`), as in TeX and KaTeX; it was accepted silently as a
  nested fraction.

- **Breaking (behavior):** the LaTeX reader takes exactly one TeX token or
  one brace group as a script argument, as TeX, KaTeX and MathJax do. A
  number is no longer one token: `x^22` now reads as `x^{2}2` (was
  `x^{22}`) and `x^2.5` as `x^{2}.5` (was `x^{2.5}`); in MathML `x^22` is
  `<msup><mi>x</mi><mn>2</mn></msup><mn>2</mn>`. This applies to every
  script argument: after `^`/`_`, after primes (`x'^22`), on `\limits`
  operators (`\sum_10^20` is `\sum_{1}` with body `0^{2}`, then `0`), in
  `\sideset` groups and prescripts. A control sequence is one token
  together with the arguments it reads (`x^\frac12` is
  `x^{\frac{1}{2}}`, `x^\alpha\beta` is `x^{\alpha}\beta`). Write the
  braced form (`x^{22}`) for the whole number. `Strictness::Strict` still
  accepts `x^22` (it is valid TeX); `Strictness::Lenient` adds a warning
  such as "only the first digit `2` is the superscript; write `^{22}` to
  raise the whole number" (at most 100, then a summary). A `\sideset`
  group may hold material after its scripts: as in amsmath,
  `\sideset{_12}{}\sum` is prescript `1`, then `2`, then the operator, and
  the right group's leftover follows the operator (`\sideset{_a b}{}\sum`,
  rejected before, now reads the same way). The mhchem and siunitx
  rewrites brace the scripts they generate (`\ce{C6H12O6}`,
  `\SI{3}{m^{22}}` are unchanged), and `\pu` now braces a `^(-?\d+)`
  exponent as mhchem does: `\pu{123 m^22}` keeps `m^{22}`, and
  `\pu{kJ mol^-1}` reads as `mol^{-1}` (it was `mol^{-}1`).

- **Breaking (behavior):** a script after an explicit empty group keeps that
  group as its base, as in TeX, KaTeX and MathJax. `x{}_1` is x followed by an
  empty atom carrying the subscript; normalization attached a trailing one to
  the node before it and the LaTeX writer wrote every such script onto the
  node before it, so `x{}_1`, `x{}^2`, `x{}'` and `x {}_1^2` became `x_{1}`,
  `x^{2}`, `x^{\prime}` and `x_{1}^{2}` in every format and in layout (also
  `x\sideset{_1 {}_2}{}\sum`, whose `_1` moved onto x). They are now written
  `x{}_{1}`, and so on; MathML keeps `<mi>x</mi><msub><mrow></mrow><mn>1</mn></msub>`.
  A script after white space (`x '`) still attaches to the atom before it,
  as in TeX, and an empty-base script before a node (`{}_1 x`) is still its
  leading script. One exception remains, unlike TeX: after a big operator or
  a function name that takes limits, such a script is still its limit or
  script (`\lim{}_x`, `\max{}_x` and `\operatorname{f}{}_1` become
  `\lim_x`, `\max_x` and `\operatorname{f}_1`, in normalization and in the
  MTEF writer), so that MathML such as `<mo>∫</mo><msub><mrow/><mi>C</mi></msub>`
  keeps its limit. The UnicodeMath writer spells an empty base as the empty
  invisible group `〖〗` (it wrote `()`, which reads back as visible
  parentheses), and the reader reads `x〖〗_1` back as `x{}_1`.

- `\big`, `\Big`, `\bigg` and `\Bigg` delimiters render with the OpenType
  MATH table's pre-designed size variants (or an extensible assembly) at TeX's
  1.2/1.8/2.4/3.0 em instead of a scaled base glyph, centred on the math axis,
  and stay text-size inside scripts as in TeX and KaTeX. **Breaking:** the
  public `render::layout::BoxKind::Delimiter` gains a `scale` field (the font
  size its glyph is chosen at), so exhaustive pattern matches on it must add
  `..` or the field.

### Fixed

- LaTeX: scripts before a word (an identifier of several letters, as MathML's
  `<mmultiscripts><mi>va</mi>…`) are written on its first letter, as they are read
  back; the rewritten text changed from pass to pass.
- Rendering: limits of large operators and labels of `\overset`, `\underset` and the
  arrows have the gaps of Latin Modern Math's MATH table, as in XeTeX; a fraction
  above `\sum` sat 0.09em low.
- LaTeX: `\bbsum`, which is written `\mathop{⅀}` for KaTeX, is read back as the
  operator, with its scripts and operand; the rewritten text differed from pass
  to pass next to primes, scripts and groups. `left=` and `right=` of empheq are
  math of its own (`left={f(x)=\empheqlbrace}`), where only empheq's
  `\empheqlbrace`, … are delimiters as tall as the display, as in pdfTeX; they
  were literal text, and `left=(` was a tall parenthesis. A compound symbol as a delimiter of `\genfrac` is a null
  one, with a warning.
- Security: a control word of the LaTeX output no longer takes in letters that
  follow it when what was between them writes nothing: the base of a leading
  script made `\int` + `eractionmode` into `\interactionmode`. A space is put
  there whatever was written in between, and before `@`, a letter to KaTeX,
  and letters and marks outside ASCII, which XeTeX and LuaTeX take as part of
  the name (`\partialΧ`).
- The MathML writer no longer drops size declarations: `\large` writes
  `<mstyle mathsize="120%">` around the content it scales, inside the
  `mstyle` of a math-style switch, again after a line break and across a
  `\middle`. MathML sizes are relative to the inherited size and TeX's are
  not, so `\small` inside `\large` is `75%`, and a physical kern
  (`\kern10pt`) keeps its width. An operator at the edge of the element
  has `form="infix"`, and a group (`{\large +}`) an `mrow`. The MathML
  reader maps the `mathsize` of `mstyle`, `math`, `mi`, `mn`, `mtext` and
  `ms`, given as a percentage, a unitless multiple, an `em`/`ex` length or
  `small`/`normal`/`big`, back to the declaration. A size between two
  declarations takes the nearer one with a warning; an absolute size
  (`mathsize="12pt"`), an invalid one, and one on another element are
  dropped with a warning.
- A space that is a TeX length (`\kern3pt`) keeps it through MathML, in
  `data-mathema-kern="3pt"` on its `mspace`, used as long as the `width`
  is still the kern's; it read back as `\hspace{0.3em}`.
- Rendering applies a size declaration inside `\left…\right`, inside font
  commands (`\mathbf{\large x}`) and after a line break, as KaTeX does.
- The heap UnicodeMath writer brackets a physical space in a fraction as
  the compact one does.
- The LaTeX reader scopes size declarations as TeX does: `{\large}a` sizes
  nothing, `\int \large x` and `\int \scriptstyle x` apply to `x` (the
  declaration was taken as the integrand), and `\mathbf{\large \begin{matrix}…\end{matrix}b}` sizes the
  table and `b`.
- Normalization no longer opens a group holding a size declaration:
  `convert` read `{\large x}y` as `\large xy`.
- Rendering a heap `MathExpr` applies size declarations and stacks the
  lines of a row with line breaks, as rendering the compact AST does.
- The MathML reader reads every `mspace` width unit: `ex`, the absolute
  `pt`, `px`, `pc`, `in`, `cm` and `mm` (CSS units, kept as physical
  widths that do not shrink in scripts), the TeX unit `mu`, and the MathML
  3 named spaces from `veryverythinmathspace` to `veryverythickmathspace`
  and their `negative…` forms. Only `em` was read before; any other width
  became zero. A percentage or unitless width is, per MathML 3, a multiple
  of the default width 0; CSS numbers (`+1em`, `1e1em`),
  valid in MathML Core, are accepted. A width that is not a length
  (`1 em`, `wide`) is read as 0 with a warning; `Strictness::Lenient`
  recovers `1 em`.

- A `\color` switch in a `\left…\right` list stays in force across
  `\middle` to the end of the fence, as in TeX and KaTeX:
  `\left( \color{red} a \middle| c \right)` left `c` uncolored.
  The `\middle` delimiter itself keeps the default color (TeX and KaTeX
  color it). An infix command after nested color switches divides the
  whole list, as in TeX: `x \color{red} a \color{blue} b \over c` left `x`
  outside the fraction. The LaTeX writer keeps the `\left…\right` pair of
  a barless fraction whose part holds a `\middle` anywhere (inside a
  color, a prescript's base or an array cell), instead of writing a
  `\binom` that does not read back.
- Infix fraction commands (`\over`, `\atop`, `\above`, `\choose`,
  `\brace`, `\brack`, `\…withdelims`) no longer leak an internal marker:
  `\operatorname{a \over b}` was `<mi>__infix_over__</mi>` in MathML and
  `\char`\_\char`\_infix…` in LaTeX; it is now a fraction, as in TeX and
  MathJax. The reader records these commands in a parser-private table
  instead of as identifiers, and every math list resolves them, including
  `\operatorname` arguments, `CD` and `\xymatrix` cells, after a
  `\color` switch (`\color{red}a \over b` lost the fraction), `\ce`/`\pu`
  rewrites and `\braket`. Scripts after one start the denominator
  (`a \over^2 b` is `\frac{a}{{}^2 b}`; the fraction was lost), and a
  large operator before one is the numerator (`\sum\over b`,
  `\sum \bf \over`, `\sum_i \color{red} a \over b`). `\color` opens no
  group, so like an old-style font switch it no longer confines a later
  infix command: `a \color{red} b \over c` is `\frac{a\color{red}b}{c}`, as
  in TeX and KaTeX. Warnings about these commands are no longer repeated by
  speculative re-parses, and a long chain of them cannot overflow the
  stack.
- The LaTeX writer no longer folds a barless fraction whose part holds a
  `\middle` into `\binom`: `\left( a \atop b \middle| c \right)` keeps its
  `\left…\right` pair (it was written `\binom{a}{b\middle|c}`, which did
  not read back the same).
- The LaTeX writer keeps `\overline`/`\overset`/`\underbrace`… around
  `\mathop` of a named operator: `\overline{\mathop\lim}` was written
  `\mathop{\lim}\limits^{‾}`, which reads back as `\lim\limits^{‾}`, so
  the round trip was not stable.
- A barless fraction no longer gains a bar. The LaTeX writer spelled
  `FractionKind::NoBar` as `\frac`: `{a \atop b}` now writes
  `\genfrac{}{}{0pt}{}{a}{b}`, `{a \brace b}` writes
  `\genfrac\{\}{0pt}{}{a}{b}` and `{a \brack b}` writes
  `\genfrac[]{0pt}{}{a}{b}` (`\binom` is unchanged), forms KaTeX and
  MathJax both read and that cannot capture the rest of a row as a bare
  `\atop` would. UnicodeMath writes and reads the barless stack as `a¦b`
  (it wrote `a/b`). The LaTeX reader keeps `\genfrac`'s thickness and
  style: a zero thickness in any unit (`0pt`, `0em`, `0`) is barless (its
  MathML now has `linethickness="0"`), an empty one keeps the default rule,
  and a style of `0`–`3` forces display/text/script/scriptscript style
  (`\genfrac{}{}{}{0}{a}{b}` is `\dfrac{a}{b}`). A nonzero thickness such
  as `2pt` is drawn at the default thickness, with a warning, as the AST
  has no rule thickness. `\above 0pt` reads as `\atop`, and the MathML
  reader reads any zero `linethickness` (`0px`, `0em`) as barless, not just
  `0`.
- `\genfrac` reads a `.` delimiter as the null delimiter, as `\left.` does:
  `\genfrac.){}{}{a}{b}` is `\left. \frac{a}{b} \right)` and
  `\genfrac{.}{.}{}{}{a}{b}` is `\frac{a}{b}` (it drew a literal period
  fence). `\above` with a nonzero thickness (`{a \above 2pt b}`) now warns
  that the rule is drawn at the default thickness, as `\genfrac` does.
- The LaTeX writer keeps `\mathop{...}` around an accent: `\mathop{\hat x}`
  (a TeX Op atom) was written as `\hat{x}`, an ordinary atom, so the operator
  class was lost and nested `\mathop`s did not round-trip stably.
- `\binom` arguments with several nodes are no longer wrapped in a second
  brace pair (`\binom{Y+}{z}` was written `\binom{{Y+}}{z}`).
- Symbols missing from the bundled Latin Modern fonts no longer render as
  silent gaps. The renderer substitutes equivalent in-font characters (MathML
  `&OverBar;`, presentation-form braces `︷︸︵︶`), draws from a Latin Modern
  text face (`ƒ`), or composes the glyph from in-font outlines (`\Coloneqq`,
  `\bigstar`, `\pitchfork`, `\subseteqq`, `\precneqq`, `\lBrace`,
  `\lmoustache`, `\Game`, `\Finv`, `\digamma`, `\circledS`, dashed arrows,
  small triangles, …) in math and in text runs. Any other uncovered character
  is still emitted as `<text>` in SVG output (rendered by the viewer's fonts)
  and drawn as a hollow box in PNG/JPEG output. Untabled ASCII punctuation
  (`^ _ ~ * @ …`) now uses real font metrics, so `\textasciicircum` is no
  longer clipped.
- Decorated and mixed-content LaTeX operator names now retain their upright
  letters, nested styles and limit placement through LaTeX and MathML export.
  The public AST exposes their content and form in `MathNode::StructuredOperator`.
  Plain named operators keep their existing string representation.
- LaTeX parsing and export preserve the mathtools smallmatrix family, including
  starred alignment options, fence spacing, and environment names supplied by macros.
  The public heap AST now represents parsed matrices and small matrices with
  `MathNode::Matrix`. Consumers that inspect table cells should handle both
  `Matrix` and `Array`. Generic arrays and previously serialized array records
  retain their `MathNode::Array` representation.
- `\limsup`, `\liminf`, `\injlim`, and `\projlim` now render as movable
  operators with a thin space between their words. `\plim` uses the KaTeX
  expansion for an operator with forced limits.
- Table normalization preserves explicit spaces and signed kerns at both cell
  edges, including native spacing in imported MathType equations.

- Fixed-size `\braket` and `\set` and scalable `\Braket` and `\Set` now retain
  the package's separator spacing and nested scopes. An explicitly doubled bar
  in `\Braket` uses native LaTeX double-bar semantics.
- MathML reimport preserves foreground colors in `mstyle` scopes and fixed
  interior bars inside explicitly marked bra and ket delimiters. The additive
  `MathNode::Color` variant keeps those foreground scopes when compact input is
  materialized as the public heap AST.

- Named LaTeX operators and compound `\mathop` expressions retain their
  side, movable, or fixed limits in MathML and canonical LaTeX. In inline
  output and nested script arguments, movable compound bounds use side scripts.
  Inline MathML now carries `display="inline"` through a default-config reimport.

- MathML groups an operator and its operand when they fill one fraction,
  root, or script argument. For example, the numerator of
  `\frac{\sum_i^n x}{2}` keeps the sum and its `x` together.

- MathML output preserves TeX's movable default limits. Sum and product-family
  operators use movable under/over elements, while integral-family operators
  keep side limits and explicit `\limits`/`\nolimits` remain distinct.
- LaTeX macro expansion now charges queued replacements, discarded selector
  branches, macro-body evaluation and token processing against a work budget.
  Queue accounting follows consumed and rolled-back source tokens without
  repeatedly scanning unread input. Recursive depth and work exhaustion use
  distinct typed expansion errors, and intermediate token vectors avoid
  reserving their full source-byte length. Ordinary reusable macros retain
  their existing behavior within the bounded budget.
- LaTeX spacing primitives preserve their natural widths through LaTeX, MathML,
  and SVG, including fractional math units and scaled points. In scripts,
  `\hskip` follows the text font and `\mskip` follows the math symbol font. A
  complete stretch or shrink component produces a diagnostic when its natural
  width is used. Explicitly sized input uses the additive `MathNode::ExplicitSpace`
  variant in the heap AST. Generic `MathNode::Space` values retain local-em semantics.
- LaTeX math accepts KaTeX’s Unicode superscript and subscript characters.
  Mixed forms such as `X_i²`, `x'₂`, and `\sumₙ²` attach to one base. A script
  character in an argument position stays literal, as in `x^²`. Operator
  placement controls can also appear between or after script fields.
- MathML import recovers matching structural fences and middle delimiters from
  explicitly fenced operator rows. Reimporting a fraction between delimiters
  preserves scalable brackets, literal interior bars, and invisible middles.
  Explicit MathML operator roles also preserve absolute values, half-open
  intervals, and one-sided LaTeX delimiters.
- LaTeX math style declarations are scoped to their group. MathML, SVG, OMML,
  MTEF, and UnicodeMath select the active `\mathchoice` branch in scripts,
  fractions, roots, and table cells. LaTeX export retains all four branches.
- `\dbinom` and `\tbinom` keep their barless fraction and locally force display
  or text style. UnicodeMath indexed radicals now round-trip their index and
  radicand through `√(index&radicand)`.
- The legacy LaTeX declarations `\rm`, `\sf`, `\tt`, `\bf`, `\it`, and
  `\cal` apply to the remainder of their group or table cell. Infix fractions
  keep their numerator and denominator in scope, and a nested matrix lets each
  cell establish its own font before the outer declaration resumes.
- `\\argmin` and `\\argmax` retain the thin mathematical space used by
  their canonical `\\operatorname*{arg\\,min/max}` forms through LaTeX,
  MathML, and rendering.
- Normalization treats empty styled or colored script arguments as transparent,
  so later scripts attach to the surviving base consistently across AST paths.
- LaTeX serialization drops invisible prescript markers while preserving
  any remaining valid prescript and the operator's surrounding spacing.
- LaTeX output writes ordinary control characters through bounded numeric
  `\\char` tokens, preserving C0, C1, and DEL symbols when the result is
  parsed and written again.

### Added

- Font sizes as typesetting attributes of nodes (`ast::NodeAttrs`,
  `FontSize`, `MathNode::Attributed`, `AstArena::attrs`): the sizes MTEF
  gives characters where a template does not imply them, MathML absolute
  `mathsize` and OMML `w:sz`. `NodeAttrs::glyph_size` is the size of a node's
  own sign, the operator of an n-ary node or the mark of an accent (MTEF,
  MathML `<mo mathsize>`, OMML `m:ctrlPr`). The MathML, OMML
  (`OmmlRunProperties::WordprocessingMl`) and MTEF writers write them and the
  renderer draws them. The LaTeX writer writes the nearest size command
  within 10 %, and in scripts only `\textstyle`, `\scriptstyle` or
  `\scriptscriptstyle`; other sizes are dropped with an Info diagnostic, as
  are those of signs and of operator names with scripts.
  `Config::typesetting` (`TypesettingPolicy`) selects whether approximated
  sizes, and in UnicodeMath dropped ones, are reported too. `MathExpr::meta`
  holds the size of ordinary text. Not kept: the sizes of delimiters (MTEF
  reports them), of other OMML objects' `m:ctrlPr`, DrawingML `sz`, MathML
  `mathsize` on table rows and cells, and the symbol and
  sub-symbol typesizes of MTEF characters (reported as Info; pictures of
  documents do not settle them, and MTEF 3 does not record their size).
- **Breaking (behavior):** `AstArena::node_mut` removes the typesetting
  attributes of the node it returns, which may become another node.
- **Breaking:** `MathExpr`, `CompactExpr` and `Config` have a new field each
  (`meta`, `meta`, `typesetting`), and `MathNode` a new variant.
- **Breaking (behavior):** MTEF runs of text, digits or function names are
  split where their size changes; absolute MathML sizes are attributes instead
  of being dropped with a warning (accent and operator signs included); OMML
  `w:sz` is no longer a diagnostic and an invalid value is an error in
  `Strictness::Strict`.
- The MTEF reader reads MTEF 2 (Equation Editor 1.x and 2.x, MathType 3.0) and
  MTEF 4 (MathType 3.5), which were errors before. New variants
  `MtefVersion::V2` and `MtefVersion::V4` (the enum is `#[non_exhaustive]`, so
  this is not a breaking change). The writer still writes only MTEF 3 and 5:
  `MtefWriteOptions::from_info` maps an MTEF 2 or 4 equation to MTEF 3
  (Equation Editor 3.0) when Equation Editor generated it, else to MTEF 5
  (MathType 6). `docs/mtef-legacy.md` lists what the reading rests on.
- The TeX primitives `\overwithdelims`, `\atopwithdelims` and
  `\abovewithdelims` (supported by MathJax; KaTeX 0.16 rejects them):
  `{a \overwithdelims() b}`, `{a \atopwithdelims[] b}` and
  `{a \abovewithdelims\{\} 0pt b}` read as the equivalent `\genfrac`
  (fences around a fraction with or without its rule; `.` is the null
  delimiter). They were undefined commands.
- The LaTeX `\displaylimits` placement modifier.
- LaTeX macro definitions take effect in source order. Group-local and global
  definitions, `\let`, `\futurelet`, expanded and protected definitions, and
  delimited parameters preserve token meanings. Declared environments accept
  opening arguments and an optional first default.
- LaTeX branch selectors choose from unexpanded tokens. `\TextOrMath`
  follows text arguments and their inline math, and `\@ifnextchar` can look
  through a macro replacement into the caller's remaining input.
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

- An undefined LaTeX control sequence is kept as itself instead of becoming
  identifier letters that merge with what follows (`\foo x` used to give
  `foox`). It is the new `MathNode::UndefinedCommand` (and
  `CompactNode::UndefinedCommand`), holding its spelling and the brace groups
  written directly after it, which may be its arguments (`\foo{a}{}`).
  Like KaTeX with `throwOnError: false` it renders upright in KaTeX's error
  colour (`#cc0000`); LaTeX output spells it as inert text by default (see
  below), MathML writes MathJax's
  `<merror><mtext>\foo</mtext><mrow>a</mrow>…</merror>`, which the MathML
  reader reads back, and OMML, UnicodeMath (arguments in parentheses) and
  MTEF write it as text. Each
  name is reported once as an `undefined control sequence \foo` warning, in
  strict and lenient mode alike (the first 100 names, then one summary).
  `\right`, `\end`, `\multicolumn`, `\cellcolor`, `\rowcolor` and
  `\columncolor` outside their context are reported as misplaced and written
  in braces. Text mode (`\text{\foo}`) is unchanged, and a builtin macro met
  where the preprocessor did not run (inside some rewritten `\ce` text) keeps
  its identifier spelling with a `control sequence \tr is not expanded here`
  warning. `CompactNode` gains a variant, so code matching it exhaustively
  needs an arm.
- LaTeX output never contains an unknown command as live LaTeX by default.
  With the default `Config::undefined_commands` (`UndefinedCommands::Escape`)
  the LaTeX writer spells an undefined command as visible, inert text with
  its groups as literal braces, `\mathord{\text{\textbackslash{}foo}\{a\}}`,
  which reads back stably (before, it was the identifier letters `fooa`).
  `Config::with_undefined_commands(UndefinedCommands::Keep)` writes it back
  live (`\foo{a}`); that mode is best effort and not safe for untrusted
  input compiled with file, Lua or shell access. In both modes, as defence
  in depth only, a fixed list of dangerous primitives (`\input`,
  `\write`, `\immediate`, `\catcode`, `\usepackage`, `\directlua`,
  `\lstinputlisting`, the `\def`/`\let` family and others; see
  `ast::undefined`) is never written live: it is spelled like the default
  inert text in both modes (`\mathord{\text{\textbackslash{}input}\{x\}}`)
  and reported with a `control sequence \input is not allowed and was kept
  as text` warning. The MathML reader
  reads `<merror><mtext>\foo</mtext>…` as an undefined command only for a
  name the LaTeX reader itself keeps as undefined, and the writers write a
  node built through the API or serde with any other name as text. The AST
  node, diagnostics, MathML and rendering are the same in both modes.
- A `#RRGGBB` colour in `\textcolor`/`\color` is stored in lower case, the way
  its `\textcolor[HTML]{…}` spelling reads back, so it round-trips stably.

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
- Negating a scalar large operator keeps the following operand separate on
  LaTeX round-trip. Negated integrals retain their bounds and bodies.

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
