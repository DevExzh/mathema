# MTEF versions 2 and 4: format notes

This is not a vendor specification. Design Science published none for MTEF 2
or for Equation Editor 1.x/2.x. These notes record the rules that mathema's
reader (`src/reader/mtef/decode/legacy.rs`) relies on. They come from three
sources, and each rule below says which ones support it:

- **[v4 doc]** the MTEF description for MathType 3.5 (`MTEFv4.md`, Design
  Science). Its version history says that version 2 was written by
  "MathType 3.x and Equation Editor 1.x", that version 4 was written by
  MathType 3.5, and that "version 2 differs from version 1 only in the format
  of the header".
- **[LibreOffice]** LibreOffice's MathType importer
  (`starmath/source/mathtype.cxx`). It accepts versions 0 to 3 (not 4). For
  `nVersion < 3` it reads a one-byte character value, stops an embellishment
  after its type byte, and maps characters of the Greek and Symbol styles
  through the Symbol font encoding.
- **[corpus]** 8,541 `Equation Native` streams (8,187 MTEF 2 and 354 MTEF 4)
  from the public govdocs1 collection.
- **[preview]** the picture (WMF or PICT) the documents store for 7,506 of
  those equations. 7,360 pictures contain text that could be extracted: the
  font and byte drawn for each character. In 6,873 every byte of the equation
  is drawn unchanged. 125 others differ only in the translated text positions
  below; 362 show some other equation, nearly always because the document
  pairs many equations with the same or a stale picture.

Every rule is marked with the sources that support it. "Inferred" marks a
reading no source states. Character codes without evidence are errors in Strict mode
and warnings in Lenient mode.

## Header

There are five bytes, laid out as in MTEF 3 **[v4 doc]**:

| byte | meaning |
| --- | --- |
| 0 | MTEF version: 2 or 4 |
| 1 | platform: 0 Mac, 1 Windows |
| 2 | product: 0 MathType, 1 Equation Editor |
| 3 | product version |
| 4 | product subversion |

These (platform, product, version, subversion) values occur in the corpus
**[corpus]**:

| version | header | streams | presumed writer |
| --- | --- | --- | --- |
| 2 | 0, 1, 11, 0 | 2,490 | Equation Editor (Mac) |
| 2 | 0, 1, 10, 0 | 1,945 | Equation Editor (Mac) |
| 2 | 1, 1, 2, 1 | 1,452 | Equation Editor 2.x (Windows, Word 6/95) |
| 2 | 0, 1, 3, 0 / 3, 1 | 801 / 665 | Equation Editor (Mac) |
| 2 | 1, 1, 2, 0 | 498 | Equation Editor 2.0 (Windows) |
| 2 | 0, 1, 2, 1 | 197 | Equation Editor (Mac) |
| 2 | 0, 1, 12, 1 / 12, 0 | 102 / 34 | Equation Editor (Mac) |
| 2 | 1, 1, 2, 2 | 3 | Equation Editor 2.x (Windows) |
| 4 | 0, 0, 3, 7 / 3, 6 | 264 / 90 | MathType 3.5+ (Mac) |

MTEF 2 and 4 headers carry no application key and no inline/display flag.

## Records

The record layout is that of MTEF 3 **[v4 doc, corpus]**: all 8,541 streams parse
to their final END. The low nibble of
the tag byte is the record type and the high nibble holds the options. The
same option flags apply: nudge 0x8; for LINE, null 0x1, ruler 0x2 and line
spacing 0x4; for PILE, ruler 0x2; for CHAR, xfAUTO 0x1 and xfEMBELL 0x2.
LINE, TMPL, PILE, MATRIX, FONT, SIZE and the typesize records 10 to 14 are read
exactly as in MTEF 3, and all of them occur in the corpus. RULER records (read
as MTEF 3 specifies) and line spacing do not occur. MTEF 2
has only one kind of SIZE record in the corpus, the explicit point size
(101). MTEF 4 also uses lsize/dsize pairs (100 and the typesize values).

The format differs from MTEF 3 in two places.

1. **CHAR**: tag, [nudge], typeface + 128, **one byte** of character value,
   then the embellishment list if xfEMBELL is set. MTEF 3 uses 16 bits here.
   **[LibreOffice for MTEF 2, corpus]**: read as 16 bits, 7,539 streams break and
   only short ones happen to parse.
