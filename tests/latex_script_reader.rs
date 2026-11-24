//! The LaTeX reader takes exactly one TeX token or one brace group as a
//! script argument, as TeX, KaTeX and MathJax do: `x^22` is `x^{2}2`.

use mathema::error::{Config, DiagnosticKind, Span, Strictness};
use mathema::{Format, convert, from_latex_with};

fn latex(input: &str) -> String {
    convert(input, Format::LaTeX, Format::LaTeX).unwrap()
}

fn mathml(input: &str) -> String {
    convert(input, Format::LaTeX, Format::MathML).unwrap()
}

fn lenient() -> Config {
    let mut config = Config::default();
    config.strictness = Strictness::Lenient;
    config
}

/// Each input reads exactly as its braced spelling does.
const SAME_AS_BRACED: &[(&str, &str)] = &[
    (r"x^22", r"x^{2}2"),
    (r"x^2.5", r"x^{2}.5"),
    (r"x_ab", r"x_{a}b"),
    (r"x_12", r"x_{1}2"),
    (r"x^\alpha\beta", r"x^{\alpha}\beta"),
    (r"x_1^22", r"x_{1}^{2}2"),
    (r"x'^22", r"x'^{2}2"),
    (r"x^ 22", r"x^{2}2"),
    (r"x^\relax22", r"x^{2}2"),
    (r"{}^22X", r"{}^{2}2X"),
    (r"\sum_10^20 k", r"\sum_{1}0^{2}0k"),
    (r"\int\limits_01 f", r"\int\limits_{0}1f"),
    (r"\lim_10 a", r"\lim_{1}0a"),
    (r"x^αβ", r"x^{α}β"),
    // A control sequence is one token together with the arguments it
    // reads itself, each again one token or one group (KaTeX:
    // `\frac` and the font commands are `allowedInArgument`).
    (r"x^\frac12", r"x^{\frac{1}{2}}"),
    (r"x^\frac123", r"x^{\frac{1}{2}}3"),
    (r"x^\mathrm{ab}", r"x^{\mathrm{ab}}"),
    (r"x^\mathbf xy", r"x^{\mathbf{x}}y"),
    (r"x^\text{ab}", r"x^{\text{ab}}"),
    // KaTeX rejects `\sqrt` without braces here; mathema reads it as
    // MathJax does.
    (r"x^\sqrt2", r"x^{\sqrt{2}}"),
    (r"x^\sqrt22", r"x^{\sqrt{2}}2"),
    (r"x^\hat a b", r"x^{\hat{a}}b"),
];

#[test]
fn a_script_argument_is_one_tex_token() {
    for &(input, braced) in SAME_AS_BRACED {
        assert_eq!(mathml(input), mathml(braced), "{input} vs {braced}");
        assert_eq!(latex(input), latex(braced), "{input} vs {braced}");
    }
}

#[test]
fn written_examples() {
    assert_eq!(latex(r"x^22"), r"x^{2}2");
    assert_eq!(latex(r"x^2.5"), r"x^{2}.5");
    assert_eq!(latex(r"x^\frac12"), r"x^{\frac{1}{2}}");
    assert_eq!(latex(r"x^{22}"), r"x^{22}");
    assert_eq!(latex(r"x^{2.5}"), r"x^{2.5}");
}

#[test]
fn mathml_of_a_split_number() {
    let math = mathml(r"x^22");
    assert!(
        math.contains("<msup><mi>x</mi><mn>2</mn></msup><mn>2</mn>"),
        "{math}"
    );
    let math = mathml(r"x^2.5");
    assert!(
        math.contains("<msup><mi>x</mi><mn>2</mn></msup><mo>.</mo><mn>5</mn>"),
        "{math}"
    );
    let math = mathml(r"x^{22}");
    assert!(
        math.contains("<msup><mi>x</mi><mn>22</mn></msup>"),
        "{math}"
    );
}

/// The braced spellings still take the whole number in every script
/// position.
#[test]
fn braced_numbers_stay_whole() {
    let math = mathml(r"\sideset{_{12}}{^{34}}\sum");
    assert!(
        math.contains("<mn>12</mn>") && math.contains("<mn>34</mn>"),
        "{math}"
    );
    let math = mathml(r"\sum\limits_{10}^{20} k");
    assert!(
        math.contains("<mn>10</mn>") && math.contains("<mn>20</mn>"),
        "{math}"
    );
    let math = mathml(r"x'^{22}");
    assert!(math.contains("<mn>22</mn>"), "{math}");
}

