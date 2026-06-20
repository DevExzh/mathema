//! Verifies that every text writer reports lossy conversions via
//! `write_compact_with_diagnostics` (feature-matrix item FM1/FM2/FM8).

use mathema::error::{Config, DiagnosticKind};
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::mtef::MtefWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;

fn compact(tex: &str) -> mathema::CompactExpr {
    LatexReader::parse_compact(tex, &Config::default())
        .unwrap()
        .expr
}

fn has_warning(diags: &[mathema::error::Diagnostic], needle: &str) -> bool {
    diags
        .iter()
        .any(|d| d.kind == DiagnosticKind::Warning && d.message.to_lowercase().contains(needle))
}

#[test]
fn mathml_reports_mathchoice_loss() {
    let expr = compact(r"\mathchoice{D}{T}{S}{SS}");
    let (_, diags) =
        MathmlWriter::write_compact_with_diagnostics(&expr, &Config::default()).unwrap();
    assert!(has_warning(&diags, "mathchoice"), "{diags:?}");
}

#[test]
fn omml_preserves_color_without_loss() {
    // OMML can express run color, so `\color` is preserved (no loss diagnostic)
    // and the resolved hex appears in the output.
    let expr = compact(r"\color{red}{x}");
    let (out, diags) =
        OmmlWriter::write_compact_with_diagnostics(&expr, &Config::default()).unwrap();
    assert!(
        !has_warning(&diags, "color"),
        "OMML preserves color, so no color-loss warning expected: {diags:?}"
    );
    assert!(
        out.to_uppercase().contains("FF0000"),
        "expected the resolved red color in OMML output: {out}"
    );
}

#[test]
fn unicode_math_reports_color_loss() {
    // UnicodeMath has no portable color syntax, so color is dropped (with a
    // loss diagnostic) while the content is preserved.
    let expr = compact(r"\color{red}{x}");
    let (out, diags) =
        UnicodeMathWriter::write_compact_with_diagnostics(&expr, &Config::default()).unwrap();
    assert!(has_warning(&diags, "color"), "{diags:?}");
    assert!(out.contains('x'), "content must be preserved: {out}");
}

#[test]
fn unicode_math_reports_box_and_cancel_loss() {
    let (_, d1) = UnicodeMathWriter::write_compact_with_diagnostics(
        &compact(r"\boxed{x}"),
        &Config::default(),
    )
    .unwrap();
    assert!(has_warning(&d1, "box"), "{d1:?}");
    let (_, d2) = UnicodeMathWriter::write_compact_with_diagnostics(
        &compact(r"\cancel{y}"),
        &Config::default(),
    )
    .unwrap();
    assert!(has_warning(&d2, "cancel"), "{d2:?}");
}

#[test]
fn mtef_reports_phantom_loss() {
    let expr = compact(r"\phantom{x}");
    let (_, diags) = MtefWriter::write_compact_with_diagnostics(&expr, &Config::default()).unwrap();
    assert!(has_warning(&diags, "phantom"), "{diags:?}");
}

#[test]
fn lossless_input_has_no_writer_diagnostics() {
    let expr = compact("x + y = z");
    for (name, diags) in [
        (
            "mathml",
            MathmlWriter::write_compact_with_diagnostics(&expr, &Config::default())
                .unwrap()
                .1,
        ),
        (
            "omml",
            OmmlWriter::write_compact_with_diagnostics(&expr, &Config::default())
                .unwrap()
                .1,
        ),
        (
            "unicode",
            UnicodeMathWriter::write_compact_with_diagnostics(&expr, &Config::default())
                .unwrap()
                .1,
        ),
        (
            "mtef",
            MtefWriter::write_compact_with_diagnostics(&expr, &Config::default())
                .unwrap()
                .1,
        ),
    ] {
        assert!(
            diags.is_empty(),
            "{name} emitted unexpected diagnostics: {diags:?}"
        );
    }
}

/// Writer-loss diagnostics must be reachable through the mainstream public
/// builder API (`from_latex().to_*_with_diagnostics()`), not only the
/// `#[doc(hidden)]` writer trait.
#[test]
fn convert_result_surfaces_writer_diagnostics() {
    // A lossy conversion (UnicodeMath cannot express color) must surface its
    // diagnostic through the mainstream builder API.
    let r = mathema::from_latex(r"\color{red}{x}").unwrap();
    let (_, diags) = r.to_unicode_math_with_diagnostics().unwrap();
    assert!(has_warning(&diags, "color"), "unicode color: {diags:?}");

    let r = mathema::from_latex(r"\phantom{x}").unwrap();
    let (_, diags) = r.to_mtef_with_diagnostics().unwrap();
    assert!(has_warning(&diags, "phantom"), "mtef: {diags:?}");

    let r = mathema::from_latex(r"\boxed{x}").unwrap();
    let (_, diags) = r.to_unicode_math_with_diagnostics().unwrap();
    assert!(has_warning(&diags, "box"), "unicode: {diags:?}");

    // OMML preserves color, so it surfaces no color diagnostic.
    let r = mathema::from_latex(r"\color{red}{x}").unwrap();
    assert!(!has_warning(
        &r.to_omml_with_diagnostics().unwrap().1,
        "color"
    ));

    // Lossless input → no writer diagnostics via the builder either.
    let r = mathema::from_latex("x + y = z").unwrap();
    assert!(r.to_mathml_with_diagnostics().unwrap().1.is_empty());
}
