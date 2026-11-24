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

/// mhchem's `pu-2` unit grammar, compared with its KaTeX output.
#[test]
fn pu_units_follow_mhchem() {
    for (input, expected) in [
        // `_` is a TeX subscript, as mhchem copies it.
        (r"\pu{a_b}", r"\mathrm{a_{b}}"),
        (r"\pu{m_{2}}", r"\mathrm{m_{2}}"),
        // A number after unit text is its exponent.
        (r"\pu{m2}", r"\mathrm{m^{2}}"),
        (
            r"\pu{kg m2 s-2}",
            r"\mathrm{kg}\,\mathrm{m^{2}}\,\mathrm{s^{- 2}}",
        ),
        (r"\pu{x_a2}", r"\mathrm{x_{a}^{2}}"),
        // An escaped underscore is a glyph; the digits after it still rise.
        (r"\pu{m\_2}", r"\mathrm{m{\char`\_}^{2}}"),
        (r"\pu{25 °C}", r"25\,\mathrm{{}^{\circ}C}"),
    ] {
        assert_eq!(latex(input), latex(expected), "{input}");
        let result = from_latex_with(input, &lenient()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
    let math = mathml(r"\pu{kg.m}");
    assert!(math.contains('⋅') || math.contains('·'), "{math}");
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