/// The mhchem and siunitx rewrites brace the scripts they generate.
#[test]
fn package_rewrites_keep_whole_numbers() {
    for (input, number) in [
        (r"\ce{C6H12O6}", "<mn>12</mn>"),
        (r"\ce{^{227}_{90}Th}", "<mn>227</mn>"),
        (r"\ce{SO4^2-}", "<mn>2</mn>"),
        (r"\SI{3e45}{m}", "<mn>45</mn>"),
        (r"\SI{3}{m^{22}}", "<mn>22</mn>"),
        (r"\textsuperscript{12}", "<mn>12</mn>"),
    ] {
        let math = mathml(input);
        assert!(math.contains(number), "{input}: {math}");
    }
    let diagnostics = from_latex_with(r"\ce{C6H12O6} \SI{3e45}{m^2}", &lenient())
        .unwrap()
        .diagnostics()
        .to_vec();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn lenient_reports_a_split_number() {
    let cases: &[(&str, &str, Span)] = &[
        (
            r"x^22",
            "only the first digit `2` is the superscript; write `^{22}` to raise the whole number",
            Span::new(2, 4),
        ),
        (
            r"x_2.5",
            "only the first digit `2` is the subscript; write `_{2.5}` to lower the whole number",
            Span::new(2, 5),
        ),
        (
            r"a+x^10.25",
            "only the first digit `1` is the superscript; write `^{10.25}` to raise the whole number",
            Span::new(4, 9),
        ),
        (
            r"x'^22",
            "only the first digit `2` is the superscript; write `^{22}` to raise the whole number",
            Span::new(3, 5),
        ),
    ];
    for &(input, message, span) in cases {
        let result = from_latex_with(input, &lenient()).unwrap();
        let diagnostics = result.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{input}: {diagnostics:?}");
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Warning);
        assert_eq!(diagnostics[0].message, message, "{input}");
        assert_eq!(diagnostics[0].span, Some(span), "{input}");
    }
    let result = from_latex_with(r"\sum_10^20 k", &lenient()).unwrap();
    assert_eq!(result.diagnostics().len(), 2, "{:?}", result.diagnostics());
}

#[test]
fn no_report_for_one_digit_or_a_braced_number() {
    for input in [
        r"x^2", r"x^2.", r"x^2 2", r"x^{22}", r"x^{2.5}", r"x^2+1", r"x^2,3", r"x^a2",
    ] {
        let result = from_latex_with(input, &lenient()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
}

/// Strict mode accepts the TeX reading without a diagnostic.
#[test]
fn strict_accepts_a_split_number() {
    for input in [r"x^22", r"x_2.5", r"\sum_10^20 k"] {
        let result = from_latex_with(input, &Config::default()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn split_numbers_round_trip() {
    for &(input, _) in SAME_AS_BRACED {
        let first = latex(input);
        assert_eq!(latex(&first), first, "{input}");
        assert_eq!(mathml(&first), mathml(input), "{input}");
    }
}

#[cfg(feature = "render")]
#[test]
fn split_numbers_render_as_braced() {
    for (input, braced) in [
        (r"x^22", r"x^{2}2"),
        (r"x^2.5", r"x^{2}.5"),
        (r"x^\frac12", r"x^{\frac{1}{2}}"),
    ] {
        let svg = convert(input, Format::LaTeX, Format::Svg).unwrap();
        assert_eq!(
            svg,
            convert(braced, Format::LaTeX, Format::Svg).unwrap(),
            "{input}"
        );
        assert_ne!(
            svg,
            convert(r"x^{22}", Format::LaTeX, Format::Svg).unwrap(),
            "{input}"
        );
    }
}

/// Each occurrence is reported once, also when no span is known.
#[test]
fn every_occurrence_is_reported() {
    let result = from_latex_with(r"x^22 + y_33", &lenient()).unwrap();
    let spans: Vec<_> = result.diagnostics().iter().map(|d| d.span).collect();
    assert_eq!(spans, [Some(Span::new(2, 4)), Some(Span::new(9, 11))]);

    // The macro definition makes the parsed text a rewrite of the source.
    let result = from_latex_with(r"\def\t{1}x^22 + y^22", &lenient()).unwrap();
    let diagnostics = result.diagnostics();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.span, None);
        assert_eq!(
            diagnostic.message,
            "after macro expansion, only the first digit `2` of `22` is the superscript; \
             brace the number to raise all of it"
        );
    }
}

/// A number completed by a macro is described without claiming the
/// caller wrote `^22`.
#[test]
fn a_number_completed_by_a_macro_has_no_span() {
    let result = from_latex_with(r"\newcommand\sq{^2} x\sq 2", &lenient()).unwrap();
    let diagnostics = result.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].span, None);
    assert!(
        diagnostics[0].message.starts_with("after macro expansion"),
        "{diagnostics:?}"
    );
}

#[test]
fn a_span_counts_bytes() {
    let result = from_latex_with("é^22", &lenient()).unwrap();
    let diagnostics = result.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].span, Some(Span::new(3, 5)));
}

