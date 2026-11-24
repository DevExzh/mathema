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
