use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{
    Config, Format, Strictness, convert, from_latex, from_mathml, from_omml, from_unicode_math,
};

/// Helper: parse LaTeX, write LaTeX, verify structural equivalence.
fn latex_roundtrip(input: &str) {
    let config = Config::default();
    let parsed = LatexReader::parse(input, &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    // Re-parse the output and compare ASTs
    let reparsed = LatexReader::parse(&output, &config).unwrap();
    assert_eq!(
        parsed.expr, reparsed.expr,
        "LaTeX round-trip failed.\n  Input:    {input}\n  Output:   {output}"
    );
}

/// Helper: parse MathML, write MathML, re-parse and compare ASTs.
fn mathml_roundtrip(input: &str) {
    let config = Config::default();
    let parsed = MathmlReader::parse(input, &config).unwrap();
    let output = MathmlWriter::write(&parsed.expr, &config).unwrap();
    let reparsed = MathmlReader::parse(&output, &config).unwrap();
    assert_eq!(
        parsed.expr, reparsed.expr,
        "MathML round-trip failed.\n  Input:  {input}\n  Output: {output}"
    );
}

// =========================================================================
// LaTeX round-trips for all 26 AST variants
// =========================================================================

#[test]
fn rt_number() {
    latex_roundtrip("42");
    latex_roundtrip("3.14");
}

#[test]
fn rt_identifier() {
    latex_roundtrip("x");
}

#[test]
fn rt_operator() {
    latex_roundtrip("\\sin");
    latex_roundtrip("\\lim");
}

#[test]
fn rt_symbol_binary() {
    latex_roundtrip("a + b");
}

#[test]
fn rt_symbol_relation() {
    latex_roundtrip("a = b");
}

#[test]
fn rt_text() {
    latex_roundtrip("\\text{hello world}");
}

#[test]
fn rt_space() {
    latex_roundtrip("\\quad");
}

#[test]
fn rt_row() {
    latex_roundtrip("{a + b}");
}

#[test]
fn rt_fenced() {
    latex_roundtrip("\\left( x \\right)");
    latex_roundtrip("\\left[ x \\right]");
}

#[test]
fn rt_subscript() {
    latex_roundtrip("x_i");
    latex_roundtrip("x_{12}");
}

#[test]
fn rt_superscript() {
    latex_roundtrip("x^2");
    latex_roundtrip("x^{n+1}");
}

#[test]
fn rt_subsup() {
    latex_roundtrip("x_i^2");
    latex_roundtrip("x^2_i");
}

#[test]
fn rt_prime_as_superscript() {
    // f' should parse as f^{\prime}, which round-trips as f^{\prime}
    let config = Config::default();
    let parsed = LatexReader::parse("f'", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f^{\\prime}");

    // f'' -> f^{\prime\prime}
    let parsed = LatexReader::parse("f''", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f^{\\prime\\prime}");

    // f''' -> f^{\prime\prime\prime}
    let parsed = LatexReader::parse("f'''", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f^{\\prime\\prime\\prime}");

    // f'_0 -> f_0^{\prime} (SubSup with sub=0, sup=prime)
    let parsed = LatexReader::parse("f'_0", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f_0^{\\prime}");

    // f'^2 -> f^{\prime 2} (prime merged with explicit superscript)
    let parsed = LatexReader::parse("f'^2", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f^{\\prime2}");

    // f'^{2} -> f^{\prime 2} (braced explicit superscript after prime)
    let parsed = LatexReader::parse("f'^{2}", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f^{\\prime2}");

    // f_0' -> f_0^{\prime} (SubSup with sub=0, sup=prime)
    let parsed = LatexReader::parse("f_0'", &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert_eq!(output, "f_0^{\\prime}");
}

#[test]
fn rt_fraction() {
    latex_roundtrip("\\frac{a}{b}");
    latex_roundtrip("\\frac{1}{2}");
    latex_roundtrip("\\dfrac{x}{y}");
}

#[test]
fn rt_sqrt() {
    latex_roundtrip("\\sqrt{x}");
    latex_roundtrip("\\sqrt{a + b}");
}

#[test]
fn rt_root() {
    latex_roundtrip("\\sqrt[3]{x}");
    latex_roundtrip("\\sqrt[n]{2}");
}

#[test]
fn rt_nary() {
    latex_roundtrip("\\sum_{i=0}^{n} a");
    latex_roundtrip("\\int_{0}^{1} f");
    latex_roundtrip("\\prod_{k=1}^{n} k");
}

#[test]
fn rt_accent() {
    latex_roundtrip("\\hat{x}");
    latex_roundtrip("\\bar{y}");
    latex_roundtrip("\\vec{v}");
    latex_roundtrip("\\dot{x}");
    latex_roundtrip("\\tilde{a}");
}

#[test]
fn rt_overline() {
    latex_roundtrip("\\overline{x}");
}

#[test]
fn rt_underline() {
    latex_roundtrip("\\underline{x}");
}

#[test]
fn rt_phantom() {
    latex_roundtrip("\\phantom{x}");
}

#[test]
fn rt_boxed() {
    latex_roundtrip("\\boxed{x}");
}

#[test]
fn rt_cancel() {
    latex_roundtrip("\\cancel{x}");
    latex_roundtrip("\\bcancel{x}");
    latex_roundtrip("\\xcancel{x}");
}

#[test]
fn rt_styled() {
    latex_roundtrip("\\mathbb{R}");
    latex_roundtrip("\\mathcal{F}");
    latex_roundtrip("\\mathbf{x}");
}

#[test]
fn rt_greek_letters() {
    latex_roundtrip("\\alpha");
    latex_roundtrip("\\beta");
    latex_roundtrip("\\Omega");
    latex_roundtrip("\\pi");
}

// =========================================================================
// MathML round-trips
// =========================================================================

#[test]
fn rt_mathml_number() {
    mathml_roundtrip("<math><mn>42</mn></math>");
}

#[test]
fn rt_mathml_identifier() {
    mathml_roundtrip("<math><mi>x</mi></math>");
}

#[test]
fn rt_mathml_fraction() {
    mathml_roundtrip("<math><mfrac><mn>1</mn><mn>2</mn></mfrac></math>");
}

#[test]
fn rt_mathml_sqrt() {
    mathml_roundtrip("<math><msqrt><mi>x</mi></msqrt></math>");
}

#[test]
fn rt_mathml_subscript() {
    mathml_roundtrip("<math><msub><mi>x</mi><mn>2</mn></msub></math>");
}

#[test]
fn rt_mathml_superscript() {
    mathml_roundtrip("<math><msup><mi>x</mi><mn>2</mn></msup></math>");
}

#[test]
fn rt_mathml_subsup() {
    mathml_roundtrip("<math><msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup></math>");
}

#[test]
fn rt_mathml_table() {
    mathml_roundtrip(
        "<math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr>\
         <mtr><mtd><mn>3</mn></mtd><mtd><mn>4</mn></mtd></mtr></mtable></math>",
    );
}

// =========================================================================
// End-to-end: quadratic formula
// =========================================================================

#[test]
fn quadratic_formula_latex_roundtrip() {
    let input = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
    let config = Config::default();
    let parsed = LatexReader::parse(input, &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    let reparsed = LatexReader::parse(&output, &config).unwrap();
    assert_eq!(parsed.expr, reparsed.expr);
}

#[test]
fn quadratic_formula_cross_format() {
    let latex = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<math"));
    assert!(mathml.contains("<mfrac>"));
    assert!(mathml.contains("<msqrt>"));
}

// =========================================================================
// Builder API
// =========================================================================

#[test]
fn builder_api_latex_to_mathml() {
    let result = from_latex("x^2 + 1").unwrap();
    let mathml = result.to_mathml().unwrap();
    assert!(mathml.contains("<math"));
    assert!(mathml.contains("<msup>"));
}

#[test]
fn builder_api_mathml_to_latex() {
    let result = from_mathml("<math><msup><mi>x</mi><mn>2</mn></msup></math>").unwrap();
    let latex = result.to_latex().unwrap();
    assert!(latex.contains("x^2") || latex.contains("x^{2}"));
}

// =========================================================================
// Convert API
// =========================================================================

#[test]
fn convert_latex_to_mathml() {
    let output = convert("\\frac{1}{2}", Format::LaTeX, Format::MathML).unwrap();
    assert!(output.contains("<mfrac>"));
}

#[test]
fn convert_mathml_to_latex() {
    let output = convert(
        "<math><mfrac><mn>1</mn><mn>2</mn></mfrac></math>",
        Format::MathML,
        Format::LaTeX,
    )
    .unwrap();
    assert!(output.contains("\\frac"));
}

#[test]
fn convert_all_formats_supported() {
    // All five formats are now supported
    let result = convert("x", Format::LaTeX, Format::Omml);
    assert!(result.is_ok());
    let result = convert("x", Format::LaTeX, Format::UnicodeMath);
    assert!(result.is_ok());
}

// =========================================================================
// Error handling
// =========================================================================

#[test]
fn latex_unmatched_brace() {
    let config = Config::default();
    let result = LatexReader::parse("\\frac{a}", &config);
    // Should fail because \frac expects two groups
    assert!(result.is_err());
}

#[test]
fn empty_input_is_ok() {
    let config = Config::default();
    let result = LatexReader::parse("", &config);
    assert!(result.is_ok());
    assert!(result.unwrap().expr.nodes.is_empty());
}

// =========================================================================
// Nested constructs
// =========================================================================

#[test]
fn nested_fractions() {
    latex_roundtrip("\\frac{\\frac{a}{b}}{\\frac{c}{d}}");
}

#[test]
fn fraction_with_scripts() {
    latex_roundtrip("\\frac{x^2}{y_1}");
}

#[test]
fn sum_of_fractions() {
    let input = "\\sum_{i=1}^{n} \\frac{1}{i}";
    let config = Config::default();
    let parsed = LatexReader::parse(input, &config).unwrap();
    let output = LatexWriter::write(&parsed.expr, &config).unwrap();
    // Verify the output contains both sum and fraction
    assert!(output.contains("\\sum"));
    assert!(output.contains("\\frac"));
}

#[test]
fn matrix_2x2() {
    let input = "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}";
    let config = Config::default();
    let parsed = LatexReader::parse(input, &config).unwrap();
    // Should produce a Fenced node containing an Array
    let mathml = MathmlWriter::write(&parsed.expr, &config).unwrap();
    assert!(mathml.contains("<mtable>"));
}

// =========================================================================
// Cross-format golden tests
// =========================================================================

#[test]
fn golden_simple_sum() {
    let latex = "a + b";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<mi>a</mi>"));
    assert!(mathml.contains("<mo>+</mo>"));
    assert!(mathml.contains("<mi>b</mi>"));
}

#[test]
fn golden_power() {
    let latex = "x^2";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<msup>"));
    assert!(mathml.contains("<mi>x</mi>"));
    assert!(mathml.contains("<mn>2</mn>"));
}

#[test]
fn golden_fraction() {
    let latex = "\\frac{a}{b}";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<mfrac>"));
    assert!(mathml.contains("<mi>a</mi>"));
    assert!(mathml.contains("<mi>b</mi>"));
}

#[test]
fn golden_sqrt() {
    let latex = "\\sqrt{x}";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<msqrt>"));
}

#[test]
fn golden_greek() {
    let latex = "\\alpha + \\beta";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("\u{03B1}"));
    assert!(mathml.contains("\u{03B2}"));
}

#[test]
fn golden_operator() {
    let latex = "\\sin x";
    let config = Config::default();
    let parsed = LatexReader::parse(latex, &config).unwrap();
    let mathml = MathmlWriter::write(&parsed.expr, &config).unwrap();
    assert!(mathml.contains("sin"));
}

#[test]
fn golden_text() {
    let latex = "\\text{if } x > 0";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<mtext>"));
}

#[test]
fn golden_subscript() {
    let latex = "x_i";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<msub>"));
}

#[test]
fn golden_hat_accent() {
    let latex = "\\hat{x}";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("<mover"));
}

#[test]
fn golden_integral() {
    let latex = "\\int_{0}^{1} f";
    let mathml = convert(latex, Format::LaTeX, Format::MathML).unwrap();
    assert!(mathml.contains("\u{222B}"));
}

// =========================================================================
// OMML cross-format tests
// =========================================================================

#[test]
fn latex_to_omml_fraction() {
    let omml = convert("\\frac{1}{2}", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:f>"));
    assert!(omml.contains("<m:num>"));
    assert!(omml.contains("<m:den>"));
}

#[test]
fn latex_to_omml_superscript() {
    let omml = convert("x^2", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:sSup>"));
}

#[test]
fn latex_to_omml_subscript() {
    let omml = convert("x_i", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:sSub>"));
}

#[test]
fn latex_to_omml_sqrt() {
    let omml = convert("\\sqrt{x}", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:rad>"));
}

#[test]
fn latex_to_omml_sum() {
    let omml = convert("\\sum_{i=0}^{n} a", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:nary>"));
    assert!(omml.contains("\u{2211}"));
}

#[test]
fn latex_to_omml_parentheses() {
    let omml = convert("\\left( x \\right)", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:d>"));
}

#[test]
fn latex_to_omml_accent() {
    let omml = convert("\\hat{x}", Format::LaTeX, Format::Omml).unwrap();
    assert!(omml.contains("<m:acc>"));
}

#[test]
fn latex_to_omml_matrix() {
    let config = Config::default();
    let parsed =
        LatexReader::parse("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", &config).unwrap();
    let omml = OmmlWriter::write(&parsed.expr, &config).unwrap();
    assert!(omml.contains("<m:m>"));
    assert!(omml.contains("<m:mr>"));
}

#[test]
fn omml_to_latex_fraction() {
    let omml = convert("\\frac{a}{b}", Format::LaTeX, Format::Omml).unwrap();
    let latex = convert(&omml, Format::Omml, Format::LaTeX).unwrap();
    assert!(latex.contains("\\frac"));
}

#[test]
fn omml_to_mathml_superscript() {
    let omml = convert("x^2", Format::LaTeX, Format::Omml).unwrap();
    let mathml = convert(&omml, Format::Omml, Format::MathML).unwrap();
    assert!(mathml.contains("<msup>"));
}

#[test]
fn builder_api_latex_to_omml() {
    let result = from_latex("x^2 + 1").unwrap();
    let omml = result.to_omml().unwrap();
    assert!(omml.contains("<m:oMath"));
    assert!(omml.contains("<m:sSup>"));
}

#[test]
fn builder_api_omml_roundtrip() {
    let omml = convert("\\frac{1}{2}", Format::LaTeX, Format::Omml).unwrap();
    let result = from_omml(&omml).unwrap();
    let latex = result.to_latex().unwrap();
    assert!(latex.contains("\\frac"));
}

// =========================================================================
// UnicodeMath cross-format tests
// =========================================================================

#[test]
fn latex_to_unicode_math_fraction() {
    let um = convert("\\frac{a}{b}", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains('/'));
}

#[test]
fn latex_to_unicode_math_superscript() {
    let um = convert("x^2", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains("x^2") || um.contains("x^(2)"));
}

#[test]
fn latex_to_unicode_math_subscript() {
    let um = convert("x_i", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains("x_i") || um.contains("x_(i)"));
}

#[test]
fn latex_to_unicode_math_sqrt() {
    let um = convert("\\sqrt{x}", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains('\u{221A}'));
}

#[test]
fn latex_to_unicode_math_sum() {
    let um = convert("\\sum_{i=0}^{n} a", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains('\u{2211}'));
}

#[test]
fn latex_to_unicode_math_matrix() {
    let config = Config::default();
    let parsed =
        LatexReader::parse("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", &config).unwrap();
    let um = UnicodeMathWriter::write(&parsed.expr, &config).unwrap();
    assert!(um.contains('\u{25A0}'));
    assert!(um.contains('&'));
    assert!(um.contains('@'));
}

#[test]
fn latex_to_unicode_math_text() {
    let um = convert("\\text{hello}", Format::LaTeX, Format::UnicodeMath).unwrap();
    assert!(um.contains("\"hello\""));
}

#[test]
fn unicode_math_to_latex_fraction() {
    let um = "a/b";
    let latex = convert(um, Format::UnicodeMath, Format::LaTeX).unwrap();
    assert!(latex.contains("\\frac"));
}

#[test]
fn unicode_math_to_mathml_superscript() {
    let um = "x^2";
    let mathml = convert(um, Format::UnicodeMath, Format::MathML).unwrap();
    assert!(mathml.contains("<msup>"));
}

#[test]
fn builder_api_latex_to_unicode_math() {
    let result = from_latex("x^2 + 1").unwrap();
    let um = result.to_unicode_math().unwrap();
    assert!(um.contains("x^2") || um.contains("x^(2)"));
}

#[test]
fn builder_api_unicode_math_roundtrip() {
    let result = from_unicode_math("x^2").unwrap();
    let latex = result.to_latex().unwrap();
    assert!(latex.contains("x^"));
}

// =========================================================================
// MTEF cross-format tests
// =========================================================================

#[test]
fn latex_to_mtef_and_back() {
    let mtef = convert("\\frac{1}{2}", Format::LaTeX, Format::Mtef).unwrap();
    // MTEF output is hex-encoded
    assert!(mtef.chars().all(|c| c.is_ascii_hexdigit()));
    // Convert back
    let latex = convert(&mtef, Format::Mtef, Format::LaTeX).unwrap();
    assert!(latex.contains("\\frac"));
}

#[test]
fn latex_to_mtef_sqrt() {
    let mtef = convert("\\sqrt{x}", Format::LaTeX, Format::Mtef).unwrap();
    let latex = convert(&mtef, Format::Mtef, Format::LaTeX).unwrap();
    assert!(latex.contains("\\sqrt"));
}

#[test]
fn latex_to_mtef_superscript() {
    let mtef = convert("x^2", Format::LaTeX, Format::Mtef).unwrap();
    let mathml = convert(&mtef, Format::Mtef, Format::MathML).unwrap();
    assert!(mathml.contains("<msup>"));
}

#[test]
fn builder_api_latex_to_mtef() {
    let result = from_latex("x^2").unwrap();
    let mtef = result.to_mtef().unwrap();
    assert!(!mtef.is_empty());
    assert!(mtef.chars().all(|c| c.is_ascii_hexdigit()));
}

// =========================================================================
// Five-format quadratic formula
// =========================================================================

#[test]
fn quadratic_formula_all_formats() {
    let latex = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
    let config = Config::default();
    let parsed = LatexReader::parse(latex, &config).unwrap();

    // LaTeX round-trip
    let latex_out = LatexWriter::write(&parsed.expr, &config).unwrap();
    assert!(latex_out.contains("\\frac"));

    // MathML
    let mathml = MathmlWriter::write(&parsed.expr, &config).unwrap();
    assert!(mathml.contains("<mfrac>"));
    assert!(mathml.contains("<msqrt>"));

    // OMML
    let omml = OmmlWriter::write(&parsed.expr, &config).unwrap();
    assert!(omml.contains("<m:f>"));
    assert!(omml.contains("<m:rad>"));

    // UnicodeMath
    let um = UnicodeMathWriter::write(&parsed.expr, &config).unwrap();
    assert!(um.contains('/'));
    assert!(um.contains('\u{221A}'));

    // MTEF (hex)
    let mtef = convert(latex, Format::LaTeX, Format::Mtef).unwrap();
    assert!(!mtef.is_empty());
}

// =========================================================================
// Convert API: all format pairs
// =========================================================================

#[test]
fn convert_latex_to_all() {
    let input = "x^2";
    for fmt in [
        Format::MathML,
        Format::Omml,
        Format::UnicodeMath,
        Format::Mtef,
    ] {
        let output = convert(input, Format::LaTeX, fmt).unwrap();
        assert!(!output.is_empty(), "LaTeX -> {fmt:?} produced empty output");
    }
}

#[test]
fn convert_mathml_to_all() {
    let input = "<math><msup><mi>x</mi><mn>2</mn></msup></math>";
    for fmt in [
        Format::LaTeX,
        Format::Omml,
        Format::UnicodeMath,
        Format::Mtef,
    ] {
        let output = convert(input, Format::MathML, fmt).unwrap();
        assert!(
            !output.is_empty(),
            "MathML -> {fmt:?} produced empty output"
        );
    }
}

// =========================================================================
// Lenient error recovery tests
// =========================================================================

fn lenient_config() -> Config {
    Config {
        strictness: Strictness::Lenient,
        ..Config::default()
    }
}

#[test]
fn lenient_latex_trailing_input() {
    let config = lenient_config();
    let result = LatexReader::parse("x^2 }", &config);
    assert!(
        result.is_ok(),
        "lenient mode should recover from trailing input"
    );
    let parsed = result.unwrap();
    assert!(!parsed.expr.nodes.is_empty());
    assert!(parsed.has_warnings());
}

#[test]
fn strict_latex_trailing_input_fails() {
    let config = Config::default();
    let result = LatexReader::parse("x^2 }", &config);
    assert!(result.is_err());
}

#[test]
fn lenient_latex_malformed_command() {
    let config = lenient_config();
    let result = LatexReader::parse("\\frac{a}", &config);
    assert!(
        result.is_ok(),
        "lenient mode should recover from malformed commands"
    );
    let parsed = result.unwrap();
    assert!(parsed.has_warnings());
}

#[test]
fn lenient_mathml_invalid_xml() {
    let config = lenient_config();
    let result = MathmlReader::parse("<math><mi>x</mi><broken", &config);
    assert!(
        result.is_ok(),
        "lenient MathML should recover from invalid XML"
    );
    let parsed = result.unwrap();
    assert!(parsed.has_warnings());
}

#[test]
fn strict_mathml_invalid_xml_fails() {
    let config = Config::default();
    let result = MathmlReader::parse("<math><mi>x</mi><broken", &config);
    assert!(result.is_err());
}

#[test]
fn lenient_mtef_invalid_hex() {
    use mathema::reader::mtef::MtefReader;
    let config = lenient_config();
    let result = MtefReader::parse("zzzz", &config);
    assert!(
        result.is_ok(),
        "lenient MTEF should recover from invalid hex"
    );
    let parsed = result.unwrap();
    assert!(parsed.has_warnings());
}

#[test]
fn strict_mtef_invalid_hex_fails() {
    use mathema::reader::mtef::MtefReader;
    let config = Config::default();
    let result = MtefReader::parse("zzzz", &config);
    assert!(result.is_err());
}

#[test]
fn lenient_preserves_valid_parse() {
    let strict = Config::default();
    let lenient = lenient_config();
    let input = "x^2 + 1";
    let strict_result = LatexReader::parse(input, &strict).unwrap();
    let lenient_result = LatexReader::parse(input, &lenient).unwrap();
    assert_eq!(strict_result.expr, lenient_result.expr);
    assert!(!lenient_result.has_warnings());
}
