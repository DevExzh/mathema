//! `\ce`, `\pu` and the siunitx commands are rewritten into standard LaTeX
//! and read again. A rewrite that does not read back is an error when strict
//! and a warning plus the source kept as text when lenient, never an empty
//! result. `\pu` follows mhchem's unit grammar, failures included.

use mathema::error::{Config, Strictness};
use mathema::{Format, convert, convert_with, from_latex_with};

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

fn mathml(input: &str) -> String {
    convert(input, Format::LaTeX, Format::MathML).unwrap()
}

fn lenient() -> Config {
    Config::default().with_strictness(Strictness::Lenient)
}

/// mhchem writes these as invalid TeX (`\mathrm{m_^{2}}`: a digit run after
/// unit text is an exponent; `\mathrm{m^{2}^{3}}`: a double superscript), so
/// KaTeX rejects them too.
const MHCHEM_ERRORS: &[&str] = &[
    r"\pu{m_2}",
    r"\pu{m^2^3}",
    r"\pu{m^-1^2}",
    r"\pu{m^a^b}",
    r"\pu{kg m_2 s}",
    r"\pu{m_}",
    r"x + \pu{m_2} + y",
];

#[test]
fn failed_pu_rewrite_is_a_strict_error() {
    for input in MHCHEM_ERRORS {
        let error = from_latex_with(input, &Config::default())
            .err()
            .unwrap_or_else(|| panic!("{input}: expected an error"));
        let message = error.to_string();
        assert!(message.contains(r"\pu{"), "{input}: {message}");
        assert!(message.contains("does not parse"), "{input}: {message}");
    }
}

#[test]
fn failed_pu_rewrite_is_a_lenient_warning_with_output() {
    for input in MHCHEM_ERRORS {
        let result = from_latex_with(input, &lenient()).unwrap();
        let warnings: Vec<_> = result
            .diagnostics()
            .iter()
            .filter(|d| d.message.contains("does not parse"))
            .collect();
        assert_eq!(warnings.len(), 1, "{input}: {:?}", result.diagnostics());
        let written = result.to_latex().unwrap();
        assert!(
            written.contains(r"\textbackslash{}pu"),
            "{input}: {written}"
        );
        // The kept text reads back unchanged.
        let again = convert_with(&written, Format::LaTeX, Format::LaTeX, &lenient()).unwrap();
        assert_eq!(again, written, "{input}");
    }
}

#[test]
fn failed_rewrite_warning_spans_the_command() {
    let input = r"x + \pu{m_2} + y";
    let result = from_latex_with(input, &lenient()).unwrap();
    let span = result.diagnostics()[0].span.expect("span");
    assert_eq!(&input[span.start..span.end], r"\pu{m_2}");
}