2. **EMBELL**: tag, [nudge], embellishment type. Equation Editor 3 writes an
   extra zero byte after the type, which these versions **do not have**
   **[LibreOffice for MTEF 2, corpus]**: with it, 1,449 streams break.
   The embellishment list still ends with an END record. The types are
   those of MTEF 3 (2 to 20); the corpus uses 2, 3, 5, 6, 8 to 17 and 19.

Template selectors, variations and slot order are those of MTEF 3, which match
the table in `MTEFv4.md` **[v4 doc, corpus]**. The BigOpBoxClass correction
noted in `templates.rs` also applies: the lower limit comes before the upper
one. Selectors 0 to 5, 7, 13 to 17, 19, 21, 22, 24 to 26, 28 to 32, 37, 39, 41 to
45 and 47 occur in MTEF 2; see "Unconfirmed" for the others.

A seeded random sample (110 MTEF 2, 55 MTEF 4) was rendered and compared by
eye with the stored picture. Four MTEF 2 pictures show another equation. In
the rest the structure agrees, except that slash fractions (tmSLFRACT) are
drawn stacked: the reader keeps them as skewed fractions, and the LaTeX writer
and the renderer draw those stacked for every MTEF version.

## Characters

A character value is a position in the font of its typeface, not Unicode.

| typeface | encoding | basis |
| --- | --- | --- |
| fnLCGREEK (4), fnUCGREEK (5), fnSYMBOL (6) | Adobe Symbol | LibreOffice (some positions), preview |
| fnMTEXTRA (11) | MT Extra positions (below) | preview |
| fnEXPAND (22), below 0x80 | delimiter letters (below) | corpus |
| fnEXPAND (22), 0x80 and up | Symbol (only 0xE9 and 0xF9 occur) | inferred |
| fnMARKER (23), values 1 and 9 | alignment marks, no glyph | as for MTEF 3 |
| fnSPACE (24), and values 1, 2, 4, 5, 8 anywhere | space codes | below |
| explicit font (negative) | the encoding its font name implies | as for MTEF 3 |
| any other style | text positions (below) | preview |

**Text positions.** 0x20 to 0x7E are ASCII. Above that only these occur. The
translated ones are Adobe StandardEncoding positions; the rest are drawn as
the same byte in a Mac font, so they are Mac OS Roman.

| value | platform | character | drawn as [preview] |
| --- | --- | --- | --- |
| 0xA9 | both | `'` | 0x27, in every picture checked; LibreOffice agrees (function style) |
| 0xB2 | both | `†` | 0x86 in Windows fonts |
| 0xAA, 0xBA | Mac | `“`, `”` | Mac Roman 0xD2, 0xD3 |
| 0xB1 | Mac | `–` | Windows 0x96, Mac Roman 0xD0 |
| 0xB7 | Mac | `•` | Mac Roman 0xA5 |
| 0x8E | Mac | `é` | 0x8E |
| 0xCA | Mac | no-break space | 0xCA |
| 0xFB | Mac | `˚` | 0xFB (LibreOffice reads `ß`, its StandardEncoding value) |

0xA9 is read as an apostrophe, so it becomes a prime by the same rules as the
apostrophe of MTEF 3 and 5 outside text. Whether StandardEncoding holds for
other positions, and for Windows beyond 0xA9 and 0xB2, is unconfirmed.

**Spaces.** Values 1, 2, 4, 5 and 8 occur, in fnSPACE and in other typefaces
alike **[corpus]**. They are read with the widths of the MTCode block U+EF00
(1 zero, 2 thin, 4 thick, 5 em, 8 one point), which is **inferred**; the
widths were not measured.

**fnEXPAND.** `( ) [ ] { }` are themselves; `a b` occur in tmBAR and tmDIRAC
(`|`), `c d` in tmDBAR (`‖`), `e f` in tmANGLE and tmDIRAC (`⟨ ⟩`), `B` in
tmLHBRACE (`⏟`) and `C` in tmRARROW (`→`) **[corpus; the values are inferred
from the selector]**. Only the arrow's character reaches the output.

