//! Manually sized delimiters (`\big`, `\Bigl`, `\biggr`, `\Biggm`, …).
//!
//! In TeX, `\bigl` is `\mathopen{\big…}`, `\bigr` is `\mathclose`, `\bigm` is
//! `\mathrel`, and plain `\big` is an ordinary atom. These tests check that the
//! suffix survives every round trip that can carry it, and that the
//! delimiter set matches `\left`.

use mathema::{Format, convert};

const SIZES: [&str; 4] = ["big", "Big", "bigg", "Bigg"];
const CLASSES: [&str; 4] = ["", "l", "r", "m"];

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap_or_else(|e| panic!("{input}: {e}"))
}

fn via_mathml(input: &str) -> String {
    let mathml =
        convert(input, Format::LaTeX, Format::MathML).unwrap_or_else(|e| panic!("{input}: {e}"));
    convert(&mathml, Format::MathML, Format::LaTeX).unwrap_or_else(|e| panic!("{mathml}: {e}"))
}

/// Every `\big` command, with delimiters written as `\left` accepts them.
fn all_commands() -> impl Iterator<Item = String> {
    SIZES
        .iter()
        .flat_map(|size| CLASSES.iter().map(move |class| format!("\\{size}{class}")))
}

#[test]
fn every_big_command_accepts_every_left_delimiter() {
    let delimiters = [
        "(",
        ")",
        "[",
        "]",
        "|",
        "/",
        ".",
        "<",
        ">",
        r"\{",
        r"\}",
        r"\|",
        r"\langle",
        r"\rangle",
        r"\lvert",
        r"\rvert",
        r"\lVert",
        r"\rVert",
        r"\vert",
        r"\Vert",
        r"\lfloor",
        r"\rfloor",
        r"\lceil",
        r"\rceil",
        r"\uparrow",
        r"\Downarrow",
        r"\updownarrow",
        r"\backslash",
        r"\llbracket",
        r"\rrbracket",
        r"\lBrace",
        r"\rBrace",
        r"\lgroup",
        r"\rmoustache",
        r"\ulcorner",
        "⟦",
        "⟧",
        "⦃",
        "⦄",
        "⟨",
        "⟩",
        "⌊",
        "⌋",
        "‖",
    ];
    for command in all_commands() {
        for delim in delimiters {
            let left = format!(r"\left{delim} x \right.");
            assert!(
                convert(&left, Format::LaTeX, Format::LaTeX).is_ok(),
                "\\left rejects {delim}"
            );
            let input = format!("a{command}{delim} x");
            let first = latex(&input);
            assert!(
                first.starts_with(&format!("a{command}")),
                "{input} -> {first}"
            );
            assert_eq!(latex(&first), first, "{input} is not stable");
        }
    }
}

#[test]
fn class_suffix_survives_latex_and_mathml_round_trips() {
    for (input, expected) in [
        (r"\bigl⟦ a \bigr⟧", r"\bigl\llbracket a\bigr\rrbracket"),
        (r"\bigl⦃ a\bigr⦄", r"\bigl\lBrace a\bigr\rBrace"),
        (
            r"\bigl\llbracket a \bigr\rrbracket",
            r"\bigl\llbracket a\bigr\rrbracket",
        ),
        (
            r"\Bigl\lBrace x \Bigr\rBrace",
            r"\Bigl\lBrace x\Bigr\rBrace",
        ),
        (r"\{x \bigm| x>0\}", r"\{x\bigm| x> 0\}"),
        (
            r"\Bigl\langle a \Bigm\vert b \Bigr\rangle",
            r"\Bigl\langle a\Bigm| b\Bigr\rangle",
        ),
        (
            r"\biggl[ \Biggl( x \Biggr) \biggr]",
            r"\biggl[\Biggl(x\Biggr)\biggr]",
        ),
        (r"\big( x \big)", r"\big(x\big)"),
        (r"\bigl. x \bigr|", r"\bigl.x\bigr|"),
        (r"\big/ \big\| \big\uparrow", r"\big/\big\|\big\uparrow"),
    ] {
        assert_eq!(latex(input), expected, "LaTeX -> LaTeX: {input}");
        assert_eq!(
            via_mathml(input),
            expected,
            "LaTeX -> MathML -> LaTeX: {input}"
        );
    }
    for command in all_commands() {
        let input = format!("a{command}( b");
        assert_eq!(via_mathml(&input), latex(&input), "{input}");
    }
}