/// Each `\pu` input reads as what KaTeX 0.16.47's mhchem writes for it
/// (the second column, copied from KaTeX), with no diagnostics. The
/// translation itself is compared byte for byte in `mhchem::pu`'s tests.
#[test]
fn pu_reads_as_katex_mhchem_writes() {
    for (input, katex) in [
        // A number with a power or an exponent is read before the units.
        (
            r"\pu{3 \times 10^8 m s-1}",
            r"{3\times 10^{8}~\mathrm{m}\mkern3mu \mathrm{s^{-1}}}",
        ),
        (
            r"\pu{6.02 \times 10^23 mol-1}",
            r"{6.02\times 10^{23}~\mathrm{mol^{-1}}}",
        ),
        (r"\pu{10^3 m}", r"{10^{3}~\mathrm{m}}"),
        (r"\pu{1.2*10^3 m}", r"{1.2\cdot 10^{3}~\mathrm{m}}"),
        (r"\pu{^2 H}", r"{^{2}~\mathrm{H}}"),
        (r"\pu{10^{3}5}", r"{10^{{3}}~\mathrm{5}}"),
        (r"\pu{2^{10} B}", r"{2^{{10}}~\mathrm{B}}"),
        (r"\pu{1.2(3) m}", r"{1.2(3)~\mathrm{m}}"),
        (r"\pu{1.2(3)e-4 m}", r"{1.2(3)\cdot 10^{-4}~\mathrm{m}}"),
        (r"\pu{1E5 Pa}", r"{1\times 10^{5}~\mathrm{Pa}}"),
        (
            r"\pu{12345.67891 J}",
            r"{12\mkern2mu 345.678\mkern2mu 91~\mathrm{J}}",
        ),
        (r"\pu{1 - 2 m}", r"{1-2~\mathrm{m}}"),
        // Unit text: `_` is a subscript, a number after a unit its exponent.
        (r"\pu{a_b}", r"{\mathrm{a_b}}"),
        (r"\pu{x_a2}", r"{\mathrm{x_a^{2}}}"),
        (
            r"\pu{kg m2 s-2}",
            r"{\mathrm{kg}\mkern3mu \mathrm{m^{2}}\mkern3mu \mathrm{s^{-2}}}",
        ),
        (r"\pu{m\_2}", r"{\mathrm{m\_^{2}}}"),
        (
            r"\pu{kg.m}",
            r"{\mathrm{kg}\mkern1mu{\cdot}\mkern1mu \mathrm{m}}",
        ),
        (r"\pu{25 °C}", r"{25~\mathrm{{}^{\circ}C}}"),
        (r"\pu{kJ/mol}", r"{\mathrm{kJ}/\mathrm{mol}}"),
        (
            r"\pu{J/(mol K)}",
            r"{\mathrm{J}\,/\,(\mathrm{mol}\mkern3mu \mathrm{K)}}",
        ),
    ] {
        let result = from_latex_with(input, &Config::default())
            .unwrap_or_else(|error| panic!("{input}: {error}"));
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
        assert_eq!(latex(input), latex(katex), "{input}");
    }
}

/// `\ce` gives a base at most one subscript and one superscript, as TeX
/// requires: a further script goes on a new empty base (mhchem's
/// `{\vphantom{X}}`), and a bare charge after a superscript joins it, as in
/// mhchem. These used to render as nothing.
#[test]
fn ce_never_writes_a_double_script() {
    for (input, expected) in [
        (r"\ce{H2_{(aq)}}", r"{\mathrm{H}}_{2}{\vphantom{X}}_{(aq)}"),
        (
            r"\ce{C_6H_6_2}",
            r"{\mathrm{C}}_{6}{\mathrm{H}}_{6}{\vphantom{X}}_{2}",
        ),
        (
            r"\ce{CO3^2-_2}",
            r"{\mathrm{CO}}_{3}^{2-}{\vphantom{X}}_{2}",
        ),
        (r"\ce{Y^{99}+}", r"{\mathrm{Y}}^{99+}"),
        (r"\ce{H^+^+}", r"{\mathrm{H}}^{++}"),
        (r"\ce{A^2^+}", r"{\mathrm{A}}^{2+}"),
        (r"\ce{Cu^{2+}^{+}}", r"{\mathrm{Cu}}^{2+}{\vphantom{X}}^{+}"),
        (r"\ce{X^{n}2+}", r"{\mathrm{X}}_{2}^{n}{\vphantom{X}}^{+}"),
        (
            r"\ce{A^{+}_22+}",
            r"{\mathrm{A}}_{22}^{+}{\vphantom{X}}^{+}",
        ),
    ] {
        let result = from_latex_with(input, &Config::default())
            .unwrap_or_else(|error| panic!("{input}: {error}"));
        assert!(result.diagnostics().is_empty(), "{input}");
        assert_eq!(latex(input), latex(expected), "{input}");
    }
}

/// A failing rewrite nested in another one reports the inner command.
#[test]
fn nested_failed_rewrite_reports_the_inner_command() {
    let input = r"\dv{\pu{m_2}}{x}";
    let Err(error) = from_latex_with(input, &Config::default()) else {
        panic!("{input}: expected an error");
    };
    assert!(error.to_string().contains(r"\pu{m_2}"), "{error}");
    let result = from_latex_with(input, &lenient()).unwrap();
    let warnings: Vec<_> = result
        .diagnostics()
        .iter()
        .filter(|d| d.message.contains("does not parse"))
        .collect();
    assert_eq!(warnings.len(), 1, "{:?}", result.diagnostics());
    assert!(warnings[0].message.starts_with(r"\pu{m_2}"));
    assert!(result.to_latex().unwrap().contains(r"\frac"));
}

