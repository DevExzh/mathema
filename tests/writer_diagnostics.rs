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
fn omml_reports_color_loss() {
    let expr = compact(r"\color{red}{x}");
    let (_, diags) = OmmlWriter::write_compact_with_diagnostics(&expr, &Config::default()).unwrap();
    assert!(has_warning(&diags, "color"), "{diags:?}");
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