**MT Extra.** 0x68 ℏ, 0x6C ℓ, 0x6D ∓, 0x6F ∘ and 0x70 ≺ are paired with those
MTCode values in MTEF 5 equations, and all but 0x70 are confirmed by
pictures **[corpus, preview]**. The positions known from MTEF 3 and 5 apply too.

**Refused:** 0x3C, 0x3E and 0x61. In access-control specifications, where
nearly all of them occur, the pictures draw the same byte of MT Extra and the
context suggests ⊲, ⊳ and ↦, but no MTEF 5 stream pairs them and the font was
not available. In one Mac equation the picture draws both 0x3C as a plain `<`
(`Ω₀/(v/2R) << 1`). So 0x3C has at least two meanings, and refusing it is right.

## Unconfirmed

Read in Strict mode without a file that shows it:

- MTEF 4 template selectors 6 to 12, 16, 18 to 20, 22, 23, 26 to 28, 30 to 38,
  40 and 43 to 48, and embellishments 4, 7, 10,
  12, 13, 15, 16 and 18 to 20: no MTEF 4 file has them. The MTEF 4
  specification defines them.
- Variations and template options are not restricted beyond those MTEF 3
  allows, in either version.

MTEF 2 has no vendor table, so there Strict mode refuses the template selectors
6, 8 to 12, 18, 20, 23, 27, 33 to 36, 38, 40, 46 and 48 and the embellishments
4, 7, 18 and 20, which no MTEF 2 file has (nothing in the corpus is lost).
Lenient mode reads them as MTEF 3 does, with a warning.

No picture confirms, though files have them: MT Extra 0x70 (paired by MTEF 5
only), 0x49 on Windows, fnEXPAND 0xE9 and 0xF9, and the space widths.

## The style of FONT records (MTEF 2 and 3)

The style byte of a FONT record is bit 0 **bold**, bit 1 **italic**, as in MTEF 5.
The descriptions of MTEF 3 and 4 say the opposite ("1 for italic and/or 2 for
bold"). **[corpus] [preview]**

The pictures name the font of every run of text, with its weight and slant. Of
the fonts of govdocs1 equations that have one style in the equation and one in
the picture, and that no MathType style uses (none of Times New Roman, Times,
Symbol and MT Extra):

| | byte 0 | byte 1 | byte 2 | byte 3 |
| --- | --- | --- | --- | --- |
| MTEF 2 (Equation Editor 2.x to 12.x) | 12 plain | 1 bold | 1 italic | - |
| MTEF 3 (Equation Editor 3.x) | 315 plain | 212 bold | 48 italic | 30 bold italic |
| MTEF 5 (bits as described) | 88 plain | 14 bold | 14 italic | 11 bold italic |

None is drawn otherwise. By character, for the letters and digits an equation
draws in such fonts only (any family, bytes 1 and 2), the pictures agree with
bold = 1 and italic = 2 in 26 MTEF 2 equations and 310 MTEF 3 equations, with
the description in 0 and 1; 3 and 18 show neither. The MTEF 2 equations are from
about 11 documents, Macintosh and Windows, those of MTEF 3 from about 100. All
are Equation Editor's: no MTEF 3 (or 2) file of MathType with such a byte has a
picture, and it is read as Equation Editor's are.

No MTEF 4 equation with such a byte has a picture (3 equations have explicit
fonts at all): there the reader keeps to the description of MTEF 4.

## Slack after the equation

**[corpus]** 496 streams of Windows Equation Editor 2 (headers 1, 1, 2, 0 and
1, 1, 2, 1) declare in their OLE header, always as a multiple of 4, up to 31
bytes more than the equation uses. The bytes look like stale memory (Windows
metafile records, font names, pieces of equations). The reader ignores up to
31 such bytes with a note when the declared size is a multiple of 4. As for
MTEF 3 and 5, a tail that decodes as equation content is refused in Strict
mode: that stops 17 of the 496. Two hold pieces of an equation; the rest are metafile or
other bytes that happen to decode. Sixteen have pictures with text, all of
which show exactly the decoded equation; the other decodes as empty. So the
refusals are false alarms of a check kept for damaged equations.

## Writing

mathema does not write MTEF 2 or 4. `MtefWriteOptions::from_info` turns an
Equation Editor equation into an Equation Editor 3.0 one (MTEF 3) and a
MathType equation into a MathType 6 one (MTEF 5).