/// siunitx and physics rewrites that fail to read back are reported too
/// (they used to vanish).
#[test]
fn failed_siunitx_and_physics_rewrites_are_reported() {
    for input in [r"\num{1^}", r"\num{1_}", r"\dv{x^}{y}"] {
        let error = from_latex_with(input, &Config::default())
            .err()
            .unwrap_or_else(|| panic!("{input}: expected an error"));
        assert!(error.to_string().contains("does not parse"), "{input}");
        let result = from_latex_with(input, &lenient()).unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.message.contains("does not parse")),
            "{input}: {:?}",
            result.diagnostics()
        );
        assert!(!result.to_latex().unwrap().is_empty(), "{input}");
    }
}

/// KaTeX reads `\_` in math as an ordinary upright underscore.
#[test]
fn escaped_underscore_in_math() {
    for input in [r"\_", r"a\_b", r"x^\_", r"\mathrm{a\_b}"] {
        let result = from_latex_with(input, &Config::default())
            .unwrap_or_else(|error| panic!("{input}: {error}"));
        assert!(result.diagnostics().is_empty(), "{input}");
        let written = result.to_latex().unwrap();
        assert!(written.contains('_'), "{input}: {written}");
        // The written form reads back to the same LaTeX.
        assert_eq!(latex(&written), written, "{input}");
    }
    // The same node as TeX's `\char` spelling of the glyph.
    assert_eq!(latex(r"a\_b"), latex(r"a\char`\_b"));
    let math = mathml(r"a\_b");
    assert!(
        math.contains(">_</mi>") || math.contains(">_</mo>"),
        "{math}"
    );
}

/// Read `input` strictly, with no diagnostics, and check it writes as
/// `expected` does.
fn assert_reads_as(input: &str, expected: &str) {
    let result = from_latex_with(input, &Config::default())
        .unwrap_or_else(|error| panic!("{input}: {error}"));
    assert!(
        result.diagnostics().is_empty(),
        "{input}: {:?}",
        result.diagnostics()
    );
    assert_eq!(latex(input), latex(expected), "{input}");
}

/// A control sequence in `\ce` is one opaque TeX token, as in mhchem: its
/// brace groups go with it and its name is never split into `\mathrm`
/// letters (`\text{heat}` used to become `\text \mathrm{heat}`, an error).
/// After it a number is an amount, not a count. The expected TeX is KaTeX's
/// mhchem output without its `{\vphantom{X}}` script bases.
#[test]
fn ce_copies_text_commands_whole() {
    for (input, expected) in [
        (
            r"\ce{A + \text{heat} -> B}",
            r"{\mathrm{A}+ \text{heat}\;\xrightarrow{}\;\mathrm{B}}",
        ),
        (r"\ce{\text{Fe}^{2+}}", r"{\text{Fe}^{2+}}"),
        (r"\ce{\text{a}2H}", r"{\text{a}2\mathrm{H}}"),
        (r"\ce{\textbf{A} B}", r"{\textbf{A}\;\mathrm{B}}"),
        (r"\ce{\mathbf{A}{B}C}", r"{\mathbf{A}{B}\mathrm{C}}"),
        (
            r"\ce{H2O \text{a}}",
            r"{\mathrm{H}_{2}\mathrm{O}\;\text{a}}",
        ),
        (r"\ce{\text{a}-B}", r"{\text{a}{-}\mathrm{B}}"),
        (r"\ce{\text{a}+}", r"{\text{a} {}+{} }"),
        (r"\ce{A\times2}", r"{\mathrm{A}\times2}"),
    ] {
        assert_reads_as(input, expected);
        assert!(!latex(input).contains(r"\mathrm{a}"), "{input}");
    }
}

