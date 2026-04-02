use mathema::{Config, Format, convert, from_latex, from_mathml};
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::reader::mathml::MathmlReader;
use mathema::writer::mathml::MathmlWriter;

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