#[test]
fn mathjax_mathml_reads_back_with_size_and_class() {
    // MathJax 4.1 output for `\bigl( a \bigr) \bigm| \big/ \Biggl.`.
    let mathjax = concat!(
        r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block">"#,
        r#"<mrow data-mjx-texclass="OPEN" data-latex="\bigl("><mo minsize="1.2em" maxsize="1.2em">(</mo></mrow>"#,
        r#"<mi data-latex="a">a</mi>"#,
        r#"<mrow data-mjx-texclass="CLOSE" data-latex="\bigr)"><mo minsize="1.2em" maxsize="1.2em">)</mo></mrow>"#,
        r#"<mrow data-mjx-texclass="REL" data-latex="\bigm|"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">|</mo></mrow>"#,
        r#"<mrow data-mjx-texclass="ORD" data-latex="\big/"><mo minsize="1.2em" maxsize="1.2em" fence="true" stretchy="true" symmetric="true">/</mo></mrow>"#,
        r#"<mrow data-mjx-texclass="OPEN" data-latex="\Biggl."><mo minsize="2.470em" maxsize="2.470em" fence="true" stretchy="true" symmetric="true"></mo></mrow>"#,
        "</math>"
    );
    assert_eq!(
        convert(mathjax, Format::MathML, Format::LaTeX).unwrap(),
        r"\bigl(a\bigr)\bigm| \big/\Biggl."
    );
}

#[test]
fn other_formats_still_convert_sized_delimiters() {
    let input = r"\bigl( a \bigm| b \bigr) \big\| \Bigl⟦ c\Bigr⟧";
    for format in [Format::Omml, Format::UnicodeMath, Format::Mtef] {
        let out =
            convert(input, Format::LaTeX, format).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        let back =
            convert(&out, format, Format::LaTeX).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        for glyph in ["(", ")", "a", "b", "c"] {
            assert!(back.contains(glyph), "{format:?}: {back}");
        }
    }
}

#[cfg(feature = "render")]
mod render {
    use mathema::from_latex;

    /// Width of the text-mode SVG, in pixels at the default 16px size.
    fn width(input: &str) -> f64 {
        let svg = from_latex(input).unwrap().to_svg_text().unwrap();
        let start = svg.find("width=\"").unwrap() + 7;
        let end = start + svg[start..].find('"').unwrap();
        svg[start..end].parse().unwrap()
    }

    const MU: f64 = 16.0 / 18.0;

    #[test]
    fn plain_big_is_ordinary_but_bigl_is_an_opening() {
        // Op-Ord takes a thin space; Op-Open none (TeXbook p. 170).
        let gap = width(r"\sin\big(x\big)") - width(r"\sin\bigl(x\bigr)");
        assert!((gap - 3.0 * MU).abs() < 0.05, "gap {gap}");
    }

    #[test]
    fn bigm_is_a_relation() {
        // Ord-Rel-Ord takes two thick spaces that Ord-Ord-Ord does not.
        let gap = width(r"a\bigm|b") - width(r"a\big|b");
        assert!((gap - 10.0 * MU).abs() < 0.05, "gap {gap}");
    }

    #[test]
    fn a_minus_after_plain_big_stays_binary() {
        // After an opening `\bigl(` a `-` is unary (Ord); after the plain
        // ordinary `\big(` it stays binary with medium spaces around it.
        let gap = width(r"\big(-x\big)") - width(r"\bigl(-x\bigr)");
        assert!((gap - 8.0 * MU).abs() < 0.05, "gap {gap}");
    }
}