/// `^`/`_` followed by a control sequence script the whole token (with its
/// groups), as mhchem's `^\x{}{}`/`_\x` read it; it used to be `^{\}`.
#[test]
fn ce_scripts_a_command() {
    for (input, expected) in [
        (r"\ce{A^\pm}", r"{\mathrm{A}^{\pm}}"),
        (r"\ce{A^\alpha}", r"{\mathrm{A}^{\alpha}}"),
        (r"\ce{A_\text{x}}", r"{\mathrm{A}_{\text{x}}}"),
        (r"\ce{A^\frac{1}{2}}", r"{\mathrm{A}^{\frac{1}{2}}}"),
        (r"\ce{k^\_}", r"{\mathrm{k}^\_}"),
        // mhchem's `^_`: a `_` before `^`, or a script at the end, is dropped.
        (r"\ce{C2_^\pm}", r"{\mathrm{C}_{2}^{\pm}}"),
        (r"\ce{10^_}", r"{10}"),
        (r"\ce{x10^_m}", r"{\mathrm{x}_{10}{\vphantom{X}}_{m}}"),
    ] {
        assert_reads_as(input, expected);
    }
}

/// `\frac{..}{..}` reads both arguments as `\ce` bodies (mhchem's
/// `frac-ce`); it used to be split into `\frac _{1}_{2}`. A `\frac` with no
/// groups is copied as it is, and a script on the token TeX takes as its
/// argument goes on an empty base, as mhchem's `{\vphantom{X}}`.
#[test]
fn ce_reads_frac_arguments() {
    for (input, expected) in [
        (
            r"\ce{\frac{1}{2}H2O}",
            r"{\frac{1}{2}\mathrm{H}_{2}\mathrm{O}}",
        ),
        (
            r"\ce{\frac{H2O}{2}}",
            r"{\frac{\mathrm{H}_{2}\mathrm{O}}{2}}",
        ),
        (r"\ce{\frac/1\,}", r"{\frac/1\,}"),
        (r"\ce{\frac)91}", r"{\frac){}_{91}}"),
    ] {
        assert_reads_as(input, expected);
    }
}

/// Other tokens as mhchem reads them: `{}` ends a species (`x1{}0` is
/// `x_1 0`, not a double subscript), Greek letters are element letters,
/// `\pm` is an operator (also as a prefix: `\pmE`), and a backslash that
/// starts no control sequence ends the species.
#[test]
fn ce_reads_other_tokens_as_mhchem() {
    for (input, expected) in [
        (r"\ce{x1{}0}", r"{\mathrm{x}_{1}0}"),
        (r"\ce{-x1{}0}", r"{{-}\mathrm{x}_{1}0}"),
        (r"\ce{\alpha2}", r"{\alpha_{2}}"),
        (r"\ce{\pm2}", r"{ {}\pm{} 2}"),
        (r"\ce{\pmE}", r"{ {}\pm{} \mathrm{E}}"),
        (r"\ce{A\,2}", r"{\mathrm{A}\,2}"),
        (r"\ce{µ1\+2}", r"{µ1 {}+{} 2}"),
    ] {
        assert_reads_as(input, expected);
    }
}

/// In `\pu`, a backslash that starts no control sequence is unit text, so
/// mhchem writes the number after it as `\^{…}`: a circumflex accent, which
/// KaTeX reads in math mode, as it does the other text accents.
#[test]
fn pu_backslash_before_a_digit_reads_as_katex() {
    for (input, katex) in [
        (r"\pu{\1,5}", r"{\mathrm{\^{1,5}}}"),
        (r"\pu{x\10^漢}", r"{\mathrm{x\^{10}^漢}}"),
        (r"\pu{és\~e-1.2C}", r"{\mathrm{és\~e^{-1.2}C}}"),
        (r"\pu{0 2\^°}", r"{0~\mathrm{2\^°}}"),
        (
            r"\pu{Ω1.2*o\frac~)}",
            r"{\mathrm{Ω^{1.2}}\mkern1mu{\cdot}\mkern1mu \mathrm{o\frac~)}}",
        ),
    ] {
        assert_reads_as(input, katex);
    }
    for (text, math) in [
        (r"\^{x}", r"\hat{x}"),
        (r"\~x", r"\tilde{x}"),
        (r"\'{e}", r"\acute{e}"),
        (r"\`{e}", r"\grave{e}"),
        (r#"\"{u}"#, r"\ddot{u}"),
        (r"\={a}", r"\bar{a}"),
        (r"\.{x}", r"\dot{x}"),
    ] {
        assert_reads_as(text, math);
    }
}
