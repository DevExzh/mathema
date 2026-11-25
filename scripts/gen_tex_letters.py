#!/usr/bin/env python3
"""Generate the table of src/unicode/tex_letters.rs.

usage: gen_tex_letters.py > letters.txt

Asks XeLaTeX and LuaLaTeX (TeX Live), which must be installed and agree, for
the characters outside ASCII that have category code 11, and prints the rows
of `LETTERS`. A range also covers the code points in it that Unicode leaves
unassigned in this Python's `unicodedata`: the table depends on the versions of
both (the one in the tree: TeX Live 2025, Unicode 16).
"""
import subprocess
import sys
import tempfile
import unicodedata

SOURCE = r"""
\documentclass{article}
\newwrite\out
\immediate\openout\out=letters.txt
\newcount\code
\code=128
\loop
  \ifnum\catcode\code=11 \immediate\write\out{\the\code}\fi
  \advance\code by 1
  \ifnum\code=55296 \code=57344 \fi
\ifnum\code<1114112 \repeat
\immediate\closeout\out
\begin{document}\end{document}
"""


def letters(engine):
    with tempfile.TemporaryDirectory() as directory:
        with open(directory + "/letters.tex", "w", encoding="ascii") as file:
            file.write(SOURCE)
        # (The document written to is not the source: \jobname differs.)
        subprocess.run(
            [engine, "-interaction=batchmode", "-jobname=run", "letters.tex"],
            cwd=directory,
            stdout=subprocess.DEVNULL,
            check=True,
        )
        with open(directory + "/letters.txt", encoding="ascii") as file:
            return {int(line) for line in file}


def ranges(codes):
    """Ranges of `codes`, over unassigned code points too."""
    result = []
    for code in sorted(codes):
        if result and all(
            unicodedata.category(chr(gap)) == "Cn"
            for gap in range(result[-1][1] + 1, code)
        ):
            result[-1][1] = code
        else:
            result.append([code, code])
    return result


def rows(pairs):
    """The pairs, as many as fit a line."""
    line = "   "
    for first, last in pairs:
        item = f" (0x{first:X}, 0x{last:X}),"
        if len(line) + len(item) > 95:
            yield line
            line = "   "
        line += item
    yield line


def main():
    xelatex, lualatex = letters("xelatex"), letters("lualatex")
    if xelatex != lualatex:
        differ = " ".join(f"{code:X}" for code in sorted(xelatex ^ lualatex))
        sys.exit(f"XeLaTeX and LuaLaTeX disagree on {differ}")
    print("\n".join(rows(ranges(xelatex))))


if __name__ == "__main__":
    main()