/// Reports are capped like undefined commands and found in linear time
/// (each used to scan every earlier diagnostic: 100k took seconds).
#[test]
fn many_split_numbers_are_capped_and_fast() {
    // Rows hold at most 65535 children, so group the occurrences.
    let input = format!("{{{}}}", "x^22+".repeat(10_000)).repeat(10);
    let start = std::time::Instant::now();
    let result = from_latex_with(&input, &lenient()).unwrap();
    let elapsed = start.elapsed();
    let diagnostics = result.diagnostics();
    assert_eq!(diagnostics.len(), 101);
    assert_eq!(
        diagnostics[100].message,
        "99900 more script arguments that split a number not listed"
    );
    assert!(elapsed < std::time::Duration::from_secs(10), "{elapsed:?}");
}

/// `\sideset` takes the scripts at the start of each group; the rest of the
/// left group stands between the prescripts and the operator, and the rest
/// of the right group follows it, as amsmath sets `{}#1`. Normalized, the
/// left scripts lead that material, as `{}_{1}2` does anywhere.
#[test]
fn sideset_keeps_material_after_its_scripts() {
    let cases = [
        (
            r"\sideset{_12}{}\sum",
            "<mmultiscripts><mn>2</mn><mprescripts/><mn>1</mn><none/></mmultiscripts><mo largeop=\"true\" form=\"prefix\">∑</mo>",
        ),
        (
            r"\sideset{}{_12}\sum",
            "<msub><mo largeop=\"true\" form=\"prefix\">∑</mo><mn>1</mn></msub><mn>2</mn>",
        ),
        (
            r"\sideset{_1}{^23}\sum",
            "<msup><mmultiscripts><mo largeop=\"true\" form=\"prefix\">∑</mo><mprescripts/><mn>1</mn><none/></mmultiscripts><mn>2</mn></msup><mn>3</mn>",
        ),
        (
            r"\sideset{_a b}{}\sum",
            "<mmultiscripts><mi>b</mi><mprescripts/><mi>a</mi><none/></mmultiscripts><mo largeop=\"true\" form=\"prefix\">∑</mo>",
        ),
        (
            r"\sideset{a}{b}\prod",
            "<mi>a</mi><mo largeop=\"true\" form=\"prefix\">∏</mo><mi>b</mi>",
        ),
    ];
    for (input, expected) in cases {
        let math = mathml(input);
        assert!(math.contains(expected), "{input}: {math}");
        let first = latex(input);
        assert_eq!(latex(&first), first, "{input}");
        assert_eq!(
            bare_ops(&mathml(&first)),
            bare_ops(&math),
            "{input}: {first}"
        );
    }
    // The writer spells stacked limits as nested `\underset`/`\overset`,
    // which read back nested, so only the LaTeX is compared here.
    let first = latex(r"\sideset{_1 2}{^3 4}\sum\limits_{k}^{n} a_k");
    assert_eq!(latex(&first), first);
    for input in [
        r"\sideset{_1^2 x}{_3^4 y}\int f",
        r"\sideset{_{12}}{^{34}}\sum",
    ] {
        let first = latex(input);
        assert_eq!(latex(&first), first, "{input}");
        assert_eq!(
            bare_ops(&mathml(&first)),
            bare_ops(&mathml(input)),
            "{input}: {first}"
        );
    }
}

/// The written `\sideset` operator reads back as a plain operator, whose
/// `<mo>` has no attributes (as for `\sideset{}{}\sum`).
fn bare_ops(math: &str) -> String {
    let mut out = String::with_capacity(math.len());
    let mut rest = math;
    while let Some(at) = rest.find("<mo ") {
        out.push_str(&rest[..at]);
        out.push_str("<mo>");
        rest = &rest[at..];
        rest = &rest[rest.find('>').map_or(rest.len(), |end| end + 1)..];
    }
    out.push_str(rest);
    out
}

/// `\pu` raises a whole `^(-?\d+)` exponent, as mhchem does.
#[test]
fn pu_exponents_are_braced() {
    // The second column is what KaTeX's mhchem writes.
    for (input, expected) in [
        (r"\pu{123 m^22}", r"{123~\mathrm{m^{22}}}"),
        (
            r"\pu{kJ mol^-1}",
            r"{\mathrm{kJ}\mkern3mu \mathrm{mol^{-1}}}",
        ),
        (r"\pu{m^x}", r"{\mathrm{m^x}}"),
    ] {
        assert_eq!(latex(input), latex(expected), "{input}");
        let result = from_latex_with(input, &lenient()).unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{input}: {:?}",
            result.diagnostics()
        );
    }
    let math = mathml(r"\pu{123 m^22}");
    assert!(math.contains("<mn>22</mn>"), "{math}");
    let math = mathml(r"\pu{kJ mol^-1}");
    assert!(
        math.contains("<mo>−</mo><mn>1</mn></mrow></msup>"),
        "{math}"
    );
}
